//! CAD rollover overlays and cursor-anchored zoom must follow the orbital camera.

use cadcraft_doc::Handle;
use cadcraft_engine::Session;
use cadcraft_geom::Vec2;
use cadcraft_ui_egui::{
    CadApp, Services, gpu,
    i18n::Preference,
    theme::{ThemePref, Tokens},
};
use egui::{Color32, Event, Modifiers, PointerButton, Pos2, Shape, vec2};
use egui_kittest::{
    Harness,
    wgpu::{WgpuTestRenderer, create_render_state, default_wgpu_setup},
};
use serde_json::json;

fn app(language: Preference, theme: ThemePref) -> (CadApp, Handle) {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.ui.interface_language = language;
    app.ui.theme = theme;
    for name in ["GRIDMODE", "GRIPS", "OSMODE", "UCSICON"] {
        app.run("setvar", json!({"name": name, "value": 0})).unwrap();
    }
    let rectangle = app.run("rectang", json!({"p1": [10, 5], "p2": [50, 25]})).unwrap();
    let block = app.run("block", json!({"name": "Orbit outline", "handles": [rectangle["handle"]]})).unwrap();
    let handle = Handle::parse_hex(block["insert"].as_str().unwrap()).unwrap();
    app.run("view.set", json!({"center": [30, 15], "height": 85})).unwrap();
    app.canvas.zoom_pending = false;
    (app, handle)
}

fn harness(mut app: CadApp, capture: bool, native_gpu: bool) -> Harness<'static, CadApp> {
    let mut builder = Harness::builder().with_size(vec2(1280.0, 800.0));
    if native_gpu {
        let rs = create_render_state(default_wgpu_setup(), egui_wgpu::RendererOptions::PREDICTABLE);
        app.canvas.gpu = Some(gpu::install(&rs, &app.gpu_faults, None));
        app.canvas.gpu_device = Some(rs.device.clone());
        builder = builder.renderer(WgpuTestRenderer::from_render_state(rs));
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
    h.run_steps(4);
    h
}

fn orbit(h: &mut Harness<'_, CadApp>) {
    let at = h.state().canvas.rect.unwrap().center();
    h.input_mut().events.extend([
        Event::ModifiersChanged(Modifiers::SHIFT),
        Event::PointerButton { pos: at, button: PointerButton::Middle, pressed: true, modifiers: Modifiers::SHIFT },
        Event::PointerMoved(at + vec2(70.0, -40.0)),
        Event::PointerButton { pos: at + vec2(70.0, -40.0), button: PointerButton::Middle, pressed: false, modifiers: Modifiers::SHIFT },
    ]);
    h.run_steps(1);
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(3);
    assert!(h.state().canvas.xf.unwrap().projection.is_some());
}

fn highlight_points(shape: &Shape, color: Color32, out: &mut Vec<Pos2>) {
    match shape {
        Shape::Path(path) if path.stroke.color == egui::epaint::ColorMode::Solid(color) => out.extend(&path.points),
        Shape::LineSegment { points, stroke } if stroke.color == color => out.extend(points),
        Shape::Mesh(mesh) => out.extend(mesh.vertices.iter().filter(|v| v.color == color.gamma_multiply(gpu::HIGHLIGHT_FILL_ALPHA)).map(|v| v.pos)),
        Shape::Vec(shapes) => shapes.iter().for_each(|s| highlight_points(s, color, out)),
        _ => {}
    }
}

