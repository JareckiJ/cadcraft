//! Drawing history — the OMNI toolkit, native.
//!
//! A *version* is a full copy of the drawing written into `_OMNI_HISTORY` next to it, named
//! `<date>.<time>_<milliseconds>_<user>_<description>_<file>.<ext>`. Versions arrive two ways:
//!
//! - **A save that changes the drawing leaves one behind**, without being asked.
//!   `SAVE`/`QSAVE`/`SAVEAS` write one when the drawing differs from what its file held (the
//!   unsaved-changes flag is the test), so a history is a timeline of the saves the user already
//!   makes. Those are filed under the description `save`. A save of an unchanged drawing adds
//!   nothing.
//! - **`OMNI-SNAP` takes a named milestone** of the drawing as it is right now — unsaved changes
//!   included, the file on disk untouched — for a state the user wants to name and come back to.
//!
//! `OMNI-LOG` lists the versions and opens one, `OMNI-DIFF` lays one under the current drawing as
//! an overlay reference recoloured to a single colour (red by default) and can also *report* what
//! changed between the two as numbers instead of pixels, `OMNI-TOGGLE` hides and shows that
//! overlay, `OMNI-CLEAR` detaches it again, `OMNI-RESTORE` puts a version back into the drawing
//! (undoably — the file on disk is never touched, and the document becomes the version's whole,
//! layers and settings included), and `OMNI-NOTE` drops a marker and a comment on the drawing.
//! `OMNI-VERSIONS` is the listing the Drawing History palette draws.
//!
//! A version is history, not a drawing of its own: opening one reads it, and a save onto it is
//! refused (see [`is_version_path`]) — `SAVEAS` is how an opened revision gets a name and a history
//! of its own.
//!
//! Versions are gzipped on disk (`…_plan.dxf.gz`): a DXF is verbose text and shrinks several times
//! over (about seven on the sample plan), so a long history does not cost what a folder of plain
//! copies would. A plain
//! `.dxf`/`.dwg` left by an older build is still listed, read and compared; an older version is
//! unpacked into `_OMNI_HISTORY/.cache` only when it has to be a file on disk (the overlay
//! reference reads it back through the io hooks).
//!
//! The AutoLISP original this comes from only ever ran inside AutoCAD on DWG files and shelled
//! out to its `-XREF`/`-LAYER` command line. Here everything is native and format-agnostic: a
//! snapshot is written through the same io hooks as a save, so a DXF history is DXF and a DWG
//! history is DWG, and the overlay is read back through the same hooks. Nothing is
//! platform-specific — paths are built with [`std::path`], a snapshot reference is stored
//! relative to the drawing's folder with `/` (every platform CADCraft builds for accepts that),
//! the user name comes from the environment (`CADCRAFT_USER`, `USER`, `USERNAME`, `LOGNAME`), and
//! the stamp is UTC, so one history folder stays sortable and comparable on Windows, Linux, macOS
//! and FreeBSD alike. The commands that need a file system report "not available on the web" in
//! the browser build; the ones that only touch the drawing (TOGGLE, CLEAR, NOTE) work there too.
//!
//! In the browser build the helpers the file-system commands use (reading a folder, naming and
//! parsing a snapshot) are compiled but unreachable, which the lint below says out loud rather
//! than hiding: the host builds keep every warning.
#![cfg_attr(target_arch = "wasm32", allow(dead_code))]

#[cfg(not(target_arch = "wasm32"))]
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cadcraft_color::Color;
use cadcraft_doc::{Common, Drawing, EntityKind, XrefStatus};
use cadcraft_geom::Vec2;
#[cfg(not(target_arch = "wasm32"))]
use flate2::Compression;
#[cfg(not(target_arch = "wasm32"))]
use flate2::read::GzDecoder;
#[cfg(not(target_arch = "wasm32"))]
use flate2::write::GzEncoder;
use serde_json::{Value, json};

use super::super::blocks::xref::load::has_prefix;
use super::super::helpers::circle;
use super::super::{CommandSpec, bad, point_req, size_param, str_param};
#[cfg(not(target_arch = "wasm32"))]
use super::{SaveAs, compare, io, store, write_bytes};
use crate::cmd::draw::make_text;
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

/// The history folder created next to a drawing (the AutoLISP toolkit's folder name, so an
/// existing history keeps working).
const HISTORY_DIR: &str = "_OMNI_HISTORY";
/// Where versions are unpacked when something needs them as a file (the overlay reference) —
/// inside the history folder, so it travels with it and never pollutes the drawing's own folder.
#[cfg(not(target_arch = "wasm32"))]
const CACHE_DIR: &str = ".cache";
/// Versions are stored gzipped; the suffix says so.
const COMPRESSED_EXT: &str = "gz";
/// The description an automatic version — the one a save leaves behind — is filed under.
const SAVE_DESCRIPTION: &str = "save";
/// The largest a single version may unpack to: a history file is input, and input is hostile
/// (a gzip bomb costs memory before anything else can refuse it).
#[cfg(not(target_arch = "wasm32"))]
const MAX_UNPACKED: u64 = 512 * 1024 * 1024;
/// Overlay references are named with this prefix, so OMNI-CLEAR and OMNI-TOGGLE touch only their
/// own and leave the user's external references alone.
const OVERLAY_PREFIX: &str = "OMNI_";
/// The layer OMNI-NOTE's markers go on.
const NOTE_LAYER: &str = "OMNI_NOTES";
/// OMNI-NOTE's defaults, in drawing units (`radius` and `height` override them).
const NOTE_RADIUS: f64 = 50.0;
const NOTE_HEIGHT: f64 = 25.0;
/// A note's text is at most this long, as AutoCAD's TEXT.
const MAX_TEXT: usize = 255;
/// Characters of one name part (a description, a user name) kept in a file name.
const MAX_PART: usize = 48;
/// Versions read from a folder in one go; a folder with more says so.
const MAX_SNAPSHOTS: usize = 2000;
/// Snapshot files are DXF and DWG drawings, never anything else that happens to be in the folder.
const DRAWING_EXTS: [&str; 2] = ["dxf", "dwg"];
/// What the file-system commands reply in a build without one (the browser). Only the browser
/// build has the commands that need it: everywhere else they are the real thing.
#[cfg(target_arch = "wasm32")]
const NO_FILES: &str = "the drawing history needs a file system: it is not available on the web";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("omni-snap", "Save Snapshot", run_snap)
            .menu(&["Tools", "Drawing History", "Save Snapshot"])
            .alias(&["omnisnap"])
            .params("{description? (default \"snapshot\"; folded into the file name, where anything but letters, digits and `-` becomes `-`), path? (the DXF/DWG file this history belongs to)} → {file, path, folder, bytes, size, time, user, description, snapshots}")
            .noundo()
            .interactive(|_| Ok(Box::new(SnapM))),
        CommandSpec::new("omni-log", "Snapshot History...", run_log)
            .menu(&["Tools", "Drawing History", "Snapshot History..."])
            .alias(&["omnilog", "omni", "versions", "history"])
            .params("{path?, open? (a listed number, a file name or a path), limit?} → {folder, count, capped, snapshots: [{n, file, path, time, user, description, kind, drawing, bytes, compressed}], opened?}")
            .noundo()
            .interactive(|_| Ok(Box::new(LogM))),
        CommandSpec::new("omni-versions", "Version History", run_versions)
            .params("{path?, limit? (default 200), kind?: save|milestone|all (default all)} → {folder, count, capped, versions: [{n, file, path, time, user, description, kind, drawing, bytes, compressed}]}: what the Drawing History palette lists, without echoing it to the command line")
            .noundo(),
        CommandSpec::new("omni-restore", "Restore Version", run_restore)
            .params("{snapshot: a listed number or a file name, path?} → {restored, description, kind, time, versions}: put that version back into the drawing as an unsaved change (undoable; the file on disk is untouched, so nothing is lost by looking)")
            .noundo(),
        CommandSpec::new("omni-diff", "Compare with Snapshot", run_diff)
            .menu(&["Tools", "Drawing History", "Compare with Snapshot"])
            .alias(&["omnidiff"])
            .params(
                "{snapshot? (1 = the newest, a file name or a path; default 1), mode?: overlay|report|both (default overlay), \
                 color?: ACI 1-255 (default 1 = red), path?} → {overlay?, file, layers, objects, counts?, regions?, truncated?, message}",
            )
            .noundo()
            .interactive(|_| Ok(Box::new(DiffM))),
        CommandSpec::new("omni-toggle", "Show/Hide Snapshot", run_toggle)
            .menu(&["Tools", "Drawing History", "Show/Hide Snapshot"])
            .alias(&["omnitoggle"])
            .params("{show?: bool (default: flip), snapshot? (the overlay's name or the snapshot's file name)} → {overlay: [names], shown, objects}")
            .noundo(),
        CommandSpec::new("omni-clear", "Clear Snapshot Overlay", run_clear)
            .menu(&["Tools", "Drawing History", "Clear Snapshot Overlay"])
            .alias(&["omniclear"])
            .params("{} → {cleared: [names], erased}: detaches the overlays OMNI-DIFF attached, and nothing else")
            .noundo(),
        CommandSpec::new("omni-note", "Add Note", run_note)
            .menu(&["Tools", "Drawing History", "Add Note"])
            .alias(&["omninote"])
            .params("{at: [x,y], text, height? (default 25), radius? (default 50), layer? (default OMNI_NOTES)} → {handle, textHandle, layer}")
            .interactive(|_| Ok(Box::new(NoteM::default()))),
    ]
}

