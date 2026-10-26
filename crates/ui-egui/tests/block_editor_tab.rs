//! The Block Editor contextual Ribbon tab: it opens by itself with the Block Editor, its buttons
//! save and close the block, and the previous tab comes back once the Block Editor closes.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services, ribbon_bedit, ribbon_hatch};
use egui::vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

fn harness() -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.run("new", json!({})).unwrap();
    app.run("ui.layout.pc", json!({})).unwrap();
    app.ui.start_tab = false;
    app.ui.in_window_menu = false;
    app.integrated_titlebar = true;
    let r = app.run("rectang", json!({ "p1": [0, 0], "p2": [20, 10] })).unwrap();
    app.run("block", json!({ "name": "Box", "base": [0, 0], "handles": [r["handle"]] })).unwrap();
    app.ui.ribbon_tab = "annotate".into();
    let mut harness = Harness::builder().with_size(vec2(1500.0, 800.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    harness.run_steps(3);
    harness
}

fn open_box(h: &mut Harness<'static, CadApp>) {
    h.state_mut().run("bedit", json!({ "name": "Box" })).unwrap();
    h.run_steps(3);
}

fn editing(h: &Harness<'static, CadApp>) -> bool {
    h.state().session.state().unwrap().block_editor().is_some()
}

#[test]
fn the_block_editor_opens_its_tab_and_closing_gives_the_previous_tab_back() {
    let mut h = harness();
    assert!(h.query_by_label("Block Editor").is_none());
    open_box(&mut h);
    assert_eq!(h.state().ui.ribbon_tab, ribbon_bedit::TAB_ID);
    for label in ["Open/Save", "Save Block", "Auto Constrain", "Parameters Manager", "Attribute Definition", "Close Block Editor"] {
        assert!(h.query_by_label(label).is_some(), "{label} shown");
    }

    // Another tab can be chosen while editing, and the Block Editor's tab chosen again.
    h.get_by_label("Home").click();
    h.run_steps(3);
    assert_eq!(h.state().ui.ribbon_tab, "home");
    h.get_by_label("Block Editor").click();
    h.run_steps(3);
    assert_eq!(h.state().ui.ribbon_tab, ribbon_bedit::TAB_ID);

    h.get_by_label("Close Block Editor").click();
    h.run_steps(3);
    assert!(!editing(&h));
    assert_eq!(h.state().ui.ribbon_tab, "annotate", "back to the tab shown before");
    assert!(h.query_by_label("Block Editor").is_none());
}

#[test]
fn save_block_writes_the_edit_into_the_block() {
    let mut h = harness();
    open_box(&mut h);
    h.state_mut().run("circle", json!({ "center": [5, 5], "radius": 2 })).unwrap();
    h.run_steps(3);
    assert!(h.state().session.state().unwrap().block_unsaved());
    h.get_by_label("Save Block").click();
    h.run_steps(3);
    assert!(editing(&h), "Save Block keeps the Block Editor open");
    assert!(!h.state().session.state().unwrap().block_unsaved());
    // Saved: closing asks nothing.
    h.get_by_label("Close Block Editor").click();
    h.run_steps(3);
    assert!(!editing(&h));
    let d = h.state().session.doc().unwrap();
    let count = d.blocks.get("Box").map(|b| b.entities.len());
    assert_eq!(count, Some(2), "the rectangle and the new circle");
}

/// HATCH run in the Block Editor shows Hatch Creation beside the Block Editor's tab (both show,
/// the newer one active); the Block Editor's tab is active again after.
#[test]
fn hatch_creation_shows_beside_the_block_editor_tab_and_gives_it_back() {
    let mut h = harness();
    open_box(&mut h);
    h.state_mut().start("hatch");
    h.run_steps(3);
    assert_eq!(h.state().ui.ribbon_tab, ribbon_hatch::CREATE_ID);
    assert!(h.query_by_label("Block Editor").is_some(), "the Block Editor's tab still shows");
    assert!(h.query_by_label(ribbon_hatch::CREATE_TITLE).is_some());
    h.state_mut().session.cancel();
    h.run_steps(3);
    assert_eq!(h.state().ui.ribbon_tab, ribbon_bedit::TAB_ID);
}