fn assert_overlay(h: &Harness<'_, CadApp>, handle: Handle, color: Color32) {
    let xf = h.state().canvas.xf.unwrap();
    let list = h.state().canvas.list.as_ref().unwrap();
    let edges: Vec<_> = list
        .prims
        .iter()
        .filter(|p| p.handle == handle)
        .flat_map(|p| {
            let points = list.points(p);
            let edges: Vec<_> = if matches!(p.kind, cadcraft_render::Kind::Tris | cadcraft_render::Kind::Mask) {
                points.as_chunks::<3>().0.iter().flat_map(|[a, b, c]| [[*a, *b], [*b, *c], [*c, *a]]).collect()
            } else {
                points.windows(2).map(|pair| [pair[0], pair[1]]).collect()
            };
            edges.into_iter().map(|[a, b]| [xf.view_to_screen(a), xf.view_to_screen(b)])
        })
        .collect();
    assert!(!edges.is_empty());
    let mut points = Vec::new();
    for shape in &h.output().shapes {
        highlight_points(&shape.shape, color, &mut points);
    }
    assert!(!points.is_empty(), "No highlight was painted");
    for point in points {
        let distance = edges
            .iter()
            .map(|[a, b]| {
                let edge = *b - *a;
                let t = ((point - *a).dot(edge) / edge.length_sq()).clamp(0.0, 1.0);
                point.distance(*a + edge * t)
            })
            .fold(f32::INFINITY, f32::min);
        assert!(distance < 0.01, "Highlight at {point:?} is {distance} points away from the displayed CAD outline");
    }
}

fn hover(h: &mut Harness<'_, CadApp>, handle: Handle, effect: u8) {
    h.state_mut().run("setvar", json!({"name": "PREVIEWEFFECT", "value": effect})).unwrap();
    let at = h.state().canvas.xf.unwrap().to_screen(Vec2::new(30.0, 5.0));
    h.hover_at(at);
    h.run_steps(3);
    assert_eq!(h.state().canvas.hover, Some(handle));
    assert_overlay(h, handle, Tokens::get().hover);
}

#[test]
fn cad_hover_and_cpu_selection_follow_orbit_pan_zoom_and_twist() {
    let (app, handle) = app(Preference::English, ThemePref::Dark);
    let document = app.session.state().unwrap().doc.clone();
    let undo = app.session.state().unwrap().undo.len();
    let mut h = harness(app, false, false);
    orbit(&mut h);
    for (center, height, twist) in [([30, 15], 85, 0.0), ([20, 10], 60, 30.0)] {
        h.state_mut().run("view.set", json!({"center": center, "height": height, "twist": twist})).unwrap();
        h.run_steps(3);
        for effect in [0, 1, 2] {
            hover(&mut h, handle, effect);
        }
    }
    h.state_mut().run("select", json!({"handles": [handle.0]})).unwrap();
    h.event(Event::PointerGone);
    for effect in [0, 1] {
        h.state_mut().run("setvar", json!({"name": "SELECTIONEFFECT", "value": effect})).unwrap();
        h.run_steps(3);
        assert_overlay(&h, handle, Tokens::get().selection);
    }
    let state = h.state().session.state().unwrap();
    assert_eq!(state.doc.model.len(), document.model.len());
    assert!(state.doc.model.iter().zip(document.model.iter()).all(|(after, before)| std::sync::Arc::ptr_eq(after, before)));
    assert_eq!(state.undo.len(), undo);
}

fn wheel(amount: f32) -> Event {
    Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: vec2(0.0, amount), phase: egui::TouchPhase::Move, modifiers: Modifiers::NONE }
}

