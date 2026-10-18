use cadcraft_doc::{Handle, Ucs};
use cadcraft_geom::Vec3;
use serde_json::{Value, json};

use super::{CommandSpec, bad, f64_or, f64_req, str_param};
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("ucs", "UCS", run_ucs)
            .alias(&["ucs", "-ucs"])
            .params("{world?: bool, origin?: [x,y[,z]], xAxis?: [x,y[,z]], yAxis?: [x,y[,z]], zAxis?: [x,y[,z]], rotate?: \"x\"|\"y\"|\"z\", angle?: degrees, view?: bool, previous?: bool, save?: string, restore?: string, delete?: string, list?: bool}")
            .interactive(|_| Ok(Box::new(UcsM::default()))),
        CommandSpec::new("ucs.world", "World", |s, _| run_ucs_world(s))
            .menu(&["Tools", "New UCS", "World"]),
        CommandSpec::new("ucs.origin", "Origin", run_ucs_origin)
            .menu(&["Tools", "New UCS", "Origin"])
            .params("{origin: [x,y[,z]]}")
            .interactive(|_| Ok(Box::new(UcsOriginM))),
        CommandSpec::new("ucs.3point", "3 Point", run_ucs_3point)
            .menu(&["Tools", "New UCS", "3 Point"])
            .params("{origin: [x,y[,z]], xAxis: [x,y[,z]], yAxis: [x,y[,z]]}")
            .interactive(|_| Ok(Box::new(Ucs3PointM::default()))),
        CommandSpec::new("ucs.zaxis", "Z Axis Vector", run_ucs_zaxis)
            .menu(&["Tools", "New UCS", "Z Axis Vector"])
            .params("{origin: [x,y[,z]], zAxis: [x,y[,z]]}")
            .interactive(|_| Ok(Box::new(UcsZAxisM::default()))),
        CommandSpec::new("ucs.view", "View", |s, _| run_ucs_view(s))
            .menu(&["Tools", "New UCS", "View"]),
        CommandSpec::new("ucs.x", "X", |s, p| run_ucs_rotate_axis(s, 'x', p))
            .menu(&["Tools", "New UCS", "X"])
            .params("{angle: degrees}")
            .interactive(|_| Ok(Box::new(UcsRotateM { axis: 'x' }))),
        CommandSpec::new("ucs.y", "Y", |s, p| run_ucs_rotate_axis(s, 'y', p))
            .menu(&["Tools", "New UCS", "Y"])
            .params("{angle: degrees}")
            .interactive(|_| Ok(Box::new(UcsRotateM { axis: 'y' }))),
        CommandSpec::new("ucs.z", "Z", |s, p| run_ucs_rotate_axis(s, 'z', p))
            .menu(&["Tools", "New UCS", "Z"])
            .params("{angle: degrees}")
            .interactive(|_| Ok(Box::new(UcsRotateM { axis: 'z' }))),
        CommandSpec::new("ucs.previous", "Previous", |s, _| run_ucs_previous(s)),
        CommandSpec::new("ucsman", "Named UCS...", run_ucsman)
            .menu(&["Tools", "Named UCS..."])
            .alias(&["ucsman"]),
        CommandSpec::new("plan", "Plan View", run_plan)
            .menu(&["View", "3D Views", "Plan View", "Current UCS"])
            .alias(&["plan"])
            .params("{mode?: \"current\"|\"world\"|\"ucs\", name?: string}")
            .interactive(|_| Ok(Box::new(PlanM::default()))),
        CommandSpec::new("plan.current", "Current UCS", |s, _| run_plan_mode(s, "current"))
            .menu(&["View", "3D Views", "Plan View", "Current UCS"]),
        CommandSpec::new("plan.world", "World UCS", |s, _| run_plan_mode(s, "world"))
            .menu(&["View", "3D Views", "Plan View", "World UCS"]),
        CommandSpec::new("plan.named", "Named UCS...", run_plan_named)
            .menu(&["View", "3D Views", "Plan View", "Named UCS"])
            .params("{name: string}"),
        CommandSpec::new("ucs.face", "Face", run_ucs_face)
            .menu(&["Tools", "New UCS", "Face"])
            .alias(&["ucsface"])
            .params("{handle?: hex, point?: [x,y,z]}")
            .interactive(|_| Ok(Box::new(UcsFaceM))),
        CommandSpec::new("elevation", "Elevation", run_elevation)
            .params("{elevation?: number, thickness?: number}")
            .interactive(|s| Ok(Box::new(ElevationM::new(s)))),
        CommandSpec::new("thickness", "Thickness", run_thickness)
            .menu(&["Format", "Thickness"])
            .params("{thickness: number}")
            .interactive(|s| Ok(Box::new(ThicknessM::new(s)))),
        CommandSpec::new("transparency", "Transparency", run_transparency)
            .menu(&["Format", "Transparency"])
            .alias(&["cetransparency"])
            .params("{value?: number | \"ByLayer\" | \"ByBlock\"}")
            .interactive(|s| Ok(Box::new(TransparencyM::new(s)))),
    ]
}

