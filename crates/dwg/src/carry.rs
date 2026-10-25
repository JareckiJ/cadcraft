//! Native passthrough of other programs' objects in a same-version DWG save (FF1).
//!
//! acadrust reads the objects of classes it doesn't know (Architectural Desktop's display
//! configuration, for one) as opaque records with their exact DWG bytes, and its writer emits
//! those bytes again. CADCraft's editable drawing goes through DXF, which can't hold them, and the
//! drawing CADCraft writes back has new handles for its tables and dictionaries. So the records
//! are carried beside the drawing instead ([`NativeCarry`], taken when the DWG is read) and put
//! back into the document the DWG writer gets ([`apply`]):
//!
//! - A carried unit is a whole subtree of the named object dictionary (an entry such as
//!   `AEC_DISP_REPS`): dictionaries, opaque records of the file's version, and the dictionaries
//!   and XRECORDs those records own. It keeps its handles, so the references inside it stay valid;
//!   the drawing CADCraft writes never uses them (its new handles start above `$HANDSEED`, and
//!   entities keep the file's own). References into another carried subtree stay valid too, as
//!   long as that subtree goes back as well.
//! - References out of the carried subtrees, in a record's handle stream or as the application of
//!   its extended data, are moved to what has the same role in the written document: a layer,
//!   linetype, text or dimension style, application or block by name, the named object
//!   dictionary, another dictionary by its key path, an entity by its handle. A subtree with a
//!   reference whose role isn't known, or records of another layout than R2010+ (AC1024 and
//!   later), is not carried.
//! - The DWG classes section is positional (a record's type code is 500 + the class's index), so
//!   the written document gets the file's class list in the file's order, with any class only it
//!   needs after them.
//!
//! What can't be carried is reported by the caller (the I/O layer's loss report, which keeps every
//! record's bytes), never dropped silently.

use std::collections::{HashMap, HashSet};

use acadrust::classes::{DxfClass, DxfClassCollection};
use acadrust::objects::{ObjectType, XRecordValue};
use acadrust::{CadDocument, DxfVersion, Handle};

use super::BridgeClass;

/// Most records carried from one file.
const MAX_CARRIED: usize = 200_000;
/// Deepest nesting followed in a carried subtree.
const MAX_DEPTH: usize = 32;
/// Most extended-data blocks read from one record.
const MAX_EED_BLOCKS: usize = 4_096;

/// What a reference out of a carried subtree points at, found again by role in another document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CarryRole {
    Layer(String),
    LineType(String),
    TextStyle(String),
    DimStyle(String),
    AppId(String),
    BlockRecord(String),
    /// The named object dictionary itself.
    NamedObjects,
    /// A dictionary by its key path from the named object dictionary.
    Dictionary(Vec<String>),
    /// An entity, which keeps its handle in CADCraft's drawing.
    Entity(u64),
    /// An object of another carried subtree (the named object dictionary entry), which keeps its
    /// handle when that subtree goes back too.
    Carried(String),
}

/// One opaque record, with what a loss report needs to restore it later.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CarriedRecord {
    pub handle: u64,
    pub owner: u64,
    /// A graphical entity (kept for the loss report only: entities aren't carried back yet).
    pub entity: bool,
    pub type_name: String,
    /// Keys from the named object dictionary down to the record's own entry.
    pub path: Vec<String>,
    pub class: Option<BridgeClass>,
    raw: Vec<u8>,
    handle_bits: i64,
}

impl CarriedRecord {
    /// The record's DWG bytes as read (the merged stream between the size prefix and the CRC).
    pub fn raw(&self) -> &[u8] {
        &self.raw
    }

    /// Bits of the raw bytes that are its handle stream (R2010+ framing).
    pub fn handle_bits(&self) -> i64 {
        self.handle_bits
    }
}

#[derive(Clone, Debug)]
struct Root {
    key: String,
    /// The entry's object: a dictionary, or a record held by the named object dictionary itself.
    top: Handle,
    hard_owned: bool,
    /// Dictionaries and XRECORDs of the subtree, written as they are.
    objects: Vec<ObjectType>,
    /// Indices into [`NativeCarry::records`].
    records: Vec<usize>,
    /// Handles out of the subtree its records refer to, with their roles.
    outside: Vec<(u64, CarryRole)>,
}

impl Root {
    /// The other entries this one refers into.
    fn needs(&self) -> impl Iterator<Item = &str> {
        self.outside.iter().filter_map(|(_, r)| if let CarryRole::Carried(k) = r { Some(k.as_str()) } else { None })
    }
}

/// Opaque records of a DWG kept for a save in the same version.
#[derive(Clone, Debug)]
pub struct NativeCarry {
    version: DxfVersion,
    version_code: String,
    classes: Vec<DxfClass>,
    roots: Vec<Root>,
    /// Every opaque record of the file with its DWG bytes, carried or not (a loss report keeps the
    /// bytes of those that can't go back).
    records: Vec<CarriedRecord>,
    /// Why a record isn't in a carried subtree, by handle.
    skipped: HashMap<u64, String>,
}

impl PartialEq for NativeCarry {
    /// Same version and the same records (bytes included).
    fn eq(&self, other: &Self) -> bool {
        self.version_code == other.version_code && self.records == other.records
    }
}

impl Eq for NativeCarry {}

impl NativeCarry {
    /// The DWG version code the records were read from (`AC1032`); only a save in it keeps them.
    pub fn version(&self) -> &str {
        &self.version_code
    }

    /// Every opaque record of the file with its DWG bytes, carried or not.
    pub fn records(&self) -> &[CarriedRecord] {
        &self.records
    }

    /// Whether nothing can go back into a same-version DWG.
    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }

    /// The handles of the records a same-version DWG save puts back.
    pub fn handles(&self) -> HashSet<u64> {
        self.roots.iter().flat_map(|r| r.records.iter().filter_map(|&i| self.records.get(i)).map(|r| r.handle)).collect()
    }

    /// Why `handle`'s record can't go back into a DWG, when it can't.
    pub fn skipped(&self, handle: u64) -> Option<&str> {
        self.skipped.get(&handle).map(String::as_str)
    }
}