#[test]
fn cad_wheel_and_pinch_zoom_keep_the_pointer_anchor_in_orbital_views() {
    let (app, _) = app(Preference::English, ThemePref::Dark);
    let mut h = harness(app, false, false);
    let document = h.state().session.state().unwrap().doc.clone();
    let undo = h.state().session.state().unwrap().undo.len();
    for preset in ["top", "seiso", "front", "bottom"] {
        h.state_mut().run("vpoint", json!({"preset": preset})).unwrap();
        h.state_mut().run("view.set", json!({"center": [20, 10], "height": 60, "twist": 30.0})).unwrap();
        h.run_steps(3);
        for offset in [vec2(80.0, -55.0), vec2(-150.0, 100.0)] {
            let pointer = h.state().canvas.rect.unwrap().center() + offset;
            h.hover_at(pointer);
            h.run_steps(2);
            for (event, zoom_in) in [(wheel(120.0), true), (wheel(-120.0), false), (Event::Zoom(1.2), true), (Event::Zoom(0.8), false)] {
                let xf = h.state().canvas.xf.unwrap();
                let anchor = xf.center + Vec2::new(f64::from(offset.x), -f64::from(offset.y)).rotate(-xf.twist) / xf.scale;
                let before = h.state().session.state().unwrap().view();
                let direction = h.state().session.view_direction();
                h.event(event);
                h.run_steps(3);
                let after = h.state().session.state().unwrap().view();
                let at = h.state().canvas.xf.unwrap().view_to_screen(anchor);
                assert!(at.distance(pointer) < 0.001, "{preset}: zoom moved the pointer anchor by {} screen points", at.distance(pointer));
                assert_eq!(after.height < before.height, zoom_in, "{preset}");
                assert_eq!(after.twist, before.twist);
                assert_eq!(h.state().session.view_direction(), direction);
            }
        }
    }
    let state = h.state().session.state().unwrap();
    assert_eq!(state.doc.model.len(), document.model.len());
    assert!(state.doc.model.iter().zip(document.model.iter()).all(|(after, before)| std::sync::Arc::ptr_eq(after, before)));
    assert_eq!(state.undo.len(), undo);
}

#[test]
fn cad_orbit_hover_visual_matrix() {
    let Some(directory) = std::env::var_os("CADCRAFT_SHOTS").map(std::path::PathBuf::from) else { return };
    std::fs::create_dir_all(&directory).unwrap();
    for native_gpu in [false, true] {
        for (language, theme, name) in [
            (Preference::English, ThemePref::Light, "en-light"),
            (Preference::English, ThemePref::Dark, "en-dark"),
            (Preference::BrazilianPortuguese, ThemePref::Light, "pt-light"),
            (Preference::BrazilianPortuguese, ThemePref::Dark, "pt-dark"),
        ] {
            let (app, handle) = app(language, theme);
            let mut h = harness(app, true, native_gpu);
            orbit(&mut h);
            hover(&mut h, handle, 1);
            let path = if native_gpu { "gpu" } else { "cpu" };
            h.render().unwrap().save(directory.join(format!("cad-orbit-hover-{name}-{path}.png"))).unwrap();
            h.event(wheel(120.0));
            h.run_steps(30);
            assert_eq!(h.state().canvas.hover, Some(handle));
            assert_overlay(&h, handle, Tokens::get().hover);
            h.render().unwrap().save(directory.join(format!("cad-orbit-hover-zoom-{name}-{path}.png"))).unwrap();
            assert!(h.state().gpu_faults.take().is_none());
        }
    }
}

#[test]
fn filled_hatch_hover_follows_the_orbital_projection() {
    let (mut app, _) = app(Preference::English, ThemePref::Dark);
    app.run("rectang", json!({"p1":[60,5],"p2":[100,25]})).unwrap();
    let result = app.run("hatch", json!({"points":[[80,15]],"pattern":"SOLID"})).unwrap();
    let handle = Handle::parse_hex(result["handle"].as_str().unwrap()).unwrap();
    let mut h = harness(app, false, false);
    orbit(&mut h);
    h.state_mut().run("view.set", json!({"center":[70,15],"height":140,"twist":37})).unwrap();
    h.run_steps(3);
    let at = h.state().canvas.xf.unwrap().to_screen(Vec2::new(80.0, 15.0));
    for effect in [0, 1, 2] {
        h.state_mut().run("setvar", json!({"name":"PREVIEWEFFECT","value":effect})).unwrap();
        h.hover_at(at);
        h.run_steps(3);
        assert_eq!(h.state().canvas.hover, Some(handle));
        assert_overlay(&h, handle, Tokens::get().hover);
    }
}
