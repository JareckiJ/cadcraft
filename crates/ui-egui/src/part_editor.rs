//! Integrated part workspace: shared dock panels, main canvas and transactional previews.
use crate::{CadApp, theme::Tokens};
use cadcraft_doc::{Drawing, FaceReference, SolidOperation};
use cadcraft_engine::cmd::modeling::{FaceFrame, bodies, face_reference, parameters, plane_frame, sketch_frame, support_frame};
use cadcraft_geom::{Vec2, Vec3};
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, vec2};
use serde_json::json;
type BodyList = std::sync::Arc<Vec<cadcraft_engine::cmd::modeling::Body>>;
type ModelKey = (u64, u64, usize);
type PreviewResult = Result<(BodyList, Option<SolidOperation>), String>;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Mode {
    #[default]
    Idle,
    Plane,
    Placement(bool),
    Sketch(bool),
    Depth,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Selected {
    #[default]
    Origin,
    Plane(u32),
    Sketch(u32),
    Extrude(u32),
    Body(u32),
    Face(u32, usize),
}

pub struct State {
    model_cache: Option<(ModelKey, BodyList)>,
    context_cache: Option<((ModelKey, Option<u32>), BodyList)>,
    preview_cache: Option<(ModelKey, serde_json::Value, PreviewResult)>,
    support_edges: Vec<[Vec2; 2]>,
    uid: u64,
    sketch: Option<u32>,
    feature: Option<u32>,
    operation: SolidOperation,
    target: Option<u32>,
    mode: Mode,
    selected: Selected,
    plane: u32,
    edit_plane: Option<u32>,
    offset: String,
    depth: String,
    face: Option<FaceReference>,
    reverse: bool,
    auto_operation: bool,
    guide: Option<(Vec3, Vec3)>,
    sketch_edit_id: u64,
    sketch_yaw: f64,
    sketch_tilt: f64,
    sketch_roll: f64,
    origin: Vec2,
    size: Vec2,
    first: Option<Vec2>,
    drawn: bool,
    layers: bool,
    yaw: f64,
    pitch: f64,
    zoom: f64,
    pan: egui::Vec2,
    error: Option<String>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            model_cache: None,
            context_cache: None,
            preview_cache: None,
            support_edges: Vec::new(),
            uid: 0,
            sketch: None,
            feature: None,
            operation: SolidOperation::NewBody,
            target: None,
            mode: Mode::Idle,
            selected: Selected::Origin,
            plane: 1,
            edit_plane: None,
            offset: "10".into(),
            depth: "10".into(),
            face: None,
            reverse: false,
            auto_operation: false,
            guide: None,
            sketch_edit_id: 0,
            sketch_yaw: 0.0,
            sketch_tilt: 0.0,
            sketch_roll: 0.0,
            origin: Vec2::ZERO,
            size: Vec2::new(40.0, 20.0),
            first: None,
            drawn: false,
            layers: false,
            yaw: 3.0 * std::f64::consts::FRAC_PI_4,
            pitch: (1.0_f64 / 3.0_f64.sqrt()).asin(),
            zoom: 1.0,
            pan: egui::Vec2::ZERO,
            error: None,
        }
    }
}

