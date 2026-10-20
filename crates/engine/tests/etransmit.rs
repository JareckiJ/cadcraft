//! Real ZIP/DXF round trips: relocated dependencies, snapshots, failure cleanup and limits.
#![cfg(not(target_arch = "wasm32"))]

use std::{fs, io::Read, path::Path, sync::Arc};

use cadcraft_doc::{Block, Common, DataLink, Drawing, EntityKind, HVal, Image, Space};
use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use serde_json::{Value, json};
use zip::ZipArchive;

fn session(d: Drawing, path: Option<&Path>) -> Session {
    set_io(IoHooks {
        read: |b, n| cadcraft_io::read(b, n).map_err(|e| e.to_string()),
        write: |d, n| {
            if d.header.get("TRANSMIT_TEST_FAIL").is_some() {
                return Err("test writer failed".into());
            }
            cadcraft_io::write(d, n).map_err(|e| e.to_string())
        },
        plot: None,
    });
    let mut s = Session::new();
    s.open_drawing(d, "Panel", path.map(|p| p.to_str().unwrap().into()));
    s
}

fn save(d: &Drawing, path: &Path) {
    fs::write(path, cadcraft_io::write(d, path.to_str().unwrap()).unwrap()).unwrap();
}

fn xref(d: &mut Drawing, name: &str, path: &str) {
    let mut b = Block::new(name);
    b.xref_path = Some(path.into());
    d.blocks.insert(name.into(), Arc::new(b));
}

fn image(d: &mut Drawing, space: &Space, path: &str) {
    d.add(space, Common::default(), EntityKind::Image(Image { path: path.into(), ..Image::default() })).unwrap();
}

fn unpack(path: &Path, dest: &Path) -> Value {
    let mut zip = ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let mut names = std::collections::BTreeSet::new();
    for i in 0..zip.len() {
        let f = zip.by_index(i).unwrap();
        assert!(f.enclosed_name().is_some());
        assert!(names.insert(f.name().to_ascii_lowercase()));
        assert!(!f.name().contains('\\'));
    }
    let mut text = String::new();
    zip.by_name("manifest.json").unwrap().read_to_string(&mut text).unwrap();
    zip.extract(dest).unwrap();
    serde_json::from_str(&text).unwrap()
}

#[test]
fn packages_unsaved_content_without_changing_session_or_source() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("panel.dxf");
    save(&Drawing::new_imperial(), &source);
    let original = fs::read(&source).unwrap();
    let mut s = session(Drawing::new_imperial(), Some(&source));
    s.execute("circle", &json!({"center": [3, 4], "radius": 5})).unwrap();
    let before = s.doc().unwrap().clone();
    let undo = s.state().unwrap().undo.len();
    let dirty = s.state().unwrap().is_dirty();
    let path = dir.path().join("package.zip");
    let r = s.execute("etransmit", &json!({"path": path})).unwrap();
    assert_eq!(r["complete"], true);
    let dest = dir.path().join("unpacked");
    let manifest = unpack(&path, &dest);
    assert_eq!(manifest["files"], r["files"]);
    assert_eq!(manifest["drawing"], "panel.dxf");
    let d = cadcraft_io::read(&fs::read(dest.join("panel.dxf")).unwrap(), "panel.dxf").unwrap();
    assert!(d.model.iter().any(|e| matches!(e.kind, EntityKind::Circle(_))));
    assert_eq!(s.doc().unwrap(), &before);
    assert_eq!(s.state().unwrap().undo.len(), undo);
    assert_eq!(s.state().unwrap().is_dirty(), dirty);
    assert_eq!(s.state().unwrap().path.as_deref(), source.to_str());
    assert_eq!(fs::read(source).unwrap(), original);
}

