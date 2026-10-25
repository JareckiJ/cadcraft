//! What a saved file lacks of the drawing's source, record by record (FF1).
//!
//! A drawing read from a DWG can lack records the editable model can't hold: other programs'
//! objects and entities, records the reader skipped, BLOCK-marker XDATA. A save never refuses
//! because of them and never drops them silently: the caller gets a [`LossReport`] (written
//! beside the file as `<file>.lost.json` by the CLI and the editor) naming each one, with the
//! record's own DWG bytes or DXF groups where they were kept, so it can be restored later. A save
//! as DWG in the source's version puts the carried records back instead (`cadcraft_dwg`'s carry).
//!
//! The import state comes only from the DWG bridge's typed issues, never from human warning text
//! and never from DXF `skipped_records`/`read_notes`.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use cadcraft_doc::foreign::{PreservationClass, PreservationFormat, PreservationIssue, PreservationKind, PreservationReason, PreservationState};
use cadcraft_doc::{Drawing, EntityKind, Handle};
use cadcraft_dwg::{BridgeIssue, BridgeIssueKind, BridgeIssueReason, CarriedRecord, DwgRead, NativeCarry, NotCarried};
use serde_json::{Value, json};

/// The drawing-side context for a DWG read's typed issues (empty when nothing was lost), with the
/// records carried for a same-version DWG save.
pub(crate) fn state_from_bridge(read: &DwgRead) -> PreservationState {
    let mut state = PreservationState::new(PreservationFormat::Dwg, &read.version);
    let mut mapped = 0u64;
    for i in &read.issues {
        mapped = mapped.saturating_add(i.count);
        state.record(map_issue(i), i.count);
    }
    // Losses beyond the bridge's detail cap still count.
    if let Some(rest) = read.issue_count.checked_sub(mapped).filter(|r| *r > 0) {
        let rest_issue = PreservationIssue {
            reason: PreservationReason::DecoderSkippedRecord,
            kind: PreservationKind::Record,
            handle: None,
            record_name: None,
            class: None,
        };
        state.record(rest_issue, rest);
    }
    if let Some(carry) = &read.carry {
        state.set_native(cadcraft_doc::foreign::NativeCarry::new(carry.clone()));
    }
    state
}

/// The drawing's view of a bridge issue (no dependency type crosses into the document).
fn map_issue(i: &BridgeIssue) -> PreservationIssue {
    PreservationIssue {
        reason: match i.reason {
            BridgeIssueReason::OpaqueCustomRecord => PreservationReason::OpaqueCustomRecord,
            BridgeIssueReason::DecoderSkippedRecord => PreservationReason::DecoderSkippedRecord,
            BridgeIssueReason::PrunedRecord => PreservationReason::PrunedRecord,
            BridgeIssueReason::BlockMarkerXdata => PreservationReason::BlockMarkerXdata,
        },
        kind: match i.kind {
            BridgeIssueKind::Entity => PreservationKind::Entity,
            BridgeIssueKind::Object => PreservationKind::Object,
            BridgeIssueKind::Metadata => PreservationKind::Metadata,
            BridgeIssueKind::Record => PreservationKind::Record,
        },
        handle: i.handle.map(Handle),
        record_name: i.record_name.clone(),
        class: i.class.as_ref().map(|c| PreservationClass {
            number: c.number,
            dxf_name: c.dxf_name.clone(),
            cpp_name: c.cpp_name.clone(),
            application_name: c.application_name.clone(),
        }),
    }
}

/// The records a DWG read carried for a same-version DWG save, if any.
pub(crate) fn carry(d: &Drawing) -> Option<&NativeCarry> {
    d.preservation.native().and_then(|n| n.get::<std::sync::Arc<NativeCarry>>()).map(std::sync::Arc::as_ref)
}

