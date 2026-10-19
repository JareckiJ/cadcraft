//! Camera-oriented overlays share navigation and preserve drawing geometry.

use std::sync::Arc;

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services, gpu, i18n::Preference, theme::ThemePref, viewcube::Layout};
use egui::{Event, Modifiers, PointerButton, Pos2, Shape, vec2};
use egui_kittest::{
    Harness,
    wgpu::{WgpuTestRenderer, create_render_state, default_wgpu_setup},
};
use serde_json::json;

fn app(language: Preference, theme: ThemePref) -> CadApp {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.ui.interface_language = language;
    app.ui.theme = theme;

    for z in [0, 4] {
        app.run("3dpoly", json!({"points":[[3,3,z],[7,3,z],[7,7,z],[3,7,z],[3,3,z]]})).unwrap();
    }
    for (x, y) in [(3, 3), (7, 3), (7, 7), (3, 7)] {
        app.run("3dpoly", json!({"points":[[x,y,0],[x,y,4]]})).unwrap();
    }

    app.canvas.zoom_pending = false;
    app
}

fn harness(mut app: CadApp, capture: bool, gpu_canvas: bool) -> Harness<'static, CadApp> {
    let mut builder = Harness::builder().with_size(vec2(1400.0, 1000.0));
    if gpu_canvas {
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

fn texts<'a>(h: &'a Harness<'_, CadApp>) -> Vec<&'a egui::epaint::TextShape> {
    h.output()
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            Shape::Text(t) => Some(t),
            _ => None,
        })
        .collect()
}

fn pose(h: &mut Harness<'_, CadApp>, preset: &str, twist: f64) {
    h.state_mut().run("vpoint", json!({"preset":preset})).unwrap();
    h.state_mut().run("dview", json!({"twist":twist})).unwrap();

    let center = h.state().session.state().unwrap().view().center;
    h.state_mut().run("view.set", json!({"center":[center.x,center.y],"height":20})).unwrap();

    h.state_mut().canvas.zoom_pending = false;
    h.event(Event::PointerGone);
    h.run_steps(5);
}

#[test]
fn camera_faces_compass_and_xyz_follow_views_in_both_languages_and_themes() {
    let dir = std::env::var_os("CADCRAFT_NAVIGATION_SHOTS").map(std::path::PathBuf::from);
    if let Some(dir) = &dir {
        std::fs::create_dir_all(dir).unwrap();
    }
    {
        for gpu_canvas in [false, true] {
            if gpu_canvas && dir.is_none() {
                continue;
            }
            for (language, theme, name, top, front) in [
                (Preference::English, ThemePref::Light, "en-light", "TOP", "FRONT"),
                (Preference::English, ThemePref::Dark, "en-dark", "TOP", "FRONT"),
                (Preference::BrazilianPortuguese, ThemePref::Light, "pt-light", "SUPERIOR", "FRONTAL"),
                (Preference::BrazilianPortuguese, ThemePref::Dark, "pt-dark", "SUPERIOR", "FRONTAL"),
            ] {
                let mut h = harness(app(language, theme), dir.is_some(), gpu_canvas);
                let original = h.state().session.doc().unwrap().clone();
                for (preset, twist, tag) in
                    [("top", 0.0, "top"), ("top", 90.0, "top-90"), ("front", 0.0, "front"), ("seiso", 0.0, "iso"), ("seiso", 30.0, "iso-roll")]
                {
                    pose(&mut h, preset, twist);
                    let labels = texts(&h);
                    let cube = Layout::new(h.state().canvas.rect.unwrap());
                    let faces: Vec<_> = labels.iter().filter(|t| cube.face.expand(35.0).contains(t.pos)).map(|t| t.galley.text()).collect();
                    assert!(!faces.contains(&"3D") && !faces.contains(&"ISO"), "real faces replace placeholders: {faces:?}");
                    if preset == "front" {
                        assert!(faces.contains(&front), "CAD {faces:?}");
                        assert!(!faces.contains(&top));
                        assert!(labels.iter().any(|t| t.galley.text() == "Z"), "a front view has a visible Z arm");
                    } else {
                        assert!(faces.contains(&top), "CAD {faces:?}");
                    }
                    if tag == "top-90" {
                        let n = labels.iter().find(|t| t.galley.text() == "N").unwrap();
                        assert!(n.visual_bounding_rect().center().x < cube.center.x - cube.ring * 0.7);
                        let face = labels.iter().find(|t| t.galley.text() == top).unwrap();
                        assert!(face.galley.mesh_bounds.height() > face.galley.mesh_bounds.width());
                    }
                    if preset == "seiso" {
                        assert!(labels.iter().any(|t| t.galley.text() == "Z"));
                    }
                    let document = h.state().session.state().unwrap().doc.clone();
                    let undo = h.state().session.state().unwrap().undo.len();
                    h.run_steps(4);
                    assert!(Arc::ptr_eq(&document, &h.state().session.state().unwrap().doc), "painting must not create a document edit");
                    assert_eq!(h.state().session.state().unwrap().undo.len(), undo);

                    assert_eq!(h.state().session.doc().unwrap().model, original.model);

                    if let Some(dir) = &dir {
                        let workspace = "cad";
                        let renderer = if gpu_canvas { "gpu" } else { "cpu" };
                        h.render().unwrap().save(dir.join(format!("{workspace}-{renderer}-{tag}-{name}.png"))).unwrap();
                    }
                }
            }
        }
    }
}

