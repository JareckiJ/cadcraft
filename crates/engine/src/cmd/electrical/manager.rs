//! Project manager commands: the sheet list with each sheet's title block, reordering, renaming
//! and removing sheets, each sheet's title block fields, and the project's own properties
//! (name and the title block fields every sheet shares).
//!
//! After the sheet order changes, sheet numbers in the title blocks, cross-references and the
//! project database follow at once. Tags and wire numbers (`-K105`, `105`) carry the sheet they
//! were made on; renumbering them is the caller's choice (EPROJECT.TAG / EPROJECT.WIRES with
//! mode "all"), as the result's `renumberSuggested` says.

use std::path::Path;

use serde_json::{Map, Value, json};

use super::project::{MAX_DRAWINGS, Project, drawing_path, for_each, opened_json, project_path, read_project, same_file, write_project};
use crate::cmd::{CommandSpec, always, bad, str_param};
use crate::{EngineError, Result, Session};

pub(super) fn specs() -> Vec<CommandSpec> {
    let menu = |item: &'static str| -> &'static [&'static str] {
        match item {
            "move" => &["Tools", "Electrical", "Project", "Move Sheet..."],
            "rename" => &["Tools", "Electrical", "Project", "Rename Sheet..."],
            "remove" => &["Tools", "Electrical", "Project", "Remove Sheet from Project..."],
            "sheet" => &["Tools", "Electrical", "Project", "Sheet Properties..."],
            "titleblock" => &["Tools", "Electrical", "Project", "Title Block Template..."],
            _ => &["Tools", "Electrical", "Project", "Project Properties..."],
        }
    };
    vec![
        CommandSpec::new("eproject.sheets", "Project Sheets", run_sheets)
            .params("{project?} → {project, name, properties, sheets: [{sheet, drawing, exists, open, dirty, fields: {TITLE, DRAWINGNO, REV…}}]}")
            .enabled(always)
            .noundo(),
        CommandSpec::new("eproject.move", "Move Sheet", run_move)
            .menu(menu("move"))
            .params("{sheet (1-based), to (1-based), project?} → {drawings, renumberSuggested}")
            .enabled(always)
            .noundo(),
        CommandSpec::new("eproject.rename", "Rename Sheet", run_rename)
            .menu(menu("rename"))
            .params("{sheet (1-based), name (new file name; .dxf added), project?} → {drawing}")
            .enabled(always)
            .noundo(),
        CommandSpec::new("eproject.remove", "Remove Sheet from Project", run_remove)
            .menu(menu("remove"))
            .params("{sheet (1-based), project?} → {drawings, renumberSuggested} (the file is kept on disk)")
            .enabled(always)
            .noundo(),
        CommandSpec::new("eproject.sheet", "Sheet Properties", run_sheet)
            .menu(menu("sheet"))
            .params("{sheet (1-based), set?: {FIELD: value} (any field of the title block: TITLE, DESCRIPTION, LOC…), project?} → {sheet, fields}")
            .enabled(always)
            .noundo(),
        CommandSpec::new("eproject.titleblock", "Title Block Template", run_titleblock)
            .menu(menu("titleblock"))
            .params(
                "{template?: ETITLE | ETITLE_A3, file? (a DXF/DWG of your own title block, its attributes named after the fields), \
                 keepBorder?: bool (keep the sheet's own frame under a block of your own; default false), project?} \
                 → {titleBlock, sheets, fields: [{tag, label, scope, shown}]}",
            )
            .enabled(always)
            .noundo(),
        CommandSpec::new("eproject.props", "Project Properties", run_props)
            .menu(menu("props"))
            .params(
                "{name?, set?: {CUSTOMER, DRAWNBY, CHECKEDBY, DATE, REV, DRAWINGNO…} (fields every sheet's title block shares; \"\" clears), \
                 sheetWidth?, columns?, project?} → {name, properties}",
            )
            .enabled(always)
            .noundo(),
    ]
}

/// The 1-based sheet number `key` of `pr`, as an index.
fn sheet_index(cmd: &str, p: &Value, key: &str, pr: &Project) -> Result<usize> {
    let n = p.get(key).and_then(Value::as_u64).ok_or_else(|| bad(cmd, format!("`{key}` (sheet number, from 1) is required")))?;
    let n = usize::try_from(n).unwrap_or(usize::MAX);
    if n == 0 || n > pr.drawings.len() {
        return Err(bad(cmd, format!("`{key}` must be 1 to {}", pr.drawings.len())));
    }
    Ok(n - 1)
}

