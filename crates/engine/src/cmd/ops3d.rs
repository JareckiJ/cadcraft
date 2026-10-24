//! 3D Operations: 3D Move, 3D Rotate, 3D Align, Align, 3D Mirror, 3D Array.
//!
//! Clean-room implementation for AutoCAD parity under `Modify > 3D Operations`.

use std::collections::HashMap;

use cadcraft_doc::{Entity, EntityKind, Face3d, Handle, Line};
use cadcraft_geom::{Mat4, Vec3};
use serde_json::{Value, json};

use super::machines::{SelOutcome, SelectPhase, number};
use super::{bad, f64_or};
use crate::{Accept, CommandSpec, EngineError, Input, Interactive, Prompt, Result, Session, Step};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("3dmove", "3D Move", run_3dmove)
            .alias(&["3dmove"])
            .menu(&["Modify", "3D Operations", "3D Move"])
            .params("{handles?, from?: [x,y,z], to?: [x,y,z] | delta?: [dx,dy,dz]}")
            .interactive(|s| Ok(Box::new(Move3dM::new(s)))),
        CommandSpec::new("3drotate", "3D Rotate", run_3drotate)
            .alias(&["3drotate"])
            .menu(&["Modify", "3D Operations", "3D Rotate"])
            .params("{handles?, base?: [x,y,z], axis?: \"x\"|\"y\"|\"z\"|[dx,dy,dz] | p2?: [x,y,z], angle: f64 (degrees), copy?: bool}")
            .interactive(|s| Ok(Box::new(Rotate3dM::new(s)))),
        CommandSpec::new("3dalign", "3D Align", run_3dalign)
            .alias(&["3dalign"])
            .menu(&["Modify", "3D Operations", "3D Align"])
            .params("{handles?, source: [[x,y,z], ...], dest: [[x,y,z], ...], copy?: bool}")
            .interactive(|s| Ok(Box::new(AlignM::new(s, true)))),
        CommandSpec::new("mirror3d", "3D Mirror", run_mirror3d)
            .alias(&["mirror3d", "3dmirror"])
            .menu(&["Modify", "3D Operations", "3D Mirror"])
            .params("{handles?, p1?: [x,y,z], p2?: [x,y,z], p3?: [x,y,z] | plane?: \"xy\"|\"yz\"|\"zx\", point?: [x,y,z], erase?: bool}")
            .interactive(|s| Ok(Box::new(Mirror3dM::new(s)))),
        CommandSpec::new("3darray", "3D Array", run_3darray)
            .alias(&["3darray"])
            .menu(&["Modify", "3D Operations", "3D Array"])
            .params("{handles?, type?: \"rectangular\"|\"polar\", rows?, cols?, levels?, rowDist?, colDist?, levelDist?, count?, fillAngle?, rotate?: bool, center?: [x,y,z], axisPt?: [x,y,z]}")
            .interactive(|s| Ok(Box::new(Array3dM::new(s)))),
        CommandSpec::new("slice", "Slice", run_slice)
            .alias(&["slice", "sl"])
            .menu(&["Modify", "3D Operations", "Slice"])
            .params("{handles?: [hex], p1?: [x,y,z], p2?: [x,y,z], p3?: [x,y,z], keepBoth?: bool}")
            .interactive(|s| Ok(Box::new(SliceM::new(s)))),
        CommandSpec::new("thicken", "Thicken", run_thicken)
            .alias(&["thicken"])
            .menu(&["Modify", "3D Operations", "Thicken"])
            .params("{handles?: [hex], thickness?: number}")
            .interactive(|s| Ok(Box::new(ThickenM::new(s)))),
        CommandSpec::new("interfere", "Interference Checking", run_interfere)
            .alias(&["interfere", "inf"])
            .menu(&["Modify", "3D Operations", "Interference Checking"])
            .params("{firstSet?: [hex], secondSet?: [hex]}")
            .interactive(|s| Ok(Box::new(InterfereM::new(s)))),
        CommandSpec::new("convtosolid", "Convert to Solid", run_convtosolid)
            .alias(&["convtosolid"])
            .menu(&["Modify", "3D Operations", "Convert to Solid"])
            .params("{handles?: [hex]}")
            .interactive(|s| Ok(Box::new(ConvToSolidM::new(s)))),
        CommandSpec::new("convtosurface", "Convert to Surface", run_convtosurface)
            .alias(&["convtosurface"])
            .menu(&["Modify", "3D Operations", "Convert to Surface"])
            .params("{handles?: [hex]}")
            .interactive(|s| Ok(Box::new(ConvToSurfaceM::new(s)))),
        CommandSpec::new("xedges", "Extract Edges", run_xedges)
            .alias(&["xedges"])
            .menu(&["Modify", "3D Operations", "Extract Edges"])
            .params("{handles?: [hex]}")
            .interactive(|s| Ok(Box::new(XEdgesM::new(s)))),
        CommandSpec::new("extractisolines", "Extract Isolines", run_extract_isolines)
            .alias(&["extractisolines", "xlines"])
            .menu(&["Modify", "3D Operations", "Extract Isolines"])
            .params("{handles?: [hex]}")
            .interactive(|s| Ok(Box::new(ExtractIsolinesM::new(s)))),
    ]
}

// ---------------- Helpers ----------------

pub(crate) fn transform_entities_3d(s: &mut Session, hs: &[Handle], m: &Mat4, copy: bool) -> Result<Vec<Handle>> {
    let d = s.doc_mut()?;
    let mut out = Vec::new();
    if copy {
        let mut handle_map: HashMap<Handle, Handle> = HashMap::new();
        for &h in hs {
            let nh = d.new_handle();
            handle_map.insert(h, nh);
        }
        for &h in hs {
            let Some(sp) = d.space_of(h) else { continue };
            let Some(e) = d.entity(h).map(|e| (**e).clone()) else { continue };
            let Some(&nh) = handle_map.get(&h) else { continue };
            let mut ne = Entity { handle: nh, ..e };
            ne.kind.transform3d(m);
            if let EntityKind::Hatch(h) = &mut ne.kind
                && h.associative
            {
                for l in &mut h.loops {
                    l.sources = l.sources.iter().map(|s| handle_map.get(s).copied().unwrap_or(*s)).collect();
                }
            }
            if let Some(st) = d.space_mut(&sp) {
                st.push(ne);
                out.push(nh);
            }
        }
    } else {
        for &h in hs {
            let locked = d.entity(h).and_then(|e| d.layer(&e.common.layer)).is_some_and(|l| l.locked);
            if locked {
                continue;
            }
            d.modify_entity(h, |e| e.kind.transform3d(m))?;
            out.push(h);
        }
    }
    Ok(out)
}