pub fn document(app: &CadApp) -> Option<&Drawing> {
    let st = app.session.state().ok()?;
    Some(st.part_edit.as_ref().map_or(st.doc.as_ref(), |edit| edit.host.doc.as_ref()))
}
pub fn is_part(app: &CadApp) -> bool {
    document(app).is_some_and(|d| d.part.is_some())
}
fn model_key(app: &CadApp, host: bool) -> ModelKey {
    let Ok(st) = app.session.state() else { return (0, 0, 0) };
    let st = if host { st.part_edit.as_ref().map_or(st, |e| e.host.as_ref()) } else { st };
    (st.uid, st.revision, std::sync::Arc::as_ptr(&st.doc) as usize)
}
fn cached_model(app: &mut CadApp) -> BodyList {
    let key = model_key(app, true);
    if let Some((old, bodies)) = &app.part_editor.model_cache
        && *old == key
    {
        return bodies.clone();
    }
    let bodies = std::sync::Arc::new(document(app).and_then(|d| bodies(d).ok()).unwrap_or_default());
    app.part_editor.model_cache = Some((key, bodies.clone()));
    bodies
}
fn cached_context(app: &mut CadApp) -> BodyList {
    let key = (model_key(app, true), app.part_editor.sketch);
    if let Some((old, bodies)) = &app.part_editor.context_cache
        && *old == key
    {
        return bodies.clone();
    }
    let bodies = std::sync::Arc::new(
        document(app).and_then(|d| cadcraft_engine::cmd::modeling::bodies_before_sketch(d, app.part_editor.sketch).ok()).unwrap_or_default(),
    );
    app.part_editor.context_cache = Some((key, bodies.clone()));
    bodies
}
fn preview_parts(app: &mut CadApp, d: &Drawing) -> PreviewResult {
    let key = model_key(app, false);
    let args = args(app);
    if let Some((old, params, result)) = &app.part_editor.preview_cache
        && *old == key
        && *params == args
    {
        return result.clone();
    }
    let result = (|| {
        let mut trial = cadcraft_engine::Session::new();
        let result = if app.session.state().is_ok_and(|st| st.part_edit.is_some()) {
            let (drawing, result) = cadcraft_engine::cmd::modeling::sketch_edit::candidate(&app.session, &args).map_err(|e| e.to_string())?;
            *trial.doc_mut().map_err(|e| e.to_string())? = drawing;
            result
        } else {
            *trial.doc_mut().map_err(|e| e.to_string())? = d.clone();
            trial.execute("extrude", &args).map_err(|e| e.to_string())?
        };
        let bodies = bodies(trial.doc().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let operation = result.get("operation").and_then(|v| serde_json::from_value(v.clone()).ok());
        Ok((std::sync::Arc::new(bodies), operation))
    })();
    app.part_editor.preview_cache = Some((key, args, result.clone()));
    result
}
pub fn drafting(app: &CadApp) -> bool {
    is_sketch(app) && app.session.state().is_ok_and(|st| st.part_edit.is_some())
}
fn profile(app: &CadApp) -> cadcraft_engine::cmd::modeling::profile::Profile {
    app.session
        .doc()
        .map(|d| cadcraft_engine::cmd::modeling::profile::analyze(d, &d.model.iter().map(|e| e.handle).collect::<Vec<_>>()))
        .unwrap_or_default()
}
fn active_frame(app: &CadApp) -> Option<FaceFrame> {
    if let Some(edit) = app.session.state().ok().and_then(|st| st.part_edit.as_ref()) {
        return Some(edit.frame);
    }
    let d = document(app)?;
    if let Some(reference) = app.part_editor.face { support_frame(d, &reference).ok() } else { plane_frame(d, app.part_editor.plane).ok() }
}
fn support_edges(app: &CadApp) -> Vec<[Vec2; 2]> {
    if app.session.state().is_ok_and(|st| st.part_edit.is_some()) {
        return app.part_editor.support_edges.clone();
    }
    document(app)
        .zip(app.part_editor.face)
        .and_then(|(d, reference)| cadcraft_engine::cmd::modeling::support_edges(d, &reference).ok())
        .unwrap_or_default()
}
pub fn sketch_xf(app: &CadApp, mut xf: crate::canvas::Xf) -> crate::canvas::Xf {
    if drafting(app) {
        let (right, up) = sketch_axes(&app.part_editor);
        xf.affine = Some([Vec2::new(right.x, up.x), Vec2::new(right.y, up.y)]);
    }
    xf
}
pub fn sketch_pickable(app: &CadApp) -> bool {
    let (right, up) = sketch_axes(&app.part_editor);
    !drafting(app) || right.cross(up).z.abs() > 0.03
}
fn sketch_axes(state: &State) -> (Vec3, Vec3) {
    let yaw = state.sketch_yaw;
    let tilt = state.sketch_tilt;
    let right = Vec3::new(yaw.cos(), 0.0, -yaw.sin());
    let up = Vec3::new(yaw.sin() * tilt.sin(), tilt.cos(), yaw.cos() * tilt.sin());
    let (s, c) = state.sketch_roll.sin_cos();
    (right * c + up * s, up * c - right * s)
}
/// Screen right, screen up and the direction toward the camera, in world space.
pub(crate) fn navigation_axes(app: &CadApp) -> (Vec3, Vec3, Vec3) {
    if drafting(app)
        && let Some((_, u, v, n)) = active_frame(app)
    {
        let (r, s) = sketch_axes(&app.part_editor);
        let right = u * r.x + v * r.y + n * r.z;
        let up = u * s.x + v * s.y + n * s.z;
        return (right, up, right.cross(up));
    }
    model_axes(&app.part_editor)
}
fn model_axes(state: &State) -> (Vec3, Vec3, Vec3) {
    camera_axes(state.yaw, state.pitch)
}
fn camera_axes(y: f64, p: f64) -> (Vec3, Vec3, Vec3) {
    (
        Vec3::new(-y.cos(), y.sin(), 0.0),
        Vec3::new(-y.sin() * p.sin(), -y.cos() * p.sin(), p.cos()),
        Vec3::new(y.sin() * p.cos(), y.cos() * p.cos(), p.sin()),
    )
}
fn orbit(app: &mut CadApp, delta: egui::Vec2) {
    let dx = f64::from(delta.x) * 0.01 * if app.options.orbit_reverse_horizontal { -1.0 } else { 1.0 };
    let dy = f64::from(delta.y) * 0.01 * if app.options.orbit_reverse_vertical { -1.0 } else { 1.0 };
    let limit = std::f64::consts::FRAC_PI_2;
    if drafting(app) {
        let (s, c) = app.part_editor.sketch_roll.sin_cos();
        let horizontal = dx * c - dy * s;
        let vertical = dx * s + dy * c;
        app.part_editor.sketch_yaw =
            (app.part_editor.sketch_yaw + horizontal / app.part_editor.sketch_tilt.cos().max(0.05)).rem_euclid(std::f64::consts::TAU);
        app.part_editor.sketch_tilt = (app.part_editor.sketch_tilt - vertical).clamp(-limit, limit);
    } else {
        app.part_editor.yaw = (app.part_editor.yaw - dx).rem_euclid(std::f64::consts::TAU);
        app.part_editor.pitch = (app.part_editor.pitch + dy).clamp(-limit, limit);
    }
}
pub(crate) fn navigation_command(app: &mut CadApp, id: &str, params: &serde_json::Value) -> Option<Result<serde_json::Value, String>> {
    if !matches!(id, "ui.part.view" | "ui.part.orbit") {
        return None;
    }
    Some((|| {
        if !is_part(app) {
            return Err("Open a 3D Part to navigate its view".into());
        }
        if id == "ui.part.orbit" {
            let delta: [f32; 2] = serde_json::from_value(params["delta"].clone()).map_err(|_| "Expected a finite orbit delta [x,y]")?;
            if delta.iter().any(|v| !v.is_finite() || v.abs() > 10_000.0) {
                return Err("Orbit delta is out of range".into());
            }
            orbit(app, vec2(delta[0], delta[1]));
        } else {
            let name = params.get("view").and_then(|v| v.as_str()).unwrap_or("home");
            let direction = if let Some(value) = params.get("direction") {
                let xyz: [f64; 3] = serde_json::from_value(value.clone()).map_err(|_| "Expected a view direction [x,y,z]")?;
                if xyz.iter().any(|v| !v.is_finite() || v.abs() > 1.0) || xyz.iter().all(|v| v.abs() < 1e-8) {
                    return Err("View direction must be finite, nonzero and within [-1,1]".into());
                }
                Vec3::new(xyz[0], xyz[1], xyz[2]).normalized()
            } else {
                match name {
                    "home" => Vec3::new(1.0, -1.0, 1.0).normalized(),
                    "top" => Vec3::new(0.0, 0.0, 1.0),
                    "bottom" => Vec3::new(0.0, 0.0, -1.0),
                    "front" => Vec3::new(0.0, -1.0, 0.0),
                    "back" => Vec3::new(0.0, 1.0, 0.0),
                    "right" => Vec3::new(1.0, 0.0, 0.0),
                    "left" => Vec3::new(-1.0, 0.0, 0.0),
                    _ => return Err("Unknown part view".into()),
                }
            };
            let yaw = if direction.x.abs() + direction.y.abs() < 1e-8 { std::f64::consts::PI } else { direction.x.atan2(direction.y) };
            let pitch = direction.z.clamp(-1.0, 1.0).asin();
            if drafting(app) {
                let Some((_, u, v, n)) = active_frame(app) else { return Err("Sketch support is unavailable".into()) };
                let local = Vec3::new(direction.dot(u), direction.dot(v), direction.dot(n));
                app.part_editor.sketch_yaw = local.x.atan2(local.z);
                app.part_editor.sketch_tilt = -local.y.clamp(-1.0, 1.0).asin();
                app.part_editor.sketch_roll = 0.0;
                let (r, s) = sketch_axes(&app.part_editor);
                let target = camera_axes(yaw, pitch).0;
                let local_right = Vec3::new(target.dot(u), target.dot(v), target.dot(n));
                app.part_editor.sketch_roll = local_right.dot(s).atan2(local_right.dot(r));
            } else {
                app.part_editor.yaw = yaw;
                app.part_editor.pitch = pitch;
            }
            if name == "home" && params.get("direction").is_none() && !drafting(app) {
                app.part_editor.zoom = 1.0;
                app.part_editor.pan = egui::Vec2::ZERO;
            }
        }
        Ok(serde_json::Value::Null)
    })())
}
pub fn sketch_navigation(app: &mut CadApp, ui: &egui::Ui, response: &egui::Response, xf: &crate::canvas::Xf) {
    if !drafting(app) || !response.dragged_by(egui::PointerButton::Middle) || crate::viewcube::part_pointer_over(app, ui, xf.rect) {
        return;
    }
    let delta = ui.input(|i| i.pointer.delta());
    if ui.input(|i| i.modifiers.shift) {
        let offset = xf.to_world(xf.rect.center() + delta) - xf.to_world(xf.rect.center());
        if let Ok(st) = app.session.state_mut() {
            let mut view = st.view();
            view.center -= offset;
            st.set_view_quiet(view);
        }
    } else {
        orbit(app, delta);
    }
}
pub fn sketch_context(app: &mut CadApp, painter: &egui::Painter, xf: crate::canvas::Xf) {
    let Some((origin, u, v, n)) = active_frame(app) else { return };
    let bodies = cached_context(app);
    let (r, s) = sketch_axes(&app.part_editor);
    let right = u * r.x + v * r.y + n * r.z;
    let up = u * s.x + v * s.y + n * s.z;
    let eye = right.cross(up);
    let center = origin + u * xf.center.x + v * xf.center.y;
    let project = |p: Vec3| {
        let q = p - center;
        (xf.rect.center() + vec2((q.dot(right) * xf.scale) as f32, (-q.dot(up) * xf.scale) as f32), q.dot(eye))
    };
    let mut visible = Vec::new();
    let t = Tokens::get();
    for body in bodies.iter() {
        let front: Vec<_> = body.faces.iter().enumerate().map(|(i, _)| body.face_normal(i).is_some_and(|normal| normal.dot(eye) > 1e-8)).collect();
        let edges = body.display_edges();
        for (index, _) in body.faces.iter().enumerate() {
            if !front.get(index).copied().unwrap_or(false) {
                continue;
            }
            let Some(points) = body.face_points(index) else { continue };
            let screen: Vec<_> = points.iter().map(|p| project(*p).0).collect();
            let depth = points.iter().map(|p| project(*p).1).sum::<f64>() / 4.0;
            let normal = body.face_normal(index).unwrap_or(n);
            let shade = (0.62 + 0.2 * normal.dot(Vec3::new(-0.35, -0.45, 0.82)).abs()) as f32;
            let fill = t.panel.gamma_multiply(shade);
            let lines: Vec<_> = edges
                .iter()
                .filter(|e| e.faces.contains(&index) && (!e.smooth || e.faces.iter().any(|i| !front.get(*i).copied().unwrap_or(false))))
                .map(|e| [project(e.a).0, project(e.b).0])
                .collect();
            visible.push((depth, screen, fill, lines));
        }
    }
    visible.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, points, fill, lines) in visible {
        let mut mesh = egui::Mesh::default();
        for point in &points {
            mesh.colored_vertex(*point, fill);
        }
        for i in 1..points.len().saturating_sub(1) {
            mesh.add_triangle(0, i as u32, (i + 1) as u32);
        }
        painter.add(Shape::mesh(mesh));
        for line in lines {
            painter.line_segment(line, Stroke::new(1.0, t.text_faint));
        }
    }
}
pub fn shade(app: &CadApp, painter: &egui::Painter, xf: crate::canvas::Xf) {
    for edge in support_edges(app) {
        painter.line_segment(edge.map(|p| xf.to_screen(p)), Stroke::new(1.5, Tokens::get().text_dim));
    }
    if let Some(d) = document(app) {
        let bounds = xf.world_bounds();
        for l in reference_lines_at(d, active_frame(app), if app.part_editor.face.is_some() { None } else { Some(app.part_editor.plane) }) {
            let points = if l.a.abs() > l.b.abs() {
                [Vec2::new((-l.c - l.b * bounds.min.y) / l.a, bounds.min.y), Vec2::new((-l.c - l.b * bounds.max.y) / l.a, bounds.max.y)]
            } else {
                [Vec2::new(bounds.min.x, (-l.c - l.a * bounds.min.x) / l.b), Vec2::new(bounds.max.x, (-l.c - l.a * bounds.max.x) / l.b)]
            };
            painter.line_segment(points.map(|p| xf.to_screen(p)), Stroke::new(1.0, Tokens::get().text_dim.gamma_multiply(0.4)));
        }
    }
    let status = profile(app);
    if !status.ready() {
        return;
    }
    let fill = Tokens::get().accent.gamma_multiply(0.18);
    for tri in status.triangles.as_chunks::<3>().0 {
        painter.add(Shape::convex_polygon(tri.iter().map(|p| xf.to_screen(*p)).collect(), fill, Stroke::NONE));
    }
}
pub fn reference_hit(app: &CadApp, p: Vec2, tolerance: f64) -> Option<cadcraft_engine::snap::SnapHit> {
    if !drafting(app) || app.session.settings.osmode == 0 || app.session.settings.osmode & 16384 != 0 || app.session.snap_override().is_some() {
        return None;
    }
    let d = document(app)?;
    let lines = reference_lines_at(d, active_frame(app), if app.part_editor.face.is_some() { None } else { Some(app.part_editor.plane) });
    let edges = support_edges(app);
    let mut points = Vec::new();
    for [a, b] in &edges {
        points.extend([(*a, "Face endpoint"), (*b, "Face endpoint"), ((*a + *b) * 0.5, "Face midpoint")]);
    }
    let hit = snap_reference(p, &lines, &points, tolerance);
    let nearest = edges
        .iter()
        .filter_map(|[a, b]| {
            let ab = *b - *a;
            let length = ab.dot(ab);
            if length < 1e-12 {
                return None;
            }
            let q = *a + ab * ((p - *a).dot(ab) / length).clamp(0.0, 1.0);
            (q.dist(p) <= tolerance).then_some(q)
        })
        .min_by(|a, b| a.dist(p).total_cmp(&b.dist(p)));
    let point = hit.map(|(q, _)| q).or(nearest)?;
    Some(cadcraft_engine::snap::SnapHit { point, mode: cadcraft_engine::snap::mode::INT, name: "Sketch reference", viewport: None, deferred: None })
}

pub fn sync(app: &mut CadApp) {
    let Ok(st) = app.session.state() else { return };
    if app.part_editor.uid != st.uid {
        app.part_editor = State { uid: st.uid, ..State::default() };
        app.ui.toolset_tab = if st.doc.part.is_some() { "Modeling" } else { "Drafting" }.into();
        app.ui.start_tab = false;
        if let Some(edit) = &st.part_edit {
            app.part_editor.mode = Mode::Sketch(false);
            app.part_editor.plane = edit.plane;
            app.part_editor.sketch = edit.sketch;
            app.part_editor.face = edit.face;
        }
    }
    if let Some(edit) = &st.part_edit {
        if app.part_editor.sketch_edit_id != edit.edit_id {
            app.part_editor.sketch_edit_id = edit.edit_id;
            app.part_editor.support_edges =
                edit.face.and_then(|reference| cadcraft_engine::cmd::modeling::support_edges(&edit.host.doc, &reference).ok()).unwrap_or_default();
            app.part_editor.sketch_yaw = 0.0;
            app.part_editor.sketch_tilt = 0.0;
            app.part_editor.sketch_roll = 0.0;
        }
        if !matches!(app.part_editor.mode, Mode::Sketch(_) | Mode::Depth) {
            app.part_editor.mode = Mode::Sketch(false);
            app.part_editor.plane = edit.plane;
            app.part_editor.sketch = edit.sketch;
            app.part_editor.face = edit.face;
        }
    } else if matches!(app.part_editor.mode, Mode::Sketch(_)) {
        app.part_editor.mode = Mode::Idle;
    }
}
fn execute(app: &mut CadApp, command: &str, p: serde_json::Value) -> bool {
    match app.run(command, p) {
        Ok(_) => {
            app.part_editor.error = None;
            true
        }
        Err(e) => {
            app.part_editor.error = Some(e);
            false
        }
    }
}

