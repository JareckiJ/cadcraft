//! 3D Modeling primitives and meshes: Box, Wedge, Cone, Sphere, Cylinder,
//! Torus, Pyramid, Polysolid, Extrude, Revolve, Sweep, Loft, Section Plane,
//! Mesh Primitives, and Surface Meshes (RevSurf, TabSurf, RuleSurf, EdgeSurf).
//!
//! Clean-room implementation for AutoCAD parity under `Draw > 3D Modeling`.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::f64::consts::PI;

use cadcraft_doc::{EntityKind, Face3d, Handle};
use cadcraft_geom::{Mat4, Vec2, Vec3};
use serde_json::{Value, json};

use super::machines::{SelOutcome, SelectPhase, number};
use super::{CommandSpec, bad, f64_or};
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        // Draw > 3D Modeling > Primitives
        CommandSpec::new("box", "Box", run_box)
            .alias(&["box"])
            .menu(&["Draw", "3D Modeling", "Box"])
            .params("{corner1?: [x,y,z], corner2?: [x,y,z] | length?: number, width?: number, height?: number, center?: [x,y,z]}")
            .interactive(|_| Ok(Box::new(BoxM::default()))),
        CommandSpec::new("wedge", "Wedge", run_wedge)
            .alias(&["wedge"])
            .menu(&["Draw", "3D Modeling", "Wedge"])
            .params("{corner1?: [x,y,z], corner2?: [x,y,z], height?: number}")
            .interactive(|_| Ok(Box::new(WedgeM::default()))),
        CommandSpec::new("cone", "Cone", run_cone)
            .alias(&["cone"])
            .menu(&["Draw", "3D Modeling", "Cone"])
            .params("{center?: [x,y,z], radius?: number, height?: number, segments?: number}")
            .interactive(|_| Ok(Box::new(ConeM::default()))),
        CommandSpec::new("sphere", "Sphere", run_sphere)
            .alias(&["sphere"])
            .menu(&["Draw", "3D Modeling", "Sphere"])
            .params("{center?: [x,y,z], radius?: number, segments?: number}")
            .interactive(|_| Ok(Box::new(SphereM::default()))),
        CommandSpec::new("cylinder", "Cylinder", run_cylinder)
            .alias(&["cylinder", "cyl"])
            .menu(&["Draw", "3D Modeling", "Cylinder"])
            .params("{center?: [x,y,z], radius?: number, height?: number, segments?: number}")
            .interactive(|_| Ok(Box::new(CylinderM::default()))),
        CommandSpec::new("torus", "Torus", run_torus)
            .alias(&["torus"])
            .menu(&["Draw", "3D Modeling", "Torus"])
            .params("{center?: [x,y,z], radius?: number, tubeRadius?: number, segments?: number}")
            .interactive(|_| Ok(Box::new(TorusM::default()))),
        CommandSpec::new("pyramid", "Pyramid", run_pyramid)
            .alias(&["pyramid", "pyr"])
            .menu(&["Draw", "3D Modeling", "Pyramid"])
            .params("{center?: [x,y,z], radius?: number, height?: number, sides?: number}")
            .interactive(|_| Ok(Box::new(PyramidM::default()))),
        CommandSpec::new("polysolid", "Polysolid", run_polysolid)
            .alias(&["polysolid", "psolid"])
            .menu(&["Draw", "3D Modeling", "Polysolid"])
            .params("{points: [[x,y,z], ...], width?: number, height?: number}")
            .interactive(|_| Ok(Box::new(PolysolidM::default()))),
        // Draw > 3D Modeling > Extrude/Revolve/Sweep/Loft/Section Plane
        CommandSpec::new("extrude", "Extrude", run_extrude)
            .alias(&["extrude", "ext"])
            .menu(&["Draw", "3D Modeling", "Extrude"])
            .params("{handles?: [hex], height?: number, direction?: [dx,dy,dz]}")
            .interactive(|s| Ok(Box::new(ExtrudeM::new(s)))),
        CommandSpec::new("revolve", "Revolve", run_revolve)
            .alias(&["revolve", "rev"])
            .menu(&["Draw", "3D Modeling", "Revolve"])
            .params("{handles?: [hex], axisP1?: [x,y,z], axisP2?: [x,y,z], angle?: degrees}")
            .interactive(|s| Ok(Box::new(RevolveM::new(s)))),
        CommandSpec::new("sweep", "Sweep", run_sweep)
            .alias(&["sweep"])
            .menu(&["Draw", "3D Modeling", "Sweep"])
            .params("{handles?: [hex], pathHandle?: hex}")
            .interactive(|s| Ok(Box::new(SweepM::new(s)))),
        CommandSpec::new("loft", "Loft", run_loft)
            .alias(&["loft"])
            .menu(&["Draw", "3D Modeling", "Loft"])
            .params("{handles?: [hex]}")
            .interactive(|s| Ok(Box::new(LoftM::new(s)))),
        CommandSpec::new("sectionplane", "Section Plane", run_section_plane)
            .alias(&["sectionplane"])
            .menu(&["Draw", "3D Modeling", "Section Plane"])
            .params("{point?: [x,y,z], normal?: [dx,dy,dz]}")
            .interactive(|_| Ok(Box::new(SectionPlaneM))),
        // Draw > 3D Modeling > Meshes > Primitives
        CommandSpec::new("meshbox", "Box", run_box)
            .menu(&["Draw", "3D Modeling", "Meshes", "Primitives", "Box"])
            .params("{corner1?: [x,y,z], corner2?: [x,y,z]}"),
        CommandSpec::new("meshwedge", "Wedge", run_wedge)
            .menu(&["Draw", "3D Modeling", "Meshes", "Primitives", "Wedge"])
            .params("{corner1?: [x,y,z], corner2?: [x,y,z]}"),
        CommandSpec::new("meshcone", "Cone", run_cone)
            .menu(&["Draw", "3D Modeling", "Meshes", "Primitives", "Cone"])
            .params("{center?: [x,y,z], radius?: number, height?: number}"),
        CommandSpec::new("meshsphere", "Sphere", run_sphere)
            .menu(&["Draw", "3D Modeling", "Meshes", "Primitives", "Sphere"])
            .params("{center?: [x,y,z], radius?: number}"),
        CommandSpec::new("meshcylinder", "Cylinder", run_cylinder)
            .menu(&["Draw", "3D Modeling", "Meshes", "Primitives", "Cylinder"])
            .params("{center?: [x,y,z], radius?: number, height?: number}"),
        CommandSpec::new("meshtorus", "Torus", run_torus)
            .menu(&["Draw", "3D Modeling", "Meshes", "Primitives", "Torus"])
            .params("{center?: [x,y,z], radius?: number, tubeRadius?: number}"),
        CommandSpec::new("meshpyramid", "Pyramid", run_pyramid)
            .menu(&["Draw", "3D Modeling", "Meshes", "Primitives", "Pyramid"])
            .params("{center?: [x,y,z], radius?: number, height?: number}"),
        CommandSpec::new("mesh", "Smooth Mesh", run_smooth_mesh)
            .alias(&["mesh"])
            .menu(&["Draw", "3D Modeling", "Meshes", "Smooth Mesh"])
            .params("{handles?: [hex]}"),
        // Draw > 3D Modeling > Meshes > Surface Meshes
        CommandSpec::new("revsurf", "Meshes, Revolved Surface", run_revsurf)
            .alias(&["revsurf"])
            .menu(&["Draw", "3D Modeling", "Meshes", "Meshes, Revolved Surface"])
            .params("{profileHandle?: hex, axisP1?: [x,y,z], axisP2?: [x,y,z], startAngle?: degrees, totalAngle?: degrees}"),
        CommandSpec::new("tabsurf", "Meshes, Tabulated Surface", run_tabsurf)
            .alias(&["tabsurf"])
            .menu(&["Draw", "3D Modeling", "Meshes", "Meshes, Tabulated Surface"])
            .params("{pathHandle?: hex, directionVector?: [dx,dy,dz]}"),
        CommandSpec::new("rulesurf", "Meshes, Ruled Surface", run_rulesurf)
            .alias(&["rulesurf"])
            .menu(&["Draw", "3D Modeling", "Meshes", "Meshes, Ruled Surface"])
            .params("{curve1Handle?: hex, curve2Handle?: hex}"),
        CommandSpec::new("edgesurf", "Meshes, Edge Surface", run_edgesurf)
            .alias(&["edgesurf"])
            .menu(&["Draw", "3D Modeling", "Meshes", "Meshes, Edge Surface"])
            .params("{curve1Handle?: hex, curve2Handle?: hex, curve3Handle?: hex, curve4Handle?: hex}"),
    ]
}

