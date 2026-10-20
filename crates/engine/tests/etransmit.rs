//! Real ZIP/DXF round trips: relocated dependencies, snapshots, failure cleanup and limits.
#![cfg(not(target_arch = "wasm32"))]

use std::{fs, io::Read, path::Path, sync::Arc};

use cadcraft_doc::{Block, Common, DataLink, Drawing, EntityKind, HVal, Image, Space};
use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use serde_json::{Value, json};
use zip::ZipArchive;

/// Install real format hooks and open a drawing without triggering xref loading.
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

/// Write synthetic source drawings through the production format writer.
fn save(d: &Drawing, path: &Path) {
    fs::write(path, cadcraft_io::write(d, path.to_str().unwrap()).unwrap()).unwrap();
}

/// Declare an external reference without reading any source file.
fn xref(d: &mut Drawing, name: &str, path: &str) {
    let mut b = Block::new(name);
    b.xref_path = Some(path.into());
    d.blocks.insert(name.into(), Arc::new(b));
}

/// Create image metadata independently of whether its source is a valid image.
fn image(d: &mut Drawing, space: &Space, path: &str) {
    d.add(space, Common::default(), EntityKind::Image(Image { path: path.into(), ..Image::default() })).unwrap();
}

/// Generate original PNG fixtures that exercise the real decoder rather than arbitrary payloads.
fn png(path: &Path, color: [u8; 3]) {
    image::RgbImage::from_pixel(1, 1, image::Rgb(color)).save(path).unwrap();
}

/// Attach source metadata without loading its bytes into table cells.
fn csv_link(s: &mut Session, path: &str) {
    let t = s.execute("table", &json!({"at": [0, 0], "rows": 1, "cols": 1})).unwrap();
    let h = cadcraft_engine::cmd::curves::handle_value(&t["handle"]).unwrap();
    s.doc_mut()
        .unwrap()
        .modify_entity(h, |e| {
            if let EntityKind::Table(t) = &mut e.kind {
                t.data_link = Some(DataLink { path: path.into(), start_row: 0 });
            }
        })
        .unwrap();
}

/// Verify safe, case-distinct archive paths before extracting a generated package.
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

/// Packaging uses unsaved geometry while preserving source bytes and session state.
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

/// A package remains usable after its authorized originals are deleted.
#[test]
fn nested_xrefs_cycles_images_and_csv_survive_relocation() {
    let dir = tempfile::tempdir().unwrap();
    let sources = dir.path().join("sources");
    fs::create_dir_all(sources.join("nested")).unwrap();
    fs::create_dir_all(sources.join("other")).unwrap();
    png(&sources.join("picture.png"), [12, 34, 56]);
    png(&sources.join("other/picture.png"), [65, 43, 21]);
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
    let r = s.execute("etransmit", &json!({"path": path, "strict": true, "allow": [sources]})).unwrap();
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

/// Missing, malformed and refused references cannot cause partial strict publication.
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
    assert!(s.execute("etransmit", &json!({"path": strict, "strict": true, "allow": [dir.path()]})).is_err());
    assert!(!strict.exists());
    let out = dir.path().join("partial.zip");
    let r = s.execute("etransmit", &json!({"path": out, "allow": [dir.path()]})).unwrap();
    assert_eq!(r["complete"], false);
    assert_eq!(r["missing"].as_array().unwrap().len(), 3);
    assert!(r["missing"].as_array().unwrap().iter().any(|m| m["reason"].as_str().unwrap().contains("path not followed")));
    assert_eq!(unpack(&out, &dir.path().join("unpacked"))["missing"], r["missing"]);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 3, "no staging file remains");
}

/// Invalid requests and existing destinations leave no staging files or overwritten content.
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
        json!({"path": target, "allow": "all"}),
        json!({"path": target, "allow": [0]}),
        json!({"path": target, "allow": [dir.path().join("missing")]}),
        json!({"path": target, "allow": vec![dir.path(); 129]}),
        json!({"path": dir.path().join("a.dxf")}),
    ] {
        assert!(s.execute("etransmit", &p).is_err(), "{p}");
    }
    assert!(!target.exists());
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

