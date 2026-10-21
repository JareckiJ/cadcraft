//! 3D modelling, first step (gaps.md F24): BOX, CYLINDER and EXTRUDE.
//!
//! There is no solid or mesh object yet, so each solid is a group of 3DFACE objects (one undo
//! step, one group that picks and erases as a whole). The faces come from
//! [`cadcraft_geom::mesh`]; 3DFACE already saves and opens in DXF and DWG, and any 3D view shows
//! them. Out of scope: booleans, tapered or path extrusions, true solids.

use cadcraft_doc::{Common, Drawing, Entity, EntityKind, Face3d, Handle};
use cadcraft_geom::mesh::{self, Face, MAX_PROFILE};
use cadcraft_geom::{Polyline, TAU, Vec2, Vec3, point_in_polygon};
use serde_json::{Value, json};

use super::helpers::*;
use super::machines::{SelOutcome, SelectPhase, number};
use super::*;
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

/// Most faces one command creates.
const MAX_FACES: usize = 50_000;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("box", "Box", run_box)
            .menu(&["Draw", "3D Modeling", "Box"])
            .params("{corner: [x,y,z?], corner2?: [x,y] | {length, width}, height} → {handles, group, faces}. A negative size goes the other way; no size is zero. The box is a group of 3D faces.")
            .interactive(|_| Ok(Box::new(BoxM::new(false)))),
        CommandSpec::new("wedge", "Wedge", run_wedge)
            .menu(&["Draw", "3D Modeling", "Wedge"])
            .params("{corner: [x,y,z?], corner2?: [x,y] | {length, width}, height} → {handles, group, faces}. A box cut in half along its diagonal: the tall end is at the corner's X and it slopes down to the other end. A negative size goes the other way.")
            .interactive(|_| Ok(Box::new(BoxM::new(true)))),
        CommandSpec::new("cylinder", "Cylinder", run_cylinder)
            .menu(&["Draw", "3D Modeling", "Cylinder"])
            .params("{center: [x,y,z?], radius, height, tolerance? (chord error, default radius/1000) | segments? (3..1024)} → {handles, group, faces}. The base is at the centre's Z; a negative height goes down.")
            .interactive(|_| Ok(Box::new(RadialM::new(Radial::Cylinder)))),
        CommandSpec::new("cone", "Cone", run_cone)
            .menu(&["Draw", "3D Modeling", "Cone"])
            .params("{center: [x,y,z?], radius, height, topRadius? (default 0: a point; above 0: a truncated cone), tolerance? | segments? (3..1024)} → {handles, group, faces}. The base is at the centre's Z; a negative height goes down.")
            .interactive(|_| Ok(Box::new(RadialM::new(Radial::Cone)))),
        CommandSpec::new("pyramid", "Pyramid", run_pyramid)
            .menu(&["Draw", "3D Modeling", "Pyramid"])
            .params("{center: [x,y,z?], radius (to the corners), height, sides? (3..1024, default 4), topRadius? (default 0: a point)} → {handles, group, faces}. The first corner is on +X; the base is at the centre's Z; a negative height goes down.")
            .interactive(|_| Ok(Box::new(RadialM::new(Radial::Pyramid)))),
        CommandSpec::new("sphere", "Sphere", run_sphere)
            .menu(&["Draw", "3D Modeling", "Sphere"])
            .params("{center: [x,y,z?], radius, tolerance? (chord error, default radius/1000) | segments? (4..1024 slices; half as many rings)} → {handles, group, faces}. Poles on the Z axis; the face count is capped.")
            .interactive(|_| Ok(Box::new(RadialM::new(Radial::Sphere)))),
        CommandSpec::new("torus", "Torus", run_torus)
            .menu(&["Draw", "3D Modeling", "Torus"])
            .params("{center: [x,y,z?], radius (centre to the middle of the tube), tubeRadius (smaller than radius), tolerance? | segments? (around), tubeSegments? (around the tube)} → {handles, group, faces}. In the XY plane.")
            .interactive(|_| Ok(Box::new(RadialM::new(Radial::Torus)))),
        CommandSpec::new("extrude", "Extrude", run_extrude)
            .menu(&["Draw", "3D Modeling", "Extrude"])
            .params("{handles? (default the selection: closed polylines and circles), holes? [handles] (closed polylines and circles fully inside one profile, cut through it; touching, overlapping or nested holes are refused), height, tolerance? (chord error for arcs, default 1/1000 of the profile), erase?: bool (default false; also erases the holes)} → {handles, groups, faces}. Extrudes along +Z from the profile's elevation, as one group of 3D faces per profile.")
            .interactive(|_| Ok(Box::new(ExtrudeM::default()))),
        CommandSpec::new("3dface", "3D Face", run_3dface)
            .menu(&["Draw", "3D Modeling", "Meshes", "3D Face"])
            .alias(&["3f"])
            .params("{points: [[x,y,z?], 3 or 4 points], invisible?: [edge numbers 1..4]} → {handle}. Edge 1 runs from the first point to the second; edge 4 closes a quad, and a triangle's closing edge is 3 or 4. The points need not be coplanar, but not all collinear.")
            .interactive(|_| Ok(Box::new(FaceM::default()))),
    ]
}