fn target_handles(s: &Session, v: &Value) -> Vec<Handle> {
    if let Some(arr) = v.get("handles").and_then(Value::as_array) {
        arr.iter().filter_map(Value::as_str).filter_map(Handle::parse_hex).collect()
    } else {
        s.selection().to_vec()
    }
}

fn parse_vec3(v: &Value) -> Option<Vec3> {
    match v {
        Value::Array(a) => {
            let x = a.first()?.as_f64()?;
            let y = a.get(1)?.as_f64()?;
            let z = a.get(2).and_then(Value::as_f64).unwrap_or(0.0);
            if x.is_finite() && y.is_finite() && z.is_finite() { Some(Vec3::new(x, y, z)) } else { None }
        }
        _ => None,
    }
}

use super::modeling3d::{parse_point3d, point_to_vec3};

// ---------------- 3DMOVE ----------------

fn run_3dmove(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = target_handles(s, p);
    if hs.is_empty() {
        return Err(EngineError::Other("3dmove: no objects selected".into()));
    }

    let delta = if let Some(d) = p.get("delta").and_then(parse_vec3) {
        d
    } else if let Some(from) = p.get("from").and_then(parse_vec3) {
        if let Some(to) = p.get("to").and_then(parse_vec3) { to - from } else { from }
    } else {
        return Err(bad("3dmove", "requires 'delta' or 'from'/'to' coordinates"));
    };

    let m = Mat4::translate(delta);
    let affected = transform_entities_3d(s, &hs, &m, false)?;
    Ok(json!({
        "status": "ok",
        "moved": affected.iter().map(|h| h.hex()).collect::<Vec<_>>()
    }))
}

struct Move3dM {
    sel: SelectPhase,
    base: Option<Vec3>,
}

impl Move3dM {
    fn new(s: &Session) -> Self {
        let sel = if !s.selection().is_empty() {
            SelectPhase { picked: s.selection().to_vec(), done: true, ..Default::default() }
        } else {
            SelectPhase::default()
        };
        Self { sel, base: None }
    }
}

impl Interactive for Move3dM {
    fn name(&self) -> &'static str {
        "3DMOVE"
    }

    fn prompt(&self, _s: &Session) -> Prompt {
        if !self.sel.done {
            return self.sel.prompt();
        }
        match self.base {
            None => Prompt::new("Specify base point:", Accept::TEXT | Accept::POINT),
            Some(_) => Prompt::new("Specify second point or <use first point as displacement>:", Accept::TEXT | Accept::POINT),
        }
    }

    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if !self.sel.done {
            match self.sel.feed(s, &i)? {
                SelOutcome::More => return Ok(Step::Continue),
                SelOutcome::Empty => return Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    self.sel.picked = hs;
                    self.sel.done = true;
                    return Ok(Step::Continue);
                }
            }
        }

        match (self.base, i) {
            (None, Input::Point(p)) => {
                self.base = Some(point_to_vec3(s, p));
                Ok(Step::Continue)
            }
            (None, Input::Text(t)) => {
                if let Some(pt) = parse_point3d(s, &t) {
                    self.base = Some(pt);
                    Ok(Step::Continue)
                } else {
                    Err(bad("3dmove", "invalid point coordinate"))
                }
            }
            (Some(b), Input::Point(p)) => {
                let p2 = point_to_vec3(s, p);
                let delta = p2 - b;
                transform_entities_3d(s, &self.sel.picked, &Mat4::translate(delta), false)?;
                Ok(Step::Done)
            }
            (Some(b), Input::Text(t)) => {
                if let Some(p2) = parse_point3d(s, &t) {
                    let delta = p2 - b;
                    transform_entities_3d(s, &self.sel.picked, &Mat4::translate(delta), false)?;
                    Ok(Step::Done)
                } else {
                    Err(bad("3dmove", "invalid point coordinate"))
                }
            }
            (Some(b), Input::Enter) => {
                transform_entities_3d(s, &self.sel.picked, &Mat4::translate(b), false)?;
                Ok(Step::Done)
            }
            _ => Ok(Step::Continue),
        }
    }
}

// ---------------- 3DROTATE ----------------

fn run_3drotate(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = target_handles(s, p);
    if hs.is_empty() {
        return Err(EngineError::Other("3drotate: no objects selected".into()));
    }

    let base = p.get("base").and_then(parse_vec3).unwrap_or(Vec3::ZERO);
    let axis_dir = if let Some(ax) = p.get("axis").and_then(Value::as_str) {
        match ax.trim().to_ascii_uppercase().as_str() {
            "X" => Vec3::X,
            "Y" => Vec3::Y,
            _ => Vec3::Z,
        }
    } else if let Some(ax) = p.get("axis").and_then(parse_vec3) {
        ax
    } else if let Some(p2) = p.get("p2").and_then(parse_vec3) {
        p2 - base
    } else {
        Vec3::Z
    };

    let angle_deg = p.get("angle").and_then(Value::as_f64).ok_or_else(|| bad("3drotate", "missing 'angle'"))?;
    let copy = p.get("copy").and_then(Value::as_bool).unwrap_or(false);

    let m = Mat4::rotate_axis(base, axis_dir, angle_deg.to_radians());
    let affected = transform_entities_3d(s, &hs, &m, copy)?;
    Ok(json!({
        "status": "ok",
        "rotated": affected.iter().map(|h| h.hex()).collect::<Vec<_>>()
    }))
}

struct Rotate3dM {
    sel: SelectPhase,
    base: Option<Vec3>,
    axis_dir: Option<Vec3>,
    copy: bool,
}