/// Oversized assets and excessive path lengths become bounded diagnostics.
#[test]
fn oversized_dependency_and_hostile_paths_are_bounded() {
    let dir = tempfile::tempdir().unwrap();
    fs::File::create(dir.path().join("big.png")).unwrap().set_len((64 << 20) + 1).unwrap();
    let mut d = Drawing::new_imperial();
    image(&mut d, &Space::Model, "big.png");
    image(&mut d, &Space::Model, &"x".repeat(4097));
    let mut s = session(d, Some(&dir.path().join("root.dxf")));
    let r = s.execute("etransmit", &json!({"path": dir.path().join("package.zip"), "allow": [dir.path()]})).unwrap();
    assert_eq!(r["missing"].as_array().unwrap().len(), 2);
    assert!(r["missing"][0]["reason"].as_str().unwrap().contains("limit"));
    assert!(r["missing"][1]["reason"].as_str().unwrap().contains("4096"));
}

/// A format-hook failure removes all unpublished output.
#[test]
fn writer_failure_cleans_up_staging_file() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = Drawing::new_imperial();
    d.header.set("TRANSMIT_TEST_FAIL", HVal::Int(1));
    let mut s = session(d, None);
    assert!(s.execute("etransmit", &json!({"path": dir.path().join("package.zip")})).is_err());
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}

/// Interactive export approval is separate from naming the package; Enter grants no access.
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
    assert!(s.current_prompt().unwrap().message.contains("approved"));
    s.cmdline("").unwrap();
    assert!(path.is_file());
    assert!(s.running.is_none());
}

/// DWG snapshots reopen, and prior read losses cannot pass strict packaging.
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

/// Packaging during block editing does not publish an unfinished block definition.
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

/// Depth and reference budgets stop hostile graphs without publishing partial output.
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
    let r = s.execute("etransmit", &json!({"path": dir.path().join("deep.zip"), "allow": [dir.path()]})).unwrap();
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

/// Long stems must not cause valid xrefs, assets or an explicitly chosen root to lose their type.
#[test]
fn long_names_keep_extensions_for_root_xrefs_images_and_csv() {
    let dir = tempfile::tempdir().unwrap();
    let child_name = format!("{}.dxf", "child".repeat(35));
    let image_name = format!("{}.png", "picture".repeat(25));
    let csv_name = format!("{}.csv", "parts".repeat(35));
    let child = dir.path().join(&child_name);
    let picture = dir.path().join(&image_name);
    let csv = dir.path().join(&csv_name);
    save(&Drawing::new_imperial(), &child);
    png(&picture, [10, 20, 30]);
    fs::write(&csv, b"Part,Qty\nRelay,2\n").unwrap();
    let mut d = Drawing::new_imperial();
    xref(&mut d, "CHILD", &child_name);
    image(&mut d, &Space::Model, &image_name);
    let mut s = session(d, Some(&dir.path().join("root.dxf")));
    csv_link(&mut s, &csv_name);
    let out = dir.path().join("package.zip");
    let r = s
        .execute(
            "etransmit",
            &json!({
                "path": out, "drawing": format!("{}.dxf", "panel".repeat(35)),
                "allow": [child, picture, csv], "strict": true,
            }),
        )
        .unwrap();
    assert_eq!(r["complete"], true);
    let root = r["drawing"].as_str().unwrap();
    assert!(root.ends_with(".dxf") && root.len() <= 124);
    let dest = dir.path().join("unpacked");
    let manifest = unpack(&out, &dest);
    assert_eq!(manifest["files"].as_array().unwrap().len(), 4);
    for f in manifest["files"].as_array().unwrap() {
        let name = f["path"].as_str().unwrap();
        let ext = Path::new(name).extension().unwrap().to_str().unwrap();
        match f["kind"].as_str().unwrap() {
            "drawing" => {
                assert_eq!(ext, "dxf");
                cadcraft_io::read(&fs::read(dest.join(name)).unwrap(), name).unwrap();
            }
            "image" => {
                assert_eq!(ext, "png");
                image::open(dest.join(name)).unwrap();
            }
            "dataLink" => {
                assert_eq!(ext, "csv");
                assert_eq!(fs::read(dest.join(name)).unwrap(), b"Part,Qty\nRelay,2\n");
            }
            other => panic!("unexpected kind {other}"),
        }
    }
}

