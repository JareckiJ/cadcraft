//! Typed report of what a native DWG read could not carry into the DXF conversion.
//!
//! The bridge converts a DWG through acadrust's DXF writer, which omits some native content
//! (custom-class entities and objects, BLOCK-marker XDATA) without any diagnostic, and the
//! decoder itself can skip records. The inventories here are taken from the native document
//! *before* conversion and pruning, from typed fields, never from message text. The report is a
//! diagnostic; the records that can go back into a same-version DWG travel separately, in
//! [`DwgRead::carry`]. Only native DWG reads produce it; `dwg` stays standalone, so callers map
//! these types to their own.

/// Most detailed issues kept in [`DwgRead::issues`]; [`DwgRead::issue_count`] still counts all.
pub const MAX_BRIDGE_ISSUES: usize = 64;
/// Byte limit for each name in an issue.
#[cfg(not(target_arch = "wasm32"))]
const MAX_TEXT_BYTES: usize = 256;

/// The result of reading a DWG: the DXF rendition plus what it lacks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DwgRead {
    /// ASCII DXF rendition of the records that were read.
    pub dxf: Vec<u8>,
    /// Human-readable notes (presentation only; never a source of safety decisions).
    pub notes: Vec<String>,
    /// File version code, e.g. `AC1032`.
    pub version: String,
    /// Detailed issues, at most [`MAX_BRIDGE_ISSUES`] entries.
    pub issues: Vec<BridgeIssue>,
    /// Every confirmed lost record, including those beyond the detail cap (saturating).
    pub issue_count: u64,
    /// Other programs' records kept beside the DXF for a save in the same DWG version: these
    /// are listed in `issues` too (the DXF lacks them), but a same-version DWG save puts them back.
    pub carry: Option<std::sync::Arc<super::NativeCarry>>,
}

impl DwgRead {
    /// True when the DXF rendition lacks content of the DWG.
    pub fn has_loss(&self) -> bool {
        self.issue_count != 0
    }
}

/// CLASS identity of a custom record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BridgeClass {
    pub number: Option<i16>,
    pub dxf_name: String,
    pub cpp_name: String,
    pub application_name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BridgeIssueKind {
    Entity,
    Object,
    Metadata,
    /// A decoder skip whose entity/non-entity category is unknown.
    Record,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BridgeIssueReason {
    /// A custom-class record the DXF conversion omits.
    OpaqueCustomRecord,
    /// A record the failsafe decoder could not decode.
    DecoderSkippedRecord,
    /// A record this bridge deliberately left out of the conversion.
    PrunedRecord,
    /// XDATA on a BLOCK marker entity, which the DXF writer omits.
    BlockMarkerXdata,
}

/// One confirmed loss, or a counted batch of them (`count`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BridgeIssue {
    pub reason: BridgeIssueReason,
    pub kind: BridgeIssueKind,
    pub handle: Option<u64>,
    pub record_name: Option<String>,
    pub class: Option<BridgeClass>,
    pub count: u64,
}

impl BridgeIssue {
    /// A short description for error messages.
    pub fn describe(&self) -> String {
        let what = match self.reason {
            BridgeIssueReason::OpaqueCustomRecord => "custom-class record",
            BridgeIssueReason::DecoderSkippedRecord => "record the reader skipped",
            BridgeIssueReason::PrunedRecord => "pruned record",
            BridgeIssueReason::BlockMarkerXdata => "BLOCK-marker XDATA",
        };
        let name = self.record_name.as_deref().map(|n| format!(" {n}")).unwrap_or_default();
        let handle = self.handle.map(|h| format!(" (handle {h:X})")).unwrap_or_default();
        format!("{what}{name}{handle}")
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn bounded(text: &str) -> String {
    let mut end = text.len().min(MAX_TEXT_BYTES);
    while !text.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    text.get(..end).unwrap_or_default().to_owned()
}

/// Gathers issues with a bounded detail list and an unbounded (saturating) count.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct Collector {
    issues: Vec<BridgeIssue>,
    count: u64,
}

#[cfg(not(target_arch = "wasm32"))]
impl Collector {
    pub(crate) fn new() -> Self {
        Self { issues: Vec::new(), count: 0 }
    }

    pub(crate) fn finish(self) -> (Vec<BridgeIssue>, u64) {
        (self.issues, self.count)
    }