fn pointer(h: &mut Harness<'_, CadApp>, at: Pos2, pressed: bool) {
    h.event(Event::PointerMoved(at));
    h.event(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    h.run_steps(1);
}

#[test]
fn compass_drag_follows_the_pointer_in_top_bottom_and_isometric_views() {
    let shots = std::env::var_os("CADCRAFT_NAVIGATION_SHOTS").map(std::path::PathBuf::from);
    let mut h = harness(app(Preference::English, ThemePref::Dark), shots.is_some(), false);
    for preset in ["top", "bottom", "seiso"] {
        for degrees in [25f32, -25f32] {
            pose(&mut h, preset, 0.0);
            let cardinal = |h: &Harness<'_, CadApp>, label: &str| {
                texts(h).iter().find(|text| text.galley.text() == label).unwrap().visual_bounding_rect().center()
            };
            let north = cardinal(&h, "N");
            let south = cardinal(&h, "S");
            let east = cardinal(&h, "E");
            let center = north + (south - north) * 0.5;
            let angle = degrees.to_radians();
            let target = center + (north - center) * angle.cos() + (east - center) * angle.sin();
            let selection = h.state().session.selection();
            let document = h.state().session.doc().unwrap().model.clone();
            pointer(&mut h, north, true);
            h.event(Event::PointerMoved(target));
            h.run_steps(2);
            pointer(&mut h, target, false);
            let moved = cardinal(&h, "N");
            assert!(moved.distance(target) < 2.0, "{preset}: compass went to {moved:?}, pointer to {target:?}");
            assert_eq!(h.state().session.selection(), selection);
            assert_eq!(h.state().session.doc().unwrap().model, document);
            assert!(h.state().session.pending_window.is_none());
            if let Some(directory) = &shots {
                h.render().unwrap().save(directory.join(format!("compass-{preset}-{degrees}-follows-pointer.png"))).unwrap();
            }
        }
    }
}

#[test]
fn projected_compass_drag_keeps_its_gesture_and_never_selects_the_model() {
    {
        let mut h = harness(app(Preference::English, ThemePref::Dark), false, false);
        pose(&mut h, "front", 0.0);
        let layout = Layout::new(h.state().canvas.rect.unwrap());
        let at = h
            .output()
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                Shape::LineSegment { points, stroke } if stroke.width == 9.0 && points[0].x > layout.center.x + 45.0 => {
                    Some(points[0].lerp(points[1], 0.5))
                }
                _ => None,
            })
            .unwrap();
        let before = h.state().session.view_direction();
        pointer(&mut h, at, true);
        for i in 1..=6 {
            h.hover_at(at + vec2(i as f32 * 6.0, 0.0));
            h.run_steps(1);
        }
        pointer(&mut h, at + vec2(36.0, 0.0), false);
        h.run_steps(3);
        let after = h.state().session.view_direction();
        assert!((after - before).len() > 0.1);
        assert!(after.z.abs() < 1e-6, "the ring keeps changing azimuth, not orbit elevation");
        assert!(h.state().session.selection().is_empty());
        assert!(h.state().session.pending_window.is_none());
    }
}