// ---------------------------------------------------------------------------------------------
// Record bits: R2010+ layout. Data stream: type code (BB + RC/RS), own handle, extended data
// blocks (BS size, application handle, bytes) …; then the handle stream, the record's last
// `handle_bits` bits: a sequence of references (code nibble, length nibble, that many bytes).

/// A handle reference in a record: where it is (bits) and what it encodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct HandleRef {
    start: usize,
    len: usize,
    code: u8,
    value: u64,
}

/// A record's handle references: its extended data's applications, then its handle stream.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Layout {
    eed_apps: Vec<HandleRef>,
    handle_start: usize,
    refs: Vec<HandleRef>,
}

/// Bit `i` of `raw`, most significant first (0 past the end).
fn bit(raw: &[u8], i: usize) -> u8 {
    raw.get(i / 8).map_or(0, |b| (b >> (7 - (i % 8))) & 1)
}

/// The next `n` bits (at most 64) at `pos`, not past bit `end`; advances `pos`.
fn read_bits(raw: &[u8], pos: &mut usize, n: usize, end: usize) -> Option<u64> {
    if n > 64 || pos.checked_add(n)? > end {
        return None;
    }
    let mut v = 0u64;
    for _ in 0..n {
        v = (v << 1) | u64::from(bit(raw, *pos));
        *pos += 1;
    }
    Some(v)
}

/// A handle reference at `pos`: code nibble, length nibble, that many bytes.
fn read_ref(raw: &[u8], pos: &mut usize, end: usize) -> Option<HandleRef> {
    let start = *pos;
    let code = read_bits(raw, pos, 4, end)?;
    let count = read_bits(raw, pos, 4, end)?;
    let value = read_bits(raw, pos, usize::try_from(count).ok()?.checked_mul(8)?, end)?;
    Some(HandleRef { start, len: *pos - start, code: u8::try_from(code).ok()?, value })
}

/// The handle stream's references (bits too few for another one at the end are padding).
fn handle_refs(raw: &[u8], handle_bits: i64) -> Option<(usize, Vec<HandleRef>)> {
    let total = raw.len().checked_mul(8)?;
    let hb = usize::try_from(handle_bits).ok().filter(|b| *b > 0 && *b <= total)?;
    let start = total - hb;
    let mut pos = start;
    let mut refs = Vec::new();
    while total - pos >= 8 {
        refs.push(read_ref(raw, &mut pos, total)?);
    }
    Some((start, refs))
}

/// The record's layout: its extended data's application handles (checking the own handle is
/// `own`) and its handle stream. `None` when it isn't a record this understands.
fn layout(raw: &[u8], handle_bits: i64, own: u64) -> Option<Layout> {
    let (handle_start, refs) = handle_refs(raw, handle_bits)?;
    let end = handle_start;
    let mut pos = 0usize;
    let type_bits = if read_bits(raw, &mut pos, 2, end)? < 2 { 8 } else { 16 };
    read_bits(raw, &mut pos, type_bits, end)?;
    if read_ref(raw, &mut pos, end)?.value != own {
        return None;
    }
    let mut eed_apps = Vec::new();
    for _ in 0..MAX_EED_BLOCKS {
        // BS: 00 a little-endian short, 01 a byte, 10 zero, 11 256.
        let size = match read_bits(raw, &mut pos, 2, end)? {
            0 => {
                let lo = read_bits(raw, &mut pos, 8, end)?;
                let hi = read_bits(raw, &mut pos, 8, end)?;
                lo | (hi << 8)
            }
            1 => read_bits(raw, &mut pos, 8, end)?,
            2 => 0,
            _ => 256,
        };
        if size == 0 {
            return Some(Layout { eed_apps, handle_start, refs });
        }
        eed_apps.push(read_ref(raw, &mut pos, end)?);
        pos = pos.checked_add(usize::try_from(size).ok()?.checked_mul(8)?).filter(|p| *p <= end)?;
    }
    None
}

/// Where a reference points: absolute codes 2–5 (and 0/1, owner forms) carry the handle; 6, 8, A
/// and C are relative to the record's own handle (+1, −1, +offset, −offset).
fn target(r: &HandleRef, own: u64) -> Option<u64> {
    match r.code {
        0..=5 => Some(r.value),
        6 => own.checked_add(1),
        8 => own.checked_sub(1),
        0xA => own.checked_add(r.value),
        0xC => own.checked_sub(r.value),
        _ => None,
    }
}

/// Append the low `n` bits of `value`, most significant first, counting them in `nbits`.
fn push_bits(out: &mut Vec<u8>, nbits: &mut usize, value: u64, n: usize) {
    for i in (0..n).rev() {
        if nbits.is_multiple_of(8) {
            out.push(0);
        }
        if (value >> i) & 1 == 1
            && let Some(last) = out.last_mut()
        {
            *last |= 1 << (7 - (*nbits % 8));
        }
        *nbits += 1;
    }
}

