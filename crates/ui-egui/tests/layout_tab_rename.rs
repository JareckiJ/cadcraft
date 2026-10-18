//! Double-clicking a layout tab renames it in place, as in AutoCAD (gaps.md U7): Enter or a click
//! elsewhere keeps the new name, Escape keeps the old one, and the Model tab is not renamed.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services};
use egui::accesskit::Role;
use egui::{Event, Key, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

fn harness() -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.run("line", json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    // 60 frames a second: the default step (0.25 s) spreads two clicks past the double-click delay.
    let mut h = Harness::builder().with_size(vec2(1400.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    h
}

fn names(h: &Harness<'_, CadApp>) -> Vec<String> {
    h.state().session.doc().unwrap().layouts.iter().map(|l| l.name.clone()).collect()
}

fn tab(h: &Harness<'_, CadApp>, label: &str) -> egui::Rect {
    h.get_by_role_and_label(Role::Tab, label).rect()
}

fn clicks(h: &mut Harness<'_, CadApp>, pos: Pos2, n: usize) {
    h.event(Event::PointerMoved(pos));
    h.run_steps(2);
    for _ in 0..n {
        for pressed in [true, false] {
            h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
            h.run_steps(1);
        }
    }
    h.run_steps(2);
}

fn key(h: &mut Harness<'_, CadApp>, key: Key) {
    for pressed in [true, false] {
        h.event(Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
    }
    h.run_steps(2);
}

fn type_text(h: &mut Harness<'_, CadApp>, text: &str) {
    h.event(Event::Text(text.into()));
    h.run_steps(2);
}

/// The open rename field: a text input in the row of layout tabs (the command line and the
/// Properties palette have their own).
fn field(h: &Harness<'_, CadApp>) -> Option<(String, egui::Rect)> {
    let model = tab(h, "Model");
    let row = egui::Rect::from_min_max(egui::pos2(0.0, model.top()), egui::pos2(700.0, model.bottom()));
    let n = h.query_all_by_role(Role::TextInput).find(|n| row.contains(n.rect().center()))?;
    Some((n.value().unwrap_or_default(), n.rect()))
}

#[test]
fn double_click_opens_the_name_on_that_tab() {
    let mut h = harness();
    let (one, two) = (tab(&h, "Layout1"), tab(&h, "Layout2"));
    assert!(field(&h).is_none());
    clicks(&mut h, two.center(), 2);
    let (text, rect) = field(&h).expect("a double-click opens the name for editing");
    assert_eq!(text, "Layout2");
    assert!(rect.intersects(two) && !rect.intersects(one), "the field {rect:?} sits on Layout2 {two:?}, not Layout1 {one:?}");
    if let Ok(directory) = std::env::var("CRAFT_UI_VISUAL_DIR") {
        std::fs::create_dir_all(&directory).unwrap();
        h.render().unwrap().save(format!("{directory}/layout-tab-rename.png")).unwrap();
    }
    // A single click only switches layouts.
    let mut h = harness();
    let at = tab(&h, "Layout1").center();
    clicks(&mut h, at, 1);
    assert!(field(&h).is_none());
}

#[test]
fn enter_renames_in_one_undo_step() {
    let mut h = harness();
    let at = tab(&h, "Layout2").center();
    clicks(&mut h, at, 2);
    type_text(&mut h, "Sheet A");
    key(&mut h, Key::Enter);
    assert_eq!(names(&h), ["Layout1", "Sheet A"]);
    assert!(field(&h).is_none(), "Enter closes the field");
    h.state_mut().session.cmdline("u").unwrap();
    h.run_steps(2);
    assert_eq!(names(&h), ["Layout1", "Layout2"]);
    assert_eq!(h.state().session.doc().unwrap().model.iter().count(), 1, "one undo takes back the rename only");
}

#[test]
fn escape_keeps_the_old_name() {
    let mut h = harness();
    let at = tab(&h, "Layout2").center();
    clicks(&mut h, at, 2);
    type_text(&mut h, "Sheet A");
    key(&mut h, Key::Escape);
    assert!(field(&h).is_none(), "Escape closes the field");
    assert_eq!(names(&h), ["Layout1", "Layout2"]);
    h.state_mut().session.cmdline("u").unwrap();
    assert_eq!(h.state().session.doc().unwrap().model.iter().count(), 0, "the last undo step is still the line");
}

#[test]
fn a_click_elsewhere_keeps_the_new_name() {
    let mut h = harness();
    let at = tab(&h, "Layout2").center();
    clicks(&mut h, at, 2);
    type_text(&mut h, "Sheet A");
    clicks(&mut h, Pos2::new(700.0, 400.0), 1);
    assert!(field(&h).is_none());
    assert_eq!(names(&h), ["Layout1", "Sheet A"]);
}

#[test]
fn a_name_layout_rename_refuses_stays_open_with_its_message() {
    for (typed, message) in [("  ", "layout name is empty"), ("layout1", "already exists"), ("a/b", "invalid layout name")] {
        let mut h = harness();
        let at = tab(&h, "Layout2").center();
        clicks(&mut h, at, 2);
        type_text(&mut h, typed);
        key(&mut h, Key::Enter);
        assert_eq!(names(&h), ["Layout1", "Layout2"], "{typed:?}");
        let status = h.state().status.as_ref().map(|(m, _)| m.clone()).unwrap_or_default();
        assert!(status.contains(message), "{typed:?}: status {status:?}");
        assert_eq!(field(&h).map(|f| f.0).as_deref(), Some(typed), "{typed:?}: the field stays open to correct it");
    }
}

#[test]
fn the_model_tab_is_not_renamed() {
    let mut h = harness();
    let at = tab(&h, "Model").center();
    clicks(&mut h, at, 2);
    assert!(field(&h).is_none());
}

#[test]
fn typing_in_the_field_does_not_reach_the_command_line() {
    let mut h = harness();
    // Enter at an empty command line would repeat CIRCLE.
    h.state_mut().cmdline("CIRCLE 0,0 2");
    h.run_steps(2);
    let at = tab(&h, "Layout2").center();
    clicks(&mut h, at, 2);
    for c in ["L", "I", "N", "E"] {
        type_text(&mut h, c);
    }
    key(&mut h, Key::Enter);
    h.run_steps(2);
    assert_eq!(names(&h), ["Layout1", "LINE"]);
    let app = h.state();
    assert_eq!(app.cmd.buffer, "");
    assert_eq!(app.session.running.as_ref().map(|r| r.id.clone()), None, "neither LINE nor a repeated CIRCLE started");
}