fn load_sketch(app: &mut CadApp) {
    let Some(d) = document(app).cloned() else { return };
    let Some(part) = &d.part else { return };
    let Some(sketch) = app.part_editor.sketch.and_then(|id| part.sketches.iter().find(|s| s.id == id)) else { return };
    app.part_editor.plane = sketch.plane;
    app.part_editor.face = sketch.face;
    app.part_editor.drawn = true;
    if let Some(e) = d.entity(sketch.profile)
        && let cadcraft_doc::EntityKind::LwPolyline(p) = &e.kind
        && let Some(v) = p.vertices.first()
    {
        app.part_editor.origin = v.p;
    }
    for row in parameters(&d) {
        if row.name == sketch.width_name() {
            app.part_editor.size.x = row.value.unwrap_or(40.0);
        }
        if row.name == sketch.height_name() {
            app.part_editor.size.y = row.value.unwrap_or(20.0);
        }
        if let Some(f) = app.part_editor.feature.and_then(|id| part.features.iter().find(|f| f.id == id))
            && row.name == f.depth
        {
            app.part_editor.depth = row.expr;
        }
    }
}
fn begin(app: &mut CadApp, mode: Mode) {
    sync(app);
    app.part_editor.error = None;
    app.part_editor.first = None;
    if mode != Mode::Plane {
        load_sketch(app);
    }
    if matches!(mode, Mode::Sketch(_)) && !app.session.state().is_ok_and(|st| st.part_edit.is_some()) {
        let p = json!({"plane":app.part_editor.plane,"sketch":app.part_editor.sketch,"face":app.part_editor.face});
        if !execute(app, "part.sketch.begin", p) {
            return;
        }
    }
    app.part_editor.mode = mode;
    app.ui.toolset_tab = "Modeling".into();
}
fn default_target(app: &CadApp) -> Option<u32> {
    match app.part_editor.selected {
        Selected::Body(id) | Selected::Face(id, _) => Some(id),
        Selected::Extrude(id) => app
            .session
            .doc()
            .ok()
            .and_then(|d| d.part.as_ref())
            .and_then(|p| p.features.iter().find(|f| f.id == id))
            .map(|f| f.target.unwrap_or(f.id)),
        _ => document(app).and_then(|d| bodies(d).ok()).filter(|b| b.len() == 1).and_then(|b| b.first().map(|b| b.id)),
    }
}
fn new_sketch(app: &mut CadApp, extrude: bool) {
    sync(app);
    let target = default_target(app);
    app.part_editor.face = if matches!(app.part_editor.selected, Selected::Face(..)) { app.part_editor.face } else { None };
    app.part_editor.reverse = false;
    app.part_editor.auto_operation = false;
    app.part_editor.guide = None;
    app.part_editor.sketch = None;
    app.part_editor.feature = None;
    app.part_editor.drawn = false;
    app.part_editor.first = None;
    app.part_editor.origin = Vec2::ZERO;
    app.part_editor.size = Vec2::new(40.0, 20.0);
    app.part_editor.depth = "10".into();
    app.part_editor.target = target;
    app.part_editor.operation = if extrude && target.is_some() { SolidOperation::Add } else { SolidOperation::NewBody };
    begin(
        app,
        if matches!(app.part_editor.selected, Selected::Plane(_) | Selected::Face(..)) { Mode::Sketch(extrude) } else { Mode::Placement(extrude) },
    );
}
fn edit_sketch_ui(app: &mut CadApp, id: u32) {
    app.part_editor.sketch = Some(id);
    app.part_editor.feature = None;
    app.part_editor.selected = Selected::Sketch(id);
    begin(app, Mode::Sketch(false));
}
fn edit_feature(app: &mut CadApp, id: u32) {
    app.part_editor.auto_operation = false;
    app.part_editor.guide = None;
    let feature = document(app).and_then(|d| d.part.as_ref()).and_then(|p| p.features.iter().find(|f| f.id == id)).cloned();
    if let Some(f) = feature {
        app.part_editor.feature = Some(id);
        app.part_editor.sketch = Some(f.sketch);
        app.part_editor.operation = f.operation;
        app.part_editor.reverse = f.reverse;
        app.part_editor.target = f.target;
        app.part_editor.selected = Selected::Extrude(id);
        begin(app, Mode::Depth);
    }
}
pub fn route(app: &mut CadApp, id: &str) -> bool {
    if !is_part(app) {
        return false;
    }
    match id {
        "ui.part.normal" => {
            app.part_editor.sketch_yaw = 0.0;
            app.part_editor.sketch_tilt = 0.0;
            app.part_editor.sketch_roll = 0.0;
            true
        }
        "extrude" => {
            let selected = if let Selected::Sketch(id) = app.part_editor.selected {
                document(app).and_then(|d| d.part.as_ref()).filter(|p| !p.features.iter().any(|f| f.sketch == id)).map(|_| id)
            } else {
                None
            };
            if let Some(id) = selected {
                let target = default_target(app);
                app.part_editor.target = target;
                app.part_editor.operation = if target.is_some() { SolidOperation::Add } else { SolidOperation::NewBody };
                app.part_editor.sketch = Some(id);
                app.part_editor.feature = None;
                app.part_editor.depth = "10".into();
                app.part_editor.auto_operation = false;
                begin(app, Mode::Depth);
            } else {
                new_sketch(app, true);
            }
            true
        }
        "ui.part.plane" => {
            begin(app, Mode::Plane);
            app.part_editor.edit_plane = None;
            true
        }
        "ui.part.sketch" => {
            if let Mode::Placement(extrude) = app.part_editor.mode {
                if matches!(app.part_editor.selected, Selected::Plane(_) | Selected::Face(..)) {
                    begin(app, Mode::Sketch(extrude));
                }
            } else if app.part_editor.mode == Mode::Idle {
                new_sketch(app, false);
            }
            true
        }
        "ui.part.placement" => {
            let extrude = matches!(app.part_editor.mode, Mode::Sketch(true));
            begin(app, Mode::Placement(extrude));
            true
        }
        "ui.part.fit" | "3dorbit" | "zoom.extents" => {
            if drafting(app) {
                let _ = app.session.zoom_extents();
                return true;
            }
            app.part_editor.zoom = 1.0;
            app.part_editor.pan = egui::Vec2::ZERO;
            app.part_editor.yaw = 3.0 * std::f64::consts::FRAC_PI_4;
            app.part_editor.pitch = (1.0_f64 / 3.0_f64.sqrt()).asin();
            true
        }
        _ => false,
    }
}

pub fn is_sketch(app: &CadApp) -> bool {
    matches!(app.part_editor.mode, Mode::Sketch(_))
}

pub fn tool_enabled(app: &CadApp, command: &str) -> bool {
    match command {
        "ui.part.plane" => app.part_editor.mode == Mode::Idle,
        "extrude" => matches!(app.part_editor.mode, Mode::Idle | Mode::Placement(_)),
        "ui.part.sketch" => !matches!(app.part_editor.mode, Mode::Plane | Mode::Depth),
        "rectang" => is_sketch(app),
        "ui.part.placement" => false,
        "wipeout" => false,
        "ui.part.fit" | "3dorbit" => true,
        "ui.part.normal" => drafting(app),
        _ => drafting(app) && cadcraft_engine::find_command(command).is_some(),
    }
}

