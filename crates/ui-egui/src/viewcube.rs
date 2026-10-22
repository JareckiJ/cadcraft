//! The navigation overlays on the drawing area: the ViewCube (the current view's face, Home,
//! compass ring and the WCS menu) and the UCS icon.
//!
//! Every overlay registers a widget that senses clicks *and* drags, so egui hands the pointer to
//! the overlay instead of the canvas underneath (a click on the ViewCube must never also pick or
//! start a selection window in the drawing). What the overlays do goes through commands and the
//! session: `vpoint`/`dview` (a click on a face), [`cadcraft_engine::Session::drag_view`] (dragging the
//! face), `zoom.extents` (Home), [`cadcraft_engine::Session::rotate_view`] (dragging the compass
//! ring), `ui.toggle.viewcube`, `ui.toggle.ucsicon`.
//!
//! The coordinate-system menu retains World and the drawing's named UCSs.

use egui::{Color32, CursorIcon, Pos2, Rect, Sense, Stroke, pos2, vec2};
use serde_json::json;

use crate::CadApp;
use crate::theme::Tokens;

mod projected;
use projected::{Camera, Target, Widget};

/// Where the ViewCube's parts are on the canvas.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub center: Pos2,
    /// Radius of the compass ring's centre line.
    pub ring: f32,
    pub face: Rect,
    pub home: Rect,
    /// The "WCS" coordinate-system menu under the cube.
    pub pill: Rect,
    /// Everything that belongs to the cube (the area kept away from the canvas).
    pub bounds: Rect,
}

impl Layout {
    pub fn new(canvas: Rect) -> Self {
        let center = pos2(canvas.right() - 92.0, canvas.top() + 82.0);
        let ring = 58.0;
        Layout {
            center,
            ring,
            face: Rect::from_center_size(center, vec2(44.0, 44.0)),
            home: Rect::from_center_size(center + vec2(-ring + 2.0, -ring + 2.0), vec2(20.0, 20.0)),
            pill: Rect::from_center_size(center + vec2(0.0, ring + 26.0), vec2(84.0, 16.0)),
            bounds: Rect::from_center_size(center, vec2(2.0 * (ring + 22.0), 2.0 * (ring + 22.0))),
        }
    }

    /// The part of the cube at `p` (not the pill, which is its own widget).
    pub fn part_at(&self, p: Pos2) -> Option<Part> {
        if self.home.contains(p) {
            return Some(Part::Home);
        }
        if self.face.contains(p) {
            return Some(Part::Face);
        }
        let d = p.distance(self.center);
        ((self.ring - 10.0..=self.ring + 10.0).contains(&d)).then_some(Part::Ring)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Face,
    Home,
    Ring,
    Up,
    Down,
    Left,
    Right,
    Clockwise,
    Counterclockwise,
}

impl Part {
    fn tip(self) -> &'static str {
        match self {
            Part::Face => crate::tl!("Click a face, edge or corner to align the view; drag to orbit"),
            Part::Home => crate::tl!("Home: zoom to the drawing extents"),
            Part::Ring => crate::tl!("Compass: drag to turn a 3D view about the Z axis"),
            Part::Up | Part::Down | Part::Left | Part::Right => crate::tl!("View the adjacent face"),
            Part::Clockwise => crate::tl!("Rotate view 90° clockwise"),
            Part::Counterclockwise => crate::tl!("Rotate view 90° counterclockwise"),
        }
    }
}