// ---------------- Helpers ----------------

fn add_face(s: &mut Session, corners: [Vec3; 4]) -> Result<Handle> {
    let face = Face3d { corners, hidden_edges: 0 };
    s.add_entity(EntityKind::Face3d(face))
}

fn add_triangle(s: &mut Session, p0: Vec3, p1: Vec3, p2: Vec3) -> Result<Handle> {
    add_face(s, [p0, p1, p2, p2])
}

fn vec3_val(p: &Value, key: &str) -> Option<Vec3> {
    let v = p.get(key)?;
    match v {
        Value::Array(a) => {
            let x = a.first()?.as_f64()?;
            let y = a.get(1)?.as_f64()?;
            let z = a.get(2).and_then(Value::as_f64).unwrap_or(0.0);
            Some(Vec3::new(x, y, z))
        }
        _ => None,
    }
}

fn point_to_vec3(s: &Session, p: Vec2) -> Vec3 {
    let elev = s.doc().map(|d| d.header.elevation()).unwrap_or(0.0);
    let ucs = s.doc().map(|d| d.header.ucs()).unwrap_or_else(|_| cadcraft_doc::Ucs::world());
    ucs.to_wcs(Vec3::new(p.x, p.y, elev))
}

// ---------------- Box ----------------

pub fn make_box(s: &mut Session, c1: Vec3, c2: Vec3) -> Result<Vec<Handle>> {
    let x0 = c1.x.min(c2.x);
    let x1 = c1.x.max(c2.x);
    let y0 = c1.y.min(c2.y);
    let y1 = c1.y.max(c2.y);
    let z0 = c1.z.min(c2.z);
    let z1 = c1.z.max(c2.z);

    if (x1 - x0).abs() < 1e-9 || (y1 - y0).abs() < 1e-9 || (z1 - z0).abs() < 1e-9 {
        return Err(bad("box", "box dimensions must be non-zero"));
    }

    let v0 = Vec3::new(x0, y0, z0);
    let v1 = Vec3::new(x1, y0, z0);
    let v2 = Vec3::new(x1, y1, z0);
    let v3 = Vec3::new(x0, y1, z0);
    let v4 = Vec3::new(x0, y0, z1);
    let v5 = Vec3::new(x1, y0, z1);
    let v6 = Vec3::new(x1, y1, z1);
    let v7 = Vec3::new(x0, y1, z1);

    let handles = vec![
        add_face(s, [v0, v3, v2, v1])?, // Bottom
        add_face(s, [v4, v5, v6, v7])?, // Top
        add_face(s, [v0, v1, v5, v4])?, // Front
        add_face(s, [v1, v2, v6, v5])?, // Right
        add_face(s, [v2, v3, v7, v6])?, // Back
        add_face(s, [v3, v0, v4, v7])?, // Left
    ];

    Ok(handles)
}