// ---------------------------------------------------------------- the history folder

/// The folder a drawing's snapshots live in.
fn history_dir(folder: &Path) -> PathBuf {
    folder.join(HISTORY_DIR)
}

/// Whether a path is a version inside a history folder (`_OMNI_HISTORY`, or its `.cache`). History
/// is the record of what a drawing's saves held, so nothing may be saved onto one: an opened
/// revision is a file to read, and `SAVEAS` is how it becomes a drawing with a name of its own.
pub(crate) fn is_version_path(path: &str) -> bool {
    Path::new(path).components().any(|c| matches!(c, std::path::Component::Normal(n) if n.to_string_lossy().eq_ignore_ascii_case(HISTORY_DIR)))
}

/// The folder a file is in (`.` when it names no folder), so a bare file name works everywhere.
fn folder_of(p: &Path) -> PathBuf {
    match p.parent() {
        Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

/// The lowercase extension of a path (empty without one).
fn ext_of(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

/// A file name split into its stem and its extension.
fn split_ext(file: &str) -> (&str, &str) {
    match file.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, ext),
        _ => (file, ""),
    }
}

/// The user a snapshot is stamped with: the environment, so the same code works on every
/// platform CADCraft builds for (Windows sets `USERNAME`, everything else `USER` or `LOGNAME`).
#[cfg(not(target_arch = "wasm32"))]
fn user() -> String {
    for k in ["CADCRAFT_USER", "USER", "USERNAME", "LOGNAME"] {
        if let Some(v) = std::env::var_os(k) {
            let s = v.to_string_lossy().trim().to_string();
            if !s.is_empty() {
                return s;
            }
        }
    }
    "unknown".to_string()
}
#[cfg(target_arch = "wasm32")]
fn user() -> String {
    "web".to_string()
}

/// The user part of a version's name. `_` separates the name's fields, so it can never appear
/// inside one: a name from an `_`-carrying account (`john_doe`) would otherwise read back with the
/// user `john` and the description `doe`, and every save of that account would list as a milestone.
fn user_part_of(me: &str) -> String {
    let s = slug(me, "-");
    if s.is_empty() { "unknown".to_string() } else { s }
}

/// The drawing a version file belongs to. A version lives in `<folder>/_OMNI_HISTORY` (or in its
/// `.cache`), so opening an old revision keeps browsing the timeline it came from instead of
/// offering the snapshot a history of its own inside the history folder.
#[cfg(not(target_arch = "wasm32"))]
fn history_host(file: &Path) -> Option<PathBuf> {
    let name = file.file_name()?.to_string_lossy().into_owned();
    if !is_snapshot(&name) {
        return None;
    }
    let parent = file.parent()?;
    let history = match parent.file_name()?.to_string_lossy() {
        d if d.eq_ignore_ascii_case(HISTORY_DIR) => parent.to_path_buf(),
        d if d.eq_ignore_ascii_case(CACHE_DIR) => parent.parent()?.to_path_buf(),
        _ => return None,
    };
    let drawing = parse(&name, file.to_path_buf(), 0).drawing;
    (!drawing.is_empty()).then(|| history.parent().unwrap_or(parent).join(drawing))
}

/// The drawing file a history belongs to: the open drawing's own file, or `path` when it has none
/// yet. A file that is neither DXF nor DWG is refused rather than guessed at.
fn drawing_file(s: &Session, cmd: &str, p: &Value) -> Result<PathBuf> {
    let path = str_param(p, "path")
        .map(str::to_string)
        .or_else(|| s.state().ok().and_then(|st| st.path.clone()))
        .ok_or_else(|| bad(cmd, "the drawing has no file name yet: pass `path`, the DXF or DWG file this history belongs to"))?;
    let file = PathBuf::from(path.trim());
    let Some(name) = file.file_name().map(|n| n.to_string_lossy().into_owned()) else {
        return Err(bad(cmd, format!("`path` must name a file, not \"{}\"", path.trim())));
    };
    // The extension a version is *read* as: a stored one carries the compression suffix on top.
    let plain = name.strip_suffix(&format!(".{COMPRESSED_EXT}")).unwrap_or(&name);
    if !DRAWING_EXTS.contains(&ext_of(Path::new(plain)).as_str()) {
        return Err(bad(cmd, format!("\"{}\" is not a DXF or DWG drawing", file.display())));
    }
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(host) = history_host(&file) {
        return Ok(host);
    }
    Ok(file)
}

// ---------------------------------------------------------------- names and stamps

/// A name part that is safe in a file name and in a block name: letters and digits of any script
/// are kept (so a description in Russian survives), the characters in `keep` are kept, and every
/// other character — a space, a path separator, the name's own `_` — becomes `-`. Runs collapse
/// and the ends are trimmed, so one part never holds two separators in a row.
fn slug(text: &str, keep: &str) -> String {
    let mut out = String::new();
    let mut n = 0;
    for c in text.trim().chars() {
        let ok = c.is_alphanumeric() || keep.contains(c);
        if ok {
            out.push(c);
            n += 1;
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
            n += 1;
        }
        if n >= MAX_PART {
            break;
        }
    }
    out.trim_matches('-').to_string()
}

/// UTC time as (civil year, month, day, hour, minute, second) from Unix seconds — Hinnant's
/// algorithm, so a stamp needs no time zone database and reads the same on every platform.
fn civil(secs: i64) -> (i64, i64, i64, i64, i64, i64) {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d, rem / 3600, (rem % 3600) / 60, rem % 60)
}

/// [`civil`] backwards: days since the epoch of a civil UTC date.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

/// Now, as (a name stamp `YYYYMMDD.HHMMSS`, Unix seconds, milliseconds).
#[cfg(not(target_arch = "wasm32"))]
fn now() -> (String, i64, u32) {
    let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let secs = i64::try_from(d.as_secs()).unwrap_or(i64::MAX);
    let (y, mo, da, h, mi, sd) = civil(secs);
    (format!("{y:04}{mo:02}{da:02}.{h:02}{mi:02}{sd:02}"), secs, d.subsec_millis())
}

/// A point in time as ISO-8601 UTC with milliseconds.
fn iso(secs: i64, ms: u32) -> String {
    let (y, mo, da, h, mi, sd) = civil(secs);
    format!("{y:04}-{mo:02}-{da:02}T{h:02}:{mi:02}:{sd:02}.{ms:03}Z")
}

/// The Unix second a name stamp (`20260819.153012`) stands for.
fn stamp_epoch(stamp: &str) -> Option<i64> {
    let (d, t) = stamp.split_once('.')?;
    if d.len() != 8 || t.len() != 6 {
        return None;
    }
    if !d.bytes().all(|b| b.is_ascii_digit()) || !t.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n = |s: &str, r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, da) = (n(d, 0..4)?, n(d, 4..6)?, n(d, 6..8)?);
    let (h, mi, sd) = (n(t, 0..2)?, n(t, 2..4)?, n(t, 4..6)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&da) || h > 23 || mi > 59 || sd > 60 {
        return None;
    }
    Some(days_from_civil(y, mo, da) * 86_400 + h * 3600 + mi * 60 + sd)
}

