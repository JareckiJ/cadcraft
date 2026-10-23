//! Projects: a set of DXF drawings, one per sheet, worked on together.
//!
//! A project file (`.ecproj`, JSON) lists its drawings in sheet order, relative to the project
//! file's folder, with the sheet layout (width, columns). The project commands open each drawing
//! in turn (or use it if it is already open), run tagging, wire numbering or cross-referencing
//! with that drawing's sheet number and the numbers used in the others, and save it: tags and
//! wire numbers are unique across the project and contacts are cross-referenced to coils on
//! other sheets. These commands save the drawings they change and are not undoable.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use cadcraft_doc::{Common, Drawing, EntityKind, HAlign, LwPolyline, Space, Text, VAlign};
use cadcraft_geom::{PolyVertex, Vec2, Vec3};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{class_overrides, is_child, is_numbered};
use crate::cmd::{CommandSpec, always, bad, str_param};
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

/// Largest project file read.
const MAX_PROJECT_BYTES: usize = 4 << 20;
/// Most drawings in a project.
const MAX_DRAWINGS: usize = 999;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct Project {
    pub name: String,
    /// Drawing paths, relative to the project file's folder (or absolute), in sheet order.
    pub drawings: Vec<String>,
    pub sheet_width: f64,
    pub columns: u64,
}

impl Default for Project {
    fn default() -> Self {
        Project { name: String::new(), drawings: Vec::new(), sheet_width: 420.0, columns: 10 }
    }
}

/// The project the project commands use when none is named.
static CURRENT: Mutex<Option<String>> = Mutex::new(None);

pub(super) fn current() -> Option<String> {
    match CURRENT.lock() {
        Ok(g) => g.clone(),
        Err(e) => e.into_inner().clone(),
    }
}

fn set_current(path: Option<String>) {
    match CURRENT.lock() {
        Ok(mut g) => *g = path,
        Err(e) => *e.into_inner() = path,
    }
}

fn project_path(cmd: &str, p: &Value) -> Result<String> {
    str_param(p, "project")
        .map(str::to_string)
        .or_else(current)
        .ok_or_else(|| bad(cmd, "no project is open: open one (EPROJECT.OPEN) or pass `project`"))
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn read_project(path: &str) -> Result<Project> {
    let bytes = crate::capped_read::read_capped(Path::new(path), MAX_PROJECT_BYTES).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    let mut pr: Project = serde_json::from_slice(&bytes).map_err(|e| EngineError::Other(format!("{path} is not a project file: {e}")))?;
    pr.drawings.truncate(MAX_DRAWINGS);
    if !(pr.sheet_width.is_finite() && pr.sheet_width > 0.0) {
        pr.sheet_width = 420.0;
    }
    if !(1..=1000).contains(&pr.columns) {
        pr.columns = 10;
    }
    Ok(pr)
}

#[cfg(target_arch = "wasm32")]
pub(super) fn read_project(_path: &str) -> Result<Project> {
    Err(EngineError::Other("Projects need files, which the web version cannot reach.".into()))
}

#[cfg(not(target_arch = "wasm32"))]
fn write_project(path: &str, pr: &Project) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(pr).map_err(|e| EngineError::Other(e.to_string()))?;
    crate::atomic_write::write_atomic(Path::new(path), &bytes).map_err(|e| EngineError::Other(format!("{path}: {e}")))
}

#[cfg(target_arch = "wasm32")]
fn write_project(_path: &str, _pr: &Project) -> Result<()> {
    Err(EngineError::Other("Projects need files, which the web version cannot reach.".into()))
}

/// Where drawing `rel` of the project at `project` is.
pub(super) fn drawing_path(project: &str, rel: &str) -> PathBuf {
    let r = Path::new(rel);
    if r.is_absolute() { r.to_path_buf() } else { Path::new(project).parent().unwrap_or(Path::new("")).join(r) }
}