pub fn tree(app: &mut CadApp, ui: &mut egui::Ui) {
    sync(app);
    let editing = app.session.state().is_ok_and(|st| st.part_edit.is_some());
    ui.horizontal(|ui| {
        ui.selectable_value(&mut app.part_editor.layers, false, "Model Tree");
        ui.selectable_value(&mut app.part_editor.layers, true, "Layers");
    });
    if app.part_editor.layers {
        crate::palettes::layers_section(app, ui);
        return;
    }
    let Some(d) = document(app).cloned() else { return };
    let Some(part) = d.part.clone() else { return };

    ui.separator();
    ui.label(egui::RichText::new(app.session.state().map_or("Part", |s| s.title.as_str())).strong());
    egui::CollapsingHeader::new("Origin / Reference planes").default_open(true).show(ui, |ui| {
        for plane in &part.planes {
            let r = tree_row(ui, crate::icons::Icon::DatumPlane, app.part_editor.selected == Selected::Plane(plane.id), &plane.name);
            if r.clicked() && !editing {
                app.part_editor.selected = Selected::Plane(plane.id);
                app.part_editor.face = None;
                app.part_editor.plane = plane.id;
            }
            if r.double_clicked() && !editing && plane.parent.is_some() {
                begin(app, Mode::Plane);
                app.part_editor.edit_plane = Some(plane.id);
                app.part_editor.offset = plane.offset.clone();
            }
        }
    });
    let limit = if drafting(app) {
        app.part_editor
            .sketch
            .and_then(|id| part.features.iter().position(|f| f.sketch == id).or_else(|| part.features.iter().position(|f| f.sketch >= id)))
            .unwrap_or(part.features.len())
    } else {
        part.features.len()
    };
    if drafting(app) {
        ui.label(egui::RichText::new("Sketch edit / later history rolled back").small());
    }
    for (index, feature) in part.features.iter().enumerate() {
        let selected = app.part_editor.selected == Selected::Extrude(feature.id);
        let title = egui::RichText::new(format!("{} · {}", feature.name, operation_label(feature.operation))).color(if index >= limit {
            Tokens::get().text_faint
        } else if selected {
            Tokens::get().selection
        } else {
            Tokens::get().text
        });
        let response = egui::CollapsingHeader::new(title).id_salt(("part_feature", feature.id)).default_open(true).show(ui, |ui| {
            if let Some(sketch) = part.sketches.iter().find(|s| s.id == feature.sketch) {
                sketch_tree_row(app, ui, sketch);
            }
        });
        if response.header_response.clicked() && !editing {
            app.part_editor.selected = Selected::Extrude(feature.id);
        }
        if response.header_response.double_clicked() && !editing {
            edit_feature(app, feature.id);
        }
        if response.header_response.secondary_clicked() && !editing {
            app.part_editor.selected = Selected::Extrude(feature.id);
        }
        response.header_response.context_menu(|ui| {
            let idle = !editing && app.part_editor.mode == Mode::Idle;
            if ui.add_enabled(idle, egui::Button::new("Edit Feature")).clicked() {
                edit_feature(app, feature.id);
                ui.close();
            }
            if ui.add_enabled(idle, egui::Button::new("Edit Sketch")).clicked() {
                edit_sketch_ui(app, feature.sketch);
                ui.close();
            }
            ui.separator();
            if ui
                .add_enabled(idle, egui::Button::new("Delete Feature"))
                .on_hover_text("Delete this extrusion and retain its source sketch. Undo restores the feature.")
                .clicked()
            {
                if execute(app, "part.feature.delete", json!({"feature":feature.id})) {
                    app.part_editor.selected = Selected::Sketch(feature.sketch);
                    app.part_editor.feature = None;
                    app.part_editor.face = None;
                }
                ui.close();
            }
        });
    }
    for sketch in &part.sketches {
        if !part.features.iter().any(|f| f.sketch == sketch.id) {
            sketch_tree_row(app, ui, sketch);
        }
    }
    {
        let parts = if drafting(app) { cached_context(app) } else { cached_model(app) };
        egui::CollapsingHeader::new("Bodies").default_open(true).show(ui, |ui| {
            for body in parts.iter() {
                if tree_row(ui, crate::icons::Icon::Blocks, app.part_editor.selected == Selected::Body(body.id), &format!("Body {}", body.id - 1999))
                    .clicked()
                    && !editing
                {
                    app.part_editor.selected = Selected::Body(body.id);
                }
            }
        });
    }
    if app.part_editor.feature.is_none() && matches!(app.part_editor.mode, Mode::Placement(true) | Mode::Sketch(true) | Mode::Depth) {
        ui.label(egui::RichText::new("Extrude · pending").small().color(Tokens::get().text_dim));
    }

    ui.separator();
    ui.label(format!("Units: {}", part.units));
}

fn tree_row(ui: &mut egui::Ui, icon: crate::icons::Icon, selected: bool, label: &str) -> egui::Response {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(16.0, 18.0), Sense::hover());
        crate::icons::paint(ui.painter(), rect.shrink(1.0), icon, false);
        ui.selectable_label(selected, label)
    })
    .inner
}

fn sketch_tree_row(app: &mut CadApp, ui: &mut egui::Ui, sketch: &cadcraft_doc::PartSketch) {
    let r = tree_row(ui, crate::icons::Icon::Sketch, app.part_editor.selected == Selected::Sketch(sketch.id), &sketch.name);
    if r.clicked() && !app.session.state().is_ok_and(|s| s.part_edit.is_some()) {
        app.part_editor.selected = Selected::Sketch(sketch.id);
    }
    if r.double_clicked() && !app.session.state().is_ok_and(|s| s.part_edit.is_some()) {
        edit_sketch_ui(app, sketch.id);
    }
}
fn operation_label(operation: SolidOperation) -> &'static str {
    match operation {
        SolidOperation::NewBody => "New Body",
        SolidOperation::Add => "Add Material",
        SolidOperation::Remove => "Remove Material",
    }
}
fn args(app: &CadApp) -> serde_json::Value {
    json!({"extrude":app.part_editor.mode==Mode::Depth,"plane":app.part_editor.plane,"autoOperation":app.part_editor.auto_operation,
        "depth":app.part_editor.depth,"sketch":app.part_editor.sketch,"feature":app.part_editor.feature,"operation":app.part_editor.operation,"target":app.part_editor.target,"reverse":app.part_editor.reverse})
}
fn reverse_extrusion(app: &mut CadApp) {
    app.part_editor.reverse = !app.part_editor.reverse;
    app.part_editor.auto_operation = app.part_editor.operation != SolidOperation::NewBody;
    app.part_editor.error = None;
}

fn confirm(app: &mut CadApp) {
    match app.part_editor.mode {
        Mode::Placement(extrude) => {
            if matches!(app.part_editor.selected, Selected::Plane(_) | Selected::Face(..)) {
                begin(app, Mode::Sketch(extrude));
            }
        }
        Mode::Sketch(true) => {
            app.session.cancel();
            let status = profile(app);
            if status.ready() {
                app.part_editor.mode = Mode::Depth;
            } else {
                app.part_editor.error = Some(status.message().into());
            }
        }
        Mode::Sketch(false) | Mode::Depth => {
            let command = if app.session.state().is_ok_and(|st| st.part_edit.is_some()) {
                "part.sketch.commit"
            } else if app.part_editor.mode == Mode::Depth {
                "extrude"
            } else {
                "part.sketch"
            };
            match app.run(command, args(app)) {
                Ok(result) => {
                    app.part_editor.error = None;
                    app.part_editor.mode = Mode::Idle;
                    if let Some(id) = result.get("feature").and_then(|v| v.as_u64()).and_then(|id| u32::try_from(id).ok()) {
                        app.part_editor.selected = Selected::Extrude(id);
                    } else if let Some(id) = result.get("sketch").and_then(|v| v.as_u64()).and_then(|id| u32::try_from(id).ok()) {
                        app.part_editor.selected = Selected::Sketch(id);
                    }
                }
                Err(e) => app.part_editor.error = Some(e),
            }
        }
        Mode::Plane => {
            let p = json!({"parent":app.part_editor.plane,"offset":app.part_editor.offset,"id":app.part_editor.edit_plane});
            if execute(app, "part.plane", p) {
                if let Some(id) =
                    app.part_editor.edit_plane.or_else(|| document(app).and_then(|d| d.part.as_ref()).and_then(|p| p.planes.last()).map(|p| p.id))
                {
                    app.part_editor.plane = id;
                    app.part_editor.selected = Selected::Plane(id);
                }
                app.part_editor.mode = Mode::Idle;
            }
        }
        _ => {}
    }
}