/// A human warning for the read; presentation only. Decoder skips already have their own read
/// note (which says saving over the original would lose them), so only other losses add one.
pub(crate) fn warning(state: &PreservationState, carried: usize) -> Option<String> {
    let other = state.issues().iter().any(|i| i.reason != PreservationReason::DecoderSkippedRecord);
    (state.blocks_save() && other).then(|| {
        let version = state.source().map_or("", |s| s.version.as_str());
        let n = state.issue_count();
        if carried as u64 >= n {
            format!("DWG {version}: {n} record(s) of other programs aren't part of the editable drawing; saving as DWG {version} keeps them, other saves list them in a <file>.lost.json beside the file")
        } else {
            format!("DWG {version}: {n} record(s) of the file aren't part of the editable drawing ({carried} of them are kept for a DWG {version} save); a save lists what its file lacks in a <file>.lost.json beside it")
        }
    })
}

/// A DWG record's bytes as read, for restoring it into a DWG of the same version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawDwg {
    pub version: String,
    pub handle_bits: i64,
    pub bytes: Vec<u8>,
}

/// One record a saved file lacks.
#[derive(Clone, Debug, PartialEq)]
pub struct LostRecord {
    /// Why it isn't in the file, e.g. `custom record of another program (DXF can't hold its DWG data)`.
    pub reason: String,
    /// `entity`, `object`, `metadata` or `record`.
    pub kind: &'static str,
    /// Its type, e.g. `AEC_DISP_PROPS_ENT`, or the block a BLOCK-marker XDATA was on.
    pub record: Option<String>,
    /// Its handle in the source file (hex).
    pub handle: Option<String>,
    pub owner: Option<String>,
    /// Keys from the named object dictionary to its entry, for dictionary-held objects.
    pub path: Vec<String>,
    /// CLASS: (DXF name, C++ name, application, number).
    pub class: Option<(String, String, String, Option<i16>)>,
    pub dwg: Option<RawDwg>,
    /// Its DXF groups, for records kept from a DXF file.
    pub dxf: Vec<(i32, String)>,
}

/// What a saved file lacks of the drawing's source: each record (with its kept data where there
/// is any), and how many more there are beyond the listed ones.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LossReport {
    /// The saved format, e.g. `DWG AC1032`, `DXF`.
    pub target: String,
    /// The source, e.g. `DWG AC1032`.
    pub source: Option<String>,
    /// Records of other programs written back natively (a same-version DWG save).
    pub kept_natively: usize,
    pub records: Vec<LostRecord>,
    /// Lost records not listed one by one (the read's detail cap).
    pub more: u64,
}

impl LossReport {
    /// Whether the file lacks nothing.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty() && self.more == 0
    }

    /// Every lost record, listed or not.
    pub fn count(&self) -> u64 {
        (self.records.len() as u64).saturating_add(self.more)
    }

    /// One line for messages: "312 record(s) lost: AEC_DISP_PROPS_ENT × 141, …".
    pub fn summary(&self) -> String {
        let mut by: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
        for r in &self.records {
            *by.entry(r.record.as_deref().unwrap_or(r.kind)).or_default() += 1;
        }
        let mut types: Vec<(&str, usize)> = by.into_iter().collect();
        types.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        let mut list: Vec<String> = types.iter().take(4).map(|(t, n)| format!("{t} × {n}")).collect();
        if types.len() > 4 {
            list.push(format!("{} more types", types.len() - 4));
        }
        if self.more > 0 {
            list.push(format!("{} more unlisted", self.more));
        }
        let kept = if self.kept_natively > 0 { format!(" ({} other programs' records kept)", self.kept_natively) } else { String::new() };
        format!("{} record(s) of the source aren't in this {} file{kept}: {}", self.count(), self.target, list.join(", "))
    }

    /// The report as JSON (the `<file>.lost.json` beside a saved file).
    pub fn to_json(&self) -> String {
        let records: Vec<Value> = self
            .records
            .iter()
            .map(|r| {
                let mut v = json!({ "reason": r.reason, "kind": r.kind });
                if let Some(x) = &r.record {
                    v["type"] = json!(x);
                }
                if let Some(x) = &r.handle {
                    v["handle"] = json!(x);
                }
                if let Some(x) = &r.owner {
                    v["owner"] = json!(x);
                }
                if !r.path.is_empty() {
                    v["dictionary_path"] = json!(r.path);
                }
                if let Some((dxf, cpp, app, number)) = &r.class {
                    v["class"] = json!({ "dxf_name": dxf, "cpp_name": cpp, "application": app, "number": number });
                }
                if let Some(raw) = &r.dwg {
                    v["dwg"] = json!({ "version": raw.version, "handle_stream_bits": raw.handle_bits, "data_base64": base64(&raw.bytes) });
                }
                if !r.dxf.is_empty() {
                    v["dxf_groups"] = Value::Array(r.dxf.iter().map(|(c, val)| json!([c, val])).collect());
                }
                v
            })
            .collect();
        let v = json!({
            "about": "Records of the source drawing that this saved file lacks, written by CADCraft. `dwg.data_base64` is the record's DWG data as read (the merged stream between the size prefix and the CRC, R2010+ framing with `handle_stream_bits`); `dxf_groups` its DXF groups.",
            "target": self.target,
            "source": self.source,
            "kept_natively": self.kept_natively,
            "lost": self.count(),
            "unlisted": self.more,
            "summary": self.summary(),
            "records": records,
        });
        serde_json::to_string_pretty(&v).unwrap_or_default()
    }
}