/// Paint the projected cube, compass, UCS pill and Home button, and handle the pointer.
pub fn show(app: &mut CadApp, ui: &mut egui::Ui, l: &Layout) {
    let widget = Widget::in_ucs(Camera::of(app), *l, app.session.ucs());
    let resp = ui.interact(widget.interaction_bounds(), ui.id().with("viewcube"), Sense::click_and_drag());
    let part = resp.hover_pos().and_then(|p| widget.part_at(p));
    let p = ui.painter_at(l.bounds.union(l.pill).union(widget.interaction_bounds()));
    widget.paint(&p, part, resp.hover_pos());
    if resp.hovered() || resp.is_pointer_button_down_on() {
        paint_home(&p, l.home, part == Some(Part::Home));
        widget.paint_controls(&p, part);
    }

    let resp = match part {
        Some(part) => resp.on_hover_cursor(CursorIcon::PointingHand).on_hover_text(part.tip()),
        None => resp,
    };
    let part_id = ui.id().with(("viewcube_pressed_part", app.session.state().map_or(0, |s| s.uid)));
    let press = ui.input(|i| i.pointer.press_origin()).or(resp.interact_pointer_pos());
    let pick = press.and_then(|p| widget.part_at(p));
    let picked_face = press.and_then(|p| widget.face_at(p));
    let face_id = part_id.with("face");
    if resp.is_pointer_button_down_on() && ui.ctx().data(|d| d.get_temp::<Option<Part>>(part_id)).is_none() {
        ui.ctx().data_mut(|d| {
            d.insert_temp(part_id, pick);
            if let Some(face) = picked_face {
                d.insert_temp(face_id, face);
            }
        });
    }
    let pressed_on = ui.ctx().data(|d| d.get_temp::<Option<Part>>(part_id)).unwrap_or(pick);
    if resp.clicked() {
        match pressed_on {
            Some(Part::Face) => {
                if let Some(face) = ui.ctx().data(|d| d.get_temp::<Target>(face_id)).or(picked_face)
                    && let Err(error) = orient_face(app, face)
                {
                    app.session.echo(error.to_string());
                }
            }
            Some(Part::Home) => {
                let _ = app.run("zoom.extents", json!({}));
            }
            Some(Part::Clockwise | Part::Counterclockwise) => {
                let angle = if pressed_on == Some(Part::Clockwise) { -std::f64::consts::FRAC_PI_2 } else { std::f64::consts::FRAC_PI_2 };
                if let Err(error) = app.session.roll_view(angle) {
                    app.session.echo(error.to_string());
                }
            }
            Some(part @ (Part::Up | Part::Down | Part::Left | Part::Right)) => {
                if let Some(face) = widget.adjacent_target(part)
                    && let Err(error) = orient_face(app, face)
                {
                    app.session.echo(error);
                }
            }
            Some(Part::Ring) | None => {}
        }
    }
    if resp.dragged() && pressed_on == Some(Part::Face) {
        let d = resp.drag_delta();
        let result = app.session.drag_view(f64::from(d.x), f64::from(d.y));
        if let Err(error) = result {
            app.session.echo(error.to_string());
        }
    }
    // Dragging the ring turns the view with the pointer, about the cube's centre.
    if resp.dragged()
        && pressed_on == Some(Part::Ring)
        && let Some(p1) = resp.interact_pointer_pos()
    {
        let p0 = if resp.drag_started() { press.unwrap_or(p1 - resp.drag_delta()) } else { p1 - resp.drag_delta() };
        let delta = widget.ring_turn(p0, p1);
        let _ = app.session.rotate_view(delta);
    }
    if !resp.is_pointer_button_down_on() {
        ui.ctx().data_mut(|d| {
            d.remove_temp::<Option<Part>>(part_id);
            d.remove::<Target>(face_id);
        });
    }
    let mut action = None;
    resp.context_menu(|ui| {
        if ui.button(crate::tl!("Home")).clicked() {
            action = Some(("zoom.extents", json!({})));
            ui.close();
        }
        if ui.button(crate::tl!("Hide ViewCube")).clicked() {
            action = Some(("ui.toggle.viewcube", json!({"on": false})));
            ui.close();
        }
    });
    if let Some((id, params)) = action {
        let _ = app.run(id, params);
    }
    // The UCS pill (painted by the canvas, wide enough for a cut-short UCS name) opens the
    // coordinate-system menu.
    let label = crate::canvas::ucs_pill_text(app);
    p.rect_filled(l.pill, 8.0, Color32::from_rgb(0x48, 0x50, 0x5c));
    let ink = Tokens::DARK.canvas_ink;
    let label = p.layout_no_wrap(label, crate::theme::small(), ink);
    let suffix = 10.0;
    let left = l.pill.center().x - (label.size().x + suffix) / 2.0;
    p.galley(pos2(left, l.pill.center().y - label.size().y / 2.0), label.clone(), ink);
    {
        let x = left + label.size().x + 4.0;
        let y = l.pill.center().y;
        p.line(vec![pos2(x, y - 1.5), pos2(x + 3.0, y + 1.5), pos2(x + 6.0, y - 1.5)], Stroke::new(1.2, ink));
    }
    let pill = ui.interact(l.pill, ui.id().with("viewcube_wcs"), Sense::click_and_drag()).on_hover_cursor(CursorIcon::PointingHand);
    let pill = pill.on_hover_text(crate::tl!("Coordinate system"));
    ucs_menu(app, egui::Popup::menu(&pill));
}