fn vec3_param(p: &Value, key: &str) -> Option<Vec3> {
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

pub fn run_ucs_world(s: &mut Session) -> Result<Value> {
    s.set_current_ucs(Ucs::world())?;
    s.echo("UCS set to World");
    Ok(json!({ "name": "", "origin": [0.0, 0.0, 0.0] }))
}

pub fn run_ucs_origin(s: &mut Session, p: &Value) -> Result<Value> {
    let org = vec3_param(p, "origin").ok_or_else(|| bad("ucs", "`origin` [x,y,z] required"))?;
    let mut cur = s.current_ucs();
    cur.name.clear();
    cur.origin = org;
    s.set_current_ucs(cur)?;
    Ok(json!({ "origin": [org.x, org.y, org.z] }))
}

pub fn run_ucs_3point(s: &mut Session, p: &Value) -> Result<Value> {
    let origin = vec3_param(p, "origin").unwrap_or(Vec3::ZERO);
    let px = vec3_param(p, "xAxis").ok_or_else(|| bad("ucs", "`xAxis` point on positive X axis required"))?;
    let py = vec3_param(p, "yAxis").ok_or_else(|| bad("ucs", "`yAxis` point on positive Y portion required"))?;

    let x_raw = px - origin;
    if x_raw.len() < 1e-9 {
        return Err(bad("ucs", "X-axis point must be distinct from origin"));
    }
    let x_axis = x_raw.normalized();
    let vy = py - origin;
    let z_raw = x_axis.cross(vy);
    if z_raw.len() < 1e-9 {
        return Err(bad("ucs", "Points are collinear"));
    }
    let z_axis = z_raw.normalized();
    let y_axis = z_axis.cross(x_axis);

    let ucs = Ucs { name: String::new(), origin, x_axis, y_axis };
    s.set_current_ucs(ucs)?;
    Ok(json!({ "origin": [origin.x, origin.y, origin.z] }))
}

pub fn run_ucs_zaxis(s: &mut Session, p: &Value) -> Result<Value> {
    let origin = vec3_param(p, "origin").unwrap_or(Vec3::ZERO);
    let pz = vec3_param(p, "zAxis").ok_or_else(|| bad("ucs", "`zAxis` point on positive Z axis required"))?;
    let z_raw = pz - origin;
    if z_raw.len() < 1e-9 {
        return Err(bad("ucs", "Z-axis point must be distinct from origin"));
    }
    let z_axis = z_raw.normalized();

    // AutoCAD Arbitrary Axis Algorithm
    let ax = if z_axis.x.abs() < (1.0 / 64.0) && z_axis.y.abs() < (1.0 / 64.0) {
        Vec3::new(0.0, 1.0, 0.0).cross(z_axis)
    } else {
        Vec3::new(0.0, 0.0, 1.0).cross(z_axis)
    };
    let x_axis = ax.normalized();
    let y_axis = z_axis.cross(x_axis);

    let ucs = Ucs { name: String::new(), origin, x_axis, y_axis };
    s.set_current_ucs(ucs)?;
    Ok(json!({ "origin": [origin.x, origin.y, origin.z] }))
}

pub fn run_ucs_view(s: &mut Session) -> Result<Value> {
    let mut cur = s.current_ucs();
    cur.name.clear();
    cur.x_axis = Vec3::new(1.0, 0.0, 0.0);
    cur.y_axis = Vec3::new(0.0, 1.0, 0.0);
    s.set_current_ucs(cur)?;
    s.echo("UCS plane set parallel to view");
    Ok(json!({ "view": true }))
}

fn rotate_ucs(cur: &Ucs, axis: char, angle_deg: f64) -> Ucs {
    let rad = angle_deg.to_radians();
    let cos = rad.cos();
    let sin = rad.sin();
    let (mut x, mut y) = (cur.x_axis, cur.y_axis);
    let z = cur.z_axis();

    match axis {
        'x' | 'X' => {
            y = y * cos + z * sin;
        }
        'y' | 'Y' => {
            x = x * cos - z * sin;
        }
        _ => {
            let nx = x * cos + y * sin;
            let ny = -x * sin + y * cos;
            x = nx;
            y = ny;
        }
    }
    Ucs { name: String::new(), origin: cur.origin, x_axis: x.normalized(), y_axis: y.normalized() }
}

pub fn run_ucs_rotate_axis(s: &mut Session, axis: char, p: &Value) -> Result<Value> {
    let angle = f64_or(p, "angle", 0.0);
    let cur = s.current_ucs();
    let next = rotate_ucs(&cur, axis, angle);
    s.set_current_ucs(next)?;
    Ok(json!({ "rotated": axis.to_string(), "angle": angle }))
}

pub fn run_ucs_previous(s: &mut Session) -> Result<Value> {
    let prev = s.state_mut()?.ucs_history.pop();
    match prev {
        Some(u) => {
            s.doc_mut()?.header.set_ucs(&u);
            s.touch();
            s.echo("Restored previous UCS");
            Ok(json!({ "name": u.name, "origin": [u.origin.x, u.origin.y, u.origin.z] }))
        }
        None => Err(bad("ucs", "No previous UCS in history")),
    }
}

pub fn run_ucs_save(s: &mut Session, name: &str) -> Result<Value> {
    let name = name.trim();
    if name.is_empty() {
        return Err(bad("ucs", "UCS name cannot be empty"));
    }
    let mut ucs = s.current_ucs();
    ucs.name = name.to_string();
    let d = s.doc_mut()?;
    if let Some(existing) = d.ucss.iter_mut().find(|u| u.name.eq_ignore_ascii_case(name)) {
        *existing = ucs.clone();
    } else {
        d.ucss.push(ucs.clone());
    }
    d.header.set_ucs(&ucs);
    s.touch();
    s.echo(format!("UCS saved as \"{name}\""));
    Ok(json!({ "saved": name }))
}

pub fn run_ucs_restore(s: &mut Session, name: &str) -> Result<Value> {
    let name = name.trim();
    let found = s.doc()?.ucss.iter().find(|u| u.name.eq_ignore_ascii_case(name)).cloned();
    match found {
        Some(u) => {
            s.set_current_ucs(u.clone())?;
            s.echo(format!("Restored UCS \"{}\"", u.name));
            Ok(json!({ "restored": u.name }))
        }
        None => Err(bad("ucs", format!("UCS \"{name}\" not found"))),
    }
}

pub fn run_ucs_delete(s: &mut Session, name: &str) -> Result<Value> {
    let name = name.trim();
    let d = s.doc_mut()?;
    let before = d.ucss.len();
    d.ucss.retain(|u| !u.name.eq_ignore_ascii_case(name));
    if d.ucss.len() == before {
        return Err(bad("ucs", format!("UCS \"{name}\" not found")));
    }
    if d.header.str("UCSNAME", "").eq_ignore_ascii_case(name) {
        d.header.set_str("UCSNAME", "");
    }
    s.touch();
    s.echo(format!("Deleted UCS \"{name}\""));
    Ok(json!({ "deleted": name }))
}

pub fn run_ucs_list(s: &mut Session) -> Result<Value> {
    let ucss = &s.doc()?.ucss;
    let names: Vec<String> = ucss.iter().map(|u| u.name.clone()).collect();
    if names.is_empty() {
        s.echo("No named coordinate systems defined.");
    } else {
        s.echo(format!("Named UCSs: {}", names.join(", ")));
    }
    Ok(json!({ "ucss": names }))
}

pub fn run_ucs(s: &mut Session, p: &Value) -> Result<Value> {
    if p.get("world").and_then(Value::as_bool) == Some(true) {
        return run_ucs_world(s);
    }
    if p.get("previous").and_then(Value::as_bool) == Some(true) {
        return run_ucs_previous(s);
    }
    if p.get("view").and_then(Value::as_bool) == Some(true) {
        return run_ucs_view(s);
    }
    if p.get("list").and_then(Value::as_bool) == Some(true) {
        return run_ucs_list(s);
    }
    if let Some(name) = str_param(p, "save") {
        return run_ucs_save(s, name);
    }
    if let Some(name) = str_param(p, "restore") {
        return run_ucs_restore(s, name);
    }
    if let Some(name) = str_param(p, "delete") {
        return run_ucs_delete(s, name);
    }
    if let Some(axis_str) = str_param(p, "rotate") {
        let axis = axis_str.chars().next().unwrap_or('z');
        return run_ucs_rotate_axis(s, axis, p);
    }
    if p.get("xAxis").is_some() || p.get("yAxis").is_some() {
        return run_ucs_3point(s, p);
    }
    if p.get("zAxis").is_some() {
        return run_ucs_zaxis(s, p);
    }
    if p.get("origin").is_some() {
        return run_ucs_origin(s, p);
    }
    run_ucs_world(s)
}

pub fn run_ucsman(s: &mut Session, _p: &Value) -> Result<Value> {
    run_ucs_list(s)
}

// ----------------------------------------------------------------------------
// PLAN command
// ----------------------------------------------------------------------------

pub fn run_plan_mode(s: &mut Session, mode: &str) -> Result<Value> {
    let mode = mode.trim().to_ascii_lowercase();
    match mode.as_str() {
        "world" | "w" => {
            s.touch();
            s.echo("Regenerating model (WCS plan view).");
            Ok(json!({ "plan": "world" }))
        }
        "current" | "c" => {
            s.touch();
            s.echo("Regenerating model (Current UCS plan view).");
            Ok(json!({ "plan": "current" }))
        }
        _ => Err(bad("plan", "unknown option")),
    }
}

pub fn run_plan_named(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("plan", "`name` required"))?;
    let found = s.doc()?.ucss.iter().any(|u| u.name.eq_ignore_ascii_case(name));
    if !found {
        return Err(bad("plan", format!("UCS \"{name}\" not found")));
    }
    s.touch();
    s.echo(format!("Regenerating model (UCS \"{name}\" plan view)."));
    Ok(json!({ "plan": name }))
}