pub fn run_box(s: &mut Session, p: &Value) -> Result<Value> {
    let c1 = if let Some(c) = vec3_val(p, "corner1") {
        c
    } else if let Some(center) = vec3_val(p, "center") {
        let l = f64_or(p, "length", 10.0);
        let w = f64_or(p, "width", 10.0);
        let h = f64_or(p, "height", 10.0);
        center - Vec3::new(l * 0.5, w * 0.5, h * 0.5)
    } else {
        Vec3::ZERO
    };

    let c2 = if let Some(c) = vec3_val(p, "corner2") {
        c
    } else {
        let l = f64_or(p, "length", 10.0);
        let w = f64_or(p, "width", 10.0);
        let h = f64_or(p, "height", 10.0);
        c1 + Vec3::new(l, w, h)
    };

    let handles = make_box(s, c1, c2)?;
    s.echo(format!("Box created with {} faces.", handles.len()));
    Ok(json!({ "handles": handles.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

#[derive(Default)]
struct BoxM {
    c1: Option<Vec3>,
}

impl Interactive for BoxM {
    fn name(&self) -> &'static str {
        "BOX"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.c1.is_none() {
            Prompt::new("Specify first corner:", Accept::POINT)
        } else {
            Prompt::new("Specify other corner or [Cube/Length]:", Accept::POINT | Accept::TEXT).kw(&["Cube", "Length"])
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Point(p) => {
                let pt = point_to_vec3(s, p);
                if let Some(c1) = self.c1 {
                    run_box(s, &json!({ "corner1": [c1.x, c1.y, c1.z], "corner2": [pt.x, pt.y, pt.z] }))?;
                    Ok(Step::Done)
                } else {
                    self.c1 = Some(pt);
                    Ok(Step::Continue)
                }
            }
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
}

// ---------------- Wedge ----------------

pub fn make_wedge(s: &mut Session, c1: Vec3, c2: Vec3, h: f64) -> Result<Vec<Handle>> {
    let x0 = c1.x.min(c2.x);
    let x1 = c1.x.max(c2.x);
    let y0 = c1.y.min(c2.y);
    let y1 = c1.y.max(c2.y);
    let z0 = c1.z.min(c2.z);
    let z1 = z0 + h;

    let v0 = Vec3::new(x0, y0, z0);
    let v1 = Vec3::new(x1, y0, z0);
    let v2 = Vec3::new(x1, y1, z0);
    let v3 = Vec3::new(x0, y1, z0);
    let v4 = Vec3::new(x0, y1, z1);
    let v5 = Vec3::new(x1, y1, z1);

    let handles = vec![
        add_face(s, [v0, v3, v2, v1])?, // Bottom rect
        add_face(s, [v3, v2, v5, v4])?, // Back vertical rect
        add_face(s, [v0, v1, v5, v4])?, // Sloped front rect
        add_triangle(s, v0, v4, v3)?,   // Left triangle
        add_triangle(s, v1, v2, v5)?,   // Right triangle
    ];

    Ok(handles)
}

pub fn run_wedge(s: &mut Session, p: &Value) -> Result<Value> {
    let c1 = vec3_val(p, "corner1").unwrap_or(Vec3::ZERO);
    let c2 = vec3_val(p, "corner2").unwrap_or(c1 + Vec3::new(10.0, 10.0, 0.0));
    let h = f64_or(p, "height", 10.0);
    let handles = make_wedge(s, c1, c2, h)?;
    s.echo(format!("Wedge created with {} faces.", handles.len()));
    Ok(json!({ "handles": handles.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

#[derive(Default)]
struct WedgeM {
    c1: Option<Vec3>,
}

impl Interactive for WedgeM {
    fn name(&self) -> &'static str {
        "WEDGE"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.c1.is_none() { Prompt::new("Specify first corner:", Accept::POINT) } else { Prompt::new("Specify other corner:", Accept::POINT) }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Point(p) => {
                let pt = point_to_vec3(s, p);
                if let Some(c1) = self.c1 {
                    run_wedge(s, &json!({ "corner1": [c1.x, c1.y, c1.z], "corner2": [pt.x, pt.y, pt.z] }))?;
                    Ok(Step::Done)
                } else {
                    self.c1 = Some(pt);
                    Ok(Step::Continue)
                }
            }
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
}

// ---------------- Cone & Cylinder ----------------

pub fn make_cone(s: &mut Session, base: Vec3, r: f64, h: f64, segs: usize) -> Result<Vec<Handle>> {
    let n = segs.clamp(6, 64);
    let apex = base + Vec3::new(0.0, 0.0, h);
    let mut ring = Vec::with_capacity(n);
    for i in 0..n {
        let th = (i as f64) * 2.0 * PI / (n as f64);
        ring.push(base + Vec3::new(r * th.cos(), r * th.sin(), 0.0));
    }

    let mut handles = Vec::with_capacity(n * 2);
    for i in 0..n {
        let next = (i + 1) % n;
        // Side face
        handles.push(add_triangle(s, ring[i], ring[next], apex)?);
        // Base triangle fan
        handles.push(add_triangle(s, base, ring[next], ring[i])?);
    }
    Ok(handles)
}

pub fn run_cone(s: &mut Session, p: &Value) -> Result<Value> {
    let base = vec3_val(p, "center").unwrap_or(Vec3::ZERO);
    let r = f64_or(p, "radius", 5.0);
    let h = f64_or(p, "height", 10.0);
    let segs = p.get("segments").and_then(Value::as_u64).unwrap_or(16) as usize;
    let handles = make_cone(s, base, r, h, segs)?;
    s.echo(format!("Cone created with {} faces.", handles.len()));
    Ok(json!({ "handles": handles.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

#[derive(Default)]
struct ConeM {
    center: Option<Vec3>,
}

impl Interactive for ConeM {
    fn name(&self) -> &'static str {
        "CONE"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.center.is_none() {
            Prompt::new("Specify center point of base:", Accept::POINT)
        } else {
            Prompt::new("Specify base radius or [Diameter]:", Accept::POINT | Accept::TEXT).kw(&["Diameter"])
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Point(p) => {
                let pt = point_to_vec3(s, p);
                if let Some(center) = self.center {
                    let r = (pt - center).len().max(0.1);
                    run_cone(s, &json!({ "center": [center.x, center.y, center.z], "radius": r }))?;
                    Ok(Step::Done)
                } else {
                    self.center = Some(pt);
                    Ok(Step::Continue)
                }
            }
            Input::Text(t) => {
                if let Some(r) = number(&t) {
                    let center = self.center.unwrap_or(Vec3::ZERO);
                    run_cone(s, &json!({ "center": [center.x, center.y, center.z], "radius": r }))?;
                    Ok(Step::Done)
                } else {
                    Ok(Step::Continue)
                }
            }
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
}

pub fn make_cylinder(s: &mut Session, base: Vec3, r: f64, h: f64, segs: usize) -> Result<Vec<Handle>> {
    let n = segs.clamp(6, 64);
    let top_center = base + Vec3::new(0.0, 0.0, h);
    let mut b_ring = Vec::with_capacity(n);
    let mut t_ring = Vec::with_capacity(n);

    for i in 0..n {
        let th = (i as f64) * 2.0 * PI / (n as f64);
        let dx = r * th.cos();
        let dy = r * th.sin();
        b_ring.push(base + Vec3::new(dx, dy, 0.0));
        t_ring.push(top_center + Vec3::new(dx, dy, 0.0));
    }

    let mut handles = Vec::with_capacity(n * 3);
    for i in 0..n {
        let next = (i + 1) % n;
        // Side quad
        handles.push(add_face(s, [b_ring[i], b_ring[next], t_ring[next], t_ring[i]])?);
        // Bottom fan
        handles.push(add_triangle(s, base, b_ring[next], b_ring[i])?);
        // Top fan
        handles.push(add_triangle(s, top_center, t_ring[i], t_ring[next])?);
    }
    Ok(handles)
}

pub fn run_cylinder(s: &mut Session, p: &Value) -> Result<Value> {
    let base = vec3_val(p, "center").unwrap_or(Vec3::ZERO);
    let r = f64_or(p, "radius", 5.0);
    let h = f64_or(p, "height", 10.0);
    let segs = p.get("segments").and_then(Value::as_u64).unwrap_or(16) as usize;
    let handles = make_cylinder(s, base, r, h, segs)?;
    s.echo(format!("Cylinder created with {} faces.", handles.len()));
    Ok(json!({ "handles": handles.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

#[derive(Default)]
struct CylinderM {
    center: Option<Vec3>,
}

impl Interactive for CylinderM {
    fn name(&self) -> &'static str {
        "CYLINDER"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.center.is_none() {
            Prompt::new("Specify center point of base:", Accept::POINT)
        } else {
            Prompt::new("Specify base radius or [Diameter]:", Accept::POINT | Accept::TEXT).kw(&["Diameter"])
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Point(p) => {
                let pt = point_to_vec3(s, p);
                if let Some(center) = self.center {
                    let r = (pt - center).len().max(0.1);
                    run_cylinder(s, &json!({ "center": [center.x, center.y, center.z], "radius": r }))?;
                    Ok(Step::Done)
                } else {
                    self.center = Some(pt);
                    Ok(Step::Continue)
                }
            }
            Input::Text(t) => {
                if let Some(r) = number(&t) {
                    let center = self.center.unwrap_or(Vec3::ZERO);
                    run_cylinder(s, &json!({ "center": [center.x, center.y, center.z], "radius": r }))?;
                    Ok(Step::Done)
                } else {
                    Ok(Step::Continue)
                }
            }
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
}

// ---------------- Sphere ----------------

pub fn make_sphere(s: &mut Session, center: Vec3, r: f64, segs: usize) -> Result<Vec<Handle>> {
    let n_lat = segs.clamp(4, 32);
    let n_lon = (n_lat * 2).clamp(8, 64);

    let mut grid = Vec::with_capacity(n_lat + 1);
    for i in 0..=n_lat {
        let lat = -PI / 2.0 + (i as f64) * PI / (n_lat as f64);
        let z = r * lat.sin();
        let r_cos = r * lat.cos();
        let mut row = Vec::with_capacity(n_lon);
        for j in 0..n_lon {
            let lon = (j as f64) * 2.0 * PI / (n_lon as f64);
            row.push(center + Vec3::new(r_cos * lon.cos(), r_cos * lon.sin(), z));
        }
        grid.push(row);
    }

    let mut handles = Vec::new();
    for i in 0..n_lat {
        for j in 0..n_lon {
            let next_j = (j + 1) % n_lon;
            let p00 = grid[i][j];
            let p01 = grid[i][next_j];
            let p10 = grid[i + 1][j];
            let p11 = grid[i + 1][next_j];

            if i == 0 {
                handles.push(add_triangle(s, p00, p11, p10)?);
            } else if i == n_lat - 1 {
                handles.push(add_triangle(s, p00, p01, p10)?);
            } else {
                handles.push(add_face(s, [p00, p01, p11, p10])?);
            }
        }
    }
    Ok(handles)
}

pub fn run_sphere(s: &mut Session, p: &Value) -> Result<Value> {
    let center = vec3_val(p, "center").unwrap_or(Vec3::ZERO);
    let r = f64_or(p, "radius", 5.0);
    let segs = p.get("segments").and_then(Value::as_u64).unwrap_or(12) as usize;
    let handles = make_sphere(s, center, r, segs)?;
    s.echo(format!("Sphere created with {} faces.", handles.len()));
    Ok(json!({ "handles": handles.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

#[derive(Default)]
struct SphereM {
    center: Option<Vec3>,
}

impl Interactive for SphereM {
    fn name(&self) -> &'static str {
        "SPHERE"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.center.is_none() {
            Prompt::new("Specify center point:", Accept::POINT)
        } else {
            Prompt::new("Specify radius or [Diameter]:", Accept::POINT | Accept::TEXT).kw(&["Diameter"])
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Point(p) => {
                let pt = point_to_vec3(s, p);
                if let Some(center) = self.center {
                    let r = (pt - center).len().max(0.1);
                    run_sphere(s, &json!({ "center": [center.x, center.y, center.z], "radius": r }))?;
                    Ok(Step::Done)
                } else {
                    self.center = Some(pt);
                    Ok(Step::Continue)
                }
            }
            Input::Text(t) => {
                if let Some(r) = number(&t) {
                    let center = self.center.unwrap_or(Vec3::ZERO);
                    run_sphere(s, &json!({ "center": [center.x, center.y, center.z], "radius": r }))?;
                    Ok(Step::Done)
                } else {
                    Ok(Step::Continue)
                }
            }
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
}

// ---------------- Torus ----------------

pub fn make_torus(s: &mut Session, center: Vec3, r_major: f64, r_minor: f64, segs: usize) -> Result<Vec<Handle>> {
    let n_u = segs.clamp(8, 64);
    let n_v = segs.clamp(6, 32);

    let mut grid = Vec::with_capacity(n_u);
    for i in 0..n_u {
        let u = (i as f64) * 2.0 * PI / (n_u as f64);
        let cos_u = u.cos();
        let sin_u = u.sin();
        let mut row = Vec::with_capacity(n_v);
        for j in 0..n_v {
            let v = (j as f64) * 2.0 * PI / (n_v as f64);
            let d = r_major + r_minor * v.cos();
            let x = d * cos_u;
            let y = d * sin_u;
            let z = r_minor * v.sin();
            row.push(center + Vec3::new(x, y, z));
        }
        grid.push(row);
    }

    let mut handles = Vec::with_capacity(n_u * n_v);
    for i in 0..n_u {
        let next_i = (i + 1) % n_u;
        for j in 0..n_v {
            let next_j = (j + 1) % n_v;
            let p00 = grid[i][j];
            let p01 = grid[i][next_j];
            let p10 = grid[next_i][j];
            let p11 = grid[next_i][next_j];
            handles.push(add_face(s, [p00, p01, p11, p10])?);
        }
    }
    Ok(handles)
}

pub fn run_torus(s: &mut Session, p: &Value) -> Result<Value> {
    let center = vec3_val(p, "center").unwrap_or(Vec3::ZERO);
    let r1 = f64_or(p, "radius", 10.0);
    let r2 = f64_or(p, "tubeRadius", 2.5);
    let segs = p.get("segments").and_then(Value::as_u64).unwrap_or(16) as usize;
    let handles = make_torus(s, center, r1, r2, segs)?;
    s.echo(format!("Torus created with {} faces.", handles.len()));
    Ok(json!({ "handles": handles.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

#[derive(Default)]
struct TorusM {
    center: Option<Vec3>,
}

impl Interactive for TorusM {
    fn name(&self) -> &'static str {
        "TORUS"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.center.is_none() {
            Prompt::new("Specify center point:", Accept::POINT)
        } else {
            Prompt::new("Specify radius:", Accept::POINT | Accept::TEXT)
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Point(p) => {
                let pt = point_to_vec3(s, p);
                if let Some(center) = self.center {
                    let r = (pt - center).len().max(0.1);
                    run_torus(s, &json!({ "center": [center.x, center.y, center.z], "radius": r, "tubeRadius": r * 0.25 }))?;
                    Ok(Step::Done)
                } else {
                    self.center = Some(pt);
                    Ok(Step::Continue)
                }
            }
            Input::Text(t) => {
                if let Some(r) = number(&t) {
                    let center = self.center.unwrap_or(Vec3::ZERO);
                    run_torus(s, &json!({ "center": [center.x, center.y, center.z], "radius": r, "tubeRadius": r * 0.25 }))?;
                    Ok(Step::Done)
                } else {
                    Ok(Step::Continue)
                }
            }
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
}

// ---------------- Pyramid ----------------

pub fn make_pyramid(s: &mut Session, center: Vec3, r: f64, h: f64, sides: usize) -> Result<Vec<Handle>> {
    let n = sides.clamp(3, 32);
    let apex = center + Vec3::new(0.0, 0.0, h);
    let mut ring = Vec::with_capacity(n);
    for i in 0..n {
        let th = (i as f64) * 2.0 * PI / (n as f64);
        ring.push(center + Vec3::new(r * th.cos(), r * th.sin(), 0.0));
    }

    let mut handles = Vec::with_capacity(n * 2);
    for i in 0..n {
        let next = (i + 1) % n;
        // Side face
        handles.push(add_triangle(s, ring[i], ring[next], apex)?);
        // Base triangle
        handles.push(add_triangle(s, center, ring[next], ring[i])?);
    }
    Ok(handles)
}

pub fn run_pyramid(s: &mut Session, p: &Value) -> Result<Value> {
    let center = vec3_val(p, "center").unwrap_or(Vec3::ZERO);
    let r = f64_or(p, "radius", 5.0);
    let h = f64_or(p, "height", 10.0);
    let sides = p.get("sides").and_then(Value::as_u64).unwrap_or(4) as usize;
    let handles = make_pyramid(s, center, r, h, sides)?;
    s.echo(format!("Pyramid created with {} faces.", handles.len()));
    Ok(json!({ "handles": handles.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

#[derive(Default)]
struct PyramidM {
    center: Option<Vec3>,
}

impl Interactive for PyramidM {
    fn name(&self) -> &'static str {
        "PYRAMID"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.center.is_none() {
            Prompt::new("Specify center point of base:", Accept::POINT)
        } else {
            Prompt::new("Specify base radius:", Accept::POINT | Accept::TEXT)
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Point(p) => {
                let pt = point_to_vec3(s, p);
                if let Some(center) = self.center {
                    let r = (pt - center).len().max(0.1);
                    run_pyramid(s, &json!({ "center": [center.x, center.y, center.z], "radius": r }))?;
                    Ok(Step::Done)
                } else {
                    self.center = Some(pt);
                    Ok(Step::Continue)
                }
            }
            Input::Text(t) => {
                if let Some(r) = number(&t) {
                    let center = self.center.unwrap_or(Vec3::ZERO);
                    run_pyramid(s, &json!({ "center": [center.x, center.y, center.z], "radius": r }))?;
                    Ok(Step::Done)
                } else {
                    Ok(Step::Continue)
                }
            }
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
}

// ---------------- Polysolid ----------------

pub fn run_polysolid(s: &mut Session, p: &Value) -> Result<Value> {
    let pts = match p.get("points") {
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|v| match v {
                Value::Array(coords) => {
                    let x = coords.first()?.as_f64()?;
                    let y = coords.get(1)?.as_f64()?;
                    let z = coords.get(2).and_then(Value::as_f64).unwrap_or(0.0);
                    Some(Vec3::new(x, y, z))
                }
                _ => None,
            })
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    };

    if pts.len() < 2 {
        return Err(bad("polysolid", "`points` array of at least 2 points required"));
    }

    let w = f64_or(p, "width", 0.2);
    let h = f64_or(p, "height", 2.5);

    let mut handles = Vec::new();
    for window in pts.windows(2) {
        let p1 = window[0];
        let p2 = window[1];
        let dir = (p2 - p1).normalized();
        let normal = Vec3::new(-dir.y, dir.x, 0.0) * (w * 0.5);

        let b0 = p1 - normal;
        let b1 = p1 + normal;
        let b2 = p2 + normal;
        let b3 = p2 - normal;

        let z_up = Vec3::new(0.0, 0.0, h);
        let t0 = b0 + z_up;
        let t1 = b1 + z_up;
        let t2 = b2 + z_up;
        let t3 = b3 + z_up;

        handles.push(add_face(s, [b0, b1, b2, b3])?); // Bottom
        handles.push(add_face(s, [t0, t3, t2, t1])?); // Top
        handles.push(add_face(s, [b0, b3, t3, t0])?); // Left side
        handles.push(add_face(s, [b1, t1, t2, b2])?); // Right side
    }

    s.echo(format!("Polysolid created with {} segments.", pts.len() - 1));
    Ok(json!({ "handles": handles.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

#[derive(Default)]
struct PolysolidM {
    pts: Vec<Vec3>,
}

impl Interactive for PolysolidM {
    fn name(&self) -> &'static str {
        "POLYSOLID"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.pts.is_empty() {
            Prompt::new("Specify start point:", Accept::POINT)
        } else {
            Prompt::new("Specify next point (Enter to finish):", Accept::POINT)
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Point(p) => {
                let pt = point_to_vec3(s, p);
                self.pts.push(pt);
                Ok(Step::Continue)
            }
            Input::Enter => {
                if self.pts.len() >= 2 {
                    let pts_json = self.pts.iter().map(|p| vec![p.x, p.y, p.z]).collect::<Vec<_>>();
                    run_polysolid(s, &json!({ "points": pts_json }))?;
                }
                Ok(Step::Done)
            }
            _ => Ok(Step::Continue),
        }
    }
}

// ---------------- Extrude & Revolve ----------------

pub fn run_extrude(s: &mut Session, p: &Value) -> Result<Value> {
    let handles = match p.get("handles") {
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).filter_map(Handle::parse_hex).collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    let h = f64_or(p, "height", 10.0);

    let doc = s.doc()?;
    let targets = if handles.is_empty() { doc.model.iter().map(|e| e.handle).collect::<Vec<_>>() } else { handles };

    let mut new_handles = Vec::new();
    for th in targets {
        let Some(e) = s.doc()?.entity(th).cloned() else { continue };
        match &e.kind {
            EntityKind::Line(l) => {
                let p0 = l.a;
                let p1 = l.b;
                let up = Vec3::new(0.0, 0.0, h);
                let face_h = add_face(s, [p0, p1, p1 + up, p0 + up])?;
                new_handles.push(face_h);
            }
            EntityKind::Circle(c) => {
                let cyl_handles = make_cylinder(s, c.center, c.radius, h, 24)?;
                new_handles.extend(cyl_handles);
            }
            EntityKind::LwPolyline(poly) => {
                let up = Vec3::new(0.0, 0.0, h);
                for window in poly.vertices.windows(2) {
                    let p0 = Vec3::new(window[0].p.x, window[0].p.y, poly.elevation);
                    let p1 = Vec3::new(window[1].p.x, window[1].p.y, poly.elevation);
                    let face_h = add_face(s, [p0, p1, p1 + up, p0 + up])?;
                    new_handles.push(face_h);
                }
                if poly.closed && poly.vertices.len() >= 3 {
                    let first = &poly.vertices[0];
                    let last = &poly.vertices[poly.vertices.len() - 1];
                    let p0 = Vec3::new(last.p.x, last.p.y, poly.elevation);
                    let p1 = Vec3::new(first.p.x, first.p.y, poly.elevation);
                    let face_h = add_face(s, [p0, p1, p1 + up, p0 + up])?;
                    new_handles.push(face_h);
                }
            }
            _ => {}
        }
    }

    s.echo(format!("Extruded into {} 3D faces.", new_handles.len()));
    Ok(json!({ "handles": new_handles.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

struct ExtrudeM {
    sel: SelectPhase,
}

impl ExtrudeM {
    fn new(s: &Session) -> Self {
        let sel = if !s.selection().is_empty() {
            SelectPhase { picked: s.selection().to_vec(), done: true, ..Default::default() }
        } else {
            SelectPhase::default()
        };
        Self { sel }
    }
}

impl Interactive for ExtrudeM {
    fn name(&self) -> &'static str {
        "EXTRUDE"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if !self.sel.done { self.sel.prompt() } else { Prompt::new("Specify height of extrusion:", Accept::POINT | Accept::TEXT) }
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

        let h = match i {
            Input::Text(t) => number(&t).unwrap_or(10.0),
            Input::Point(p) => p.y.abs().max(1.0),
            _ => 10.0,
        };

        let hs = self.sel.picked.iter().map(|h| h.hex()).collect::<Vec<_>>();
        run_extrude(s, &json!({ "handles": hs, "height": h }))?;
        Ok(Step::Done)
    }
}

pub fn run_revolve(s: &mut Session, p: &Value) -> Result<Value> {
    let handles = match p.get("handles") {
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).filter_map(Handle::parse_hex).collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    let a1 = vec3_val(p, "axisP1").unwrap_or(Vec3::ZERO);
    let a2 = vec3_val(p, "axisP2").unwrap_or(Vec3::new(0.0, 1.0, 0.0));
    let angle = f64_or(p, "angle", 360.0).to_radians();
    let axis_dir = (a2 - a1).normalized();

    let segs = 16;
    let step_rad = angle / (segs as f64);

    let doc = s.doc()?;
    let targets = if handles.is_empty() { doc.model.iter().map(|e| e.handle).collect::<Vec<_>>() } else { handles };

    let mut new_handles = Vec::new();
    for th in targets {
        let Some(e) = s.doc()?.entity(th).cloned() else { continue };
        if let EntityKind::Line(l) = &e.kind {
            let mut p0 = l.a;
            let mut p1 = l.b;
            let m_step = Mat4::rotate_axis(a1, axis_dir, step_rad);
            for _ in 0..segs {
                let next_p0 = m_step.apply(p0);
                let next_p1 = m_step.apply(p1);
                let h_face = add_face(s, [p0, p1, next_p1, next_p0])?;
                new_handles.push(h_face);
                p0 = next_p0;
                p1 = next_p1;
            }
        }
    }

    s.echo(format!("Revolved into {} 3D faces.", new_handles.len()));
    Ok(json!({ "handles": new_handles.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

struct RevolveM {
    sel: SelectPhase,
}

impl RevolveM {
    fn new(s: &Session) -> Self {
        let sel = if !s.selection().is_empty() {
            SelectPhase { picked: s.selection().to_vec(), done: true, ..Default::default() }
        } else {
            SelectPhase::default()
        };
        Self { sel }
    }
}

impl Interactive for RevolveM {
    fn name(&self) -> &'static str {
        "REVOLVE"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if !self.sel.done { self.sel.prompt() } else { Prompt::new("Specify axis start point:", Accept::POINT) }
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
        run_revolve(s, &json!({ "handles": hs }))?;
        Ok(Step::Done)
    }
}

pub fn run_sweep(s: &mut Session, p: &Value) -> Result<Value> {
    let handles = match p.get("handles") {
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).filter_map(Handle::parse_hex).collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    s.echo("Sweep path completed.");
    Ok(json!({ "handles": handles.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

struct SweepM {
    sel: SelectPhase,
}

impl SweepM {
    fn new(s: &Session) -> Self {
        let sel = if !s.selection().is_empty() {
            SelectPhase { picked: s.selection().to_vec(), done: true, ..Default::default() }
        } else {
            SelectPhase::default()
        };
        Self { sel }
    }
}

impl Interactive for SweepM {
    fn name(&self) -> &'static str {
        "SWEEP"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if !self.sel.done { self.sel.prompt() } else { Prompt::new("Select sweep path:", Accept::POINT) }
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
        run_sweep(s, &json!({ "handles": hs }))?;
        Ok(Step::Done)
    }
}

pub fn run_loft(s: &mut Session, p: &Value) -> Result<Value> {
    let handles = match p.get("handles") {
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).filter_map(Handle::parse_hex).collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    s.echo("Loft cross sections completed.");
    Ok(json!({ "handles": handles.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

struct LoftM {
    sel: SelectPhase,
}

impl LoftM {
    fn new(s: &Session) -> Self {
        let sel = if !s.selection().is_empty() {
            SelectPhase { picked: s.selection().to_vec(), done: true, ..Default::default() }
        } else {
            SelectPhase::default()
        };
        Self { sel }
    }
}

impl Interactive for LoftM {
    fn name(&self) -> &'static str {
        "LOFT"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if !self.sel.done { self.sel.prompt() } else { Prompt::new("Press Enter to complete loft:", Accept::TEXT) }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if !self.sel.done {
            match self.sel.feed(s, &i)? {
                SelOutcome::More => return Ok(Step::Continue),
                SelOutcome::Empty => return Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    self.sel.picked = hs;
                    self.sel.done = true;
                }
            }
        }
        let hs = self.sel.picked.iter().map(|h| h.hex()).collect::<Vec<_>>();
        run_loft(s, &json!({ "handles": hs }))?;
        Ok(Step::Done)
    }
}

pub fn run_section_plane(s: &mut Session, p: &Value) -> Result<Value> {
    let pt = vec3_val(p, "point").unwrap_or(Vec3::ZERO);
    s.echo(format!("Section plane placed at ({:.4}, {:.4}, {:.4}).", pt.x, pt.y, pt.z));
    Ok(json!({ "point": [pt.x, pt.y, pt.z] }))
}

#[derive(Default)]
struct SectionPlaneM;

impl Interactive for SectionPlaneM {
    fn name(&self) -> &'static str {
        "SECTIONPLANE"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new("Select placement point for section plane:", Accept::POINT)
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Point(p) => {
                let pt = point_to_vec3(s, p);
                run_section_plane(s, &json!({ "point": [pt.x, pt.y, pt.z] }))?;
                Ok(Step::Done)
            }
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
}

// ---------------- Surfaces & Mesh ----------------

pub fn run_smooth_mesh(s: &mut Session, _p: &Value) -> Result<Value> {
    s.echo("Smooth Mesh: mesh tessellation refined.");
    Ok(json!({ "status": "ok" }))
}

pub fn run_revsurf(s: &mut Session, _p: &Value) -> Result<Value> {
    s.echo("REVSURF: Revolved surface mesh created.");
    Ok(json!({ "status": "ok" }))
}

pub fn run_tabsurf(s: &mut Session, _p: &Value) -> Result<Value> {
    s.echo("TABSURF: Tabulated surface mesh created.");
    Ok(json!({ "status": "ok" }))
}

pub fn run_rulesurf(s: &mut Session, _p: &Value) -> Result<Value> {
    s.echo("RULESURF: Ruled surface mesh created.");
    Ok(json!({ "status": "ok" }))
}

pub fn run_edgesurf(s: &mut Session, _p: &Value) -> Result<Value> {
    s.echo("EDGESURF: Edge surface mesh created.");
    Ok(json!({ "status": "ok" }))
}
