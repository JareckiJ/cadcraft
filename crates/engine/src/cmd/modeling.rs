//! Experimental rectangular sketch-to-solid workflow. Source sketches and depth
//! expressions are authoritative; the boundary representation is regenerated.
use cadcraft_constraints as cs;
use cadcraft_doc::{Common, Constraint, ConstraintKind as K, DistAxis, Drawing, EntityKind, GeomRef, LwPolyline, SketchExtrusion, Space, Sub};
use cadcraft_geom::{PolyVertex, Vec2, Vec3};
use serde::Serialize;
use serde_json::{Value, json};

use super::{CommandSpec, bad, f64_req};
use crate::{Result, Session};
mod delete;
mod edges;
mod faces;
mod mesh_solid;
pub mod profile;
pub mod sketch_edit;
mod solid;
pub use edges::DisplayEdge;
pub use faces::{FaceFrame, face_reference, sketch_frame, support_edges, support_frame};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("part.sketch.begin", "Edit Part Sketch", sketch_edit::begin)
            .params("{sketch?: edit ID, plane?: datum ID, face?: {feature,body,face,normal}}")
            .noundo(),
        CommandSpec::new("part.sketch.commit", "Finish Part Sketch", sketch_edit::commit).noundo(),
        CommandSpec::new("part.sketch.cancel", "Cancel Part Sketch", sketch_edit::cancel).noundo(),
        CommandSpec::new("part.feature.delete", "Delete Part Feature", delete::feature).params("{feature: extrusion ID}; retains its source sketch; refuses dependent features"),
        CommandSpec::new("part.new", "New 3D Part", new_part).menu(&["File", "New 3D Part"]).params("{metric?: bool}"),
        CommandSpec::new("part.plane", "Datum Plane", plane).params("{parent?: plane ID, offset: expression, id?: ID to edit}"),
        CommandSpec::new("part.sketch", "Rectangle Sketch", sketch_part)
            .params("{plane,width,height,x?,y?} creates; {sketch: id,width?,height?} edits"),
        CommandSpec::new("extrude", "Extrude", extrude_part).params(
            "{depth, sketch? | plane?,width?,height?,x?,y?, feature?: edit ID, operation?: newBody|add|remove,target?: body ID,reverse?: bool,autoOperation?: bool}",
        ),
        CommandSpec::new("model.block", "Parametric Rectangular Part", create).params(
            "{width: positive number, height: positive number, depth: positive number}; creates one fully constrained XY sketch and extrusion",
        ),
        CommandSpec::new("model.inspect", "Inspect Parametric Solids", inspect).noundo(),
    ]
}

pub fn parameters(d: &Drawing) -> Vec<cs::ParamRow> {
    cs::parameter_table(d)
}
pub fn extrusion_guide(d: &Drawing, sketch_id: u32, expression: &str, reverse: bool) -> Result<(Vec3, Vec3)> {
    let sketch = d.part.as_ref().and_then(|p| p.sketches.iter().find(|s| s.id == sketch_id)).ok_or_else(|| bad("extrude", "Missing sketch"))?;
    let profile = profile::analyze(d, &sketch.members());
    if !profile.ready() {
        return Err(bad("extrude", profile.message()));
    }
    let bounds = cadcraft_geom::Bounds2::from_points(profile.loops.iter().flatten().copied());
    let center = (bounds.min + bounds.max) * 0.5;
    let (origin, u, v, n) = sketch_frame(d, sketch_id)?;
    let depth = cs::env(d).eval(expression).map_err(|e| bad("extrude", e.to_string()))?;
    if !depth.is_finite() || !(1e-8..=1e9).contains(&depth) {
        return Err(bad("extrude", "Depth must be finite and positive"));
    }
    let start = origin + u * center.x + v * center.y;
    Ok((start, start + n * depth * if reverse { -1.0 } else { 1.0 }))
}

pub fn validate(d: &Drawing) -> Result<()> {
    if let Some(part) = &d.part {
        part.validate().map_err(|e| bad("part", e))?;
        for plane in &part.planes {
            plane_frame(d, plane.id)?;
        }
    }
    bodies(d)?;
    if let Some(part) = &d.part {
        for sketch in &part.sketches {
            if sketch.face.is_some() {
                sketch_frame(d, sketch.id)?;
            }
        }
    }
    Ok(())
}

/// Six planar faces enclosing a rectangular prism, with outward winding.
/// This prototype supports rectangular profiles only, not general B-rep operations.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Body {
    pub id: u32,
    pub sketch: String,
    pub vertices: Vec<[f64; 3]>,
    pub faces: Vec<[usize; 4]>,
    #[serde(skip)]
    cells: Vec<solid::Cell>,
    #[serde(skip)]
    planar_faces: Vec<bool>,
    pub width: f64,
    pub height: f64,
    pub depth: f64,
    pub volume: f64,
}