fn cancel(app: &mut CadApp) {
    if app.session.state().is_ok_and(|st| st.part_edit.is_some()) {
        let _ = app.run("part.sketch.cancel", json!({}));
    }
    app.part_editor.mode = Mode::Idle;
    app.part_editor.first = None;
    app.part_editor.error = None;
    app.part_editor.drawn = false;
}
pub fn properties(app: &mut CadApp, ui: &mut egui::Ui) {
    sync(app);
    let Some(part) = document(app).and_then(|d| d.part.clone()) else { return };
    let mode = app.part_editor.mode;
    let title = match mode {
        Mode::Placement(_) => "Sketch Placement",
        Mode::Sketch(_) => "Sketch",
        Mode::Plane => "Datum Plane",
        Mode::Depth => "Extrude",
        Mode::Idle => match app.part_editor.selected {
            Selected::Plane(_) => "Datum Plane",
            Selected::Sketch(_) => "Sketch",
            Selected::Extrude(_) => "Extrude",
            Selected::Body(_) => "Body",
            Selected::Face(..) => "Planar Face",
            _ => "Part",
        },
    };
    crate::palettes::section_header(ui, title);
    crate::palettes::prop_row(ui, "Units", |ui| {
        ui.label(&part.units);
    });
    if mode == Mode::Idle {
        match app.part_editor.selected {
            Selected::Face(body, _) => {
                crate::palettes::prop_row(ui, "Support", |ui| {
                    ui.label(format!("Body {} · planar face", body - 1999));
                });
                ui.horizontal(|ui| {
                    if ui.button("Sketch on Face").clicked() {
                        new_sketch(app, false);
                    }
                    if ui.button("Extrude from Face").clicked() {
                        new_sketch(app, true);
                    }
                });
            }
            Selected::Plane(id) => {
                if let Some(p) = part.planes.iter().find(|p| p.id == id) {
                    crate::palettes::prop_row(ui, "Name", |ui| {
                        ui.label(&p.name);
                    });
                    crate::palettes::prop_row(ui, "Offset", |ui| {
                        ui.label(format!("{} {}", p.offset, part.units));
                    });
                    ui.horizontal(|ui| {
                        ui.add_space(12.0);
                        if p.parent.is_some() && ui.button("Edit Plane").clicked() {
                            begin(app, Mode::Plane);
                            app.part_editor.edit_plane = Some(id);
                            app.part_editor.offset = p.offset.clone();
                        }
                        if ui.button("New Sketch").clicked() {
                            app.part_editor.plane = id;
                            new_sketch(app, false);
                        }
                    });
                }
            }
            Selected::Sketch(id) => {
                if ui.button("Edit Sketch").clicked() {
                    edit_sketch_ui(app, id);
                }
            }
            Selected::Extrude(id) => {
                crate::palettes::prop_row(ui, "Depth type", |ui| {
                    ui.label("Blind");
                });
                if let Some(feature) = part.features.iter().find(|f| f.id == id) {
                    crate::palettes::prop_row(ui, "Operation", |ui| {
                        ui.label(operation_label(feature.operation));
                    });
                    let value = app
                        .session
                        .doc()
                        .ok()
                        .map(parameters)
                        .and_then(|rows| rows.into_iter().find(|r| r.name == feature.depth))
                        .and_then(|row| row.value);
                    crate::palettes::prop_row(ui, "Distance", |ui| {
                        ui.label(value.map_or_else(|| feature.depth.clone(), |v| format!("{v:.3} {}", part.units)));
                    });
                }
                if ui.button("Edit Extrude").clicked() {
                    edit_feature(app, id);
                }
            }
            _ => {
                ui.add_space(4.0);
                ui.label(egui::RichText::new("Select a plane or feature to view its properties.").small().color(Tokens::get().text_dim));
            }
        }
        let cached = cached_model(app);
        if let Some(b) = cached.iter().find(|b| match app.part_editor.selected {
            Selected::Body(id) => b.id == id,
            Selected::Extrude(id) => part.features.iter().find(|f| f.id == id).is_some_and(|f| b.id == f.target.unwrap_or(f.id)),
            _ => true,
        }) {
            crate::palettes::prop_row(ui, "Volume", |ui| {
                ui.label(format!("{:.3} {}³", b.volume, part.units));
            });
        }
    } else {
        if matches!(mode, Mode::Placement(_) | Mode::Plane) && app.part_editor.edit_plane.is_none() {
            crate::palettes::prop_row(ui, "Reference", |ui| {
                let selected = matches!(app.part_editor.selected, Selected::Plane(_)) || mode == Mode::Plane;
                let name = if selected {
                    part.planes.iter().find(|p| p.id == app.part_editor.plane).map_or("Select plane", |p| p.name.as_str())
                } else {
                    "Select plane"
                };
                egui::ComboBox::from_id_salt("part_reference").width((ui.available_width() - 10.0).max(40.0)).selected_text(name).show_ui(ui, |ui| {
                    for p in &part.planes {
                        if ui.selectable_value(&mut app.part_editor.plane, p.id, &p.name).clicked() {
                            app.part_editor.selected = Selected::Plane(p.id);
                            app.part_editor.face = None;
                        }
                    }
                });
            });
        } else if let Some(face) = app.part_editor.face {
            crate::palettes::prop_row(ui, "Support", |ui| {
                ui.label(format!("Body {} / planar face", face.body - 1999));
            });
        } else if let Some(p) = part.planes.iter().find(|p| p.id == app.part_editor.plane) {
            crate::palettes::prop_row(ui, "Plane", |ui| {
                ui.label(&p.name);
            });
        }
        match mode {
            Mode::Plane => crate::palettes::prop_row(ui, "Offset", |ui| {
                ui.add(egui::TextEdit::singleline(&mut app.part_editor.offset).desired_width((ui.available_width() - 10.0).max(40.0)));
            }),
            Mode::Sketch(_) => {
                crate::palettes::prop_row(ui, "Width", |ui| {
                    ui.add(egui::DragValue::new(&mut app.part_editor.size.x).range(0.001..=1e6).speed(0.1));
                });
                crate::palettes::prop_row(ui, "Height", |ui| {
                    ui.add(egui::DragValue::new(&mut app.part_editor.size.y).range(0.001..=1e6).speed(0.1));
                });
                crate::palettes::prop_row(ui, "Profile", |ui| {
                    ui.label(if app.part_editor.drawn { "Closed rectangle" } else { "Awaiting corners" });
                });
                crate::palettes::prop_row(ui, "Snapping", |ui| {
                    ui.label("Datum references");
                });
            }
            Mode::Depth => {
                crate::palettes::prop_row(ui, "Output", |ui| {
                    ui.label("Solid");
                });
                crate::palettes::prop_row(ui, "Operation", |ui| {
                    egui::ComboBox::from_id_salt("solid_operation")
                        .width((ui.available_width() - 10.0).max(60.0))
                        .selected_text(if app.part_editor.auto_operation {
                            format!("{} (Auto)", operation_label(app.part_editor.operation))
                        } else {
                            operation_label(app.part_editor.operation).into()
                        })
                        .show_ui(ui, |ui| {
                            for operation in [SolidOperation::NewBody, SolidOperation::Add, SolidOperation::Remove] {
                                if ui
                                    .selectable_value(&mut app.part_editor.operation, operation, operation_label(operation))
                                    .on_hover_text(match operation {
                                        SolidOperation::NewBody => "Creates a separate solid, even when touching or overlapping another body.",
                                        SolidOperation::Add => "Merges the extrusion into the selected target body.",
                                        SolidOperation::Remove => "Cuts material from the selected target body.",
                                    })
                                    .clicked()
                                {
                                    app.part_editor.auto_operation = false;
                                    app.part_editor.error = None;
                                }
                            }
                        });
                });
                if app.part_editor.operation != SolidOperation::NewBody {
                    let targets = cached_model(app);
                    crate::palettes::prop_row(ui, "Target body", |ui| {
                        egui::ComboBox::from_id_salt("solid_target")
                            .width((ui.available_width() - 10.0).max(60.0))
                            .selected_text(app.part_editor.target.map_or_else(|| "Select body".into(), |id| format!("Body {}", id - 1999)))
                            .show_ui(ui, |ui| {
                                for body in targets.iter() {
                                    if app.part_editor.feature.is_none_or(|id| body.id < id) {
                                        ui.selectable_value(&mut app.part_editor.target, Some(body.id), format!("Body {}", body.id - 1999));
                                    }
                                }
                            });
                    });
                }
                crate::palettes::prop_row(ui, "Depth type", |ui| {
                    ui.label("Blind");
                });
                crate::palettes::prop_row(ui, "Distance", |ui| {
                    ui.add(egui::TextEdit::singleline(&mut app.part_editor.depth).desired_width((ui.available_width() - 10.0).max(40.0)));
                });
                crate::palettes::prop_row(ui, "Direction", |ui| {
                    let mut reverse = app.part_editor.reverse;
                    if ui.checkbox(&mut reverse, "Reverse").changed() {
                        reverse_extrusion(app);
                    }
                });
                if ui.button("Edit Sketch").clicked() {
                    begin(app, Mode::Sketch(true));
                }
            }
            Mode::Placement(_) => {
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new("Select a planar solid face or a datum plane, then choose Sketch.").small().color(Tokens::get().text_dim),
                );
            }
            _ => {}
        }
    }
    if let Some(error) = &app.part_editor.error {
        ui.colored_label(ui.visuals().error_fg_color, error);
    }
}

fn project(p: Vec3, center: Vec3, state: &State, rect: Rect, scale: f64) -> (Pos2, f64) {
    let p = p - center;
    let (right, up, eye) = model_axes(state);
    (rect.center() + state.pan + vec2((p.dot(right) * scale) as f32, (-p.dot(up) * scale) as f32), p.dot(eye))
}
fn polygon(ui: &egui::Ui, points: Vec<Pos2>, fill: Color32) {
    if points.len() < 3 {
        return;
    }
    let area: f32 = points.iter().zip(points.iter().cycle().skip(1)).take(points.len()).map(|(a, b)| a.x * b.y - a.y * b.x).sum();
    if area.abs() > 1e-6 {
        let mut mesh = egui::Mesh::default();
        for point in &points {
            mesh.colored_vertex(*point, fill);
        }
        for i in 1..points.len().saturating_sub(1) {
            mesh.add_triangle(0, i as u32, (i + 1) as u32);
        }
        ui.painter().add(Shape::mesh(mesh));
    }
}

pub fn toolbar(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    if app.part_editor.mode != Mode::Idle {
        egui::Frame::NONE.fill(t.chrome).inner_margin(egui::Margin::symmetric(8, 5)).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                let sketch = matches!(app.part_editor.mode, Mode::Sketch(_));
                let placement = matches!(app.part_editor.mode, Mode::Placement(_));
                ui.strong(if sketch {
                    "Sketch"
                } else if placement {
                    if matches!(app.part_editor.mode, Mode::Placement(true)) { "Extrude · Placement" } else { "Sketch · Placement" }
                } else if app.part_editor.mode == Mode::Depth {
                    "Extrude · Solid · Blind"
                } else {
                    "Datum Plane"
                });
                if app.part_editor.mode == Mode::Depth {
                    ui.label("Distance");
                    ui.add(egui::TextEdit::singleline(&mut app.part_editor.depth).desired_width(100.0));
                }
                if sketch && drafting(app) {
                    let status = profile(app);
                    ui.colored_label(if status.ready() { t.accent } else { t.text_dim }, status.message());
                }
                let enabled = if placement { matches!(app.part_editor.selected, Selected::Plane(_)) } else { true };
                if ui
                    .add_enabled(
                        enabled,
                        egui::Button::new(if placement {
                            "Sketch"
                        } else if sketch {
                            "Finish Sketch"
                        } else {
                            "Confirm"
                        }),
                    )
                    .clicked()
                {
                    confirm(app);
                }
                if ui.button("Cancel").clicked() {
                    cancel(app);
                }
                if sketch && ui.button("Normal to Sketch").clicked() {
                    route(app, "ui.part.normal");
                }
                if sketch && !sketch_pickable(app) {
                    ui.label("Sketch is edge-on; orbit slightly to draw");
                }
            });
        });
    }
    if let Some(error) = &app.part_editor.error {
        ui.colored_label(ui.visuals().error_fg_color, error);
    }
}