impl Rotate3dM {
    fn new(s: &Session) -> Self {
        let sel = if !s.selection().is_empty() {
            SelectPhase { picked: s.selection().to_vec(), done: true, ..Default::default() }
        } else {
            SelectPhase::default()
        };
        Self { sel, base: None, axis_dir: None, copy: false }
    }
}

impl Interactive for Rotate3dM {
    fn name(&self) -> &'static str {
        "3DROTATE"
    }

    fn prompt(&self, _s: &Session) -> Prompt {
        if !self.sel.done {
            return self.sel.prompt();
        }
        if self.base.is_none() {
            return Prompt::new("Specify base point:", Accept::TEXT | Accept::POINT);
        }
        if self.axis_dir.is_none() {
            return Prompt::new("Specify rotation axis [X/Y/Z] <Z>:", Accept::TEXT).kw(&["X", "Y", "Z"]);
        }
        Prompt::new("Specify angle of rotation or [Copy]:", Accept::TEXT | Accept::POINT_OR_NUMBER).kw(&["Copy"])
    }

    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if !self.sel.done {
            match self.sel.feed(s, &i)? {
                SelOutcome::More => return Ok(Step::Continue),
                SelOutcome::Empty => return Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    self.sel.picked = hs;
                    self.sel.done = true;
                    return Ok(Step::Continue);
                }
            }
        }

        if self.base.is_none() {
            match i {
                Input::Point(p) => {
                    self.base = Some(point_to_vec3(s, p));
                    return Ok(Step::Continue);
                }
                Input::Text(t) => {
                    if let Some(pt) = parse_point3d(s, &t) {
                        self.base = Some(pt);
                        return Ok(Step::Continue);
                    }
                    return Err(bad("3drotate", "invalid base point"));
                }
                _ => return Ok(Step::Continue),
            }
        }

        if self.axis_dir.is_none() {
            match i {
                Input::Keyword(k) | Input::Text(k) => {
                    let dir = match k.trim().to_ascii_uppercase().as_str() {
                        "X" => Vec3::X,
                        "Y" => Vec3::Y,
                        _ => Vec3::Z,
                    };
                    self.axis_dir = Some(dir);
                    return Ok(Step::Continue);
                }
                Input::Enter => {
                    self.axis_dir = Some(Vec3::Z);
                    return Ok(Step::Continue);
                }
                _ => return Ok(Step::Continue),
            }
        }

        match i {
            Input::Keyword(k) if k.eq_ignore_ascii_case("Copy") => {
                self.copy = true;
                Ok(Step::Continue)
            }
            Input::Text(t) => {
                if t.trim().eq_ignore_ascii_case("c") || t.trim().eq_ignore_ascii_case("copy") {
                    self.copy = true;
                    return Ok(Step::Continue);
                }
                if let Ok(n) = t.trim().parse::<f64>() {
                    let base = self.base.unwrap_or(Vec3::ZERO);
                    let axis = self.axis_dir.unwrap_or(Vec3::Z);
                    let m = Mat4::rotate_axis(base, axis, n.to_radians());
                    transform_entities_3d(s, &self.sel.picked, &m, self.copy)?;
                    return Ok(Step::Done);
                }
                Err(bad("3drotate", "invalid rotation angle"))
            }
            _ => Ok(Step::Continue),
        }
    }
}

// ---------------- 3DALIGN ----------------

fn run_3dalign(s: &mut Session, p: &Value) -> Result<Value> {
    execute_align(s, p, true)
}

fn execute_align(s: &mut Session, p: &Value, is_3dalign: bool) -> Result<Value> {
    let hs = target_handles(s, p);
    if hs.is_empty() {
        return Err(EngineError::Other("align: no objects selected".into()));
    }

    let src_pts: Vec<Vec3> = p.get("source").and_then(Value::as_array).map(|arr| arr.iter().filter_map(parse_vec3).collect()).unwrap_or_default();
    let dst_pts: Vec<Vec3> = p.get("dest").and_then(Value::as_array).map(|arr| arr.iter().filter_map(parse_vec3).collect()).unwrap_or_default();

    if src_pts.is_empty() || dst_pts.is_empty() {
        return Err(bad("align", "missing source or destination points"));
    }

    let scale = p.get("scale").and_then(Value::as_bool).unwrap_or(false);
    let copy = is_3dalign && p.get("copy").and_then(Value::as_bool).unwrap_or(false);

    let m = if src_pts.len() >= 3 && dst_pts.len() >= 3 {
        Mat4::align_3point([src_pts[0], src_pts[1], src_pts[2]], [dst_pts[0], dst_pts[1], dst_pts[2]])
            .ok_or_else(|| bad("align", "cannot compute 3-point alignment"))?
    } else if src_pts.len() >= 2 && dst_pts.len() >= 2 {
        Mat4::align_2point([src_pts[0], src_pts[1]], [dst_pts[0], dst_pts[1]], scale)
            .ok_or_else(|| bad("align", "cannot compute 2-point alignment"))?
    } else {
        Mat4::translate(dst_pts[0] - src_pts[0])
    };

    let affected = transform_entities_3d(s, &hs, &m, copy)?;
    Ok(json!({
        "status": "ok",
        "aligned": affected.iter().map(|h| h.hex()).collect::<Vec<_>>()
    }))
}

struct AlignM {
    sel: SelectPhase,
    is_3dalign: bool,
    source: Vec<Vec3>,
    dest: Vec<Vec3>,
    waiting_for_dest: bool,
}

impl AlignM {
    fn new(s: &Session, is_3dalign: bool) -> Self {
        let sel = if !s.selection().is_empty() {
            SelectPhase { picked: s.selection().to_vec(), done: true, ..Default::default() }
        } else {
            SelectPhase::default()
        };
        Self { sel, is_3dalign, source: Vec::new(), dest: Vec::new(), waiting_for_dest: false }
    }
}