/// One version on disk: what its name says about it, and where it is.
#[derive(Clone, Debug, PartialEq)]
struct Snap {
    file: String,
    path: PathBuf,
    /// The name without the compression suffix — what the drawing inside is called.
    inner: String,
    /// Stored gzipped.
    compressed: bool,
    /// When it was taken (UTC seconds), `None` when the name doesn't follow the pattern.
    epoch: Option<i64>,
    millis: u32,
    user: String,
    description: String,
    /// The drawing file it was taken from.
    drawing: String,
    /// On disk, so a compressed version compares as what it costs.
    bytes: u64,
}

impl Snap {
    /// What a call that takes no snapshot of its own uses: the newest.
    fn time(&self) -> Value {
        match self.epoch {
            Some(e) => json!(iso(e, self.millis)),
            None => Value::Null,
        }
    }
    /// An automatic version (one a save left behind) or a named milestone (OMNI-SNAP).
    fn kind(&self) -> &'static str {
        if self.description.eq_ignore_ascii_case(SAVE_DESCRIPTION) { "save" } else { "milestone" }
    }
    fn json(&self, n: usize) -> Value {
        json!({
            "n": n,
            "file": self.file,
            "path": self.path.to_string_lossy(),
            "time": self.time(),
            "user": self.user,
            "description": self.description,
            "kind": self.kind(),
            "drawing": self.drawing,
            "bytes": self.bytes,
            "compressed": self.compressed,
        })
    }
}

/// Read a version's metadata out of its file name. The description is one `_`-free field and the
/// drawing's own name is everything after it, so the drawing is recovered exactly however many
/// separators the name parts before it had; a file that doesn't follow the pattern is still
/// listed, with what its name didn't say left empty.
fn parse(file: &str, path: PathBuf, bytes: u64) -> Snap {
    let compressed = file.to_ascii_lowercase().ends_with(&format!(".{COMPRESSED_EXT}"));
    let inner = if compressed { &file[..file.len() - COMPRESSED_EXT.len() - 1] } else { file };
    let (stem, ext) = split_ext(inner);
    let f: Vec<&str> = stem.split('_').collect();
    let epoch = f.first().copied().and_then(stamp_epoch);
    let millis = f.get(1).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    let tail = f.get(4..).map(|t| t.join("_")).unwrap_or_default();
    let drawing = match (epoch.is_some(), tail.is_empty(), ext.is_empty()) {
        (true, false, false) => format!("{tail}.{ext}"),
        (true, false, true) => tail,
        _ => inner.to_string(),
    };
    Snap {
        file: file.to_string(),
        path,
        inner: inner.to_string(),
        compressed,
        epoch,
        millis,
        user: f.get(2).copied().unwrap_or_default().to_string(),
        description: f.get(3).copied().unwrap_or_default().to_string(),
        drawing,
        bytes,
    }
}

/// Whether a file name is a version's (a DXF or DWG drawing in the history folder, stored plain by
/// an older build or gzipped now), whatever case the extensions are written in.
fn is_snapshot(file: &str) -> bool {
    let lower = file.to_ascii_lowercase();
    let plain = lower.strip_suffix(&format!(".{COMPRESSED_EXT}")).unwrap_or(&lower);
    DRAWING_EXTS.contains(&split_ext(plain).1)
}

/// The snapshots in `dir`, newest first — the name starts with a zero-padded UTC stamp, so a plain
/// name sort is a chronological one. Returns whether the folder holds more than [`MAX_SNAPSHOTS`].
///
/// Every version in the folder is collected before the sort: `read_dir` hands its entries over in
/// no defined order, so capping the loop itself would keep an arbitrary [`MAX_SNAPSHOTS`] of them
/// and the "newest first" list, the `1` that OMNI-RESTORE and OMNI-DIFF mean by it, and the
/// duplicate check in [`version_of_save`] would all point at the wrong end of the timeline. The
/// names are cheap to hold — nothing is read until a version is asked for.
fn snapshots(dir: &Path) -> (Vec<Snap>, bool) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut out: Vec<Snap> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                let file = e.file_name().to_string_lossy().into_owned();
                if !is_snapshot(&file) {
                    continue;
                }
                let bytes = e.metadata().map(|m| m.len()).unwrap_or(0);
                out.push(parse(&file, dir.join(&file), bytes));
            }
        }
        out.sort_by(|a, b| b.file.cmp(&a.file));
        let capped = out.len() > MAX_SNAPSHOTS;
        out.truncate(MAX_SNAPSHOTS);
        (out, capped)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = dir;
        (Vec::new(), false)
    }
}

// ---------------------------------------------------------------- compression

/// A version's bytes, gzipped: a DXF is verbose text, so this is roughly a tenth of the size.
#[cfg(not(target_arch = "wasm32"))]
fn gzip(cmd: &str, bytes: &[u8]) -> Result<Vec<u8>> {
    let mut e = GzEncoder::new(Vec::new(), Compression::default());
    e.write_all(bytes).and_then(|()| e.finish()).map_err(|e| bad(cmd, format!("compressing the version: {e}")))
}

/// A version's bytes as they went in. Bounded: a version file is input, and input is hostile.
#[cfg(not(target_arch = "wasm32"))]
fn gunzip(cmd: &str, bytes: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut d = GzDecoder::new(bytes).take(MAX_UNPACKED);
    d.read_to_end(&mut out).map_err(|e| bad(cmd, format!("unpacking the version: {e}")))?;
    Ok(out)
}

/// The bytes of a version, unpacked when it is stored gzipped.
#[cfg(not(target_arch = "wasm32"))]
fn read_snap(snap: &Snap, cmd: &str) -> Result<Vec<u8>> {
    let raw = std::fs::read(&snap.path).map_err(|e| bad(cmd, format!("{}: {e}", snap.path.display())))?;
    if snap.compressed { gunzip(cmd, &raw) } else { Ok(raw) }
}

/// The version as a file on disk, unpacking it into the history's cache when it is stored
/// compressed — the overlay reference reads a snapshot back through the io hooks, which take a
/// path. Unpacked once and reused: the cache file is named after the version, so it never goes
/// stale, and it lives inside the history folder so it travels with it.
#[cfg(not(target_arch = "wasm32"))]
fn materialize(snap: &Snap, cmd: &str) -> Result<PathBuf> {
    if !snap.compressed {
        return Ok(snap.path.clone());
    }
    let dir = snap.path.parent().ok_or_else(|| bad(cmd, format!("{}: no folder", snap.path.display())))?.join(CACHE_DIR);
    let out = dir.join(&snap.inner);
    if out.is_file() {
        return Ok(out);
    }
    let bytes = read_snap(snap, cmd)?;
    std::fs::create_dir_all(&dir).map_err(|e| bad(cmd, format!("{}: {e}", dir.display())))?;
    store(&out.to_string_lossy(), &bytes)?;
    Ok(out)
}

/// The snapshot a value names: a number as listed (1 = the newest), or a file name or path.
fn chosen(list: &[Snap], v: &Value) -> Option<Snap> {
    match v {
        Value::Number(_) => v.as_u64().map(|i| i as usize).and_then(|i| i.checked_sub(1)).and_then(|i| list.get(i)).cloned(),
        Value::String(t) => {
            let t = t.trim();
            if let Ok(i) = t.parse::<usize>() {
                return chosen(list, &json!(i));
            }
            let name = t.rsplit(['/', '\\']).next().unwrap_or(t);
            list.iter().find(|s| s.file.eq_ignore_ascii_case(name) || s.path.to_string_lossy() == t).cloned()
        }
        _ => None,
    }
}

/// A size for the command line's listing.
fn size(n: u64) -> String {
    match n {
        n if n < 10_000 => format!("{n} B"),
        n if n < 10_000_000 => format!("{:.1} kB", n as f64 / 1000.0),
        n => format!("{:.1} MB", n as f64 / 1_000_000.0),
    }
}