/// The record with the references at `moved` (bit start → new absolute handle) re-encoded and
/// every other bit copied as it was, in order: a longer or shorter reference in the data stream
/// shifts what follows it, and the handle stream starts that much later or earlier. The record is
/// padded to a byte. Returns the bytes and the new handle-stream bit count.
fn rewrite(raw: &[u8], l: &Layout, moved: &HashMap<usize, u64>) -> Option<(Vec<u8>, i64)> {
    let total = raw.len().checked_mul(8)?;
    let mut out = Vec::with_capacity(raw.len() + 16);
    let mut n = 0usize;
    let mut pos = 0usize;
    let mut new_handle_start = None;
    for r in l.eed_apps.iter().chain(&l.refs) {
        if new_handle_start.is_none() && r.start >= l.handle_start {
            for i in pos..l.handle_start {
                push_bits(&mut out, &mut n, u64::from(bit(raw, i)), 1);
            }
            pos = l.handle_start;
            new_handle_start = Some(n);
        }
        for i in pos..r.start {
            push_bits(&mut out, &mut n, u64::from(bit(raw, i)), 1);
        }
        match moved.get(&r.start) {
            Some(&h) => {
                let bytes = (64 - h.leading_zeros()).div_ceil(8) as usize;
                push_bits(&mut out, &mut n, u64::from(r.code), 4);
                push_bits(&mut out, &mut n, bytes as u64, 4);
                push_bits(&mut out, &mut n, h, bytes * 8);
            }
            None => {
                for i in r.start..r.start + r.len {
                    push_bits(&mut out, &mut n, u64::from(bit(raw, i)), 1);
                }
            }
        }
        pos = r.start + r.len;
    }
    if new_handle_start.is_none() {
        for i in pos..l.handle_start {
            push_bits(&mut out, &mut n, u64::from(bit(raw, i)), 1);
        }
        pos = pos.max(l.handle_start);
        new_handle_start = Some(n);
    }
    for i in pos..total {
        push_bits(&mut out, &mut n, u64::from(bit(raw, i)), 1);
    }
    let handle_bits = i64::try_from(out.len().checked_mul(8)?.checked_sub(new_handle_start?)?).ok()?;
    Some((out, handle_bits))
}

// ---------------------------------------------------------------------------------------------
// Roles.

/// Every dictionary's entries by handle → (dictionary, key), for key paths and roles.
fn entry_index(doc: &CadDocument) -> HashMap<Handle, (Handle, String)> {
    let mut index = HashMap::new();
    for (h, o) in &doc.objects {
        if let ObjectType::Dictionary(d) = o {
            for (k, e) in &d.entries {
                index.entry(*e).or_insert((*h, k.clone()));
            }
        }
    }
    index
}

/// The keys from the named object dictionary down to `h` (None when it isn't reachable).
fn key_path(index: &HashMap<Handle, (Handle, String)>, nod: Handle, h: Handle) -> Option<Vec<String>> {
    let mut keys = Vec::new();
    let mut cur = h;
    for _ in 0..MAX_DEPTH {
        if cur == nod {
            keys.reverse();
            return Some(keys);
        }
        let (parent, key) = index.get(&cur)?;
        keys.push(key.clone());
        cur = *parent;
    }
    None
}

/// What handle `h` is to the document (layer, linetype, style, application, block, dictionary path, entity, the named object dictionary), so a carried reference can be re-encoded to what has the same role in the written file.
fn role(doc: &CadDocument, index: &HashMap<Handle, (Handle, String)>, h: Handle) -> Option<CarryRole> {
    let nod = doc.header.named_objects_dict_handle;
    if h == nod {
        return Some(CarryRole::NamedObjects);
    }
    if let Some(x) = doc.layers.iter().find(|x| x.handle == h) {
        return Some(CarryRole::Layer(x.name.clone()));
    }
    if let Some(x) = doc.line_types.iter().find(|x| x.handle == h) {
        return Some(CarryRole::LineType(x.name.clone()));
    }
    if let Some(x) = doc.text_styles.iter().find(|x| x.handle == h) {
        return Some(CarryRole::TextStyle(x.name.clone()));
    }
    if let Some(x) = doc.dim_styles.iter().find(|x| x.handle == h) {
        return Some(CarryRole::DimStyle(x.name.clone()));
    }
    if let Some(x) = doc.app_ids.iter().find(|x| x.handle == h) {
        return Some(CarryRole::AppId(x.name.clone()));
    }
    if let Some(x) = doc.block_records.iter().find(|x| x.handle == h) {
        return Some(CarryRole::BlockRecord(x.name.clone()));
    }
    if let Some(ObjectType::Dictionary(_)) = doc.objects.get(&h) {
        return key_path(index, nod, h).map(CarryRole::Dictionary);
    }
    if doc.get_entity(h).is_some() {
        return Some(CarryRole::Entity(h.value()));
    }
    None
}

/// The handle with `role` in `doc` (None when nothing there has it). A [`CarryRole::Carried`]
/// object keeps its handle: the caller checks its subtree goes back too.
fn resolve(doc: &CadDocument, role: &CarryRole, old: u64) -> Option<Handle> {
    match role {
        CarryRole::Layer(n) => doc.layers.get(n).map(|x| x.handle),
        CarryRole::LineType(n) => doc.line_types.get(n).map(|x| x.handle),
        CarryRole::TextStyle(n) => doc.text_styles.get(n).map(|x| x.handle),
        CarryRole::DimStyle(n) => doc.dim_styles.get(n).map(|x| x.handle),
        CarryRole::AppId(n) => doc.app_ids.get(n).map(|x| x.handle),
        CarryRole::BlockRecord(n) => doc.block_records.get(n).map(|x| x.handle),
        CarryRole::NamedObjects => Some(doc.header.named_objects_dict_handle).filter(|h| !h.is_null()),
        CarryRole::Dictionary(keys) => {
            let mut cur = doc.header.named_objects_dict_handle;
            for k in keys {
                let Some(ObjectType::Dictionary(d)) = doc.objects.get(&cur) else { return None };
                cur = d.entries.iter().find(|(key, _)| key.eq_ignore_ascii_case(k)).map(|(_, h)| *h)?;
            }
            matches!(doc.objects.get(&cur), Some(ObjectType::Dictionary(_))).then_some(cur)
        }
        CarryRole::Entity(h) => doc.get_entity(Handle::new(*h)).map(|_| Handle::new(*h)),
        CarryRole::Carried(_) => Some(Handle::new(old)),
    }
}

/// The CLASS record named `name`, for the report.
fn class_of(doc: &CadDocument, name: &str) -> Option<BridgeClass> {
    doc.classes.get_by_name(name).map(|c| BridgeClass {
        number: Some(c.class_number),
        dxf_name: c.dxf_name.clone(),
        cpp_name: c.cpp_class_name.clone(),
        application_name: c.application_name.clone(),
    })
}