#[test]
fn each_visible_face_selects_its_own_orthographic_view() {
    use cadcraft_geom::Vec3;
    {
        let mut h = harness(app(Preference::English, ThemePref::Dark), false, false);
        let before = h.state().session.doc().unwrap().clone();
        for (label, normal, right, up) in [
            ("TOP", Vec3::Z, Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0)),
            ("BOTTOM", -Vec3::Z, Vec3::new(-1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0)),
            ("FRONT", Vec3::new(0.0, -1.0, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::Z),
            ("BACK", Vec3::new(0.0, 1.0, 0.0), Vec3::new(-1.0, 0.0, 0.0), Vec3::Z),
            ("RIGHT", Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::Z),
            ("LEFT", Vec3::new(-1.0, 0.0, 0.0), Vec3::new(0.0, -1.0, 0.0), Vec3::Z),
        ] {
            let direction = normal + right * 0.4 + up * 0.3;
            h.state_mut().run("vpoint", json!({"direction":[direction.x,direction.y,direction.z]})).unwrap();
            h.run_steps(4);
            let at = texts(&h).iter().find(|t| t.galley.text() == label).unwrap().pos;
            pointer(&mut h, at, true);
            pointer(&mut h, at, false);
            h.run_steps(4);
            assert!((h.state().session.view_direction() - normal).len() < 1e-6, "CAD {label}");
            assert!(h.state().session.selection().is_empty());
            assert!(h.state().session.pending_window.is_none());
        }
        let after = h.state().session.doc().unwrap();

        assert_eq!(after.model, before.model);
    }
}

#[test]
fn current_named_ucs_rotates_the_cube_and_face_click_aligns_to_its_axes() {
    use cadcraft_engine::ucs::UcsFrame;
    use cadcraft_geom::Vec3;
    let shots = std::env::var_os("CADCRAFT_NAVIGATION_SHOTS").map(std::path::PathBuf::from);
    {
        let mut h = harness(app(Preference::English, ThemePref::Dark), shots.is_some(), false);
        pose(&mut h, "top", 0.0);
        let north = texts(&h).iter().find(|t| t.galley.text() == "N").unwrap().pos;
        let direction = h.state().session.view_direction();
        let a = 37f64.to_radians();
        let frame = UcsFrame::new(Vec3::new(1.0, 2.0, 0.0), Vec3::new(a.cos(), a.sin(), 0.0), Vec3::new(-a.sin(), a.cos(), 0.0)).unwrap();

        h.state_mut().run("ucs", json!({"option":"origin","origin":[1,2]})).unwrap();
        h.state_mut().run("ucs", json!({"option":"z","angle":37})).unwrap();
        h.state_mut().run("ucs", json!({"option":"named","action":"save","name":"SiteGrid"})).unwrap();

        h.run_steps(4);
        assert_eq!(h.state().session.view_direction(), direction, "a UCS definition does not rotate the camera");
        let new_north = texts(&h).iter().find(|t| t.galley.text() == "N").unwrap().pos;
        assert!(new_north.distance(north) < 1e-4, "north remains world-referenced");
        assert!(texts(&h).iter().any(|t| t.galley.text() == "SiteGrid"));
        let workspace = "cad";
        if let Some(dir) = &shots {
            h.render().unwrap().save(dir.join(format!("{workspace}-ucs-defined.png"))).unwrap();
        }
        let top = texts(&h).iter().find(|t| t.galley.text() == "TOP").unwrap().pos;
        pointer(&mut h, top, true);
        pointer(&mut h, top, false);
        h.run_steps(4);
        assert!((h.state().session.ucs().x - frame.x).len() < 1e-6, "face navigation must not redefine the UCS");
        assert_eq!(h.state().session.ucs_named().1, "SiteGrid");
        let label = texts(&h).iter().find(|t| t.galley.text() == "TOP").copied().unwrap();
        assert!(label.galley.mesh_bounds.width() > label.galley.mesh_bounds.height(), "the UCS top face becomes horizontal");

        let angle = h.state().session.state().unwrap().view().twist + a;
        assert!(angle.sin().abs() < 1e-6 && angle.cos() > 0.999999, "{angle}");

        if let Some(dir) = &shots {
            h.render().unwrap().save(dir.join(format!("{workspace}-ucs-top-clicked.png"))).unwrap();
        }
        // A front-face click is relative to the same named UCS, not the world -Y preset.
        let dir = frame.x * 0.5 - frame.y + frame.z() * 0.4;
        h.state_mut().run("vpoint", json!({"direction":[dir.x,dir.y,dir.z]})).unwrap();
        h.run_steps(4);
        let front = texts(&h).iter().find(|t| t.galley.text() == "FRONT").unwrap().pos;
        pointer(&mut h, front, true);
        pointer(&mut h, front, false);
        h.run_steps(4);
        assert!((h.state().session.view_direction() + frame.y).len() < 1e-6, "the front follows the UCS Y axis");
    }
}