/// Run `f` on sheet `index` alone (opened if it is not open), saving it after when `save`.
fn on_sheet(
    s: &mut Session,
    path: &str,
    pr: &Project,
    index: usize,
    save: bool,
    f: impl FnMut(&mut Session, usize, u64) -> Result<Value>,
) -> Result<Value> {
    let one = Project { drawings: pr.drawings.get(index).cloned().into_iter().collect(), ..pr.clone() };
    let mut f = f;
    let r = for_each(s, path, &one, save, |s, _, _| f(s, index, index as u64 + 1))?;
    Ok(r.into_iter().next().map(|v| v["result"].clone()).unwrap_or(Value::Null))
}

/// After a change to the sheet list: sheet numbers in every title block, the cross-references
/// and the project database.
fn refresh(s: &mut Session, path: &str) -> Result<()> {
    let pr = read_project(path)?;
    let total = pr.drawings.len();
    let present: Vec<usize> = (0..total).filter(|i| pr.drawings.get(*i).is_some_and(|d| drawing_path(path, d).exists())).collect();
    for i in present {
        let file = pr.drawings.get(i).cloned().unwrap_or_default();
        on_sheet(s, path, &pr, i, true, |s, _, sheet| {
            super::titleblock::write_project_fields(s, &pr.name, &pr.properties, sheet, total, &file).map(|n| json!(n))
        })?;
    }
    if total > 0 {
        s.execute("eproject.xref", &json!({ "project": path }))?;
    }
    s.execute("eproject.sync", &json!({ "project": path }))?;
    Ok(())
}

fn run_sheets(s: &mut Session, p: &Value) -> Result<Value> {
    let path = project_path("eproject.sheets", p)?;
    let pr = read_project(&path)?;
    let mut sheets = Vec::with_capacity(pr.drawings.len());
    for (i, rel) in pr.drawings.iter().enumerate() {
        let full = drawing_path(&path, rel);
        let open = s.docs.iter().find(|d| d.path.as_deref().is_some_and(|q| same_file(Path::new(q), &full)));
        let dirty = open.is_some_and(|d| d.is_dirty());
        let is_open = open.is_some();
        let exists = full.exists();
        let fields = if exists || is_open {
            on_sheet(s, &path, &pr, i, false, |s, _, _| Ok(super::titleblock::values(s)?.map(Value::Object).unwrap_or(Value::Null)))
                .unwrap_or(Value::Null)
        } else {
            Value::Null
        };
        sheets.push(json!({ "sheet": i + 1, "drawing": rel, "path": full.to_string_lossy(), "exists": exists, "open": is_open, "dirty": dirty, "fields": fields }));
    }
    let template = pr.title_block().to_string();
    let fields = title_fields(s, &path, &pr, &template);
    Ok(json!({
        "project": path, "name": pr.name, "properties": pr.properties, "sheetWidth": pr.sheet_width, "columns": pr.columns, "sheets": sheets,
        "titleBlock": template, "titleFields": fields,
    }))
}

/// The fields of the project's title block: a built-in one's, or those of the block of your own
/// on the first sheet that has it.
fn title_fields(s: &mut Session, path: &str, pr: &Project, template: &str) -> Vec<Value> {
    if super::titleblock::is_template(template) {
        return super::titleblock::field_list(s, template);
    }
    for i in 0..pr.drawings.len() {
        if !pr.drawings.get(i).is_some_and(|d| drawing_path(path, d).exists()) {
            continue;
        }
        let r = on_sheet(s, path, pr, i, false, |s, _, _| Ok(json!(super::titleblock::field_list(s, template))));
        if let Ok(Value::Array(v)) = r
            && !v.is_empty()
        {
            return v;
        }
    }
    Vec::new()
}