/// An object's type for messages (its enum variant's name).
fn object_kind(o: &ObjectType) -> String {
    let s = format!("{o:?}");
    s.split(['(', ' ', '{']).next().unwrap_or("object").to_string()
}

// ---------------------------------------------------------------------------------------------
// Collect.

/// Dictionaries and XRECORDs by their owner: what an opaque record owns besides its entries.
fn owned_index(doc: &CadDocument) -> HashMap<Handle, Vec<Handle>> {
    let mut owned: HashMap<Handle, Vec<Handle>> = HashMap::new();
    for (h, o) in &doc.objects {
        let owner = match o {
            ObjectType::Dictionary(d) => d.owner,
            ObjectType::XRecord(x) => x.owner,
            _ => continue,
        };
        owned.entry(owner).or_default().push(*h);
    }
    for v in owned.values_mut() {
        v.sort();
    }
    owned
}

/// The handles of one named object dictionary entry's subtree, with each node's parent: a
/// dictionary's entries, and what an opaque record or XRECORD owns.
fn subtree_nodes(doc: &CadDocument, owned: &HashMap<Handle, Vec<Handle>>, nod: Handle, top: Handle) -> Result<Vec<(Handle, Handle)>, String> {
    let mut nodes: Vec<(Handle, Handle, usize)> = vec![(top, nod, 0)];
    let mut seen: HashSet<Handle> = HashSet::from([top]);
    let mut i = 0;
    while let Some(&(h, _, depth)) = nodes.get(i) {
        i += 1;
        if nodes.len() > MAX_CARRIED {
            return Err("too many records".into());
        }
        let children: Vec<Handle> = match doc.objects.get(&h) {
            Some(ObjectType::Dictionary(d)) => d.entries.iter().map(|(_, e)| *e).filter(|e| !e.is_null()).collect(),
            Some(ObjectType::Unknown { .. } | ObjectType::XRecord(_)) => owned.get(&h).cloned().unwrap_or_default(),
            _ => Vec::new(),
        };
        if !children.is_empty() && depth >= MAX_DEPTH {
            return Err("nested too deep".into());
        }
        for c in children {
            if !seen.insert(c) {
                return Err(format!("{:X} is in it twice (not a plain tree)", c.value()));
            }
            nodes.push((c, h, depth + 1));
        }
    }
    Ok(nodes.into_iter().map(|(h, p, _)| (h, p)).collect())
}

type Parts = (Vec<ObjectType>, Vec<usize>, Vec<(u64, CarryRole)>);

/// One entry's subtree as a carried unit. `others` maps the handles of every entry's subtree to
/// its key, for references between subtrees. `Err` says why it can't be carried.
fn carry_root(
    doc: &CadDocument,
    index: &HashMap<Handle, (Handle, String)>,
    nodes: &[(Handle, Handle)],
    key: &str,
    at: &HashMap<Handle, usize>,
    others: &HashMap<Handle, String>,
) -> Result<Parts, String> {
    let version = doc.version;
    let mine: HashSet<Handle> = nodes.iter().map(|n| n.0).collect();
    let inside = |h: Handle| h.is_null() || mine.contains(&h);
    let mut objects = Vec::new();
    let mut records = Vec::new();
    for &(h, parent) in nodes {
        match doc.objects.get(&h) {
            Some(o @ ObjectType::Dictionary(d)) => {
                if d.owner != parent {
                    return Err(format!("dictionary {:X} isn't owned by what holds it", h.value()));
                }
                if d.xdictionary_handle.is_some_and(|x| !x.is_null()) {
                    return Err(format!("dictionary {:X} has an extension dictionary", h.value()));
                }
                if !d.reactors.iter().all(|r| *r == parent || inside(*r)) {
                    return Err(format!("dictionary {:X} reacts to something outside it", h.value()));
                }
                objects.push(o.clone());
            }
            Some(o @ ObjectType::XRecord(x)) => {
                if x.owner != parent || x.xdictionary_handle.is_some_and(|h| !h.is_null()) || !x.reactors.iter().all(|r| *r == parent || inside(*r)) {
                    return Err(format!("XRECORD {:X} isn't a plain part of it", h.value()));
                }
                let refs_inside = x.entries.iter().all(|e| match &e.value {
                    XRecordValue::Handle(t) => inside(*t),
                    _ => true,
                });
                if !refs_inside || !x.object_references.is_empty() {
                    return Err(format!("XRECORD {:X} refers outside it", h.value()));
                }
                objects.push(o.clone());
            }
            Some(ObjectType::Unknown { owner, raw_dwg_version, .. }) if at.contains_key(&h) => {
                if *owner != parent {
                    return Err(format!("record {:X} isn't owned by what holds it", h.value()));
                }
                if raw_dwg_version.is_some_and(|v| v != version) {
                    return Err(format!("record {:X} holds data of another DWG version", h.value()));
                }
                if let Some(&i) = at.get(&h) {
                    records.push(i);
                }
            }
            Some(other) => return Err(format!("it also holds a {} ({:X}), which CADCraft writes itself", object_kind(other), h.value())),
            None => return Err(format!("it names {:X}, which isn't in the file", h.value())),
        }
    }
    let mut outside: Vec<(u64, CarryRole)> = Vec::new();
    for &h in nodes.iter().map(|n| &n.0).filter(|h| at.contains_key(h)) {
        let Some(ObjectType::Unknown { raw_dwg_data: Some(raw), raw_dwg_handle_bits, .. }) = doc.objects.get(&h) else { continue };
        let Some(l) = layout(raw, *raw_dwg_handle_bits, h.value()) else { return Err(format!("record {:X}'s data couldn't be read", h.value())) };
        for (r, eed) in l.eed_apps.iter().map(|r| (r, true)).chain(l.refs.iter().map(|r| (r, false))) {
            let t = if eed { Some(r.value) } else { target(r, h.value()) };
            let Some(t) = t else { return Err(format!("record {:X} has a handle reference of an unknown kind", h.value())) };
            if t == 0 || mine.contains(&Handle::new(t)) || outside.iter().any(|(o, _)| *o == t) {
                continue;
            }
            // A relative reference out of the subtree can't keep its form once moved.
            if r.code > 5 {
                return Err(format!("record {:X} refers out of the subtree by a relative handle", h.value()));
            }
            let role = match others.get(&Handle::new(t)) {
                Some(k) if k != key => CarryRole::Carried(k.clone()),
                _ => role(doc, index, Handle::new(t))
                    .ok_or_else(|| format!("record {:X} refers to {t:X}, whose role CADCraft doesn't know", h.value()))?,
            };
            if eed && !matches!(role, CarryRole::AppId(_)) {
                return Err(format!("record {:X}'s extended data names {t:X}, which isn't an application", h.value()));
            }
            outside.push((t, role));
        }
    }
    Ok((objects, records, outside))
}

