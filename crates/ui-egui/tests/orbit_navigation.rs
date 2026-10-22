//! Temporary orbit preserves drafting commands and switches back to pan on release.

use cadcraft_engine::Session;
use cadcraft_geom::Vec2;
use cadcraft_ui_egui::{CadApp, Services, i18n::Preference, theme::ThemePref};
use egui::{CursorIcon, Event, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use serde_json::json;

fn app() -> CadApp {
    let mut app = CadApp::new(Session::new(), Services::default());

    app.run("rectang", json!({"p1":[-20,-10],"p2":[20,10]})).unwrap();
    app.run("circle", json!({"center":[0,0],"radius":8})).unwrap();
    app.run("view.set", json!({"center":[0,0],"height":80})).unwrap();

    app.canvas.zoom_pending = false;
    app
}

fn harness(app: CadApp) -> Harness<'static, CadApp> {
    let mut h = Harness::builder().with_size(vec2(1280.0, 800.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    h
}

fn button(pos: Pos2, button: PointerButton, pressed: bool, modifiers: Modifiers) -> Event {
    Event::PointerButton { pos, button, pressed, modifiers }
}

fn orbit_cursor(h: &Harness<'_, CadApp>, at: Pos2) -> bool {
    h.output().shapes.iter().any(|shape| {
        matches!(&shape.shape, egui::Shape::Circle(circle)
            if circle.center.distance(at) < 0.01 && circle.radius == 4.0 && circle.stroke.color == egui::Color32::WHITE)
    })
}

#[test]
fn shift_middle_orbits_a_reopened_dwg_and_plain_middle_resumes_pan() {
    let source = app();
    let bytes = cadcraft_io::write(source.session.doc().unwrap(), "Orbit.dwg").unwrap();
    let drawing = cadcraft_io::read(&bytes, "Orbit.dwg").unwrap();
    let mut session = Session::empty();
    session.open_drawing(drawing, "Orbit.dwg", None);
    let mut h = harness(CadApp::new(session, Services::default()));
    assert_eq!(h.state().session.layout_space(), cadcraft_doc::Space::Model);
    // The imported UCS icon occupies the world origin at the canvas center.
    let at = h.state().canvas.rect.unwrap().center() + vec2(110.0, -60.0);
    h.hover_at(at);
    h.run_steps(2);
    let before = h.state().session.state().unwrap();
    let document = before.doc.clone();
    let undo = before.undo.len();
    let height = before.view().height;
    let direction = h.state().session.view_direction();
    h.event(Event::ModifiersChanged(Modifiers::SHIFT));
    h.event(button(at, PointerButton::Middle, true, Modifiers::SHIFT));
    h.run_steps(1);
    assert!(orbit_cursor(&h, at), "Middle press did not acquire the canvas: {:?}", h.state().canvas.rect);
    h.hover_at(at + vec2(70.0, -40.0));
    h.run_steps(1);
    assert_ne!(h.state().session.view_direction(), direction);
    assert!(orbit_cursor(&h, at + vec2(70.0, -40.0)));
    h.event(button(at + vec2(70.0, -40.0), PointerButton::Middle, false, Modifiers::SHIFT));
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(2);
    let direction = h.state().session.view_direction();
    let before = h.state().session.state().unwrap().view();
    h.input_mut().events.extend([
        button(at, PointerButton::Middle, true, Modifiers::NONE),
        Event::PointerMoved(at + vec2(40.0, 15.0)),
        button(at + vec2(40.0, 15.0), PointerButton::Middle, false, Modifiers::NONE),
    ]);
    h.run_steps(1);
    let after = h.state().session.state().unwrap();
    let units = height / f64::from(h.state().canvas.rect.unwrap().height());
    assert!(after.view().center.near(before.center + Vec2::new(-40.0, 15.0) * units, 1e-9));
    assert_eq!(h.state().session.view_direction(), direction);
    assert_eq!(after.view().height, height);
    assert_eq!(after.doc.model.len(), document.model.len());
    assert!(after.doc.model.iter().zip(document.model.iter()).all(|(after, before)| std::sync::Arc::ptr_eq(after, before)));
    assert_eq!(after.undo.len(), undo);
    assert!(h.state().session.running.is_none());
    assert!(h.state().session.pending_window.is_none());
    assert!(h.state().session.selection().is_empty());
}

#[test]
fn wheel_zoom_keeps_the_cursor_anchor_in_projected_and_twisted_views() {
    let mut h = harness(app());
    for twist in [0.0, 30.0] {
        h.state_mut().run("vpoint", json!({"preset":"seiso"})).unwrap();
        h.state_mut().run("dview", json!({"twist":twist})).unwrap();
        h.run_steps(4);
        let world = Vec2::new(12.0, -6.0);
        let screen = h.state().canvas.xf.unwrap().to_screen(world);
        h.hover_at(screen);
        h.run_steps(2);
        let direction = h.state().session.view_direction();
        let height = h.state().session.state().unwrap().view().height;
        h.event(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: vec2(0.0, 1.0),
            phase: egui::TouchPhase::Move,
            modifiers: Modifiers::NONE,
        });
        h.run_steps(2);
        let after = h.state().canvas.xf.unwrap().to_screen(world);
        assert!(after.distance(screen) < 0.1, "twist {twist}: anchor moved from {screen:?} to {after:?}");
        assert!(h.state().session.state().unwrap().view().height < height);
        assert_eq!(h.state().session.view_direction(), direction);
    }
}

#[test]
fn batched_shift_middle_orbit_preserves_the_running_drafting_command() {
    let mut h = harness(app());
    h.state_mut().session.start("line").unwrap();
    let prompt = h.state().session.current_prompt().unwrap().message;
    let direction = h.state().session.view_direction();
    let at = h.state().canvas.rect.unwrap().center();
    h.input_mut().events.extend([
        Event::ModifiersChanged(Modifiers::SHIFT),
        button(at, PointerButton::Middle, true, Modifiers::SHIFT),
        Event::PointerMoved(at + vec2(35.0, -20.0)),
        button(at + vec2(70.0, -40.0), PointerButton::Middle, false, Modifiers::SHIFT),
    ]);
    h.run_steps(1);
    assert_ne!(h.state().session.view_direction(), direction);
    assert_eq!(h.state().session.running.as_ref().unwrap().id, "line");
    assert_eq!(h.state().session.current_prompt().unwrap().message, prompt);
    let direction = h.state().session.view_direction();
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.hover_at(at + vec2(140.0, 60.0));
    h.run_steps(2);
    assert_eq!(h.state().session.view_direction(), direction);
}

#[test]
fn temporary_orbit_must_start_in_the_canvas_away_from_overlays() {
    let mut h = harness(app());
    let rect = h.state().canvas.rect.unwrap();
    let direction = h.state().session.view_direction();
    let view = h.state().session.state().unwrap().view();
    for start in [egui::pos2(rect.left() - 5.0, rect.center().y), rect.right_top() + vec2(-50.0, 50.0)] {
        h.input_mut().events.extend([
            Event::ModifiersChanged(Modifiers::SHIFT),
            button(start, PointerButton::Middle, true, Modifiers::SHIFT),
            Event::PointerMoved(rect.center()),
            button(rect.center(), PointerButton::Middle, false, Modifiers::SHIFT),
        ]);
        h.run_steps(1);
        assert_eq!(h.state().session.view_direction(), direction);
        assert_eq!(h.state().session.state().unwrap().view(), view);
    }
}

#[test]
fn orbit_cursor_follows_command_gesture_release_and_escape() {
    {
        for system_cursor in [false, true] {
            let mut app = app();
            app.ui.system_cursor = system_cursor;
            let mut h = harness(app);
            let at = h.state().canvas.rect.unwrap().center();
            h.hover_at(at);
            h.run_steps(2);
            assert!(!orbit_cursor(&h, at));
            let idle_cursor = h.output().platform_output.cursor_icon;
            h.state_mut().session.start("3dorbit").unwrap();
            h.run_steps(2);
            assert!(orbit_cursor(&h, at));
            assert_eq!(h.output().platform_output.cursor_icon, if system_cursor { CursorIcon::Grab } else { CursorIcon::None });
            h.event(button(at, PointerButton::Primary, true, Modifiers::NONE));
            h.run_steps(1);
            assert_eq!(h.output().platform_output.cursor_icon, if system_cursor { CursorIcon::Grabbing } else { CursorIcon::None });
            h.event(button(at, PointerButton::Primary, false, Modifiers::NONE));
            h.key_press(egui::Key::Escape);
            h.run_steps(2);
            assert!(!orbit_cursor(&h, at));
            assert_eq!(h.output().platform_output.cursor_icon, idle_cursor);
            h.event(Event::ModifiersChanged(Modifiers::SHIFT));
            h.event(button(at, PointerButton::Middle, true, Modifiers::SHIFT));
            h.run_steps(1);
            assert!(orbit_cursor(&h, at));
            h.event(button(at, PointerButton::Middle, false, Modifiers::SHIFT));
            h.event(Event::ModifiersChanged(Modifiers::NONE));
            h.run_steps(2);
            assert!(!orbit_cursor(&h, at));
            assert_eq!(h.output().platform_output.cursor_icon, idle_cursor);
        }
    }
}

#[test]
fn orbit_cursor_visual_matrix() {
    use egui_kittest::wgpu::{WgpuTestRenderer, create_render_state, default_wgpu_setup};
    let Some(directory) = std::env::var_os("CADCRAFT_SHOTS").map(std::path::PathBuf::from) else { return };
    std::fs::create_dir_all(&directory).unwrap();
    {
        for (language, theme, name) in [
            (Preference::English, ThemePref::Light, "en-light"),
            (Preference::English, ThemePref::Dark, "en-dark"),
            (Preference::BrazilianPortuguese, ThemePref::Light, "pt-light"),
            (Preference::BrazilianPortuguese, ThemePref::Dark, "pt-dark"),
        ] {
            let rs = create_render_state(default_wgpu_setup(), egui_wgpu::RendererOptions::PREDICTABLE);
            let mut app = app();
            app.ui.interface_language = language;
            app.ui.theme = theme;
            app.set_wgpu(&rs, None);
            let mut h = Harness::builder().with_size(vec2(1280.0, 800.0)).renderer(WgpuTestRenderer::from_render_state(rs)).build_ui_state(
                |ui, app: &mut CadApp| {
                    app.logic(ui.ctx());
                    app.ui(ui);
                },
                app,
            );
            h.run_steps(4);
            let kind = "cad";
            let at = h.state().canvas.rect.unwrap().center() + vec2(110.0, -60.0);
            h.hover_at(at);
            h.run_steps(2);
            h.state_mut().session.start("3dorbit").unwrap();
            h.run_steps(2);
            assert!(orbit_cursor(&h, at));
            h.render().unwrap().save(directory.join(format!("orbit-command-{kind}-{name}.png"))).unwrap();
            h.key_press(egui::Key::Escape);
            h.run_steps(2);
            h.event(Event::ModifiersChanged(Modifiers::SHIFT));
            h.event(button(at, PointerButton::Middle, true, Modifiers::SHIFT));
            h.run_steps(1);
            h.hover_at(at + vec2(60.0, -40.0));
            h.run_steps(2);
            assert!(orbit_cursor(&h, at + vec2(60.0, -40.0)));
            h.render().unwrap().save(directory.join(format!("orbit-gesture-{kind}-{name}.png"))).unwrap();
            assert!(h.state().gpu_faults.take().is_none());
        }
    }
}