pub fn run_plan(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(name) = str_param(p, "name") {
        return run_plan_named(s, &json!({ "name": name }));
    }
    let mode = str_param(p, "mode").unwrap_or("current");
    run_plan_mode(s, mode)
}

// ----------------------------------------------------------------------------
// ELEVATION & THICKNESS commands
// ----------------------------------------------------------------------------

pub fn run_elevation(s: &mut Session, p: &Value) -> Result<Value> {
    let elev = p.get("elevation").and_then(Value::as_f64);
    let thick = p.get("thickness").and_then(Value::as_f64);
    let d = s.doc_mut()?;
    if let Some(e) = elev {
        d.header.set_f64("ELEVATION", e);
    }
    if let Some(t) = thick {
        d.header.set_f64("THICKNESS", t);
    }
    s.touch();
    Ok(json!({
        "elevation": s.doc()?.header.f64("ELEVATION", 0.0),
        "thickness": s.doc()?.header.f64("THICKNESS", 0.0),
    }))
}

pub fn run_thickness(s: &mut Session, p: &Value) -> Result<Value> {
    let thick = f64_req("thickness", p, "thickness")?;
    s.doc_mut()?.header.set_f64("THICKNESS", thick);
    s.touch();
    Ok(json!({ "thickness": thick }))
}