/// Drop the roots that refer into a root that isn't there (repeatedly: one drop can drop more).
/// Returns the dropped ones with the entry they needed.
fn without_missing_needs(roots: &mut Vec<Root>) -> Vec<(Root, String)> {
    let mut dropped = Vec::new();
    loop {
        let keys: HashSet<String> = roots.iter().map(|r| r.key.clone()).collect();
        let Some(i) = roots.iter().position(|r| r.needs().any(|k| !keys.contains(k))) else { return dropped };
        let r = roots.remove(i);
        let need = r.needs().find(|k| !keys.contains(*k)).unwrap_or_default().to_string();
        dropped.push((r, need));
    }
}

/// Every opaque record of `doc` with its DWG bytes, and the subtrees of the named object dictionary
/// that can be carried (see the module docs). Records outside such subtrees are kept with the
/// reason, for a loss report. `None` when the file has no opaque records with DWG bytes.
pub fn collect(doc: &CadDocument) -> Option<NativeCarry> {
    let version = doc.version;
    let nod = doc.header.named_objects_dict_handle;
    let index = entry_index(doc);
    let mut carry = NativeCarry {
        version,
        version_code: format!("{version:?}"),
        classes: doc.classes.iter().cloned().collect(),
        roots: Vec::new(),
        records: Vec::new(),
        skipped: HashMap::new(),
    };
    // Every opaque object with DWG bytes, in handle order (the object map is unordered).
    let mut all: Vec<(Handle, Handle, &String, &Vec<u8>, i64)> = doc
        .objects
        .iter()
        .filter_map(|(h, o)| match o {
            ObjectType::Unknown { type_name, owner, raw_dwg_data: Some(raw), raw_dwg_handle_bits, .. } => {
                Some((*h, *owner, type_name, raw, *raw_dwg_handle_bits))
            }
            _ => None,
        })
        .collect();
    let has_entities = doc.entities().any(|e| matches!(e, acadrust::entities::EntityType::Unknown(u) if u.raw_dwg_data.is_some()));
    if all.is_empty() && !has_entities {
        return None;
    }
    all.sort_by_key(|r| r.0);
    all.truncate(MAX_CARRIED);
    let mut at: HashMap<Handle, usize> = HashMap::new();
    for (h, owner, type_name, raw, hb) in all {
        at.insert(h, carry.records.len());
        let path = key_path(&index, nod, h).unwrap_or_default();
        carry.records.push(CarriedRecord {
            handle: h.value(),
            owner: owner.value(),
            entity: false,
            class: class_of(doc, type_name),
            type_name: type_name.clone(),
            path,
            raw: raw.clone(),
            handle_bits: hb,
        });
    }
    // Other programs' entities: their bytes for the loss report (they aren't carried back yet).
    let mut entities: Vec<(Handle, Handle, String, Vec<u8>, i64)> = doc
        .entities()
        .filter_map(|e| match e {
            acadrust::entities::EntityType::Unknown(u) => {
                u.raw_dwg_data.as_ref().map(|raw| (u.common.handle, u.common.owner_handle, u.dxf_name.clone(), raw.clone(), u.dwg_handle_bits))
            }
            _ => None,
        })
        .collect();
    entities.sort_by_key(|e| e.0);
    for (h, owner, type_name, raw, hb) in entities.into_iter().take(MAX_CARRIED.saturating_sub(carry.records.len())) {
        carry.skipped.insert(h.value(), "an entity of another program (CADCraft doesn't carry entities back into a DWG yet)".into());
        carry.records.push(CarriedRecord {
            handle: h.value(),
            owner: owner.value(),
            entity: true,
            class: class_of(doc, &type_name),
            type_name,
            path: Vec::new(),
            raw,
            handle_bits: hb,
        });
    }
    let skip_all = |carry: &mut NativeCarry, why: String| {
        let handles: Vec<u64> = carry.records.iter().map(|r| r.handle).collect();
        for h in handles {
            carry.skipped.insert(h, why.clone());
        }
    };
    // R2010+ framing: the handle stream is the record's last bits. Older layouts aren't carried.
    if version < DxfVersion::AC1024 {
        skip_all(&mut carry, format!("DWG {version:?} records aren't carried (only 2010 and later)"));
        return Some(carry);
    }
    let Some(ObjectType::Dictionary(nod_dict)) = doc.objects.get(&nod) else {
        skip_all(&mut carry, "the file has no named object dictionary".into());
        return Some(carry);
    };
    let owned = owned_index(doc);
    // Each entry's subtree first: references between subtrees need them all.
    let mut subtrees: Vec<(&String, Handle, Result<Vec<(Handle, Handle)>, String>)> = Vec::new();
    let mut others: HashMap<Handle, String> = HashMap::new();
    for (key, top) in &nod_dict.entries {
        let nodes = subtree_nodes(doc, &owned, nod, *top);
        if let Ok(n) = &nodes {
            for (h, _) in n {
                others.entry(*h).or_insert_with(|| key.clone());
            }
        }
        subtrees.push((key, *top, nodes));
    }
    let mut failed: Vec<(Vec<u64>, String)> = Vec::new();
    for (key, top, nodes) in subtrees {
        let opaque = |n: &[(Handle, Handle)]| n.iter().filter_map(|(h, _)| at.contains_key(h).then_some(h.value())).collect::<Vec<u64>>();
        let nodes = match nodes {
            Ok(n) => n,
            Err(why) => {
                // Without its subtree, its records are found again below as not carried.
                failed.push((Vec::new(), format!("entry `{key}` of the named object dictionary isn't carried: {why}")));
                continue;
            }
        };
        let handles = opaque(&nodes);
        if handles.is_empty() {
            continue;
        }
        match carry_root(doc, &index, &nodes, key, &at, &others) {
            Ok((objects, records, outside)) => {
                let hard_owned = nod_dict.hard_owner_entries.iter().any(|k| k == key);
                carry.roots.push(Root { key: key.clone(), top, hard_owned, objects, records, outside });
            }
            Err(why) => failed.push((handles, format!("entry `{key}` of the named object dictionary isn't carried: {why}"))),
        }
    }
    for (r, need) in without_missing_needs(&mut carry.roots) {
        let handles: Vec<u64> = r.records.iter().filter_map(|&i| carry.records.get(i)).map(|x| x.handle).collect();
        failed.push((handles, format!("entry `{}` of the named object dictionary isn't carried: it refers into entry `{need}`, which isn't", r.key)));
    }
    for (handles, why) in failed {
        for h in handles {
            carry.skipped.insert(h, why.clone());
        }
    }
    let carried = carry.handles();
    let rest: Vec<u64> = carry.records.iter().map(|r| r.handle).filter(|h| !carried.contains(h) && !carry.skipped.contains_key(h)).collect();
    for h in rest {
        carry.skipped.insert(h, "not held by a carried entry of the named object dictionary".into());
    }
    Some(carry)
}

