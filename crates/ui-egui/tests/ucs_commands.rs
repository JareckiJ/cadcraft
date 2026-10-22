//! Mouse placement, live selection and in-place text agree with the engine's UCS frame.
use cadcraft_doc::{Block, Entity, EntityKind, Handle, Line};
use cadcraft_engine::{
    Input, Session,
    geom::{Vec2, Vec3},
};
use cadcraft_ui_egui::{CadApp, Services, gpu, i18n::Preference, theme::ThemePref};
use egui::{Event, Key, Modifiers, PointerButton, Shape, vec2};
use egui_kittest::{
    Harness,
    wgpu::{WgpuTestRenderer, create_render_state, default_wgpu_setup},
};
use serde_json::json;
use std::sync::Arc;

fn harness(language: Preference, theme: ThemePref, twist: f64, capture: bool) -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.ui.interface_language = language;
    app.ui.theme = theme;
    app.session.settings.osmode = 0;
    app.session.settings.orthomode = false;
    app.session.settings.snapmode = false;
    app.session.settings.polarmode = false;
    app.run("ucs", json!({"option":"origin","origin":[1000,1000]})).unwrap();
    app.run("ucs", json!({"option":"z","angle":37})).unwrap();
    app.run("dview", json!({"twist":twist})).unwrap();
    app.run("view.set", json!({"center":[1000,1000],"height":900})).unwrap();
    app.run("setvar", json!({"name":"TEXTSIZE","value":25})).unwrap();
    let mut block = Block::new("UCS probe");
    block.entities.push(Entity::new(Handle(30_000), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(70.0, 50.0, 0.0) })));
    app.session.doc_mut().unwrap().blocks.insert(block.name.clone(), Arc::new(block));
    app.canvas.zoom_pending = false;
    let mut builder = Harness::builder().with_size(vec2(1440.0, 1000.0));
    if capture && theme == ThemePref::Dark {
        let state = create_render_state(default_wgpu_setup(), egui_wgpu::RendererOptions::PREDICTABLE);
        app.canvas.gpu = Some(gpu::install(&state, &app.gpu_faults, None));
        app.canvas.gpu_device = Some(state.device.clone());
        builder = builder.renderer(WgpuTestRenderer::from_render_state(state));
    } else if capture {
        builder = builder.wgpu();
    }
    let mut h = builder.build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.event(Event::PointerGone);
    h.run_steps(5);
    h
}
fn world(h: &Harness<'_, CadApp>, x: f64, y: f64) -> Vec2 {
    h.state().session.input_ucs().to_wcs(Vec2::new(x, y))
}
fn click(h: &mut Harness<'_, CadApp>, x: f64, y: f64) {
    let at = h.state().canvas.xf.unwrap().to_screen(world(h, x, y));
    h.hover_at(at);
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        h.run_steps(1);
    }
    h.run_steps(3);
}
fn input(h: &mut Harness<'_, CadApp>, i: Input) {
    h.state_mut().session.input(i).unwrap();
}
fn shot(h: &mut Harness<'_, CadApp>, dir: &Option<std::path::PathBuf>, name: &str) {
    if let Some(dir) = dir {
        h.render().unwrap().save(dir.join(format!("{name}.png"))).unwrap();
    }
}