/// `path` as listed in the project at `project`: relative to its folder when inside it.
fn relative_to(project: &str, path: &Path) -> String {
    let dir = Path::new(project).parent().unwrap_or(Path::new(""));
    path.strip_prefix(dir).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

pub(super) fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

/// The grid parameters for sheet `sheet` of `pr`.
pub(super) fn sheet_params(pr: &Project, sheet: u64) -> Value {
    json!({ "firstSheet": sheet, "sheetWidth": pr.sheet_width, "columns": pr.columns, "origin": [0, 0] })
}

/// Merge `extra` into the object `base`.
fn with(mut base: Value, extra: &Value) -> Value {
    if let (Some(b), Some(e)) = (base.as_object_mut(), extra.as_object()) {
        for (k, v) in e {
            b.insert(k.clone(), v.clone());
        }
    }
    base
}

/// Run `f` on every drawing of the project, in sheet order, as the active drawing: an open one
/// as it is, another opened for the run and closed after. Drawings are saved after `f` when
/// `save` is set. The drawing active before stays active.
pub(super) fn for_each(
    s: &mut Session,
    project: &str,
    pr: &Project,
    save: bool,
    mut f: impl FnMut(&mut Session, usize, u64) -> Result<Value>,
) -> Result<Vec<Value>> {
    let original = s.active;
    let mut out = Vec::with_capacity(pr.drawings.len());
    for (i, rel) in pr.drawings.iter().enumerate() {
        let path = drawing_path(project, rel);
        let path_s = path.to_string_lossy().to_string();
        let open = s.docs.iter().position(|d| d.path.as_deref().is_some_and(|q| same_file(Path::new(q), &path)));
        let (idx, opened_here) = match open {
            Some(i) => (i, false),
            None => {
                let rf = crate::cmd::file::read_file(&path_s).map_err(EngineError::Other)?;
                crate::cmd::file::finish_open(s, rf)?;
                let i = s.docs.iter().position(|d| d.path.as_deref().is_some_and(|q| same_file(Path::new(q), &path))).unwrap_or(s.active);
                (i, true)
            }
        };
        s.active = idx;
        let sheet = i as u64 + 1;
        let r = f(s, i, sheet).and_then(|v| if save { crate::cmd::file::save_to(s, &path_s).map(|_| v) } else { Ok(v) });
        if opened_here && idx < s.docs.len() {
            s.docs.remove(idx);
        }
        s.active = original.min(s.docs.len().saturating_sub(1));
        out.push(json!({ "drawing": rel, "sheet": sheet, "result": r? }));
    }
    s.active = original.min(s.docs.len().saturating_sub(1));
    Ok(out)
}

/// A new drawing for sheet `sheet`: a border, column numbers along the top and a title.
pub(super) fn sheet_drawing(pr: &Project, sheet: u64, total: u64) -> Drawing {
    let mut d = Drawing::new_metric();
    let (w, h) = (pr.sheet_width, pr.sheet_width * 297.0 / 420.0);
    let cols = pr.columns.max(1);
    let cw = w / cols as f64;
    let frame = Common { layer: "FRAME".into(), ..Common::default() };
    let pl = |pts: &[(f64, f64)], closed: bool| {
        EntityKind::LwPolyline(LwPolyline {
            vertices: pts.iter().map(|(x, y)| PolyVertex { p: Vec2::new(*x, *y), ..Default::default() }).collect(),
            closed,
            const_width: 0.0,
            elevation: 0.0,
            plinegen: false,
        })
    };
    let text = |x: f64, y: f64, height: f64, value: String, halign: HAlign| {
        let p = Vec3::new(x, y, 0.0);
        EntityKind::Text(Text {
            insert: p,
            align_pt: Some(p),
            height,
            value,
            rotation: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            style: "Standard".into(),
            halign,
            valign: VAlign::Middle,
        })
    };
    let mut kinds = vec![pl(&[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)], true), pl(&[(0.0, h - 10.0), (w, h - 10.0)], false)];
    for c in 0..cols {
        let x = c as f64 * cw;
        if c > 0 {
            kinds.push(pl(&[(x, h), (x, h - 10.0)], false));
        }
        kinds.push(text(x + cw / 2.0, h - 5.0, 3.5, (c + 1).to_string(), HAlign::Center));
    }
    for k in kinds {
        let _ = d.add(&Space::Model, frame.clone(), k);
    }
    // The title block, with the project's name and the sheet's number.
    let mut values = serde_json::Map::new();
    values.insert("PROJECT".into(), json!(pr.name));
    values.insert("SHEET".into(), json!(sheet.to_string()));
    values.insert("SHEETS".into(), json!(total.max(sheet).to_string()));
    super::titleblock::add_to(&mut d, Vec2::new(w, 0.0), &values);
    d
}