// ----------------------------------------------------------------------------
// Interactive machines
// ----------------------------------------------------------------------------

#[derive(Default)]
enum UcsState {
    #[default]
    Prompt,
    Origin,
    ThreePointOrigin,
    ThreePointX(Vec3),
    ThreePointY(Vec3, Vec3),
    ZAxisOrigin,
    ZAxisPoint(Vec3),
    Rotate(char),
    NamedAsk,
    NamedSave,
    NamedRestore,
    NamedDelete,
}

#[derive(Default)]
struct UcsM {
    state: UcsState,
}

impl Interactive for UcsM {
    fn name(&self) -> &'static str {
        "UCS"
    }

    fn prompt(&self, _s: &Session) -> Prompt {
        match &self.state {
            UcsState::Prompt => {
                Prompt::new("Specify origin of UCS or [Face/NAmed/OBject/Previous/View/World/X/Y/Z/ZAxis/3point/Origin] <World>", Accept::POINT)
                    .kw(&["Face", "NAmed", "OBject", "Previous", "View", "World", "X", "Y", "Z", "ZAxis", "3point", "Origin"])
                    .default("World")
            }
            UcsState::Origin => Prompt::new("Specify new origin point <0,0,0>", Accept::POINT).default("0,0,0"),
            UcsState::ThreePointOrigin => Prompt::new("Specify new origin point <0,0,0>", Accept::POINT).default("0,0,0"),
            UcsState::ThreePointX(_) => Prompt::new("Specify point on positive portion of X-axis", Accept::POINT),
            UcsState::ThreePointY(_, _) => Prompt::new("Specify point on positive-Y portion of the UCS XY plane", Accept::POINT),
            UcsState::ZAxisOrigin => Prompt::new("Specify new origin point <0,0,0>", Accept::POINT).default("0,0,0"),
            UcsState::ZAxisPoint(_) => Prompt::new("Specify point on positive portion of Z-axis", Accept::POINT),
            UcsState::Rotate(axis) => Prompt::new(format!("Specify rotation angle about {axis} axis <0>"), Accept::NUMBER).default("0"),
            UcsState::NamedAsk => Prompt::new("Enter an option [Restore/Save/Delete/?]", Accept::default()).kw(&["Restore", "Save", "Delete", "?"]),
            UcsState::NamedSave => Prompt::new("Enter name to save current UCS", Accept::TEXT),
            UcsState::NamedRestore => Prompt::new("Enter name of UCS to restore", Accept::TEXT),
            UcsState::NamedDelete => Prompt::new("Enter name of UCS to delete", Accept::TEXT),
        }
    }

    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match std::mem::take(&mut self.state) {
            UcsState::Prompt => match i {
                Input::Enter => {
                    run_ucs_world(s)?;
                    Ok(Step::Done)
                }
                Input::Point(p) => {
                    run_ucs_origin(s, &json!({ "origin": [p.x, p.y, 0.0] }))?;
                    Ok(Step::Done)
                }
                Input::Keyword(k) => match k.to_ascii_lowercase().as_str() {
                    "world" | "w" => {
                        run_ucs_world(s)?;
                        Ok(Step::Done)
                    }
                    "previous" | "p" => {
                        run_ucs_previous(s)?;
                        Ok(Step::Done)
                    }
                    "view" | "v" => {
                        run_ucs_view(s)?;
                        Ok(Step::Done)
                    }
                    "x" => {
                        self.state = UcsState::Rotate('X');
                        Ok(Step::Continue)
                    }
                    "y" => {
                        self.state = UcsState::Rotate('Y');
                        Ok(Step::Continue)
                    }
                    "z" => {
                        self.state = UcsState::Rotate('Z');
                        Ok(Step::Continue)
                    }
                    "3point" | "3p" | "3" => {
                        self.state = UcsState::ThreePointOrigin;
                        Ok(Step::Continue)
                    }
                    "zaxis" | "za" => {
                        self.state = UcsState::ZAxisOrigin;
                        Ok(Step::Continue)
                    }
                    "origin" | "o" => {
                        self.state = UcsState::Origin;
                        Ok(Step::Continue)
                    }
                    "named" | "na" => {
                        self.state = UcsState::NamedAsk;
                        Ok(Step::Continue)
                    }
                    _ => Ok(Step::Done),
                },
                _ => Ok(Step::Continue),
            },
            UcsState::Origin => match i {
                Input::Point(p) => {
                    run_ucs_origin(s, &json!({ "origin": [p.x, p.y, 0.0] }))?;
                    Ok(Step::Done)
                }
                Input::Enter => {
                    run_ucs_origin(s, &json!({ "origin": [0.0, 0.0, 0.0] }))?;
                    Ok(Step::Done)
                }
                _ => Ok(Step::Continue),
            },
            UcsState::ThreePointOrigin => match i {
                Input::Point(p) => {
                    self.state = UcsState::ThreePointX(Vec3::new(p.x, p.y, 0.0));
                    Ok(Step::Continue)
                }
                Input::Enter => {
                    self.state = UcsState::ThreePointX(Vec3::ZERO);
                    Ok(Step::Continue)
                }
                _ => Ok(Step::Continue),
            },
            UcsState::ThreePointX(origin) => match i {
                Input::Point(p) => {
                    self.state = UcsState::ThreePointY(origin, Vec3::new(p.x, p.y, 0.0));
                    Ok(Step::Continue)
                }
                _ => Ok(Step::Continue),
            },
            UcsState::ThreePointY(origin, px) => match i {
                Input::Point(py) => {
                    run_ucs_3point(
                        s,
                        &json!({
                            "origin": [origin.x, origin.y, origin.z],
                            "xAxis": [px.x, px.y, px.z],
                            "yAxis": [py.x, py.y, 0.0],
                        }),
                    )?;
                    Ok(Step::Done)
                }
                _ => Ok(Step::Continue),
            },
            UcsState::ZAxisOrigin => match i {
                Input::Point(p) => {
                    self.state = UcsState::ZAxisPoint(Vec3::new(p.x, p.y, 0.0));
                    Ok(Step::Continue)
                }
                Input::Enter => {
                    self.state = UcsState::ZAxisPoint(Vec3::ZERO);
                    Ok(Step::Continue)
                }
                _ => Ok(Step::Continue),
            },
            UcsState::ZAxisPoint(origin) => match i {
                Input::Point(pz) => {
                    run_ucs_zaxis(
                        s,
                        &json!({
                            "origin": [origin.x, origin.y, origin.z],
                            "zAxis": [pz.x, pz.y, 1.0],
                        }),
                    )?;
                    Ok(Step::Done)
                }
                _ => Ok(Step::Continue),
            },
            UcsState::Rotate(axis) => match i {
                Input::Text(t) => {
                    let ang: f64 = t.trim().parse().unwrap_or(0.0);
                    run_ucs_rotate_axis(s, axis, &json!({ "angle": ang }))?;
                    Ok(Step::Done)
                }
                Input::Enter => {
                    run_ucs_rotate_axis(s, axis, &json!({ "angle": 0.0 }))?;
                    Ok(Step::Done)
                }
                _ => Ok(Step::Continue),
            },
            UcsState::NamedAsk => match i {
                Input::Keyword(k) => match k.to_ascii_lowercase().as_str() {
                    "save" | "s" => {
                        self.state = UcsState::NamedSave;
                        Ok(Step::Continue)
                    }
                    "restore" | "r" => {
                        self.state = UcsState::NamedRestore;
                        Ok(Step::Continue)
                    }
                    "delete" | "d" => {
                        self.state = UcsState::NamedDelete;
                        Ok(Step::Continue)
                    }
                    "?" => {
                        run_ucs_list(s)?;
                        Ok(Step::Done)
                    }
                    _ => Ok(Step::Done),
                },
                _ => Ok(Step::Continue),
            },
            UcsState::NamedSave => match i {
                Input::Text(t) => {
                    run_ucs_save(s, &t)?;
                    Ok(Step::Done)
                }
                _ => Ok(Step::Done),
            },
            UcsState::NamedRestore => match i {
                Input::Text(t) => {
                    run_ucs_restore(s, &t)?;
                    Ok(Step::Done)
                }
                _ => Ok(Step::Done),
            },
            UcsState::NamedDelete => match i {
                Input::Text(t) => {
                    run_ucs_delete(s, &t)?;
                    Ok(Step::Done)
                }
                _ => Ok(Step::Done),
            },
        }
    }
}