fn err(cmd: &str, m: impl std::fmt::Display) -> EngineError {
    bad(cmd, m.to_string())
}

fn point3(p: &Value, key: &str) -> Option<Vec3> {
    let a = p.get(key)?.as_array()?;
    let v = Vec3::new(a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2).and_then(Value::as_f64).unwrap_or(0.0));
    v.is_finite().then_some(v)
}

fn number_req(cmd: &str, p: &Value, key: &str) -> Result<f64> {
    f64_req(cmd, p, key)
}

/// The new faces as 3DFACE objects in one group. `src` gives them the properties of the profile
/// they came from (layer, colour, linetype…); without it they take the current ones.
fn add_solid(s: &mut Session, faces: &[Face], src: Option<&Common>, what: &str) -> Result<(Vec<Handle>, String)> {
    if faces.len() > MAX_FACES {
        return Err(err(what, "the solid would have too many faces"));
    }
    let mut hs = Vec::with_capacity(faces.len());
    for f in faces {
        let h = s.add_entity(EntityKind::Face3d(Face3d { corners: f.corners, hidden_edges: f.hidden }))?;
        if let Some(c) = src {
            let c = c.clone();
            s.doc_mut()?.modify_entity(h, move |e| {
                e.common.layer = c.layer;
                e.common.color = c.color;
                e.common.linetype = c.linetype;
                e.common.lineweight = c.lineweight;
                e.common.ltscale = c.ltscale;
                e.common.transparency = c.transparency;
                e.common.plot_style = c.plot_style;
            })?;
        }
        hs.push(h);
    }
    let name = super::groups::create(s, None, &format!("3D solid ({what})"), &hs, true)?;
    Ok((hs, name))
}

fn hexes(hs: &[Handle]) -> Vec<String> {
    hs.iter().map(|h| h.hex()).collect()
}

// ---------------------------------------------------------------------------------- BOX, WEDGE

fn solid_json(s: &mut Session, faces: std::result::Result<Vec<Face>, mesh::MeshError>, cmd: &str, label: &str) -> Result<Value> {
    let faces = faces.map_err(|e| err(cmd, e))?;
    let (hs, group) = add_solid(s, &faces, None, cmd)?;
    Ok(
        json!({ "handles": hexes(&hs), "group": group, "faces": hs.len(), "message": format!("{label} created ({} faces, group {group}).", hs.len()) }),
    )
}

fn block_at(s: &mut Session, corner: Vec3, size: Vec3, wedge: bool) -> Result<Value> {
    if wedge { solid_json(s, mesh::wedge(corner, size), "wedge", "Wedge") } else { solid_json(s, mesh::box_faces(corner, size), "box", "Box") }
}

fn run_block(s: &mut Session, p: &Value, cmd: &str) -> Result<Value> {
    let corner = point3(p, "corner").ok_or_else(|| err(cmd, "`corner` ([x, y, z?]) is required"))?;
    let height = number_req(cmd, p, "height")?;
    let (dx, dy) = if let Some(c2) = p.get("corner2").and_then(point_value) {
        (c2.x - corner.x, c2.y - corner.y)
    } else {
        (number_req(cmd, p, "length").map_err(|_| err(cmd, "`corner2` or `length` and `width` are required"))?, number_req(cmd, p, "width")?)
    };
    block_at(s, corner, Vec3::new(dx, dy, height), cmd == "wedge")
}

