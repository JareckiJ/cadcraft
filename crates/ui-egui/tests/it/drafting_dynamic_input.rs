//! Drafting Settings opens on its tabs, and the Dynamic Input tab turns pointer input and
//! dimension input on and off apart (DYNMODE's bits) and opens their settings windows.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services, dialogs, theme};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

fn harness(dialog: &str) -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.ui.dialog = Some(dialog.into());
    let mut h = Harness::builder().with_size(egui::vec2(900.0, 700.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            theme::apply(ui.ctx(), egui::Theme::Dark, false);
            dialogs::show(app, ui.ctx());
        },
        app,
    );
    h.run_steps(5);
    h
}

fn dynmode(h: &mut Harness<'_, CadApp>) -> serde_json::Value {
    h.state_mut().run("getvar", json!({"name": "DYNMODE"})).unwrap()["value"].clone()
}

#[test]
fn the_tabs_switch_and_keep_the_dialog_open() {
    let mut h = harness("dsettings");
    assert!(h.query_by_label("Snap On (F9)").is_some(), "Snap and Grid is the first tab");
    h.get_by_label("Object Snap").click();
    h.run_steps(3);
    assert_eq!(h.state().ui.dialog.as_deref(), Some("dsettings:osnap"));
    assert!(h.query_by_label("Object Snap On (F3)").is_some());
    assert!(h.query_by_label("Snap On (F9)").is_none());
    h.get_by_label("Dynamic Input").click();
    h.run_steps(3);
    assert!(h.query_by_label("Enable Pointer Input").is_some());
}

#[test]
fn pointer_and_dimension_input_tick_apart() {
    let mut h = harness("dsettings:dynamic");
    assert_eq!(dynmode(&mut h), json!(3));
    h.get_by_label("Enable Pointer Input").click();
    h.run_steps(3);
    assert_eq!(dynmode(&mut h), json!(2), "dimension input stays on");
    h.get_by_label("Enable Dimension Input where possible").click();
    h.run_steps(3);
    assert_eq!(dynmode(&mut h), json!(0));
    h.get_by_label("Enable Pointer Input").click();
    h.run_steps(3);
    assert_eq!(dynmode(&mut h), json!(1));
    // F12 off: both show unticked; ticking one turns Dynamic Input back on with it.
    h.state_mut().run("dynmode", json!({"on": false})).unwrap();
    h.run_steps(3);
    h.get_by_label("Enable Dimension Input where possible").click();
    h.run_steps(3);
    assert_eq!(dynmode(&mut h), json!(2));
}

#[test]
fn settings_buttons_open_the_pointer_and_dimension_windows() {
    let mut h = harness("dsettings:dynamic");
    h.get_all_by_label("Settings...").next().expect("Pointer Input's Settings...").click();
    h.run_steps(3);
    h.get_by_label("Always, even with no command running").click();
    h.run_steps(3);
    assert_eq!(h.state_mut().run("getvar", json!({"name": "DYNPIVIS"})).unwrap()["value"], json!(2));
    h.get_all_by_label("Settings...").nth(1).expect("Dimension Input's Settings...").click();
    h.run_steps(3);
    h.get_by_label("These dimension input fields together:").click();
    h.run_steps(3);
    h.get_by_label("Arc Radius").click();
    h.run_steps(3);
    let app = h.state_mut();
    assert_eq!(app.run("getvar", json!({"name": "DYNDIVIS"})).unwrap()["value"], json!(2));
    assert_eq!(app.run("getvar", json!({"name": "DYNDIGRIP"})).unwrap()["value"], json!(15));
}