pub fn canvas(app: &mut CadApp, ui: &mut egui::Ui) {
    toolbar(app, ui);
    let t = Tokens::get();
    let rect = ui.available_rect_before_wrap();
    let response = ui.allocate_rect(rect, Sense::click_and_drag());
    app.canvas.rect = Some(rect);
    app.session.viewport_px = (f64::from(rect.width()), f64::from(rect.height()));
    ui.painter().rect_filled(rect, 0.0, t.canvas);
    let over_cube = crate::viewcube::part_pointer_over(app, ui, rect);
    let Some(d) = document(app).cloned() else { return };
    let Some(part) = &d.part else { return };
    if !over_cube && response.dragged_by(egui::PointerButton::Middle) && ui.input(|i| i.modifiers.shift) {
        app.part_editor.pan += ui.input(|i| i.pointer.delta());
    } else if !over_cube && response.dragged_by(egui::PointerButton::Middle) {
        let delta = ui.input(|i| i.pointer.delta());
        orbit(app, delta);
    }
    if response.hovered() && !over_cube {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        app.part_editor.zoom = (app.part_editor.zoom * (f64::from(scroll) * 0.002).exp()).clamp(0.1, 20.0);
    }
    let mut parts = cached_model(app);
    if app.part_editor.mode == Mode::Depth {
        match preview_parts(app, &d) {
            Ok((preview, operation)) => {
                parts = preview;
                if let Some(operation) = operation {
                    app.part_editor.operation = operation;
                }
                app.part_editor.error = None;
            }
            Err(e) => {
                ui.painter().text(rect.left_top() + vec2(16.0, 52.0), Align2::LEFT_TOP, e, FontId::proportional(14.0), t.text);
            }
        }
    }
    let mut center = Vec3::new(15.0, 10.0, 0.0);
    let mut extent: f64 = 100.0;
    if !parts.is_empty() {
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        for body in parts.iter() {
            for p in &body.vertices {
                for i in 0..3 {
                    min[i] = min[i].min(p[i]);
                    max[i] = max[i].max(p[i]);
                }
            }
        }
        center = Vec3::new((min[0] + max[0]) / 2.0, (min[1] + max[1]) / 2.0, (min[2] + max[2]) / 2.0);
        extent = (max[0] - min[0]).hypot(max[1] - min[1]).hypot(max[2] - min[2]).max(60.0) * 1.7;
    }
    let scale = f64::from(rect.width().min(rect.height())) / extent * app.part_editor.zoom;
    let mut pick: Vec<(u32, Vec<Pos2>)> = Vec::new();
    for plane in &part.planes {
        let Ok((mut origin, u, v, n)) = plane_frame(&d, plane.id) else { continue };
        if app.part_editor.mode == Mode::Plane
            && app.part_editor.edit_plane == Some(plane.id)
            && let Ok(offset) = app.part_editor.offset.parse::<f64>()
            && let Ok(old) = plane.offset.parse::<f64>()
        {
            origin = origin + n * (offset - old);
        }
        let half = 25.0;
        let points: Vec<_> = [(-half, -half), (half, -half), (half, half), (-half, half)]
            .into_iter()
            .map(|(a, b)| project(origin + u * a + v * b, center, &app.part_editor, rect, scale).0)
            .collect();
        let active = app.part_editor.plane == plane.id || app.part_editor.selected == Selected::Plane(plane.id);
        if parts.is_empty()
            || app.part_editor.selected == Selected::Plane(plane.id)
            || matches!(app.part_editor.mode, Mode::Plane | Mode::Placement(_))
        {
            let color = if active { t.accent } else { t.text_faint };
            ui.painter().add(Shape::closed_line(points.clone(), Stroke::new(if active { 2.0 } else { 1.0 }, color)));
            if let Some(label) = points.get(2) {
                ui.painter().text(*label, Align2::LEFT_BOTTOM, &plane.name, FontId::proportional(13.0), color);
            }
            pick.push((plane.id, points));
        }
    }
    if app.part_editor.mode == Mode::Plane
        && app.part_editor.edit_plane.is_none()
        && let Ok((origin, u, v, n)) = plane_frame(&d, app.part_editor.plane)
        && let Ok(offset) = app.part_editor.offset.parse::<f64>()
        && offset.is_finite()
    {
        let points: Vec<_> = [(-25.0, -25.0), (25.0, -25.0), (25.0, 25.0), (-25.0, 25.0)]
            .into_iter()
            .map(|(a, b)| project(origin + n * offset + u * a + v * b, center, &app.part_editor, rect, scale).0)
            .collect();
        ui.painter().add(Shape::closed_line(points, Stroke::new(2.0, t.selection)));
    }
    let mut visible_faces = Vec::new();
    for body in parts.iter() {
        let points: Vec<_> = body.vertices.iter().map(|p| project(Vec3::new(p[0], p[1], p[2]), center, &app.part_editor, rect, scale)).collect();
        let edges = body.display_edges();
        let front: Vec<_> = body
            .faces
            .iter()
            .map(|face| {
                let p: Vec<_> = face.iter().filter_map(|n| points.get(*n).map(|p| p.0)).collect();
                p.iter().zip(p.iter().cycle().skip(1)).take(p.len()).map(|(a, b)| a.x * b.y - a.y * b.x).sum::<f32>() < 0.0
            })
            .collect();
        for (i, face) in body.faces.iter().enumerate() {
            let vertices: Vec<_> = face.iter().filter_map(|n| points.get(*n).map(|p| p.0)).collect();
            let area: f32 = vertices.iter().zip(vertices.iter().cycle().skip(1)).take(vertices.len()).map(|(a, b)| a.x * b.y - a.y * b.x).sum();
            if area >= 0.0 {
                continue;
            }
            let Some(normal) = body.face_normal(i) else { continue };
            let selected = match app.part_editor.selected {
                Selected::Body(id) => body.id == id,
                Selected::Face(id, index) if body.id == id => {
                    body.face_normal(index).is_some_and(|n| n.dot(normal) > 1.0 - 1e-7)
                        && body.face_points(index).zip(body.face_points(i)).is_some_and(|(a, b)| (a[0] - b[0]).dot(normal).abs() < 1e-6)
                }
                Selected::Extrude(id) => part.features.iter().find(|f| f.id == id).is_some_and(|f| body.id == f.target.unwrap_or(f.id)),
                _ => false,
            };
            let depth = face.iter().filter_map(|n| points.get(*n)).map(|p| p.1).sum::<f64>() / 4.0;
            let light = Vec3::new(-0.35, -0.45, 0.82).normalized();
            let shade = 0.55 + 0.35 * normal.dot(light).max(0.0) as f32 + if selected { 0.08 } else { 0.0 };
            let fill = Color32::from_rgb(
                (f32::from(t.accent.r()) * shade) as u8,
                (f32::from(t.accent.g()) * shade) as u8,
                (f32::from(t.accent.b()) * shade) as u8,
            );
            let lines = edges
                .iter()
                .filter(|e| e.faces.contains(&i) && (!e.smooth || e.faces.iter().any(|j| !front.get(*j).copied().unwrap_or(false))))
                .map(|e| [project(e.a, center, &app.part_editor, rect, scale).0, project(e.b, center, &app.part_editor, rect, scale).0])
                .collect();
            visible_faces.push(FacePaint {
                depth,
                points: vertices,
                fill,
                edge: if selected { t.selection } else { t.text },
                body: body.id,
                index: i,
                lines,
            });
        }
    }
    visible_faces.sort_by(|a, b| a.depth.total_cmp(&b.depth));
    for face in &visible_faces {
        polygon(ui, face.points.clone(), face.fill);
        for line in &face.lines {
            ui.painter().line_segment(*line, Stroke::new(1.3, face.edge));
        }
    }
    for sketch in &part.sketches {
        if !parts.is_empty() && app.part_editor.selected != Selected::Sketch(sketch.id) {
            continue;
        }
        if let Ok((origin, u, v, _)) = sketch_frame(&d, sketch.id) {
            for handle in sketch.members() {
                let Some(e) = d.entity(handle) else { continue };
                for prim in e.kind.prims() {
                    let mut points = Vec::new();
                    match prim {
                        cadcraft_doc::Prim::Seg(seg) => seg.tessellate(0.05, &mut points),
                        cadcraft_doc::Prim::Circle(c) => c.tessellate((c.radius.abs() / 512.0).max(0.01), &mut points),
                        cadcraft_doc::Prim::Ellipse(e) => e.tessellate((e.major.len() / 512.0).max(0.01), &mut points),
                        cadcraft_doc::Prim::Spline(s) => points = s.tessellate(0.05),
                        _ => continue,
                    }
                    let points =
                        points.iter().take(4096).map(|p| project(origin + u * p.x + v * p.y, center, &app.part_editor, rect, scale).0).collect();
                    ui.painter().add(Shape::line(points, Stroke::new(2.0, t.selection)));
                }
            }
        }
    }
    if !over_cube
        && response.clicked()
        && let Some(pos) = response.interact_pointer_pos()
    {
        let point = Vec2::new(f64::from(pos.x), f64::from(pos.y));
        let hit_plane = pick.iter().rev().find(|(_, points)| {
            cadcraft_geom::point_in_polygon(&points.iter().map(|p| Vec2::new(f64::from(p.x), f64::from(p.y))).collect::<Vec<_>>(), point)
        });
        let hit_face = visible_faces.iter().rev().find(|face| {
            cadcraft_geom::point_in_polygon(&face.points.iter().map(|p| Vec2::new(f64::from(p.x), f64::from(p.y))).collect::<Vec<_>>(), point)
        });
        if matches!(app.part_editor.mode, Mode::Idle | Mode::Placement(_))
            && let Some(face) = hit_face
        {
            match face_reference(&d, face.body, face.index) {
                Ok(reference) => {
                    app.part_editor.selected = Selected::Face(face.body, face.index);
                    app.part_editor.face = Some(reference);
                    app.part_editor.error = None;
                    if let Mode::Placement(extrude) = app.part_editor.mode {
                        if extrude {
                            app.part_editor.target = Some(face.body);
                            app.part_editor.operation = SolidOperation::Add;
                        }
                        begin(app, Mode::Sketch(extrude));
                    }
                }
                Err(e) => app.part_editor.error = Some(e.to_string()),
            }
        } else if matches!(app.part_editor.mode, Mode::Idle | Mode::Placement(_))
            && let Some((id, _)) = hit_plane
        {
            app.part_editor.plane = *id;
            app.part_editor.face = None;
            app.part_editor.selected = Selected::Plane(*id);
            if let Mode::Placement(extrude) = app.part_editor.mode {
                begin(app, Mode::Sketch(extrude));
            }
        }
    }

    if app.part_editor.mode == Mode::Depth {
        viewport_depth(app, ui, &d, center, rect, scale);
    }
    let hint = match app.part_editor.mode {
        Mode::Depth => "Extrude · Solid · Blind — set distance in Properties, then Confirm",
        Mode::Plane => "Datum Plane — set offset in Properties",
        Mode::Placement(_) => "Select a planar solid face or datum plane to begin the sketch",
        _ => "",
    };
    ui.painter().text(rect.left_top() + vec2(16.0, 16.0), Align2::LEFT_TOP, hint, FontId::proportional(15.0), t.text);
    crate::viewcube::show_part(app, ui, rect);
}

struct FacePaint {
    depth: f64,
    points: Vec<Pos2>,
    fill: Color32,
    edge: Color32,
    body: u32,
    index: usize,
    lines: Vec<[Pos2; 2]>,
}