    fn push(&mut self, mut issue: BridgeIssue) {
        if issue.count == 0 {
            return;
        }
        self.count = self.count.saturating_add(issue.count);
        if self.issues.len() < MAX_BRIDGE_ISSUES {
            issue.record_name = issue.record_name.as_deref().map(bounded);
            if let Some(c) = &mut issue.class {
                c.dxf_name = bounded(&c.dxf_name);
                c.cpp_name = bounded(&c.cpp_name);
                c.application_name = bounded(&c.application_name);
            }
            self.issues.push(issue);
        }
    }

    /// Records the bridge pruned from the conversion (DGN line style components no definition uses).
    pub(crate) fn pruned(&mut self, count: usize) {
        self.push(BridgeIssue {
            reason: BridgeIssueReason::PrunedRecord,
            kind: BridgeIssueKind::Object,
            handle: None,
            record_name: Some("DGN line style component".into()),
            class: None,
            count: count as u64,
        });
    }

    /// Records the failsafe decoder skipped. Per-record diagnostics are capped by the decoder, so
    /// the total comes from its skip count and any records beyond the diagnostics are a batch.
    pub(crate) fn decoder_skips(&mut self, stats: &acadrust::ReadStats, label: fn(Option<&str>) -> String) {
        let skipped = stats.skipped_source_records as u64;
        let mut seen = 0u64;
        for d in stats.diagnostics.iter().filter(|d| d.record_handle.is_some()) {
            if seen >= skipped {
                break;
            }
            seen += 1;
            self.push(BridgeIssue {
                reason: BridgeIssueReason::DecoderSkippedRecord,
                kind: BridgeIssueKind::Record,
                handle: d.record_handle,
                record_name: Some(label(d.record_type.as_deref())),
                class: None,
                count: 1,
            });
        }
        self.push(BridgeIssue {
            reason: BridgeIssueReason::DecoderSkippedRecord,
            kind: BridgeIssueKind::Record,
            handle: None,
            record_name: None,
            class: None,
            count: skipped - seen,
        });
    }

    /// The native inventories, taken before any conversion or pruning: unknown entities and
    /// non-entity objects with their CLASS identity, and BLOCK-marker XDATA.
    pub(crate) fn native_document(&mut self, doc: &acadrust::CadDocument) {
        use acadrust::entities::EntityType;
        use acadrust::objects::ObjectType;
        let class_of = |number: Option<i16>, name: &str| {
            let found = doc.classes.iter().find(|c| number.is_some_and(|n| n == c.class_number)).or_else(|| doc.classes.get_by_name(name));
            found.map(|c| BridgeClass {
                number: Some(c.class_number),
                dxf_name: c.dxf_name.clone(),
                cpp_name: c.cpp_class_name.clone(),
                application_name: c.application_name.clone(),
            })
        };
        for e in doc.entities() {
            if let EntityType::Unknown(u) = e {
                self.push(BridgeIssue {
                    reason: BridgeIssueReason::OpaqueCustomRecord,
                    kind: BridgeIssueKind::Entity,
                    handle: Some(u.common.handle.value()),
                    record_name: Some(u.dxf_name.clone()),
                    class: class_of(Some(u.dwg_type_code).filter(|n| *n != 0), &u.dxf_name),
                    count: 1,
                });
            }
        }
        // The object map is unordered: report in handle order so the details are deterministic.
        let mut objects: Vec<(u64, &str)> = doc
            .objects
            .iter()
            .filter_map(|(h, o)| match o {
                ObjectType::Unknown { type_name, .. } => Some((h.value(), type_name.as_str())),
                _ => None,
            })
            .collect();
        objects.sort_unstable();
        for (handle, name) in objects {
            self.push(BridgeIssue {
                reason: BridgeIssueReason::OpaqueCustomRecord,
                kind: BridgeIssueKind::Object,
                handle: Some(handle),
                record_name: Some(name.to_string()),
                class: class_of(None, name),
                count: 1,
            });
        }
        // `entities()` hides BLOCK/ENDBLK, so the markers are resolved through the block records.
        for br in doc.block_records.iter() {
            let Some(EntityType::Block(b)) = doc.get_entity(br.block_entity_handle) else { continue };
            if !b.common.extended_data.records().is_empty() || !b.common.extended_data.raw_dwg_eed.is_empty() {
                self.push(BridgeIssue {
                    reason: BridgeIssueReason::BlockMarkerXdata,
                    kind: BridgeIssueKind::Metadata,
                    handle: Some(br.block_entity_handle.value()),
                    record_name: Some(br.name.clone()),
                    class: None,
                    count: 1,
                });
            }
        }
    }
}