#[test]
fn mouse_commands_and_window_selection_follow_ucs_with_camera_twist() {
    let dir = std::env::var_os("CADCRAFT_UCS_SHOTS").map(std::path::PathBuf::from);
    if let Some(dir) = &dir {
        std::fs::create_dir_all(dir).unwrap();
    }
    for (language, theme, name) in [
        (Preference::English, ThemePref::Dark, "en-dark"),
        (Preference::English, ThemePref::Light, "en-light"),
        (Preference::BrazilianPortuguese, ThemePref::Dark, "pt-dark"),
        (Preference::BrazilianPortuguese, ThemePref::Light, "pt-light"),
    ] {
        {
            for twist in [-37.0, 13.0] {
                let captures = if twist == -37.0 { dir.clone() } else { None };
                let mut h = harness(language, theme, twist, captures.is_some());
                let prefix = format!("cad-{name}");
                h.state_mut().session.start("rectang").unwrap();
                click(&mut h, -150.0, -100.0);
                let cursor = world(&h, 150.0, 100.0);
                let at = h.state().canvas.xf.unwrap().to_screen(cursor);
                h.hover_at(at);
                h.run_steps(3);
                let preview = h.state().session.preview(h.state().canvas.cursor.unwrap());
                let preview_rectangle = preview
                    .iter()
                    .find_map(|e| match &e.kind {
                        EntityKind::LwPolyline(p) => Some(p.clone()),
                        _ => None,
                    })
                    .unwrap();
                shot(&mut h, &captures, &format!("{prefix}-rectangle-preview"));
                click(&mut h, 150.0, 100.0);
                let rectangle = h.state().session.doc().unwrap().model.iter().last().unwrap().handle;
                let frame = h.state().session.input_ucs();
                let EntityKind::LwPolyline(rect) = &h.state().session.doc().unwrap().entity(rectangle).unwrap().kind else { panic!("rectangle") };
                assert_eq!(rect.vertices.len(), preview_rectangle.vertices.len());
                for (actual, expected) in rect.vertices.iter().zip(&preview_rectangle.vertices) {
                    assert!(actual.p.dist(expected.p) < 1e-8, "preview and committed rectangle agree");
                    assert_eq!(actual.bulge, expected.bulge);
                }
                for p in [Vec2::new(-150.0, -100.0), Vec2::new(150.0, -100.0), Vec2::new(150.0, 100.0), Vec2::new(-150.0, 100.0)] {
                    assert!(rect.vertices.iter().any(|v| frame.to_ucs(v.p).dist(p) < 1e-3), "{prefix} {twist}");
                }
                h.state_mut().session.start("text").unwrap();
                click(&mut h, -100.0, 150.0);
                input(&mut h, Input::Text("25".into()));
                input(&mut h, Input::Enter);
                input(&mut h, Input::Text("UCS TEXT".into()));
                input(&mut h, Input::Enter);
                h.run_steps(3);
                h.state_mut().session.start("insert").unwrap();
                input(&mut h, Input::Text("UCS probe".into()));
                click(&mut h, 0.0, 0.0);
                for _ in 0..3 {
                    input(&mut h, Input::Enter);
                }
                h.run_steps(3);
                h.state_mut().session.start("mtext").unwrap();
                click(&mut h, -120.0, -120.0);
                click(&mut h, 120.0, -200.0);
                h.event(Event::Text("UCS MTEXT".into()));
                h.run_steps(3);
                h.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
                h.run_steps(4);
                assert!(!h.state().session.prompting(), "{prefix}: {:?}", h.state().session.current_prompt());
                let drawing = h.state().session.doc().unwrap();
                let handles: Vec<_> = drawing.model.iter().map(|e| e.handle).collect();
                assert_eq!(handles.len(), 4, "{prefix}: four committed commands");
                for e in drawing.model.iter() {
                    let rotation = match &e.kind {
                        EntityKind::Text(t) => Some(t.rotation),
                        EntityKind::MText(t) => Some(t.rotation),
                        EntityKind::Insert(i) => Some(i.rotation),
                        _ => None,
                    };
                    if let Some(rotation) = rotation {
                        assert!((rotation - 37f64.to_radians()).sin().abs() < 1e-8);
                    }
                }
                shot(&mut h, &captures, &format!("{prefix}-objects"));
                h.state_mut().session.set_selection(Vec::new());
                click(&mut h, -240.0, -240.0);
                assert!(h.state().session.pending_window.is_some(), "{prefix}");
                let opposite = world(&h, 240.0, 260.0);
                h.hover_at(h.state().canvas.xf.unwrap().to_screen(opposite));
                h.run_steps(3);
                let expected = frame.rectangle(world(&h, -240.0, -240.0), opposite).map(|p| h.state().canvas.xf.unwrap().to_screen(p));
                assert!(
                    h.output().shapes.iter().any(|s| match &s.shape {
                        Shape::Path(p) => p.points.len() == 4 && expected.iter().all(|q| p.points.iter().any(|p| p.distance(*q) < 0.1)),
                        _ => false,
                    }),
                    "{prefix}: exact oriented selection preview"
                );
                shot(&mut h, &captures, &format!("{prefix}-selection-window"));
                click(&mut h, 240.0, 260.0);
                assert!(h.state().session.pending_window.is_none());
                for handle in handles {
                    assert!(h.state().session.selection().contains(&handle), "{prefix} {twist}: {handle:?}");
                }
            }
        }
    }
}