#[cfg(not(target_arch = "wasm32"))]
fn create_drawing(path: &Path, pr: &Project, sheet: u64, total: u64) -> Result<()> {
    let hooks = crate::cmd::file::io().ok_or_else(|| EngineError::Other("file formats are not available in this build".into()))?;
    let d = sheet_drawing(pr, sheet, total);
    let bytes = (hooks.write)(&d, &path.to_string_lossy()).map_err(EngineError::Other)?;
    crate::atomic_write::write_atomic(path, &bytes).map_err(|e| EngineError::Other(format!("{}: {e}", path.display())))
}

#[cfg(target_arch = "wasm32")]
fn create_drawing(_path: &Path, _pr: &Project, _sheet: u64, _total: u64) -> Result<()> {
    Err(EngineError::Other("Projects need files, which the web version cannot reach.".into()))
}

pub(super) fn specs() -> Vec<CommandSpec> {
    let menu = |item: &'static str| -> &'static [&'static str] {
        match item {
            "new" => &["Tools", "Electrical", "Project", "New Project..."],
            "open" => &["Tools", "Electrical", "Project", "Open Project..."],
            "fromopen" => &["Tools", "Electrical", "Project", "New Project from Open Drawings"],
            "add" => &["Tools", "Electrical", "Project", "Add Current Drawing"],
            "addsheet" => &["Tools", "Electrical", "Project", "Add New Sheet"],
            "list" => &["Tools", "Electrical", "Project", "List Drawings"],
            "tag" => &["Tools", "Electrical", "Project", "Tag Devices (Whole Project)"],
            "wires" => &["Tools", "Electrical", "Project", "Number Wires (Whole Project)"],
            _ => &["Tools", "Electrical", "Project", "Cross-References (Whole Project)"],
        }
    };
    vec![
        CommandSpec::new("eproject.new", "New Project", run_new)
            .menu(menu("new"))
            .params("{path (.ecproj), name?, sheets? (blank sheets to create, default 1), overwrite?} → {project, drawings}")
            .enabled(always)
            .noundo()
            .interactive(|_| Ok(Box::new(PathM { cmd: "eproject.new", msg: "Project file to create (.ecproj)" }))),
        CommandSpec::new("eproject.open", "Open Project", run_open)
            .menu(menu("open"))
            .params("{path, openDrawings? (default true)} → {project, drawings}")
            .enabled(always)
            .noundo()
            .interactive(|_| Ok(Box::new(PathM { cmd: "eproject.open", msg: "Project file to open (.ecproj)" }))),
        CommandSpec::new("eproject.fromopen", "New Project from Open Drawings", run_fromopen)
            .menu(menu("fromopen"))
            .params("{path? (default: <folder of the first drawing>/<folder name>.ecproj), name?} → {project, drawings}")
            .noundo(),
        CommandSpec::new("eproject.add", "Add Drawing to Project", run_add)
            .menu(menu("add"))
            .params("{drawing? (default: the current drawing), project?} → {drawings}")
            .noundo(),
        CommandSpec::new("eproject.addsheet", "Add New Sheet", run_addsheet)
            .menu(menu("addsheet"))
            .params("{name? (file name, default <project>-NN.dxf), project?, open? (default true)} → {drawing, sheet}")
            .enabled(always)
            .noundo(),
        CommandSpec::new("eproject.list", "List Project Drawings", run_list)
            .menu(menu("list"))
            .params("{project?} → {project, drawings: [{sheet, drawing, exists}]}")
            .enabled(always)
            .noundo(),
        CommandSpec::new("eproject.tag", "Tag Devices in Project", run_tag)
            .menu(menu("tag"))
            .params("{project?, mode?: new | all, format?, classes?} → {drawings: [{drawing, sheet, result}], contactsFollowed}")
            .enabled(always)
            .noundo(),
        CommandSpec::new("eproject.wires", "Number Wires in Project", run_wires)
            .menu(menu("wires"))
            .params("{project?, mode?: new | all, format?, layers?, height?} → {drawings}")
            .enabled(always)
            .noundo(),
        CommandSpec::new("eproject.xref", "Cross-Reference Project", run_xref)
            .menu(menu("xref"))
            .params("{project?, layer?, height?} → {devices, orphans}")
            .enabled(always)
            .noundo(),
    ]
}

