//! Shift+left over the drawing area is a zoom modifier: a press-and-drag sets a region and runs
//! the transparent `zoom.window` command, and a double-click runs `zoom.previous`. These fire
//! whenever Shift is held in the drawing area, with or without a command running.

use cadcraft_engine::Session;
use cadcraft_ui_egui::control::{ControlRequest, Outcome, handle};
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};
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

/// One frame with an explicit event list. `time` advances so double-clicks are seen as such.
fn step(app: &mut CadApp, ctx: &egui::Context, events: Vec<Event>, time: f64) {
    let mut raw =
        RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 800.0))), time: Some(time), events, ..Default::default() };
    app.raw_input_hook(&mut raw);
    cadcraft_ui_egui::cmdline::capture_tab(ctx, &mut raw);
    let mut out = ctx.run_ui(raw, |ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    });
    out.textures_delta.clear();
}

fn view_height(app: &CadApp) -> f64 {
    app.session.state().map(|s| s.view().height).unwrap_or(f64::NAN)
}

fn new_app() -> (egui::Context, CadApp) {
    let ctx = egui::Context::default();
    let mut app = CadApp::new(Session::new(), Services::default());
    for i in 0..4 {
        step(&mut app, &ctx, vec![], f64::from(i) * 0.05);
    }
    assert!(app.synthetic.is_empty());
    assert!(app.canvas.rect.is_some());
    (ctx, app)
}

fn shift() -> Modifiers {
    Modifiers { shift: true, ..Default::default() }
}

/// A real Shift+left press, move and release through egui (the control channel's `ui.drag` cannot
/// carry a modifier).
fn shift_drag(app: &mut CadApp, ctx: &egui::Context, from: Pos2, to: Pos2, mut t: f64) {
    let s = shift();
    step(
        app,
        ctx,
        vec![
            Event::ModifiersChanged(s),
            Event::PointerMoved(from),
            Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: s },
        ],
        t,
    );
    for i in 1..=8 {
        t += 0.02;
        let p = from + (to - from) * (i as f32 / 8.0);
        step(app, ctx, vec![Event::ModifiersChanged(s), Event::PointerMoved(p)], t);
    }
    t += 0.02;
    step(
        app,
        ctx,
        vec![Event::ModifiersChanged(s), Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: s }],
        t,
    );
}

#[test]
fn shift_drag_when_idle_applies_zoom_window() {
    let (ctx, mut app) = new_app();
    let r = app.canvas.rect.unwrap();
    let h0 = view_height(&app);
    let from = r.center() + Vec2::new(-r.width() * 0.15, -r.height() * 0.15);
    let to = r.center() + Vec2::new(r.width() * 0.15, r.height() * 0.15);
    shift_drag(&mut app, &ctx, from, to, 0.2);
    let h1 = view_height(&app);
    assert!(h1 < h0 * 0.5, "a Shift+drag region must zoom the view in (h0={h0}, h1={h1})");
}

#[test]
fn shift_drag_while_command_runs_applies_zoom_window() {
    let (ctx, mut app) = new_app();
    call(&mut app, &ctx, "cmdline.input", json!({"text": "LINE"}));
    for i in 0..4 {
        step(&mut app, &ctx, vec![], 1.0 + f64::from(i) * 0.05);
    }
    let started = call(&mut app, &ctx, "cmdline.state", json!({}));
    assert_eq!(started["running"], "line");
    let r = app.canvas.rect.unwrap();
    let h0 = view_height(&app);
    let from = r.center() + Vec2::new(-r.width() * 0.15, -r.height() * 0.15);
    let to = r.center() + Vec2::new(r.width() * 0.15, r.height() * 0.15);
    shift_drag(&mut app, &ctx, from, to, 2.0);
    let h1 = view_height(&app);
    assert!(h1 < h0 * 0.5, "a Shift+drag region must zoom the view in while a command runs (h0={h0}, h1={h1})");
    let after = call(&mut app, &ctx, "cmdline.state", json!({}));
    assert_eq!(after["running"], "line", "the zoom gesture must not cancel the running command");
}

#[test]
fn shift_double_click_returns_to_the_previous_view() {
    let (ctx, mut app) = new_app();
    let center = app.canvas.rect.unwrap().center();
    let start = view_height(&app);
    call(&mut app, &ctx, "engine.execute", json!({"command": "zoom", "params": {"mode": "in"}}));
    let zoomed_in = view_height(&app);
    assert!(zoomed_in < start, "zoom in must shrink the view height (start={start}, in={zoomed_in})");

    let s = shift();
    let mut t = 2.0;
    for _ in 0..2 {
        t += 0.05;
        step(
            &mut app,
            &ctx,
            vec![
                Event::ModifiersChanged(s),
                Event::PointerMoved(center),
                Event::PointerButton { pos: center, button: PointerButton::Primary, pressed: true, modifiers: s },
            ],
            t,
        );
        t += 0.05;
        step(
            &mut app,
            &ctx,
            vec![Event::ModifiersChanged(s), Event::PointerButton { pos: center, button: PointerButton::Primary, pressed: false, modifiers: s }],
            t,
        );
    }
    let back = view_height(&app);
    assert!((back - start).abs() < 1e-6, "a Shift+double-click must restore the previous view (start={start}, back={back})");
}