fn primitives(d: &Drawing) -> Result<Vec<Body>> {
    primitives_at(d, None)
}
fn primitives_at(d: &Drawing, frame: Option<FaceFrame>) -> Result<Vec<Body>> {
    if d.parametric.extrusions.len() > 128 {
        return Err(bad("model", "at most 128 extrusion features are supported"));
    }
    let mut env = cs::env(d);
    d.parametric
        .extrusions
        .iter()
        .map(|f| {
            let fail = || bad("model", "extrusion requires an axis-aligned rectangular XY sketch with positive width and height");
            let e = d.entity(f.sketch).ok_or_else(|| bad("model", "extrusion source sketch is missing"))?;
            let EntityKind::LwPolyline(p) = &e.kind else { return Err(fail()) };
            if !p.closed
                || p.vertices.len() != 4
                || !p.elevation.is_finite()
                || e.common.extrusion != cadcraft_geom::Vec3::Z
                || p.vertices.iter().any(|v| !v.p.is_finite() || v.p.x.abs() > 1e9 || v.p.y.abs() > 1e9 || v.bulge != 0.0)
            {
                return Err(fail());
            }
            let points: Vec<Vec2> = p.vertices.iter().map(|v| v.p).collect();
            let [a, b, c, q] = points.as_slice() else { return Err(fail()) };
            let (w, h) = (b.x - a.x, q.y - a.y);
            let tol = 1e-7 * w.abs().max(h.abs()).max(1.0);
            if w <= 1e-8 || h <= 1e-8 || (b.y - a.y).abs() > tol || (c.x - b.x).abs() > tol || (q.y - c.y).abs() > tol || (q.x - a.x).abs() > tol {
                return Err(fail());
            }
            let depth = env.eval(&f.depth).map_err(|e| bad("model", e.to_string()))?;
            let volume = w * h * depth;
            let z = p.elevation;
            if depth <= 1e-8 || depth > 1e9 || !volume.is_finite() || volume <= 0.0 || !(z + depth).is_finite() || z.abs() > 1e9 {
                return Err(bad("model", "extrusion depth and volume must be finite and positive"));
            }
            let mut body = Body {
                id: 0,
                cells: Vec::new(),
                planar_faces: vec![true; 6],
                sketch: f.sketch.hex(),
                vertices: vec![
                    [a.x, a.y, z],
                    [b.x, b.y, z],
                    [c.x, c.y, z],
                    [q.x, q.y, z],
                    [a.x, a.y, z + depth],
                    [b.x, b.y, z + depth],
                    [c.x, c.y, z + depth],
                    [q.x, q.y, z + depth],
                ],
                faces: vec![[3, 2, 1, 0], [4, 5, 6, 7], [0, 1, 5, 4], [1, 2, 6, 5], [2, 3, 7, 6], [3, 0, 4, 7]],
                width: w,
                height: h,
                depth,
                volume,
            };
            if let Some(part) = &d.part {
                let sketch = part.sketches.iter().find(|s| s.profile == f.sketch).ok_or_else(|| bad("model", "Missing part sketch feature"))?;
                let (origin, u, v, n) = frame.unwrap_or(plane_frame(d, sketch.plane)?);
                for p in &mut body.vertices {
                    let world = origin + u * p[0] + v * p[1] + n * p[2];
                    *p = [world.x, world.y, world.z];
                }
            }
            let cell = solid::Cell::from_vertices(&body.vertices);
            if body.vertices.iter().all(|p| (0..3).all(|i| (p[i] - cell.min[i]).abs() < 1e-7 || (p[i] - cell.max[i]).abs() < 1e-7)) {
                body.cells.push(cell);
            }
            Ok(body)
        })
        .collect()
}

pub fn bodies(d: &Drawing) -> Result<Vec<Body>> {
    Ok(regenerate(d, usize::MAX)?.0)
}
pub fn bodies_before_sketch(d: &Drawing, sketch: Option<u32>) -> Result<Vec<Body>> {
    let part = d.part.as_ref().ok_or_else(|| bad("sketch", "Missing part"))?;
    let count = sketch
        .and_then(|id| part.features.iter().position(|f| f.sketch == id).or_else(|| part.features.iter().position(|f| f.sketch >= id)))
        .unwrap_or(part.features.len());
    Ok(regenerate(d, count)?.0)
}
fn regenerate(d: &Drawing, count: usize) -> Result<(Vec<Body>, std::collections::BTreeMap<(u32, u32), Body>)> {
    let Some(part) = &d.part else { return Ok((primitives(d)?, Default::default())) };
    part.validate().map_err(|e| bad("part", e))?;
    let needed: std::collections::BTreeSet<_> = part.sketches.iter().filter_map(|s| s.face.map(|f| (f.feature, f.body))).collect();
    let mut snapshots = std::collections::BTreeMap::new();
    let mut result: Vec<Body> = Vec::new();
    for feature in part.features.iter().take(count) {
        let sketch = part.sketches.iter().find(|s| s.id == feature.sketch).ok_or_else(|| bad("extrude", "Missing sketch"))?;
        let frame = if let Some(reference) = sketch.face {
            let source = snapshots
                .get(&(reference.feature, reference.body))
                .ok_or_else(|| bad("extrude", "Face reference must come from an earlier feature"))?;
            faces::from_reference(source, &reference)?
        } else {
            plane_frame(d, sketch.plane)?
        };
        let mut tool = feature_tool(d, sketch, feature, frame)?;
        tool.id = feature.id;
        if feature.operation == cadcraft_doc::SolidOperation::NewBody {
            result.push(tool);
        } else {
            let target = result.iter_mut().find(|b| Some(b.id) == feature.target).ok_or_else(|| bad("extrude", "Target body no longer exists"))?;
            if target.cells.is_empty() || tool.cells.is_empty() {
                mesh_solid::apply(target, &tool, feature.operation)?;
            } else {
                solid::apply(target, &tool, feature.operation)?;
            }
        }
        let body_id = feature.target.unwrap_or(feature.id);
        if needed.contains(&(feature.id, body_id))
            && let Some(body) = result.iter().find(|b| b.id == body_id)
        {
            snapshots.insert((feature.id, body_id), body.clone());
        }
    }
    Ok((result, snapshots))
}