#[derive(Default)]
struct UcsOriginM;

impl Interactive for UcsOriginM {
    fn name(&self) -> &'static str {
        "UCS Origin"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new("Specify new origin point <0,0,0>", Accept::POINT).default("0,0,0")
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Point(p) => {
                run_ucs_origin(s, &json!({ "origin": [p.x, p.y, 0.0] }))?;
                Ok(Step::Done)
            }
            Input::Enter => {
                run_ucs_origin(s, &json!({ "origin": [0.0, 0.0, 0.0] }))?;
                Ok(Step::Done)
            }
            _ => Ok(Step::Continue),
        }
    }
}

#[derive(Default)]
enum Ucs3PointStep {
    #[default]
    Origin,
    XAxis(Vec3),
    YAxis(Vec3, Vec3),
}

#[derive(Default)]
struct Ucs3PointM {
    step: Ucs3PointStep,
}

impl Interactive for Ucs3PointM {
    fn name(&self) -> &'static str {
        "UCS 3Point"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match &self.step {
            Ucs3PointStep::Origin => Prompt::new("Specify new origin point <0,0,0>", Accept::POINT).default("0,0,0"),
            Ucs3PointStep::XAxis(_) => Prompt::new("Specify point on positive portion of X-axis", Accept::POINT),
            Ucs3PointStep::YAxis(_, _) => Prompt::new("Specify point on positive-Y portion of the UCS XY plane", Accept::POINT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match std::mem::take(&mut self.step) {
            Ucs3PointStep::Origin => match i {
                Input::Point(p) => {
                    self.step = Ucs3PointStep::XAxis(Vec3::new(p.x, p.y, 0.0));
                    Ok(Step::Continue)
                }
                Input::Enter => {
                    self.step = Ucs3PointStep::XAxis(Vec3::ZERO);
                    Ok(Step::Continue)
                }
                _ => Ok(Step::Continue),
            },
            Ucs3PointStep::XAxis(origin) => match i {
                Input::Point(px) => {
                    self.step = Ucs3PointStep::YAxis(origin, Vec3::new(px.x, px.y, 0.0));
                    Ok(Step::Continue)
                }
                _ => Ok(Step::Continue),
            },
            Ucs3PointStep::YAxis(origin, px) => match i {
                Input::Point(py) => {
                    run_ucs_3point(
                        s,
                        &json!({
                            "origin": [origin.x, origin.y, origin.z],
                            "xAxis": [px.x, px.y, px.z],
                            "yAxis": [py.x, py.y, 0.0],
                        }),
                    )?;
                    Ok(Step::Done)
                }
                _ => Ok(Step::Continue),
            },
        }
    }
}

#[derive(Default)]
enum UcsZAxisStep {
    #[default]
    Origin,
    ZAxis(Vec3),
}

#[derive(Default)]
struct UcsZAxisM {
    step: UcsZAxisStep,
}

impl Interactive for UcsZAxisM {
    fn name(&self) -> &'static str {
        "UCS ZAxis"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match &self.step {
            UcsZAxisStep::Origin => Prompt::new("Specify new origin point <0,0,0>", Accept::POINT).default("0,0,0"),
            UcsZAxisStep::ZAxis(_) => Prompt::new("Specify point on positive portion of Z-axis", Accept::POINT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match std::mem::take(&mut self.step) {
            UcsZAxisStep::Origin => match i {
                Input::Point(p) => {
                    self.step = UcsZAxisStep::ZAxis(Vec3::new(p.x, p.y, 0.0));
                    Ok(Step::Continue)
                }
                Input::Enter => {
                    self.step = UcsZAxisStep::ZAxis(Vec3::ZERO);
                    Ok(Step::Continue)
                }
                _ => Ok(Step::Continue),
            },
            UcsZAxisStep::ZAxis(origin) => match i {
                Input::Point(pz) => {
                    run_ucs_zaxis(
                        s,
                        &json!({
                            "origin": [origin.x, origin.y, origin.z],
                            "zAxis": [pz.x, pz.y, 1.0],
                        }),
                    )?;
                    Ok(Step::Done)
                }
                _ => Ok(Step::Continue),
            },
        }
    }
}

struct UcsRotateM {
    axis: char,
}

impl Interactive for UcsRotateM {
    fn name(&self) -> &'static str {
        "UCS Rotate"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new(format!("Specify rotation angle about {} axis <0>", self.axis), Accept::NUMBER).default("0")
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Text(t) => {
                let ang: f64 = t.trim().parse().unwrap_or(0.0);
                run_ucs_rotate_axis(s, self.axis, &json!({ "angle": ang }))?;
                Ok(Step::Done)
            }
            Input::Enter => {
                run_ucs_rotate_axis(s, self.axis, &json!({ "angle": 0.0 }))?;
                Ok(Step::Done)
            }
            _ => Ok(Step::Continue),
        }
    }
}

