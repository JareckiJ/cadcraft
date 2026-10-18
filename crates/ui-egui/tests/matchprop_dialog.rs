//! MATCHPROP's Settings opens the Property Settings dialog, as in AutoCAD: OK keeps the ticks,
//! Cancel the settings as they were, and both go back to "Select destination object(s)".

use cadcraft_engine::doc::Handle;
use cadcraft_engine::doc::color::Color;
use cadcraft_engine::{Input, Session};
use cadcraft_ui_egui::{CadApp, Services};
use egui_kittest::kittest::Queryable;
use serde_json::json;

fn last(app: &CadApp) -> Handle {
    app.session.doc().unwrap().model.last().unwrap().handle
}

/// A red line on layer A with linetype scale 2.5 (the source) and two plain lines on layer 0,
/// with MATCHPROP waiting at its Settings prompt.
fn at_settings() -> (CadApp, Handle, Handle) {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.run("layer.new", json!({ "name": "A" })).unwrap();
    app.run("line", json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    let src = last(&app);
    app.run("properties.set", json!({ "handles": [src.hex()], "layer": "A", "color": "red", "ltscale": 2.5 })).unwrap();
    app.run("line", json!({ "points": [[0, 5], [10, 5]] })).unwrap();
    let a = last(&app);
    app.cmdline("ma");
    app.session.input(Input::Pick(vec![src])).unwrap();
    app.cmdline("s");
    (app, src, a)
}

fn harness(app: CadApp) -> egui_kittest::Harness<'static, CadApp> {
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1600.0, 1000.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h
}

fn props(app: &CadApp, h: Handle) -> (String, Color, f64) {
    let e = app.session.doc().unwrap().entity(h).cloned().unwrap();
    (e.common.layer.clone(), e.common.color, e.common.ltscale)
}

#[test]
fn ok_copies_only_the_ticked_properties() {
    let (app, _, a) = at_settings();
    let mut h = harness(app);
    assert_eq!(h.state().ui.dialog.as_deref(), Some("matchprop"));
    h.get_by_label("Layer").click();
    h.run_steps(2);
    if let Some(directory) = std::env::var_os("CADCRAFT_MATCHPROP_SHOTS").map(std::path::PathBuf::from) {
        std::fs::create_dir_all(&directory).unwrap();
        h.render().unwrap().save(directory.join("cadcraft-matchprop.png")).unwrap();
    }
    h.get_by_label("OK").click();
    h.run_steps(2);
    let app = h.state_mut();
    assert_eq!(app.ui.dialog, None);
    assert!(app.session.prompt_text().contains("Select destination object(s)"), "{}", app.session.prompt_text());
    assert!(!app.session.last_used.matchprop.layer);
    app.session.input(Input::Pick(vec![a])).unwrap();
    assert_eq!(props(app, a), ("0".into(), Color::Index(1), 2.5), "colour and linetype scale copied, layer kept");
}

#[test]
fn cancel_keeps_the_settings_as_they_were() {
    let (app, _, a) = at_settings();
    let mut h = harness(app);
    h.get_by_label("Layer").click();
    h.run_steps(2);
    h.get_by_label("Cancel").click();
    h.run_steps(2);
    let app = h.state_mut();
    assert_eq!(app.ui.dialog, None);
    assert!(app.session.prompt_text().contains("Select destination object(s)"), "{}", app.session.prompt_text());
    assert_eq!(app.session.last_used.matchprop, cadcraft_engine::cmd::MatchProps::default());
    app.session.input(Input::Pick(vec![a])).unwrap();
    assert_eq!(props(app, a), ("A".into(), Color::Index(1), 2.5));
}

#[test]
fn keywords_typed_while_the_dialog_is_open_still_work() {
    let (app, _, _) = at_settings();
    let mut h = harness(app);
    assert_eq!(h.state().ui.dialog.as_deref(), Some("matchprop"));
    h.state_mut().cmdline("c");
    h.state_mut().cmdline("");
    h.run_steps(2);
    let app = h.state();
    assert_eq!(app.ui.dialog, None, "the prompt it belonged to has gone");
    assert!(!app.session.last_used.matchprop.color);
    assert!(app.session.prompt_text().contains("Select destination object(s)"), "{}", app.session.prompt_text());
}