/// Published ZIPs follow ordinary-save umask permissions, while restrictive umasks remain private.
#[cfg(unix)]
#[test]
fn published_zip_uses_ordinary_save_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let control = dir.path().join("ordinary-save");
    fs::write(&control, b"ordinary save permissions").unwrap();
    let out = dir.path().join("package.zip");
    session(Drawing::new_imperial(), None).execute("etransmit", &json!({"path": out})).unwrap();
    assert_eq!(fs::metadata(out).unwrap().permissions().mode() & 0o777, fs::metadata(control).unwrap().permissions().mode() & 0o777);
}

/// A supplied drawing cannot export even valid co-located assets or drawings without caller consent.
#[test]
fn unapproved_sources_are_not_exported_and_file_approval_does_not_cover_siblings() {
    let dir = tempfile::tempdir().unwrap();
    let picture = dir.path().join("private.png");
    let csv = dir.path().join("private.csv");
    png(&picture, [10, 20, 30]);
    let secret = b"private,please-do-not-share\n";
    fs::write(&csv, secret).unwrap();
    save(&Drawing::new_imperial(), &dir.path().join("private.dxf"));
    let mut d = Drawing::new_imperial();
    xref(&mut d, "PRIVATE", "private.dxf");
    image(&mut d, &Space::Model, "private.png");
    let mut s = session(d, Some(&dir.path().join("root.dxf")));
    csv_link(&mut s, "private.csv");
    let out = dir.path().join("unapproved.zip");
    let r = s.execute("etransmit", &json!({"path": out})).unwrap();
    assert_eq!(r["files"].as_array().unwrap().len(), 1);
    assert_eq!(r["missing"].as_array().unwrap().len(), 3);
    assert!(r["missing"].as_array().unwrap().iter().all(|m| m["reason"].as_str().unwrap().contains("export not authorized")));
    let mut zip = ZipArchive::new(fs::File::open(&out).unwrap()).unwrap();
    for i in 0..zip.len() {
        let mut bytes = Vec::new();
        zip.by_index(i).unwrap().read_to_end(&mut bytes).unwrap();
        assert!(!bytes.windows(secret.len()).any(|w| w == secret));
    }
    let strict = dir.path().join("strict.zip");
    assert!(s.execute("etransmit", &json!({"path": strict, "strict": true})).is_err());
    assert!(!strict.exists());
    let r = s.execute("etransmit", &json!({"path": dir.path().join("approved.zip"), "allow": [csv]})).unwrap();
    assert_eq!(r["files"].as_array().unwrap().len(), 2);
    assert_eq!(r["missing"].as_array().unwrap().len(), 2);
    assert_eq!(r["files"][0]["kind"], "dataLink");
}

