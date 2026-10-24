//! Data from a drawing file that CADCraft does not model, kept so that saving the drawing writes
//! it back: other programs' objects, extension dictionaries, xdata and properties (#528).
//!
//! It is kept as the file's own DXF groups, with the file's handles. The file reader fills it and
//! the writer puts it back, pointing references at whatever now has the referenced role (the
//! named object dictionary, a table record, a block, an entity); references to objects that are
//! gone are dropped. Commands don't edit it.
//!
//! Confirmed import losses live separately in [`PreservationState`]: a bounded diagnostic
//! context, shared by snapshots. Records a same-version DWG save can put back travel with it as an
//! opaque [`NativeCarry`] the I/O layer fills and reads; this crate never interprets it.

use std::collections::BTreeMap;

use crate::{Handle, RawTag};

/// Maximum retained import-loss diagnostics. The blocking count is independent of this cap.
pub const MAX_PRESERVATION_DETAILS: usize = 64;
/// Byte limit for each source/version/class/record name in import-loss diagnostics.
pub const MAX_PRESERVATION_TEXT_BYTES: usize = 256;

/// Source format of a confirmed incomplete import. DXF warnings alone do not create issues.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreservationFormat {
    Dwg,
    Dxf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreservationSource {
    pub format: PreservationFormat,
    /// File version identifier, e.g. `AC1032` (bounded on insertion).
    pub version: String,
}

/// CLASS identity for diagnostics, independent of any native decoder's types.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreservationClass {
    pub number: Option<i16>,
    pub dxf_name: String,
    pub cpp_name: String,
    pub application_name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreservationKind {
    Entity,
    Object,
    Metadata,
    /// A decoder skip whose entity/non-entity category was not available.
    Record,
}

/// Confirmed reasons why the editable model cannot represent all source database content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreservationReason {
    OpaqueCustomRecord,
    DecoderSkippedRecord,
    PrunedRecord,
    BlockMarkerXdata,
}

/// A diagnostic, never a raw-payload carrier or a promise of relocation/serialization support.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreservationIssue {
    pub reason: PreservationReason,
    pub kind: PreservationKind,
    pub handle: Option<Handle>,
    pub record_name: Option<String>,
    pub class: Option<PreservationClass>,
}

/// Other programs' records a DWG reader kept for a save in the same version, opaque here: the I/O
/// layer puts its own type in and takes it out ([`NativeCarry::get`]). Snapshots share it.
#[derive(Clone)]
pub struct NativeCarry(std::sync::Arc<dyn std::any::Any + Send + Sync>);

impl NativeCarry {
    pub fn new<T: std::any::Any + Send + Sync>(value: T) -> Self {
        Self(std::sync::Arc::new(value))
    }

    pub fn get<T: std::any::Any>(&self) -> Option<&T> {
        self.0.downcast_ref::<T>()
    }
}

impl std::fmt::Debug for NativeCarry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativeCarry(..)")
    }
}

/// The same carried records: one read, shared by its copies.
impl PartialEq for NativeCarry {
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for NativeCarry {}

/// Confirmed import loss, shared by drawing/undo snapshots. Details and text are bounded; the
/// total count saturates without ever becoming safe again. There is deliberately no clear API:
/// editing the partial model cannot recover records omitted before it was constructed.
///
/// This context is not serialized into DXF or DWG. File readers populate it; database writers use
/// it to report what a saved file lacks (and a same-version DWG save to put the carried records
/// back). It does not derive issues from human warning text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PreservationState {
    source: Option<PreservationSource>,
    issues: Vec<PreservationIssue>,
    issue_count: u64,
    native: Option<NativeCarry>,
}

impl PreservationState {
    pub fn new(format: PreservationFormat, version: &str) -> Self {
        Self { source: Some(PreservationSource { format, version: bounded_text(version) }), ..Self::default() }
    }

    pub fn source(&self) -> Option<&PreservationSource> {
        self.source.as_ref()
    }

    pub fn issues(&self) -> &[PreservationIssue] {
        &self.issues
    }

    /// Total confirmed issues, including details omitted by the cap (saturates at `u64::MAX`).
    pub fn issue_count(&self) -> u64 {
        self.issue_count
    }

    /// Whether the import lost anything: a save then reports what the file it writes lacks.
    pub fn blocks_save(&self) -> bool {
        self.issue_count != 0
    }

    /// The records kept for a same-version DWG save (they are among [`Self::issues`] too: the
    /// editable drawing lacks them).
    pub fn native(&self) -> Option<&NativeCarry> {
        self.native.as_ref()
    }

