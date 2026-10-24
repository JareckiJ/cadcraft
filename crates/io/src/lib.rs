//! CADCraft file formats: DXF/DWG read, SVG and SVGZ import, SVG/PNG/PDF export (plotting).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

/// What a DXF file holds that CADCraft doesn't model, kept and written back (#528).
mod dxf_raw;

mod dxf_ext;
mod dxf_image;
mod dxf_mleader;
mod dxf_read;
mod dxf_table;
mod dxf_write;
mod dxf_xref;
pub mod pdf;
pub mod plotter;
mod preservation;
pub mod svg;
mod svg_read;

/// Multilines and their styles (MLINE, MLINESTYLE).
mod dxf_mline;

use std::collections::HashSet;

use cadcraft_doc::{Drawing, Space};
use cadcraft_geom::Bounds2;

pub use cadcraft_dxf::Version;
pub use dxf_read::entity_from_groups;
pub use dxf_write::entity_groups;
pub use pdf::{PdfOptions, pdf, plot};
pub use preservation::{LossReport, LostRecord, RawDwg};

/// Solid-modeler entity types whose ACIS payload the data store can carry from AutoCAD 2013 on.
pub(crate) const MODELER_TYPES: &[&str] = &["3DSOLID", "REGION", "BODY"];

/// DXF names of foreign CLASS records whose C++ name is not an Autodesk `AcDb*` type: those are
/// custom classes of other programs. CADCraft keeps them in DXF; a DWG save cannot.
pub(crate) fn custom_class_names(d: &Drawing) -> HashSet<String> {
    let mut out = HashSet::new();
    for class in &d.foreign.classes {
        let dxf = class.iter().find(|t| t.code == 1).map(|t| t.value.trim());
        let cpp = class.iter().find(|t| t.code == 2).map(|t| t.value.trim()).unwrap_or("");
        let Some(dxf) = dxf.filter(|n| !n.is_empty()) else { continue };
        if !cpp.to_ascii_lowercase().starts_with("acdb") {
            out.insert(dxf.to_ascii_uppercase());
        }
    }
    out
}

/// How a drawing is saved as DXF or DWG.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SaveOptions {
    /// The file version; `None` saves in the drawing's own version ([`save_version`]).
    pub version: Option<Version>,
    /// Binary instead of ASCII DXF (DXF only; DWG is always binary).
    pub binary: bool,
}

/// The version a drawing is saved in when none is asked for: the version of the file it was
/// read from (`$ACADVER`, the nearest one CADCraft writes), and 2018 for a new drawing.
pub fn save_version(d: &Drawing) -> Version {
    d.header.get("ACADVER").and_then(cadcraft_doc::HVal::as_str).and_then(Version::for_acadver).unwrap_or_default()
}

#[derive(Debug, thiserror::Error)]
pub enum IoError {
    #[error("{0}")]
    Format(String),
    #[error("unsupported file type `{0}`")]
    Unsupported(String),
}

pub type Result<T> = std::result::Result<T, IoError>;