fn run_titleblock(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "eproject.titleblock";
    let path = project_path(CMD, p)?;
    let mut pr = read_project(&path)?;
    let template =
        match (str_param(p, "template").map(str::trim).filter(|t| !t.is_empty()), str_param(p, "file").map(str::trim).filter(|t| !t.is_empty())) {
            (Some(t), None) if super::titleblock::is_template(t) => t.to_ascii_uppercase(),
            (Some(t), None) => {
                return Err(bad(CMD, format!("`template` must be ETITLE or ETITLE_A3, not \"{t}\" (a file of your own goes in `file`)")));
            }
            (None, Some(f)) => {
                let ext = Path::new(f).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
                if !matches!(ext.as_str(), "dxf" | "dwg") {
                    return Err(bad(CMD, "`file` must be a .dxf or .dwg drawing"));
                }
                if !Path::new(f).exists() {
                    return Err(EngineError::Other(format!("{f} does not exist.")));
                }
                f.to_string()
            }
            _ => return Err(bad(CMD, "give either `template` (ETITLE, ETITLE_A3) or `file`")),
        };
    let keep_border = p.get("keepBorder").and_then(Value::as_bool).unwrap_or(false);
    for (k, v) in super::titleblock::template_defaults(&template) {
        pr.properties.entry((*k).to_string()).or_insert_with(|| (*v).to_string());
    }
    pr.title_block = template.clone();
    write_project(&path, &pr)?;
    let total = pr.drawings.len();
    let present = Project { drawings: pr.drawings.iter().filter(|d| drawing_path(&path, d).exists()).cloned().collect(), ..pr.clone() };
    let mut done = 0usize;
    for (i, file) in pr.drawings.iter().enumerate() {
        if !present.drawings.contains(file) {
            continue;
        }
        on_sheet(s, &path, &pr, i, true, |s, _, sheet| {
            super::titleblock::replace(s, &template, pr.sheet_width, keep_border)?;
            super::titleblock::write_project_fields(s, &pr.name, &pr.properties, sheet, total, file).map(|n| json!(n))
        })?;
        done += 1;
    }
    s.log.push(format!("Title block {template} on {done} sheet(s)."));
    let fields = title_fields(s, &path, &pr, &template);
    Ok(json!({ "titleBlock": template, "sheets": done, "fields": fields }))
}

fn run_move(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "eproject.move";
    let path = project_path(CMD, p)?;
    let mut pr = read_project(&path)?;
    let from = sheet_index(CMD, p, "sheet", &pr)?;
    let to = sheet_index(CMD, p, "to", &pr)?;
    if from == to {
        return Ok(json!({ "drawings": opened_json(&path, &pr)["drawings"], "renumberSuggested": false }));
    }
    let d = pr.drawings.remove(from);
    pr.drawings.insert(to, d.clone());
    write_project(&path, &pr)?;
    refresh(s, &path)?;
    s.log.push(format!("{d} is now sheet {}.", to + 1));
    Ok(json!({ "drawings": opened_json(&path, &pr)["drawings"], "renumberSuggested": true }))
}

fn run_rename(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "eproject.rename";
    let path = project_path(CMD, p)?;
    let mut pr = read_project(&path)?;
    let i = sheet_index(CMD, p, "sheet", &pr)?;
    let name = str_param(p, "name").map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad(CMD, "`name` (new file name) is required"))?;
    if name.chars().count() > 120 || name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']) || name.starts_with('.') {
        return Err(bad(CMD, "`name` must be a plain file name (no folders or \\ / : * ? \" < > |)"));
    }
    let name = if Path::new(name).extension().is_some_and(|e| e.eq_ignore_ascii_case("dxf")) { name.to_string() } else { format!("{name}.dxf") };
    let old_rel = pr.drawings.get(i).cloned().unwrap_or_default();
    let old = drawing_path(&path, &old_rel);
    // The new name sits where the old file is (same folder).
    let new_rel = match Path::new(&old_rel).parent().filter(|d| !d.as_os_str().is_empty()) {
        Some(dir) => dir.join(&name).to_string_lossy().replace('\\', "/"),
        None => name.clone(),
    };
    let new = drawing_path(&path, &new_rel);
    if new.exists() && !same_file(&new, &old) {
        return Err(EngineError::Other(format!("{} already exists.", new.display())));
    }
    if pr.drawings.iter().enumerate().any(|(j, d)| j != i && d.eq_ignore_ascii_case(&new_rel)) {
        return Err(EngineError::Other(format!("{new_rel} is already in the project.")));
    }
    let open = s.docs.iter().position(|d| d.path.as_deref().is_some_and(|q| same_file(Path::new(q), &old)));
    if let Some(ix) = open
        && s.docs.get(ix).is_some_and(|d| d.is_dirty())
    {
        return Err(EngineError::Other(format!("Save {old_rel} first: it has unsaved changes.")));
    }
    if old.exists() {
        rename_file(&old, &new)?;
    }
    if let Some(ix) = open
        && let Some(d) = s.docs.get_mut(ix)
    {
        d.path = Some(new.to_string_lossy().to_string());
        d.title = name.clone();
    }
    if let Some(slot) = pr.drawings.get_mut(i) {
        *slot = new_rel.clone();
    }
    write_project(&path, &pr)?;
    // The database keys pages by file name: the old page goes, the new one comes.
    s.execute("eproject.sync", &json!({ "project": path }))?;
    s.log.push(format!("Sheet {} renamed {old_rel} → {new_rel}.", i + 1));
    Ok(json!({ "drawing": new_rel, "sheet": i + 1 }))
}