impl Interactive for AlignM {
    fn name(&self) -> &'static str {
        if self.is_3dalign { "3DALIGN" } else { "ALIGN" }
    }

    fn prompt(&self, _s: &Session) -> Prompt {
        if !self.sel.done {
            return self.sel.prompt();
        }
        if self.waiting_for_dest {
            match self.dest.len() {
                0 => Prompt::new("Specify first destination point:", Accept::TEXT | Accept::POINT),
                1 => Prompt::new("Specify second destination point:", Accept::TEXT | Accept::POINT),
                _ => Prompt::new("Specify third destination point:", Accept::TEXT | Accept::POINT),
            }
        } else {
            match self.source.len() {
                0 => Prompt::new("Specify first source point:", Accept::TEXT | Accept::POINT),
                1 => Prompt::new("Specify second source point or <continue>:", Accept::TEXT | Accept::POINT),
                2 => Prompt::new("Specify third source point or <continue>:", Accept::TEXT | Accept::POINT),
                _ => Prompt::new("Press Enter to align:", Accept::TEXT),
            }
        }
    }

    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if !self.sel.done {
            match self.sel.feed(s, &i)? {
                SelOutcome::More => return Ok(Step::Continue),
                SelOutcome::Empty => return Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    self.sel.picked = hs;
                    self.sel.done = true;
                    return Ok(Step::Continue);
                }
            }
        }

        let parse_pt = |s: &Session, i: &Input| -> Option<Vec3> {
            match i {
                Input::Point(p) => Some(point_to_vec3(s, *p)),
                Input::Text(t) => parse_point3d(s, t),
                _ => None,
            }
        };

        if self.waiting_for_dest {
            if let Some(pt) = parse_pt(s, &i) {
                self.dest.push(pt);
                self.waiting_for_dest = false;
                if self.source.len() >= 3 && self.dest.len() >= 3 {
                    let m = Mat4::align_3point([self.source[0], self.source[1], self.source[2]], [self.dest[0], self.dest[1], self.dest[2]])
                        .ok_or_else(|| bad("align", "cannot align 3 points"))?;
                    transform_entities_3d(s, &self.sel.picked, &m, false)?;
                    return Ok(Step::Done);
                }
                return Ok(Step::Continue);
            }
        } else if let Some(pt) = parse_pt(s, &i) {
            self.source.push(pt);
            self.waiting_for_dest = true;
            return Ok(Step::Continue);
        } else if matches!(i, Input::Enter) {
            if self.source.len() == 1 && self.dest.len() == 1 {
                let m = Mat4::translate(self.dest[0] - self.source[0]);
                transform_entities_3d(s, &self.sel.picked, &m, false)?;
                return Ok(Step::Done);
            } else if self.source.len() == 2 && self.dest.len() == 2 {
                let m = Mat4::align_2point([self.source[0], self.source[1]], [self.dest[0], self.dest[1]], false)
                    .ok_or_else(|| bad("align", "cannot align 2 points"))?;
                transform_entities_3d(s, &self.sel.picked, &m, false)?;
                return Ok(Step::Done);
            }
        }

        Ok(Step::Continue)
    }
}

// ---------------- MIRROR3D ----------------

fn run_mirror3d(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = target_handles(s, p);
    if hs.is_empty() {
        return Err(EngineError::Other("mirror3d: no objects selected".into()));
    }

    let (plane_pt, normal) = if let Some(plane_kw) = p.get("plane").and_then(Value::as_str) {
        let pt = p.get("point").and_then(parse_vec3).unwrap_or(Vec3::ZERO);
        match plane_kw.trim().to_ascii_uppercase().as_str() {
            "XY" => (pt, Vec3::Z),
            "YZ" => (pt, Vec3::X),
            "ZX" => (pt, Vec3::Y),
            _ => (pt, Vec3::Z),
        }
    } else if let (Some(p1), Some(p2), Some(p3)) =
        (p.get("p1").and_then(parse_vec3), p.get("p2").and_then(parse_vec3), p.get("p3").and_then(parse_vec3))
    {
        let n = (p2 - p1).cross(p3 - p1);
        if n.len() < 1e-12 {
            return Err(bad("mirror3d", "points are collinear; plane not defined"));
        }
        (p1, n.normalized())
    } else {
        return Err(bad("mirror3d", "specify p1, p2, p3 or standard plane (XY/YZ/ZX)"));
    };

    let erase = p.get("erase").and_then(Value::as_bool).unwrap_or(false);
    let m = Mat4::mirror_plane(plane_pt, normal);
    let affected = transform_entities_3d(s, &hs, &m, !erase)?;
    Ok(json!({
        "status": "ok",
        "mirrored": affected.iter().map(|h| h.hex()).collect::<Vec<_>>()
    }))
}

struct Mirror3dM {
    sel: SelectPhase,
    pts: Vec<Vec3>,
}

impl Mirror3dM {
    fn new(s: &Session) -> Self {
        let sel = if !s.selection().is_empty() {
            SelectPhase { picked: s.selection().to_vec(), done: true, ..Default::default() }
        } else {
            SelectPhase::default()
        };
        Self { sel, pts: Vec::new() }
    }
}

impl Interactive for Mirror3dM {
    fn name(&self) -> &'static str {
        "MIRROR3D"
    }

    fn prompt(&self, _s: &Session) -> Prompt {
        if !self.sel.done {
            return self.sel.prompt();
        }
        match self.pts.len() {
            0 => Prompt::new("Specify first point on mirror plane:", Accept::TEXT | Accept::POINT),
            1 => Prompt::new("Specify second point on mirror plane:", Accept::TEXT | Accept::POINT),
            2 => Prompt::new("Specify third point on mirror plane:", Accept::TEXT | Accept::POINT),
            _ => Prompt::new("Delete source objects? [Yes/No] <N>:", Accept::TEXT).kw(&["Yes", "No"]),
        }
    }

    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if !self.sel.done {
            match self.sel.feed(s, &i)? {
                SelOutcome::More => return Ok(Step::Continue),
                SelOutcome::Empty => return Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    self.sel.picked = hs;
                    self.sel.done = true;
                    return Ok(Step::Continue);
                }
            }
        }

        let parse_pt = |s: &Session, i: &Input| -> Option<Vec3> {
            match i {
                Input::Point(p) => Some(point_to_vec3(s, *p)),
                Input::Text(t) => parse_point3d(s, t),
                _ => None,
            }
        };

        if self.pts.len() < 3 {
            if let Some(pt) = parse_pt(s, &i) {
                self.pts.push(pt);
                return Ok(Step::Continue);
            }
        } else {
            let erase = match i {
                Input::Keyword(k) | Input::Text(k) => k.trim().eq_ignore_ascii_case("y") || k.trim().eq_ignore_ascii_case("yes"),
                _ => false,
            };
            let p1 = self.pts[0];
            let p2 = self.pts[1];
            let p3 = self.pts[2];
            let n = (p2 - p1).cross(p3 - p1);
            if n.len() < 1e-12 {
                return Err(bad("mirror3d", "points are collinear; plane not defined"));
            }
            let m = Mat4::mirror_plane(p1, n.normalized());
            transform_entities_3d(s, &self.sel.picked, &m, !erase)?;
            return Ok(Step::Done);
        }

        Ok(Step::Continue)
    }
}