#[derive(Default)]
enum PlanStep {
    #[default]
    Mode,
    Named,
}

#[derive(Default)]
struct PlanM {
    step: PlanStep,
}

impl Interactive for PlanM {
    fn name(&self) -> &'static str {
        "PLAN"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match &self.step {
            PlanStep::Mode => Prompt::new("Enter an option [Current ucs/Ucs/World] <Current>", Accept::default())
                .kw(&["Current ucs", "Ucs", "World"])
                .default("Current ucs"),
            PlanStep::Named => Prompt::new("Enter name of UCS", Accept::TEXT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match std::mem::take(&mut self.step) {
            PlanStep::Mode => match i {
                Input::Enter => {
                    run_plan_mode(s, "current")?;
                    Ok(Step::Done)
                }
                Input::Keyword(k) => match k.to_ascii_lowercase().as_str() {
                    "world" | "w" => {
                        run_plan_mode(s, "world")?;
                        Ok(Step::Done)
                    }
                    "current ucs" | "current" | "c" => {
                        run_plan_mode(s, "current")?;
                        Ok(Step::Done)
                    }
                    "ucs" | "u" => {
                        self.step = PlanStep::Named;
                        Ok(Step::Continue)
                    }
                    _ => Ok(Step::Done),
                },
                _ => Ok(Step::Continue),
            },
            PlanStep::Named => match i {
                Input::Text(t) => {
                    run_plan_named(s, &json!({ "name": t }))?;
                    Ok(Step::Done)
                }
                _ => Ok(Step::Done),
            },
        }
    }
}

enum ElevationStep {
    Elevation,
    Thickness(f64),
}

struct ElevationM {
    step: ElevationStep,
    cur_elev: f64,
    cur_thick: f64,
}

impl ElevationM {
    fn new(s: &Session) -> Self {
        let (cur_elev, cur_thick) = match s.doc() {
            Ok(d) => (d.header.f64("ELEVATION", 0.0), d.header.f64("THICKNESS", 0.0)),
            Err(_) => (0.0, 0.0),
        };
        Self { step: ElevationStep::Elevation, cur_elev, cur_thick }
    }
}

impl Interactive for ElevationM {
    fn name(&self) -> &'static str {
        "ELEVATION"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match &self.step {
            ElevationStep::Elevation => {
                Prompt::new(format!("Specify new default elevation <{:.4}>", self.cur_elev), Accept::NUMBER).default(format!("{:.4}", self.cur_elev))
            }
            ElevationStep::Thickness(_) => Prompt::new(format!("Specify new default thickness <{:.4}>", self.cur_thick), Accept::NUMBER)
                .default(format!("{:.4}", self.cur_thick)),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match std::mem::replace(&mut self.step, ElevationStep::Elevation) {
            ElevationStep::Elevation => {
                let elev = match i {
                    Input::Text(t) => t.trim().parse::<f64>().unwrap_or(self.cur_elev),
                    _ => self.cur_elev,
                };
                self.step = ElevationStep::Thickness(elev);
                Ok(Step::Continue)
            }
            ElevationStep::Thickness(elev) => {
                let thick = match i {
                    Input::Text(t) => t.trim().parse::<f64>().unwrap_or(self.cur_thick),
                    _ => self.cur_thick,
                };
                run_elevation(s, &json!({ "elevation": elev, "thickness": thick }))?;
                Ok(Step::Done)
            }
        }
    }
}

struct ThicknessM {
    cur_thick: f64,
}

impl ThicknessM {
    fn new(s: &Session) -> Self {
        let cur_thick = s.doc().map(|d| d.header.f64("THICKNESS", 0.0)).unwrap_or(0.0);
        Self { cur_thick }
    }
}

impl Interactive for ThicknessM {
    fn name(&self) -> &'static str {
        "THICKNESS"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new(format!("Enter new value for THICKNESS <{:.4}>", self.cur_thick), Accept::NUMBER).default(format!("{:.4}", self.cur_thick))
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let thick = match i {
            Input::Text(t) => t.trim().parse::<f64>().unwrap_or(self.cur_thick),
            _ => self.cur_thick,
        };
        run_thickness(s, &json!({ "thickness": thick }))?;
        Ok(Step::Done)
    }
}

