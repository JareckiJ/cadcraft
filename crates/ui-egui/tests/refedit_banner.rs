//! Tests for the in-place reference editing (REFEDIT) canvas banner.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services};
use serde_json::json;

fn app_with_block() -> (CadApp, String) {
    let mut app = CadApp::new(Session::new(), Services::default());
    let l = app.run("line", json!({ "points": [[0, 0], [10, 0]] })).unwrap()["handles"][0].clone();
    app.run("block", json!({ "name": "Desk", "base": [0, 0], "handles": [l] })).unwrap();
    let ins = app.run("insert", json!({ "name": "Desk", "at": [100, 100] })).unwrap();
    let ih = ins["handle"].as_str().unwrap().to_string();
    app.session.set_selection(Vec::new());
    (app, ih)
}

fn painted(app: &mut CadApp, ctx: &egui::Context) -> String {
    fn collect(shape: &egui::Shape, text: &mut String) {
        match shape {
            egui::Shape::Text(s) => {
                text.push_str(s.galley.text());
                text.push('\n');
            }
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, text)),
            _ => {}
        }
    }
    let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))), ..Default::default() };
    let mut output = ctx.run_ui(input, |ui| app.ui(ui));
    output.textures_delta.clear();
    let mut text = String::new();
    for s in &output.shapes {
        collect(&s.shape, &mut text);
    }
    text
}

#[test]
fn refedit_banner_shows_when_editing_in_place() {
    let (mut app, ih) = app_with_block();
    let ctx = egui::Context::default();

    // Before REFEDIT: no banner
    let text = painted(&mut app, &ctx);
    assert!(!text.contains("Edit Reference In-Place"));

    // Enter REFEDIT
    app.run("refedit", json!({ "handle": ih })).unwrap();
    assert!(app.session.doc().unwrap().refedit.is_some());

    app.logic(&ctx);
    painted(&mut app, &ctx);
    let text = painted(&mut app, &ctx);
    assert!(text.contains("Edit Reference In-Place"), "missing title from:\n{text}");
    assert!(text.contains("Desk"), "missing block name from:\n{text}");
    assert!(text.contains("Add to Working Set"), "missing Add button from:\n{text}");
    assert!(text.contains("Remove"), "missing Remove button from:\n{text}");
    assert!(text.contains("Save"), "missing Save button from:\n{text}");
    assert!(text.contains("Discard"), "missing Discard button from:\n{text}");

    // Save and close
    app.run("refclose", json!({ "save": true })).unwrap();
    assert!(app.session.doc().unwrap().refedit.is_none());

    // After REFCLOSE: banner is gone
    let text = painted(&mut app, &ctx);
    assert!(!text.contains("Edit Reference In-Place"));
}