fn viewport_depth(app: &mut CadApp, ui: &mut egui::Ui, d: &Drawing, center: Vec3, rect: Rect, scale: f64) {
    let candidate = app.session.state().ok().and_then(|s| s.part_edit.as_ref()).and_then(|edit| edit.compose(app.session.doc().ok()?).ok());
    let (source, sketch) = if let Some((ref draft, id)) = candidate {
        (draft, id)
    } else {
        let Some(id) = app.part_editor.sketch else { return };
        (d, id)
    };
    let guide = cadcraft_engine::cmd::modeling::extrusion_guide(source, sketch, &app.part_editor.depth, app.part_editor.reverse).ok();
    let Some((start, end)) = guide.or(app.part_editor.guide) else { return };
    if guide.is_some() {
        app.part_editor.guide = guide;
    }
    let axis = (end - start).normalized();
    let Ok((_, u, v, _)) = sketch_frame(source, sketch) else { return };
    let tip = end + axis * (24.0 / scale);
    let base = tip - axis * (18.0 / scale);
    let rim = [u, v, u * (-1.0), v * (-1.0)].map(|dir| base + dir * (7.0 / scale));
    let project_point = |p| project(p, center, &app.part_editor, rect, scale).0;
    let p0 = project_point(start);
    let p1 = project_point(tip);
    let ink = Tokens::get().text;
    ui.painter().line_segment([p0, project_point(base)], Stroke::new(2.5, ink));
    ui.painter().circle_filled(p0, 3.5, ink);
    for (&a, &b) in rim.iter().zip(rim.iter().cycle().skip(1)).take(4) {
        let points = [project_point(a), project_point(b), p1];
        let ab = points[1] - points[0];
        let ac = points[2] - points[0];
        let area = ab.x * ac.y - ab.y * ac.x;
        if area < 0.0 {
            polygon(ui, points.to_vec(), Tokens::get().accent);
            for (&a, &b) in points.iter().zip(points.iter().cycle().skip(1)).take(3) {
                ui.painter().line_segment([a, b], Stroke::new(1.0, ink));
            }
        }
    }
    let pos = egui::pos2(
        (p1.x + 16.0).clamp(rect.left() + 4.0, (rect.right() - 240.0).max(rect.left() + 4.0)),
        (p1.y - 20.0).clamp(rect.top() + 60.0, (rect.bottom() - 70.0).max(rect.top() + 60.0)),
    );
    ui.painter().line_segment([p1, pos + vec2(0.0, 20.0)], Stroke::new(1.0, ink));
    egui::Area::new(egui::Id::unique("part_viewport_depth")).order(egui::Order::Foreground).fixed_pos(pos).show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Depth");
                ui.add(egui::TextEdit::singleline(&mut app.part_editor.depth).desired_width(78.0));
                ui.label(source.part.as_ref().map_or("", |p| p.units.as_str()));
                if ui.button("Reverse").on_hover_text("Reverse extrusion direction").clicked() {
                    reverse_extrusion(app);
                }
            });
        });
    });
}