/// The history folder, its snapshots and whether the folder holds more than [`MAX_SNAPSHOTS`].
fn history_of(s: &Session, cmd: &str, p: &Value) -> Result<(PathBuf, Vec<Snap>, bool)> {
    let file = drawing_file(s, cmd, p)?;
    let dir = history_dir(&folder_of(&file));
    let (list, capped) = snapshots(&dir);
    Ok((dir, list, capped))
}

// ---------------------------------------------------------------- the commands

/// OMNI-SNAP: write a snapshot of the drawing as it is now.
#[cfg(not(target_arch = "wasm32"))]
fn run_snap(s: &mut Session, p: &Value) -> Result<Value> {
    let file = drawing_file(s, "omni-snap", p)?;
    let dir = history_dir(&folder_of(&file));
    std::fs::create_dir_all(&dir).map_err(|e| bad("omni-snap", format!("{}: {e}", dir.display())))?;
    let (stamp, secs, ms) = now();
    let word = |t: Option<&str>, default: &str| {
        let s = slug(t.unwrap_or(default), "-");
        if s.is_empty() { default.to_string() } else { s }
    };
    let description = word(str_param(p, "description"), "snapshot");
    let me = user_part_of(&user());
    let drawing = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    // The stamp's milliseconds keep two snapshots taken in the same second apart.
    let name = format!("{stamp}_{ms:03}_{me}_{description}_{drawing}");
    let name = format!("{name}.{COMPRESSED_EXT}");
    let target = dir.join(&name);
    let target = target.to_string_lossy().into_owned();
    // The snapshot is the drawing as it is now, unsaved changes included: it goes through the
    // same writer a save uses, never through a copy of the file on disk (which would be older).
    let plain = dir.join(name.strip_suffix(&format!(".{COMPRESSED_EXT}")).unwrap_or(&name));
    let bytes = write_bytes(s, "omni-snap", &plain.to_string_lossy(), &SaveAs::default())?;
    let packed = gzip("omni-snap", &bytes)?;
    store(&target, &packed)?;
    let (list, _) = snapshots(&dir);
    s.echo(format!("[OMNI] Snapshot saved: {name} ({} in {HISTORY_DIR}).", list.len()));
    Ok(json!({
        "file": name,
        "path": target,
        "folder": dir.to_string_lossy(),
        "bytes": packed.len(),
        "size": bytes.len(),
        "compressed": true,
        "time": iso(secs, ms),
        "user": me,
        "description": description,
        "snapshots": list.len(),
    }))
}
#[cfg(target_arch = "wasm32")]
fn run_snap(_s: &mut Session, _p: &Value) -> Result<Value> {
    Err(bad("omni-snap", NO_FILES))
}

/// A save leaves a version behind. The save path calls this once the drawing's own file is
/// written; nothing here may fail the save — a history that cannot be written is a warning on the
/// command line, never a lost drawing.
///
/// Returns what was written (or `None` when the version would repeat the newest one).
#[cfg(not(target_arch = "wasm32"))]
pub fn after_save(s: &mut Session, path: &str, bytes: &[u8]) -> Option<Value> {
    let file = Path::new(path);
    if !DRAWING_EXTS.contains(&ext_of(file).as_str()) {
        return None;
    }
    match version_of_save(s, file, bytes) {
        Ok(v) => v,
        Err(msg) => {
            s.echo(format!("[OMNI] The save left no version: {msg}"));
            None
        }
    }
}
#[cfg(target_arch = "wasm32")]
pub fn after_save(_s: &mut Session, _path: &str, _bytes: &[u8]) -> Option<Value> {
    None
}