/// Standard base64 with padding (the record bytes in the JSON report).
fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let b = [c.first().copied().unwrap_or(0), c.get(1).copied().unwrap_or(0), c.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18u32, 12, 6, 0].into_iter().enumerate() {
            if i <= c.len() {
                out.push(char::from(T.get(((n >> shift) & 63) as usize).copied().unwrap_or(b'A')));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// A handle in upper-case hex.
fn hex(h: u64) -> String {
    format!("{h:X}")
}

/// The report's name for a record kind.
fn kind_name(k: PreservationKind) -> &'static str {
    match k {
        PreservationKind::Entity => "entity",
        PreservationKind::Object => "object",
        PreservationKind::Metadata => "metadata",
        PreservationKind::Record => "record",
    }
}

/// Why a record the import lost isn't in the file.
fn import_reason(r: PreservationReason) -> &'static str {
    match r {
        PreservationReason::OpaqueCustomRecord => "custom record of another program, not part of the editable drawing",
        PreservationReason::DecoderSkippedRecord => "record the DWG reader couldn't decode (left out when the file was opened)",
        PreservationReason::PrunedRecord => "record left out when the file was opened (unused DGN line style component)",
        PreservationReason::BlockMarkerXdata => "extended data on a BLOCK marker, which the DWG bridge doesn't carry",
    }
}

/// A carried record the file lacks, with its DWG bytes.
fn carried_record(r: &CarriedRecord, version: &str, reason: String) -> LostRecord {
    LostRecord {
        reason,
        kind: if r.entity { "entity" } else { "object" },
        record: Some(r.type_name.clone()),
        handle: Some(hex(r.handle)),
        owner: Some(hex(r.owner)),
        path: r.path.clone(),
        class: r.class.as_ref().map(|c| (c.dxf_name.clone(), c.cpp_name.clone(), c.application_name.clone(), c.number)),
        dwg: (!r.raw().is_empty()).then(|| RawDwg { version: version.to_string(), handle_bits: r.handle_bits(), bytes: r.raw().to_vec() }),
        dxf: Vec::new(),
    }
}

/// An import issue the file lacks (no bytes: the record wasn't carried).
fn issue_record(i: &PreservationIssue) -> LostRecord {
    LostRecord {
        reason: import_reason(i.reason).to_string(),
        kind: kind_name(i.kind),
        record: i.record_name.clone(),
        handle: i.handle.map(|h| h.hex()),
        owner: None,
        path: Vec::new(),
        class: i.class.as_ref().map(|c| (c.dxf_name.clone(), c.cpp_name.clone(), c.application_name.clone(), c.number)),
        dwg: None,
        dxf: Vec::new(),
    }
}