fn opened_json(path: &str, pr: &Project) -> Value {
    let list: Vec<Value> =
        pr.drawings.iter().enumerate().map(|(i, d)| json!({ "sheet": i + 1, "drawing": d, "exists": drawing_path(path, d).exists() })).collect();
    json!({ "project": path, "name": pr.name, "drawings": list })
}

fn run_new(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "eproject.new";
    let path = str_param(p, "path").map(str::trim).filter(|x| !x.is_empty()).ok_or_else(|| bad(CMD, "`path` is required"))?;
    let path = if Path::new(path).extension().is_none() { format!("{path}.ecproj") } else { path.to_string() };
    if Path::new(&path).exists() && !p.get("overwrite").and_then(Value::as_bool).unwrap_or(false) {
        return Err(EngineError::Other(format!("{path} already exists; open it, or pass overwrite: true.")));
    }
    let sheets = p.get("sheets").and_then(Value::as_u64).unwrap_or(1).min(MAX_DRAWINGS as u64);
    let stem = Path::new(&path).file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_else(|| "Project".into());
    let mut pr = Project { name: str_param(p, "name").map(str::to_string).unwrap_or_else(|| stem.clone()), ..Project::default() };
    for n in 1..=sheets {
        let file = format!("{stem}-{n:02}.dxf");
        let full = drawing_path(&path, &file);
        if !full.exists() {
            create_drawing(&full, &pr, n, sheets)?;
        }
        pr.drawings.push(file);
    }
    write_project(&path, &pr)?;
    set_current(Some(path.clone()));
    s.log.push(format!("Project {} created with {} sheet(s).", pr.name, pr.drawings.len()));
    Ok(opened_json(&path, &pr))
}

fn run_open(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "eproject.open";
    let path = str_param(p, "path").map(str::trim).filter(|x| !x.is_empty()).ok_or_else(|| bad(CMD, "`path` is required"))?.to_string();
    let pr = read_project(&path)?;
    set_current(Some(path.clone()));
    if p.get("openDrawings").and_then(Value::as_bool).unwrap_or(true) {
        for rel in &pr.drawings {
            let full = drawing_path(&path, rel);
            if !full.exists() || s.docs.iter().any(|d| d.path.as_deref().is_some_and(|q| same_file(Path::new(q), &full))) {
                continue;
            }
            let rf = crate::cmd::file::read_file(&full.to_string_lossy()).map_err(EngineError::Other)?;
            crate::cmd::file::finish_open(s, rf)?;
        }
    }
    s.log.push(format!("Project {} open: {} sheet(s).", pr.name, pr.drawings.len()));
    Ok(opened_json(&path, &pr))
}

