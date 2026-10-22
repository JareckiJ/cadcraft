//! Navigation geometry derived from the renderer's camera basis, with no camera state of its own.

use cadcraft_geom::{Mat4, Vec3};
use egui::{Color32, FontId, Painter, Pos2, Shape, Stroke, Vec2, vec2};

use super::{Layout, Part};
use crate::CadApp;

const X: Vec3 = Vec3::new(1.0, 0.0, 0.0);
const Y: Vec3 = Vec3::new(0.0, 1.0, 0.0);

#[derive(Clone, Copy, Debug)]
pub(super) struct Camera {
    right: Vec3,
    up: Vec3,
    toward: Vec3,
}

impl Camera {
    pub fn of(app: &CadApp) -> Self {
        let Ok(state) = app.session.state() else { return Self::projection(Mat4::IDENTITY, 0.0) };
        let matrix = state.active_viewport().and_then(|(_, v)| v.view3d).map_or_else(|| state.projection().unwrap_or(Mat4::IDENTITY), |v| v.matrix());
        Self::projection(matrix, state.view().twist)
    }

    pub fn axes(right: Vec3, up: Vec3) -> Self {
        if !right.is_finite()
            || !up.is_finite()
            || (right.len() - 1.0).abs() > 1e-5
            || (up.len() - 1.0).abs() > 1e-5
            || !right.len().is_finite()
            || !up.len().is_finite()
            || right.dot(up).abs() > 1e-5
        {
            return Self { right: X, up: Y, toward: Vec3::Z };
        }
        Self { right, up, toward: right.cross(up).normalized() }
    }

    pub fn projection(m: Mat4, twist: f64) -> Self {
        // Mat4's rows are the same untwisted basis used by the CAD display list.
        let [x, y, ..] = m.m;
        Self::axes(Vec3::new(x[0], x[1], x[2]), Vec3::new(y[0], y[1], y[2])).turned(twist)
    }

    fn turned(self, twist: f64) -> Self {
        if !twist.is_finite() {
            return self;
        }
        let (s, c) = twist.sin_cos();
        Self::axes(self.right * c - self.up * s, self.right * s + self.up * c)
    }

    pub fn screen(self, v: Vec3) -> Vec2 {
        vec2(v.dot(self.right) as f32, -v.dot(self.up) as f32)
    }

    fn in_ucs(self, u: cadcraft_engine::ucs::UcsFrame) -> Self {
        let local = |v: Vec3| Vec3::new(v.dot(u.x), v.dot(u.y), v.dot(u.z()));
        Self::axes(local(self.right), local(self.up))
    }

    fn depth(self, v: Vec3) -> f64 {
        v.dot(self.toward)
    }
}

const HALF: f32 = 24.0;
const SAMPLES: usize = 128;
const FACE_CENTER: f64 = 0.64;

struct Face {
    normal: Vec3,
    right: Vec3,
    up: Vec3,
    label: &'static str,
}

const FACES: [Face; 6] = [
    Face { normal: Vec3::Z, right: X, up: Y, label: "TOP" },
    Face { normal: Vec3::new(0.0, 0.0, -1.0), right: Vec3::new(-1.0, 0.0, 0.0), up: Y, label: "Bottom" },
    Face { normal: Vec3::new(0.0, -1.0, 0.0), right: X, up: Vec3::Z, label: "Front" },
    Face { normal: Y, right: Vec3::new(-1.0, 0.0, 0.0), up: Vec3::Z, label: "Back" },
    Face { normal: X, right: Y, up: Vec3::Z, label: "Right" },
    Face { normal: Vec3::new(-1.0, 0.0, 0.0), right: Vec3::new(0.0, -1.0, 0.0), up: Vec3::Z, label: "Left" },
];

struct Region {
    horizontal: i8,
    vertical: i8,
    direction: Vec3,
}

fn regions(face: &Face) -> impl Iterator<Item = Region> + '_ {
    (-1..=1).flat_map(move |horizontal| {
        (-1..=1).map(move |vertical| Region {
            horizontal,
            vertical,
            direction: face.normal + face.right * f64::from(horizontal) + face.up * f64::from(vertical),
        })
    })
}

