//! A Shift+left click over the drawing area must never cancel a running command: it feeds no
//! point (so the command keeps waiting), while a plain left click still feeds its point.

use cadcraft_engine::Session;
use cadcraft_ui_egui::control::{ControlRequest, Outcome, handle};
use cadcraft_ui_egui::{CadApp, Services};
use serde_json::{Value, json};

fn call(app: &mut CadApp, ctx: &egui::Context, method: &str, params: Value) -> Value {
    let (req, _reply) = ControlRequest::new(method, params);
    match handle(app, ctx, &req) {
        Outcome::Done(v) => {
            assert_eq!(v["ok"], true, "{v}");
            v["result"].clone()
        }
        _ => panic!("unexpected screenshot"),
    }
}

fn frame(app: &mut CadApp, ctx: &egui::Context) {
    let mut raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0))), ..Default::default() };
    app.raw_input_hook(&mut raw);
    cadcraft_ui_egui::cmdline::capture_tab(ctx, &mut raw);
    let mut out = ctx.run_ui(raw, |ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    });
    out.textures_delta.clear();
}

fn pump(app: &mut CadApp, ctx: &egui::Context) {
    for _ in 0..4 {
        frame(app, ctx);
    }
    assert!(app.synthetic.is_empty());
    assert!(app.canvas.rect.is_some());
}

#[test]
fn shift_left_click_does_not_cancel_a_running_command() {
    let ctx = egui::Context::default();
    let mut app = CadApp::new(Session::new(), Services::default());
    pump(&mut app, &ctx);
    let center = app.canvas.rect.unwrap().center();

    call(&mut app, &ctx, "cmdline.input", json!({"text": "LINE"}));
    let started = call(&mut app, &ctx, "cmdline.state", json!({}));
    assert_eq!(started["running"], "line");

    call(&mut app, &ctx, "ui.click", json!({"x": center.x, "y": center.y}));
    pump(&mut app, &ctx);
    let after_plain = call(&mut app, &ctx, "cmdline.state", json!({}));
    assert_eq!(after_plain["running"], "line", "a plain click keeps the command running");
    assert_ne!(after_plain["prompt"], started["prompt"], "a plain click must feed its point");

    call(&mut app, &ctx, "ui.key", json!({"key": "escape"}));
    pump(&mut app, &ctx);

    call(&mut app, &ctx, "cmdline.input", json!({"text": "LINE"}));
    call(&mut app, &ctx, "ui.click", json!({"x": center.x, "y": center.y, "shift": true}));
    pump(&mut app, &ctx);
    let after_shift = call(&mut app, &ctx, "cmdline.state", json!({}));
    assert_eq!(after_shift["running"], "line", "Shift+left must not cancel the running command");
    assert_eq!(after_shift["prompt"], started["prompt"], "Shift+left must not feed a point");
}