fn run_fromopen(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "eproject.fromopen";
    let paths: Vec<String> = s.docs.iter().filter_map(|d| d.path.clone()).collect();
    let first = paths.first().ok_or_else(|| bad(CMD, "no saved drawings are open: save the sheets first"))?;
    let dir = Path::new(first).parent().unwrap_or(Path::new("")).to_path_buf();
    let path = match str_param(p, "path") {
        Some(x) => x.to_string(),
        None => {
            let name = dir.file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_else(|| "Project".into());
            dir.join(format!("{name}.ecproj")).to_string_lossy().to_string()
        }
    };
    let stem = Path::new(&path).file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
    let pr = Project {
        name: str_param(p, "name").map(str::to_string).unwrap_or(stem),
        drawings: paths.iter().take(MAX_DRAWINGS).map(|d| relative_to(&path, Path::new(d))).collect(),
        ..Project::default()
    };
    write_project(&path, &pr)?;
    set_current(Some(path.clone()));
    s.log.push(format!("Project {} made from {} open drawing(s).", pr.name, pr.drawings.len()));
    Ok(opened_json(&path, &pr))
}

fn run_add(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "eproject.add";
    let path = project_path(CMD, p)?;
    let mut pr = read_project(&path)?;
    let drawing = match str_param(p, "drawing") {
        Some(d) => d.to_string(),
        None => s.state()?.path.clone().ok_or_else(|| bad(CMD, "save the drawing first, so the project can refer to it"))?,
    };
    let rel = relative_to(&path, Path::new(&drawing));
    if pr.drawings.iter().any(|d| same_file(&drawing_path(&path, d), &drawing_path(&path, &rel))) {
        return Err(EngineError::Other(format!("{rel} is already in the project.")));
    }
    if pr.drawings.len() >= MAX_DRAWINGS {
        return Err(EngineError::Other(format!("A project holds at most {MAX_DRAWINGS} drawings.")));
    }
    pr.drawings.push(rel.clone());
    write_project(&path, &pr)?;
    s.log.push(format!("{rel} is sheet {} of {}.", pr.drawings.len(), pr.name));
    Ok(opened_json(&path, &pr))
}

fn run_addsheet(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "eproject.addsheet";
    let path = project_path(CMD, p)?;
    let mut pr = read_project(&path)?;
    if pr.drawings.len() >= MAX_DRAWINGS {
        return Err(EngineError::Other(format!("A project holds at most {MAX_DRAWINGS} drawings.")));
    }
    let sheet = pr.drawings.len() as u64 + 1;
    let stem = Path::new(&path).file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_else(|| "Project".into());
    let file = str_param(p, "name").map(str::to_string).unwrap_or_else(|| format!("{stem}-{sheet:02}.dxf"));
    let full = drawing_path(&path, &file);
    if full.exists() {
        return Err(EngineError::Other(format!("{} already exists.", full.display())));
    }
    create_drawing(&full, &pr, sheet, sheet)?;
    pr.drawings.push(file.clone());
    write_project(&path, &pr)?;
    if p.get("open").and_then(Value::as_bool).unwrap_or(true) {
        let rf = crate::cmd::file::read_file(&full.to_string_lossy()).map_err(EngineError::Other)?;
        crate::cmd::file::finish_open(s, rf)?;
    }
    s.log.push(format!("Sheet {sheet} ({file}) added."));
    Ok(json!({ "drawing": file, "sheet": sheet }))
}

fn run_list(s: &mut Session, p: &Value) -> Result<Value> {
    let path = project_path("eproject.list", p)?;
    let pr = read_project(&path)?;
    let out = opened_json(&path, &pr);
    let names: Vec<String> = pr.drawings.iter().enumerate().map(|(i, d)| format!("{}: {d}", i + 1)).collect();
    s.log.push(format!("{} — {}", pr.name, names.join(", ")));
    Ok(out)
}

fn mode(cmd: &str, p: &Value) -> Result<&'static str> {
    match str_param(p, "mode").map(str::to_ascii_lowercase).as_deref() {
        None | Some("new") => Ok("new"),
        Some("all") => Ok("all"),
        Some(m) => Err(bad(cmd, format!("`mode` must be \"new\" or \"all\", not \"{m}\""))),
    }
}

/// The parameters a project command passes on to the per-drawing command.
fn passed(p: &Value, keys: &[&str]) -> Value {
    let mut m = serde_json::Map::new();
    for k in keys {
        if let Some(v) = p.get(*k) {
            m.insert((*k).to_string(), v.clone());
        }
    }
    Value::Object(m)
}