fn control_painted(h: &Harness<'_, CadApp>, at: Pos2) -> bool {
    h.output().shapes.iter().any(|s| match &s.shape {
        Shape::Path(path) => path.closed && path.points.len() == 3 && path.points.iter().all(|p| p.distance(at) < 12.0),
        _ => false,
    })
}

fn home_painted(h: &Harness<'_, CadApp>, layout: Layout) -> bool {
    h.output().shapes.iter().any(|s| match &s.shape {
        Shape::Path(path) => !path.closed && path.points.len() == 3 && path.points.iter().all(|p| layout.home.contains(*p)),
        _ => false,
    })
}

fn camera_up(app: &CadApp) -> cadcraft_geom::Vec3 {
    use cadcraft_geom::{Mat4, Vec3};
    let state = app.session.state().unwrap();
    let [x, y, ..] = state.projection().unwrap_or(Mat4::IDENTITY).m;
    let right = Vec3::new(x[0], x[1], x[2]);
    let up = Vec3::new(y[0], y[1], y[2]);
    let (s, c) = state.view().twist.sin_cos();
    right * s + up * c
}

#[test]
fn face_edge_and_corner_clicks_use_the_same_26_targets_in_world_and_named_ucs() {
    use cadcraft_engine::ucs::UcsFrame;
    use cadcraft_geom::Vec3;
    let x = Vec3::new(1.0, 0.0, 0.0);
    let y = Vec3::new(0.0, 1.0, 0.0);
    {
        for degrees in [0.0_f64, 37.0] {
            let mut h = harness(app(Preference::English, ThemePref::Dark), false, false);
            let (s, c) = degrees.to_radians().sin_cos();
            let frame = UcsFrame::new(Vec3::new(2.0, 3.0, 0.0), Vec3::new(c, s, 0.0), Vec3::new(-s, c, 0.0)).unwrap();
            h.state_mut().session.set_ucs(frame, "SiteGrid").unwrap();
            let world = |v: Vec3| frame.x * v.x + frame.y * v.y + frame.z() * v.z;
            let original = h.state().session.doc().unwrap().clone();
            for (normal, right, up) in [(Vec3::Z, x, y), (-Vec3::Z, -x, y), (-y, x, Vec3::Z), (y, -x, Vec3::Z), (x, y, Vec3::Z), (-x, -y, Vec3::Z)] {
                for horizontal in -1..=1 {
                    for vertical in -1..=1 {
                        let n = world(normal);
                        let u = world(up);
                        h.state_mut().run("vpoint", json!({"direction":[n.x,n.y,n.z],"up":[u.x,u.y,u.z]})).unwrap();
                        h.run_steps(3);
                        let l = Layout::new(h.state().canvas.rect.unwrap());
                        let at = l.center + vec2(horizontal as f32 * 19.68, -vertical as f32 * 19.68);
                        pointer(&mut h, at, true);
                        pointer(&mut h, at, false);
                        h.run_steps(3);
                        let expected = world((normal + right * f64::from(horizontal) + up * f64::from(vertical)).normalized());
                        assert!((h.state().session.view_direction() - expected).len() < 1e-6, "CAD {degrees} {normal:?} {horizontal} {vertical}");
                        let expected_up =
                            if horizontal == 0 && vertical == 0 { world(up) } else { (frame.z() - expected * frame.z().dot(expected)).normalized() };
                        assert!((camera_up(h.state()) - expected_up).len() < 1e-6);
                        assert!(h.state().session.selection().is_empty());
                        assert!(h.state().session.pending_window.is_none());
                    }
                }
            }
            assert_eq!(h.state().session.doc().unwrap().model, original.model);
        }
    }
}