/// Approval authorizes a source location, but never bypasses asset type or decoder limits.
#[test]
fn authorized_assets_are_validated_before_original_bytes_are_archived() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("secret.png"), b"private,not-an-image\n").unwrap();
    fs::write(dir.path().join("truncated.png"), b"\x89PNG\r\n\x1a\n").unwrap();
    fs::write(dir.path().join("binary.csv"), b"private\0binary").unwrap();
    fs::write(dir.path().join("not-csv.txt"), b"not a CSV source").unwrap();
    png(&dir.path().join("wrong.jpg.png"), [1, 2, 3]);
    fs::rename(dir.path().join("wrong.jpg.png"), dir.path().join("wrong.jpg")).unwrap();
    image::RgbImage::from_pixel(1, 16_385, image::Rgb([0, 0, 0])).save(dir.path().join("huge.png")).unwrap();
    let mut d = Drawing::new_imperial();
    for name in ["secret.png", "truncated.png", "wrong.jpg", "huge.png"] {
        image(&mut d, &Space::Model, name);
    }
    png(&dir.path().join("valid.png"), [4, 5, 6]);
    image(&mut d, &Space::Model, "valid.png");
    let mut s = session(d, Some(&dir.path().join("root.dxf")));
    csv_link(&mut s, "binary.csv");
    csv_link(&mut s, "not-csv.txt");
    // Having copied this source as an image must not bypass CSV validation for another reference.
    csv_link(&mut s, "valid.png");
    let r = s.execute("etransmit", &json!({"path": dir.path().join("package.zip"), "allow": [dir.path()]})).unwrap();
    assert_eq!(r["files"].as_array().unwrap().len(), 2);
    assert_eq!(r["missing"].as_array().unwrap().len(), 7);
    assert!(r["missing"].as_array().unwrap().iter().all(|m| !m["reason"].as_str().unwrap().contains("not authorized")));
}

/// Canonical authorization rejects a symlink escaping an approved source folder.
#[cfg(unix)]
#[test]
fn source_allowlist_does_not_follow_symlinks_outside_an_approved_folder() {
    let dir = tempfile::tempdir().unwrap();
    let public = dir.path().join("public");
    fs::create_dir(&public).unwrap();
    let secret = dir.path().join("private.png");
    png(&secret, [5, 6, 7]);
    std::os::unix::fs::symlink(&secret, public.join("alias.png")).unwrap();
    let mut d = Drawing::new_imperial();
    image(&mut d, &Space::Model, "alias.png");
    let mut s = session(d, Some(&public.join("root.dxf")));
    let r = s.execute("etransmit", &json!({"path": dir.path().join("denied.zip"), "allow": [public]})).unwrap();
    assert_eq!(r["files"].as_array().unwrap().len(), 1);
    assert!(r["missing"][0]["reason"].as_str().unwrap().contains("not authorized"));
    let r = s.execute("etransmit", &json!({"path": dir.path().join("allowed.zip"), "allow": [secret], "strict": true})).unwrap();
    assert_eq!(r["complete"], true);
}

/// The interactive source prompt can explicitly approve a project folder for packaging.
#[test]
fn interactive_source_approval_includes_a_valid_image() {
    let dir = tempfile::tempdir().unwrap();
    png(&dir.path().join("picture.png"), [3, 4, 5]);
    let mut d = Drawing::new_imperial();
    image(&mut d, &Space::Model, "picture.png");
    let mut s = session(d, Some(&dir.path().join("root.dxf")));
    let out = dir.path().join("typed.zip");
    s.cmdline("ETRANSMIT").unwrap();
    s.cmdline(out.to_str().unwrap()).unwrap();
    assert!(!out.exists());
    s.cmdline(dir.path().to_str().unwrap()).unwrap();
    let r = unpack(&out, &dir.path().join("recipient"));
    assert_eq!(r["complete"], true);
    assert_eq!(r["files"].as_array().unwrap().len(), 2);
}

/// Valid JPEG files remain supported by the same bounded image validation as PNG files.
#[test]
fn authorized_jpeg_keeps_its_original_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let picture = dir.path().join("picture.jpg");
    image::RgbImage::from_pixel(2, 3, image::Rgb([20, 40, 60])).save(&picture).unwrap();
    let original = fs::read(&picture).unwrap();
    let mut d = Drawing::new_imperial();
    image(&mut d, &Space::Model, "picture.jpg");
    let mut s = session(d, Some(&dir.path().join("root.dxf")));
    let out = dir.path().join("package.zip");
    let r = s.execute("etransmit", &json!({"path": out, "allow": [picture], "strict": true})).unwrap();
    let mut zip = ZipArchive::new(fs::File::open(out).unwrap()).unwrap();
    let mut bytes = Vec::new();
    zip.by_name(r["files"][0]["path"].as_str().unwrap()).unwrap().read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, original);
}