/// The version file a save writes, or `None` when it would be a copy of the newest.
#[cfg(not(target_arch = "wasm32"))]
fn version_of_save(s: &mut Session, file: &Path, bytes: &[u8]) -> std::result::Result<Option<Value>, String> {
    let dir = history_dir(&folder_of(file));
    let (list, _) = snapshots(&dir);
    let packed = gzip("omni-snap", bytes).map_err(|e| e.to_string())?;
    // An edit and its undo, or a restore of the newest version, come back to what the history
    // already holds: that is not a change the user made since the last save, so it leaves no
    // version behind. A compressed version is compared as it is stored (gzip is deterministic),
    // a plain one left by an older build as it is.
    if let Some(newest) = list.first() {
        let same = if newest.compressed {
            newest.bytes as usize == packed.len() && std::fs::read(&newest.path).is_ok_and(|b| b == packed)
        } else {
            read_snap(newest, "omni-snap").is_ok_and(|b| b == bytes)
        };
        if same {
            return Ok(None);
        }
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let (stamp, secs, ms) = now();
    let drawing = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let name = format!("{stamp}_{ms:03}_{}_{}_{}.{COMPRESSED_EXT}", user_part_of(&user()), SAVE_DESCRIPTION, drawing);
    let target = dir.join(&name);
    store(&target.to_string_lossy(), &packed).map_err(|e| e.to_string())?;
    s.echo(format!("[OMNI] Version saved: {name} ({} → {}).", size(bytes.len() as u64), size(packed.len() as u64)));
    Ok(Some(json!({
        "file": name,
        "path": target.to_string_lossy(),
        "folder": dir.to_string_lossy(),
        "time": iso(secs, ms),
        "bytes": packed.len(),
        "size": bytes.len(),
        "description": SAVE_DESCRIPTION,
        "kind": "save",
        "versions": snapshots(&dir).0.len(),
    })))
}

/// OMNI-VERSIONS: what the Drawing History palette lists, without the command-line echo and with
/// the kind filter the palette offers. A drawing that was never saved has no history — that is an
/// empty list rather than an error, so the palette still opens on an untitled drawing.
fn run_versions(s: &mut Session, p: &Value) -> Result<Value> {
    let kind = str_param(p, "kind").unwrap_or("all").trim().to_ascii_lowercase();
    if !["all", "save", "milestone"].contains(&kind.as_str()) {
        return Err(bad("omni-versions", format!("`kind` must be all, save or milestone, not \"{kind}\"")));
    }
    let limit = p.get("limit").and_then(Value::as_u64).map(|l| l as usize).unwrap_or(200).min(MAX_SNAPSHOTS);
    let (folder, list, capped) = match history_of(s, "omni-versions", p) {
        Ok(v) => v,
        Err(EngineError::BadParams { msg, .. }) if str_param(p, "path").is_none() => {
            return Ok(json!({ "folder": Value::Null, "count": 0, "capped": false, "versions": [], "note": msg }));
        }
        Err(e) => return Err(e),
    };
    let versions: Vec<Snap> = list.into_iter().filter(|x| kind == "all" || x.kind() == kind).collect();
    Ok(json!({
        "folder": folder.to_string_lossy(),
        "count": versions.len(),
        "capped": capped,
        "versions": versions.iter().take(limit).enumerate().map(|(i, x)| x.json(i + 1)).collect::<Vec<_>>(),
    }))
}

/// OMNI-RESTORE: put a version back into the drawing. The drawing's own file is not touched, the
/// step undoes like any other change, and the next save writes it and leaves a version of its own,
/// so looking at an old revision costs nothing: the timeline carries on from it.
#[cfg(not(target_arch = "wasm32"))]
fn run_restore(s: &mut Session, p: &Value) -> Result<Value> {
    let want = p
        .get("snapshot")
        .filter(|v| !v.is_null())
        .cloned()
        .ok_or_else(|| bad("omni-restore", "say which version to restore: `snapshot`, a listed number or a file name"))?;
    let (_dir, list, _) = history_of(s, "omni-restore", p)?;
    let snap = chosen(&list, &want).ok_or_else(|| bad("omni-restore", format!("no version {want}")))?;
    if s.state()?.block_edit.is_some() {
        return Err(bad("omni-restore", "close the Block Editor first: it is editing a block of this drawing"));
    }
    let hooks = io().ok_or_else(|| bad("omni-restore", "file formats are not available in this build"))?;
    let bytes = read_snap(&snap, "omni-restore")?;
    let restored = (hooks.read)(&bytes, &snap.inner).map_err(|e| bad("omni-restore", format!("{}: {e}", snap.file)))?;
    let st = s.state_mut()?;
    let before = crate::Snapshot { label: "restore version".to_string(), doc: st.drawing().clone(), selection: st.selection.clone() };
    st.doc = Arc::new(restored);
    st.selection.clear();
    // A layout the restored version doesn't have sends the view back to the model tab, and the
    // tiled viewports belong to the version that had them.
    if let cadcraft_doc::Space::Paper(name) = &st.space
        && !st.doc.layouts.iter().any(|l| &l.name == name)
    {
        st.space = cadcraft_doc::Space::Model;
    }
    st.mspace = None;
    st.tiles = Default::default();
    st.revision += 1;
    st.push_undo(before);
    s.echo(format!("[OMNI] Restored {}. The file on disk is untouched; UNDO goes back.", snap.file));
    Ok(json!({
        "restored": snap.file,
        "time": snap.time(),
        "description": snap.description,
        "kind": snap.kind(),
        "dirty": true,
        "versions": list.len(),
    }))
}
#[cfg(target_arch = "wasm32")]
fn run_restore(_s: &mut Session, _p: &Value) -> Result<Value> {
    Err(bad("omni-restore", NO_FILES))
}

/// The listing OMNI-LOG and OMNI-DIFF show.
fn echo_list(s: &mut Session, dir: &Path, list: &[Snap], limit: usize) {
    if list.is_empty() {
        s.echo(format!("[OMNI] No snapshots in {}. Take one with OMNI-SNAP.", dir.display()));
        return;
    }
    s.echo(format!("[OMNI] Snapshot history ({} in {}):", list.len(), dir.display()));
    for (i, x) in list.iter().take(limit).enumerate() {
        let when = match x.epoch {
            Some(e) => iso(e, x.millis),
            None => "-".to_string(),
        };
        s.echo(format!("[{:>3}] {}  {:>8}  {:<12} {}", i + 1, when, size(x.bytes), x.description.chars().take(12).collect::<String>(), x.file));
    }
}

/// Open a snapshot as a drawing of its own (OMNI-LOG). A version stored gzipped is opened from
/// the history's cache, where it is unpacked once; the history it belongs to stays the drawing's
/// own (`history_host`), so browsing the timeline carries on in the opened revision.
#[cfg(not(target_arch = "wasm32"))]
fn open_snapshot(s: &mut Session, snap: &Snap) -> Result<Value> {
    let file = materialize(snap, "omni-log")?;
    s.execute("open", &json!({ "path": file.to_string_lossy() }))?;
    s.echo(format!("[OMNI] Opened {}.", snap.file));
    Ok(json!(snap.file))
}
#[cfg(target_arch = "wasm32")]
fn open_snapshot(_s: &mut Session, _snap: &Snap) -> Result<Value> {
    Err(bad("omni-log", NO_FILES))
}

/// OMNI-LOG: list the drawing's snapshots, and open one when asked to.
fn run_log(s: &mut Session, p: &Value) -> Result<Value> {
    let (dir, list, capped) = history_of(s, "omni-log", p)?;
    let limit = p.get("limit").and_then(Value::as_u64).map(|l| l as usize).unwrap_or(list.len()).min(list.len());
    echo_list(s, &dir, &list, limit);
    let mut out = json!({
        "folder": dir.to_string_lossy(),
        "count": list.len(),
        "capped": capped,
        "snapshots": list.iter().take(limit).enumerate().map(|(i, x)| x.json(i + 1)).collect::<Vec<_>>(),
    });
    if let Some(sel) = p.get("open").filter(|v| !v.is_null()) {
        let snap = chosen(&list, sel).ok_or_else(|| bad("omni-log", format!("no snapshot {}", json!(sel))))?;
        out["opened"] = open_snapshot(s, &snap)?;
    }
    Ok(out)
}

/// The overlay reference name of a snapshot: the drawing's name and the moment, `_`-separated so
/// it is a valid block name and unique per snapshot.
fn overlay_name(snap: &Snap) -> String {
    let (stem, _) = split_ext(&snap.drawing);
    match snap.epoch {
        Some(e) => {
            let (y, mo, da, h, mi, sd) = civil(e);
            format!("{OVERLAY_PREFIX}{}_{y:04}{mo:02}{da:02}_{h:02}{mi:02}{sd:02}", slug(stem, "-_"))
        }
        None => format!("{OVERLAY_PREFIX}{}", slug(stem, "-_")),
    }
}

/// The overlay references attached to `d` (and nothing else).
fn overlays(d: &Drawing) -> Vec<String> {
    d.blocks
        .values()
        .filter(|b| b.xref_path.is_some() && !b.name.contains('|') && has_prefix(&b.name, OVERLAY_PREFIX))
        .map(|b| b.name.clone())
        .collect()
}

/// The path an overlay stores for `snap`: relative to the drawing's folder when the snapshot is
/// inside its history folder or the folder's cache, written with `/` so the drawing resolves it
/// on every platform.
fn overlay_path(host: &Path, snap: &Path) -> String {
    let dir = history_dir(&folder_of(host));
    if let Ok(rel) = snap.strip_prefix(&dir) {
        let rel = rel.to_string_lossy().replace('\\', "/");
        if !rel.is_empty() {
            return format!("{HISTORY_DIR}/{rel}");
        }
    }
    snap.to_string_lossy().into_owned()
}

/// Attach `snap` as an overlay reference of the open drawing (a snapshot that is already attached
/// is reused), and recolour everything it brings to `color`, so the old revision reads as a
/// background under the new one. Returns the reference's name, how many layers it recoloured and
/// how many objects.
#[cfg(not(target_arch = "wasm32"))]
fn attach_overlay(s: &mut Session, snap: &Snap, color: Color) -> Result<(String, usize, usize)> {
    let name = overlay_name(snap);
    let attached = s.doc()?.block(&name).is_some();
    if attached {
        // Already attached: only make sure it is placed (its reference may have been erased).
        if !placed(s, &name) {
            super::super::blocks::insert_with(s, &name, Vec2::ZERO, Vec2::new(1.0, 1.0), 0.0, None, &serde_json::Map::new())?;
        }
    } else {
        let host = s.state()?.path.clone();
        let file = materialize(snap, "omni-diff")?;
        let path = match host.as_deref() {
            Some(h) => overlay_path(Path::new(h), &file),
            None => file.to_string_lossy().into_owned(),
        };
        s.execute("xattach", &json!({ "path": path, "name": name, "overlay": true, "at": [0.0, 0.0] }))?;
    }
    let (layers, objects) = recolour(s, &name, color)?;
    Ok((name, layers, objects))
}

/// Whether the block `name` is placed anywhere in the drawing.
fn placed(s: &Session, name: &str) -> bool {
    let ins = |st: &cadcraft_doc::EntityStore| st.iter().any(|e| matches!(&e.kind, EntityKind::Insert(i) if i.block.eq_ignore_ascii_case(name)));
    let Ok(d) = s.doc() else { return false };
    ins(&d.model) || d.layouts.iter().any(|l| ins(&l.entities)) || d.blocks.values().filter(|b| !b.name.contains('|')).any(|b| ins(&b.entities))
}

/// Recolour the layers and objects an overlay brought. Both, not only the layers as the AutoLISP
/// original did: an object that carries a colour of its own would otherwise stay visible in it.
fn recolour(s: &mut Session, name: &str, color: Color) -> Result<(usize, usize)> {
    let prefix = format!("{name}|");
    let mine = |n: &str| n.eq_ignore_ascii_case(name) || has_prefix(n, &prefix);
    let d = s.doc_mut()?;
    let mut layers = 0;
    for l in d.layers.iter_mut().filter(|l| has_prefix(&l.name, &prefix)) {
        l.color = color;
        l.on = true;
        layers += 1;
    }
    let keys: Vec<String> = d.blocks.keys().filter(|k| mine(k)).cloned().collect();
    let mut objects = 0;
    for k in keys {
        let Some(b) = d.blocks.get_mut(&k) else { continue };
        let b = Arc::make_mut(b);
        for h in b.entities.handles() {
            if b.entities.modify(h, |e| e.common.color = color) {
                objects += 1;
            }
        }
    }
    Ok((layers, objects))
}

/// The handles of the overlay references OMNI itself laid under a drawing: a report is about the
/// drawing, not about the underlay the tool put there.
fn overlay_handles(d: &Drawing, space: &cadcraft_doc::Space) -> Vec<cadcraft_doc::Handle> {
    d.space(space)
        .map(|st| st.iter().filter(|e| matches!(&e.kind, EntityKind::Insert(i) if has_prefix(&i.block, OVERLAY_PREFIX))).map(|e| e.handle).collect())
        .unwrap_or_default()
}

/// The native comparison of the open drawing with `snap`: the same object matching COMPARE does
/// (geometry and properties, handle first), reported as numbers and change areas. The underlay a
/// previous OMNI-DIFF laid under the drawing (or that is attached in the snapshot) is not counted:
/// attaching it is the tool's own doing, not a change the user made.
#[cfg(not(target_arch = "wasm32"))]
fn report(s: &Session, snap: &Snap) -> Result<Value> {
    let hooks = io().ok_or_else(|| bad("omni-diff", "file formats are not available in this build"))?;
    let bytes = read_snap(snap, "omni-diff")?;
    let other = (hooks.read)(&bytes, &snap.inner).map_err(|e| bad("omni-diff", format!("{}: {e}", snap.file)))?;
    let st = s.state()?;
    let (cur, space) = (st.doc.clone(), st.space.clone());
    let mut d = compare::diff(&cur, &other, &space);
    let (mine, theirs) = (overlay_handles(&cur, &space), overlay_handles(&other, &space));
    d.added.retain(|h| !mine.contains(h));
    d.removed.retain(|h| !theirs.contains(h));
    d.changed.retain(|(a, b, _, _)| !mine.contains(a) && !theirs.contains(b));
    let regions = compare::diff_regions(&cur, &other, &d);
    Ok(json!({
        "counts": { "added": d.added.len(), "removed": d.removed.len(), "changed": d.changed.len(), "common": d.common },
        "regions": regions.iter().map(|b| json!([[b.min.x, b.min.y], [b.max.x, b.max.y]])).collect::<Vec<_>>(),
        "truncated": d.truncated,
        "message": format!(
            "Compared with {}: {} only in this drawing, {} only in the snapshot, {} changed, {} the same{}.",
            snap.file,
            d.added.len(),
            d.removed.len(),
            d.changed.len(),
            d.common,
            if d.truncated { " (the comparison stopped early: too many objects)" } else { "" }
        ),
    }))
}

/// OMNI-DIFF: lay a snapshot under the drawing (an overlay reference, recoloured), report what
/// changed between the two, or both.
#[cfg(not(target_arch = "wasm32"))]
fn run_diff(s: &mut Session, p: &Value) -> Result<Value> {
    let (dir, list, _) = history_of(s, "omni-diff", p)?;
    if list.is_empty() {
        return Err(bad("omni-diff", format!("no snapshots in {}: take one with OMNI-SNAP first", dir.display())));
    }
    let sel = p.get("snapshot").filter(|v| !v.is_null()).cloned().unwrap_or(json!(1));
    let snap = chosen(&list, &sel).ok_or_else(|| bad("omni-diff", format!("no snapshot {}", json!(sel))))?;
    let mode = str_param(p, "mode").unwrap_or("overlay").trim().to_ascii_lowercase();
    if !["overlay", "report", "both"].contains(&mode.as_str()) {
        return Err(bad("omni-diff", format!("`mode` must be overlay, report or both, not \"{mode}\"")));
    }
    let color = match p.get("color") {
        None | Some(Value::Null) => 1u64,
        Some(v) => v.as_u64().ok_or_else(|| bad("omni-diff", "`color` must be an ACI colour 1-255"))?,
    };
    let aci = u8::try_from(color)
        .ok()
        .filter(|v| (1..=255).contains(v))
        .ok_or_else(|| bad("omni-diff", "`color` must be an ACI colour 1-255 (1 is red)"))?;
    let mut out = json!({ "file": snap.file, "path": snap.path.to_string_lossy(), "time": snap.time(), "mode": mode });
    // The report comes first when both are asked for: the underlay is laid under the *drawing*,
    // and its own reference would otherwise be counted as something only this drawing has.
    if mode != "overlay" {
        let r = report(s, &snap)?;
        if let Some(m) = r.get("message").and_then(Value::as_str) {
            s.echo(m.to_string());
        }
        for (k, v) in r.as_object().into_iter().flatten() {
            out[k] = v.clone();
        }
    }
    if mode != "report" {
        let (name, layers, objects) = attach_overlay(s, &snap, Color::Index(aci))?;
        s.echo(format!(
            "[OMNI] {name}: {objects} object(s) on {layers} layer(s) laid under the drawing in ACI {aci}. Hide it with OMNI-TOGGLE, detach it with OMNI-CLEAR."
        ));
        out["overlay"] = json!(name);
        out["layers"] = json!(layers);
        out["objects"] = json!(objects);
    }
    s.touch();
    Ok(out)
}
#[cfg(target_arch = "wasm32")]
fn run_diff(_s: &mut Session, _p: &Value) -> Result<Value> {
    Err(bad("omni-diff", NO_FILES))
}

/// The overlays a call names (`snapshot` is an overlay name, or the file name of the snapshot it
/// shows), or all of them.
fn overlays_named(s: &Session, p: &Value, cmd: &str) -> Result<Vec<String>> {
    let d = s.doc()?;
    let pattern = str_param(p, "snapshot");
    let picked: Vec<String> = overlays(d)
        .into_iter()
        .filter(|n| match pattern {
            None => true,
            Some(f) => {
                let file = d.block(n).and_then(|b| b.xref_path.clone()).unwrap_or_default();
                let name = file.rsplit(['/', '\\']).next().unwrap_or_default();
                n == f || has_prefix(n, f) || file == f || name == f
            }
        })
        .collect();
    if picked.is_empty() {
        return Err(bad(cmd, "no snapshot overlay matches: run OMNI-DIFF first"));
    }
    Ok(picked)
}

/// OMNI-TOGGLE: show or hide the snapshot overlays.
///
/// The overlay is unloaded and reloaded rather than hidden layer by layer. An external reference's
/// objects on layer 0 keep the *host's* layer 0 — in AutoCAD as here — so a layer toggle would
/// leave them in the drawing, and the AutoLISP original had exactly that hole. Unloading cannot
/// leave a stray object behind. The colours set on the overlay's layers stay with them, so
/// reloading brings the underlay back red.
fn run_toggle(s: &mut Session, p: &Value) -> Result<Value> {
    let names = overlays_named(s, p, "omni-toggle")?;
    let show = match p.get("show") {
        None | Some(Value::Null) => None,
        Some(Value::Bool(b)) => Some(*b),
        Some(_) => return Err(bad("omni-toggle", "`show` must be true or false")),
    };
    let showing = names.first().and_then(|n| s.doc().ok()?.block(n)).is_some_and(|b| b.xref_status != XrefStatus::Unloaded);
    let want = show.unwrap_or(!showing);
    s.execute("-xref", &json!({ "option": if want { "reload" } else { "unload" }, "names": names.join(",") }))?;
    if want {
        // Reloading reads the snapshot again, so what it brings is one colour again: the layers
        // keep the colour the overlay was given, and their objects are forced back to it.
        for n in &names {
            let prefix = format!("{n}|");
            let color = s.doc()?.layers.iter().find(|l| has_prefix(&l.name, &prefix)).map(|l| l.color).unwrap_or(Color::Index(1));
            recolour(s, n, color)?;
        }
    }
    let objects: usize = names.iter().filter_map(|n| s.doc().ok()?.block(n).map(|b| b.entities.len())).sum();
    s.echo(format!("[OMNI] Overlay {} ({} reference(s), {objects} object(s)).", if want { "shown" } else { "hidden" }, names.len()));
    Ok(json!({ "overlay": names, "shown": want, "objects": objects }))
}

/// OMNI-CLEAR: detach the overlays OMNI-DIFF attached, and only those.
fn run_clear(s: &mut Session, p: &Value) -> Result<Value> {
    if overlays(s.doc()?).is_empty() {
        s.echo("[OMNI] No snapshot overlay to clear.");
        return Ok(json!({ "cleared": [], "erased": 0 }));
    }
    let names = overlays_named(s, p, "omni-clear")?;
    let r = s.execute("-xref", &json!({ "option": "detach", "names": names.join(",") }))?;
    let erased = r.get("erased").and_then(Value::as_u64).unwrap_or(0);
    s.echo(format!("[OMNI] Overlay cleared: {} reference(s), {erased} object(s) removed.", names.len()));
    Ok(json!({ "cleared": names, "erased": erased }))
}

/// OMNI-NOTE: a marker and a comment on the drawing's own note layer.
fn run_note(s: &mut Session, p: &Value) -> Result<Value> {
    let at = point_req("omni-note", p, "at")?;
    let text = str_param(p, "text").map(str::to_string).filter(|t| !t.trim().is_empty());
    let value: String = match text {
        Some(t) => t.chars().take(MAX_TEXT).collect(),
        None => return Err(bad("omni-note", "`text` is required")),
    };
    let height = size_param("omni-note", p, "height", false)?.unwrap_or(NOTE_HEIGHT);
    let radius = size_param("omni-note", p, "radius", false)?.unwrap_or(NOTE_RADIUS);
    let layer = str_param(p, "layer").map(str::trim).filter(|l| !l.is_empty()).unwrap_or(NOTE_LAYER).to_string();
    if layer.len() > 255 || layer.chars().any(|c| "<>/\\\":;?*|,=`".contains(c)) {
        return Err(bad("omni-note", format!("\"{layer}\" is not a valid layer name")));
    }
    // Both objects are built before the drawing is borrowed mutably (the text style comes from it).
    let marker = circle(at, radius);
    let label = make_text(s, at, &value, height, 0.0, (cadcraft_doc::HAlign::Left, cadcraft_doc::VAlign::Baseline));
    let space = s.space();
    let common = Common { layer: layer.clone(), ..Common::default() };
    let d = s.doc_mut()?;
    if d.ensure_layer(&layer) {
        // A new OMNI_NOTES layer is red, as OMNI-NOTE's original made it.
        if let Some(l) = d.layer_mut(&layer) {
            l.color = Color::Index(1);
        }
    }
    let handle = d.add(&space, common.clone(), marker)?;
    let text_handle = d.add(&space, common, label)?;
    s.echo(format!("[OMNI] Note added on layer \"{layer}\"."));
    Ok(json!({ "handle": handle.hex(), "textHandle": text_handle.hex(), "layer": layer, "at": [at.x, at.y] }))
}

// ---------------------------------------------------------------- interactive

/// OMNI-SNAP typed at the command line: the description, then the snapshot.
struct SnapM;

impl Interactive for SnapM {
    fn name(&self) -> &'static str {
        "OMNI-SNAP"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new("Enter a description for this snapshot", Accept::TEXT).default("snapshot")
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let description = match &i {
            Input::Text(t) => t.trim().to_string(),
            Input::Enter => "snapshot".to_string(),
            Input::Cancel => return Ok(Step::Cancel),
            _ => return Ok(Step::Continue),
        };
        match run_snap(s, &json!({ "description": description })) {
            Ok(_) => Ok(Step::Done),
            Err(EngineError::BadParams { msg, .. }) => {
                s.echo(msg);
                Ok(Step::Done)
            }
            Err(e) => Err(e),
        }
    }
}