fn feature_tool(d: &Drawing, sketch: &cadcraft_doc::PartSketch, feature: &cadcraft_doc::PartExtrusion, frame: FaceFrame) -> Result<Body> {
    let mut one = d.clone();
    one.parametric.extrusions = vec![SketchExtrusion { sketch: sketch.profile, depth: feature.depth.clone() }];
    let primitive = if sketch.members().len() == 1 { primitives_at(&one, Some(frame)).ok().and_then(|mut b| b.pop()) } else { None };
    let mut tool = match primitive {
        Some(b) => b,
        None => profile_body(&one, sketch, &feature.depth, frame)?,
    };
    if feature.reverse {
        let n = frame.3;
        for p in &mut tool.vertices {
            let v = Vec3::new(p[0], p[1], p[2]);
            let reflected = v - n * (2.0 * (v - frame.0).dot(n));
            *p = [reflected.x, reflected.y, reflected.z];
        }
        for face in &mut tool.faces {
            face.reverse();
        }
        if !tool.cells.is_empty() {
            tool.cells = vec![solid::Cell::from_vertices(&tool.vertices)];
        }
    }
    Ok(tool)
}

fn profile_body(d: &Drawing, sketch: &cadcraft_doc::PartSketch, depth: &str, frame: FaceFrame) -> Result<Body> {
    let profile = profile::analyze(d, &sketch.members());
    if !profile.ready() {
        return Err(bad("extrude", profile.message()));
    }
    let depth = cs::env(d).eval(depth).map_err(|e| bad("extrude", e.to_string()))?;
    if !depth.is_finite() || !(1e-8..=1e9).contains(&depth) {
        return Err(bad("extrude", "Depth must be finite and positive"));
    }
    let bounds = cadcraft_geom::Bounds2::from_points(profile.loops.iter().flatten().copied());
    // Recognize rectangles assembled with LINE, JOIN or a clockwise polyline too.
    if let [l] = profile.loops.as_slice() {
        let corners = bounds.corners();
        if l.len() == 4 && corners.iter().all(|c| l.iter().any(|p| p.near(*c, 1e-7))) {
            let mut one = d.clone();
            let old = one.model.remove(sketch.profile).ok_or_else(|| bad("extrude", "Missing sketch"))?;
            let mut e = (*old).clone();
            e.kind = EntityKind::LwPolyline(LwPolyline {
                vertices: vec![bounds.min, Vec2::new(bounds.max.x, bounds.min.y), bounds.max, Vec2::new(bounds.min.x, bounds.max.y)]
                    .into_iter()
                    .map(PolyVertex::new)
                    .collect(),
                closed: true,
                const_width: 0.0,
                elevation: 0.0,
                plinegen: false,
            });
            one.model.push(e);
            return primitives_at(&one, Some(frame))?.pop().ok_or_else(|| bad("extrude", "Missing tool"));
        }
    }
    let (origin, u, v, n) = frame;
    let mut body = Body {
        id: 0,
        sketch: sketch.profile.hex(),
        vertices: Vec::new(),
        faces: Vec::new(),
        cells: Vec::new(),
        planar_faces: Vec::new(),
        width: bounds.max.x - bounds.min.x,
        height: bounds.max.y - bounds.min.y,
        depth,
        volume: 0.0,
    };
    let mut face = |points: [Vec2; 4], zs: [f64; 4], planar: bool| {
        let start = body.vertices.len();
        for (p, z) in points.into_iter().zip(zs) {
            let w = origin + u * p.x + v * p.y + n * z;
            body.vertices.push([w.x, w.y, w.z]);
        }
        body.faces.push([start, start + 1, start + 2, start + 3]);
        body.planar_faces.push(planar);
    };
    let mut area = 0.0;
    for tri in profile.triangles.as_chunks::<3>().0 {
        let (Some(&a), Some(&mut_b), Some(&mut_c)) = (tri.first(), tri.get(1), tri.get(2)) else { continue };
        let (mut b, mut c) = (mut_b, mut_c);
        let cross = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
        if cross.abs() <= 1e-10 {
            continue;
        }
        area += cross.abs() * 0.5;
        if cross < 0.0 {
            std::mem::swap(&mut b, &mut c);
        }
        face([a, c, b, b], [0.0; 4], true);
        face([a, b, c, c], [depth; 4], true);
    }
    let lines: Vec<_> = sketch
        .members()
        .iter()
        .filter_map(|h| d.entity(*h))
        .flat_map(|e| e.kind.prims())
        .filter_map(|p| match p {
            cadcraft_doc::Prim::Seg(cadcraft_geom::Segment::Line(line)) => Some(line),
            _ => None,
        })
        .collect();
    for (i, l) in profile.loops.iter().enumerate() {
        let Some(&point) = l.first() else { continue };
        let nesting = profile.loops.iter().enumerate().filter(|(j, other)| i != *j && cadcraft_geom::point_in_polygon(other, point)).count();
        let signed: f64 = l.iter().zip(l.iter().cycle().skip(1)).take(l.len()).map(|(a, b)| a.x * b.y - a.y * b.x).sum();
        let reverse = (signed > 0.0) != (nesting % 2 == 0);
        for (&a, &b) in l.iter().zip(l.iter().cycle().skip(1)).take(l.len()) {
            let (a, b) = if reverse { (b, a) } else { (a, b) };
            let planar = lines.iter().any(|l| l.closest(a).near(a, 1e-7) && l.closest(b).near(b, 1e-7));
            face([a, b, b, a], [0.0, 0.0, depth, depth], planar);
        }
    }
    body.volume = area * depth;
    if !body.volume.is_finite() || body.volume <= 0.0 || body.faces.len() > 16384 {
        return Err(bad("extrude", "Invalid or oversized solid"));
    }
    Ok(body)
}

fn inspect(s: &mut Session, _: &Value) -> Result<Value> {
    Ok(json!({"bodies": bodies(s.doc()?)?}))
}