// ---------------- 3DARRAY ----------------

fn run_3darray(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = target_handles(s, p);
    if hs.is_empty() {
        return Err(EngineError::Other("3darray: no objects selected".into()));
    }

    let arr_type = p.get("type").and_then(Value::as_str).unwrap_or("rectangular").to_ascii_lowercase();

    let mut created = Vec::new();
    if arr_type.starts_with('p') {
        // Polar
        let count = p.get("count").and_then(Value::as_u64).unwrap_or(4) as usize;
        let angle = p.get("fillAngle").or_else(|| p.get("angle")).and_then(Value::as_f64).unwrap_or(360.0);
        let center = p.get("center").and_then(parse_vec3).unwrap_or(Vec3::ZERO);
        let axis_pt = p.get("axisPt").and_then(parse_vec3).unwrap_or(center + Vec3::Z);
        let axis_dir = (axis_pt - center).normalized();

        if count > 1 && axis_dir.len() > 1e-12 {
            let step_angle = angle.to_radians() / (count as f64);
            for k in 1..count {
                let a = step_angle * (k as f64);
                let m = Mat4::rotate_axis(center, axis_dir, a);
                let copies = transform_entities_3d(s, &hs, &m, true)?;
                created.extend(copies);
            }
        }
    } else {
        // Rectangular
        let rows = p.get("rows").and_then(Value::as_u64).unwrap_or(1) as usize;
        let cols = p.get("cols").and_then(Value::as_u64).unwrap_or(1) as usize;
        let levels = p.get("levels").and_then(Value::as_u64).unwrap_or(1) as usize;
        let total = rows.saturating_mul(cols).saturating_mul(levels).saturating_mul(hs.len());
        if (total as u64) > s.settings.maxarray {
            return Err(bad("3darray", format!("Maximum array size ({}) exceeded", s.settings.maxarray)));
        }
        let row_dist = p.get("rowDist").or_else(|| p.get("rowSpacing")).and_then(Value::as_f64).unwrap_or(0.0);
        let col_dist = p.get("colDist").or_else(|| p.get("colSpacing")).and_then(Value::as_f64).unwrap_or(0.0);
        let level_dist = p.get("levelDist").or_else(|| p.get("levelSpacing")).and_then(Value::as_f64).unwrap_or(0.0);

        for l in 0..levels {
            for r in 0..rows {
                for c in 0..cols {
                    if l == 0 && r == 0 && c == 0 {
                        continue;
                    }
                    let shift = Vec3::new(c as f64 * col_dist, r as f64 * row_dist, l as f64 * level_dist);
                    let m = Mat4::translate(shift);
                    let copies = transform_entities_3d(s, &hs, &m, true)?;
                    created.extend(copies);
                }
            }
        }
    }

    Ok(json!({
        "status": "ok",
        "created": created.iter().map(|h| h.hex()).collect::<Vec<_>>()
    }))
}

struct Array3dM {
    sel: SelectPhase,
    is_polar: Option<bool>,
    rows: usize,
    cols: usize,
    levels: usize,
    row_dist: f64,
    col_dist: f64,
    level_dist: f64,
    step: u8,
}

impl Array3dM {
    fn new(s: &Session) -> Self {
        let sel = if !s.selection().is_empty() {
            SelectPhase { picked: s.selection().to_vec(), done: true, ..Default::default() }
        } else {
            SelectPhase::default()
        };
        Self { sel, is_polar: None, rows: 1, cols: 1, levels: 1, row_dist: 10.0, col_dist: 10.0, level_dist: 10.0, step: 0 }
    }
}

impl Interactive for Array3dM {
    fn name(&self) -> &'static str {
        "3DARRAY"
    }

    fn prompt(&self, _s: &Session) -> Prompt {
        if !self.sel.done {
            return self.sel.prompt();
        }
        match self.is_polar {
            None => Prompt::new("Enter the type of array [Rectangular/Polar] <R>:", Accept::TEXT).kw(&["Rectangular", "Polar"]),
            Some(false) => match self.step {
                0 => Prompt::new("Enter the number of rows (---) <1>:", Accept::POINT_OR_NUMBER),
                1 => Prompt::new("Enter the number of columns (|||) <1>:", Accept::POINT_OR_NUMBER),
                2 => Prompt::new("Enter the number of levels (...) <1>:", Accept::POINT_OR_NUMBER),
                3 => Prompt::new("Specify the distance between rows (---):", Accept::POINT_OR_NUMBER),
                4 => Prompt::new("Specify the distance between columns (|||):", Accept::POINT_OR_NUMBER),
                _ => Prompt::new("Specify the distance between levels (...):", Accept::POINT_OR_NUMBER),
            },
            Some(true) => Prompt::new("Polar 3D array: specify count, angle, center and axis", Accept::TEXT),
        }
    }

    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if !self.sel.done {
            match self.sel.feed(s, &i)? {
                SelOutcome::More => return Ok(Step::Continue),
                SelOutcome::Empty => return Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    self.sel.picked = hs;
                    self.sel.done = true;
                    return Ok(Step::Continue);
                }
            }
        }

        if self.is_polar.is_none() {
            let polar = match i {
                Input::Keyword(k) | Input::Text(k) => k.trim().to_ascii_uppercase().starts_with('P'),
                _ => false,
            };
            self.is_polar = Some(polar);
            return Ok(Step::Continue);
        }

        if self.is_polar == Some(false) {
            let num = match i {
                Input::Text(t) => t.trim().parse::<f64>().unwrap_or(1.0),
                Input::Enter => {
                    if self.step <= 2 {
                        1.0
                    } else {
                        10.0
                    }
                }
                _ => 1.0,
            };

            match self.step {
                0 => self.rows = (num as usize).max(1),
                1 => self.cols = (num as usize).max(1),
                2 => self.levels = (num as usize).max(1),
                3 => self.row_dist = num,
                4 => self.col_dist = num,
                _ => {
                    self.level_dist = num;
                    let total = self.rows.saturating_mul(self.cols).saturating_mul(self.levels).saturating_mul(self.sel.picked.len());
                    if (total as u64) > s.settings.maxarray {
                        return Err(bad("3darray", format!("Maximum array size ({}) exceeded", s.settings.maxarray)));
                    }
                    for l in 0..self.levels {
                        for r in 0..self.rows {
                            for c in 0..self.cols {
                                if l == 0 && r == 0 && c == 0 {
                                    continue;
                                }
                                let shift = Vec3::new(c as f64 * self.col_dist, r as f64 * self.row_dist, l as f64 * self.level_dist);
                                transform_entities_3d(s, &self.sel.picked, &Mat4::translate(shift), true)?;
                            }
                        }
                    }
                    return Ok(Step::Done);
                }
            }
            self.step += 1;
        } else {
            return Ok(Step::Done);
        }

        Ok(Step::Continue)
    }
}

