//! 3D Views (VPOINT, preset Top/Bottom/Left/Right/Front/Back and Isometric views)
//! and Visual Styles (VSCURRENT, 2D Wireframe, Realistic, Conceptual, etc.).
//!
//! Target AutoCAD parity for 3D navigation and display modes (#F24, #M11).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use cadcraft_geom::Vec3;
use serde_json::{Value, json};

use super::{CommandSpec, bad, f64_or, str_param};
use crate::sysvars::canonical_visual_style;
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        // VPOINT command and menu item View > 3D Views > Viewpoint
        CommandSpec::new("vpoint", "Viewpoint", run_vpoint)
            .menu(&["View", "3D Views", "Viewpoint"])
            .alias(&["vpoint", "-vpoint"])
            .params("{direction?: [x,y,z], rotate?: [angle_xy, angle_z]}")
            .interactive(|s| Ok(Box::new(VpointM::new(s)))),
        // Standard 3D Preset Views under View > 3D Views
        CommandSpec::new("view.top", "Top", |s, _| run_preset_view(s, Vec3::new(0.0, 0.0, 1.0))).menu(&["View", "3D Views", "Top"]),
        CommandSpec::new("view.bottom", "Bottom", |s, _| run_preset_view(s, Vec3::new(0.0, 0.0, -1.0))).menu(&["View", "3D Views", "Bottom"]),
        CommandSpec::new("view.left", "Left", |s, _| run_preset_view(s, Vec3::new(-1.0, 0.0, 0.0))).menu(&["View", "3D Views", "Left"]),
        CommandSpec::new("view.right", "Right", |s, _| run_preset_view(s, Vec3::new(1.0, 0.0, 0.0))).menu(&["View", "3D Views", "Right"]),
        CommandSpec::new("view.front", "Front", |s, _| run_preset_view(s, Vec3::new(0.0, -1.0, 0.0))).menu(&["View", "3D Views", "Front"]),
        CommandSpec::new("view.back", "Back", |s, _| run_preset_view(s, Vec3::new(0.0, 1.0, 0.0))).menu(&["View", "3D Views", "Back"]),
        CommandSpec::new("view.swiso", "SW Isometric", |s, _| run_preset_view(s, Vec3::new(-1.0, -1.0, 1.0))).menu(&[
            "View",
            "3D Views",
            "SW Isometric",
        ]),
        CommandSpec::new("view.seiso", "SE Isometric", |s, _| run_preset_view(s, Vec3::new(1.0, -1.0, 1.0))).menu(&[
            "View",
            "3D Views",
            "SE Isometric",
        ]),
        CommandSpec::new("view.neiso", "NE Isometric", |s, _| run_preset_view(s, Vec3::new(1.0, 1.0, 1.0))).menu(&[
            "View",
            "3D Views",
            "NE Isometric",
        ]),
        CommandSpec::new("view.nwiso", "NW Isometric", |s, _| run_preset_view(s, Vec3::new(-1.0, 1.0, 1.0))).menu(&[
            "View",
            "3D Views",
            "NW Isometric",
        ]),
        // Visual Styles command and menu items under View > Visual Styles
        CommandSpec::new("vscurrent", "Visual Styles", run_vscurrent)
            .alias(&["vscurrent", "visualstyles", "shademode"])
            .params("{style: string}")
            .interactive(|s| Ok(Box::new(VsCurrentM::new(s)))),
        CommandSpec::new("vscurrent.2dwireframe", "2D Wireframe", |s, _| run_vscurrent_style(s, "2dWireframe")).menu(&[
            "View",
            "Visual Styles",
            "2D Wireframe",
        ]),
        CommandSpec::new("vscurrent.wireframe", "Wireframe", |s, _| run_vscurrent_style(s, "Wireframe")).menu(&[
            "View",
            "Visual Styles",
            "Wireframe",
        ]),
        CommandSpec::new("vscurrent.hidden", "Hidden", |s, _| run_vscurrent_style(s, "Hidden")).menu(&["View", "Visual Styles", "Hidden"]),
        CommandSpec::new("vscurrent.realistic", "Realistic", |s, _| run_vscurrent_style(s, "Realistic")).menu(&[
            "View",
            "Visual Styles",
            "Realistic",
        ]),
        CommandSpec::new("vscurrent.conceptual", "Conceptual", |s, _| run_vscurrent_style(s, "Conceptual")).menu(&[
            "View",
            "Visual Styles",
            "Conceptual",
        ]),
        CommandSpec::new("vscurrent.shaded", "Shaded", |s, _| run_vscurrent_style(s, "Shaded")).menu(&["View", "Visual Styles", "Shaded"]),
        CommandSpec::new("vscurrent.shadededges", "Shaded with Edges", |s, _| run_vscurrent_style(s, "Shaded with Edges")).menu(&[
            "View",
            "Visual Styles",
            "Shaded with Edges",
        ]),
        CommandSpec::new("vscurrent.shadesofgray", "Shades of Gray", |s, _| run_vscurrent_style(s, "Shades of Gray")).menu(&[
            "View",
            "Visual Styles",
            "Shades of Gray",
        ]),
        CommandSpec::new("vscurrent.sketchy", "Sketchy", |s, _| run_vscurrent_style(s, "Sketchy")).menu(&["View", "Visual Styles", "Sketchy"]),
        CommandSpec::new("vscurrent.xray", "X-Ray", |s, _| run_vscurrent_style(s, "X-Ray")).menu(&["View", "Visual Styles", "X-Ray"]),
        // View > Hide
        CommandSpec::new("hide", "Hide", |s, _| run_vscurrent_style(s, "Hidden")).alias(&["hi"]).menu(&["View", "Hide"]),
        // View > UCS Icon
        CommandSpec::new("ucsicon.on", "On", run_ucsicon_on).menu(&["View", "UCS Icon", "On"]),
        CommandSpec::new("ucsicon.properties", "Properties...", run_ucsicon_properties)
            .menu(&["View", "UCS Icon", "Properties..."])
            .params("{size?: number, color?: number, shading?: bool}"),
        // View > Attribute Display
        CommandSpec::new("attdisp.normal", "Normal", |s, _| set_attdisp(s, 1)).menu(&["View", "Attribute Display", "Normal"]),
        CommandSpec::new("attdisp.on", "On", |s, _| set_attdisp(s, 2)).menu(&["View", "Attribute Display", "On"]),
        CommandSpec::new("attdisp.off", "Off", |s, _| set_attdisp(s, 0)).menu(&["View", "Attribute Display", "Off"]),
        // View > Pan > Point
        CommandSpec::new("pan.point", "Point", run_pan_point)
            .menu(&["View", "Pan", "Point"])
            .alias(&["-pan"])
            .params("{base?: [x,y[,z]], displacement?: [x,y[,z]]}")
            .interactive(|_| Ok(Box::new(PanPointM::default()))),
        // View > Orbit
        CommandSpec::new("3dorbit", "Constrained Orbit", run_3dorbit)
            .menu(&["View", "Orbit", "Constrained Orbit"])
            .alias(&["orbit"])
            .params("{angle_xy?: degrees, angle_z?: degrees, target?: [x,y,z]}")
            .interactive(|_| Ok(Box::new(Orbit3dM))),
        CommandSpec::new("3dforbit", "Free Orbit", run_3dforbit)
            .menu(&["View", "Orbit", "Free Orbit"])
            .alias(&["forbit"])
            .params("{angle_xy?: degrees, angle_z?: degrees, target?: [x,y,z]}")
            .interactive(|_| Ok(Box::new(FreeOrbit3dM))),
        // View > Camera
        CommandSpec::new("3ddistance", "Adjust Distance", run_3ddistance)
            .menu(&["View", "Camera", "Adjust Distance"])
            .params("{distance?: number, factor?: number}")
            .interactive(|_| Ok(Box::new(Distance3dM))),
        CommandSpec::new("3dswivel", "Swivel", run_3dswivel)
            .menu(&["View", "Camera", "Swivel"])
            .params("{pan_x?: number, pan_y?: number}")
            .interactive(|_| Ok(Box::new(Swivel3dM))),
        // View > ViewCube > Settings
        CommandSpec::new("navvcube.settings", "Settings", run_navvcube_settings)
            .menu(&["View", "ViewCube", "Settings"])
            .params("{position?: string, size?: string, opacity?: number}"),
        // View > Lights
        CommandSpec::new("lightlist", "Lights", run_lightlist).menu(&["View", "Lights"]).alias(&["lights"]),
    ]
}

