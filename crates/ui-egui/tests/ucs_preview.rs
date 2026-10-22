//! Actual pointer placement and optional canvas captures of the three UCS stages.

use cadcraft_engine::{Session, geom::Vec2};
use cadcraft_ui_egui::{CadApp, Services, i18n::Preference, theme::ThemePref};
use egui::{Event, PointerButton, Shape, vec2};
use egui_kittest::Harness;
use serde_json::json;

fn app(language: Preference, theme: ThemePref) -> CadApp {
    let session = Session::new();
    let mut app = CadApp::new(session, Services::default());
    app.ui.interface_language = language;
    app.ui.theme = theme;
    app.ui.show_viewcube = false;
    for name in ["OSMODE", "ORTHOMODE", "SNAPMODE", "POLARMODE"] {
        app.run("setvar", json!({"name":name,"value":0})).unwrap();
    }
    app.run("view.set", json!({"center":[20,15],"height":100})).unwrap();
    app.canvas.zoom_pending = false;
    app
}

fn move_to(h: &mut Harness<'_, CadApp>, point: Vec2) {
    let at = h.state().canvas.xf.unwrap().to_screen(point);
    h.event(Event::PointerMoved(at));
    h.run_steps(3);
}

fn click(h: &mut Harness<'_, CadApp>, point: Vec2) {
    let at = h.state().canvas.xf.unwrap().to_screen(point);
    h.event(Event::PointerMoved(at));
    h.event(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Default::default() });
    h.event(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Default::default() });
    h.run_steps(3);
}

fn text_positions(shape: &Shape, text: &str, out: &mut Vec<egui::Pos2>) {
    match shape {
        Shape::Text(shape) if shape.galley.text() == text => out.push(shape.pos),
        Shape::Vec(shapes) => {
            for shape in shapes {
                text_positions(shape, text, out);
            }
        }
        _ => {}
    }
}

fn check_axis_label(h: &Harness<'_, CadApp>, label: &str, direction: Vec2) {
    let xf = h.state().canvas.xf.unwrap();
    let frame = h.state().session.ucs_preview(h.state().canvas.cursor.unwrap()).unwrap().frame;
    let origin = xf.to_screen(frame.origin.xy());
    let end = xf.to_screen(frame.origin.xy() + direction);
    let at = origin + (end - origin).normalized() * 70.0;
    let mut positions = Vec::new();
    for shape in &h.output().shapes {
        text_positions(&shape.shape, label, &mut positions);
    }
    assert!(positions.iter().any(|pos| pos.distance(at) < 16.0), "{label}: expected label near {at:?}, found {positions:?}");
}

#[test]
fn pointer_ucs_placement_previews_all_stages() {
    {
        let mut h = Harness::builder().with_size(vec2(1440.0, 1000.0)).build_ui_state(
            |ui, app: &mut CadApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            app(Preference::English, ThemePref::Dark),
        );
        h.run_steps(5);
        let before = h.state().session.state().unwrap().doc.clone();
        let undo = h.state().session.state().unwrap().undo.len();
        h.state_mut().session.start("ucs").unwrap();
        let origin = Vec2::new(15.0, 20.0);
        move_to(&mut h, origin);
        check_axis_label(&h, "X", Vec2::X);
        check_axis_label(&h, "Y", Vec2::Y);
        click(&mut h, origin);
        let x = origin + Vec2::new(20.0, -10.0);
        move_to(&mut h, x);
        check_axis_label(&h, "X", x - origin);
        click(&mut h, x);
        let side = (x - origin).perp();
        for sign in [1.0, -1.0] {
            move_to(&mut h, origin + side * sign);
            check_axis_label(&h, "Y", side * sign);
            assert!(std::sync::Arc::ptr_eq(&before, &h.state().session.state().unwrap().doc));
        }
        click(&mut h, origin - side);
        assert!(h.state().session.running.is_none());
        let frame = h.state().session.input_ucs();
        assert!(frame.reversed());
        assert!(frame.origin.xy().dist(origin) < 1e-5);
        assert!(frame.x.xy().dist((x - origin).normalized()) < 1e-5);
        assert_eq!(h.state().session.state().unwrap().undo.len(), undo + 1);
        h.state_mut().session.undo().unwrap();
        assert!(h.state().session.ucs().is_world());
    }
}

#[test]
fn ucs_preview_visual_matrix() {
    let Some(directory) = std::env::var_os("CADCRAFT_UCS_SHOTS").map(std::path::PathBuf::from) else { return };
    std::fs::create_dir_all(&directory).unwrap();
    for (language, theme, name) in [
        (Preference::English, ThemePref::Light, "en-light"),
        (Preference::English, ThemePref::Dark, "en-dark"),
        (Preference::BrazilianPortuguese, ThemePref::Light, "pt-light"),
        (Preference::BrazilianPortuguese, ThemePref::Dark, "pt-dark"),
    ] {
        let mut h = Harness::builder().with_size(vec2(1440.0, 1000.0)).wgpu().build_ui_state(
            |ui, app: &mut CadApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            app(language, theme),
        );
        h.run_steps(5);
        h.state_mut().session.start("ucs").unwrap();
        let origin = Vec2::new(15.0, 30.0);
        move_to(&mut h, origin);
        h.render().unwrap().save(directory.join(format!("ucs-{name}-1-origin.png"))).unwrap();
        click(&mut h, origin);
        let x = Vec2::new(50.0, 10.0);
        move_to(&mut h, x);
        h.render().unwrap().save(directory.join(format!("ucs-{name}-2-x-axis.png"))).unwrap();
        click(&mut h, x);
        for (side, suffix) in [(Vec2::new(30.0, 50.0), "3-y-left"), (Vec2::new(0.0, 0.0), "3-y-right")] {
            move_to(&mut h, side);
            h.render().unwrap().save(directory.join(format!("ucs-{name}-{suffix}.png"))).unwrap();
        }
        h.state_mut().session.cancel();
        h.run_steps(3);
        assert!(h.state().session.ucs_preview(Vec2::ZERO).is_none());
    }
}