    pub fn set_native(&mut self, native: NativeCarry) {
        self.native = Some(native);
    }

    /// Record one issue or a counted batch (e.g. decoder skips), in constant bounded work.
    /// A zero count is a no-op. Only confirmed losses belong here, never generic read warnings.
    pub fn record(&mut self, mut issue: PreservationIssue, count: u64) {
        if count == 0 {
            return;
        }
        self.issue_count = self.issue_count.saturating_add(count);
        if self.issues.len() >= MAX_PRESERVATION_DETAILS {
            return;
        }
        if let Some(name) = &mut issue.record_name {
            *name = bounded_text(name);
        }
        if let Some(class) = &mut issue.class {
            class.dxf_name = bounded_text(&class.dxf_name);
            class.cpp_name = bounded_text(&class.cpp_name);
            class.application_name = bounded_text(&class.application_name);
        }
        self.issues.push(issue);
    }
}

fn bounded_text(text: &str) -> String {
    let mut end = text.len().min(MAX_PRESERVATION_TEXT_BYTES);
    while !text.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    text.get(..end).unwrap_or_default().to_owned()
}

/// What a file handle stood for when the object it named is one CADCraft writes itself (under a
/// new handle on save): a kind and an upper-case name, e.g. `("LAYER", "WALLS")`,
/// `("TABLE", "LAYER")`, `("BLOCK", "DOOR")`, `("NOD", "")` for the named object dictionary and
/// `("NOD", "ACAD_GROUP")` for one of its entries.
pub type ForeignKey = (String, String);

/// A non-graphical object of a type or role CADCraft does not model.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ForeignObject {
    /// Its handle in the file it came from (kept on save).
    pub handle: Handle,
    /// Its DXF type (group 0), e.g. `XRECORD`.
    pub kind: String,
    /// Its groups after the type, as read.
    pub tags: Vec<RawTag>,
}

/// What an entity, table record or block carries beyond what CADCraft models.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ForeignExtra {
    /// Its extension dictionary (one of [`Foreign::objects`]).
    pub xdict: Option<Handle>,
    /// Objects of [`Foreign::objects`] that react to it (persistent reactors).
    pub reactors: Vec<Handle>,
    /// Other applications' extended data of a table record or block: groups from `1001` on. An
    /// entity's is part of the entity ([`crate::Common::xdata`]), so it follows the entity's copies.
    pub xdata: Vec<RawTag>,
    /// The colour book colour (group 430) and the true colour (group 420) it was read with: it is
    /// written back while the colour is unchanged.
    pub color_book: Option<(String, String)>,
    /// Other unmodelled property groups (material 347, shadow mode 284).
    pub props: Vec<RawTag>,
}

impl ForeignExtra {
    pub fn is_empty(&self) -> bool {
        self.xdict.is_none() && self.reactors.is_empty() && self.xdata.is_empty() && self.color_book.is_none() && self.props.is_empty()
    }
}

/// Everything of a file CADCraft keeps without modelling it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Foreign {
    /// CLASS records (groups after `0 CLASS`) of the kept objects' and entities' types.
    pub classes: Vec<Vec<RawTag>>,
    /// Kept objects, in file order.
    pub objects: Vec<ForeignObject>,
    /// Entries of the named object dictionary that CADCraft doesn't write itself: (key, object).
    pub dictionary: Vec<(String, Handle)>,
    /// Extras of entities, by handle.
    pub entities: BTreeMap<Handle, ForeignExtra>,
    /// Extras of table records (`("LAYER", "WALLS")`), tables (`("TABLE", "LAYER")`) and blocks
    /// (`("BLOCK", "DOOR")`), by [`ForeignKey`].
    pub records: BTreeMap<ForeignKey, ForeignExtra>,
    /// The roles of the file handles the kept data refers to, for objects CADCraft rewrites.
    pub targets: BTreeMap<Handle, ForeignKey>,
    /// The upper-case names of the file's classes in its CLASSES order, kept when the file has
    /// proxy objects or entities: a proxy's class number (group 91) is 500 plus its class's place
    /// here, and saving renumbers it to the written CLASSES (#1812).
    pub class_order: Vec<String>,
    /// The file's data store section (ACDSDATA, AutoCAD 2013 and later), as read: its groups
    /// after the section name. It holds the ACIS geometry of 3D solids, regions, bodies and
    /// surfaces, which from 2013 on those entities don't carry themselves: each `ACDSRECORD`
    /// names its entity with a 320 handle (see [`Foreign::data_store_entities`]).
    pub data_store: Vec<RawTag>,
}