#[test]
fn adjacent_arrow_clicks_keep_side_faces_upright_and_pole_heading() {
    use cadcraft_geom::Vec3;
    {
        let mut h = harness(app(Preference::English, ThemePref::Dark), false, false);
        for (preset, offset, direction, up) in
            [("top", vec2(37.0, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::Z), ("back", vec2(0.0, -37.0), Vec3::Z, Vec3::new(0.0, -1.0, 0.0))]
        {
            pose(&mut h, preset, 0.0);
            let l = Layout::new(h.state().canvas.rect.unwrap());
            pointer(&mut h, l.center + offset, true);
            pointer(&mut h, l.center + offset, false);
            h.run_steps(3);
            assert!((h.state().session.view_direction() - direction).len() < 1e-6);
            assert!((camera_up(h.state()) - up).len() < 1e-6, "CAD {preset}");
        }
    }
}

#[test]
fn edge_and_corner_highlights_and_snaps_in_both_languages_and_themes() {
    let dir = std::env::var_os("CADCRAFT_NAVIGATION_SHOTS").map(std::path::PathBuf::from);
    if let Some(dir) = &dir {
        std::fs::create_dir_all(dir).unwrap();
    }
    for gpu_canvas in [false, true] {
        if gpu_canvas && dir.is_none() {
            continue;
        }
        let renderer = if gpu_canvas { "gpu" } else { "cpu" };
        for (language, theme, name) in [
            (Preference::English, ThemePref::Light, "en-light"),
            (Preference::English, ThemePref::Dark, "en-dark"),
            (Preference::BrazilianPortuguese, ThemePref::Light, "pt-light"),
            (Preference::BrazilianPortuguese, ThemePref::Dark, "pt-dark"),
        ] {
            let mut h = harness(app(language, theme), dir.is_some(), gpu_canvas);
            for (tag, offset) in [("edge", vec2(0.0, 19.68)), ("corner", vec2(19.68, 19.68))] {
                pose(&mut h, "top", 0.0);
                let l = Layout::new(h.state().canvas.rect.unwrap());
                let at = l.center + offset;
                h.hover_at(at);
                h.run_steps(3);
                assert!(h.output().shapes.iter().any(|s| matches!(&s.shape, Shape::Path(p) if p.fill == egui::Color32::from_rgb(0xab,0xd5,0xf3))));
                let capture = |h: &mut Harness<'_, CadApp>, state| {
                    if let Some(dir) = &dir {
                        h.render().unwrap().save(dir.join(format!("regions-cad-{renderer}-{name}-{tag}-{state}.png"))).unwrap();
                    }
                };
                capture(&mut h, "hover");
                pointer(&mut h, at, true);
                pointer(&mut h, at, false);
                h.run_steps(3);
                capture(&mut h, "view");
            }
        }
    }
}