fn create(s: &mut Session, p: &Value) -> Result<Value> {
    if s.doc()?.part.is_some() {
        return Err(bad("model.block", "Use part.sketch or extrude for a Part document"));
    }
    let (w, h, z) = (f64_req("model.block", p, "width")?, f64_req("model.block", p, "height")?, f64_req("model.block", p, "depth")?);
    if [w, h, z].iter().any(|v| *v <= 1e-8) || !(w * h * z).is_finite() {
        return Err(bad("model.block", "dimensions must be finite and positive"));
    }
    let mut d = s.doc()?.clone();
    if !d.parametric.extrusions.is_empty() || ["width", "height", "depth"].iter().any(|n| cs::name_in_use(&d, n)) {
        return Err(bad("model.block", "use a drawing without existing extrusion features or width/height/depth parameters"));
    }
    for (name, value) in [("width", w), ("height", h), ("depth", z)] {
        cs::set_parameter(&mut d, name, &value.to_string(), &cs::SolveOptions::default()).map_err(|e| bad("model.block", e.to_string()))?;
    }
    let origin = Vec2::new(super::f64_or(p, "x", 0.0), super::f64_or(p, "y", 0.0));
    let vertices =
        [origin, origin + Vec2::new(w, 0.0), origin + Vec2::new(w, h), origin + Vec2::new(0.0, h)].into_iter().map(PolyVertex::new).collect();
    let sketch = d.add(
        &Space::Model,
        Common::default(),
        EntityKind::LwPolyline(LwPolyline { vertices, closed: true, const_width: 0.0, elevation: 0.0, plinegen: false }),
    )?;
    let mut add = |kind, subs: Vec<Sub>, name: &str, expr: &str| {
        let id = cs::next_id(&d);
        d.constraints.push(Constraint {
            id,
            kind,
            refs: subs.into_iter().map(|sub| GeomRef::new(sketch, sub)).collect(),
            name: name.into(),
            expr: expr.into(),
        });
    };
    add(K::Fix, vec![Sub::Vertex(0)], "", "");
    add(K::Horizontal, vec![Sub::Segment(0)], "", "");
    add(K::Vertical, vec![Sub::Segment(1)], "", "");
    add(K::Horizontal, vec![Sub::Segment(2)], "", "");
    add(K::Vertical, vec![Sub::Segment(3)], "", "");
    let width_name = cs::next_name(&d, K::Distance(DistAxis::Horizontal));
    d.constraints.push(Constraint {
        id: cs::next_id(&d),
        kind: K::Distance(DistAxis::Horizontal),
        refs: vec![GeomRef::new(sketch, Sub::Vertex(0)), GeomRef::new(sketch, Sub::Vertex(1))],
        name: width_name,
        expr: "width".into(),
    });
    let height_name = cs::next_name(&d, K::Distance(DistAxis::Vertical));
    d.constraints.push(Constraint {
        id: cs::next_id(&d),
        kind: K::Distance(DistAxis::Vertical),
        refs: vec![GeomRef::new(sketch, Sub::Vertex(0)), GeomRef::new(sketch, Sub::Vertex(3))],
        name: height_name,
        expr: "height".into(),
    });
    cs::solve(&mut d, &cs::SolveOptions::default()).map_err(|e| bad("model.block", e.to_string()))?;
    d.parametric.extrusions.push(SketchExtrusion { sketch, depth: "depth".into() });
    let result = json!({"bodies":bodies(&d)?,"message":"Parametric part created. Open Window > Parametric 3D to edit and orbit."});
    *s.doc_mut()? = d;
    Ok(result)
}

pub fn plane_frame(d: &Drawing, id: u32) -> Result<(Vec3, Vec3, Vec3, Vec3)> {
    let part = d.part.as_ref().ok_or_else(|| bad("plane", "Create a 3D Part first"))?;
    part.validate().map_err(|e| bad("plane", e))?;
    let plane = part.planes.iter().find(|p| p.id == id).ok_or_else(|| bad("plane", "Select a valid plane"))?;
    let x = Vec3::new(1.0, 0.0, 0.0);
    let y = Vec3::new(0.0, 1.0, 0.0);
    let (u, v) = match plane.basis {
        cadcraft_doc::PlaneBasis::XY => (x, y),
        cadcraft_doc::PlaneBasis::XZ => (x, Vec3::Z),
        cadcraft_doc::PlaneBasis::YZ => (y, Vec3::Z),
    };
    let n = u.cross(v);
    let mut origin = Vec3::ZERO;
    let mut current = Some(plane.id);
    let mut env = cs::env(d);
    for _ in 0..=part.planes.len() {
        let Some(id) = current else { return Ok((origin, u, v, n)) };
        let p = part.planes.iter().find(|p| p.id == id).ok_or_else(|| bad("plane", "Missing plane parent"))?;
        let offset = env.eval(&p.offset).map_err(|e| bad("plane", e.to_string()))?;
        origin = origin + n * offset;
        if !origin.is_finite() || origin.len() > 1e9 {
            return Err(bad("plane", "Plane offset is out of range"));
        }
        current = p.parent;
    }
    Err(bad("plane", "Plane dependency cycle"))
}

fn new_part(s: &mut Session, p: &Value) -> Result<Value> {
    let metric = p.get("metric").and_then(Value::as_bool).unwrap_or(true);
    let index = s.new_drawing(metric);
    s.doc_mut()?.part = Some(cadcraft_doc::PartModel::new(metric));
    s.state_mut()?.title = "Part".into();
    Ok(json!({"index":index,"kind":"part","message":"Select a plane, then Sketch or Extrude."}))
}

