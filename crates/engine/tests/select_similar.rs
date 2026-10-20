//! SELECTSIMILAR (with SELECTSIMILARMODE) and object isolation — ISOLATEOBJECTS, HIDEOBJECTS,
//! UNISOLATEOBJECTS (#428): JSON forms, typed forms, and hidden objects skipped by display and
//! selection.

use cadcraft_engine::doc::{Handle, Space};
use cadcraft_engine::geom::{Bounds2, Vec2};
use cadcraft_engine::{Session, render, select};
use serde_json::json;

/// Two lines and a circle on layer 0, a line and a circle on "Walls" (the last circle red).
fn drawing() -> (Session, Vec<Handle>) {
    let mut s = Session::new();
    s.execute("layer.new", &json!({ "name": "Walls" })).unwrap();
    s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    s.execute("line", &json!({ "points": [[0, 5], [10, 5]] })).unwrap();
    s.execute("circle", &json!({ "center": [20, 0], "radius": 2 })).unwrap();
    s.execute("line", &json!({ "points": [[0, 10], [10, 10]] })).unwrap();
    s.execute("circle", &json!({ "center": [30, 0], "radius": 3 })).unwrap();
    let hs: Vec<Handle> = s.doc().unwrap().model.iter().map(|e| e.handle).collect();
    s.execute("properties.set", &json!({ "handles": [hs[3].hex(), hs[4].hex()], "layer": "Walls" })).unwrap();
    s.execute("properties.set", &json!({ "handles": [hs[4].hex()], "color": 1 })).unwrap();
    s.set_selection(Vec::new());
    (s, hs)
}

fn sorted(mut v: Vec<Handle>) -> Vec<Handle> {
    v.sort();
    v
}

#[test]
fn selectsimilar_matches_type_and_mode_properties() {
    let (mut s, hs) = drawing();
    // Default SELECTSIMILARMODE 130: same type and layer (and name).
    let r = s.execute("selectsimilar", &json!({ "handles": [hs[0].hex()] })).unwrap();
    assert_eq!(r["selected"], 2);
    assert_eq!(sorted(s.selection()), vec![hs[0], hs[1]]);
    // Type only: every line.
    s.execute("selectsimilar", &json!({ "handles": [hs[0].hex()], "mode": 0 })).unwrap();
    assert_eq!(sorted(s.selection()), vec![hs[0], hs[1], hs[3]]);
    // Several references: objects like any of them. Colour only (1): the red circle stands alone.
    s.execute("selectsimilar", &json!({ "handles": [hs[0].hex(), hs[4].hex()], "mode": 1 })).unwrap();
    assert_eq!(sorted(s.selection()), vec![hs[0], hs[1], hs[3], hs[4]]);
    // The current selection is the default; the variable sets the default mode.
    s.execute("setvar", &json!({ "name": "SELECTSIMILARMODE", "value": 0 })).unwrap();
    s.set_selection(vec![hs[2]]);
    s.execute("selectsimilar", &json!({})).unwrap();
    assert_eq!(sorted(s.selection()), vec![hs[2], hs[4]]);
    // Hostile or empty input is an error, never a panic.
    s.set_selection(Vec::new());
    assert!(s.execute("selectsimilar", &json!({})).is_err());
    assert!(s.execute("selectsimilar", &json!({ "handles": ["FFFFFF"] })).is_err());
    for m in [json!(-1), json!(256), json!("x"), json!(1.5), json!(f64::MAX)] {
        assert!(s.execute("selectsimilar", &json!({ "handles": [hs[0].hex()], "mode": m })).is_err(), "{m}");
    }
    assert!(s.execute("setvar", &json!({ "name": "SELECTSIMILARMODE", "value": 999 })).is_err());
    assert!(s.running.is_none(), "JSON calls never prompt");
}

#[test]
fn selectsimilar_typed_with_settings_keyword() {
    let (mut s, hs) = drawing();
    s.cmdline("selectsimilar").unwrap();
    assert!(s.prompt_text().contains("Select objects or [SEttings]"), "{}", s.prompt_text());
    // SEttings asks for the bit code: 0 = type only.
    s.cmdline("se").unwrap();
    assert!(s.prompt_text().contains("SELECTSIMILARMODE"), "{}", s.prompt_text());
    s.cmdline("0").unwrap();
    assert_eq!(s.settings.selectsimilarmode, 0);
    s.cmdline("10,0").unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    assert_eq!(sorted(s.selection()), vec![hs[0], hs[1], hs[3]]);
    // With a pickfirst selection it selects at once.
    s.set_selection(vec![hs[2]]);
    s.cmdline("selectsimilar").unwrap();
    assert!(s.running.is_none());
    assert_eq!(sorted(s.selection()), vec![hs[2], hs[4]]);
}