#[test]
fn nested_xrefs_cycles_images_and_csv_survive_relocation() {
    let dir = tempfile::tempdir().unwrap();
    let sources = dir.path().join("sources");
    fs::create_dir_all(sources.join("nested")).unwrap();
    fs::create_dir_all(sources.join("other")).unwrap();
    fs::write(sources.join("picture.png"), b"original synthetic picture").unwrap();
    fs::write(sources.join("other/picture.png"), b"different synthetic picture").unwrap();
    fs::write(sources.join("parts.csv"), b"Part,Qty\nRelay,2\n").unwrap();
    let root_path = sources.join("panel.dxf");
    let child_path = sources.join("nested/child.dxf");
    let mut child = Drawing::new_imperial();
    xref(&mut child, "ROOT", "../panel.dxf");
    image(&mut child, &Space::Model, "../picture.png");
    save(&child, &child_path);
    let mut root = Drawing::new_imperial();
    xref(&mut root, "CHILD", r"nested\child.dxf");
    xref(&mut root, "DUPLICATE", child_path.to_str().unwrap());
    image(&mut root, &Space::Model, "picture.png");
    let paper = Space::Paper(root.layouts.first().unwrap().name.clone());
    image(&mut root, &paper, "other/picture.png");
    let mut symbol = Block::new("SYMBOL");
    let mut bdoc = Drawing::new_imperial();
    image(&mut bdoc, &Space::Model, "picture.png");
    symbol.entities = bdoc.model;
    root.blocks.insert("SYMBOL".into(), Arc::new(symbol));
    save(&root, &root_path);
    let child_original = fs::read(&child_path).unwrap();
    let mut s = session(root, Some(&root_path));
    let table = s.execute("table", &json!({"at": [0, 0], "rows": 1, "cols": 2})).unwrap();
    let h = cadcraft_engine::cmd::curves::handle_value(&table["handle"]).unwrap();
    s.doc_mut()
        .unwrap()
        .modify_entity(h, |e| {
            if let EntityKind::Table(t) = &mut e.kind {
                t.data_link = Some(DataLink { path: "parts.csv".into(), start_row: 0 });
            }
        })
        .unwrap();
    let path = dir.path().join("package.zip");
    let r = s.execute("etransmit", &json!({"path": path, "strict": true})).unwrap();
    assert_eq!(r["files"].as_array().unwrap().len(), 5, "root, child, two pictures, CSV: {r}");
    assert_eq!(fs::read(&child_path).unwrap(), child_original);
    let dest = dir.path().join("recipient");
    unpack(&path, &dest);
    fs::remove_dir_all(&sources).unwrap();
    let root = cadcraft_io::read(&fs::read(dest.join("panel.dxf")).unwrap(), "panel.dxf").unwrap();
    let child_name = root.block("CHILD").unwrap().xref_path.as_ref().unwrap();
    assert_eq!(root.block("DUPLICATE").unwrap().xref_path.as_ref().unwrap(), child_name);
    let child_file = dest.join(child_name);
    let child = cadcraft_io::read(&fs::read(&child_file).unwrap(), "child.dxf").unwrap();
    let back = child_file.parent().unwrap().join(child.block("ROOT").unwrap().xref_path.as_ref().unwrap());
    assert_eq!(fs::canonicalize(back).unwrap(), fs::canonicalize(dest.join("panel.dxf")).unwrap());
    for (d, folder) in [(&root, dest.as_path()), (&child, child_file.parent().unwrap())] {
        let stores = std::iter::once(&d.model).chain(d.layouts.iter().map(|l| &l.entities)).chain(d.blocks.values().map(|b| &b.entities));
        for e in stores.flat_map(|s| s.iter()) {
            match &e.kind {
                EntityKind::Image(i) => assert!(folder.join(&i.path).is_file(), "{}", i.path),
                EntityKind::Table(t) => {
                    if let Some(link) = &t.data_link {
                        assert_eq!(fs::read(folder.join(&link.path)).unwrap(), b"Part,Qty\nRelay,2\n");
                    }
                }
                _ => {}
            }
        }
    }
}

#[test]
fn missing_refused_and_invalid_files_are_reported_and_strict_is_atomic() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = Drawing::new_imperial();
    xref(&mut d, "MISSING", "gone.dxf");
    xref(&mut d, "BROKEN", "broken.dxf");
    fs::write(dir.path().join("broken.dxf"), b"not a drawing").unwrap();
    image(&mut d, &Space::Model, "file://192.0.2.1/share/image.png");
    let mut s = session(d, Some(&dir.path().join("root.dxf")));
    let strict = dir.path().join("strict.zip");
    assert!(s.execute("etransmit", &json!({"path": strict, "strict": true})).is_err());
    assert!(!strict.exists());
    let out = dir.path().join("partial.zip");
    let r = s.execute("etransmit", &json!({"path": out})).unwrap();
    assert_eq!(r["complete"], false);
    assert_eq!(r["missing"].as_array().unwrap().len(), 3);
    assert!(r["missing"].as_array().unwrap().iter().any(|m| m["reason"].as_str().unwrap().contains("path not followed")));
    assert_eq!(unpack(&out, &dir.path().join("unpacked"))["missing"], r["missing"]);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 3, "no staging file remains");
}

#[test]
fn refuses_overwrite_and_bad_parameters() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = session(Drawing::new_imperial(), None);
    let out = dir.path().join("existing.zip");
    fs::write(&out, b"keep me").unwrap();
    assert!(s.execute("etransmit", &json!({"path": out})).is_err());
    assert_eq!(fs::read(&out).unwrap(), b"keep me");
    let target = dir.path().join("new.zip");
    for p in [
        json!({}),
        json!({"path": 4}),
        json!({"path": target, "strict": 1}),
        json!({"path": target, "drawing": "../escape.dxf"}),
        json!({"path": target, "drawing": "..\\escape.dxf"}),
        json!({"path": target, "drawing": "drawing.png"}),
        json!({"path": target, "bogus": true}),
        json!({"path": dir.path().join("a.dxf")}),
    ] {
        assert!(s.execute("etransmit", &p).is_err(), "{p}");
    }
    assert!(!target.exists());
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn oversized_dependency_and_hostile_paths_are_bounded() {
    let dir = tempfile::tempdir().unwrap();
    fs::File::create(dir.path().join("big.png")).unwrap().set_len((64 << 20) + 1).unwrap();
    let mut d = Drawing::new_imperial();
    image(&mut d, &Space::Model, "big.png");
    image(&mut d, &Space::Model, &"x".repeat(4097));
    let mut s = session(d, Some(&dir.path().join("root.dxf")));
    let r = s.execute("etransmit", &json!({"path": dir.path().join("package.zip")})).unwrap();
    assert_eq!(r["missing"].as_array().unwrap().len(), 2);
    assert!(r["missing"][0]["reason"].as_str().unwrap().contains("limit"));
    assert!(r["missing"][1]["reason"].as_str().unwrap().contains("4096"));
}