fn run_tag(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "eproject.tag";
    let path = project_path(CMD, p)?;
    let pr = read_project(&path)?;
    let mode = mode(CMD, p)?;
    let extra = passed(p, &["format", "classes", "attribute", "function", "location"]);
    let overrides = class_overrides(CMD, p)?;
    // New mode keeps every numbered tag in the project; all mode only those assigned so far.
    let mut reserved: HashSet<String> = HashSet::new();
    if mode == "new" {
        for_each(s, &path, &pr, false, |s, _, _| {
            for d in super::devices(s, None, None)? {
                if !is_child(&d, &overrides) && is_numbered(&d.tag) {
                    reserved.insert(d.tag);
                }
            }
            Ok(Value::Null)
        })?;
    }
    // (old tag → (new tag, drawing)) of renamed main devices, for contacts on other sheets.
    let mut renamed: HashMap<String, Vec<(String, usize)>> = HashMap::new();
    let results = for_each(s, &path, &pr, true, |s, i, sheet| {
        let mut params = with(sheet_params(&pr, sheet), &extra);
        params["mode"] = json!(mode);
        params["reserved"] = json!(reserved.iter().collect::<Vec<_>>());
        let r = super::run_etag(s, &params)?;
        for d in r["devices"].as_array().into_iter().flatten() {
            if let (Some(old), Some(new)) = (d["old"].as_str(), d["tag"].as_str()) {
                reserved.insert(new.to_string());
                if is_numbered(old) && old != new {
                    renamed.entry(old.to_string()).or_default().push((new.to_string(), i));
                }
            }
        }
        Ok(r)
    })?;
    let mut followed = 0usize;
    let unique: HashMap<String, (String, usize)> =
        renamed.into_iter().filter_map(|(old, v)| if let [one] = v.as_slice() { Some((old, one.clone())) } else { None }).collect();
    if !unique.is_empty() {
        for_each(s, &path, &pr, true, |s, i, _| {
            for d in super::devices(s, None, None)? {
                if let Some((new, home)) = unique.get(&d.tag)
                    && *home != i
                    && is_child(&d, &overrides)
                {
                    super::set_tag(s, d.handle, &d.attr, new)?;
                    followed += 1;
                }
            }
            Ok(Value::Null)
        })?;
    }
    s.log.push(format!("Project tagged: {} sheet(s), {followed} contact(s) on other sheets followed their coil.", pr.drawings.len()));
    Ok(json!({ "drawings": results, "contactsFollowed": followed }))
}

fn run_wires(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "eproject.wires";
    let path = project_path(CMD, p)?;
    let pr = read_project(&path)?;
    let mode = mode(CMD, p)?;
    let extra = passed(p, &["format", "layers", "labelLayer", "height", "tolerance"]);
    let label_layer = str_param(p, "labelLayer").unwrap_or(super::wires::LABEL_LAYER).to_string();
    let mut reserved: HashSet<String> = HashSet::new();
    if mode == "new" {
        for_each(s, &path, &pr, false, |s, _, _| {
            for (_, _, v) in super::wires::labels(s, &label_layer)? {
                if !v.is_empty() {
                    reserved.insert(v);
                }
            }
            Ok(Value::Null)
        })?;
    }
    let results = for_each(s, &path, &pr, true, |s, _, sheet| {
        let mut params = with(sheet_params(&pr, sheet), &extra);
        params["mode"] = json!(mode);
        params["reserved"] = json!(reserved.iter().collect::<Vec<_>>());
        let r = super::wires::run_ewire(s, &params)?;
        for w in r["wires"].as_array().into_iter().flatten() {
            if let Some(n) = w["number"].as_str() {
                reserved.insert(n.to_string());
            }
        }
        Ok(r)
    })?;
    s.log.push(format!("Project wires numbered on {} sheet(s).", pr.drawings.len()));
    Ok(json!({ "drawings": results }))
}