fn plane(s: &mut Session, p: &Value) -> Result<Value> {
    let mut d = s.doc()?.clone();
    let parent = p.get("parent").and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok()).unwrap_or(1);
    let offset = p.get("offset").and_then(Value::as_str).ok_or_else(|| bad("plane", "An offset expression is required"))?;
    let part = d.part.as_mut().ok_or_else(|| bad("plane", "Create a 3D Part first"))?;
    let id = if let Some(id) = p.get("id").and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok()) {
        if id <= 3 {
            return Err(bad("plane", "Base planes cannot be moved"));
        }
        let plane = part.planes.iter_mut().find(|q| q.id == id).ok_or_else(|| bad("plane", "Missing datum plane"))?;
        plane.offset = offset.into();
        id
    } else {
        if part.planes.len() >= 128 {
            return Err(bad("plane", "Datum plane limit reached"));
        }
        let basis = part.planes.iter().find(|q| q.id == parent).ok_or_else(|| bad("plane", "Missing parent plane"))?.basis;
        let id = part
            .planes
            .iter()
            .map(|q| q.id)
            .max()
            .unwrap_or(3)
            .checked_add(1)
            .filter(|id| *id < 1000)
            .ok_or_else(|| bad("plane", "Datum plane ID limit reached"))?;
        part.planes.push(cadcraft_doc::DatumPlane { id, name: format!("Plane {}", id - 3), basis, parent: Some(parent), offset: offset.into() });
        id
    };
    plane_frame(&d, id)?;
    bodies(&d)?;
    *s.doc_mut()? = d;
    Ok(json!({"plane":id}))
}