/// OMNI-LOG typed at the command line: the history, then the number of the snapshot to open.
struct LogM;

impl Interactive for LogM {
    fn name(&self) -> &'static str {
        "OMNI-LOG"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        match history_of(s, "omni-log", &json!({})) {
            Ok((dir, list, _)) => {
                echo_list(s, &dir, &list, list.len());
                Ok(if list.is_empty() { Step::Cancel } else { Step::Continue })
            }
            Err(EngineError::BadParams { msg, .. }) => {
                s.echo(msg);
                Ok(Step::Cancel)
            }
            Err(e) => Err(e),
        }
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new("Enter the number of the snapshot to open (0 to cancel)", Accept::NUMBER).default("0")
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let t = match &i {
            Input::Enter => "0".to_string(),
            Input::Text(t) | Input::Keyword(t) => t.trim().to_string(),
            Input::Cancel => return Ok(Step::Cancel),
            _ => return Ok(Step::Continue),
        };
        let Ok((_, list, _)) = history_of(s, "omni-log", &json!({})) else { return Ok(Step::Done) };
        match chosen(&list, &json!(t)) {
            Some(snap) => {
                open_snapshot(s, &snap)?;
                Ok(Step::Done)
            }
            None => {
                s.echo(format!("\"{t}\" is not a snapshot in this drawing's history."));
                Ok(Step::Done)
            }
        }
    }
}