fn run_xref(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "eproject.xref";
    let path = project_path(CMD, p)?;
    let pr = read_project(&path)?;
    let (layer, height) = super::xref::layer_and_height(CMD, p)?;
    let overrides = class_overrides(CMD, p)?;
    let mut devs = Vec::new();
    for_each(s, &path, &pr, false, |s, i, sheet| {
        let grid = super::Grid::from_params(CMD, &sheet_params(&pr, sheet))?;
        devs.extend(super::xref::ref_devices(s, None, &grid, &overrides, i)?);
        Ok(Value::Null)
    })?;
    let refs = super::xref::compute(&devs);
    let results = for_each(s, &path, &pr, true, |s, i, _| {
        let (written, attrs) = super::xref::write_refs(s, i, &devs, &refs, &layer, height)?;
        Ok(json!({ "written": written, "inAttributes": attrs }))
    })?;
    let orphans: Vec<Value> = refs.orphans.iter().map(|(i, h)| json!({ "drawing": pr.drawings.get(*i), "handle": h.hex() })).collect();
    s.log.push(format!("Project cross-referenced: {} device(s) with contacts, {} orphan contact(s).", refs.report.len(), orphans.len()));
    Ok(json!({ "devices": refs.report, "orphans": orphans, "drawings": results }))
}

/// A project command asking for a file path at the command line.
struct PathM {
    cmd: &'static str,
    msg: &'static str,
}

impl Interactive for PathM {
    fn name(&self) -> &'static str {
        "EPROJECT"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new(self.msg, Accept { point: false, number: false, text: true, select: false, enter: true })
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Text(t) | Input::Keyword(t) if !t.trim().is_empty() => {
                let path = t.trim().trim_matches('"').to_string();
                match self.cmd {
                    "eproject.new" => run_new(s, &json!({ "path": path }))?,
                    _ => run_open(s, &json!({ "path": path }))?,
                };
                Ok(Step::Done)
            }
            Input::Enter | Input::Cancel => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_file_round_trip_and_paths() {
        let pr: Project = serde_json::from_str(r#"{"name": "P", "drawings": ["a.dxf", "sub/b.dxf"]}"#).unwrap();
        assert_eq!(pr.sheet_width, 420.0);
        assert_eq!(pr.columns, 10);
        let p = drawing_path("/x/y/p.ecproj", "sub/b.dxf");
        assert_eq!(p, PathBuf::from("/x/y/sub/b.dxf"));
        assert_eq!(relative_to("/x/y/p.ecproj", Path::new("/x/y/sub/b.dxf")), "sub/b.dxf");
        assert_eq!(relative_to("/x/y/p.ecproj", Path::new("/other/c.dxf")), "/other/c.dxf");
    }

    #[test]
    fn sheet_drawing_has_a_frame() {
        let pr = Project { name: "Demo".into(), ..Project::default() };
        let d = sheet_drawing(&pr, 3, 5);
        let texts: Vec<String> =
            d.model.iter().filter_map(|e| if let EntityKind::Text(t) = &e.kind { Some(t.value.clone()) } else { None }).collect();
        assert!(texts.contains(&"10".to_string()), "column numbers");
        let title = d.model.iter().find_map(|e| if let EntityKind::Insert(i) = &e.kind { Some(i.clone()) } else { None }).unwrap();
        let get = |t: &str| title.attribs.iter().find(|a| a.tag == t).map(|a| a.text.value.clone()).unwrap();
        assert_eq!((get("PROJECT").as_str(), get("SHEET").as_str(), get("SHEETS").as_str()), ("Demo", "3", "5"));
    }

    #[test]
    fn commands_without_a_project_explain_themselves() {
        set_current(None);
        let mut s = Session::new();
        for c in ["eproject.tag", "eproject.wires", "eproject.xref", "eproject.list", "eproject.add", "eproject.addsheet"] {
            let e = s.execute(c, &json!({})).unwrap_err().to_string();
            assert!(e.contains("project"), "{c}: {e}");
        }
        assert!(s.execute("eproject.open", &json!({ "path": "/no/such/file.ecproj" })).is_err());
        assert!(s.execute("eproject.fromopen", &json!({})).is_err(), "no saved drawings");
    }
}