/// Align a face through the existing camera commands, including the pole's right direction.
fn orient_face(app: &mut CadApp, face: Target) -> Result<(), String> {
    let up = face.normal.cross(face.right);
    app.run("vpoint", json!({"direction":[face.normal.x,face.normal.y,face.normal.z],"up":[up.x,up.y,up.z]})).map(|_| ())
}

/// A small house, drawn in code.
fn paint_home(p: &egui::Painter, r: Rect, hot: bool) {
    let t = Tokens::get();
    if hot {
        p.rect_filled(r, 3.0, t.control_hover);
    }
    let st = Stroke::new(1.3, if hot { t.text } else { t.canvas_ink.gamma_multiply(0.75) });
    let c = r.center();
    let roof = [c + vec2(-6.5, 0.5), c + vec2(0.0, -6.0), c + vec2(6.5, 0.5)];
    p.line(roof.to_vec(), st);
    p.line(vec![c + vec2(-4.5, -1.0), c + vec2(-4.5, 6.0), c + vec2(4.5, 6.0), c + vec2(4.5, -1.0)], st);
    p.line_segment([c + vec2(-1.2, 6.0), c + vec2(-1.2, 2.5)], st);
    p.line_segment([c + vec2(1.2, 6.0), c + vec2(1.2, 2.5)], st);
    p.line_segment([c + vec2(-1.2, 2.5), c + vec2(1.2, 2.5)], st);
}

/// How long the UCS icon's arms are, in points.
pub const ARM: f32 = 60.0;

/// Where the UCS icon is drawn: the point its arms start from, the screen directions of its X
/// and Y arms (unit vectors: the current UCS's axes seen in the plan view) and whether the UCS is
/// the World UCS (the icon then has a square at its corner).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IconPlace {
    pub origin: Pos2,
    pub x: egui::Vec2,
    pub y: egui::Vec2,
    pub world: bool,
    /// Projected Z in a 3D view; an edge-on axis may have zero screen length.
    pub z: Option<egui::Vec2>,
}

impl IconPlace {
    /// Origin placement must not cover selectable drawing geometry, and its selectable geometry.
    /// Keep both drawing input and the icon menu usable by falling back to the corner.
    pub(crate) fn in_drawing(app: &CadApp, canvas: Rect, transform: &crate::canvas::Xf) -> IconPlace {
        let camera = Camera::projection(transform.projection.unwrap_or(cadcraft_geom::Mat4::IDENTITY), transform.twist);
        let mut place = Self::projected(app, canvas, camera, transform.projection.is_some(), |point| {
            let view = transform.projection.map_or(point, |m| m.apply(point));
            transform.view_to_screen(view.xy())
        });
        let corner = Self::corner(canvas).origin;
        if place.origin != corner
            && let Ok(drawing) = app.session.doc()
        {
            let space = app.session.space();
            let aperture = app.session.settings.pickbox.max(1.0) * 1.5 / transform.scale;
            let covered = !aperture.is_finite()
                || aperture <= 0.0
                || cadcraft_engine::select::pick(drawing, &space, transform.to_world(place.origin), aperture).is_some()
                || place.arm_rects().any(|arm| {
                    let points = [arm.left_top(), arm.right_top(), arm.right_bottom(), arm.left_bottom()].map(|point| transform.to_world(point));
                    if points.iter().any(|point| !point.is_finite()) {
                        return true;
                    }
                    let bounds = cadcraft_geom::Bounds2::from_points(points);
                    !cadcraft_engine::select::select_window(drawing, &space, bounds, true).is_empty()
                });
            if covered {
                place.origin = place.at_corner(canvas);
            }
        }
        place
    }

    /// The World UCS's icon at the lower-left corner of the drawing area.
    pub fn corner(canvas: Rect) -> IconPlace {
        IconPlace { origin: pos2(canvas.left() + 36.0, canvas.bottom() - 34.0), x: vec2(1.0, 0.0), y: vec2(0.0, -1.0), world: true, z: None }
    }

    /// The icon for the current UCS: at its origin when UCSICON asks for that (ORigin) and the
    /// whole icon fits there, else at the corner. `to_screen` maps a point of the space shown.
    pub fn of(app: &CadApp, canvas: Rect, view_twist: f64, to_screen: impl Fn(cadcraft_geom::Vec2) -> Pos2) -> IconPlace {
        Self::projected(app, canvas, Camera::projection(cadcraft_geom::Mat4::IDENTITY, view_twist), false, |point| to_screen(point.xy()))
    }