/// What the import lost that `target` doesn't hold either: every carried record not in `kept`
/// (with its DWG bytes), and the read's other issues. `why_carried` says why carried records
/// aren't in the file. Returns the records and how many more there are unlisted.
pub(crate) fn import_losses(d: &Drawing, kept: &HashSet<u64>, why_carried: &dyn Fn(u64) -> String) -> (Vec<LostRecord>, u64) {
    let state = &d.preservation;
    let mut out = Vec::new();
    let carry = carry(d);
    // Every opaque record kept with its bytes, carried back or not: listed from the carry.
    let carried: HashSet<u64> = carry.map(|c| c.records().iter().map(|r| r.handle).collect()).unwrap_or_default();
    if let Some(c) = carry {
        for r in c.records().iter().filter(|r| !kept.contains(&r.handle)) {
            let why = match c.skipped(r.handle) {
                Some(s) if r.entity => format!("custom entity of another program, not part of the editable drawing: {s}"),
                Some(s) => format!("custom record of another program, not part of the editable drawing and not carried for a DWG save: {s}"),
                None => why_carried(r.handle),
            };
            out.push(carried_record(r, c.version(), why));
        }
    }
    for i in state.issues() {
        if !i.handle.is_some_and(|h| carried.contains(&h.0)) {
            out.push(issue_record(i));
        }
    }
    // Every carried record is one of the read's issues, so what the file lacks is the issues less
    // the records kept; those beyond the read's detail cap aren't listed one by one.
    let lost = state.issue_count().saturating_sub(kept.len() as u64);
    let more = lost.saturating_sub(out.len() as u64);
    (out, more)
}

