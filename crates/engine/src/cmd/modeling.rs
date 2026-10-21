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
            .interactive(|_| Ok(Box::new(BoxM::default()))),
        CommandSpec::new("cylinder", "Cylinder", run_cylinder)
            .menu(&["Draw", "3D Modeling", "Cylinder"])
            .params("{center: [x,y,z?], radius, height, tolerance? (chord error, default radius/1000) | segments? (3..1024)} → {handles, group, faces}. The base is at the centre's Z; a negative height goes down.")
            .interactive(|_| Ok(Box::new(CylinderM::default()))),
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

// ---------------------------------------------------------------------------------- BOX

fn box_at(s: &mut Session, corner: Vec3, size: Vec3) -> Result<Value> {
    let faces = mesh::box_faces(corner, size).map_err(|e| err("box", e))?;
    let (hs, group) = add_solid(s, &faces, None, "box")?;
    Ok(json!({ "handles": hexes(&hs), "group": group, "faces": hs.len(), "message": format!("Box created ({} faces, group {group}).", hs.len()) }))
}

fn run_box(s: &mut Session, p: &Value) -> Result<Value> {
    let corner = point3(p, "corner").ok_or_else(|| err("box", "`corner` ([x, y, z?]) is required"))?;
    let height = number_req("box", p, "height")?;
    let (dx, dy) = if let Some(c2) = p.get("corner2").and_then(point_value) {
        (c2.x - corner.x, c2.y - corner.y)
    } else {
        (number_req("box", p, "length").map_err(|_| err("box", "`corner2` or `length` and `width` are required"))?, number_req("box", p, "width")?)
    };
    box_at(s, corner, Vec3::new(dx, dy, height))
}

#[derive(Default)]
struct BoxM {
    first: Option<Vec2>,
    second: Option<Vec2>,
}

impl Interactive for BoxM {
    fn name(&self) -> &'static str {
        "BOX"
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
                box_at(s, a.to3(0.0), Vec3::new(b.x - a.x, b.y - a.y, h))?;
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

// ---------------------------------------------------------------------------------- CYLINDER

fn cylinder_at(s: &mut Session, center: Vec3, radius: f64, height: f64, segments: usize) -> Result<Value> {
    let faces = mesh::cylinder(center, radius, height, segments).map_err(|e| err("cylinder", e))?;
    let (hs, group) = add_solid(s, &faces, None, "cylinder")?;
    Ok(
        json!({ "handles": hexes(&hs), "group": group, "faces": hs.len(), "message": format!("Cylinder created ({} faces, group {group}).", hs.len()) }),
    )
}

fn run_cylinder(s: &mut Session, p: &Value) -> Result<Value> {
    let center = point3(p, "center").ok_or_else(|| err("cylinder", "`center` ([x, y, z?]) is required"))?;
    let radius = number_req("cylinder", p, "radius")?;
    let height = number_req("cylinder", p, "height")?;
    let segments = match p.get("segments") {
        None | Some(Value::Null) => {
            let tol = size_param("cylinder", p, "tolerance", false)?.unwrap_or(radius.abs() * 1e-3);
            mesh::circle_segments(radius.abs(), tol)
        }
        Some(v) => {
            v.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or_else(|| err("cylinder", "`segments` must be a whole number from 3 to 1024"))?
        }
    };
    cylinder_at(s, center, radius, height, segments)
}

#[derive(Default)]
struct CylinderM {
    center: Option<Vec2>,
    radius: Option<f64>,
}

impl Interactive for CylinderM {
    fn name(&self) -> &'static str {
        "CYLINDER"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match (self.center, self.radius) {
            (None, _) => Prompt::new("Specify center point of base", Accept::POINT),
            (Some(c), None) => Prompt::new("Specify base radius", Accept::POINT_OR_NUMBER).base(c),
            (Some(c), Some(_)) => Prompt::new("Specify height", Accept::POINT_OR_NUMBER).base(c),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
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
                let r = dist(&i, c).filter(|r| r.is_finite() && *r > 0.0).ok_or_else(|| EngineError::Other("Requires a positive radius.".into()))?;
                self.radius = Some(r);
            }
            (Some(c), Some(r)) => {
                let h = dist(&i, c).filter(|h| h.is_finite() && *h != 0.0).ok_or_else(|| EngineError::Other("Requires a non-zero height.".into()))?;
                cylinder_at(s, c.to3(0.0), r, h, mesh::circle_segments(r, r * 1e-3))?;
                return Ok(Step::Done);
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
