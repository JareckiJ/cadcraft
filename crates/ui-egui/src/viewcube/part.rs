//! Original 3D navigation overlay, sharing the drawing ViewCube's layout and visibility switch.
use super::{Layout, paint_home};
use crate::{CadApp, part_editor, theme::Tokens};
use cadcraft_geom::{Vec2, Vec3};
use egui::{Align2, CursorIcon, FontId, Pos2, Rect, Sense, Shape, Stroke, vec2};
use serde_json::json;

const FACES: [([f64; 3], &str, [usize; 4]); 6] = [
    ([1.0, 0.0, 0.0], "RIGHT", [1, 3, 7, 5]),
    ([-1.0, 0.0, 0.0], "LEFT", [0, 4, 6, 2]),
    ([0.0, 1.0, 0.0], "BACK", [2, 6, 7, 3]),
    ([0.0, -1.0, 0.0], "FRONT", [0, 1, 5, 4]),
    ([0.0, 0.0, 1.0], "TOP", [4, 5, 7, 6]),
    ([0.0, 0.0, -1.0], "BOTTOM", [0, 2, 3, 1]),
];
#[derive(Clone, Copy, Debug, PartialEq)]
enum Hit {
    Home,
    View([f64; 3]),
}
struct Face {
    normal: [f64; 3],
    label: &'static str,
    ids: [usize; 4],
    points: [Pos2; 4],
}
fn vertex(i: usize) -> Vec3 {
    Vec3::new(if i & 1 == 0 { -1.0 } else { 1.0 }, if i & 2 == 0 { -1.0 } else { 1.0 }, if i & 4 == 0 { -1.0 } else { 1.0 })
}
fn coordinates(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}
fn faces(app: &CadApp, l: &Layout) -> Vec<Face> {
    let (right, up, eye) = part_editor::navigation_axes(app);
    FACES
        .iter()
        .filter_map(|&(normal, label, ids)| {
            let n = Vec3::new(normal[0], normal[1], normal[2]);
            if n.dot(eye) < 1e-6 {
                return None;
            }
            let points = ids.map(|i| {
                let v = vertex(i);
                l.center + vec2((v.dot(right) * 22.0) as f32, (-v.dot(up) * 22.0) as f32)
            });
            Some(Face { normal, label, ids, points })
        })
        .collect()
}
fn hit(l: &Layout, faces: &[Face], at: Pos2) -> Option<Hit> {
    if l.home.contains(at) {
        return Some(Hit::Home);
    }
    // Corners and edges have precedence over the face interior, giving all 26 snap directions.
    for face in faces {
        for (&id, &p) in face.ids.iter().zip(&face.points) {
            if at.distance(p) <= 6.0 {
                return Some(Hit::View(coordinates(vertex(id))));
            }
        }
    }
    for face in faces {
        for i in 0..4 {
            let j = (i + 1) % 4;
            let (a, b) = (face.points[i], face.points[j]);
            let ab = b - a;
            let t = ((at - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
            if at.distance(a + ab * t) <= 5.0 {
                return Some(Hit::View(coordinates((vertex(face.ids[i]) + vertex(face.ids[j])) * 0.5)));
            }
        }
    }
    for face in faces {
        let points = face.points.map(|p| Vec2::new(f64::from(p.x), f64::from(p.y)));
        if cadcraft_geom::point_in_polygon(&points, Vec2::new(f64::from(at.x), f64::from(at.y))) {
            return Some(Hit::View(face.normal));
        }
    }
    let offset = at - l.center;
    if (l.ring - 10.0..=l.ring + 10.0).contains(&offset.length()) {
        return Some(Hit::View(if offset.x.abs() > offset.y.abs() {
            [offset.x.signum() as f64, 0.0, 0.0]
        } else {
            [0.0, -offset.y.signum() as f64, 0.0]
        }));
    }
    None
}
pub(crate) fn part_pointer_over(app: &CadApp, ui: &egui::Ui, canvas: Rect) -> bool {
    app.ui.show_viewcube
        && part_editor::is_part(app)
        && ui.input(|i| {
            let l = Layout::new(canvas);
            i.pointer.hover_pos().is_some_and(|p| l.bounds.union(l.pill).contains(p))
        })
}
pub(crate) fn show_part(app: &mut CadApp, ui: &mut egui::Ui, canvas: Rect) {
    if !app.ui.show_viewcube {
        return;
    }
    let l = Layout::new(canvas);
    let t = Tokens::get();
    let faces = faces(app, &l);
    let resp = ui.interact(l.bounds, ui.scope_id().with("part_viewcube"), Sense::click_and_drag());
    let hovered = resp.hover_pos().and_then(|p| hit(&l, &faces, p));
    let painter = ui.painter_at(canvas);
    painter.circle_stroke(l.center, l.ring, Stroke::new(8.0, t.control));
    painter.circle_stroke(l.center, l.ring + 4.0, Stroke::new(1.0, t.text_dim));
    for (label, offset) in [("N", vec2(0.0, -l.ring)), ("S", vec2(0.0, l.ring)), ("E", vec2(l.ring, 0.0)), ("W", vec2(-l.ring, 0.0))] {
        painter.text(l.center + offset, Align2::CENTER_CENTER, label, FontId::proportional(15.0), t.text);
    }
    for face in &faces {
        let hot = hovered == Some(Hit::View(face.normal));
        painter.add(Shape::convex_polygon(face.points.to_vec(), if hot { t.selection } else { t.control }, Stroke::new(1.2, t.text_dim)));
        let center = face.points.iter().fold(egui::Vec2::ZERO, |sum, p| sum + p.to_vec2()) / 4.0;
        painter.text(center.to_pos2(), Align2::CENTER_CENTER, face.label, FontId::proportional(9.0), t.text);
    }
    if let Some(Hit::View(direction)) = hovered {
        // Highlight the selected edge/corner as well as faces.
        let v = Vec3::new(direction[0], direction[1], direction[2]);
        let (right, up, _) = part_editor::navigation_axes(app);
        let at = l.center + vec2((v.dot(right) * 22.0) as f32, (-v.dot(up) * 22.0) as f32);
        if direction.iter().filter(|v| v.abs() > 0.5).count() > 1 {
            painter.circle_filled(at, 4.0, t.selection);
        }
    }
    paint_home(&painter, l.home, hovered == Some(Hit::Home));
    let resp = if hovered.is_some() { resp.on_hover_cursor(CursorIcon::PointingHand) } else { resp };
    let resp = resp.on_hover_text(if hovered == Some(Hit::Home) {
        "Home: default isometric view"
    } else {
        "Click a face, edge or corner to orient the view; drag to orbit"
    });
    if resp.clicked() {
        match resp.interact_pointer_pos().and_then(|p| hit(&l, &faces, p)) {
            Some(Hit::Home) => {
                let _ = app.run("ui.part.view", json!({"view":"home"}));
            }
            Some(Hit::View(direction)) => {
                let _ = app.run("ui.part.view", json!({"direction":direction}));
            }
            None => {}
        }
    }
    if resp.dragged_by(egui::PointerButton::Primary) || resp.dragged_by(egui::PointerButton::Middle) {
        let d = ui.input(|i| i.pointer.delta());
        let _ = app.run("ui.part.orbit", json!({"delta":[d.x,d.y]}));
    }
    let mut view = None;
    resp.context_menu(|ui| {
        for (name, label) in [
            ("home", "Home (Isometric)"),
            ("top", "Top"),
            ("bottom", "Bottom"),
            ("front", "Front"),
            ("back", "Back"),
            ("left", "Left"),
            ("right", "Right"),
        ] {
            if ui.button(label).clicked() {
                view = Some(name);
                ui.close();
            }
        }
        if ui.button("Hide ViewCube").clicked() {
            let _ = app.run("ui.toggle.viewcube", json!({"on":false}));
            ui.close();
        }
    });
    if let Some(name) = view {
        let _ = app.run("ui.part.view", json!({"view":name}));
    }
    painter.rect_filled(l.pill, 8.0, t.control);
    painter.text(l.pill.center() - vec2(4.0, 0.0), Align2::CENTER_CENTER, "WCS", FontId::proportional(11.0), t.text);
    let c = l.pill.right_center() - vec2(9.0, 0.0);
    painter.line(vec![c + vec2(-3.0, -1.5), c + vec2(0.0, 1.5), c + vec2(3.0, -1.5)], Stroke::new(1.0, t.text));
    let pill = ui.interact(l.pill, ui.scope_id().with("part_viewcube_wcs"), Sense::click_and_drag()).on_hover_text("World Coordinate System");
    egui::Popup::menu(&pill).show(|ui| {
        ui.label("World (WCS)");
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;

    fn part() -> CadApp {
        let mut app = CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
        app.ui.in_window_menu = true;
        app.run("part.new", json!({})).unwrap();
        app.run("extrude", json!({"width":40,"height":20,"depth":10})).unwrap();
        app
    }
    #[test]
    fn navigation_presets_cover_all_26_directions_and_reject_invalid_input() {
        let mut app = part();
        let original = app.session.doc().unwrap().clone();
        for x in -1..=1 {
            for y in -1..=1 {
                for z in -1..=1 {
                    if x == 0 && y == 0 && z == 0 {
                        continue;
                    }
                    app.run("ui.part.view", json!({"direction":[x,y,z]})).unwrap();
                    let (_, _, eye) = part_editor::navigation_axes(&app);
                    assert!(eye.dot(Vec3::new(x as f64, y as f64, z as f64).normalized()) > 1.0 - 1e-9);
                }
            }
        }
        for value in [json!({"direction":[0,0,0]}), json!({"direction":[2,0,0]}), json!({"view":"invalid"}), json!({"direction":[1,2]})] {
            let before = part_editor::navigation_axes(&app);
            assert!(app.run("ui.part.view", value).is_err());
            assert_eq!(part_editor::navigation_axes(&app), before);
        }
        app.run("ui.part.view", json!({"view":"home"})).unwrap();
        let (_, _, eye) = part_editor::navigation_axes(&app);
        assert!(eye.dot(Vec3::new(1.0, -1.0, 1.0).normalized()) > 1.0 - 1e-9);
        assert_eq!(*app.session.doc().unwrap(), original);
    }
    #[test]
    fn sketch_presets_match_model_orientation_on_every_datum_plane() {
        let mut app = part();
        for plane in 1..=3 {
            for name in ["home", "top", "bottom", "front", "back", "left", "right"] {
                app.run("ui.part.view", json!({"view":name})).unwrap();
                let expected = part_editor::navigation_axes(&app);
                app.run("part.sketch.begin", json!({"plane":plane})).unwrap();
                app.run("ui.part.view", json!({"view":name})).unwrap();
                let actual = part_editor::navigation_axes(&app);
                assert!(actual.0.dot(expected.0) > 0.999, "{name} on plane {plane}: right axis differs");
                assert!(actual.1.dot(expected.1) > 0.999, "{name} on plane {plane}: up axis differs");
                assert!(actual.2.dot(expected.2) > 0.999, "{name} on plane {plane}: viewing direction differs");
                app.run("part.sketch.cancel", json!({})).unwrap();
            }
        }
    }
    #[test]
    fn cube_clicks_home_faces_and_sketch_views_without_editing_geometry() {
        fn click(h: &mut Harness<'_, CadApp>, at: Pos2) {
            h.event(egui::Event::PointerMoved(at));
            h.run_steps(1);
            for pressed in [true, false] {
                h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::default() });
                h.run_steps(1);
            }
            h.run_steps(2);
        }
        fn save(h: &mut Harness<'_, CadApp>, name: &str) {
            if let Some(dir) = std::env::var_os("CADCRAFT_PART_UI_ARTIFACTS") {
                let dir = std::path::PathBuf::from(dir);
                std::fs::create_dir_all(&dir).unwrap();
                h.render().unwrap().save(dir.join(name)).unwrap();
            }
        }
        let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).build_ui_state(
            |ui, app: &mut CadApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            part(),
        );
        h.run_steps(3);
        save(&mut h, "cube-isometric.png");
        let l = Layout::new(h.state().canvas.rect.unwrap());
        let top = faces(h.state(), &l).into_iter().find(|f| f.label == "TOP").unwrap();
        let at = (top.points.iter().fold(egui::Vec2::ZERO, |sum, p| sum + p.to_vec2()) / 4.0).to_pos2();
        click(&mut h, at);
        assert!(part_editor::navigation_axes(h.state()).2.z > 0.999);
        save(&mut h, "cube-top.png");
        click(&mut h, l.home.center());
        assert!(part_editor::navigation_axes(h.state()).2.dot(Vec3::new(1.0, -1.0, 1.0).normalized()) > 0.999);
        h.state_mut().run("part.sketch.begin", json!({"sketch":1000})).unwrap();
        h.run_steps(3);
        let geometry = h.state().session.doc().unwrap().clone();
        for name in ["top", "bottom", "front", "back", "left", "right", "home"] {
            h.state_mut().run("ui.part.view", json!({"view":name})).unwrap();
            let eye = part_editor::navigation_axes(h.state()).2;
            assert!(eye.is_finite());
        }
        h.run_steps(2);
        save(&mut h, "cube-sketch.png");
        let l = Layout::new(h.state().canvas.rect.unwrap());
        click(&mut h, l.home.center());
        assert!(part_editor::navigation_axes(h.state()).2.dot(Vec3::new(1.0, -1.0, 1.0).normalized()) > 0.999);
        assert_eq!(*h.state().session.doc().unwrap(), geometry);
    }
}