/// OMNI-DIFF typed at the command line: the history, then the number to lay under the drawing.
struct DiffM;

impl Interactive for DiffM {
    fn name(&self) -> &'static str {
        "OMNI-DIFF"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        match history_of(s, "omni-diff", &json!({})) {
            Ok((dir, list, _)) => {
                echo_list(s, &dir, &list, list.len());
                Ok(if list.is_empty() { Step::Cancel } else { Step::Continue })
            }
            Err(EngineError::BadParams { msg, .. }) => {
                s.echo(msg);
                Ok(Step::Cancel)
            }
            Err(e) => Err(e),
        }
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new("Enter the number of the snapshot to compare with (Enter: the newest, 0 to cancel)", Accept::NUMBER).default("1")
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let t = match &i {
            Input::Enter => "1".to_string(),
            Input::Text(t) | Input::Keyword(t) => t.trim().to_string(),
            Input::Cancel => return Ok(Step::Cancel),
            _ => return Ok(Step::Continue),
        };
        if t == "0" {
            return Ok(Step::Done);
        }
        if t.parse::<usize>().is_err() {
            s.echo("Enter the number of a snapshot (0 to cancel).");
            return Ok(Step::Continue);
        }
        match run_diff(s, &json!({ "snapshot": t })) {
            Ok(_) => Ok(Step::Done),
            Err(EngineError::BadParams { msg, .. }) => {
                s.echo(msg);
                Ok(Step::Done)
            }
            Err(e) => Err(e),
        }
    }
}

/// OMNI-NOTE typed at the command line: the point, then the comment.
#[derive(Default)]
struct NoteM {
    at: Option<Vec2>,
}