    fn projected(app: &CadApp, canvas: Rect, camera: Camera, three_d: bool, to_screen: impl Fn(cadcraft_geom::Vec3) -> Pos2) -> Self {
        let u = app.session.ucs();
        let mut place = Self {
            x: camera.screen(u.x),
            y: camera.screen(u.y),
            z: three_d.then(|| camera.screen(u.x.cross(u.y))),
            world: u.is_world(),
            ..Self::corner(canvas)
        };
        place.origin = place.at_corner(canvas);
        let same_space = app.session.space() == app.session.layout_space();
        if app.session.ucsicon_bits() & 2 != 0 && same_space {
            let at = to_screen(u.origin);
            let inner = canvas.shrink(12.0);
            if at.is_finite() && place.offsets().all(|offset| inner.contains(at + offset)) {
                place.origin = at;
            }
        }
        place
    }

    fn directions(&self) -> impl Iterator<Item = (&'static str, egui::Vec2)> {
        [("X", Some(self.x)), ("Y", Some(self.y)), ("Z", self.z)]
            .into_iter()
            .filter_map(|(label, d)| d.map(|d| (label, d)))
            .filter(|(_, d)| d.length_sq() > 1e-4)
    }

    fn offsets(&self) -> impl Iterator<Item = egui::Vec2> {
        std::iter::once(egui::Vec2::ZERO).chain(self.directions().map(|(_, d)| d * ARM + d.normalized() * 20.0))
    }

    fn at_corner(&self, canvas: Rect) -> Pos2 {
        let bounds = Rect::from_points(&self.offsets().map(|v| v.to_pos2()).collect::<Vec<_>>());
        let base = Self::corner(canvas).origin;
        let low = canvas.min + vec2(12.0, 12.0) - bounds.min.to_vec2();
        let high = canvas.max - vec2(12.0, 12.0) - bounds.max.to_vec2();
        pos2(base.x.clamp(low.x, high.x.max(low.x)), base.y.clamp(low.y, high.y.max(low.y)))
    }

    fn arm_rects(&self) -> impl Iterator<Item = Rect> {
        self.directions().map(|(_, d)| Rect::from_two_pos(self.origin, self.origin + d * ARM + d.normalized() * 20.0).expand(8.0))
    }

    /// The icon's arms with room around them: the X arm and the Y arm.
    pub fn arms(&self) -> [Rect; 2] {
        let o = self.origin;
        [Rect::from_two_pos(o, o + self.x * (ARM + 20.0)).expand(8.0), Rect::from_two_pos(o, o + self.y * (ARM + 22.0)).expand(8.0)]
    }
}

/// How the current UCS is named on screen.
fn ucs_label(app: &CadApp) -> String {
    let (u, name) = app.session.ucs_named();
    if u.is_world() && name.is_empty() {
        crate::tl!("World Coordinate System").into()
    } else if name.is_empty() {
        crate::tl!("Unnamed UCS").into()
    } else {
        name
    }
}

/// Paint the projected axes without registering an input target.
pub(crate) fn paint_ucs_icon(p: &egui::Painter, place: &IconPlace) {
    let ink = Tokens::get().canvas_ink;
    let stroke = Stroke::new(1.0, ink);
    for (label, d) in place.directions() {
        p.line_segment([place.origin, place.origin + d * ARM], stroke);
        let end = place.origin + d * ARM + d.normalized() * 10.0;
        p.text(end, egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(13.0), ink);
    }
    if place.world {
        let (x, y) = (place.x, place.y);
        let o = place.origin;
        let h = 4.5;
        p.add(egui::Shape::closed_line(vec![o + (x + y) * h, o + (x - y) * h, o - (x + y) * h, o - (x - y) * h], stroke));
        if place.z.is_some() {
            p.circle_stroke(o, 2.0, stroke);
        }
    }
}

