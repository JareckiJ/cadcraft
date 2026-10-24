//! Integration tests for the WBLOCK dialog's confirm() path: verify that the file is written and
//! contains the expected entities for Entire Drawing and Objects modes.

use cadcraft_doc::{Common, Drawing, EntityKind, Line, Space};
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::Session;
use cadcraft_geom::Vec3;
use cadcraft_ui_egui::{CadApp, Services, wblock};
use serde_json::json;

fn setup_io() {
    // Writes a minimal JSON blob so tests can read it back without a real DXF codec.
    set_io(IoHooks {
        read: |_, _| Ok(Drawing::new_imperial()),
        write: |d, _| {
            let model_n = d.model.len();
            let layout_n: usize = d.layouts.iter().map(|l| l.entities.len()).sum();
            let insbase = d.header.point("INSBASE").unwrap_or_default();
            Ok(serde_json::json!({
                "model": model_n,
                "layouts": layout_n,
                "insbase": [insbase.x, insbase.y],
            })
            .to_string()
            .into_bytes())
        },
        plot: None,
    });
}

fn app_with_two_entities() -> (CadApp, cadcraft_doc::Handle, cadcraft_doc::Handle) {
    let mut s = Session::new();
    let r1 = s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    let h1 = cadcraft_doc::Handle::parse_hex(r1["handles"][0].as_str().unwrap()).unwrap();
    let r2 = s.execute("line", &json!({ "points": [[0, 5], [10, 5]] })).unwrap();
    let h2 = cadcraft_doc::Handle::parse_hex(r2["handles"][0].as_str().unwrap()).unwrap();
    // Add one paper-space entity to verify the whole-drawing mode writes layouts too.
    s.doc_mut()
        .unwrap()
        .add(
            &Space::Paper("Layout1".into()),
            Common::default(),
            EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(10.0, 0.0, 0.0) }),
        )
        .unwrap();
    s.set_selection(Vec::new());
    (CadApp::new(s, Services::default()), h1, h2)
}

fn out_path(label: &str) -> String {
    std::env::temp_dir()
        .join(format!("cadcraft-wblock-{label}-{}.json", std::process::id()))
        .to_string_lossy()
        .into_owned()
}

fn read_json(path: &str) -> serde_json::Value {
    let bytes = std::fs::read(path).expect("wblock should have written the file");
    serde_json::from_slice(&bytes).expect("write hook should produce valid JSON")
}

#[test]
fn confirm_entire_drawing_writes_all_model_and_layout_entities() {
    setup_io();
    let (mut app, _h1, _h2) = app_with_two_entities();
    let path = out_path("entire");
    app.wblock.source = wblock::Source::EntireDrawing;
    app.wblock.path = path.clone();
    // Exercise confirm() — the same code path the dialog's OK button takes.
    assert!(wblock::confirm(&mut app), "confirm() should succeed: {:?}", app.wblock.error);
    let j = read_json(&path);
    assert_eq!(j["model"].as_u64(), Some(2), "entire-drawing mode must write all model entities");
    assert_eq!(j["layouts"].as_u64(), Some(1), "entire-drawing mode must write layout entities");
}

#[test]
fn confirm_objects_with_base_point_writes_only_selected_entity() {
    setup_io();
    let (mut app, h1, _h2) = app_with_two_entities();
    let path = out_path("objects");
    // Only h1 is selected; base point at (5, 3).
    app.wblock.source = wblock::Source::Objects;
    app.wblock.picked_handles = vec![h1.hex()];
    app.wblock.base_x = "5".into();
    app.wblock.base_y = "3".into();
    app.wblock.path = path.clone();
    // Exercise confirm() — the same code path the dialog's OK button takes.
    assert!(wblock::confirm(&mut app), "confirm() should succeed: {:?}", app.wblock.error);
    let j = read_json(&path);
    assert_eq!(j["model"].as_u64(), Some(1), "objects mode must write only the selected handle");
    // INSBASE is reset to 0,0 in objects mode (the base subtraction is the offset).
    let insbase = j["insbase"].as_array().unwrap();
    assert_eq!((insbase[0].as_f64(), insbase[1].as_f64()), (Some(0.0), Some(0.0)));
}