/// Other programs' records a written DWG leaves out. `dropped`, the DWG writer's own list, decides
/// what is lost; each one gets its DXF groups when CADCraft kept the record from a DXF file
/// (matched by kind, type and handle, else by kind and type in order). A record the writer kept
/// isn't listed.
pub(crate) fn dxf_customs_lost(d: &Drawing, version: crate::Version, dropped: &[cadcraft_dwg::Dropped]) -> Vec<LostRecord> {
    let held = format!("custom record of another program: CADCraft keeps it in DXF, a DWG {} can't hold it", version.code());
    let unheld = format!("record of another program the DWG writer can't write as {}", version.code());
    // What CADCraft kept of each candidate: (kind, upper-case type, handle, DXF groups).
    let mut kept: Vec<Option<(&'static str, String, u64, Vec<(i32, String)>)>> = Vec::new();
    let spaces = std::iter::once(&d.model).chain(d.layouts.iter().map(|l| &l.entities)).chain(d.blocks.values().map(|b| &b.entities));
    for e in spaces.flat_map(|s| s.iter()) {
        if let EntityKind::Unknown(u) = &e.kind {
            kept.push(Some(("entity", u.dxf_type.to_ascii_uppercase(), e.handle.0, u.tags.iter().map(|t| (t.code, t.value.clone())).collect())));
        }
    }
    for o in &d.foreign.objects {
        kept.push(Some(("object", o.kind.to_ascii_uppercase(), o.handle.0, o.tags.iter().map(|t| (t.code, t.value.clone())).collect())));
    }
    // Indexes into `kept`, each slot used once: by (kind, type, handle), and by (kind, type) in order.
    let mut exact: HashMap<(&'static str, String, u64), Vec<usize>> = HashMap::new();
    let mut by_type: HashMap<(&'static str, String), std::collections::VecDeque<usize>> = HashMap::new();
    for (i, (k, t, h, _)) in kept.iter().enumerate().filter_map(|(i, c)| c.as_ref().map(|c| (i, c))) {
        exact.entry((*k, t.clone(), *h)).or_default().push(i);
        by_type.entry((*k, t.clone())).or_default().push_back(i);
    }
    // Exact handles first, for every dropped record, so a type-only match can't take a slot
    // another record names by handle.
    let mut slot: Vec<Option<usize>> =
        dropped.iter().map(|r| exact.get_mut(&(r.kind, r.type_name.to_ascii_uppercase(), r.handle)).and_then(Vec::pop)).collect();
    let mut taken = vec![false; kept.len()];
    for i in slot.iter().flatten() {
        if let Some(t) = taken.get_mut(*i) {
            *t = true;
        }
    }
    let mut out = Vec::with_capacity(dropped.len());
    for (r, at) in dropped.iter().zip(slot.iter_mut()) {
        let name = r.type_name.to_ascii_uppercase();
        if at.is_none()
            && let Some(q) = by_type.get_mut(&(r.kind, name))
        {
            while let Some(i) = q.pop_front() {
                if let Some(t @ false) = taken.get_mut(i) {
                    *t = true;
                    *at = Some(i);
                    break;
                }
            }
        }
        let (reason, dxf) = match at.and_then(|i| kept.get_mut(i)).and_then(Option::take) {
            Some((_, _, _, groups)) => (held.clone(), groups),
            None => (unheld.clone(), Vec::new()),
        };
        out.push(LostRecord {
            reason,
            kind: r.kind,
            record: Some(r.type_name.clone()),
            handle: Some(cadcraft_doc::Handle(r.handle).hex()),
            owner: None,
            path: Vec::new(),
            class: None,
            dwg: None,
            dxf,
        });
    }
    out
}

/// BLOCK markers whose extended data a DWG can't hold.
pub(crate) fn block_xdata_lost(found: &[((String, String), String)], version: crate::Version) -> Vec<LostRecord> {
    found
        .iter()
        .map(|((_, block), app)| LostRecord {
            reason: format!(
                "extended data of application `{app}` on the BLOCK marker of block `{block}`: a DWG {} written by CADCraft can't hold it",
                version.code()
            ),
            kind: "metadata",
            record: Some(block.clone()),
            handle: None,
            owner: None,
            path: Vec::new(),
            class: None,
            dwg: None,
            dxf: Vec::new(),
        })
        .collect()
}

/// Why each carried record of `not` isn't in the DWG, by handle.
pub(crate) fn not_carried_reasons(not: &[NotCarried]) -> std::collections::HashMap<u64, String> {
    let mut m = std::collections::HashMap::new();
    for n in not {
        for h in &n.handles {
            let mut why = String::from("custom record of another program, not put back into the DWG: ");
            let _ = write!(why, "{}", n.reason);
            m.insert(*h, why);
        }
    }
    m
}

/// The report for `target`: `None` when the file lacks nothing and kept nothing natively (a report
/// with nothing lost still says how many records of other programs went back as they were).
pub(crate) fn report(d: &Drawing, target: String, kept_natively: usize, records: Vec<LostRecord>, more: u64) -> Option<LossReport> {
    let source = d.preservation.source().map(|s| format!("{} {}", if s.format == PreservationFormat::Dwg { "DWG" } else { "DXF" }, s.version));
    let r = LossReport { target, source, kept_natively, records, more };
    (!r.is_empty() || kept_natively > 0).then_some(r)
}

#[cfg(test)]
mod tests {
    /// The base64 encoder matches the standard alphabet and padding.
    #[test]
    fn base64_matches_the_standard_alphabet_and_padding() {
        assert_eq!(super::base64(b""), "");
        assert_eq!(super::base64(b"f"), "Zg==");
        assert_eq!(super::base64(b"fo"), "Zm8=");
        assert_eq!(super::base64(b"foo"), "Zm9v");
        assert_eq!(super::base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(super::base64(&[0xFF, 0xFE, 0x00]), "//4A");
    }

    /// Exact handles are matched first: a record listed earlier, matched only by type, can't take
    /// the slot a later record names by handle.
    #[test]
    fn exact_handles_are_matched_before_types() {
        use cadcraft_doc::foreign::ForeignObject;
        use cadcraft_doc::{Drawing, Handle, RawTag};
        let object =
            |h: u64, payload: &str| ForeignObject { handle: Handle(h), kind: "X_OBJ".into(), tags: vec![RawTag { code: 1, value: payload.into() }] };
        let mut d = Drawing::default();
        std::sync::Arc::make_mut(&mut d.foreign).objects = vec![object(0x40, "forty"), object(0x41, "forty-one")];
        let dropped = |h: u64| cadcraft_dwg::Dropped { kind: "object", type_name: "X_OBJ".into(), handle: h };
        // 0x99 matches nothing by handle; 0x40 names the first slot.
        let lost = super::dxf_customs_lost(&d, crate::Version::R2018, &[dropped(0x99), dropped(0x40)]);
        let payload = |i: usize| lost.get(i).and_then(|r| r.dxf.first()).map(|(_, v)| v.clone());
        assert_eq!((payload(0), payload(1)), (Some("forty-one".into()), Some("forty".into())));
    }
}