#[test]
fn writer_failure_cleans_up_staging_file() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = Drawing::new_imperial();
    d.header.set("TRANSMIT_TEST_FAIL", HVal::Int(1));
    let mut s = session(d, None);
    assert!(s.execute("etransmit", &json!({"path": dir.path().join("package.zip")})).is_err());
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn command_line_prompts_and_cancel_work() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = session(Drawing::new_imperial(), None);
    s.cmdline("ETRANSMIT").unwrap();
    assert!(s.current_prompt().unwrap().message.contains("ZIP"));
    s.input(cadcraft_engine::Input::Cancel).unwrap();
    assert!(s.running.is_none());
    let path = dir.path().join("typed.zip");
    s.cmdline("ETRANSMIT").unwrap();
    s.cmdline(path.to_str().unwrap()).unwrap();
    assert!(path.is_file());
    assert!(s.running.is_none());
}

#[test]
fn dwg_root_round_trips_and_read_warnings_fail_strict_mode() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = session(Drawing::new_imperial(), None);
    s.execute("circle", &json!({"center": [0, 0], "radius": 5})).unwrap();
    let path = dir.path().join("dwg.zip");
    let r = s.execute("etransmit", &json!({"path": path, "drawing": "panel.dwg", "strict": true})).unwrap();
    assert_eq!(r["drawing"], "panel.dwg");
    let dest = dir.path().join("unpacked");
    unpack(&path, &dest);
    let bytes = fs::read(dest.join("panel.dwg")).unwrap();
    assert!(bytes.starts_with(b"AC10"));
    let d = cadcraft_io::read(&bytes, "panel.dwg").unwrap();
    assert!(d.model.iter().any(|e| matches!(e.kind, EntityKind::Circle(_))));
    s.doc_mut().unwrap().read_notes.push("one unsupported object was left out".into());
    let strict = dir.path().join("strict.zip");
    assert!(s.execute("etransmit", &json!({"path": strict, "strict": true})).is_err());
    assert!(!strict.exists());
    let r = s.execute("etransmit", &json!({"path": dir.path().join("partial.zip")})).unwrap();
    assert_eq!(r["complete"], false);
    assert_eq!(r["warnings"].as_array().unwrap().len(), 1);
}

#[test]
fn packaging_during_bedit_uses_the_committed_host_drawing() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = session(Drawing::new_imperial(), None);
    let circle = s.execute("circle", &json!({"center": [0, 0], "radius": 2})).unwrap();
    s.execute("block", &json!({"name": "SYMBOL", "base": [0, 0], "handles": [circle["handle"]]})).unwrap();
    s.execute("bedit", &json!({"name": "SYMBOL"})).unwrap();
    s.execute("circle", &json!({"center": [0, 0], "radius": 7})).unwrap();
    let before = s.doc().unwrap().clone();
    let path = dir.path().join("package.zip");
    s.execute("etransmit", &json!({"path": path})).unwrap();
    let dest = dir.path().join("unpacked");
    unpack(&path, &dest);
    let d = cadcraft_io::read(&fs::read(dest.join("Drawing.dxf")).unwrap(), "Drawing.dxf").unwrap();
    assert_eq!(d.block("SYMBOL").unwrap().entities.len(), 1);
    assert_eq!(s.doc().unwrap(), &before);
    assert_eq!(s.state().unwrap().block_editor(), Some("SYMBOL"));
}

#[test]
fn deeply_nested_xrefs_are_reported_and_reference_floods_abort_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    for i in 0..=cadcraft_doc::MAX_BLOCK_DEPTH {
        let mut d = Drawing::new_imperial();
        xref(&mut d, "NEXT", &format!("{}.dxf", i + 1));
        save(&d, &dir.path().join(format!("{i}.dxf")));
    }
    let mut root = Drawing::new_imperial();
    xref(&mut root, "FIRST", "0.dxf");
    let mut s = session(root, Some(&dir.path().join("root.dxf")));
    let r = s.execute("etransmit", &json!({"path": dir.path().join("deep.zip")})).unwrap();
    assert_eq!(r["complete"], false);
    assert_eq!(r["missing"].as_array().unwrap().len(), 1);
    assert!(r["missing"][0]["reason"].as_str().unwrap().contains("too deep"));

    let out = tempfile::tempdir().unwrap();
    let mut d = Drawing::new_imperial();
    for i in 0..10_001 {
        xref(&mut d, &format!("REF{i}"), "missing.dxf");
    }
    let mut s = session(d, None);
    assert!(s.execute("etransmit", &json!({"path": out.path().join("flood.zip")})).is_err());
    assert_eq!(fs::read_dir(out.path()).unwrap().count(), 0);
}