/// Handles the display list draws something for.
fn drawn(s: &Session) -> usize {
    let d = s.doc().unwrap();
    render::build(d, &Space::Model, &render::Options::default()).prims.len()
}

#[test]
fn isolate_hide_and_unisolate() {
    let (mut s, hs) = drawing();
    let all_prims = drawn(&s);
    // ISOLATE: everything else is hidden — not drawn, not selectable, not in ALL.
    let r = s.execute("isolateobjects", &json!({ "handles": [hs[0].hex()] })).unwrap();
    assert_eq!(r["hidden"], 4);
    let d = s.doc().unwrap();
    assert!(d.is_hidden_object(hs[1]) && !d.is_hidden_object(hs[0]));
    assert!(drawn(&s) < all_prims);
    let everything = Bounds2::new(Vec2::new(-100.0, -100.0), Vec2::new(100.0, 100.0));
    assert_eq!(select::select_window(s.doc().unwrap(), &Space::Model, everything, true), vec![hs[0]]);
    assert!(select::pick(s.doc().unwrap(), &Space::Model, Vec2::new(5.0, 5.0), 0.5).is_none());
    // New objects show; HIDEOBJECTS hides more.
    s.execute("line", &json!({ "points": [[0, 20], [10, 20]] })).unwrap();
    let r = s.execute("hideobjects", &json!({ "handles": [hs[0].hex()] })).unwrap();
    assert_eq!(r["hidden"], 1);
    // Undo shows the hidden object again, redo hides it.
    s.execute("u", &json!({})).unwrap();
    assert!(!s.doc().unwrap().is_hidden_object(hs[0]));
    s.execute("redo", &json!({})).unwrap();
    assert!(s.doc().unwrap().is_hidden_object(hs[0]));
    // UNISOLATE shows everything again.
    let r = s.execute("unisolateobjects", &json!({})).unwrap();
    assert_eq!(r["shown"], 5);
    assert!(s.doc().unwrap().hidden_objects.is_empty());
    assert_eq!(drawn(&s), all_prims + 1);
    assert_eq!(s.execute("unisolateobjects", &json!({})).unwrap()["shown"], 0);
    // Nothing to isolate or hide is an error; test OBJECTISOLATIONMODE.
    assert!(s.execute("isolateobjects", &json!({})).is_err());
    assert!(s.execute("hideobjects", &json!({ "handles": "1A" })).is_err());
    assert!(s.execute("isolateobjects", &json!({ "handles": ["FFFFFF"] })).is_err());
    assert!(s.execute("setvar", &json!({ "name": "OBJECTISOLATIONMODE", "value": 2 })).is_err());
    s.execute("setvar", &json!({ "name": "OBJECTISOLATIONMODE", "value": 1 })).unwrap();
    assert_eq!(s.execute("getvar", &json!({ "name": "OBJECTISOLATIONMODE" })).unwrap()["value"], json!(1));
    s.execute("setvar", &json!({ "name": "OBJECTISOLATIONMODE", "value": 0 })).unwrap();
    assert_eq!(s.execute("getvar", &json!({ "name": "OBJECTISOLATIONMODE" })).unwrap()["value"], json!(0));
}

#[test]
fn isolate_typed_and_all_skips_hidden() {
    let (mut s, hs) = drawing();
    s.cmdline("hideobjects").unwrap();
    assert!(s.prompt_text().contains("Select objects"), "{}", s.prompt_text());
    s.cmdline("10,0").unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    assert!(s.doc().unwrap().is_hidden_object(hs[0]));
    assert!(s.selection().is_empty(), "hidden objects don't stay selected");
    // ALL at Select objects leaves hidden objects out.
    s.cmdline("isolateobjects").unwrap();
    s.cmdline("all").unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    assert_eq!(s.doc().unwrap().hidden_objects.len(), 1);
    s.cmdline("unisolateobjects").unwrap();
    assert!(s.doc().unwrap().hidden_objects.is_empty());
}