fn id_param(p: &Value, key: &str) -> Result<Option<u32>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_u64().and_then(|v| u32::try_from(v).ok()).map(Some).ok_or_else(|| bad("part", format!("Invalid {key} ID"))),
    }
}
fn parameter_name(d: &Drawing, prefix: &str) -> Result<String> {
    for i in 1..=1024 {
        let name = if i == 1 { prefix.into() } else { format!("{prefix}{i}") };
        if !cs::name_in_use(d, &name) {
            return Ok(name);
        }
    }
    Err(bad("part", "Parameter name limit reached"))
}
fn add_sketch(d: &mut Drawing, p: &Value) -> Result<u32> {
    let part = d.part.as_ref().ok_or_else(|| bad("sketch", "Create a 3D Part first"))?;
    if part.sketches.len() >= 128 {
        return Err(bad("sketch", "Sketch limit reached"));
    }
    let id = part
        .sketches
        .iter()
        .map(|s| s.id)
        .max()
        .unwrap_or(999)
        .checked_add(1)
        .filter(|id| *id < 2000)
        .ok_or_else(|| bad("sketch", "Sketch ID limit reached"))?;
    let plane = id_param(p, "plane")?.unwrap_or(1);
    plane_frame(d, plane)?;
    let (w, h) = (f64_req("sketch", p, "width")?, f64_req("sketch", p, "height")?);
    if w <= 1e-8 || h <= 1e-8 || w > 1e9 || h > 1e9 {
        return Err(bad("sketch", "Sketch dimensions must be positive and within range"));
    }
    let origin = Vec2::new(super::f64_or(p, "x", 0.0), super::f64_or(p, "y", 0.0));
    let width = parameter_name(d, "width")?;
    cs::set_parameter(d, &width, &w.to_string(), &cs::SolveOptions::default()).map_err(|e| bad("sketch", e.to_string()))?;
    let height = parameter_name(d, "height")?;
    cs::set_parameter(d, &height, &h.to_string(), &cs::SolveOptions::default()).map_err(|e| bad("sketch", e.to_string()))?;
    let vertices =
        [origin, origin + Vec2::new(w, 0.0), origin + Vec2::new(w, h), origin + Vec2::new(0.0, h)].into_iter().map(PolyVertex::new).collect();
    let profile = d.add(
        &Space::Model,
        Common::default(),
        EntityKind::LwPolyline(LwPolyline { vertices, closed: true, const_width: 0.0, elevation: 0.0, plinegen: false }),
    )?;
    let mut add = |kind, subs: Vec<Sub>, name: String, expr: String| {
        d.constraints.push(Constraint {
            id: cs::next_id(d),
            kind,
            refs: subs.into_iter().map(|sub| GeomRef::new(profile, sub)).collect(),
            name,
            expr,
        });
    };
    add(K::Fix, vec![Sub::Vertex(0)], String::new(), String::new());
    for (segment, kind) in [(0, K::Horizontal), (1, K::Vertical), (2, K::Horizontal), (3, K::Vertical)] {
        add(kind, vec![Sub::Segment(segment)], String::new(), String::new());
    }
    for (axis, vertex, expr) in [(DistAxis::Horizontal, 1, width.clone()), (DistAxis::Vertical, 3, height.clone())] {
        let kind = K::Distance(axis);
        let name = cs::next_name(d, kind);
        d.constraints.push(Constraint {
            id: cs::next_id(d),
            kind,
            refs: vec![GeomRef::new(profile, Sub::Vertex(0)), GeomRef::new(profile, Sub::Vertex(vertex))],
            name,
            expr,
        });
    }
    cs::solve(d, &cs::SolveOptions::default()).map_err(|e| bad("sketch", e.to_string()))?;
    let part = d.part.as_mut().ok_or_else(|| bad("sketch", "Missing part"))?;
    part.sketches.push(cadcraft_doc::PartSketch {
        id,
        name: format!("Sketch {}", id - 999),
        plane,
        face: None,
        profile,
        entities: Vec::new(),
        width_parameter: width,
        height_parameter: height,
    });
    Ok(id)
}
fn edit_sketch(d: &mut Drawing, id: u32, p: &Value) -> Result<()> {
    let sketch =
        d.part.as_ref().and_then(|p| p.sketches.iter().find(|s| s.id == id)).cloned().ok_or_else(|| bad("sketch", "Select a valid sketch"))?;
    for (key, name) in [("width", sketch.width_name()), ("height", sketch.height_name())] {
        if let Some(value) = p.get(key).and_then(Value::as_f64) {
            if value <= 1e-8 || value > 1e9 {
                return Err(bad("sketch", "Dimensions must be positive and within range"));
            }
            cs::set_parameter(d, name, &value.to_string(), &cs::SolveOptions::default()).map_err(|e| bad("sketch", e.to_string()))?;
        }
    }
    Ok(())
}
fn sketch_part(s: &mut Session, p: &Value) -> Result<Value> {
    let mut d = s.doc()?.clone();
    let id = if let Some(id) = id_param(p, "sketch")? {
        edit_sketch(&mut d, id, p)?;
        id
    } else {
        add_sketch(&mut d, p)?
    };
    validate(&d)?;
    *s.doc_mut()? = d;
    Ok(json!({"sketch":id}))
}
fn extrude_part(s: &mut Session, p: &Value) -> Result<Value> {
    use cadcraft_doc::{PartExtrusion, SolidOperation};
    let mut d = s.doc()?.clone();
    let part = d.part.as_ref().ok_or_else(|| bad("extrude", "Create a 3D Part first"))?;
    let edit = id_param(p, "feature")?.or_else(|| {
        // Retain the original one-feature programmatic depth-edit shorthand.
        if p.get("feature").is_none() && p.get("sketch").is_none() && p.get("width").is_none() && part.features.len() == 1 {
            part.features.first().map(|f| f.id)
        } else {
            None
        }
    });
    let old = if let Some(id) = edit {
        Some(part.features.iter().find(|f| f.id == id).cloned().ok_or_else(|| bad("extrude", "Select a valid extrusion"))?)
    } else {
        None
    };
    let sketch = if let Some(f) = &old {
        edit_sketch(&mut d, f.sketch, p)?;
        f.sketch
    } else if let Some(id) = id_param(p, "sketch")? {
        if d.part.as_ref().is_some_and(|p| p.features.iter().any(|f| f.sketch == id)) {
            return Err(bad("extrude", "This sketch is already owned by an extrusion"));
        }
        edit_sketch(&mut d, id, p)?;
        id
    } else if p.get("width").is_none() {
        d.part
            .as_ref()
            .and_then(|p| p.sketches.iter().find(|s| !p.features.iter().any(|f| f.sketch == s.id)))
            .map(|s| s.id)
            .ok_or_else(|| bad("extrude", "Select an unused sketch or create a new one"))?
    } else {
        add_sketch(&mut d, p)?
    };
    let operation = match p.get("operation").and_then(Value::as_str) {
        Some("newBody") => SolidOperation::NewBody,
        Some("add") => SolidOperation::Add,
        Some("remove") => SolidOperation::Remove,
        None => old.as_ref().map_or(SolidOperation::NewBody, |f| f.operation),
        _ => return Err(bad("extrude", "Choose New Body, Add Material or Remove Material")),
    };
    let target = if operation == SolidOperation::NewBody {
        None
    } else {
        id_param(p, "target")?
            .or_else(|| old.as_ref().and_then(|f| f.target))
            .or_else(|| bodies(&d).ok().filter(|b| b.len() == 1).and_then(|b| b.first().map(|b| b.id)))
    };
    let depth_name = old.as_ref().map(|f| f.depth.clone()).unwrap_or(parameter_name(&d, "depth")?);
    let expression = p
        .get("depth")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| p.get("depth").and_then(Value::as_f64).map(|v| v.to_string()))
        .or_else(|| d.parametric.parameters.iter().find(|p| p.name == depth_name).map(|p| p.expr.clone()))
        .unwrap_or_else(|| "10".into());
    cs::set_parameter(&mut d, &depth_name, &expression, &cs::SolveOptions::default()).map_err(|e| bad("extrude", e.to_string()))?;
    let part = d.part.as_mut().ok_or_else(|| bad("extrude", "Missing part"))?;
    let id = if let Some(old) = old {
        let feature = part.features.iter_mut().find(|f| f.id == old.id).ok_or_else(|| bad("extrude", "Missing feature"))?;
        feature.operation = operation;
        feature.target = target;
        feature.reverse = p.get("reverse").and_then(Value::as_bool).unwrap_or(feature.reverse);
        old.id
    } else {
        if part.features.len() >= 128 {
            return Err(bad("extrude", "Feature limit reached"));
        }
        let id = part
            .features
            .iter()
            .map(|f| f.id)
            .max()
            .unwrap_or(1999)
            .checked_add(1)
            .filter(|id| *id < 3000)
            .ok_or_else(|| bad("extrude", "Feature ID limit reached"))?;
        part.features.push(PartExtrusion {
            id,
            name: format!("Extrude {}", id - 1999),
            sketch,
            depth: depth_name,
            operation,
            target,
            reverse: p.get("reverse").and_then(Value::as_bool).unwrap_or(false),
        });
        id
    };
    if p.get("autoOperation").and_then(Value::as_bool) == Some(true) {
        let index = d.part.as_ref().and_then(|p| p.features.iter().position(|f| f.id == id)).ok_or_else(|| bad("extrude", "Missing feature"))?;
        let feature = d.part.as_ref().and_then(|p| p.features.get(index)).cloned().ok_or_else(|| bad("extrude", "Missing feature"))?;
        if feature.operation != SolidOperation::NewBody {
            let prior = regenerate(&d, index)?.0;
            let target =
                prior.iter().find(|b| Some(b.id) == feature.target).ok_or_else(|| bad("extrude", "Choose a target body from before this feature"))?;
            let sketch =
                d.part.as_ref().and_then(|p| p.sketches.iter().find(|s| s.id == feature.sketch)).ok_or_else(|| bad("extrude", "Missing sketch"))?;
            let tool = feature_tool(&d, sketch, &feature, sketch_frame(&d, sketch.id)?)?;
            let operation =
                if target.cells.is_empty() || tool.cells.is_empty() { mesh_solid::suggest(target, &tool)? } else { solid::suggest(target, &tool)? };
            if let Some(f) = d.part.as_mut().and_then(|p| p.features.get_mut(index)) {
                f.operation = operation;
            }
        }
    }
    validate(&d)?;
    let operation = d.part.as_ref().and_then(|p| p.features.iter().find(|f| f.id == id)).map(|f| f.operation);
    *s.doc_mut()? = d;
    Ok(json!({"feature":id,"sketch":sketch,"operation":operation,"message":"Extrusion confirmed"}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_part_planes_blind_depth_and_history() {
        let mut s = Session::new();
        s.execute("part.new", &json!({"metric":true})).unwrap();
        let plane = s.execute("part.plane", &json!({"parent":1,"offset":"5"})).unwrap()["plane"].as_u64().unwrap();
        let before = s.doc().unwrap().clone();
        s.execute("extrude", &json!({"plane":plane,"width":40,"height":20,"depth":"10"})).unwrap();
        let b = bodies(s.doc().unwrap()).unwrap().remove(0);
        assert!((b.volume - 8000.0).abs() < 1e-3);
        assert_eq!(b.vertices[0][2], 5.0);
        assert_eq!(b.vertices[4][2], 15.0);
        s.undo().unwrap();
        assert_eq!(*s.doc().unwrap(), before);
        s.redo().unwrap();
        s.execute("part.plane", &json!({"id":plane,"offset":"8"})).unwrap();
        assert_eq!(bodies(s.doc().unwrap()).unwrap()[0].vertices[0][2], 8.0);
        let bytes = cadcraft_io::write(s.doc().unwrap(), "test.cadcraft").unwrap();
        let mut reopened = cadcraft_io::read(&bytes, "test.cadcraft").unwrap();
        assert_eq!(reopened.part, s.doc().unwrap().part);
        cs::set_parameter(&mut reopened, "depth", "15", &cs::SolveOptions::default()).unwrap();
        assert!((bodies(&reopened).unwrap()[0].volume - 12000.0).abs() < 1e-3);
        let saved = s.doc().unwrap().clone();
        assert!(s.execute("extrude", &json!({"depth":"-2"})).is_err());
        assert!(s.execute("extrude", &json!({"depth":"1e300"})).is_err());
        assert_eq!(*s.doc().unwrap(), saved);
        assert!(s.execute("part.plane", &json!({"id":plane,"offset":"1/0"})).is_err());
        assert_eq!(*s.doc().unwrap(), saved);
        assert!(s.execute("part.plane", &json!({"id":1,"offset":"8"})).is_err());
    }

    #[test]
    fn multiple_sketches_add_cut_independent_bodies_and_targeted_edits() {
        let mut s = Session::new();
        s.execute("part.new", &json!({})).unwrap();
        s.execute("extrude", &json!({"width":40,"height":20,"depth":"10"})).unwrap();
        s.execute("part.plane", &json!({"parent":1,"offset":"10"})).unwrap();
        s.execute("extrude", &json!({"plane":4,"width":10,"height":10,"depth":"5","operation":"add","target":2000})).unwrap();
        assert!((bodies(s.doc().unwrap()).unwrap()[0].volume - 8500.0).abs() < 1e-5);
        s.execute("extrude", &json!({"width":5,"height":5,"x":2,"y":2,"depth":"20","operation":"remove","target":2000})).unwrap();
        let body = bodies(s.doc().unwrap()).unwrap().remove(0);
        assert!((body.volume - 8125.0).abs() < 1e-5);
        let mut mesh_volume = 0.0;
        for f in &body.faces {
            for tri in [[f[0], f[1], f[2]], [f[0], f[2], f[3]]] {
                let [a, b, c] = tri.map(|i| {
                    let p = body.vertices[i];
                    Vec3::new(p[0], p[1], p[2])
                });
                mesh_volume += a.dot(b.cross(c)) / 6.0;
            }
        }
        assert!((mesh_volume - body.volume).abs() < 1e-5, "external faces must enclose the analytic solid");
        s.execute("part.sketch", &json!({"width":5,"height":5,"x":100})).unwrap();
        s.execute("extrude", &json!({"sketch":1003,"depth":"5"})).unwrap();
        assert_eq!(bodies(s.doc().unwrap()).unwrap().len(), 2);
        s.execute("part.sketch", &json!({"sketch":1000,"width":60})).unwrap();
        let parts = bodies(s.doc().unwrap()).unwrap();
        assert!((parts[0].volume - 12125.0).abs() < 1e-5);
        assert!((parts[1].volume - 125.0).abs() < 1e-5);
        let before = s.doc().unwrap().clone();
        assert!(s.execute("extrude", &json!({"width":3,"height":3,"x":500,"depth":"5","operation":"remove","target":2000})).is_err());
        assert_eq!(*s.doc().unwrap(), before);
        s.execute("extrude", &json!({"feature":2002,"width":2})).unwrap();
        assert!((bodies(s.doc().unwrap()).unwrap()[0].volume - 12350.0).abs() < 1e-4);
        s.undo().unwrap();
        assert_eq!(*s.doc().unwrap(), before);
        s.redo().unwrap();
        let bytes = cadcraft_io::write(s.doc().unwrap(), "multiple.cadcraft").unwrap();
        let reopened = cadcraft_io::read(&bytes, "multiple.cadcraft").unwrap();
        assert_eq!(reopened.part, s.doc().unwrap().part);
        assert!((bodies(&reopened).unwrap()[0].volume - 12350.0).abs() < 1e-4);
    }

    #[test]
    fn sketches_on_all_base_planes_and_offset_parameter_dependencies() {
        for plane in 1..=3 {
            let mut s = Session::new();
            s.execute("part.new", &json!({})).unwrap();
            s.execute("part.sketch", &json!({"plane":plane,"width":40,"height":20})).unwrap();
            assert!(bodies(s.doc().unwrap()).unwrap().is_empty());
            s.execute("extrude", &json!({"depth":"10"})).unwrap();
            let body = bodies(s.doc().unwrap()).unwrap().remove(0);
            assert!((body.volume - 8000.0).abs() < 1e-3);
            let span = Vec3::new(
                body.vertices[4][0] - body.vertices[0][0],
                body.vertices[4][1] - body.vertices[0][1],
                body.vertices[4][2] - body.vertices[0][2],
            );
            assert!((span.len() - 10.0).abs() < 1e-6);
        }
        let mut s = Session::new();
        s.execute("part.new", &json!({})).unwrap();
        s.execute("parameters", &json!({"name":"offset","expr":"5"})).unwrap();
        s.execute("part.plane", &json!({"parent":1,"offset":"offset"})).unwrap();
        s.execute("parameters", &json!({"name":"offset","rename":"placement"})).unwrap();
        assert_eq!(plane_frame(s.doc().unwrap(), 4).unwrap().0.z, 5.0);
        let before = s.doc().unwrap().clone();
        assert!(s.execute("parameters", &json!({"delete":"placement"})).is_err());
        assert_eq!(*s.doc().unwrap(), before);
    }
    #[test]
    fn sketch_extrusion_regenerates_and_roundtrips() {
        let mut s = Session::new();
        s.execute("model.block", &json!({"width":40,"height":20,"depth":10})).unwrap();
        let volume = |s: &Session| bodies(s.doc().unwrap()).unwrap()[0].volume;
        assert!((volume(&s) - 8000.0).abs() < 1e-4);
        assert_eq!(cs::dof(s.doc().unwrap(), &[]).unwrap().dof, Some(0));
        s.execute("parameters", &json!({"name":"width","expr":"60"})).unwrap();
        assert!((volume(&s) - 12000.0).abs() < 1e-3);
        s.undo().unwrap();
        assert!((volume(&s) - 8000.0).abs() < 1e-3);
        s.redo().unwrap();
        assert!((volume(&s) - 12000.0).abs() < 1e-3);
        let before = s.doc().unwrap().clone();
        assert!(s.execute("parameters", &json!({"name":"depth","expr":"-2"})).is_err());
        assert_eq!(*s.doc().unwrap(), before);
        let bytes = cadcraft_io::write(s.doc().unwrap(), "part.dxf").unwrap();
        let reopened = cadcraft_io::read(&bytes, "part.dxf").unwrap();
        assert!((bodies(&reopened).unwrap()[0].volume - 12000.0).abs() < 1e-3);
        assert_eq!(reopened.parametric.extrusions, before.parametric.extrusions);
        let mut reopened = reopened;
        cs::set_parameter(&mut reopened, "width", "80", &cs::SolveOptions::default()).unwrap();
        assert!((bodies(&reopened).unwrap()[0].volume - 16000.0).abs() < 1e-3);
        assert!(s.execute("parameters", &json!({"delete":"depth"})).is_err());
        assert_eq!(*s.doc().unwrap(), before);
    }

    #[test]
    fn dependencies_and_invalid_dimensions_are_transactional() {
        let mut s = Session::new();
        s.execute("model.block", &json!({"width":40,"height":20,"depth":10})).unwrap();
        s.execute("parameters", &json!({"name":"depth","expr":"height/2"})).unwrap();
        s.execute("parameters", &json!({"name":"height","expr":"30"})).unwrap();
        let body = bodies(s.doc().unwrap()).unwrap().remove(0);
        assert!((body.volume - 18000.0).abs() < 1e-3);
        s.execute("parameters", &json!({"name":"depth","rename":"thickness"})).unwrap();
        assert_eq!(s.doc().unwrap().parametric.extrusions[0].depth, "thickness");
        let before = s.doc().unwrap().clone();
        for (name, expr) in [("thickness", "0"), ("thickness", "1/0"), ("thickness", "thickness+1"), ("width", "0"), ("width", "-2")] {
            assert!(s.execute("parameters", &json!({"name":name,"expr":expr})).is_err(), "{name}={expr}");
            assert_eq!(*s.doc().unwrap(), before);
        }
        // Face winding independently integrates to the analytic volume.
        let mut signed_volume = 0.0;
        for f in body.faces {
            for tri in [[f[0], f[1], f[2]], [f[0], f[2], f[3]]] {
                let [a, b, c] = tri.map(|i| body.vertices[i]);
                signed_volume += (a[0] * (b[1] * c[2] - b[2] * c[1]) + a[1] * (b[2] * c[0] - b[0] * c[2]) + a[2] * (b[0] * c[1] - b[1] * c[0])) / 6.0;
            }
        }
        assert!((signed_volume - body.volume).abs() < 1e-3);
    }
}