impl Foreign {
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
            && self.dictionary.is_empty()
            && self.entities.is_empty()
            && self.records.is_empty()
            && self.classes.is_empty()
            && self.targets.is_empty()
            && self.class_order.is_empty()
            && self.data_store.is_empty()
    }

    /// The entities the data store has records for (their 320 handles).
    pub fn data_store_entities(&self) -> std::collections::BTreeSet<Handle> {
        self.data_store.iter().filter(|t| t.code == 320).filter_map(|t| Handle::parse_hex(t.value.trim())).collect()
    }
}

#[cfg(test)]
mod preservation_tests {
    use super::*;
    use crate::{Drawing, RefEdit, Space};
    use std::sync::Arc;

    fn issue() -> PreservationIssue {
        PreservationIssue {
            reason: PreservationReason::OpaqueCustomRecord,
            kind: PreservationKind::Object,
            handle: Some(Handle(0x42)),
            record_name: Some("FF1_OBJECT".into()),
            class: Some(PreservationClass {
                number: Some(501),
                dxf_name: "FF1_OBJECT".into(),
                cpp_name: "RustFF1_OBJECT".into(),
                application_name: "FF1".into(),
            }),
        }
    }

    #[test]
    fn preservation_defaults_have_no_unresolved_import_loss() {
        for d in [Drawing::default(), Drawing::new_metric()] {
            assert!(!d.preservation.blocks_save());
            assert_eq!(d.preservation.issue_count(), 0);
            assert!(d.preservation.issues().is_empty());
            assert!(d.preservation.source().is_none());
        }
    }

    #[test]
    fn preservation_survives_clones_edits_and_committed_refedit_snapshots() {
        let mut state = PreservationState::new(PreservationFormat::Dwg, "AC1032");
        state.record(issue(), 1);
        let mut d = Drawing { preservation: Arc::new(state), ..Drawing::default() };
        let snapshot = d.clone();
        d.header.set_str("CUSTOM", "edited");
        d.refedit = Some(RefEdit {
            block: "TEST".into(),
            insert: Handle(1),
            insert_was_visible: true,
            space: Space::Model,
            to_world: crate::geom::Mat3::IDENTITY,
            items: vec![],
            ghosts: vec![],
            released: vec![],
        });
        let committed = d.committed();
        assert!(committed.refedit.is_none());
        for other in [&snapshot, committed.as_ref()] {
            assert!(Arc::ptr_eq(&d.preservation, &other.preservation));
            assert!(other.preservation.blocks_save());
            assert_eq!(other.preservation.issue_count(), 1);
            assert_eq!(other.preservation.issues(), &[issue()]);
            assert_eq!(other.preservation.source().unwrap().version, "AC1032");
        }
        assert_ne!(d, Drawing::default());
        assert_ne!(Drawing { preservation: d.preservation.clone(), ..Drawing::default() }, Drawing::default());
    }

    #[test]
    fn preservation_detail_cap_does_not_cap_or_wrap_blocking_count() {
        let mut state = PreservationState::new(PreservationFormat::Dwg, "AC1032");
        state.record(issue(), 0);
        assert!(!state.blocks_save());
        assert!(state.issues().is_empty());
        for _ in 0..MAX_PRESERVATION_DETAILS + 7 {
            state.record(issue(), 1);
        }
        assert_eq!(state.issues().len(), MAX_PRESERVATION_DETAILS);
        assert_eq!(state.issue_count(), (MAX_PRESERVATION_DETAILS + 7) as u64);
        state.record(issue(), u64::MAX);
        state.record(issue(), 1);
        assert_eq!(state.issue_count(), u64::MAX);
        assert!(state.blocks_save());
    }

    #[test]
    fn preservation_bounds_all_untrusted_strings_at_utf8_boundaries() {
        // The byte cap lands inside a four-byte character, not at a character boundary.
        let huge = "🛠a".repeat(MAX_PRESERVATION_TEXT_BYTES);
        let mut state = PreservationState::new(PreservationFormat::Dwg, &huge);
        let mut entry = issue();
        entry.record_name = Some(huge.clone());
        entry.class = Some(PreservationClass { number: None, dxf_name: huge.clone(), cpp_name: huge.clone(), application_name: huge.clone() });
        state.record(entry, 1);
        let kept = &state.issues()[0];
        let class = kept.class.as_ref().unwrap();
        for s in [&state.source().unwrap().version, kept.record_name.as_ref().unwrap(), &class.dxf_name, &class.cpp_name, &class.application_name] {
            assert!(s.len() <= MAX_PRESERVATION_TEXT_BYTES);
            assert!(!s.is_empty());
            assert!(huge.starts_with(s));
        }
        assert!(state.blocks_save());
    }
}
