//! 3D Face command (3DFACE).
//!
//! Clean-room implementation for AutoCAD parity under `Draw > 3D Modeling > Meshes > 3D Face`.

use cadcraft_doc::{Face3d, Handle};
use cadcraft_geom::Vec3;
use serde_json::{Value, json};

use super::bad;
use crate::{Accept, CommandSpec, Input, Interactive, Prompt, Result, Session, Step};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("3dface", "3D Face", run_3dface)
            .alias(&["3dface", "face"])
            .menu(&["Draw", "3D Modeling", "Meshes", "3D Face"])
            .params("{points: [[x,y,z], ...], invisible?: [0..3], invisibleEdges?: u8}")
            .interactive(|_| Ok(Box::new(Face3dM::default()))),
    ]
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

fn run_3dface(s: &mut Session, p: &Value) -> Result<Value> {
    let pts: Vec<Vec3> = p.get("points").and_then(Value::as_array).map(|arr| arr.iter().filter_map(parse_vec3).collect()).unwrap_or_default();

    if pts.len() < 3 {
        return Err(bad("3dface", "at least 3 points are required to define a face"));
    }

    let p0 = pts[0];
    let p1 = pts[1];
    let p2 = pts[2];
    let p3 = if pts.len() >= 4 { pts[3] } else { p2 };

    let mut hidden_edges = p.get("invisibleEdges").and_then(Value::as_u64).unwrap_or(0) as u8;
    if let Some(arr) = p.get("invisible").and_then(Value::as_array) {
        for idx in arr.iter().filter_map(Value::as_u64) {
            if idx < 4 {
                hidden_edges |= 1 << idx;
            }
        }
    }

    let face = Face3d { corners: [p0, p1, p2, p3], hidden_edges };

    let h = s.add_entity(cadcraft_doc::EntityKind::Face3d(face))?;

    Ok(json!({
        "status": "ok",
        "handle": h.hex()
    }))
}

#[derive(Default)]
struct Face3dM {
    pts: Vec<Vec3>,
    hidden_edges: u8,
    next_invisible: bool,
}

impl Interactive for Face3dM {
    fn name(&self) -> &'static str {
        "3DFACE"
    }

    fn prompt(&self, _s: &Session) -> Prompt {
        let msg = match self.pts.len() {
            0 => "Specify first point or [Invisible]:",
            1 => "Specify second point or [Invisible]:",
            2 => "Specify third point or [Invisible] <exit>:",
            _ => "Specify fourth point or [Invisible] <create three-sided face>:",
        };
        Prompt::new(msg, Accept::TEXT | Accept::POINT).kw(&["Invisible"])
    }

    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Keyword(k) if k.eq_ignore_ascii_case("Invisible") => {
                self.next_invisible = true;
                return Ok(Step::Continue);
            }
            Input::Text(t) if t.trim().eq_ignore_ascii_case("i") || t.trim().eq_ignore_ascii_case("invisible") => {
                self.next_invisible = true;
                return Ok(Step::Continue);
            }
            Input::Enter => {
                if self.pts.len() == 3 {
                    // Create three-sided face
                    let p0 = self.pts[0];
                    let p1 = self.pts[1];
                    let p2 = self.pts[2];
                    let face = Face3d { corners: [p0, p1, p2, p2], hidden_edges: self.hidden_edges };
                    create_face_entity(s, face)?;
                    // Prepare for next face using edges from previous
                    self.pts = vec![p2, p1];
                    self.hidden_edges = 0;
                    return Ok(Step::Continue);
                } else if self.pts.len() >= 4 {
                    let p0 = self.pts[0];
                    let p1 = self.pts[1];
                    let p2 = self.pts[2];
                    let p3 = self.pts[3];
                    let face = Face3d { corners: [p0, p1, p2, p3], hidden_edges: self.hidden_edges };
                    create_face_entity(s, face)?;
                    self.pts = vec![p3, p2];
                    self.hidden_edges = 0;
                    return Ok(Step::Continue);
                } else {
                    return Ok(Step::Done);
                }
            }
            Input::Point(p) => {
                let pt = point_to_vec3(s, p);
                self.add_pt(s, pt)?;
            }
            Input::Text(t) => {
                if let Some(pt) = parse_point3d(s, &t) {
                    self.add_pt(s, pt)?;
                } else {
                    return Err(bad("3dface", "invalid point coordinate"));
                }
            }
            _ => return Ok(Step::Continue),
        }
        Ok(Step::Continue)
    }
}

impl Face3dM {
    fn add_pt(&mut self, s: &mut Session, pt: Vec3) -> Result<()> {
        let idx = self.pts.len();
        if self.next_invisible && idx < 4 {
            self.hidden_edges |= 1 << idx;
            self.next_invisible = false;
        }
        self.pts.push(pt);

        if self.pts.len() == 4 {
            let p0 = self.pts[0];
            let p1 = self.pts[1];
            let p2 = self.pts[2];
            let p3 = self.pts[3];
            let face = Face3d { corners: [p0, p1, p2, p3], hidden_edges: self.hidden_edges };
            create_face_entity(s, face)?;
            // Next connected face starts with edges 3 and 4 as 1 and 2
            self.pts = vec![p3, p2];
            self.hidden_edges = 0;
        }
        Ok(())
    }
}

fn create_face_entity(s: &mut Session, face: Face3d) -> Result<Handle> {
    s.add_entity(cadcraft_doc::EntityKind::Face3d(face))
}