impl Interactive for NoteM {
    fn name(&self) -> &'static str {
        "OMNI-NOTE"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.at {
            None => Prompt::new("Specify the point to comment on", Accept::POINT),
            Some(_) => Prompt::new("Enter the comment", Accept::TEXT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match (self.at, i) {
            (None, Input::Point(p)) => self.at = Some(p),
            (Some(at), Input::Text(t)) if !t.trim().is_empty() => {
                run_note(s, &json!({ "at": [at.x, at.y], "text": t }))?;
                return Ok(Step::Done);
            }
            (_, Input::Cancel) => return Ok(Step::Cancel),
            (Some(_), Input::Enter) => return Ok(Step::Cancel),
            _ => {}
        }
        Ok(Step::Continue)
    }
    fn preview(&self, _s: &Session, cursor: Vec2) -> Vec<EntityKind> {
        match self.at {
            None => vec![circle(cursor, NOTE_RADIUS)],
            Some(_) => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stamp_round_trips_through_civil_time() {
        for secs in [0i64, 1_760_000_000, 1_766_000_000, 4_102_444_800] {
            let (y, mo, da, h, mi, sd) = civil(secs);
            assert_eq!(days_from_civil(y, mo, da) * 86_400 + h * 3600 + mi * 60 + sd, secs, "{secs}");
        }
        // A known moment: 2026-10-14T08:35:12Z.
        let (y, mo, da, h, mi, sd) = civil(1_791_966_912);
        assert_eq!((y, mo, da, h, mi, sd), (2026, 10, 14, 8, 35, 12));
        assert_eq!(iso(1_791_966_912, 482), "2026-10-14T08:35:12.482Z");
        assert_eq!(stamp_epoch("20261014.083512"), Some(1_791_966_912));
    }

    #[test]
    fn hostile_stamps_are_refused_not_guessed() {
        for s in [
            "",
            ".",
            "2026",
            "20261014",
            "20261014.",
            "20261014.0835",
            "x0261014.083512",
            "20261014.0835127",
            "20261314.083512",
            "20261014.253512",
            "é0261014.083512",
        ] {
            assert_eq!(stamp_epoch(s), None, "{s}");
        }
    }

    #[test]
    fn names_keep_their_fields_and_survive_any_script() {
        let s = parse("20261014.083512_482_kostyk_правка-стен_план_на_2026.dxf", PathBuf::from("x.dxf"), 12);
        assert_eq!(s.epoch, Some(1_791_966_912));
        assert_eq!(s.millis, 482);
        assert_eq!(s.user, "kostyk");
        assert_eq!(s.description, "правка-стен");
        assert_eq!(s.drawing, "план_на_2026.dxf", "everything after the description is the drawing");
        assert_eq!(s.bytes, 12);
        // A file that doesn't follow the pattern is still read, with what its name didn't say empty.
        let f = parse("notes.dxf", PathBuf::from("notes.dxf"), 0);
        assert_eq!((f.epoch, f.drawing.as_str()), (None, "notes.dxf"));
    }

    #[test]
    fn slugs_drop_separators_and_path_characters() {
        assert_eq!(slug("правка стен", "-"), "правка-стен");
        assert_eq!(slug("a/b\\c:d*e?f", "-"), "a-b-c-d-e-f");
        assert_eq!(slug("__x__", "-"), "x", "the name's own separator never survives a description");
        assert_eq!(slug("", "-"), "");
        assert_eq!(slug("  ", "-"), "");
        assert_eq!(slug("a--b", "-"), "a--b", "a separator the user typed is kept");
        assert_eq!(slug("-x-", "-"), "x");
        assert_eq!(slug("x".repeat(200).as_str(), "-").chars().count(), MAX_PART);
        // A block name keeps the `_` that separates its parts.
        assert_eq!(slug("план_2026", "-_"), "план_2026");
        assert_eq!(slug("план 2026", "-_"), "план-2026", "a space becomes a dash, even where a `_` would do");
    }

    #[test]
    fn an_overlay_is_named_after_its_snapshot() {
        let s = parse("20261014.083512_482_kostyk_правка_план.dxf", PathBuf::from("x.dxf"), 0);
        assert_eq!(overlay_name(&s), "OMNI_план_20261014_083512");
        let untimed = parse("notes.dxf", PathBuf::from("notes.dxf"), 0);
        assert_eq!(overlay_name(&untimed), "OMNI_notes");
    }

    #[test]
    fn overlay_paths_are_relative_to_the_drawing_and_use_slashes() {
        // The paths OMNI-SNAP builds: the history folder next to the drawing, one file in it.
        let host = Path::new("jobs/site/plan.dxf");
        let snap = history_dir(&folder_of(host)).join("20261014.083512_482_u_d_plan.dxf");
        assert_eq!(overlay_path(host, &snap), format!("{HISTORY_DIR}/20261014.083512_482_u_d_plan.dxf"));
        // A drawing opened by a bare file name has a folder too (`.`, as everywhere else).
        let bare = Path::new("plan.dxf");
        let here = history_dir(&folder_of(bare)).join("a.dxf");
        assert_eq!(overlay_path(bare, &here), format!("{HISTORY_DIR}/a.dxf"));
        // Anywhere else the snapshot is named by its full path: only this drawing's own history
        // folder is stored relative (another drawing's is not this drawing's to find).
        let other = PathBuf::from("elsewhere/plan.dxf");
        assert_eq!(overlay_path(host, &other), other.to_string_lossy());
        let theirs = PathBuf::from("other").join(HISTORY_DIR).join("plan.dxf");
        assert_eq!(overlay_path(host, &theirs), theirs.to_string_lossy());
    }

    #[test]
    fn a_folder_past_the_limit_still_starts_with_the_newest_version() {
        let dir = std::env::temp_dir().join(format!("cadcraft-omni-cap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // One version more than a listing holds. The names sort the way versions do (a stamp
        // first), and the loop writes them oldest first, so a cap applied while reading would keep
        // the oldest [`MAX_SNAPSHOTS`] and lose the newest.
        for i in 0..=MAX_SNAPSHOTS {
            let name = format!("{i:08}_000_u_save_plan.dxf.gz");
            std::fs::write(dir.join(name), b"").unwrap();
        }
        let (list, capped) = snapshots(&dir);
        assert!(capped, "the folder holds more than the listing does");
        assert_eq!(list.len(), MAX_SNAPSHOTS);
        let newest = format!("{MAX_SNAPSHOTS:08}_000_u_save_plan.dxf.gz");
        assert_eq!(list.first().map(|s| s.file.as_str()), Some(newest.as_str()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_account_with_an_underscore_is_still_a_save() {
        // `_` separates a version's fields, so an account named `john_doe` must lose it here:
        // otherwise the name reads back as user `john`, description `doe` — a milestone, not a save
        // — and the drawing it belongs to is never found again.
        assert_eq!(user_part_of("john_doe"), "john-doe");
        assert_eq!(user_part_of("john doe"), "john-doe");
        assert_eq!(user_part_of("___"), "unknown", "a part that is only separators still names someone");
        let name = format!("20261014.083512_482_{}_save_plan.dxf.gz", user_part_of("john_doe"));
        let s = parse(&name, PathBuf::from("h").join(&name), 9);
        assert_eq!(s.user, "john-doe");
        assert_eq!(s.description, SAVE_DESCRIPTION);
        assert_eq!(s.kind(), "save");
        assert_eq!(s.drawing, "plan.dxf");
    }

    #[test]
    fn history_paths_are_recognised_in_any_case_and_folder() {
        for p in [
            "jobs/site/_OMNI_HISTORY/20261014.083512_482_u_save_plan.dxf.gz",
            "C:\\jobs\\_omni_history\\a.dxf",
            "jobs/site/_OMNI_HISTORY/.cache/a.dxf",
        ] {
            assert!(is_version_path(p), "{p}");
        }
        for p in ["jobs/site/plan.dxf", "jobs/_OMNI_HISTORY_FULL/plan.dxf", "", "plan.dxf"] {
            assert!(!is_version_path(p), "{p}");
        }
    }

    #[test]
    fn only_drawings_are_snapshots() {
        assert!(is_snapshot("20261014.083512_482_u_d_plan.dxf"));
        assert!(is_snapshot("a.DWG"));
        assert!(!is_snapshot("desktop.ini"));
        assert!(!is_snapshot("readme.md"));
        assert!(!is_snapshot("_OMNI_HISTORY"));
    }

    #[test]
    fn a_snapshot_is_chosen_by_number_name_or_path() {
        let new = "20261014.083512_482_u_new_plan.dxf";
        let old = "20261013.083512_482_u_old_plan.dxf";
        let list = vec![parse(new, PathBuf::from("h").join(new), 3), parse(old, PathBuf::from("h").join(old), 4)];
        assert_eq!(chosen(&list, &json!(1)).map(|s| s.file), Some(new.to_string()));
        assert_eq!(chosen(&list, &json!(2)).map(|s| s.file), Some(old.to_string()));
        assert_eq!(chosen(&list, &json!(3)), None, "past the end");
        assert_eq!(chosen(&list, &json!(0)), None, "0 is cancel, not the first");
        assert_eq!(chosen(&list, &json!("2")).map(|s| s.bytes), Some(4));
        assert_eq!(chosen(&list, &json!(old)).map(|s| s.bytes), Some(4), "a file name");
        assert_eq!(chosen(&list, &json!("OLD")), None, "a name is matched whole, not by its parts");
        assert_eq!(chosen(&list, &json!("old/plan.dxf")), None, "a folder is not a snapshot");
        assert_eq!(chosen(&list, &json!(format!("h/{old}"))).map(|s| s.bytes), Some(4), "a full path");
        assert_eq!(chosen(&list, &json!(true)), None);
        assert_eq!(chosen(&[], &json!(1)), None);
    }
}