pub fn run_ucs_face(s: &mut Session, p: &Value) -> Result<Value> {
    let handle = p.get("handle").and_then(Value::as_str);
    let pt = vec3_param(p, "point");

    let doc = s.doc()?;
    let target = if let Some(h_str) = handle {
        let h = Handle::parse_hex(h_str).ok_or_else(|| bad("ucs", "invalid handle"))?;
        doc.entity(h).cloned().ok_or_else(|| bad("ucs", "entity not found"))?
    } else if let Some(p) = pt {
        doc.model
            .iter()
            .find(|e| match &e.kind {
                cadcraft_doc::EntityKind::Face3d(f) => f.corners.iter().any(|c| (*c - p).len() < 5.0),
                cadcraft_doc::EntityKind::Solid(sol) | cadcraft_doc::EntityKind::Trace(sol) => sol.corners.iter().any(|c| (*c - p).len() < 5.0),
                _ => false,
            })
            .cloned()
            .ok_or_else(|| bad("ucs", "no 3D face found near point"))?
    } else {
        doc.model
            .iter()
            .find(|e| matches!(e.kind, cadcraft_doc::EntityKind::Face3d(_)))
            .cloned()
            .ok_or_else(|| bad("ucs", "specify `handle` or `point` of face"))?
    };

    let (origin, x_axis, y_axis) = match &target.kind {
        cadcraft_doc::EntityKind::Face3d(f) => {
            let p0 = f.corners[0];
            let p1 = f.corners[1];
            let p2 = f.corners[2];
            let vx = p1 - p0;
            let vy_cand = p2 - p0;
            let normal = vx.cross(vy_cand);
            if vx.len() < 1e-9 || normal.len() < 1e-9 {
                return Err(bad("ucs", "face is degenerate"));
            }
            let x_axis = vx.normalized();
            let z_axis = normal.normalized();
            let y_axis = z_axis.cross(x_axis).normalized();
            (p0, x_axis, y_axis)
        }
        cadcraft_doc::EntityKind::Solid(sol) | cadcraft_doc::EntityKind::Trace(sol) => {
            let p0 = sol.corners[0];
            let p1 = sol.corners[1];
            let p2 = sol.corners[2];
            let vx = p1 - p0;
            let vy_cand = p2 - p0;
            let normal = vx.cross(vy_cand);
            if vx.len() < 1e-9 || normal.len() < 1e-9 {
                return Err(bad("ucs", "solid face is degenerate"));
            }
            let x_axis = vx.normalized();
            let z_axis = normal.normalized();
            let y_axis = z_axis.cross(x_axis).normalized();
            (p0, x_axis, y_axis)
        }
        _ => return Err(bad("ucs", "selected entity is not a 3D face")),
    };

    let ucs = Ucs { name: String::new(), origin, x_axis, y_axis };
    s.set_current_ucs(ucs)?;
    s.echo("UCS aligned to selected face.");
    Ok(json!({
        "origin": [origin.x, origin.y, origin.z],
        "xAxis": [x_axis.x, x_axis.y, x_axis.z],
        "yAxis": [y_axis.x, y_axis.y, y_axis.z],
    }))
}