fn interval(region: i8) -> (f64, f64) {
    match region {
        -1 => (-1.0, -FACE_CENTER),
        0 => (-FACE_CENTER, FACE_CENTER),
        _ => (FACE_CENTER, 1.0),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Target {
    pub normal: Vec3,
    pub right: Vec3,
    pub label: &'static str,
}

pub(super) struct Widget {
    camera: Camera,
    cube: Camera,
    ucs: cadcraft_engine::ucs::UcsFrame,
    layout: Layout,
    ring_center: Pos2,
    ring: [Pos2; SAMPLES + 1],
}

impl Widget {
    #[cfg(test)]
    pub fn new(camera: Camera, layout: Layout) -> Self {
        Self::in_ucs(camera, layout, cadcraft_engine::ucs::UcsFrame::WORLD)
    }

    pub fn in_ucs(camera: Camera, layout: Layout, ucs: cadcraft_engine::ucs::UcsFrame) -> Self {
        let ucs = cadcraft_engine::ucs::UcsFrame::new(ucs.origin, ucs.x, ucs.y).unwrap_or_default();
        let ring_center = layout.center - camera.screen(Vec3::Z) * (HALF * 1.4);
        let ring = std::array::from_fn(|i| {
            let a = std::f64::consts::TAU * i as f64 / SAMPLES as f64;
            ring_center + camera.screen(Vec3::new(a.cos(), a.sin(), 0.0)) * layout.ring
        });
        Self { camera, cube: camera.in_ucs(ucs), ucs, layout, ring_center, ring }
    }

    fn polygon(&self, face: &Face) -> [Pos2; 4] {
        [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .map(|(x, y)| self.layout.center + self.cube.screen(face.normal + face.right * x + face.up * y) * HALF)
    }

    fn region_polygon(&self, face: &Face, region: &Region) -> [Pos2; 4] {
        let (left, right) = interval(region.horizontal);
        let (bottom, top) = interval(region.vertical);
        [(left, bottom), (right, bottom), (right, top), (left, top)]
            .map(|(x, y)| self.layout.center + self.cube.screen(face.normal + face.right * x + face.up * y) * HALF)
    }

    fn region_target(&self, face: &Face, region: &Region) -> Target {
        let normal = region.direction.normalized();
        let (right, label) = match (region.horizontal != 0, region.vertical != 0) {
            (false, false) => (face.right, face.label),
            (true, true) => (Vec3::Z.cross(normal).normalized(), "Corner"),
            _ => (Vec3::Z.cross(normal).normalized(), "Edge"),
        };
        Target { normal: self.world(normal), right: self.world(right), label }
    }

    pub fn interaction_bounds(&self) -> egui::Rect {
        let mut bounds = egui::Rect::from_points(&self.ring).expand(10.0).union(self.layout.home);
        for face in FACES.iter().filter(|f| self.cube.depth(f.normal) > 1e-6) {
            bounds = bounds.union(egui::Rect::from_points(&self.polygon(face)));
        }
        if self.aligned_face().is_some() {
            for (_, rect) in self.controls() {
                bounds = bounds.union(rect);
            }
        }
        bounds
    }

    fn aligned_face(&self) -> Option<&'static Face> {
        FACES.iter().find(|face| self.cube.depth(face.normal) > 1.0 - 1e-6)
    }

    fn world(&self, v: Vec3) -> Vec3 {
        self.ucs.x * v.x + self.ucs.y * v.y + self.ucs.z() * v.z
    }

    /// Adjacent-face controls rotate with the face; quarter-turn buttons stay in the corner.
    fn controls(&self) -> [(Part, egui::Rect); 6] {
        let (right, up) =
            self.aligned_face().map_or((vec2(1.0, 0.0), vec2(0.0, -1.0)), |face| (self.cube.screen(face.right), self.cube.screen(face.up)));
        let gap = HALF + 13.0;
        [
            (Part::Up, up * gap),
            (Part::Down, -up * gap),
            (Part::Left, -right * gap),
            (Part::Right, right * gap),
            (Part::Counterclockwise, vec2(46.0, -72.0)),
            (Part::Clockwise, vec2(74.0, -43.0)),
        ]
        .map(|(part, offset)| {
            let size = match part {
                Part::Clockwise => vec2(26.0, 24.0),
                Part::Counterclockwise => vec2(26.0, 20.0),
                _ => Vec2::splat(18.0),
            };
            (part, egui::Rect::from_center_size(self.layout.center + offset, size))
        })
    }

    pub fn adjacent_target(&self, part: Part) -> Option<Target> {
        let face = self.aligned_face()?;
        let direction = match part {
            Part::Up => vec2(0.0, -1.0),
            Part::Down => vec2(0.0, 1.0),
            Part::Left => vec2(-1.0, 0.0),
            Part::Right => vec2(1.0, 0.0),
            _ => return None,
        };
        let normal = face.right * f64::from(direction.x) - face.up * f64::from(direction.y);
        let target = FACES.iter().find(|f| (f.normal - normal).len() < 1e-6)?;
        // Side faces retain UCS Z as their vertical. At a pole, retain the departing
        // face's heading: Back -> Top shows Top upside down.
        let right = if normal.z.abs() > 0.5 { face.right } else { target.right };
        let up = normal.cross(right);
        let heading = self.cube.screen(face.right);
        let roll = f64::from((-heading.y).atan2(heading.x));
        let (s, c) = roll.sin_cos();
        Some(Target { normal: self.world(normal), right: self.world(right * c - up * s), label: target.label })
    }

    pub fn paint_controls(&self, p: &Painter, hot: Option<Part>) {
        if self.aligned_face().is_none() {
            return;
        }
        let ink = crate::theme::Tokens::get().canvas_ink;
        for (part, rect) in self.controls() {
            let roll = matches!(part, Part::Clockwise | Part::Counterclockwise);
            if hot == Some(part) && !roll {
                p.rect_filled(rect, 3.0, crate::theme::Tokens::get().control_hover);
            }
            let c = rect.center();
            if roll {
                let ink = if hot == Some(part) { Color32::from_rgb(0xab, 0xd5, 0xf3) } else { ink.gamma_multiply(0.75) };
                paint_roll_arrow(p, c, part == Part::Clockwise, ink);
            } else {
                let inward = (self.layout.center - c).normalized();
                let side = vec2(-inward.y, inward.x);
                p.add(Shape::convex_polygon(vec![c + inward * 5.0, c - inward * 3.0 + side * 5.0, c - inward * 3.0 - side * 5.0], ink, Stroke::NONE));
            }
        }
    }

    pub fn face_at(&self, p: Pos2) -> Option<Target> {
        FACES.iter().filter(|face| self.cube.depth(face.normal) > 1e-6).find_map(|face| {
            regions(face).find(|region| contains(self.region_polygon(face, region), p)).map(|region| self.region_target(face, &region))
        })
    }

    pub fn part_at(&self, p: Pos2) -> Option<Part> {
        if self.layout.home.contains(p) {
            return Some(Part::Home);
        }
        if self.aligned_face().is_some()
            && let Some((part, _)) = self.controls().into_iter().find(|(_, rect)| rect.contains(p))
        {
            return Some(part);
        }
        if self.face_at(p).is_some() {
            return Some(Part::Face);
        }
        self.ring
            .windows(2)
            .any(|pair| {
                let [a, b] = pair else { return false };
                let v = *b - *a;
                let t = ((p - *a).dot(v) / v.length_sq().max(1e-8)).clamp(0.0, 1.0);
                p.distance(*a + v * t) <= 10.0
            })
            .then_some(Part::Ring)
    }

    pub fn ring_delta(&self, from: Pos2, to: Pos2) -> f64 {
        let x = self.camera.screen(X);
        let y = self.camera.screen(Y);
        let det = x.x * y.y - x.y * y.x;
        if det.abs() < 0.05 {
            // An edge-on ring has no invertible XY plane. Follow its projected horizon,
            // including camera roll, instead of assuming it is horizontal on screen.
            let horizon = self.camera.screen(Vec3::Z.cross(self.camera.toward).normalized());
            return f64::from((to - from).dot(horizon) / self.layout.ring);
        }
        let angle = |p: Pos2| {
            let p = p - self.ring_center;
            let a = (p.x * y.y - p.y * y.x) / det;
            let b = (x.x * p.y - x.y * p.x) / det;
            f64::from(b.atan2(a))
        };
        (angle(to) - angle(from) + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI
    }

    /// Convert pointer travel into the existing camera gesture. At the poles the engine
    /// uses screen roll instead of azimuth; their signs differ for a top view.
    pub fn ring_turn(&self, from: Pos2, to: Pos2) -> f64 {
        let delta = self.ring_delta(from, to);
        if self.camera.toward.z.abs() > 1.0 - 1e-12 { delta * self.camera.toward.z.signum() } else { -delta }
    }

    pub fn paint(&self, p: &Painter, hot: Option<Part>, pointer: Option<Pos2>) {
        self.paint_ring(p, hot, false);
        let hovered = pointer.and_then(|p| self.face_at(p));
        for face in FACES.iter().filter(|f| self.cube.depth(f.normal) > 1e-6) {
            let light = if face.normal.z.abs() > 0.5 {
                174
            } else if face.normal.x.abs() > 0.5 {
                139
            } else {
                154
            };
            p.add(Shape::convex_polygon(self.polygon(face).to_vec(), Color32::from_gray(light), Stroke::new(1.0, Color32::from_gray(100))));
            for region in regions(face) {
                let selected = hovered.is_some_and(|target| (target.normal - self.region_target(face, &region).normal).len() < 1e-6);
                if selected || (region.horizontal == 0 && region.vertical == 0) {
                    let fill = if selected { Color32::from_rgb(0xab, 0xd5, 0xf3) } else { Color32::from_gray(light.saturating_sub(12)) };
                    let stroke = if selected { Stroke::new(1.0, Color32::from_rgb(0x24, 0x8c, 0xd5)) } else { Stroke::NONE };
                    p.add(Shape::convex_polygon(self.region_polygon(face, &region).to_vec(), fill, stroke));
                }
            }
            let label = crate::i18n::t(face.label).to_uppercase();
            let at = self.layout.center + self.cube.screen(face.normal) * HALF;
            plane_text(p, at, &label, 12.0, self.cube.screen(face.right), -self.cube.screen(face.up), 42.0, Color32::from_rgb(0x46, 0x4c, 0x54));
        }
        self.paint_ring(p, hot, true);
    }

    fn paint_ring(&self, p: &Painter, hot: Option<Part>, near: bool) {
        let color = if hot == Some(Part::Ring) { Color32::from_rgb(0x5c, 0x67, 0x7c) } else { Color32::from_rgb(0x48, 0x50, 0x5c) };
        for (i, pair) in self.ring.windows(2).enumerate() {
            let a = std::f64::consts::TAU * (i as f64 + 0.5) / SAMPLES as f64;
            let front = self.camera.depth(Vec3::new(a.cos(), a.sin(), 0.0)) > 1e-6;
            if front == near
                && let [a, b] = pair
            {
                p.line_segment([*a, *b], Stroke::new(9.0, color));
            }
        }
        // At an edge-on angle the compass lettering also collapses; do not invent a direction.
        if self.camera.depth(Vec3::Z).abs() < 0.18 {
            return;
        }
        for (label, direction) in [("N", Y), ("S", -Y), ("E", X), ("W", -X)] {
            if (self.camera.depth(direction) > 1e-6) == near {
                plane_text(
                    p,
                    self.ring_center + self.camera.screen(direction) * self.layout.ring,
                    crate::i18n::t(label),
                    17.0,
                    self.camera.screen(X),
                    -self.camera.screen(Y),
                    24.0,
                    Color32::from_rgb(0xc8, 0xcc, 0xd2),
                );
            }
        }
    }
}

/// A filled curved shaft and broad arrowhead, matching the reference navigation controls.
fn paint_roll_arrow(p: &Painter, center: Pos2, clockwise: bool, ink: Color32) {
    let transform = |point: Vec2| center + if clockwise { vec2(-point.y, -point.x) } else { point };
    let sample = |t: f32| {
        let a = 1.0 - t;
        let (p0, p1, p2, p3) = (vec2(10.0, 4.0), vec2(5.0, -1.0), vec2(0.0, -3.0), vec2(-3.0, -2.0));
        let point = p0 * a.powi(3) + p1 * (3.0 * a * a * t) + p2 * (3.0 * a * t * t) + p3 * t.powi(3);
        let tangent = ((p1 - p0) * (3.0 * a * a) + (p2 - p1) * (6.0 * a * t) + (p3 - p2) * (3.0 * t * t)).normalized();
        let side = vec2(-tangent.y, tangent.x) * 2.0;
        (transform(point + side), transform(point - side))
    };
    // Shared vertices keep the filled shaft continuous, without anti-aliased seams
    // between separately painted curve segments.
    let mut mesh = egui::Mesh::default();
    for step in 0..=12_u32 {
        let (left, right) = sample(step as f32 / 12.0);
        mesh.colored_vertex(left, ink);
        mesh.colored_vertex(right, ink);
        if step > 0 {
            let base = (step - 1) * 2;
            mesh.add_triangle(base, base + 2, base + 3);
            mesh.add_triangle(base, base + 3, base + 1);
        }
    }
    p.add(Shape::mesh(mesh));
    p.add(Shape::convex_polygon([vec2(-11.0, -2.0), vec2(-3.0, -8.0), vec2(-3.0, 4.0)].map(transform).to_vec(), ink, Stroke::NONE));
}

fn contains(points: [Pos2; 4], p: Pos2) -> bool {
    let mut sign = 0.0f32;
    for (a, b) in points.iter().zip(points.iter().cycle().skip(1)).take(4) {
        let a = *b - *a;
        let b = p - *b;
        let cross = a.x * b.y - a.y * b.x;
        if cross.abs() > 1e-4 {
            if sign != 0.0 && cross.signum() != sign {
                return false;
            }
            sign = cross.signum();
        }
    }
    sign != 0.0
}

/// Transform the existing font galley in the face plane. Keeping a TextShape lets egui normalize
/// font-atlas UVs after the rest of the frame has finished allocating glyphs.
#[allow(clippy::too_many_arguments)]
fn plane_text(p: &Painter, at: Pos2, label: &str, size: f32, right: Vec2, down: Vec2, width: f32, ink: Color32) {
    let mut galley = (*p.layout_no_wrap(label.to_owned(), FontId::proportional(size), ink)).clone();
    let center = galley.rect.center().to_vec2();
    let fit = (width / galley.size().x.max(1.0)).min(1.0);
    let mut bounds = egui::Rect::NOTHING;
    for placed in &mut galley.rows {
        let row = std::sync::Arc::make_mut(&mut placed.row);
        for v in &mut row.visuals.mesh.vertices {
            let q = (v.pos.to_vec2() + placed.pos.to_vec2() - center) * fit;
            v.pos = (right * q.x + down * q.y).to_pos2();
        }
        placed.pos = Pos2::ZERO;
        row.visuals.mesh_bounds = row.visuals.mesh.calc_bounds();
        bounds = bounds.union(row.visuals.mesh_bounds);
    }
    galley.mesh_bounds = bounds;
    galley.rect = bounds;
    p.add(Shape::Text(egui::epaint::TextShape::new(at, std::sync::Arc::new(galley), ink)));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> Layout {
        Layout::new(egui::Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 700.0)))
    }

    #[test]
    fn compass_and_cube_follow_twist_instead_of_screen_directions() {
        let camera = Camera::projection(Mat4::IDENTITY, std::f64::consts::FRAC_PI_2);
        assert!((camera.screen(X) - vec2(0.0, -1.0)).length() < 1e-6);
        assert!((camera.screen(Y) - vec2(-1.0, 0.0)).length() < 1e-6);
        let l = layout();
        let widget = Widget::new(camera, l);
        assert!((widget.ring[0] - (l.center + vec2(0.0, -l.ring))).length() < 1e-4);
        assert_eq!(widget.part_at(widget.ring[0]), Some(Part::Ring));
        assert_eq!(widget.part_at(l.center), Some(Part::Face));
    }

    #[test]
    fn front_projection_flattens_the_ring_and_keeps_its_hit_test_and_drag() {
        let camera = Camera::axes(X, Vec3::Z);
        let l = layout();
        let widget = Widget::new(camera, l);
        assert_eq!(camera.screen(Y), Vec2::ZERO);
        assert_eq!(camera.screen(Vec3::Z), vec2(0.0, -1.0));
        assert!(egui::Rect::from_points(&widget.ring).height() < 1e-4);
        let at = widget.ring_center + vec2(l.ring, 0.0);
        assert_eq!(widget.part_at(at), Some(Part::Ring));
        assert!(widget.ring_delta(at, at + vec2(10.0, 0.0)).is_finite());
        assert!(widget.ring_center.y > l.center.y + HALF);
        assert!(!widget.interaction_bounds().contains(l.center + vec2(-74.5, 52.1)), "empty projected margins must leave drawing picks usable");
        assert!(widget.ring.iter().all(|p| widget.interaction_bounds().contains(*p)));
        assert_eq!(FACES.iter().filter(|f| camera.depth(f.normal) > 1e-6).map(|f| f.label).collect::<Vec<_>>(), ["Front"]);
    }

    #[test]
    fn projected_font_geometry_rotates_with_the_top_face() {
        let ctx = egui::Context::default();
        let l = layout();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            Widget::new(Camera::projection(Mat4::IDENTITY, std::f64::consts::FRAC_PI_2), l).paint(ui.painter(), None, None);
        });
        let top = output
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                Shape::Text(t) if t.galley.text() == "TOP" => Some(t),
                _ => None,
            })
            .unwrap();
        assert!(top.galley.mesh_bounds.height() > top.galley.mesh_bounds.width());
        assert!(top.galley.mesh_bounds.is_finite());
        output.textures_delta.clear();
    }

    #[test]
    fn malformed_camera_basis_cannot_create_non_finite_navigation_geometry() {
        for camera in [Camera::axes(Vec3::ZERO, Vec3::Z), Camera::axes(Vec3::new(f64::NAN, 0.0, 0.0), Y), Camera::axes(Vec3::new(1e308, 0.0, 0.0), Y)]
        {
            let widget = Widget::new(camera, layout());
            assert!(widget.ring.iter().all(|p| p.is_finite()));
            assert_eq!(widget.part_at(widget.layout.center), Some(Part::Face));
        }
    }

    #[test]
    fn stored_tilted_ucs_drives_face_targets_without_using_the_drafting_fallback() {
        let u = cadcraft_engine::ucs::UcsFrame::new(Vec3::ZERO, Vec3::Z, Y).unwrap();
        let widget = Widget::in_ucs(Camera::projection(Mat4::IDENTITY, 0.0), layout(), u);
        let face = widget.face_at(widget.layout.center).unwrap();
        assert_eq!(face.label, "Right");
        assert!((face.normal - Vec3::Z).len() < 1e-8);
        assert!((face.right - Y).len() < 1e-8);
    }

    #[test]
    fn an_edge_on_compass_drag_uses_its_rolled_horizon() {
        let camera = Camera::axes(X, Vec3::Z).turned(std::f64::consts::FRAC_PI_2);
        let widget = Widget::new(camera, layout());
        let start = widget.ring[0];
        assert_eq!(widget.part_at(start), Some(Part::Ring));
        assert!(widget.ring_delta(start, start + vec2(0.0, -10.0)) > 0.1);
        assert!(widget.ring_delta(start, start + vec2(10.0, 0.0)).abs() < 1e-5);
        for degrees in (0..360).step_by(15) {
            let widget = Widget::new(Camera::axes(X, Vec3::Z).turned(f64::from(degrees).to_radians()), layout());
            for at in [widget.ring[0], widget.ring[SAMPLES / 2]] {
                assert!(
                    !matches!(widget.part_at(at), Some(Part::Clockwise | Part::Counterclockwise)),
                    "rotation buttons must not cover the compass endpoints"
                );
            }
        }
    }

    #[test]
    fn adjacent_controls_cover_four_perpendicular_ucs_faces_at_every_roll() {
        let ucs = cadcraft_engine::ucs::UcsFrame::new(Vec3::new(10.0, -4.0, 3.0), Vec3::Z, Y).unwrap();
        for face in &FACES {
            for degrees in [0.0_f64, 37.0, 45.0, 90.0, 180.0, 270.0] {
                let world = |v: Vec3| ucs.x * v.x + ucs.y * v.y + ucs.z() * v.z;
                let camera = Camera::axes(world(face.right), world(face.up)).turned(degrees.to_radians());
                let widget = Widget::in_ucs(camera, layout(), ucs);
                let normal = world(face.normal);
                let mut normals = Vec::new();
                for (part, rect) in widget.controls() {
                    assert_eq!(widget.part_at(rect.center()), Some(part));
                    assert!(widget.interaction_bounds().contains_rect(rect));
                    if let Some(target) = widget.adjacent_target(part) {
                        let screen = widget.camera.screen(target.normal).normalized();
                        assert!(
                            ((rect.center() - widget.layout.center).normalized() - screen).length() < 1e-5,
                            "arrows must follow the face's inclination"
                        );
                        assert!(
                            [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()]
                                .into_iter()
                                .all(|p| !contains(widget.polygon(face), p)),
                            "rolled faces must not overlap their controls"
                        );
                        assert!(target.normal.dot(normal).abs() < 1e-6);
                        assert!(target.normal.dot(target.right).abs() < 1e-6);
                        assert!((target.right.len() - 1.0).abs() < 1e-6);
                        assert!(!normals.contains(&target.normal));
                        normals.push(target.normal);
                    }
                }
                assert_eq!(normals.len(), 4);
            }
        }
        let widget = Widget::new(Camera::axes(X, (Y + Vec3::Z).normalized()), layout());
        assert!(widget.aligned_face().is_none());
        assert!(widget.adjacent_target(Part::Up).is_none());
        assert_ne!(widget.part_at(widget.layout.center + vec2(72.0, -43.0)), Some(Part::Clockwise));
    }

    #[test]
    fn compass_drag_uses_roll_at_both_poles_and_azimuth_elsewhere() {
        for (camera, sign) in [(Camera::axes(X, Y), 1.0), (Camera::axes(-X, Y), -1.0), (Camera::axes(X, (Y + Vec3::Z).normalized()), -1.0)] {
            let widget = Widget::new(camera.turned(0.4), layout());
            let from = widget.ring_center + widget.camera.screen(X) * widget.layout.ring;
            let to = widget.ring_center + widget.camera.screen(Vec3::new(0.3_f64.cos(), 0.3_f64.sin(), 0.0)) * widget.layout.ring;
            assert!((widget.ring_turn(from, to) - 0.3 * sign).abs() < 1e-6);
            assert!((widget.ring_turn(to, from) + 0.3 * sign).abs() < 1e-6);
        }
    }

    #[test]
    fn face_edge_and_corner_regions_cover_all_26_ucs_directions() {
        let ucs = cadcraft_engine::ucs::UcsFrame::new(Vec3::new(4.0, 9.0, 2.0), Vec3::Z, Y).unwrap();
        let world = |v: Vec3| ucs.x * v.x + ucs.y * v.y + ucs.z() * v.z;
        let mut targets = Vec::new();
        for face in &FACES {
            for angle in [0.0_f64, 37.0, 90.0] {
                let widget = Widget::in_ucs(Camera::axes(world(face.right), world(face.up)).turned(angle.to_radians()), layout(), ucs);
                for region in regions(face) {
                    let at = egui::Rect::from_points(&widget.region_polygon(face, &region)).center();
                    let target = widget.face_at(at).unwrap();
                    assert!((target.normal - world(region.direction.normalized())).len() < 1e-6);
                    assert!(target.normal.dot(target.right).abs() < 1e-6);
                    assert!((target.right.len() - 1.0).abs() < 1e-6);
                    assert_eq!(widget.part_at(at), Some(Part::Face));
                    if !targets.iter().any(|(direction, _): &(Vec3, &str)| (*direction - target.normal).len() < 1e-6) {
                        targets.push((target.normal, target.label));
                    }
                }
            }
        }
        assert_eq!(targets.len(), 26);
        assert_eq!(targets.iter().filter(|(_, label)| *label == "Edge").count(), 12);
        assert_eq!(targets.iter().filter(|(_, label)| *label == "Corner").count(), 8);
    }

    #[test]
    fn adjacent_arrow_sequence_keeps_side_faces_upright_and_pole_heading() {
        let top = Widget::new(Camera::axes(X, Y), layout());
        let right = top.adjacent_target(Part::Right).unwrap();
        assert_eq!(right.normal, X);
        assert!((right.normal.cross(right.right) - Vec3::Z).len() < 1e-6);
        let back = Widget::new(Camera::axes(-X, Vec3::Z), layout());
        let top = back.adjacent_target(Part::Up).unwrap();
        assert_eq!(top.normal, Vec3::Z);
        assert!((top.right + X).len() < 1e-6);
        assert!((top.normal.cross(top.right) + Y).len() < 1e-6);
    }
}