#[test]
fn hover_controls_navigate_adjacent_faces_and_roll_without_changing_zoom_or_selecting() {
    use cadcraft_geom::Vec3;
    let dir = std::env::var_os("CADCRAFT_NAVIGATION_SHOTS").map(std::path::PathBuf::from);
    if let Some(dir) = &dir {
        std::fs::create_dir_all(dir).unwrap();
    }
    for gpu_canvas in [false, true] {
        if gpu_canvas && dir.is_none() {
            continue;
        }
        let renderer = if gpu_canvas { "gpu" } else { "cpu" };
        for (language, theme, name) in [
            (Preference::English, ThemePref::Light, "en-light"),
            (Preference::English, ThemePref::Dark, "en-dark"),
            (Preference::BrazilianPortuguese, ThemePref::Light, "pt-light"),
            (Preference::BrazilianPortuguese, ThemePref::Dark, "pt-dark"),
        ] {
            let mut h = harness(app(language, theme), dir.is_some(), gpu_canvas);
            let original = h.state().session.doc().unwrap().clone();
            pose(&mut h, "top", 0.0);
            let l = Layout::new(h.state().canvas.rect.unwrap());
            let offsets = [vec2(0.0, -37.0), vec2(0.0, 37.0), vec2(-37.0, 0.0), vec2(37.0, 0.0), vec2(46.0, -72.0), vec2(74.0, -43.0)];
            assert!(!home_painted(&h, l));
            assert!(offsets.iter().all(|o| !control_painted(&h, l.center + *o)));
            let capture = |h: &mut Harness<'_, CadApp>, tag: &str| {
                if let Some(dir) = &dir {
                    h.render().unwrap().save(dir.join(format!("hover-cad-{renderer}-{name}-{tag}.png"))).unwrap();
                }
            };
            capture(&mut h, "idle");
            h.hover_at(l.center);
            h.run_steps(3);
            assert!(home_painted(&h, l));
            assert!(offsets.iter().all(|o| control_painted(&h, l.center + *o)));
            capture(&mut h, "face");
            pose(&mut h, "top", 37.0);
            h.hover_at(l.center);
            h.run_steps(3);
            capture(&mut h, "face-37");
            for (tag, offset) in [("rotate-cw-hover", offsets[5]), ("rotate-ccw-hover", offsets[4])] {
                h.hover_at(l.center + offset);
                h.run_steps(3);
                capture(&mut h, tag);
            }
            for (offset, normal) in
                offsets[..4].iter().zip([Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, -1.0, 0.0), Vec3::new(-1.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0)])
            {
                for degrees in [0.0_f32, 37.0, 90.0] {
                    pose(&mut h, "top", f64::from(degrees));
                    let (s, c) = degrees.to_radians().sin_cos();
                    let at = l.center + vec2(offset.x * c + offset.y * s, -offset.x * s + offset.y * c);
                    h.hover_at(at);
                    h.run_steps(2);
                    assert!(control_painted(&h, at), "the inclined control must be painted at its hit region");
                    pointer(&mut h, at, true);
                    pointer(&mut h, at, false);
                    h.run_steps(3);
                    assert!((h.state().session.view_direction() - normal).len() < 1e-6, "CAD {name} {offset:?} {degrees}");
                    assert!(h.state().session.selection().is_empty());
                    assert!(h.state().session.pending_window.is_none());
                }
            }
            for preset in ["top", "front"] {
                pose(&mut h, preset, 37.0);
                let before = h.state().session.state().unwrap().view();
                let direction = h.state().session.view_direction();
                for (offset, expected) in [(offsets[5], -53.0_f64), (offsets[4], 37.0)] {
                    pointer(&mut h, l.center + offset, true);
                    pointer(&mut h, l.center + offset, false);
                    h.run_steps(3);
                    assert!((h.state().session.view_direction() - direction).len() < 1e-6);
                    let now = h.state().session.state().unwrap().view();
                    assert_eq!(now.center, before.center);
                    assert_eq!(now.height, before.height);
                    assert!((now.twist - expected.to_radians()).sin().abs() < 1e-6);
                }
            }
            capture(&mut h, "front-hover");
            pose(&mut h, "seiso", 0.0);
            h.hover_at(l.center);
            h.run_steps(3);
            assert!(home_painted(&h, l));
            assert!(offsets.iter().all(|o| !control_painted(&h, l.center + *o)));
            capture(&mut h, "iso");
            h.event(Event::PointerGone);
            h.run_steps(3);
            assert!(!home_painted(&h, l));
            assert_eq!(h.state().session.doc().unwrap().model, original.model);
        }
    }
}

#[test]
fn compass_tracks_the_pointer_in_both_directions_at_the_poles_and_in_3d() {
    {
        let mut h = harness(app(Preference::English, ThemePref::Dark), false, false);
        for preset in ["top", "bottom", "seiso"] {
            for sign in [-1.0_f32, 1.0] {
                pose(&mut h, preset, 37.0);
                let label = |name| texts(&h).iter().find(|t| t.galley.text() == name).unwrap().pos;
                let east = label("E");
                let center = east.lerp(label("W"), 0.5);
                let x = east - center;
                let y = label("N") - center;
                pointer(&mut h, east, true);
                for i in 1..=10 {
                    let angle = sign * (i as f32 * 2.0).to_radians();
                    h.hover_at(center + x * angle.cos() + y * angle.sin());
                    h.run_steps(1);
                }
                let angle = sign * 20.0_f32.to_radians();
                let end = center + x * angle.cos() + y * angle.sin();
                pointer(&mut h, end, false);
                h.run_steps(3);
                let east = texts(&h).iter().find(|t| t.galley.text() == "E").unwrap().pos;
                assert!(east.distance(end) < 1.0, "compass must follow the pointer: CAD {preset} {sign}: {east:?} vs {end:?}");
                assert!(h.state().session.selection().is_empty());
                assert!(h.state().session.pending_window.is_none());
            }
        }
    }
}