#[cfg(not(target_arch = "wasm32"))]
fn rename_file(old: &Path, new: &Path) -> Result<()> {
    std::fs::rename(old, new).map_err(|e| EngineError::Other(format!("{}: {e}", old.display())))
}

#[cfg(target_arch = "wasm32")]
fn rename_file(_old: &Path, _new: &Path) -> Result<()> {
    Err(EngineError::Other("Projects need files, which the web version cannot reach.".into()))
}

fn run_remove(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "eproject.remove";
    let path = project_path(CMD, p)?;
    let mut pr = read_project(&path)?;
    let i = sheet_index(CMD, p, "sheet", &pr)?;
    let d = pr.drawings.remove(i);
    write_project(&path, &pr)?;
    refresh(s, &path)?;
    s.log.push(format!("{d} removed from the project (the file is kept)."));
    Ok(json!({ "drawings": opened_json(&path, &pr)["drawings"], "removed": d, "renumberSuggested": i < pr.drawings.len() }))
}

fn run_sheet(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "eproject.sheet";
    let path = project_path(CMD, p)?;
    let pr = read_project(&path)?;
    let i = sheet_index(CMD, p, "sheet", &pr)?;
    let set = super::titleblock::fields_param(p)?;
    let save = !set.is_empty();
    let fields = on_sheet(s, &path, &pr, i, save, |s, _, _| {
        if !set.is_empty() && super::titleblock::set_fields(s, &set)? == 0 {
            return Err(bad(CMD, "this sheet has no title block (Title Block adds one to sheets made by New Project or Add New Sheet)"));
        }
        Ok(super::titleblock::values(s)?.map(Value::Object).unwrap_or(Value::Null))
    })?;
    Ok(json!({ "sheet": i + 1, "fields": fields }))
}

fn run_props(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "eproject.props";
    let path = project_path(CMD, p)?;
    let mut pr = read_project(&path)?;
    let set = super::titleblock::fields_param(p)?;
    let mut changed = !set.is_empty();
    if let Some(name) = str_param(p, "name").map(str::trim) {
        if name.is_empty() || name.chars().count() > 120 {
            return Err(bad(CMD, "`name` must be 1 to 120 characters"));
        }
        pr.name = name.to_string();
        changed = true;
    }
    if let Some(w) = p.get("sheetWidth") {
        pr.sheet_width =
            w.as_f64().filter(|w| w.is_finite() && *w > 0.0 && *w <= 100_000.0).ok_or_else(|| bad(CMD, "`sheetWidth` must be positive"))?;
        changed = true;
    }
    if let Some(c) = p.get("columns") {
        pr.columns = c.as_u64().filter(|c| (1..=1000).contains(c)).ok_or_else(|| bad(CMD, "`columns` must be 1 to 1000"))?;
        changed = true;
    }
    for (k, v) in &set {
        let v = v.as_str().unwrap_or("");
        if v.is_empty() {
            pr.properties.remove(k);
        } else {
            pr.properties.insert(k.clone(), v.to_string());
        }
    }
    if pr.properties.len() > 128 {
        return Err(bad(CMD, "too many project properties"));
    }
    if changed {
        write_project(&path, &pr)?;
        // Every sheet's title block shows the project's name and shared fields (references
        // such as {JOBNO} resolved for each sheet); a field just cleared is cleared there too.
        let cleared: Map<String, Value> =
            set.iter().filter(|(_, v)| v.as_str().is_some_and(str::is_empty)).map(|(k, v)| (k.clone(), v.clone())).collect();
        let total = pr.drawings.len();
        for (i, file) in pr.drawings.iter().enumerate() {
            if !drawing_path(&path, file).exists() {
                continue;
            }
            on_sheet(s, &path, &pr, i, true, |s, _, sheet| {
                super::titleblock::set_fields(s, &cleared)?;
                super::titleblock::write_project_fields(s, &pr.name, &pr.properties, sheet, total, file).map(|n| json!(n))
            })?;
        }
        s.log.push(format!("Project properties written to {total} sheet(s)."));
    }
    Ok(
        json!({ "name": pr.name, "properties": pr.properties, "sheetWidth": pr.sheet_width, "columns": pr.columns, "drawings": pr.drawings.len().min(MAX_DRAWINGS) }),
    )
}