fn run_box(s: &mut Session, p: &Value) -> Result<Value> {
    run_block(s, p, "box")
}

fn run_wedge(s: &mut Session, p: &Value) -> Result<Value> {
    run_block(s, p, "wedge")
}

struct BoxM {
    wedge: bool,
    first: Option<Vec2>,
    second: Option<Vec2>,
}

impl BoxM {
    fn new(wedge: bool) -> Self {
        BoxM { wedge, first: None, second: None }
    }
}

impl Interactive for BoxM {
    fn name(&self) -> &'static str {
        if self.wedge { "WEDGE" } else { "BOX" }
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match (self.first, self.second) {
            (None, _) => Prompt::new("Specify first corner", Accept::POINT),
            (Some(a), None) => Prompt::new("Specify other corner", Accept::POINT).base(a),
            (Some(a), Some(_)) => Prompt::new("Specify height", Accept::POINT_OR_NUMBER).base(a),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match (self.first, self.second, i) {
            (None, _, Input::Point(p)) => self.first = Some(p),
            (Some(a), None, Input::Point(p)) => {
                if (p.x - a.x).abs() < 1e-12 || (p.y - a.y).abs() < 1e-12 {
                    return Err(EngineError::Other("The corners must differ in both X and Y.".into()));
                }
                self.second = Some(p);
            }
            (Some(a), Some(b), i) => {
                let h = match i {
                    Input::Text(t) => number(&t),
                    Input::Point(p) => Some(a.dist(p)),
                    _ => None,
                }
                .filter(|h| h.is_finite() && *h != 0.0)
                .ok_or_else(|| EngineError::Other("Requires a non-zero height.".into()))?;
                block_at(s, a.to3(0.0), Vec3::new(b.x - a.x, b.y - a.y, h), self.wedge)?;
                return Ok(Step::Done);
            }
            _ => {}
        }
        Ok(Step::Continue)
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        match (self.first, self.second) {
            (Some(a), None) => vec![lwpoly(rect_vertices(a, c), true)],
            (Some(a), Some(b)) => vec![lwpoly(rect_vertices(a, b), true)],
            _ => Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------------- CYLINDER, CONE, PYRAMID, SPHERE, TORUS

/// Sides around a round shape: `key` when given (a whole number the geometry then checks), else
/// from the chord `tolerance` (default radius/1000), at most `cap`.
fn segs(cmd: &str, p: &Value, key: &str, radius: f64, cap: usize) -> Result<usize> {
    match p.get(key) {
        None | Some(Value::Null) => {
            let tol = size_param(cmd, p, "tolerance", false)?.unwrap_or(radius.abs() * 1e-3);
            Ok(mesh::circle_segments(radius.abs(), tol).min(cap))
        }
        Some(v) => v.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or_else(|| err(cmd, format!("`{key}` must be a whole number"))),
    }
}

fn centre_radius(p: &Value, cmd: &str) -> Result<(Vec3, f64)> {
    Ok((point3(p, "center").ok_or_else(|| err(cmd, "`center` ([x, y, z?]) is required"))?, number_req(cmd, p, "radius")?))
}

fn cylinder_at(s: &mut Session, center: Vec3, radius: f64, height: f64, segments: usize) -> Result<Value> {
    solid_json(s, mesh::cylinder(center, radius, height, segments), "cylinder", "Cylinder")
}

fn run_cylinder(s: &mut Session, p: &Value) -> Result<Value> {
    let (center, radius) = centre_radius(p, "cylinder")?;
    let height = number_req("cylinder", p, "height")?;
    let n = segs("cylinder", p, "segments", radius, mesh::MAX_SEGMENTS)?;
    cylinder_at(s, center, radius, height, n)
}

fn run_cone(s: &mut Session, p: &Value) -> Result<Value> {
    let (center, radius) = centre_radius(p, "cone")?;
    let height = number_req("cone", p, "height")?;
    let top = size_param("cone", p, "topRadius", true)?.unwrap_or(0.0);
    let n = segs("cone", p, "segments", radius, mesh::MAX_SEGMENTS)?;
    solid_json(s, mesh::frustum(center, radius, top, height, n), "cone", "Cone")
}

fn run_pyramid(s: &mut Session, p: &Value) -> Result<Value> {
    let (center, radius) = centre_radius(p, "pyramid")?;
    let height = number_req("pyramid", p, "height")?;
    let top = size_param("pyramid", p, "topRadius", true)?.unwrap_or(0.0);
    let sides = match p.get("sides") {
        None | Some(Value::Null) => 4,
        Some(v) => v.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or_else(|| err("pyramid", "`sides` must be a whole number from 3 to 1024"))?,
    };
    solid_json(s, mesh::frustum(center, radius, top, height, sides), "pyramid", "Pyramid")
}

fn run_sphere(s: &mut Session, p: &Value) -> Result<Value> {
    let (center, radius) = centre_radius(p, "sphere")?;
    let n = segs("sphere", p, "segments", radius, 256)?;
    solid_json(s, mesh::sphere(center, radius, n), "sphere", "Sphere")
}

fn run_torus(s: &mut Session, p: &Value) -> Result<Value> {
    let (center, radius) = centre_radius(p, "torus")?;
    let tube = number_req("torus", p, "tubeRadius")?;
    let n = segs("torus", p, "segments", radius + tube.abs(), 256)?;
    let k = segs("torus", p, "tubeSegments", tube, 128)?;
    solid_json(s, mesh::torus(center, radius, tube, n, k), "torus", "Torus")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Radial {
    Cylinder,
    Cone,
    Pyramid,
    Sphere,
    Torus,
}

struct RadialM {
    kind: Radial,
    center: Option<Vec2>,
    radius: Option<f64>,
    sides: usize,
    asking_sides: bool,
}

impl RadialM {
    fn new(kind: Radial) -> Self {
        RadialM { kind, center: None, radius: None, sides: 4, asking_sides: false }
    }
    fn has_height(&self) -> bool {
        matches!(self.kind, Radial::Cylinder | Radial::Cone | Radial::Pyramid)
    }
    /// Make the solid; `last` is the height, or the tube radius of a torus.
    fn finish(&self, s: &mut Session, c: Vec2, r: f64, last: f64) -> Result<Step> {
        let ctr = c.to3(0.0);
        let n = mesh::circle_segments(r, r * 1e-3);
        match self.kind {
            Radial::Cylinder => cylinder_at(s, ctr, r, last, n)?,
            Radial::Cone => solid_json(s, mesh::frustum(ctr, r, 0.0, last, n), "cone", "Cone")?,
            Radial::Pyramid => solid_json(s, mesh::frustum(ctr, r, 0.0, last, self.sides), "pyramid", "Pyramid")?,
            Radial::Sphere => solid_json(s, mesh::sphere(ctr, r, n.min(256)), "sphere", "Sphere")?,
            Radial::Torus => {
                let ring = mesh::circle_segments(r + last, (r + last) * 1e-3).min(256);
                solid_json(s, mesh::torus(ctr, r, last, ring, mesh::circle_segments(last, last * 1e-3).min(128)), "torus", "Torus")?
            }
        };
        Ok(Step::Done)
    }
}

impl Interactive for RadialM {
    fn name(&self) -> &'static str {
        match self.kind {
            Radial::Cylinder => "CYLINDER",
            Radial::Cone => "CONE",
            Radial::Pyramid => "PYRAMID",
            Radial::Sphere => "SPHERE",
            Radial::Torus => "TORUS",
        }
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.asking_sides {
            return Prompt::new("Enter number of sides", Accept::NUMBER).default(self.sides.to_string());
        }
        let on_base = self.has_height();
        match (self.center, self.radius) {
            (None, _) => Prompt::new(if on_base { "Specify center point of base" } else { "Specify center point" }, Accept::POINT),
            (Some(c), None) => {
                let msg = if on_base { "Specify base radius" } else { "Specify radius" };
                if self.kind == Radial::Pyramid {
                    Prompt::new("Specify base radius or", Accept::POINT_OR_NUMBER).kw(&["Sides"]).base(c)
                } else {
                    Prompt::new(msg, Accept::POINT_OR_NUMBER).base(c)
                }
            }
            (Some(c), Some(_)) => Prompt::new(if on_base { "Specify height" } else { "Specify tube radius" }, Accept::POINT_OR_NUMBER).base(c),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.asking_sides {
            if let Input::Text(t) = &i {
                let n = number(t)
                    .filter(|n| n.fract() == 0.0 && (3.0..=1024.0).contains(n))
                    .ok_or_else(|| EngineError::Other("Requires a whole number from 3 to 1024.".into()))?;
                self.sides = n as usize;
            }
            self.asking_sides = false;
            return Ok(Step::Continue);
        }
        let dist = |i: &Input, c: Vec2| match i {
            Input::Text(t) => number(t),
            Input::Point(p) => Some(c.dist(*p)),
            _ => None,
        };
        match (self.center, self.radius) {
            (None, _) => {
                if let Input::Point(p) = i {
                    self.center = Some(p);
                }
            }
            (Some(c), None) => {
                if let Input::Keyword(k) = &i
                    && k == "Sides"
                {
                    self.asking_sides = true;
                    return Ok(Step::Continue);
                }
                let r = dist(&i, c).filter(|r| r.is_finite() && *r > 0.0).ok_or_else(|| EngineError::Other("Requires a positive radius.".into()))?;
                self.radius = Some(r);
                if self.kind == Radial::Sphere {
                    return self.finish(s, c, r, 0.0);
                }
            }
            (Some(c), Some(r)) => {
                let (what, need_sign) = if self.has_height() { ("a non-zero height", true) } else { ("a positive tube radius", false) };
                let v = dist(&i, c)
                    .filter(|v| v.is_finite() && if need_sign { *v != 0.0 } else { *v > 0.0 })
                    .ok_or_else(|| EngineError::Other(format!("Requires {what}.")))?;
                return self.finish(s, c, r, v);
            }
        }
        Ok(Step::Continue)
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        match (self.center, self.radius) {
            (Some(ct), None) => vec![circle(ct, ct.dist(c))],
            (Some(ct), Some(r)) => vec![circle(ct, r)],
            _ => Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------------- 3DFACE

/// A 3DFACE from 3 or 4 points; `invisible` has the edge bits (1 = first point to second … 8 =
/// fourth back to first). Refuses a face with no area.
fn face_from(pts: &[Vec3], invisible: u8) -> Result<Face3d> {
    // A triangle repeats its third point; its closing edge (third back to first) is bit 8, and
    // asking for its "third edge" hides that one.
    let tri = |inv: u8| (inv & 3) | if inv & 12 != 0 { 8 } else { 0 };
    let (corners, hidden) = match pts {
        [a, b, c] => ([*a, *b, *c, *c], tri(invisible)),
        [a, b, c, d] if d == c => ([*a, *b, *c, *c], tri(invisible)),
        [a, b, c, d] => ([*a, *b, *c, *d], invisible),
        _ => return Err(err("3dface", "a 3D face needs 3 or 4 points")),
    };
    if corners.iter().any(|c| !c.is_finite()) {
        return Err(err("3dface", "the points must be numbers"));
    }
    let diag = corners.iter().flat_map(|a| corners.iter().map(move |b| (*a - *b).len())).fold(0.0, f64::max);
    let eps = 1e-12 * diag * diag;
    let [a, b, c, d] = corners;
    if (b - a).cross(c - a).len() <= eps && (c - a).cross(d - a).len() <= eps {
        return Err(err("3dface", "the points are collinear: the face has no area"));
    }
    Ok(Face3d { corners, hidden_edges: hidden })
}

fn run_3dface(s: &mut Session, p: &Value) -> Result<Value> {
    let pts: Vec<Vec3> = p
        .get("points")
        .and_then(Value::as_array)
        .ok_or_else(|| err("3dface", "`points` ([[x,y,z?], 3 or 4 points]) is required"))?
        .iter()
        .map(|v| {
            let a = v.as_array()?;
            let q = Vec3::new(a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2).and_then(Value::as_f64).unwrap_or(0.0));
            q.is_finite().then_some(q)
        })
        .collect::<Option<_>>()
        .ok_or_else(|| err("3dface", "each point must be [x, y, z?] numbers"))?;
    let mut hidden = 0u8;
    match p.get("invisible") {
        None | Some(Value::Null) => {}
        Some(Value::Array(a)) => {
            for v in a {
                let n = v.as_u64().filter(|n| (1..=4).contains(n)).ok_or_else(|| err("3dface", "`invisible` lists edge numbers from 1 to 4"))?;
                hidden |= 1u8 << (n - 1);
            }
        }
        Some(_) => return Err(err("3dface", "`invisible` must be an array of edge numbers from 1 to 4")),
    }
    let f = face_from(&pts, hidden)?;
    let h = s.add_entity(EntityKind::Face3d(f))?;
    Ok(json!({ "handle": h.hex() }))
}

#[derive(Default)]
struct FaceM {
    pts: Vec<Vec3>,
    hidden: u8,
    next_hidden: bool,
}

impl FaceM {
    fn finish(&mut self, s: &mut Session) -> Result<Step> {
        let f = face_from(&self.pts, self.hidden)?;
        s.add_entity(EntityKind::Face3d(f))?;
        Ok(Step::Done)
    }
}

impl Interactive for FaceM {
    fn name(&self) -> &'static str {
        "3DFACE"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        let base = self.pts.last().map(|p| p.xy());
        let (msg, kw): (&str, &[&str]) = match self.pts.len() {
            0 => ("Specify first point or", &["Invisible"]),
            1 => ("Specify second point or", &["Invisible"]),
            2 => ("Specify third point or", &["Invisible"]),
            _ => ("Specify fourth point or", &["Invisible"]),
        };
        let p = Prompt::new(msg, Accept::POINT).kw(kw);
        let p = if self.pts.len() == 3 { p.default("create three-sided face") } else { p };
        match base {
            Some(b) => p.base(b),
            None => p,
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Keyword(k) if k == "Invisible" => {
                self.next_hidden = true;
                Ok(Step::Continue)
            }
            Input::Point(p) => {
                if self.next_hidden {
                    // The edge that starts at this point is the invisible one.
                    self.hidden |= 1u8 << self.pts.len();
                    self.next_hidden = false;
                }
                self.pts.push(p.to3(0.0));
                if self.pts.len() == 4 { self.finish(s) } else { Ok(Step::Continue) }
            }
            Input::Enter if self.pts.len() == 3 => self.finish(s),
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        let mut pts: Vec<Vec2> = self.pts.iter().map(|p| p.xy()).collect();
        if pts.is_empty() {
            return Vec::new();
        }
        pts.push(c);
        vec![lwpoly(pts.into_iter().map(cadcraft_geom::PolyVertex::new).collect(), pts_closed(self.pts.len()))]
    }
}

fn pts_closed(placed: usize) -> bool {
    placed >= 2
}

// ---------------------------------------------------------------------------------- EXTRUDE

/// A closed profile ready to extrude: its points, elevation and properties.
struct Profile {
    pts: Vec<Vec2>,
    z: f64,
    common: Common,
}

fn profile_of(e: &Entity, tol: Option<f64>) -> std::result::Result<Profile, String> {
    match &e.kind {
        EntityKind::Circle(c) => {
            if !c.radius.is_finite() || c.radius <= 0.0 || !c.center.is_finite() {
                return Err("a circle with no usable radius".into());
            }
            let n = mesh::circle_segments(c.radius, tol.unwrap_or(c.radius * 1e-3));
            let ctr = c.center.xy();
            let pts = (0..n).map(|i| Vec2::polar(ctr, c.radius, TAU * i as f64 / n as f64)).collect();
            Ok(Profile { pts, z: c.center.z, common: e.common.clone() })
        }
        EntityKind::LwPolyline(p) => {
            let closed = p.closed || p.vertices.first().zip(p.vertices.last()).is_some_and(|(a, b)| p.vertices.len() > 2 && a.p.near(b.p, 1e-9));
            if !closed {
                return Err("an open polyline (close it first)".into());
            }
            if p.vertices.len() > MAX_PROFILE {
                return Err("a polyline with too many vertices".into());
            }
            if p.vertices.iter().any(|v| !v.p.is_finite() || !v.bulge.is_finite()) {
                return Err("a polyline with a vertex that is not a number".into());
            }
            let (mut lo, mut hi) = (Vec2::new(f64::MAX, f64::MAX), Vec2::new(f64::MIN, f64::MIN));
            for v in &p.vertices {
                lo = lo.min(v.p);
                hi = hi.max(v.p);
            }
            let tol = tol.unwrap_or((hi - lo).len() * 1e-3);
            let mut pts: Vec<Vec2> = Vec::new();
            for seg in (Polyline { vertices: p.vertices.clone(), closed: true }).segments() {
                let mut t = Vec::new();
                seg.tessellate(tol, &mut t);
                let skip = usize::from(pts.last().zip(t.first()).is_some_and(|(l, f)| l.near(*f, 1e-12)));
                pts.extend(t.into_iter().skip(skip));
                if pts.len() > MAX_PROFILE {
                    return Err("a profile that needs too many points (raise the tolerance)".into());
                }
            }
            Ok(Profile { pts, z: p.elevation, common: e.common.clone() })
        }
        other => Err(format!("a {} (EXTRUDE takes closed polylines and circles)", other.type_name())),
    }
}

fn extrude_handles(s: &mut Session, hs: &[Handle], holes: &[Handle], height: f64, tol: Option<f64>, erase: bool) -> Result<Value> {
    if !height.is_finite() || height == 0.0 {
        return Err(err("extrude", "`height` must be a non-zero number"));
    }
    if hs.is_empty() {
        return Err(err("extrude", "nothing to extrude: select closed polylines or circles, or pass `handles`"));
    }
    if holes.len() > mesh::MAX_HOLES {
        return Err(err("extrude", "too many holes"));
    }
    if let Some(h) = holes.iter().find(|h| hs.contains(h)) {
        return Err(err("extrude", format!("{} is both a profile and a hole", h.hex())));
    }
    // Build every solid before changing the drawing, so a bad profile leaves it untouched.
    let load = |list: &[Handle]| -> Result<Vec<(Handle, Profile)>> {
        let mut out = Vec::with_capacity(list.len());
        for h in list {
            if erase && super::curves::is_locked(s, *h) {
                return Err(err("extrude", "a profile is locked"));
            }
            let d: &Drawing = s.doc()?;
            let e = d.entity(*h).ok_or_else(|| err("extrude", format!("{} is not an object in this drawing", h.hex())))?;
            out.push((*h, profile_of(e, tol).map_err(|m| err("extrude", format!("{}: {m}", h.hex())))?));
        }
        Ok(out)
    };
    let outers = load(hs)?;
    let cutouts = load(holes)?;
    let mut assigned: Vec<Vec<usize>> = vec![Vec::new(); outers.len()];
    for (k, (h, hp)) in cutouts.iter().enumerate() {
        let first = hp.pts.first().copied().ok_or_else(|| err("extrude", format!("{}: the hole has no points", h.hex())))?;
        let owners: Vec<usize> = outers.iter().enumerate().filter(|(_, (_, op))| point_in_polygon(&op.pts, first)).map(|(i, _)| i).collect();
        match owners.as_slice() {
            [i] => {
                if let Some(list) = assigned.get_mut(*i) {
                    list.push(k);
                }
            }
            [] => return Err(err("extrude", format!("{}: the hole is not inside any profile", h.hex()))),
            _ => return Err(err("extrude", format!("{}: the hole is inside more than one profile", h.hex()))),
        }
    }
    let mut built: Vec<(Handle, Common, Vec<Face>)> = Vec::new();
    let mut total = 0usize;
    for (i, (h, prof)) in outers.iter().enumerate() {
        let cuts: Vec<Vec<Vec2>> = assigned.get(i).into_iter().flatten().filter_map(|k| cutouts.get(*k)).map(|(_, p)| p.pts.clone()).collect();
        let faces = mesh::extrude_with_holes(&prof.pts, &cuts, prof.z, height).map_err(|m| err("extrude", format!("{}: {m}", h.hex())))?;
        total += faces.len();
        if total > MAX_FACES {
            return Err(err("extrude", "the solids would have too many faces"));
        }
        built.push((*h, prof.common.clone(), faces));
    }
    let mut all = Vec::new();
    let mut groups = Vec::new();
    for (src, common, faces) in &built {
        let (new, group) = add_solid(s, faces, Some(common), "extrude")?;
        all.extend(new);
        groups.push(group);
        if erase {
            s.doc_mut()?.remove_entity(*src);
        }
    }
    if erase {
        for (h, _) in &cutouts {
            s.doc_mut()?.remove_entity(*h);
        }
    }
    Ok(json!({
        "handles": hexes(&all),
        "groups": groups,
        "faces": all.len(),
        "message": format!("{} solid(s) created ({} faces).", groups.len(), all.len()),
    }))
}

/// `holes`: an array of object handles (hex strings or numbers), absent or null for none.
fn holes_param(p: &Value) -> Result<Vec<Handle>> {
    match p.get("holes") {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(a)) => a
            .iter()
            .map(|v| {
                v.as_str()
                    .and_then(Handle::parse_hex)
                    .or_else(|| v.as_u64().map(Handle))
                    .ok_or_else(|| err("extrude", format!("`holes`: {v} is not an object handle")))
            })
            .collect(),
        Some(v) => Err(err("extrude", format!("`holes` must be an array of object handles, got {v}"))),
    }
}

fn run_extrude(s: &mut Session, p: &Value) -> Result<Value> {
    let height = number_req("extrude", p, "height")?;
    let tol = size_param("extrude", p, "tolerance", false)?;
    let holes = holes_param(p)?;
    let hs = targets(s, p)?;
    extrude_handles(s, &hs, &holes, height, tol, bool_or(p, "erase", false))
}

#[derive(Default)]
struct ExtrudeM {
    sel: SelectPhase,
    picked: Vec<Handle>,
    holes: Vec<Handle>,
    hole_sel: SelectPhase,
    picking_holes: bool,
}

/// The objects of `hs` that have a usable profile, with a note when some were left out.
fn usable(s: &mut Session, hs: Vec<Handle>) -> Vec<Handle> {
    let total = hs.len();
    let keep: Vec<Handle> = match s.doc() {
        Ok(d) => hs.into_iter().filter(|h| d.entity(*h).is_some_and(|e| profile_of(e, None).is_ok())).collect(),
        Err(_) => Vec::new(),
    };
    let ignored = total.saturating_sub(keep.len());
    if ignored > 0 {
        s.echo(format!("{ignored} object(s) cannot be extruded and were ignored."));
    }
    keep
}

impl ExtrudeM {
    /// Keep the picked objects that have a usable profile. Ends the command when none remain.
    fn take(&mut self, s: &mut Session, hs: Vec<Handle>) -> Step {
        self.picked = usable(s, hs);
        if self.picked.is_empty() { Step::Done } else { Step::Continue }
    }
}

impl Interactive for ExtrudeM {
    fn name(&self) -> &'static str {
        "EXTRUDE"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.sel = SelectPhase::begin(s);
        if self.sel.done {
            let picked = self.sel.picked.clone();
            return Ok(self.take(s, picked));
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.picked.is_empty() {
            self.sel.prompt()
        } else if self.picking_holes {
            self.hole_sel.prompt()
        } else {
            Prompt::new("Specify height of extrusion or", Accept::NUMBER).kw(&["Holes"])
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.picked.is_empty() {
            return Ok(match self.sel.feed(s, &i)? {
                SelOutcome::More => Step::Continue,
                SelOutcome::Empty => Step::Done,
                SelOutcome::Done(hs) => self.take(s, hs),
            });
        }
        if self.picking_holes {
            match self.hole_sel.feed(s, &i)? {
                SelOutcome::More => {}
                SelOutcome::Empty => self.picking_holes = false,
                SelOutcome::Done(hs) => {
                    let picked = self.picked.clone();
                    let mut keep = usable(s, hs);
                    keep.retain(|h| !picked.contains(h));
                    self.holes = keep;
                    self.picking_holes = false;
                }
            }
            return Ok(Step::Continue);
        }
        if let Input::Keyword(k) = &i
            && k == "Holes"
        {
            self.picking_holes = true;
            self.hole_sel = SelectPhase::default();
            s.echo("Select the closed polylines and circles that are holes.");
            return Ok(Step::Continue);
        }
        let h = match &i {
            Input::Text(t) => number(t),
            _ => None,
        }
        .filter(|h| h.is_finite() && *h != 0.0)
        .ok_or_else(|| EngineError::Other("Requires a non-zero height.".into()))?;
        let r = extrude_handles(s, &self.picked.clone(), &self.holes.clone(), h, None, false)?;
        if let Some(m) = r.get("message").and_then(Value::as_str) {
            s.echo(m.to_string());
        }
        s.set_selection(Vec::new());
        Ok(Step::Done)
    }
}