struct ReferenceLine {
    a: f64,
    b: f64,
    c: f64,
    name: String,
}
#[cfg(test)]
fn reference_lines(d: &Drawing, plane: u32) -> Vec<ReferenceLine> {
    reference_lines_at(d, plane_frame(d, plane).ok(), Some(plane))
}
fn reference_lines_at(d: &Drawing, frame: Option<FaceFrame>, exclude: Option<u32>) -> Vec<ReferenceLine> {
    let mut lines = vec![
        ReferenceLine { a: 1.0, b: 0.0, c: 0.0, name: "Vertical axis".into() },
        ReferenceLine { a: 0.0, b: 1.0, c: 0.0, name: "Horizontal axis".into() },
    ];
    let Some((origin, u, v, _)) = frame else { return lines };
    if let Some(part) = &d.part {
        for reference in &part.planes {
            if Some(reference.id) == exclude {
                continue;
            }
            let Ok((other, _, _, normal)) = plane_frame(d, reference.id) else { continue };
            let (a, b, c) = (normal.dot(u), normal.dot(v), normal.dot(origin - other));
            if a * a + b * b > 1e-12 {
                lines.push(ReferenceLine { a, b, c, name: format!("{} intersection", reference.name) });
            }
        }
    }
    lines
}
fn snap_reference(p: Vec2, lines: &[ReferenceLine], points: &[(Vec2, &str)], tolerance: f64) -> Option<(Vec2, String)> {
    let mut best: Option<(Vec2, String, f64)> = None;
    let mut offer = |q: Vec2, name: &str| {
        let distance = p.dist(q);
        if q.is_finite() && distance <= tolerance && best.as_ref().is_none_or(|(_, _, d)| distance < *d) {
            best = Some((q, name.into(), distance));
        }
    };
    offer(Vec2::ZERO, "Origin");
    for (point, name) in points {
        offer(*point, name);
    }
    for (i, l) in lines.iter().enumerate() {
        for r in lines.iter().skip(i + 1) {
            let det = l.a * r.b - r.a * l.b;
            if det.abs() > 1e-12 {
                offer(Vec2::new((l.b * r.c - r.b * l.c) / det, (r.a * l.c - l.a * r.c) / det), "Datum intersection");
            }
        }
    }
    if let Some((q, name, _)) = best {
        return Some((q, name));
    }
    for l in lines {
        let n = l.a * l.a + l.b * l.b;
        let q = p - Vec2::new(l.a, l.b) * ((l.a * p.x + l.b * p.y + l.c) / n);
        let distance = p.dist(q);
        if distance <= tolerance && best.as_ref().is_none_or(|(_, _, d)| distance < *d) {
            best = Some((q, l.name.clone(), distance));
        }
    }
    best.map(|(p, name, _)| (p, name))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn model_projection_is_right_handed_and_orbit_respects_camera_axes() {
        let mut app = part();
        for horizontal in [false, true] {
            for vertical in [false, true] {
                app.options.orbit_reverse_horizontal = horizontal;
                app.options.orbit_reverse_vertical = vertical;
                let (right, up, eye) = model_axes(&app.part_editor);
                assert!(right.cross(up).dot(eye) > 0.999);
                orbit(&mut app, vec2(1.0, 0.0));
                let change = model_axes(&app.part_editor).2 - eye;
                assert!(change.dot(right) * if horizontal { -1.0 } else { 1.0 } > 0.0);
                let eye = model_axes(&app.part_editor).2;
                orbit(&mut app, vec2(0.0, 1.0));
                let change = model_axes(&app.part_editor).2 - eye;
                assert!(change.dot(up) * if vertical { -1.0 } else { 1.0 } > 0.0);
            }
        }
    }
    #[test]
    fn feature_right_click_menu_deletes_and_undo_restores_the_feature() {
        use egui_kittest::{Harness, kittest::Queryable};
        fn text_rect(shapes: &[egui::epaint::ClippedShape], label: &str) -> Option<Rect> {
            fn find(shape: &Shape, label: &str) -> Option<Rect> {
                match shape {
                    Shape::Text(text) if text.galley.text() == label => Some(text.visual_bounding_rect()),
                    Shape::Vec(shapes) => shapes.iter().find_map(|shape| find(shape, label)),
                    _ => None,
                }
            }
            shapes.iter().find_map(|shape| find(&shape.shape, label))
        }
        let mut app = part();
        app.ui.in_window_menu = true;
        app.run("extrude", json!({"width":40,"height":20,"depth":10})).unwrap();
        app.run("extrude", json!({"width":5,"height":5,"x":100,"depth":5})).unwrap();
        let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).build_ui_state(
            |ui, app: &mut CadApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            app,
        );
        h.run_steps(4);
        let at = text_rect(&h.output().shapes, "Extrude 2 \u{b7} New Body").unwrap().center();
        h.event(egui::Event::PointerMoved(at));
        h.run_steps(1);
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Secondary, pressed, modifiers: egui::Modifiers::default() });
            h.run_steps(1);
        }
        h.run_steps(3);
        assert_eq!(h.state().part_editor.selected, Selected::Extrude(2001));
        if let Some(dir) = std::env::var_os("CADCRAFT_PART_UI_ARTIFACTS") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap();
            h.render().unwrap().save(dir.join("feature-context-menu.png")).unwrap();
        }
        h.get_by_label("Delete Feature").click();
        h.run_steps(3);
        let part = document(h.state()).unwrap().part.as_ref().unwrap();
        assert_eq!(part.features.len(), 1);
        assert_eq!(part.sketches.len(), 2);
        assert_eq!(h.state().part_editor.selected, Selected::Sketch(1001));
        h.state_mut().session.undo().unwrap();
        assert_eq!(document(h.state()).unwrap().part.as_ref().unwrap().features.len(), 2);
        h.state_mut().run("extrude", json!({"width":2,"height":2,"depth":2,"operation":"remove","target":2000})).unwrap();
        h.run_steps(3);
        let at = text_rect(&h.output().shapes, "Extrude 1 \u{b7} New Body").unwrap().center();
        h.event(egui::Event::PointerMoved(at));
        h.run_steps(1);
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Secondary, pressed, modifiers: egui::Modifiers::default() });
            h.run_steps(1);
        }
        h.run_steps(3);
        h.get_by_label("Delete Feature").click();
        h.run_steps(3);
        let error = h.state().part_editor.error.as_ref().unwrap();
        assert!(error.contains("Extrude 3"));
        assert!(text_rect(&h.output().shapes, error).is_some(), "Deletion dependencies must be visible in the viewport");
        assert_eq!(document(h.state()).unwrap().part.as_ref().unwrap().features.len(), 3);
    }
    fn part() -> CadApp {
        let mut app = CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
        app.run("part.new", json!({})).unwrap();
        sync(&mut app);
        app
    }
    #[test]
    fn sketch_stays_on_its_plane_with_rolled_back_context_and_projected_input() {
        let mut app = part();
        app.run("extrude", json!({"width":40,"height":20,"depth":10})).unwrap();
        app.run("part.plane", json!({"parent":1,"offset":"10"})).unwrap();
        app.run("extrude", json!({"plane":4,"width":10,"height":10,"depth":5,"operation":"add","target":2000})).unwrap();
        app.run("extrude", json!({"width":5,"height":5,"x":100,"depth":5})).unwrap();
        let before = app.session.doc().unwrap().clone();
        edit_sketch_ui(&mut app, 1001);
        sync(&mut app);
        let context = cadcraft_engine::cmd::modeling::bodies_before_sketch(document(&app).unwrap(), app.part_editor.sketch).unwrap();
        assert_eq!(context.len(), 1);
        assert!((context[0].volume - 8000.0).abs() < 1e-5);
        assert_eq!(app.part_editor.sketch_yaw, 0.0);
        assert_eq!(app.part_editor.sketch_tilt, 0.0);
        assert!((active_frame(&app).unwrap().0.z - 10.0).abs() < 1e-6);
        app.part_editor.sketch_yaw = 0.7;
        app.part_editor.sketch_tilt = 0.6;
        let xf = sketch_xf(
            &app,
            crate::canvas::Xf { affine: None, rect: Rect::from_min_size(Pos2::ZERO, vec2(900.0, 600.0)), center: Vec2::new(5.0, 5.0), scale: 20.0 },
        );
        for p in [Vec2::ZERO, Vec2::new(15.0, 20.0), Vec2::new(-7.0, 11.0)] {
            assert!(xf.to_world(xf.to_screen(p)).near(p, 1e-5));
        }
        route(&mut app, "ui.part.normal");
        assert_eq!(app.part_editor.sketch_yaw, 0.0);
        assert_eq!(app.part_editor.sketch_tilt, 0.0);
        cancel(&mut app);
        assert_eq!(*app.session.doc().unwrap(), before);
    }
    #[test]
    fn live_sketch_orbit_uses_model_directions_and_saved_preferences() {
        use egui_kittest::Harness;
        fn drag(h: &mut Harness<'_, CadApp>, shift: bool) {
            let at = h.state().canvas.rect.unwrap().center();
            let modifiers = egui::Modifiers { shift, ..Default::default() };
            h.event(egui::Event::ModifiersChanged(modifiers));
            h.event(egui::Event::PointerMoved(at));
            h.run_steps(1);
            h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Middle, pressed: true, modifiers });
            h.run_steps(1);
            for i in 1..=4 {
                h.event(egui::Event::ModifiersChanged(modifiers));
                h.event(egui::Event::PointerMoved(at + vec2(60.0, 40.0) * (i as f32 / 4.0)));
                h.run_steps(1);
            }
            h.event(egui::Event::PointerButton { pos: at + vec2(60.0, 40.0), button: egui::PointerButton::Middle, pressed: false, modifiers });
            h.run_steps(2);
        }
        let mut app = part();
        app.run("extrude", json!({"width":40,"height":20,"depth":10})).unwrap();
        app.run("part.plane", json!({"parent":1,"offset":"10"})).unwrap();
        app.run("extrude", json!({"plane":4,"width":10,"height":10,"depth":5,"operation":"add","target":2000})).unwrap();
        app.run("extrude", json!({"width":5,"height":5,"x":100,"depth":5})).unwrap();
        edit_sketch_ui(&mut app, 1001);
        let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).build_ui_state(
            |ui, app: &mut CadApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            app,
        );
        h.run_steps(3);
        let artifacts = std::env::var_os("CADCRAFT_PART_UI_ARTIFACTS").map(std::path::PathBuf::from);
        if let Some(dir) = &artifacts {
            std::fs::create_dir_all(dir).unwrap();
            h.render().unwrap().save(dir.join("sketch-normal.png")).unwrap();
        }
        assert_eq!(h.state().part_editor.sketch_tilt, 0.0);
        assert_eq!(cached_context(h.state_mut()).len(), 1);
        drag(&mut h, false);
        let (right, up) = sketch_axes(&h.state().part_editor);
        let eye = right.cross(up);
        assert!(eye.x < 0.0, "Default horizontal drag must match the reversed model direction");
        assert!(eye.y > 0.0, "Default vertical drag must move the camera up, as in model view");
        if let Some(dir) = &artifacts {
            h.render().unwrap().save(dir.join("sketch-orbit.png")).unwrap();
        }
        let before = h.state().session.state().unwrap().view().center;
        let angles = (h.state().part_editor.sketch_yaw, h.state().part_editor.sketch_tilt);
        drag(&mut h, true);
        assert_ne!(h.state().session.state().unwrap().view().center, before);
        assert_eq!((h.state().part_editor.sketch_yaw, h.state().part_editor.sketch_tilt), angles);
        route(h.state_mut(), "ui.part.normal");
        h.state_mut().run("options", json!({"orbitReverseHorizontal":false,"orbitReverseVertical":true})).unwrap();
        drag(&mut h, false);
        let (right, up) = sketch_axes(&h.state().part_editor);
        let eye = right.cross(up);
        assert!(eye.x > 0.0);
        assert!(eye.y < 0.0);
        if let Some(dir) = &artifacts {
            h.state_mut().ui.dialog = Some("options:preferences".into());
            h.run_steps(3);
            h.render().unwrap().save(dir.join("orbit-preferences.png")).unwrap();
        }
    }
    #[test]
    fn extrude_first_preview_cancel_and_confirm_are_transactional() {
        let mut app = part();
        let before = app.session.doc().unwrap().clone();
        let undo = app.session.state().unwrap().undo.len();
        assert!(route(&mut app, "extrude"));
        assert_eq!(app.part_editor.mode, Mode::Placement(true));
        app.part_editor.selected = Selected::Plane(1);
        confirm(&mut app);
        assert_eq!(app.part_editor.mode, Mode::Sketch(true));
        app.run("rectang", json!({"p1":[0,0],"p2":[40,20]})).unwrap();
        confirm(&mut app);
        assert_eq!(app.part_editor.mode, Mode::Depth);
        assert_eq!(*document(&app).unwrap(), before);
        cancel(&mut app);
        assert_eq!(*app.session.doc().unwrap(), before);
        assert_eq!(app.session.state().unwrap().undo.len(), undo);
        route(&mut app, "extrude");
        if matches!(app.part_editor.mode, Mode::Placement(_)) {
            app.part_editor.selected = Selected::Plane(1);
            confirm(&mut app);
        }
        app.run("rectang", json!({"p1":[0,0],"p2":[40,20]})).unwrap();
        confirm(&mut app);
        confirm(&mut app);
        assert_eq!(app.part_editor.mode, Mode::Idle);
        assert!((bodies(app.session.doc().unwrap()).unwrap()[0].volume - 8000.0).abs() < 1e-3);
        app.session.undo().unwrap();
        assert_eq!(*app.session.doc().unwrap(), before);
    }

    #[test]
    fn sketch_snaps_to_datum_intersections_and_axis_references() {
        let mut app = part();
        app.run("part.plane", json!({"parent":3,"offset":"5"})).unwrap();
        app.run("part.plane", json!({"parent":2,"offset":"3"})).unwrap();
        let lines = reference_lines(app.session.doc().unwrap(), 1);
        let snap = snap_reference(Vec2::new(5.05, -2.95), &lines, &[], 0.2).unwrap();
        assert!(snap.0.near(Vec2::new(5.0, -3.0), 1e-8));
        assert_eq!(snap.1, "Datum intersection");
        assert!(snap_reference(Vec2::new(5.05, 8.0), &lines, &[], 0.2).unwrap().0.near(Vec2::new(5.0, 8.0), 1e-8));
        assert_eq!(snap_reference(Vec2::new(0.01, 0.01), &lines, &[], 0.2).unwrap().1, "Origin");
        assert!(snap_reference(Vec2::new(20.0, 20.0), &lines, &[], 0.2).is_none());
        let local = reference_lines(app.session.doc().unwrap(), 2);
        assert!((snap_reference(Vec2::new(5.05, 8.0), &local, &[], 0.2).unwrap().0.x - 5.0).abs() < 1e-8);
    }
    #[test]
    fn failed_feature_edit_keeps_the_committed_model() {
        let mut app = part();
        app.run("extrude", json!({"width":40,"height":20,"depth":"10"})).unwrap();
        let before = app.session.doc().unwrap().clone();
        edit_feature(&mut app, 2000);
        app.part_editor.depth = "-1".into();
        confirm(&mut app);
        assert!(app.part_editor.error.is_some());
        assert_eq!(app.part_editor.mode, Mode::Depth);
        assert_eq!(*app.session.doc().unwrap(), before);
        cancel(&mut app);
    }
    #[test]
    fn preselected_face_starts_extrude_with_support_and_target() {
        let mut app = part();
        app.run("extrude", json!({"width":40,"height":20,"depth":10})).unwrap();
        app.part_editor.selected = Selected::Face(2000, 1);
        app.part_editor.face = Some(face_reference(app.session.doc().unwrap(), 2000, 1).unwrap());
        assert!(route(&mut app, "extrude"));
        assert_eq!(app.part_editor.mode, Mode::Sketch(true));
        assert_eq!(app.part_editor.target, Some(2000));
        assert_eq!(app.part_editor.operation, SolidOperation::Add);
        assert!(app.session.state().unwrap().part_edit.as_ref().unwrap().face.is_some());
        app.run("rectang", json!({"p1":[2,2],"p2":[12,12]})).unwrap();
        confirm(&mut app);
        app.part_editor.depth = "5".into();
        confirm(&mut app);
        assert_eq!(app.part_editor.mode, Mode::Idle);
        assert!((bodies(app.session.doc().unwrap()).unwrap()[0].volume - 8500.0).abs() < 1e-5);
    }
    #[test]
    fn three_d_middle_drag_orbits_shift_middle_pans_and_left_drag_does_not_orbit() {
        fn frame(app: &mut CadApp, ctx: &egui::Context, mut events: Vec<egui::Event>, modifiers: egui::Modifiers) {
            events.insert(0, egui::Event::ModifiersChanged(modifiers));
            let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 800.0))), events, ..Default::default() };
            let mut out = ctx.run_ui(raw, |ui| canvas(app, ui));
            out.textures_delta.clear();
        }
        for (button, shift) in [(egui::PointerButton::Primary, false), (egui::PointerButton::Middle, false), (egui::PointerButton::Middle, true)] {
            let mut app = part();
            let ctx = egui::Context::default();
            let modifiers = egui::Modifiers { shift, ..Default::default() };
            let start = egui::pos2(400.0, 300.0);
            let end = start + vec2(55.0, 25.0);
            frame(&mut app, &ctx, vec![egui::Event::PointerMoved(start)], modifiers);
            let before = (app.part_editor.yaw, app.part_editor.pitch, app.part_editor.pan);
            frame(&mut app, &ctx, vec![egui::Event::PointerButton { pos: start, button, pressed: true, modifiers }], modifiers);
            frame(&mut app, &ctx, vec![egui::Event::PointerMoved(end)], modifiers);
            frame(&mut app, &ctx, vec![egui::Event::PointerButton { pos: end, button, pressed: false, modifiers }], modifiers);
            if button == egui::PointerButton::Middle && !shift {
                assert_ne!((app.part_editor.yaw, app.part_editor.pitch), (before.0, before.1));
                assert_eq!(app.part_editor.pan, before.2);
            } else {
                assert_eq!((app.part_editor.yaw, app.part_editor.pitch), (before.0, before.1));
                if shift {
                    assert_ne!(app.part_editor.pan, before.2);
                } else {
                    assert_eq!(app.part_editor.pan, before.2);
                }
            }
        }
    }
}