// ---------------------------------------------------------------------------------------------
// Apply.

/// A carried subtree that didn't go into the written document, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotCarried {
    pub key: String,
    pub handles: Vec<u64>,
    pub reason: String,
}

/// What [`apply`] did: records put back, and subtrees that couldn't be.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Applied {
    pub kept: usize,
    pub not_carried: Vec<NotCarried>,
    /// The bytes each kept record was written with (after its references moved), by handle.
    pub(crate) written: HashMap<u64, Vec<u8>>,
    pub(crate) keys: Vec<(String, u64)>,
}

/// Every handle the written document already uses.
fn used_handles(doc: &CadDocument) -> HashSet<Handle> {
    let mut used: HashSet<Handle> = doc.objects.keys().copied().collect();
    used.extend(doc.entities().map(|e| e.common().handle));
    used.extend(doc.layers.iter().map(|x| x.handle));
    used.extend(doc.line_types.iter().map(|x| x.handle));
    used.extend(doc.text_styles.iter().map(|x| x.handle));
    used.extend(doc.dim_styles.iter().map(|x| x.handle));
    used.extend(doc.app_ids.iter().map(|x| x.handle));
    used.extend(doc.block_records.iter().flat_map(|x| [x.handle, x.block_entity_handle, x.block_end_handle]));
    used
}

/// The handles of a dictionary subtree in `doc` (the dictionary and everything under it).
fn written_subtree(doc: &CadDocument, top: Handle) -> Vec<Handle> {
    let mut out = vec![top];
    let mut i = 0;
    while let Some(&h) = out.get(i) {
        i += 1;
        if out.len() > MAX_CARRIED {
            break;
        }
        if let Some(ObjectType::Dictionary(d)) = doc.objects.get(&h) {
            out.extend(d.entries.iter().map(|(_, e)| *e).filter(|e| !e.is_null()));
        }
    }
    out
}

/// The handle of a dictionary or XRECORD (the types a carried subtree may hold besides opaque records).
fn object_handle(o: &ObjectType) -> Option<Handle> {
    match o {
        ObjectType::Dictionary(d) => Some(d.handle),
        ObjectType::XRecord(x) => Some(x.handle),
        _ => None,
    }
}

/// A subtree that can't go back, with its records' handles and why.
fn not_carried(carry: &NativeCarry, root: &Root, reason: String) -> NotCarried {
    NotCarried { key: root.key.clone(), handles: root.records.iter().filter_map(|&i| carry.records.get(i)).map(|r| r.handle).collect(), reason }
}

/// A root ready to go in: its records re-encoded, and its written-copy handles to remove.
struct Ready<'a> {
    root: &'a Root,
    records: Vec<(&'a CarriedRecord, Vec<u8>, i64, Handle)>,
    replaced: HashSet<Handle>,
}