#[test]
fn select_by_class_and_isolate_toggle() {
    let (mut s, hs) = drawing();
    // Test CL / CLass keyword in prompt
    s.cmdline("erase").unwrap();
    s.cmdline("cl").unwrap();
    assert!(s.prompt_text().contains("Enter class name"), "{}", s.prompt_text());
    s.cmdline("CIRCLE").unwrap();
    s.cmdline("").unwrap();
    // Both circles were erased, all 3 lines remain
    assert_eq!(s.doc().unwrap().model.handles(), vec![hs[0], hs[1], hs[3]]);

    // Test isolateobjects.toggle: with no hidden objects, it runs isolateobjects
    let res = s.execute("isolateobjects.toggle", &json!({ "handles": [hs[0].hex()] })).unwrap();
    assert_eq!(res["hidden"], 2);

    // Now isolate with another object
    let (mut s2, hs2) = drawing();
    s2.execute("isolateobjects", &json!({ "handles": [hs2[0].hex()] })).unwrap();
    assert_eq!(s2.doc().unwrap().hidden_objects.len(), 4);
    // Toggle when objects are isolated runs unisolate
    let res2 = s2.execute("isolateobjects.toggle", &json!({})).unwrap();
    assert_eq!(res2["shown"], 4);
    assert!(s2.doc().unwrap().hidden_objects.is_empty());
}

#[test]
fn object_isolation_mode_persistence_roundtrip() {
    cadcraft_engine::cmd::file::set_io(cadcraft_engine::cmd::file::IoHooks {
        read: |bytes, path| cadcraft_io::read(bytes, path).map_err(|e| e.to_string()),
        write: |doc, path| cadcraft_io::write(doc, path).map_err(|e| e.to_string()),
        plot: None,
    });

    let (mut s, hs) = drawing();
    s.execute("isolateobjects", &json!({ "handles": [hs[0].hex()] })).unwrap();
    assert_eq!(s.doc().unwrap().hidden_objects.len(), 4);

    let tmp = std::env::temp_dir().join(format!("cadcraft-iso-test-{}.dxf", std::process::id()));
    let tmp_path = tmp.to_string_lossy().to_string();

    // Default OBJECTISOLATIONMODE = 0: save clears isolation in saved file
    s.execute("saveas", &json!({ "path": tmp_path })).unwrap();
    let bytes = std::fs::read(&tmp).unwrap();
    let read_doc0 = cadcraft_io::read(&bytes, &tmp_path).unwrap();
    assert!(read_doc0.hidden_objects.is_empty(), "OBJECTISOLATIONMODE 0 does not persist isolation");

    // OBJECTISOLATIONMODE = 1: save preserves hidden objects in file
    s.execute("setvar", &json!({ "name": "OBJECTISOLATIONMODE", "value": 1 })).unwrap();
    s.execute("saveas", &json!({ "path": tmp_path })).unwrap();
    let bytes1 = std::fs::read(&tmp).unwrap();
    let read_doc1 = cadcraft_io::read(&bytes1, &tmp_path).unwrap();
    assert_eq!(read_doc1.hidden_objects.len(), 4, "OBJECTISOLATIONMODE 1 persists isolation in file");

    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn selection_cycling_toggles_and_cycles_overlapping_entities() {
    let mut s = cadcraft_engine::Session::new();
    // Draw two overlapping circles at 0,0
    s.execute("circle", &json!({ "center": [0, 0], "radius": 5 })).unwrap();
    s.execute("circle", &json!({ "center": [0, 0], "radius": 5 })).unwrap();
    let hs = s.doc().unwrap().model.handles();
    assert_eq!(hs.len(), 2);

    // SELECTIONCYCLING is off by default
    assert_eq!(cadcraft_engine::sysvars::get(&s, "SELECTIONCYCLING"), Some(json!(0)));

    // With cycling off, idle_click on the rim repeatedly picks the same topmost circle
    s.idle_click(cadcraft_geom::Vec2::new(5.0, 0.0), false).unwrap();
    assert_eq!(s.selection(), vec![hs[1]]);
    s.idle_click(cadcraft_geom::Vec2::new(5.0, 0.0), false).unwrap();
    assert_eq!(s.selection(), vec![hs[1]]);

    // Turn SELECTIONCYCLING on (2 in AutoCAD)
    s.execute("setvar", &json!({ "name": "SELECTIONCYCLING", "value": 2 })).unwrap();
    assert_eq!(cadcraft_engine::sysvars::get(&s, "SELECTIONCYCLING"), Some(json!(2)));

    // First pick gets one, next pick cycles to the other overlapping circle
    s.idle_click(cadcraft_geom::Vec2::new(5.0, 0.0), false).unwrap();
    assert_eq!(s.selection(), vec![hs[0]]);
}