struct UcsFaceM;

impl Interactive for UcsFaceM {
    fn name(&self) -> &'static str {
        "UCS"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new("Select face of 3D object", Accept::POINT)
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Point(p) => {
                run_ucs_face(s, &json!({ "point": [p.x, p.y, 0.0] }))?;
                Ok(Step::Done)
            }
            Input::Text(t) => {
                let trimmed = t.trim();
                if let Some(h) = Handle::parse_hex(trimmed) {
                    run_ucs_face(s, &json!({ "handle": h.hex() }))?;
                    Ok(Step::Done)
                } else {
                    Ok(Step::Done)
                }
            }
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
}

pub fn run_transparency(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(v) = p.get("value") {
        let val_str = match v {
            Value::String(str_val) => {
                let lower = str_val.trim().to_ascii_lowercase();
                if lower == "bylayer" {
                    "ByLayer".to_string()
                } else if lower == "byblock" {
                    "ByBlock".to_string()
                } else if let Ok(num) = str_val.trim().trim_end_matches('%').parse::<u8>() {
                    if num > 90 {
                        return Err(bad("transparency", "transparency must be between 0 and 90"));
                    }
                    format!("{num}")
                } else {
                    return Err(bad("transparency", "invalid transparency value (use ByLayer, ByBlock, or 0-90)"));
                }
            }
            Value::Number(num) => {
                if let Some(n) = num.as_u64() {
                    if n > 90 {
                        return Err(bad("transparency", "transparency must be between 0 and 90"));
                    }
                    format!("{n}")
                } else {
                    return Err(bad("transparency", "transparency must be an integer 0-90"));
                }
            }
            _ => return Err(bad("transparency", "invalid transparency parameter")),
        };
        s.doc_mut()?.header.set_str("CETRANSPARENCY", &val_str);
        s.echo(format!("CETRANSPARENCY set to {val_str}"));
        return Ok(json!({ "value": val_str }));
    }

    let cur = s.doc()?.header.str("CETRANSPARENCY", "ByLayer");
    Ok(json!({ "value": cur }))
}

struct TransparencyM {
    cur_val: String,
}

impl TransparencyM {
    fn new(s: &Session) -> Self {
        let cur_val = s.doc().map(|d| d.header.str("CETRANSPARENCY", "ByLayer").to_string()).unwrap_or_else(|_| "ByLayer".into());
        Self { cur_val }
    }
}

impl Interactive for TransparencyM {
    fn name(&self) -> &'static str {
        "CETRANSPARENCY"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new(format!("Enter new value for CETRANSPARENCY [ByLayer/ByBlock/0-90%] <{}>", self.cur_val), Accept::TEXT)
            .kw(&["ByLayer", "ByBlock"])
            .default(&self.cur_val)
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Keyword(k) => {
                run_transparency(s, &json!({ "value": k }))?;
                Ok(Step::Done)
            }
            Input::Text(t) => {
                let trimmed = t.trim();
                let val = if trimmed.is_empty() { self.cur_val.as_str() } else { trimmed };
                run_transparency(s, &json!({ "value": val }))?;
                Ok(Step::Done)
            }
            Input::Enter => {
                run_transparency(s, &json!({ "value": self.cur_val }))?;
                Ok(Step::Done)
            }
            _ => Ok(Step::Continue),
        }
    }
}