fn ext(name: &str) -> String {
    std::path::Path::new(name).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

/// Read a drawing; the format comes from the content (DXF ASCII/binary, DWG) or the name
/// (SVG and gzip-compressed SVGZ).
pub fn read(bytes: &[u8], name: &str) -> Result<Drawing> {
    if cadcraft_dwg::is_dwg(bytes) {
        let read = cadcraft_dwg::dwg_to_dxf_detailed(bytes, cadcraft_dwg::Limits::DEFAULT).map_err(IoError::Format)?;
        let mut d = dxf_read::read(&read.dxf).map_err(|e| IoError::Format(format!("{e} (in the DXF converted from this DWG)")))?;
        d.read_notes = read.notes.clone();
        let state = preservation::state_from_bridge(&read);
        if let Some(w) = preservation::warning(&state, read.carry.as_ref().map_or(0, |c| c.handles().len())) {
            d.read_notes.push(w);
        }
        d.preservation = std::sync::Arc::new(state);
        return Ok(d);
    }
    match ext(name).as_str() {
        "svg" | "svgz" => svg_read::read(bytes),
        "dxf" | "" => dxf_read::read(bytes),
        e if bytes.len() > 4 => dxf_read::read(bytes).map_err(|_| IoError::Unsupported(e.to_string())),
        e => Err(IoError::Unsupported(e.to_string())),
    }
}

/// Write a drawing in the format chosen by the name's extension. Images (PNG, SVG, PDF) show
/// model space fitted to its extents. DXF and DWG are written in the drawing's version
/// ([`save_version`]), DXF as ASCII.
pub fn write(d: &Drawing, name: &str) -> Result<Vec<u8>> {
    write_framed(d, name, None)
}

/// Like [`write`], with the DXF/DWG version and binary DXF chosen by `opts`. Binary applies to
/// DXF only; asking for it with another format is an error. What the file lacks of the drawing's
/// source isn't reported here: callers that save files use [`write_with_report`].
pub fn write_with(d: &Drawing, name: &str, opts: &SaveOptions) -> Result<Vec<u8>> {
    write_with_report(d, name, opts).map(|(bytes, _)| bytes)
}

/// [`write_with`], also returning what the file lacks of the drawing's source (`None`: nothing):
/// records the import couldn't carry into the editable drawing, other programs' records the
/// format can't hold, BLOCK-marker XDATA a DWG can't hold. Nothing is refused for them; a DWG in
/// the source's own version puts back the records carried from it.
pub fn write_with_report(d: &Drawing, name: &str, opts: &SaveOptions) -> Result<(Vec<u8>, Option<LossReport>)> {
    let version = opts.version.unwrap_or_else(|| save_version(d));
    match ext(name).as_str() {
        "dxf" | "" => {
            let tags = dxf_write::tags(d, version);
            let bytes = if opts.binary {
                cadcraft_dxf::write_binary(&tags, version.utf8())
            } else {
                cadcraft_dxf::write_ascii_as(&tags, version.utf8()).into_bytes()
            };
            let target = if opts.binary { "binary DXF" } else { "DXF" };
            let (records, more) = preservation::import_losses(d, &HashSet::new(), &|_| {
                "custom record of another program: a DXF file can't hold its DWG data".to_string()
            });
            Ok((bytes, preservation::report(d, format!("{target} {}", version.code()), 0, records, more)))
        }
        // The DWG bridge reads our R2000 DXF and writes the DWG at the version asked for. Without
        // one, at least 2004: R2000 DWG has no true colours, transparency, table styles or
        // gradients, so a drawing from an R2000 DXF doesn't lose them by default. A drawing
        // template (.dwt) is DWG content.
        "dwg" | "dwt" if !opts.binary => {
            let version = opts.version.unwrap_or_else(|| version.max(Version::R2004));
            let carry = preservation::carry(d).filter(|c| c.version().eq_ignore_ascii_case(version.code()));
            let (dxf, block_xdata) = dxf_write::dwg_bridge(d, version);
            let w = cadcraft_dwg::dxf_to_dwg_carrying(dxf.as_bytes(), version.code(), carry).map_err(IoError::Format)?;
            let not = preservation::not_carried_reasons(&w.not_carried);
            let kept: HashSet<u64> = carry.map(|c| c.handles().into_iter().filter(|h| !not.contains_key(h)).collect()).unwrap_or_default();
            let source = preservation::carry(d).map(|c| c.version().to_string()).unwrap_or_default();
            let why = |h: u64| {
                not.get(&h)
                    .cloned()
                    .unwrap_or_else(|| format!("custom record of another program: kept only for a DWG {source} save, this one is {}", version.code()))
            };
            let (mut records, more) = preservation::import_losses(d, &kept, &why);
            records.extend(preservation::dxf_customs_lost(d, version));
            records.extend(preservation::block_xdata_lost(&block_xdata, version));
            Ok((w.bytes, preservation::report(d, format!("DWG {}", version.code()), w.kept, records, more)))
        }
        e if opts.binary => Err(IoError::Format(format!("binary output is for DXF files, not `{e}`"))),
        _ => write_framed(d, name, None).map(|b| (b, None)),
    }
}

/// Like [`write`], but image formats (PNG, SVG, PDF) show `window` (a model-space rectangle in
/// drawing units, centred and scaled to fit the image without distortion) instead of the
/// extents. Formats that are not images (DXF, DWG) ignore the window. The window is checked with
/// [`check_window`].
pub fn write_framed(d: &Drawing, name: &str, window: Option<Bounds2>) -> Result<Vec<u8>> {
    write_framed_report(d, name, window).map(|(bytes, _)| bytes)
}

/// [`write_framed`], also returning what a DXF, DWG or DWT file lacks of the drawing's source
/// (see [`write_with_report`]); images report nothing.
pub fn write_framed_report(d: &Drawing, name: &str, window: Option<Bounds2>) -> Result<(Vec<u8>, Option<LossReport>)> {
    // A drawing template (.dwt) is DWG content.
    if ext(name) == "dwt" {
        return write_with_report(d, &format!("{name}.dwg"), &SaveOptions::default());
    }
    if let Some(w) = &window
        && is_image(name)
    {
        check_window(w)?;
    }
    match ext(name).as_str() {
        "dxf" | "" | "dwg" => write_with_report(d, name, &SaveOptions::default()),
        "svg" => Ok((svg::export_window(d, &Space::Model, window).into_bytes(), None)),
        "png" => png_framed(d, &Space::Model, 2400, 1600, window).map(|b| (b, None)),
        "pdf" => pdf::pdf(d, &Space::Model, &PdfOptions { compress: true, window, ..PdfOptions::default() }).map(|b| (b, None)),
        e => Err(IoError::Unsupported(e.to_string())),
    }
}

/// What saving a file loses, found without writing anything: the file read, saved in memory as
/// `target` (an extension: `dwg`, `dxf`; `opts` for version and binary), its [`LossReport`], and
/// how the saved file's records differ from the file's, kind by kind (read back with acadrust: an
/// independent count that also shows losses the report doesn't know of).
#[derive(Clone, Debug, PartialEq)]
pub struct Audit {
    pub source: String,
    pub target: String,
    pub report: Option<LossReport>,
    /// (kind, in the file, in the saved file) for each kind whose count differs.
    pub differences: Vec<(String, u64, u64)>,
    /// Why the counts couldn't be compared, when they couldn't.
    pub inventory_error: Option<String>,
}

impl Audit {
    pub fn to_json(&self) -> String {
        let report: serde_json::Value = self.report.as_ref().map(|r| serde_json::from_str(&r.to_json()).unwrap_or_default()).unwrap_or_default();
        let diffs: Vec<serde_json::Value> =
            self.differences.iter().map(|(k, a, b)| serde_json::json!({ "kind": k, "file": a, "saved": b })).collect();
        serde_json::to_string_pretty(&serde_json::json!({
            "source": self.source,
            "target": self.target,
            "kept_natively": self.report.as_ref().map_or(0, |r| r.kept_natively),
            "lost": self.report.as_ref().map_or(0, LossReport::count),
            "report": report,
            "differences": diffs,
            "inventory_error": self.inventory_error,
        }))
        .unwrap_or_default()
    }
}

/// Audit saving `bytes` (named `name`) as `target` (see [`Audit`]).
pub fn audit(bytes: &[u8], name: &str, target: &str, opts: &SaveOptions) -> Result<Audit> {
    let d = read(bytes, name)?;
    let out_name = format!("audit.{target}");
    let (out, report) = write_with_report(&d, &out_name, opts)?;
    let source = match cadcraft_dwg::version(bytes) {
        Some(v) => format!("DWG {v}"),
        None => format!("DXF {}", save_version(&d).code()),
    };
    let target_label = match ext(&out_name).as_str() {
        "dwg" => format!("DWG {}", opts.version.unwrap_or_else(|| save_version(&d).max(Version::R2004)).code()),
        _ => format!("{}DXF {}", if opts.binary { "binary " } else { "" }, opts.version.unwrap_or_else(|| save_version(&d)).code()),
    };
    let (differences, inventory_error) = match (cadcraft_dwg::inventory(bytes), cadcraft_dwg::inventory(&out)) {
        (Ok(a), Ok(b)) => {
            let keys: std::collections::BTreeSet<&String> = a.keys().chain(b.keys()).collect();
            let diffs = keys
                .into_iter()
                .filter_map(|k| {
                    let (x, y) = (a.get(k).copied().unwrap_or(0), b.get(k).copied().unwrap_or(0));
                    (x != y).then(|| (k.clone(), x, y))
                })
                .collect();
            (diffs, None)
        }
        (Err(e), _) | (_, Err(e)) => (Vec::new(), Some(e)),
    };
    Ok(Audit { source, target: target_label, report, differences, inventory_error })
}

/// Whether `name` is written as an image (PNG, SVG, PDF), i.e. whether framing applies to it.
pub fn is_image(name: &str) -> bool {
    matches!(ext(name).as_str(), "png" | "svg" | "pdf")
}

/// Largest coordinate magnitude accepted in an export window.
pub const MAX_WINDOW_COORD: f64 = 1.0e12;

/// Validate an export window: finite corners within ±[`MAX_WINDOW_COORD`], and a width and height
/// that are positive and not vanishingly small next to the coordinates (at least 1e-9 of the
/// largest coordinate magnitude, and at least 1e-9 drawing units).
pub fn check_window(w: &Bounds2) -> Result<()> {
    let coords = [w.min.x, w.min.y, w.max.x, w.max.y];
    if coords.iter().any(|c| !c.is_finite()) {
        return Err(IoError::Format("export window: coordinates must be finite numbers".into()));
    }
    let mag = coords.iter().fold(0.0_f64, |m, c| m.max(c.abs()));
    if mag > MAX_WINDOW_COORD {
        return Err(IoError::Format(format!("export window: coordinates must lie within ±{MAX_WINDOW_COORD:e}")));
    }
    let min_side = mag.max(1.0) * 1e-9;
    let (width, height) = (w.max.x - w.min.x, w.max.y - w.min.y);
    if !(width >= min_side && height >= min_side) {
        return Err(IoError::Format(format!("export window: width and height must be positive (got {width} x {height})")));
    }
    Ok(())
}

/// Render a space to PNG, fitted to its extents.
pub fn png(d: &Drawing, space: &Space, width: u32, height: u32) -> Result<Vec<u8>> {
    png_framed(d, space, width, height, None)
}

/// Render a space to PNG showing `window` (centred, fitted without margin), or fitted to its
/// extents (5% margin) when `window` is `None`. Objects are drawn with their transparency, as on
/// screen.
pub fn png_framed(d: &Drawing, space: &Space, width: u32, height: u32, window: Option<Bounds2>) -> Result<Vec<u8>> {
    let list = cadcraft_render::build(d, space, &cadcraft_render::Options { transparency: true, ..cadcraft_render::Options::default() });
    let view = png_view(&list.bounds, width, height, window);
    cadcraft_render::raster::render_png(&list, &view, &cadcraft_render::raster::RasterOptions::default())
        .ok_or_else(|| IoError::Format("render failed".into()))
}

/// The raster view a PNG export uses.
pub fn png_view(extents: &Bounds2, width: u32, height: u32, window: Option<Bounds2>) -> cadcraft_render::raster::View {
    match window {
        Some(w) => cadcraft_render::raster::View::fit(&w, width, height, 0.0),
        None => cadcraft_render::raster::View::fit(extents, width, height, 0.05),
    }
}

pub fn read_dxf(bytes: &[u8]) -> Result<Drawing> {
    dxf_read::read(bytes)
}
/// The drawing as R2000 ASCII DXF, whatever its own version ([`write_with`] chooses one). What
/// it lacks of the drawing's source is reported by [`write_with_report`].
pub fn write_dxf(d: &Drawing) -> String {
    dxf_write::write(d)
}

/// Regions, written as closed polylines.
mod dxf_region;

#[cfg(test)]
mod tests;