// ---------------- Slice ----------------

pub fn run_slice(s: &mut Session, p: &Value) -> Result<Value> {
    let targets = target_handles(s, p);
    if targets.is_empty() {
        return Err(bad("slice", "no objects selected"));
    }
    let p1 = p.get("p1").and_then(parse_vec3).unwrap_or(Vec3::ZERO);
    let p2 = p.get("p2").and_then(parse_vec3).unwrap_or(Vec3::new(1.0, 0.0, 0.0));
    let p3 = p.get("p3").and_then(parse_vec3).unwrap_or(Vec3::new(0.0, 1.0, 0.0));
    let keep_both = p.get("keepBoth").and_then(Value::as_bool).unwrap_or(false);

    let v1 = p2 - p1;
    let v2 = p3 - p1;
    let normal = if v1.cross(v2).len() > 1e-6 { v1.cross(v2).normalized() } else { Vec3::Z };

    let mut sliced_handles = Vec::new();
    for h in targets {
        let Some(e) = s.doc()?.entity(h).cloned() else { continue };
        if let EntityKind::Face3d(face) = &e.kind {
            let d0 = (face.corners[0] - p1).dot(normal);
            let d1 = (face.corners[1] - p1).dot(normal);
            let d2 = (face.corners[2] - p1).dot(normal);
            let d3 = (face.corners[3] - p1).dot(normal);

            let all_pos = d0 >= -1e-6 && d1 >= -1e-6 && d2 >= -1e-6 && d3 >= -1e-6;
            let all_neg = d0 <= 1e-6 && d1 <= 1e-6 && d2 <= 1e-6 && d3 <= 1e-6;

            if all_neg && !keep_both {
                s.doc_mut()?.remove_entity(h);
            } else if all_pos || keep_both {
                sliced_handles.push(h);
            }
        }
    }

    s.echo(format!("Slice: {} entities sliced.", sliced_handles.len()));
    Ok(json!({ "handles": sliced_handles.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

pub struct SliceM {
    sel: SelectPhase,
    p1: Option<Vec3>,
    p2: Option<Vec3>,
}

impl SliceM {
    pub fn new(s: &Session) -> Self {
        let sel = if !s.selection().is_empty() {
            SelectPhase { picked: s.selection().to_vec(), done: true, ..Default::default() }
        } else {
            SelectPhase::default()
        };
        Self { sel, p1: None, p2: None }
    }
}

impl Interactive for SliceM {
    fn name(&self) -> &'static str {
        "SLICE"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if !self.sel.done {
            self.sel.prompt()
        } else if self.p1.is_none() {
            Prompt::new("Specify start point of slicing plane:", Accept::POINT)
        } else if self.p2.is_none() {
            Prompt::new("Specify second point on plane:", Accept::POINT)
        } else {
            Prompt::new("Specify third point on plane or [keep Both sides]:", Accept::POINT | Accept::TEXT).kw(&["Both"])
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if !self.sel.done {
            match self.sel.feed(s, &i)? {
                SelOutcome::More => return Ok(Step::Continue),
                SelOutcome::Empty => return Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    self.sel.picked = hs;
                    self.sel.done = true;
                    return Ok(Step::Continue);
                }
            }
        }

        match i {
            Input::Point(p) => {
                let pt = Vec3::new(p.x, p.y, 0.0);
                if self.p1.is_none() {
                    self.p1 = Some(pt);
                } else if self.p2.is_none() {
                    self.p2 = Some(pt);
                } else {
                    let p1 = self.p1.unwrap_or(Vec3::ZERO);
                    let p2 = self.p2.unwrap_or(Vec3::X);
                    let hs = self.sel.picked.iter().map(|h| h.hex()).collect::<Vec<_>>();
                    run_slice(s, &json!({ "handles": hs, "p1": [p1.x, p1.y, p1.z], "p2": [p2.x, p2.y, p2.z], "p3": [pt.x, pt.y, pt.z] }))?;
                    return Ok(Step::Done);
                }
            }
            Input::Keyword(k) if k.eq_ignore_ascii_case("both") => {
                let p1 = self.p1.unwrap_or(Vec3::ZERO);
                let p2 = self.p2.unwrap_or(Vec3::X);
                let hs = self.sel.picked.iter().map(|h| h.hex()).collect::<Vec<_>>();
                run_slice(s, &json!({ "handles": hs, "p1": [p1.x, p1.y, p1.z], "p2": [p2.x, p2.y, p2.z], "keepBoth": true }))?;
                return Ok(Step::Done);
            }
            Input::Enter => return Ok(Step::Done),
            _ => {}
        }
        Ok(Step::Continue)
    }
}

// ---------------- Thicken ----------------

pub fn run_thicken(s: &mut Session, p: &Value) -> Result<Value> {
    let handles = match p.get("handles") {
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).filter_map(Handle::parse_hex).collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    let thickness = f64_or(p, "thickness", 1.0);

    let doc = s.doc()?;
    let targets = if handles.is_empty() { doc.model.iter().map(|e| e.handle).collect::<Vec<_>>() } else { handles };

    let mut new_handles = Vec::new();
    for h in targets {
        let Some(e) = s.doc()?.entity(h).cloned() else { continue };
        if let EntityKind::Face3d(face) = &e.kind {
            let v1 = face.corners[1] - face.corners[0];
            let v2 = face.corners[2] - face.corners[0];
            let n = if v1.cross(v2).len() > 1e-6 { v1.cross(v2).normalized() } else { Vec3::Z };
            let offset = n * thickness;

            let c = face.corners;
            let o = [c[0] + offset, c[1] + offset, c[2] + offset, c[3] + offset];

            // Opposite face
            let f_top = Face3d { corners: [o[3], o[2], o[1], o[0]], hidden_edges: 0 };
            new_handles.push(s.add_entity(EntityKind::Face3d(f_top))?);

            // 4 side faces
            new_handles.push(s.add_entity(EntityKind::Face3d(Face3d { corners: [c[0], c[1], o[1], o[0]], hidden_edges: 0 }))?);
            new_handles.push(s.add_entity(EntityKind::Face3d(Face3d { corners: [c[1], c[2], o[2], o[1]], hidden_edges: 0 }))?);
            new_handles.push(s.add_entity(EntityKind::Face3d(Face3d { corners: [c[2], c[3], o[3], o[2]], hidden_edges: 0 }))?);
            new_handles.push(s.add_entity(EntityKind::Face3d(Face3d { corners: [c[3], c[0], o[0], o[3]], hidden_edges: 0 }))?);
        }
    }

    s.echo(format!("Thicken: created {} faces.", new_handles.len()));
    Ok(json!({ "handles": new_handles.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

pub struct ThickenM {
    sel: SelectPhase,
}

impl ThickenM {
    pub fn new(s: &Session) -> Self {
        let sel = if !s.selection().is_empty() {
            SelectPhase { picked: s.selection().to_vec(), done: true, ..Default::default() }
        } else {
            SelectPhase::default()
        };
        Self { sel }
    }
}

impl Interactive for ThickenM {
    fn name(&self) -> &'static str {
        "THICKEN"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if !self.sel.done { self.sel.prompt() } else { Prompt::new("Specify surface thickness:", Accept::POINT | Accept::TEXT) }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if !self.sel.done {
            match self.sel.feed(s, &i)? {
                SelOutcome::More => return Ok(Step::Continue),
                SelOutcome::Empty => return Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    self.sel.picked = hs;
                    self.sel.done = true;
                    return Ok(Step::Continue);
                }
            }
        }

        let thick = match i {
            Input::Text(t) => number(&t).unwrap_or(1.0),
            _ => 1.0,
        };

        let hs = self.sel.picked.iter().map(|h| h.hex()).collect::<Vec<_>>();
        run_thicken(s, &json!({ "handles": hs, "thickness": thick }))?;
        Ok(Step::Done)
    }
}

// ---------------- Interfere ----------------

pub fn run_interfere(s: &mut Session, p: &Value) -> Result<Value> {
    let first = match p.get("firstSet") {
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).filter_map(Handle::parse_hex).collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    let second = match p.get("secondSet") {
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).filter_map(Handle::parse_hex).collect::<Vec<_>>(),
        _ => Vec::new(),
    };

    let doc = s.doc()?;
    let set1 = if first.is_empty() { doc.model.iter().map(|e| e.handle).collect::<Vec<_>>() } else { first };
    let same_set = second.is_empty();
    let set2 = if same_set { set1.clone() } else { second };

    let mut interference_count = 0;
    for &h1 in &set1 {
        let Some(e1) = doc.entity(h1) else { continue };
        let b1 = cadcraft_doc::entity_bounds(doc, e1, 0);
        for &h2 in &set2 {
            if h1 == h2 || (same_set && h1 >= h2) {
                continue;
            }
            let Some(e2) = doc.entity(h2) else { continue };
            let b2 = cadcraft_doc::entity_bounds(doc, e2, 0);
            if b1.intersects(&b2) {
                interference_count += 1;
            }
        }
    }

    s.echo(format!("Interference Checking: {interference_count} interfering pairs found."));
    Ok(json!({ "interferences": interference_count }))
}

pub struct InterfereM {
    sel: SelectPhase,
}

impl InterfereM {
    pub fn new(s: &Session) -> Self {
        let sel = if !s.selection().is_empty() {
            SelectPhase { picked: s.selection().to_vec(), done: true, ..Default::default() }
        } else {
            SelectPhase::default()
        };
        Self { sel }
    }
}

impl Interactive for InterfereM {
    fn name(&self) -> &'static str {
        "INTERFERE"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if !self.sel.done { self.sel.prompt() } else { Prompt::new("Press Enter to check interference:", Accept::TEXT) }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if !self.sel.done {
            match self.sel.feed(s, &i)? {
                SelOutcome::More => return Ok(Step::Continue),
                SelOutcome::Empty => return Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    self.sel.picked = hs;
                    self.sel.done = true;
                    return Ok(Step::Continue);
                }
            }
        }

        let hs = self.sel.picked.iter().map(|h| h.hex()).collect::<Vec<_>>();
        run_interfere(s, &json!({ "firstSet": hs }))?;
        Ok(Step::Done)
    }
}

// ---------------- Convert to Solid / Surface ----------------

pub fn run_convtosolid(_s: &mut Session, _p: &Value) -> Result<Value> {
    Err(bad("convtosolid", "CONVTOSOLID is not available yet"))
}

pub struct ConvToSolidM {
    sel: SelectPhase,
}

impl ConvToSolidM {
    pub fn new(s: &Session) -> Self {
        let sel = if !s.selection().is_empty() {
            SelectPhase { picked: s.selection().to_vec(), done: true, ..Default::default() }
        } else {
            SelectPhase::default()
        };
        Self { sel }
    }
}

impl Interactive for ConvToSolidM {
    fn name(&self) -> &'static str {
        "CONVTOSOLID"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if !self.sel.done { self.sel.prompt() } else { Prompt::new("Press Enter to convert to solid:", Accept::TEXT) }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if !self.sel.done {
            match self.sel.feed(s, &i)? {
                SelOutcome::More => return Ok(Step::Continue),
                SelOutcome::Empty => return Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    self.sel.picked = hs;
                    self.sel.done = true;
                    return Ok(Step::Continue);
                }
            }
        }

        let hs = self.sel.picked.iter().map(|h| h.hex()).collect::<Vec<_>>();
        run_convtosolid(s, &json!({ "handles": hs }))?;
        Ok(Step::Done)
    }
}

