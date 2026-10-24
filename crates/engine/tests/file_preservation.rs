//! FF1 at the command boundary: real I/O hooks, the native import context, files on disk, loss
//! reports beside them and backups of overwritten sources. This executable owns its OnceLock
//! hooks; no dummy writer can hide what a save lacks.
#![cfg(not(target_arch = "wasm32"))]

#[path = "../../io/tests/support/ff1.rs"]
mod ff1;

use std::fs;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use acadrust::objects::ObjectType;
use acadrust::{DwgReader, EntityType};
use cadcraft_doc::{Drawing, EntityKind, Handle};
use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, Loss, SaveAs, base64_decode, set_io, set_write_as, set_write_report};
use ff1::{CustomRecords, TestResult};
use serde_json::{Value, json};

static NEXT: AtomicUsize = AtomicUsize::new(0);
const SENTINEL: &[u8] = b"existing engine target\0\xff";

struct TestDir(PathBuf);

impl TestDir {
    /// A fresh directory, removed when dropped.
    fn new() -> TestResult<Self> {
        let path = std::env::temp_dir().join(format!("cc-engine-ff1-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TestDir {
    /// Remove the directory and what is in it.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Install the real I/O hooks, the loss-reporting writer among them.
fn install() {
    set_io(IoHooks {
        read: |b, n| cadcraft_io::read(b, n).map_err(|e| e.to_string()),
        write: |d, n| cadcraft_io::write(d, n).map_err(|e| e.to_string()),
        plot: None,
    });
    set_write_as(|d, n, o: &SaveAs| {
        cadcraft_io::write_with(d, n, &cadcraft_io::SaveOptions { version: o.version, binary: o.binary }).map_err(|e| e.to_string())
    });
    set_write_report(|d, n, o: &SaveAs| {
        let (bytes, report) =
            cadcraft_io::write_with_report(d, n, &cadcraft_io::SaveOptions { version: o.version, binary: o.binary }).map_err(|e| e.to_string())?;
        Ok((bytes, report.filter(|r| !r.is_empty()).map(|r| Loss { count: r.count(), summary: r.summary(), json: r.to_json() })))
    });
}

// Catch committing before serialization/storage succeeds, including saved version/binary state.
struct Before {
    doc: Arc<Drawing>,
    saved: Arc<Drawing>,
    path: Option<String>,
    title: String,
    dirty: bool,
    binary: bool,
    undo: usize,
    redo: usize,
}

impl Before {
    /// The session state a failed save or byte delivery must not change.
    fn capture(s: &Session) -> Self {
        let st = s.state().unwrap();
        Self {
            doc: st.doc.clone(),
            saved: st.saved.clone(),
            path: st.path.clone(),
            title: st.title.clone(),
            dirty: st.is_dirty(),
            binary: st.binary_dxf,
            undo: st.undo.len(),
            redo: st.redo.len(),
        }
    }

    /// Assert the session state is as captured.
    fn unchanged(&self, s: &Session) {
        let st = s.state().unwrap();
        assert!(Arc::ptr_eq(&st.doc, &self.doc), "live drawing changed on failure/byte delivery");
        assert!(Arc::ptr_eq(&st.saved, &self.saved), "saved snapshot changed");
        assert_eq!((&st.path, &st.title, st.is_dirty(), st.binary_dxf), (&self.path, &self.title, self.dirty, self.binary));
        assert_eq!((st.undo.len(), st.redo.len()), (self.undo, self.redo));
    }
}

/// The LINE's handle.
fn line(d: &Drawing) -> Handle {
    d.model.iter().find(|e| matches!(e.kind, EntityKind::Line(_))).expect("LINE present").handle
}

/// Assert the LINE is moved by `shift` along X.
fn assert_line(d: &Drawing, shift: f64) {
    let EntityKind::Line(l) = &d.entity(line(d)).unwrap().kind else { panic!("LINE absent") };
    assert_eq!((l.a.x, l.a.y, l.b.x, l.b.y), (1. + shift, 2., 11. + shift, 12.));
}

/// MOVE the LINE by 5 along X.
fn edit(s: &mut Session) {
    let handle = line(s.doc().unwrap());
    let r = s.execute("move", &json!({"handle": handle.hex(), "delta": [5, 0]})).unwrap();
    assert_eq!(r["moved"], 1);
}

/// Assert a saved file keeps the custom records of `variant` and the LINE moved by `shift`.
fn assert_foreign(bytes: &[u8], variant: CustomRecords, shift: f64) -> TestResult {
    let d = cadcraft_io::read(bytes, "saved.dxf")?;
    assert_line(&d, shift);
    let tags = cadcraft_dxf::parse(bytes)?;
    let records = cadcraft_dxf::records(&tags);
    let value = |body: &[cadcraft_dxf::Tag], code| body.iter().find(|t| t.code == code).map(cadcraft_dxf::Tag::str);
    let line = &records.iter().find(|(k, _)| k == "LINE").ok_or("LINE record absent")?.1;
    let line_handle = value(line, 5).ok_or("LINE handle absent")?;
    for (entity, name, payload, binary) in [(true, "FF1_WIDGET", "FF1_ENTITY_UNIQUE", "B4A8"), (false, "FF1_OBJECT", "FF1_OBJECT_UNIQUE", "D358")] {
        let expected = !matches!((variant, entity), (CustomRecords::EntityOnly, false) | (CustomRecords::ObjectOnly, true));
        let classes: Vec<_> = records.iter().filter(|(k, b)| k == "CLASS" && value(b, 1).as_deref() == Some(name)).collect();
        let objects: Vec<_> = records.iter().filter(|(k, _)| k == name).collect();
        assert_eq!((classes.len(), objects.len()), (usize::from(expected), usize::from(expected)));
        if !expected {
            continue;
        }
        let class = &classes[0].1;
        assert_eq!(value(class, 2), Some(format!("Rust{name}")));
        assert_eq!(value(class, 3).as_deref(), Some("FF1_SYNTHETIC"));
        assert_eq!(value(class, 281).as_deref(), Some(if entity { "1" } else { "0" }));
        assert_eq!(value(class, 91).as_deref(), Some("1"), "actual instance count");
        let body = &objects[0].1;
        for (code, expected) in [(1, payload), (310, binary), (90, "13"), (340, line_handle.as_str())] {
            assert_eq!(body.iter().filter(|t| t.code == code && t.str() == expected).count(), 1, "{name} group {code}");
        }
        let owner = value(body, 330).ok_or("foreign owner absent")?;
        if entity {
            assert_eq!(value(line, 330), Some(owner));
        } else {
            let handle = value(body, 5).ok_or("foreign object handle absent")?;
            let nod = &records.iter().find(|(k, b)| k == "DICTIONARY" && value(b, 5).as_deref() == Some(&owner)).ok_or("owner dictionary absent")?.1;
            assert_eq!(nod.windows(2).filter(|w| w[0].code == 3 && w[0].str() == name && w[1].code == 350 && w[1].str() == handle).count(), 1);
        }
    }
    Ok(())
}

/// A save that lacks records of the source: it succeeds, says so on the command line and in its
/// result, and keeps the report beside the file, naming each of `names`.
fn lossy_save(s: &mut Session, cmd: &str, params: Value, names: &[&str]) -> TestResult<Value> {
    let log = s.log.len();
    let r = s.execute(cmd, &params)?;
    assert!(r["lost"].as_u64().is_some_and(|n| n >= 1), "{cmd} {params}: {r}");
    let report = r["lost_report"].as_str().ok_or("no report path")?;
    let json: Value = serde_json::from_str(&fs::read_to_string(report)?)?;
    let types: Vec<&str> = json["records"].as_array().ok_or("records")?.iter().filter_map(|x| x["type"].as_str()).collect();
    for name in names {
        assert!(types.contains(name), "{cmd} {params}: {name} not in {types:?}");
    }
    assert!(s.log.iter().skip(log).any(|l| l.contains("aren't in this")), "the loss isn't on the command line");
    Ok(r)
}

// Source oracle precedes CADCraft import. Full bit/text/reference stream assertions are also
// exercised in io::ff1_preservation using this same original builder; no AutoCAD oracle claimed.
/// Assert a saved DWG holds the source's custom records unchanged.
fn assert_source(source: &ff1::DwgSource, bytes: &[u8]) -> TestResult {
    let out = DwgReader::from_stream(Cursor::new(bytes)).read_with_stats()?;
    assert!(out.stats.record_stream_read && out.stats.stream_completed && out.stats.source_records > 0);
    assert_eq!((out.stats.skipped_source_records, out.stats.recovered_errors), (0, 0));
    let doc = out.document;
    assert!(matches!(doc.get_entity(source.line), Some(EntityType::Line(_))));
    for (entity, record) in [(true, source.entity.as_ref()), (false, source.object.as_ref())] {
        let Some(record) = record else { continue };
        let class = doc.classes.get_by_name(record.name).ok_or("source CLASS absent")?;
        assert_eq!(class.class_number, record.class_number);
        assert_eq!(class.cpp_class_name, format!("Rust{}", record.name));
        assert_eq!(class.application_name, "FF1_SYNTHETIC");
        assert_eq!(class.is_an_entity, entity);
        if entity {
            let Some(EntityType::Unknown(u)) = doc.get_entity(record.handle) else { return Err("source custom entity absent".into()) };
            assert_eq!(u.raw_dwg_data.as_deref(), Some(record.raw.as_ref()));
            assert_eq!(u.common.owner_handle, record.owner);
            assert_eq!(u.dwg_handle_bits, record.handle_bits);
        } else {
            let Some(ObjectType::Unknown { raw_dwg_data, owner, raw_dwg_handle_bits, .. }) = doc.objects.get(&record.handle) else {
                return Err("source custom object absent".into());
            };
            assert_eq!(raw_dwg_data.as_deref(), Some(record.raw.as_ref()));
            assert_eq!(*owner, record.owner);
            assert_eq!(*raw_dwg_handle_bits, record.handle_bits);
        }
    }
    Ok(())
}

/// Every engine save of a DWG with `variant`: kept, or reported and the source backed up.
fn native_saves(variant: CustomRecords) -> TestResult {
    install();
    let source = ff1::dwg_source(variant)?;
    assert_source(&source, &source.bytes)?;
    let dir = TestDir::new()?;
    let input = dir.0.join("source.dwg");
    fs::write(&input, &source.bytes)?;
    let mut s = Session::empty();
    let opened = s.execute("open", &json!({"path": input}))?;
    assert!(!opened["warnings"].as_array().unwrap().is_empty());
    let state = s.doc()?.preservation.clone();
    assert!(state.blocks_save());
    assert_eq!(state.issue_count(), u64::from(source.entity.is_some()) + u64::from(source.object.is_some()));
    let mut lost: Vec<&str> = Vec::new();
    if let Some(e) = &source.entity {
        lost.push(e.name);
    }
    let mut every: Vec<&str> = lost.clone();
    if let Some(o) = &source.object {
        every.push(o.name);
    }
    edit(&mut s);
    assert_line(s.doc()?, 5.);
    assert!(Arc::ptr_eq(&s.doc()?.preservation, &state));
    assert!(s.undo()?.is_some());
    assert_line(s.doc()?, 0.);
    assert!(Arc::ptr_eq(&s.doc()?.preservation, &state));
    // Bytes for the caller: the report is in the result, the drawing's state untouched.
    let before = Before::capture(&s);
    let r = s.execute("document.bytes", &json!({}))?;
    assert!(r["lost"].as_u64().is_some_and(|n| n >= 1));
    assert!(r["lost_report"]["records"].as_array().is_some_and(|a| !a.is_empty()));
    before.unchanged(&s);
    assert!(s.redo()?.is_some());
    assert_line(s.doc()?, 5.);

    // In place, in the source's version: the carried object goes back; only the entity is lost,
    // and then the source is kept as source.orig.dwg before it is overwritten.
    let r = s.execute("qsave", &json!({}))?;
    if lost.is_empty() {
        assert!(r.get("lost").is_none() && r.get("backup").is_none(), "{r}");
        assert!(!dir.0.join("source.dwg.lost.json").exists());
    } else {
        assert_eq!(r["lost"], lost.len());
        assert_eq!(fs::read(dir.0.join("source.orig.dwg"))?, source.bytes.as_ref(), "the original is kept");
    }
    assert!(!s.state()?.is_dirty());
    let native = DwgReader::from_stream(Cursor::new(fs::read(&input)?)).read()?;
    if let Some(o) = &source.object {
        assert!(matches!(native.objects.get(&o.handle), Some(ObjectType::Unknown { .. })), "the carried object is in the saved DWG");
    }
    // In place as 2004: everything carried is lost from that file; the report keeps each one's
    // bytes. The file is backed up when it holds more than this save keeps (the carried object).
    let r = lossy_save(&mut s, "saveas", json!({"path": input, "version": 2004}), &every)?;
    assert_eq!(r.get("backup").is_some(), source.object.is_some(), "{r}");
    for (ext, binary) in [("dwg", false), ("dwt", false), ("dxf", false), ("dxf", true)] {
        let target = dir.0.join(format!("target.{ext}"));
        if !target.exists() {
            fs::write(&target, SENTINEL)?;
        }
        // Another file, or (the second DXF) the drawing's file that already lacks as much.
        let r = lossy_save(&mut s, "saveas", json!({"path": target, "version": "2007", "binary": binary}), &every)?;
        assert!(r.get("backup").is_none(), "nothing more to lose there: {r}");
        assert_ne!(fs::read(&target)?, SENTINEL);
    }
    // Rendering is never a database save: no report.
    let before = Before::capture(&s);
    let image = dir.0.join("view.svg");
    let r = s.execute("saveas", &json!({"path": image}))?;
    assert!(r.get("lost").is_none());
    assert!(String::from_utf8(fs::read(image)?)?.contains("<svg"));
    before.unchanged(&s);
    Ok(())
}

/// A custom entity is reported and the overwritten source backed up.
#[test]
fn native_custom_entity_is_reported_and_its_source_backed_up() -> TestResult {
    native_saves(CustomRecords::EntityOnly)
}

/// A custom object goes back into a same-version DWG.
#[test]
fn native_custom_object_goes_back_into_a_same_version_dwg() -> TestResult {
    native_saves(CustomRecords::ObjectOnly)
}

/// The import context survives UNDO and REDO.
#[test]
fn native_custom_entity_and_object_context_survives_undo_redo() -> TestResult {
    native_saves(CustomRecords::Combined)
}

/// A DWG export of a DXF drawing reports what it lacks; a later DXF save keeps everything.
#[test]
fn native_dxf_dwg_export_reports_its_losses_and_a_later_dxf_save_keeps_everything() -> TestResult {
    install();
    for variant in [CustomRecords::EntityOnly, CustomRecords::ObjectOnly, CustomRecords::Combined] {
        let dir = TestDir::new()?;
        let input = dir.0.join("source.dxf");
        fs::write(&input, ff1::dxf_source(variant))?;
        let mut s = Session::empty();
        s.execute("open", &json!({"path": input}))?;
        edit(&mut s);
        let mut names = Vec::new();
        if !matches!(variant, CustomRecords::ObjectOnly) {
            names.push("FF1_WIDGET");
        }
        if !matches!(variant, CustomRecords::EntityOnly) {
            names.push("FF1_OBJECT");
        }
        for ext in ["dwg", "dwt"] {
            let target = dir.0.join(format!("target.{ext}"));
            lossy_save(&mut s, "saveas", json!({"path": target, "version": 2004}), &names)?;
            let r = s.execute("document.bytes", &json!({"format": ext, "version": 2004}))?;
            assert!(r["lost"].as_u64().is_some_and(|n| n as usize == names.len()), "{r}");
        }
        assert!(!s.doc()?.preservation.blocks_save());
        // Back to the DXF: it keeps everything, and a report from an earlier save isn't left there.
        s.execute("saveas", &json!({"path": input}))?;
        assert!(!s.state()?.is_dirty());
        assert!(!dir.0.join("source.dxf.lost.json").exists());
        assert_foreign(&fs::read(input)?, variant, 5.)?;
    }
    Ok(())
}

/// Real saves and byte delivery keep edits, versions and binary choices.
#[test]
fn real_save_and_byte_delivery_keep_edits_versions_and_binary_choices() -> TestResult {
    install();
    let dir = TestDir::new()?;
    let input = dir.0.join("source.dxf");
    fs::write(&input, ff1::dxf_source(CustomRecords::Combined))?;
    let mut s = Session::empty();
    s.execute("open", &json!({"path": input}))?;
    for (cycle, binary) in [(1, false), (2, true)] {
        edit(&mut s);
        let target = dir.0.join(format!("cycle{cycle}.dxf"));
        let r = s.execute("saveas", &json!({"path": target, "version": 2007, "binary": binary}))?;
        assert!(r.get("notSaved").is_none());
        let bytes = fs::read(&target)?;
        assert_eq!(cadcraft_dxf::is_binary(&bytes), binary);
        let back = cadcraft_io::read(&bytes, "saved.dxf")?;
        assert_foreign(&bytes, CustomRecords::Combined, 5. * f64::from(cycle))?;
        assert_eq!(back.header.str("ACADVER", ""), "AC1021");
        let st = s.state()?;
        assert!(!st.is_dirty());
        assert!(Arc::ptr_eq(&st.saved, st.drawing()));
        assert_eq!(st.path.as_deref(), target.to_str());
        assert_eq!(st.title, format!("cycle{cycle}.dxf"));
        assert_eq!(st.binary_dxf, binary);
        let before = Before::capture(&s);
        let r = s.execute("document.bytes", &json!({"version": 2007, "binary": binary}))?;
        let delivered = base64_decode(r["data"].as_str().ok_or("missing delivered data")?).ok_or("invalid delivered base64")?;
        assert_eq!(delivered, bytes);
        before.unchanged(&s);
        s.execute("qsave", &json!({}))?;
        assert_eq!(fs::read(&target)?, bytes, "QSAVE must retain version and binary choice");
        // Reopen between edits: preserved foreign data must come from the saved bytes.
        s.execute("open", &json!({"path": target}))?;
    }

    // Ordinary DWG can save at an explicit version through the same hooks.
    let plain = cadcraft_io::read_dxf(b"0\nSECTION\n2\nENTITIES\n0\nLINE\n10\n1\n20\n2\n11\n11\n21\n12\n0\nENDSEC\n0\nEOF\n")?;
    s.open_drawing(plain, "plain.dxf", None);
    edit(&mut s);
    let before = Before::capture(&s);
    let r = s.execute("document.bytes", &json!({}))?;
    assert_line(&cadcraft_io::read(&base64_decode(r["data"].as_str().ok_or("missing data")?).ok_or("invalid base64")?, "plain.dxf")?, 5.);
    before.unchanged(&s);
    assert!(s.state()?.is_dirty(), "delivering bytes does not commit a save");
    let target = dir.0.join("plain.dwg");
    s.execute("saveas", &json!({"path": target, "version": 2004}))?;
    let bytes = fs::read(&target)?;
    assert_eq!(bytes.get(..6), Some(b"AC1018".as_slice()));
    assert_line(&cadcraft_io::read(&bytes, "plain.dwg")?, 5.);
    assert!(!s.state()?.is_dirty());
    let before = Before::capture(&s);
    let r = s.execute("document.bytes", &json!({"format": "dwg", "version": 2004}))?;
    let delivered = base64_decode(r["data"].as_str().ok_or("missing DWG data")?).ok_or("invalid DWG base64")?;
    assert_eq!(delivered.get(..6), Some(b"AC1018".as_slice()));
    assert_line(&cadcraft_io::read(&delivered, "plain.dwg")?, 5.);
    before.unchanged(&s);
    Ok(())
}

/// A storage failure keeps the drawing dirty, with its path, title and saved snapshot.
#[test]
fn real_storage_failure_keeps_dirty_path_title_and_saved_snapshot() -> TestResult {
    install();
    let dir = TestDir::new()?;
    let mut s = Session::new();
    s.cmdline("line 1,2 11,12")?;
    let before = Before::capture(&s);
    // Serialization succeeds; rename onto a directory fails after the atomic temp write.
    let target = dir.0.join("directory.dxf");
    fs::create_dir(&target)?;
    assert!(s.execute("saveas", &json!({"path": target, "version": 2007, "binary": true})).is_err());
    before.unchanged(&s);
    assert!(fs::read_dir(&target)?.next().is_none());
    assert_eq!(fs::read_dir(&dir.0)?.count(), 1, "atomic temporary file leaked");
    Ok(())
}