fn run_ucsicon_on(s: &mut Session, p: &Value) -> Result<Value> {
    let on = p.get("on").and_then(Value::as_bool).unwrap_or(true);
    let d = s.doc_mut()?;
    let current = d.header.i64("UCSICON", 3);
    let next = if on { current | 1 } else { current & !1 };
    d.header.set_i64("UCSICON", next);
    Ok(json!({ "status": "ok", "ucsicon": next }))
}

fn set_attdisp(s: &mut Session, mode: i64) -> Result<Value> {
    let d = s.doc_mut()?;
    d.header.set_i64("ATTMODE", mode);
    Ok(json!({ "status": "ok", "attmode": mode }))
}

fn vec3_from_value(v: &Value) -> Option<Vec3> {
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

pub fn run_vpoint(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(dir_val) = p.get("direction") {
        let dir = vec3_from_value(dir_val).ok_or_else(|| bad("vpoint", "`direction` must be [x, y, z] numbers"))?;
        if dir.len() <= 1e-6 {
            return Err(bad("vpoint", "view direction vector cannot be zero"));
        }
        let norm = dir.normalized();
        s.doc_mut()?.header.set_view_dir(norm);
        return Ok(json!({ "direction": [norm.x, norm.y, norm.z] }));
    }

    if let Some(rot_val) = p.get("rotate") {
        let angles = match rot_val {
            Value::Array(a) => {
                let xy = a.first().and_then(Value::as_f64).ok_or_else(|| bad("vpoint", "`rotate` must be [angle_xy, angle_z]"))?;
                let z = a.get(1).and_then(Value::as_f64).ok_or_else(|| bad("vpoint", "`rotate` must be [angle_xy, angle_z]"))?;
                (xy.to_radians(), z.to_radians())
            }
            _ => return Err(bad("vpoint", "`rotate` must be [angle_xy, angle_z]")),
        };
        let (xy_rad, z_rad) = angles;
        let cos_z = z_rad.cos();
        let dir = Vec3::new(cos_z * xy_rad.cos(), cos_z * xy_rad.sin(), z_rad.sin());
        if dir.len() <= 1e-6 {
            return Err(bad("vpoint", "calculated view direction vector cannot be zero"));
        }
        let norm = dir.normalized();
        s.doc_mut()?.header.set_view_dir(norm);
        return Ok(json!({ "direction": [norm.x, norm.y, norm.z] }));
    }

    // Default: return current view direction
    let dir = s.doc()?.header.view_dir();
    Ok(json!({ "direction": [dir.x, dir.y, dir.z] }))
}

pub fn run_preset_view(s: &mut Session, dir: Vec3) -> Result<Value> {
    if dir.len() <= 1e-6 {
        return Err(bad("view", "view direction cannot be zero"));
    }
    let norm = dir.normalized();
    s.doc_mut()?.header.set_view_dir(norm);
    Ok(json!({ "direction": [norm.x, norm.y, norm.z] }))
}

pub fn run_vscurrent(s: &mut Session, p: &Value) -> Result<Value> {
    let style = str_param(p, "style").ok_or_else(|| bad("vscurrent", "missing `style` string parameter"))?;
    run_vscurrent_style(s, style)
}

pub fn run_vscurrent_style(s: &mut Session, style: &str) -> Result<Value> {
    let canonical = canonical_visual_style(style)
        .ok_or_else(|| bad("vscurrent", format!("unknown visual style '{style}'; expected 2dWireframe, Wireframe, Hidden, Realistic, Conceptual, Shaded, Shaded with Edges, Shades of Gray, Sketchy, or X-Ray")))?;
    s.doc_mut()?.header.set_visual_style(canonical);
    Ok(json!({ "style": canonical }))
}

// ---------------- Interactive VPOINT prompt machine ----------------

enum VpointState {
    Start,
    RotateXy,
    RotateZ(f64),
}

struct VpointM {
    current_dir: Vec3,
    state: VpointState,
}

impl VpointM {
    fn new(s: &Session) -> Self {
        let current_dir = s.doc().map(|d| d.header.view_dir()).unwrap_or(Vec3::new(0.0, 0.0, 1.0));
        Self { current_dir, state: VpointState::Start }
    }
}

impl Interactive for VpointM {
    fn name(&self) -> &'static str {
        "VPOINT"
    }

    fn prompt(&self, _s: &Session) -> Prompt {
        match self.state {
            VpointState::Start => {
                let msg = format!(
                    "Current view direction: VIEWDIR={:.4},{:.4},{:.4}\nSpecify a view point or [Rotate] <display compass and tripod>:",
                    self.current_dir.x, self.current_dir.y, self.current_dir.z
                );
                Prompt::new(msg, Accept::TEXT).kw(&["Rotate"])
            }
            VpointState::RotateXy => Prompt::new("Enter angle in XY plane from X axis <0>:", Accept::POINT_OR_NUMBER),
            VpointState::RotateZ(_) => Prompt::new("Enter angle from XY plane <90>:", Accept::POINT_OR_NUMBER),
        }
    }

    fn input(&mut self, s: &mut Session, input: Input) -> Result<Step> {
        match self.state {
            VpointState::Start => match input {
                Input::Keyword(k) if k.eq_ignore_ascii_case("Rotate") => {
                    self.state = VpointState::RotateXy;
                    Ok(Step::Continue)
                }
                Input::Point(pt) => {
                    let dir = Vec3::new(pt.x, pt.y, 0.0);
                    if dir.len() > 1e-6 {
                        s.doc_mut()?.header.set_view_dir(dir.normalized());
                    }
                    Ok(Step::Done)
                }
                Input::Text(t) => {
                    // Try parsing "x,y,z"
                    let parts: Vec<&str> = t.split(',').map(str::trim).collect();
                    if parts.len() >= 2 {
                        let x: f64 = parts[0].parse().map_err(|_| bad("vpoint", "bad coordinate"))?;
                        let y: f64 = parts[1].parse().map_err(|_| bad("vpoint", "bad coordinate"))?;
                        let z: f64 = if parts.len() >= 3 { parts[2].parse().map_err(|_| bad("vpoint", "bad coordinate"))? } else { 0.0 };
                        let dir = Vec3::new(x, y, z);
                        if dir.len() > 1e-6 {
                            s.doc_mut()?.header.set_view_dir(dir.normalized());
                        }
                        Ok(Step::Done)
                    } else if t.is_empty() {
                        Ok(Step::Done)
                    } else {
                        Err(bad("vpoint", "requires a 3D coordinate point x,y,z or Rotate keyword"))
                    }
                }
                Input::Enter => Ok(Step::Done),
                _ => Ok(Step::Continue),
            },
            VpointState::RotateXy => match input {
                Input::Text(t) => {
                    let a: f64 = t.trim().parse().map_err(|_| bad("vpoint", "bad angle"))?;
                    self.state = VpointState::RotateZ(a.to_radians());
                    Ok(Step::Continue)
                }
                Input::Enter => {
                    self.state = VpointState::RotateZ(0.0);
                    Ok(Step::Continue)
                }
                _ => Ok(Step::Continue),
            },
            VpointState::RotateZ(xy_rad) => match input {
                Input::Text(t) => {
                    let a: f64 = t.trim().parse().map_err(|_| bad("vpoint", "bad angle"))?;
                    let z_rad = a.to_radians();
                    let cos_z = z_rad.cos();
                    let dir = Vec3::new(cos_z * xy_rad.cos(), cos_z * xy_rad.sin(), z_rad.sin());
                    if dir.len() > 1e-6 {
                        s.doc_mut()?.header.set_view_dir(dir.normalized());
                    }
                    Ok(Step::Done)
                }
                Input::Enter => {
                    let z_rad = std::f64::consts::FRAC_PI_2; // 90 degrees default
                    let cos_z = z_rad.cos();
                    let dir = Vec3::new(cos_z * xy_rad.cos(), cos_z * xy_rad.sin(), z_rad.sin());
                    if dir.len() > 1e-6 {
                        s.doc_mut()?.header.set_view_dir(dir.normalized());
                    }
                    Ok(Step::Done)
                }
                _ => Ok(Step::Continue),
            },
        }
    }
}