pub fn run_convtosurface(_s: &mut Session, _p: &Value) -> Result<Value> {
    Err(bad("convtosurface", "CONVTOSURFACE is not available yet"))
}

pub struct ConvToSurfaceM {
    sel: SelectPhase,
}

impl ConvToSurfaceM {
    pub fn new(s: &Session) -> Self {
        let sel = if !s.selection().is_empty() {
            SelectPhase { picked: s.selection().to_vec(), done: true, ..Default::default() }
        } else {
            SelectPhase::default()
        };
        Self { sel }
    }
}

impl Interactive for ConvToSurfaceM {
    fn name(&self) -> &'static str {
        "CONVTOSURFACE"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if !self.sel.done { self.sel.prompt() } else { Prompt::new("Press Enter to convert to surface:", Accept::TEXT) }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if !self.sel.done {
            match self.sel.feed(s, &i)? {
                SelOutcome::More => return Ok(Step::Continue),
                SelOutcome::Empty => return Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    self.sel.picked = hs;
                    self.sel.done = true;
                    return Ok(Step::Continue);
                }
            }
        }

        let hs = self.sel.picked.iter().map(|h| h.hex()).collect::<Vec<_>>();
        run_convtosurface(s, &json!({ "handles": hs }))?;
        Ok(Step::Done)
    }
}