/// Put the carried subtrees into `doc`, the document the DWG writer gets (same version as the
/// carry). Each subtree goes in whole or not at all; one that can't is listed (and so is one that
/// refers into it), and the caller reports their records.
pub fn apply(doc: &mut CadDocument, carry: &NativeCarry) -> Applied {
    let mut applied = Applied::default();
    if doc.version != carry.version {
        applied.not_carried =
            carry.roots.iter().map(|r| not_carried(carry, r, format!("saved as {:?}, not {}", doc.version, carry.version_code))).collect();
        return applied;
    }
    let nod = doc.header.named_objects_dict_handle;
    let Some(ObjectType::Dictionary(nod_dict)) = doc.objects.get(&nod) else {
        applied.not_carried =
            carry.roots.iter().map(|r| not_carried(carry, r, "the written drawing has no named object dictionary".into())).collect();
        return applied;
    };
    let existing: HashMap<String, Handle> = nod_dict.entries.iter().map(|(k, h)| (k.to_ascii_uppercase(), *h)).collect();
    let used = used_handles(doc);
    let mut ready: Vec<Ready> = Vec::new();
    for root in &carry.roots {
        // The written drawing's own copy of this entry (from the DXF it was built from) goes: the
        // carried subtree replaces it.
        let replaced: HashSet<Handle> =
            existing.get(&root.key.to_ascii_uppercase()).map(|top| written_subtree(doc, *top)).unwrap_or_default().into_iter().collect();
        let mine = root
            .objects
            .iter()
            .filter_map(object_handle)
            .chain(root.records.iter().filter_map(|&i| carry.records.get(i)).map(|r| Handle::new(r.handle)));
        if let Some(h) = mine.into_iter().find(|h| used.contains(h) && !replaced.contains(h)) {
            applied.not_carried.push(not_carried(carry, root, format!("handle {:X} is used by the written drawing", h.value())));
            continue;
        }
        let mut moved_to: HashMap<u64, u64> = HashMap::new();
        let mut missing = None;
        for (old, role) in &root.outside {
            match resolve(doc, role, *old) {
                Some(h) => {
                    moved_to.insert(*old, h.value());
                }
                None => missing = Some(role.clone()),
            }
        }
        if let Some(role) = missing {
            applied.not_carried.push(not_carried(carry, root, format!("nothing in the written drawing has the role {role:?} its records refer to")));
            continue;
        }
        let mut records = Vec::new();
        for r in root.records.iter().filter_map(|&i| carry.records.get(i)) {
            let Some(l) = layout(&r.raw, r.handle_bits, r.handle) else { break };
            let moved: HashMap<usize, u64> = l
                .eed_apps
                .iter()
                .map(|x| (x, Some(x.value)))
                .chain(l.refs.iter().map(|x| (x, target(x, r.handle))))
                .filter_map(|(x, t)| t.and_then(|t| moved_to.get(&t)).filter(|n| x.code <= 5 && Some(**n) != t).map(|n| (x.start, *n)))
                .collect();
            let bytes = if moved.is_empty() { Some((r.raw.clone(), r.handle_bits)) } else { rewrite(&r.raw, &l, &moved) };
            let Some((raw, hb)) = bytes else { break };
            let owner = Handle::new(moved_to.get(&r.owner).copied().unwrap_or(r.owner));
            records.push((r, raw, hb, owner));
        }
        if records.len() != root.records.len() {
            applied.not_carried.push(not_carried(carry, root, "a record couldn't be re-encoded".into()));
            continue;
        }
        ready.push(Ready { root, records, replaced });
    }
    // A root that refers into one that isn't going in can't go in either.
    loop {
        let keys: HashSet<&str> = ready.iter().map(|r| r.root.key.as_str()).collect();
        let Some(i) = ready.iter().position(|r| r.root.needs().any(|k| !keys.contains(k))) else { break };
        let r = ready.remove(i);
        let need = r.root.needs().find(|k| !keys.contains(*k)).unwrap_or_default().to_string();
        applied.not_carried.push(not_carried(carry, r.root, format!("it refers into entry `{need}`, which didn't go back")));
    }
    if ready.is_empty() {
        return applied;
    }
    for r in ready {
        let root = r.root;
        for h in &r.replaced {
            doc.objects.remove(h);
        }
        if let Some(ObjectType::Dictionary(d)) = doc.objects.get_mut(&nod) {
            d.entries.retain(|(k, _)| !k.eq_ignore_ascii_case(&root.key));
            d.hard_owner_entries.retain(|k| !k.eq_ignore_ascii_case(&root.key));
        }
        for o in &root.objects {
            let mut o = o.clone();
            if let ObjectType::Dictionary(d) = &mut o
                && d.handle == root.top
            {
                d.owner = nod;
                d.reactors = vec![nod];
            }
            if let Some(h) = object_handle(&o) {
                doc.objects.insert(h, o);
            }
        }
        for (rec, raw, hb, owner) in r.records {
            let h = Handle::new(rec.handle);
            applied.written.insert(rec.handle, raw.clone());
            doc.objects.insert(
                h,
                ObjectType::Unknown {
                    type_name: rec.type_name.clone(),
                    handle: h,
                    owner,
                    raw_dxf_codes: None,
                    raw_dwg_data: Some(raw),
                    raw_dwg_handle_bits: hb,
                    raw_dwg_version: Some(carry.version),
                },
            );
            applied.kept += 1;
        }
        if let Some(ObjectType::Dictionary(d)) = doc.objects.get_mut(&nod) {
            d.entries.push((root.key.clone(), root.top));
            if root.hard_owned {
                d.hard_owner_entries.push(root.key.clone());
            }
        }
        applied.keys.push((root.key.clone(), root.top.value()));
    }
    positional_classes(doc, &carry.classes);
    let top = carry
        .records
        .iter()
        .map(|r| r.handle)
        .chain(carry.roots.iter().flat_map(|r| r.objects.iter().filter_map(object_handle).map(|h| h.value())))
        .max()
        .unwrap_or(0);
    doc.header.handle_seed = doc.header.handle_seed.max(top.saturating_add(1));
    applied
}

/// The file's classes in the file's order (a record's type code is its class's position), then
/// the classes only the written document has, numbered after them.
fn positional_classes(doc: &mut CadDocument, source: &[DxfClass]) {
    let mut out = DxfClassCollection::new();
    let mut names: HashSet<String> = HashSet::new();
    let mut n: i16 = 500;
    for c in source {
        let mut c = c.clone();
        c.class_number = n;
        n = n.saturating_add(1);
        names.insert(c.dxf_name.to_ascii_uppercase());
        out.push_preserving(c);
    }
    for c in doc.classes.iter() {
        if names.insert(c.dxf_name.to_ascii_uppercase()) {
            let mut c = c.clone();
            c.class_number = n;
            n = n.saturating_add(1);
            out.push_preserving(c);
        }
    }
    doc.classes = out;
}