/// Make the UCS icon clickable: hovering highlights it, a click (either button) opens the
/// coordinate-system menu.
pub fn ucs_icon(app: &mut CadApp, ui: &mut egui::Ui, canvas: Rect, place: &IconPlace) {
    let t = Tokens::get();
    let mut areas = place.arm_rects().enumerate().map(|(i, r)| ui.interact(r, ui.id().with(("ucs_axis", i)), Sense::click_and_drag()));
    let Some(mut resp) = areas.next() else { return };
    for area in areas {
        resp |= area;
    }
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand).on_hover_text(crate::tf!("UCS icon: {name}", name = ucs_label(app)));
    if resp.hovered() {
        let p = ui.painter_at(canvas);
        for (_, d) in place.directions() {
            p.line_segment([place.origin, place.origin + d * ARM], Stroke::new(2.0, t.hover));
        }
    }
    // Opened with `Bool(true)`: a pointer-anchored popup records its position only then.
    let open = (resp.clicked() || resp.secondary_clicked()).then_some(egui::SetOpenCommand::Bool(true));
    ucs_menu(app, egui::Popup::menu(&resp).open_memory(open).at_pointer_fixed());
}

/// The coordinate-system menu: World, the drawing's named UCSs (picking one makes it current,
/// through the UCS command) and the icon switch.
fn ucs_menu(app: &mut CadApp, popup: egui::Popup<'_>) {
    let (u, current) = app.session.ucs_named();
    let names: Vec<String> = app.session.doc().map(|d| d.ucss.iter().map(|u| u.name.clone()).collect()).unwrap_or_default();
    let mut action: Option<(&str, serde_json::Value)> = None;
    popup.show(|ui| {
        if ui.radio(u.is_world() && current.is_empty(), crate::tl!("World (WCS)")).clicked() {
            action = Some(("ucs.world", json!({})));
            ui.close();
        }
        for n in &names {
            if ui.radio(current.eq_ignore_ascii_case(n), n.as_str()).clicked() {
                action = Some(("ucs", json!({"option": "named", "action": "restore", "name": n})));
                ui.close();
            }
        }
        if !u.is_world() && current.is_empty() {
            let _ = ui.radio(true, crate::tl!("Unnamed"));
        }
        ui.separator();
        if ui.button(crate::tl!("Hide UCS Icon")).clicked() {
            action = Some(("ui.toggle.ucsicon", json!({"on": false})));
            ui.close();
        }
    });
    if let Some((id, params)) = action
        && let Err(e) = app.run(id, params)
    {
        app.session.echo(e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_engine::Session;

    /// The app with the sample drawing, laid out once in a 1400×900 window.
    fn app_and_ctx() -> (CadApp, egui::Context) {
        let mut app = CadApp::new(Session::empty(), crate::Services::default());
        app.ui.in_window_menu = true;
        app.start("ui.sample");
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, Vec::new());
        frame(&mut app, &ctx, Vec::new());
        (app, ctx)
    }

    fn frame(app: &mut CadApp, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
        let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))), events, ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        out.textures_delta.clear();
        out
    }

    /// A real pointer click (move, press, release in separate frames, like the control channel).
    pub(crate) fn click(app: &mut CadApp, ctx: &egui::Context, at: Pos2, button: egui::PointerButton) {
        let m = egui::Modifiers::default();
        frame(app, ctx, vec![egui::Event::PointerMoved(at)]);
        frame(app, ctx, vec![egui::Event::PointerButton { pos: at, button, pressed: true, modifiers: m }]);
        frame(app, ctx, vec![egui::Event::PointerButton { pos: at, button, pressed: false, modifiers: m }]);
        frame(app, ctx, Vec::new());
    }

    /// A menu popup was shown in the last frame.
    fn menu_shown(ctx: &egui::Context) -> bool {
        ctx.memory(|m| m.areas().visible_layer_ids().iter().any(|l| l.order == egui::Order::Foreground))
    }

    fn canvas(app: &CadApp) -> Rect {
        app.canvas.rect.unwrap_or(Rect::NOTHING)
    }

    fn view(app: &CadApp) -> (f64, f64, f64) {
        let v = app.session.state().map(|s| s.view()).unwrap_or_default();
        (v.center.x, v.center.y, v.height)
    }

    #[test]
    fn parts_are_found_where_they_are_drawn() {
        let l = Layout::new(Rect::from_min_size(pos2(200.0, 80.0), vec2(1000.0, 800.0)));
        assert_eq!(l.part_at(l.center), Some(Part::Face));
        assert_eq!(l.part_at(l.home.center()), Some(Part::Home));
        assert_eq!(l.part_at(l.center + vec2(0.0, -l.ring)), Some(Part::Ring), "N");
        assert_eq!(l.part_at(l.center + vec2(l.ring + 3.0, 0.0)), Some(Part::Ring), "E");
        assert_eq!(l.part_at(l.center + vec2(30.0, 30.0)), None, "between the face and the ring");
        assert!(l.bounds.contains_rect(l.home) && l.bounds.contains_rect(l.face));
    }

    #[test]
    fn viewcube_clicks_reach_the_cube_not_the_drawing() {
        let (mut app, ctx) = app_and_ctx();
        let l = Layout::new(canvas(&app));
        let fitted = view(&app);
        // Zoomed in, the face (PLAN) fits the drawing again (zoom extents) and nothing is picked.
        let _ = app.run("zoom.in", json!({}));
        assert_ne!(view(&app), fitted);
        click(&mut app, &ctx, l.center, egui::PointerButton::Primary);
        assert_eq!(view(&app), fitted, "Top face zooms to extents");
        // From a 3D view the face goes back to the plan view.
        let _ = app.run("vpoint", json!({"preset": "seiso"}));
        assert_eq!(app.session.view_name(), "SE Isometric");
        let output = frame(&mut app, &ctx, Vec::new());
        let top = output
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text() == "TOP" => Some(t.pos),
                _ => None,
            })
            .unwrap();
        click(&mut app, &ctx, top, egui::PointerButton::Primary);
        assert_eq!(app.session.view_name(), "Top");
        assert_eq!(view(&app), fitted);
        assert!(app.session.pending_window.is_none(), "the click didn't start a selection window");
        assert!(app.session.selection().is_empty());
        // Home does the same.
        let _ = app.run("zoom.in", json!({}));
        click(&mut app, &ctx, l.home.center(), egui::PointerButton::Primary);
        assert_eq!(view(&app), fitted, "Home zooms to extents");
        // A click on the ring (it turns the view when dragged) doesn't touch the drawing.
        click(&mut app, &ctx, l.center + vec2(0.0, -l.ring), egui::PointerButton::Primary);
        assert!(app.session.pending_window.is_none());
        assert_eq!(view(&app), fitted);
        // A click in the drawing still reaches the drawing.
        let empty = canvas(&app).center() + vec2(0.0, -300.0);
        click(&mut app, &ctx, empty, egui::PointerButton::Primary);
        assert!(app.session.pending_window.is_some(), "an empty-space click starts a selection window");
    }

    #[test]
    fn the_ucs_icon_follows_the_ucs() {
        let (mut app, _ctx) = app_and_ctx();
        let c = canvas(&app);
        // A plain mapping: the canvas centre is world (0, 0), one point per unit.
        let to_screen = |p: cadcraft_geom::Vec2| pos2(c.center().x + p.x as f32, c.center().y - p.y as f32);
        let world = IconPlace::of(&app, c, 0.0, to_screen);
        assert!(world.world && (world.x - vec2(1.0, 0.0)).length() < 1e-6 && (world.y - vec2(0.0, -1.0)).length() < 1e-6);
        // At the World origin (on screen, with ORigin on by default).
        assert!((world.origin - c.center()).length() < 1e-3, "{:?} vs {:?}", world.origin, c.center());
        // A UCS turned 90° at (20, 10): the X arm points up the screen, the Y arm left.
        app.run("ucs", json!({"option": "origin", "origin": [20, 10]})).unwrap();
        app.run("ucs", json!({"option": "z", "angle": 90})).unwrap();
        let p = IconPlace::of(&app, c, 0.0, to_screen);
        assert!(!p.world);
        assert!((p.x - vec2(0.0, -1.0)).length() < 1e-6 && (p.y - vec2(-1.0, 0.0)).length() < 1e-6, "{p:?}");
        assert!((p.origin - pos2(c.center().x + 20.0, c.center().y - 10.0)).length() < 1e-3);
        // Its click areas lie along the turned arms.
        let [ax, ay] = p.arms();
        assert!(ax.contains(p.origin + p.x * 40.0) && ay.contains(p.origin + p.y * 40.0));
        // Noorigin, or an origin off screen: the corner.
        app.run("ucsicon", json!({"origin": false})).unwrap();
        let corner = IconPlace::of(&app, c, 0.0, to_screen);
        assert!(corner.arm_rects().all(|r| c.contains_rect(r)), "the rotated arms and labels fit in the corner");
        app.run("ucsicon", json!({"origin": true})).unwrap();
        app.run("ucs", json!({"option": "origin", "origin": [1e6, 0]})).unwrap();
        assert_eq!(IconPlace::of(&app, c, 0.0, to_screen).origin, corner.origin);
        // The menu switch turns UCSICON off through its command.
        app.run("ui.toggle.ucsicon", json!({"on": false})).unwrap();
        assert_eq!(app.session.ucsicon_bits() & 1, 0);
        app.run("ui.toggle.ucsicon", json!({})).unwrap();
        assert_eq!(app.session.ucsicon_bits() & 1, 1, "a toggle without `on` flips it");
        app.run("ui.toggle.ucsicon.origin", json!({"on": false})).unwrap();
        assert_eq!(app.session.ucsicon_bits(), 1);
    }

    #[test]
    fn dragging_the_face_orbits_the_view() {
        let (mut app, ctx) = app_and_ctx();
        let l = Layout::new(canvas(&app));
        let m = egui::Modifiers::default();
        let end = l.center + vec2(60.0, 40.0);
        frame(&mut app, &ctx, vec![egui::Event::PointerMoved(l.center)]);
        frame(&mut app, &ctx, vec![egui::Event::PointerButton { pos: l.center, button: egui::PointerButton::Primary, pressed: true, modifiers: m }]);
        for k in 1..=4 {
            frame(&mut app, &ctx, vec![egui::Event::PointerMoved(l.center + (end - l.center) * (k as f32 / 4.0))]);
        }
        frame(&mut app, &ctx, vec![egui::Event::PointerButton { pos: end, button: egui::PointerButton::Primary, pressed: false, modifiers: m }]);
        let d = app.session.state().unwrap().view_dir;
        assert!(d.z < 1.0 && d.z > 0.0, "looking down at the model from the side: {d:?}");
        assert!(app.session.pending_window.is_none(), "the drag didn't start a selection window");
    }

    #[test]
    fn ucs_icon_and_wcs_pill_open_the_coordinate_system_menu() {
        let (mut app, ctx) = app_and_ctx();
        // Where the canvas drew it: at the World origin when that is on screen (UCSICON 3).
        let xf = app.canvas.xf.expect("laid out");
        let place = IconPlace::in_drawing(&app, canvas(&app), &xf);
        click(&mut app, &ctx, place.origin + place.x * 40.0, egui::PointerButton::Primary);
        assert!(menu_shown(&ctx), "UCS icon menu");
        assert!(app.session.pending_window.is_none(), "the click stayed on the icon");
        let l = Layout::new(canvas(&app));
        click(&mut app, &ctx, l.pill.center(), egui::PointerButton::Primary);
        assert!(menu_shown(&ctx), "WCS menu");
        assert!(app.session.pending_window.is_none());
    }

    #[test]
    fn ucs_origin_icon_leaves_drawing_geometry_selectable() {
        let (mut app, ctx) = app_and_ctx();
        let result = app.run("line", json!({"points":[[-10,0],[10,0]]})).unwrap();
        let handle = cadcraft_doc::Handle::parse_hex(result["handles"][0].as_str().unwrap()).unwrap();
        app.run("view.set", json!({"center":[0,0],"height":100})).unwrap();
        app.canvas.zoom_pending = false;
        frame(&mut app, &ctx, Vec::new());
        frame(&mut app, &ctx, Vec::new());
        let transform = app.canvas.xf.unwrap();
        let rect = canvas(&app);
        let place = IconPlace::in_drawing(&app, rect, &transform);
        assert_eq!(place.origin, IconPlace::corner(rect).origin);
        click(&mut app, &ctx, transform.to_screen(cadcraft_geom::Vec2::ZERO), egui::PointerButton::Primary);
        assert!(app.session.selection().contains(&handle));
        assert!(!menu_shown(&ctx));
    }

    #[test]
    fn ucs_icon_axes_include_the_view_twist() {
        let mut app = CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
        app.run("ucs", json!({"option":"z","angle":30})).unwrap();
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 700.0));
        let transform =
            crate::canvas::Xf { rect, center: cadcraft_geom::Vec2::ZERO, scale: 10.0, twist: std::f64::consts::FRAC_PI_3, projection: None };
        let place = IconPlace::in_drawing(&app, rect, &transform);
        assert!((place.x - vec2(0.0, -1.0)).length() < 1e-6);
        assert!((place.y - vec2(-1.0, 0.0)).length() < 1e-6);
    }
}