// ---------------- Extract Edges ----------------

pub fn run_xedges(s: &mut Session, p: &Value) -> Result<Value> {
    let handles = match p.get("handles") {
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).filter_map(Handle::parse_hex).collect::<Vec<_>>(),
        _ => Vec::new(),
    };

    let doc = s.doc()?;
    let targets = if handles.is_empty() { doc.model.iter().map(|e| e.handle).collect::<Vec<_>>() } else { handles };

    let mut line_handles = Vec::new();
    for h in targets {
        let Some(e) = s.doc()?.entity(h).cloned() else { continue };
        match &e.kind {
            EntityKind::Face3d(face) => {
                for i in 0..4 {
                    if (face.hidden_edges & (1 << i)) == 0 {
                        let next = (i + 1) % 4;
                        let line = Line { a: face.corners[i], b: face.corners[next] };
                        line_handles.push(s.add_entity(EntityKind::Line(line))?);
                    }
                }
            }
            EntityKind::Solid(sol) | EntityKind::Trace(sol) => {
                for i in 0..4 {
                    let next = (i + 1) % 4;
                    let line = Line { a: sol.corners[i], b: sol.corners[next] };
                    line_handles.push(s.add_entity(EntityKind::Line(line))?);
                }
            }
            _ => {}
        }
    }

    s.echo(format!("Extract Edges: {} wireframe lines extracted.", line_handles.len()));
    Ok(json!({ "handles": line_handles.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

pub struct XEdgesM {
    sel: SelectPhase,
}

impl XEdgesM {
    pub fn new(s: &Session) -> Self {
        let sel = if !s.selection().is_empty() {
            SelectPhase { picked: s.selection().to_vec(), done: true, ..Default::default() }
        } else {
            SelectPhase::default()
        };
        Self { sel }
    }
}

impl Interactive for XEdgesM {
    fn name(&self) -> &'static str {
        "XEDGES"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if !self.sel.done { self.sel.prompt() } else { Prompt::new("Press Enter to extract edges:", Accept::TEXT) }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if !self.sel.done {
            match self.sel.feed(s, &i)? {
                SelOutcome::More => return Ok(Step::Continue),
                SelOutcome::Empty => return Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    self.sel.picked = hs;
                    self.sel.done = true;
                    return Ok(Step::Continue);
                }
            }
        }

        let hs = self.sel.picked.iter().map(|h| h.hex()).collect::<Vec<_>>();
        run_xedges(s, &json!({ "handles": hs }))?;
        Ok(Step::Done)
    }
}

// ---------------- Extract Isolines ----------------

pub fn run_extract_isolines(_s: &mut Session, _p: &Value) -> Result<Value> {
    Err(bad("extractisolines", "EXTRACTISOLINES is not available yet"))
}

pub struct ExtractIsolinesM {
    sel: SelectPhase,
}

impl ExtractIsolinesM {
    pub fn new(s: &Session) -> Self {
        let sel = if !s.selection().is_empty() {
            SelectPhase { picked: s.selection().to_vec(), done: true, ..Default::default() }
        } else {
            SelectPhase::default()
        };
        Self { sel }
    }
}

impl Interactive for ExtractIsolinesM {
    fn name(&self) -> &'static str {
        "EXTRACTISOLINES"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if !self.sel.done { self.sel.prompt() } else { Prompt::new("Press Enter to extract isolines:", Accept::TEXT) }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if !self.sel.done {
            match self.sel.feed(s, &i)? {
                SelOutcome::More => return Ok(Step::Continue),
                SelOutcome::Empty => return Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    self.sel.picked = hs;
                    self.sel.done = true;
                    return Ok(Step::Continue);
                }
            }
        }

        let hs = self.sel.picked.iter().map(|h| h.hex()).collect::<Vec<_>>();
        run_extract_isolines(s, &json!({ "handles": hs }))?;
        Ok(Step::Done)
    }
}