/// Check a written DWG (read back with acadrust) holds every record [`apply`] kept, byte for byte,
/// under the named object dictionary keys it added. `Err` names the first difference.
pub fn verify(doc: &CadDocument, applied: &Applied) -> Result<(), String> {
    let nod = doc.header.named_objects_dict_handle;
    let Some(ObjectType::Dictionary(d)) = doc.objects.get(&nod) else { return Err("no named object dictionary".into()) };
    for (key, top) in &applied.keys {
        if !d.entries.iter().any(|(k, h)| k.eq_ignore_ascii_case(key) && h.value() == *top) {
            return Err(format!("named object dictionary entry `{key}` is missing"));
        }
    }
    for (h, raw) in &applied.written {
        match doc.objects.get(&Handle::new(*h)) {
            Some(ObjectType::Unknown { raw_dwg_data: Some(r), .. }) if r == raw => {}
            Some(ObjectType::Unknown { .. }) => return Err(format!("record {h:X} was read back with other bytes")),
            _ => return Err(format!("record {h:X} wasn't read back")),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Append a handle reference: 4-bit code, byte count, then the value's bytes.
    fn push_ref(out: &mut Vec<u8>, n: &mut usize, code: u8, value: u64) {
        let bytes = (64 - value.leading_zeros()).div_ceil(8) as usize;
        push_bits(out, n, u64::from(code), 4);
        push_bits(out, n, bytes as u64, 4);
        push_bits(out, n, value, bytes * 8);
    }

    /// An R2010+ record: type code, own handle, one EED block (application, 3 bytes), 13 data
    /// bits, a string-stream flag; then a handle stream.
    fn record(own: u64, app: u64, refs: &[(u8, u64)]) -> (Vec<u8>, i64) {
        let mut raw = Vec::new();
        let mut n = 0;
        push_bits(&mut raw, &mut n, 0b01, 2); // type code: RC + 0x1F0
        push_bits(&mut raw, &mut n, 0x4D, 8);
        push_ref(&mut raw, &mut n, 0, own);
        push_bits(&mut raw, &mut n, 0b01, 2); // EED size: a byte
        push_bits(&mut raw, &mut n, 3, 8);
        push_ref(&mut raw, &mut n, 5, app);
        push_bits(&mut raw, &mut n, 0xABCDEF, 24);
        push_bits(&mut raw, &mut n, 0b10, 2); // EED size 0: end
        push_bits(&mut raw, &mut n, 0b1011001110101, 13);
        push_bits(&mut raw, &mut n, 1, 1); // string stream flag
        let data = n;
        for &(code, value) in refs {
            push_ref(&mut raw, &mut n, code, value);
        }
        let hb = (raw.len() * 8 - data) as i64;
        (raw, hb)
    }

    /// A record's handle stream is decoded and encoded again bit for bit.
    #[test]
    fn records_are_read_and_rewritten_bit_exactly() {
        let (raw, hb) = record(0x100, 0x12, &[(4, 0x2A), (5, 0x10), (6, 0)]);
        let Some(l) = layout(&raw, hb, 0x100) else { panic!("layout") };
        assert_eq!(l.eed_apps.iter().map(|r| r.value).collect::<Vec<_>>(), [0x12]);
        let values: Vec<(u8, u64)> = l.refs.iter().map(|r| (r.code, r.value)).collect();
        assert_eq!(values.get(..3), Some(&[(4, 0x2A), (5, 0x10), (6, 0)][..]));
        assert_eq!(l.refs.get(2).and_then(|r| target(r, 0x100)), Some(0x101));
        // Nothing moved: the same bytes.
        assert_eq!(rewrite(&raw, &l, &HashMap::new()).map(|r| r.0), Some(raw.clone()));
        // The application and a handle-stream reference moved to longer handles: the data after
        // the application shifts, every other bit is kept, the handle stream starts later.
        let (Some(app), Some(hard)) = (l.eed_apps.first(), l.refs.get(1)) else { panic!("refs") };
        let moved = HashMap::from([(app.start, 0x5_1234), (hard.start, 0x1_2345)]);
        let Some((out, out_hb)) = rewrite(&raw, &l, &moved) else { panic!("rewrite") };
        let Some(again) = layout(&out, out_hb, 0x100) else { panic!("layout again") };
        assert_eq!(again.eed_apps.iter().map(|r| r.value).collect::<Vec<_>>(), [0x5_1234]);
        let values: Vec<(u8, u64)> = again.refs.iter().map(|r| (r.code, r.value)).collect();
        assert_eq!(values.get(..3), Some(&[(4, 0x2A), (5, 0x1_2345), (6, 0)][..]));
        assert_eq!(again.handle_start, l.handle_start + 16, "two more bytes of application handle");
        // The EED bytes, data bits and string-stream flag after the application are unchanged.
        let (Some(a0), Some(a1)) = (l.eed_apps.first(), again.eed_apps.first()) else { panic!("apps") };
        let tail = l.handle_start - (a0.start + a0.len);
        for i in 0..tail {
            assert_eq!(bit(&raw, a0.start + a0.len + i), bit(&out, a1.start + a1.len + i), "data bit {i}");
        }
    }

    /// Truncated or oversized handle streams are refused, never read past their end.
    #[test]
    fn hostile_records_are_refused_not_misread() {
        assert!(handle_refs(&[], 8).is_none());
        assert!(handle_refs(&[0xFF], 0).is_none());
        assert!(handle_refs(&[0xFF], 9).is_none());
        // A length nibble that runs past the end.
        assert!(handle_refs(&[0x0F, 0x00], 16).is_none());
        assert!(handle_refs(&[0x0F], i64::MAX).is_none());
        // Another own handle, or an EED block longer than the data stream.
        let (raw, hb) = record(0x100, 0x12, &[(4, 0x2A)]);
        assert!(layout(&raw, hb, 0x101).is_none());
        let mut long = raw.clone();
        if let Some(b) = long.get_mut(3) {
            *b = 0xFF;
        }
        assert!(layout(&long, hb, 0x100).is_none_or(|l| l.eed_apps.len() <= 1));
    }
}