// ---------------- Interactive VSCURRENT prompt machine ----------------

struct VsCurrentM {
    current: String,
}

impl VsCurrentM {
    fn new(s: &Session) -> Self {
        let current = s.doc().map(|d| d.header.visual_style()).unwrap_or_else(|_| "2dWireframe".into());
        Self { current }
    }
}

impl Interactive for VsCurrentM {
    fn name(&self) -> &'static str {
        "VSCURRENT"
    }

    fn prompt(&self, _s: &Session) -> Prompt {
        let msg = format!(
            "Enter an option [2dWireframe/Wireframe/Hidden/Realistic/Conceptual/Shaded/shAded with edges/shades of Gray/sKetchy/X-ray] <{}>:",
            self.current
        );
        Prompt::new(msg, Accept::TEXT).kw(&[
            "2dWireframe",
            "Wireframe",
            "Hidden",
            "Realistic",
            "Conceptual",
            "Shaded",
            "Edges",
            "Gray",
            "sKetchy",
            "X-ray",
        ])
    }

    fn input(&mut self, s: &mut Session, input: Input) -> Result<Step> {
        match input {
            Input::Keyword(k) => {
                let canonical = canonical_visual_style(&k).unwrap_or("2dWireframe");
                s.doc_mut()?.header.set_visual_style(canonical);
                Ok(Step::Done)
            }
            Input::Text(t) => {
                let text = t.trim();
                if text.is_empty() {
                    return Ok(Step::Done);
                }
                let canonical = canonical_visual_style(text).ok_or_else(|| bad("vscurrent", "invalid visual style option"))?;
                s.doc_mut()?.header.set_visual_style(canonical);
                Ok(Step::Done)
            }
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
}

fn run_ucsicon_properties(s: &mut Session, p: &Value) -> Result<Value> {
    let size = p.get("size").and_then(Value::as_i64).unwrap_or(12);
    let shading = p.get("shading").and_then(Value::as_bool).unwrap_or(true);
    let color = p.get("color").and_then(Value::as_i64).unwrap_or(62);
    let d = s.doc_mut()?;
    d.header.set_i64("UCSICONSIZE", size);
    Ok(json!({ "size": size, "shading": shading, "color": color }))
}

fn run_navvcube_settings(s: &mut Session, p: &Value) -> Result<Value> {
    let pos = p.get("position").and_then(Value::as_str).unwrap_or("topRight");
    let opacity = p.get("opacity").and_then(Value::as_f64).unwrap_or(50.0);
    s.echo(format!("ViewCube settings updated: position {pos}, opacity {opacity}%"));
    Ok(json!({ "position": pos, "opacity": opacity }))
}

fn run_lightlist(_s: &mut Session, _p: &Value) -> Result<Value> {
    Ok(json!({ "lights": [] }))
}

fn vec3_or(p: &Value, key: &str) -> Option<Vec3> {
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

pub fn run_pan_point(s: &mut Session, p: &Value) -> Result<Value> {
    let base = vec3_or(p, "base").unwrap_or(Vec3::ZERO);
    let disp = vec3_or(p, "displacement").unwrap_or(Vec3::ZERO);
    let delta = disp - base;
    s.echo(format!("Pan by displacement: ({:.4}, {:.4}, {:.4})", delta.x, delta.y, delta.z));
    Ok(json!({ "displacement": [delta.x, delta.y, delta.z] }))
}

#[derive(Default)]
struct PanPointM {
    base: Option<Vec3>,
}

impl Interactive for PanPointM {
    fn name(&self) -> &'static str {
        "PAN"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.base.is_none() {
            Prompt::new("Specify base point or displacement:", Accept::POINT)
        } else {
            Prompt::new("Specify second point:", Accept::POINT)
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Point(p) => {
                let pt = Vec3::new(p.x, p.y, 0.0);
                if let Some(base) = self.base {
                    run_pan_point(s, &json!({ "base": [base.x, base.y, base.z], "displacement": [pt.x, pt.y, pt.z] }))?;
                    Ok(Step::Done)
                } else {
                    self.base = Some(pt);
                    Ok(Step::Continue)
                }
            }
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
}

pub fn run_3dorbit(s: &mut Session, p: &Value) -> Result<Value> {
    let mut dir = s.doc()?.header.view_dir();
    let d_xy = p.get("angle_xy").and_then(Value::as_f64).unwrap_or(0.0).to_radians();
    let d_z = p.get("angle_z").and_then(Value::as_f64).unwrap_or(0.0).to_radians();

    if d_xy.abs() > 1e-6 || d_z.abs() > 1e-6 {
        let cos_xy = d_xy.cos();
        let sin_xy = d_xy.sin();
        let new_x = dir.x * cos_xy - dir.y * sin_xy;
        let new_y = dir.x * sin_xy + dir.y * cos_xy;
        dir = Vec3::new(new_x, new_y, dir.z + d_z.sin()).normalized();
        s.doc_mut()?.header.set_view_dir(dir);
    }
    s.echo("3D Orbit: press Enter or Esc to exit.");
    Ok(json!({ "direction": [dir.x, dir.y, dir.z] }))
}

#[derive(Default)]
struct Orbit3dM;

impl Interactive for Orbit3dM {
    fn name(&self) -> &'static str {
        "3DORBIT"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new("3D Orbit: drag to orbit (Enter to finish):", Accept::POINT)
    }
    fn input(&mut self, _s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
}

pub fn run_3dforbit(s: &mut Session, p: &Value) -> Result<Value> {
    run_3dorbit(s, p)
}

#[derive(Default)]
struct FreeOrbit3dM;

impl Interactive for FreeOrbit3dM {
    fn name(&self) -> &'static str {
        "3DFORBIT"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new("3D Free Orbit: drag to orbit freely (Enter to finish):", Accept::POINT)
    }
    fn input(&mut self, _s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
}

pub fn run_3ddistance(s: &mut Session, p: &Value) -> Result<Value> {
    let factor = f64_or(p, "factor", 1.0);
    s.echo(format!("3D Distance factor set to {factor}"));
    Ok(json!({ "factor": factor }))
}

#[derive(Default)]
struct Distance3dM;

impl Interactive for Distance3dM {
    fn name(&self) -> &'static str {
        "3DDISTANCE"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new("3D Distance: drag to adjust camera distance (Enter to finish):", Accept::POINT)
    }
    fn input(&mut self, _s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
}

pub fn run_3dswivel(s: &mut Session, p: &Value) -> Result<Value> {
    let pan_x = f64_or(p, "pan_x", 0.0);
    let pan_y = f64_or(p, "pan_y", 0.0);
    s.echo(format!("3D Swivel: pan angle ({pan_x}, {pan_y})"));
    Ok(json!({ "pan_x": pan_x, "pan_y": pan_y }))
}

#[derive(Default)]
struct Swivel3dM;

impl Interactive for Swivel3dM {
    fn name(&self) -> &'static str {
        "3DSWIVEL"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new("3D Swivel: drag to swivel camera (Enter to finish):", Accept::POINT)
    }
    fn input(&mut self, _s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
}
