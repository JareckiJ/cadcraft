//! ECHECK: the check run (EPLAN's "Check project", AutoCAD Electrical's audit) on this drawing or
//! every sheet of the open project: duplicate device tags, contacts with no coil, two potential
//! names on one network (a short between them), empty ladder rungs shorting the rails, and symbol
//! pins wired to nothing.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde_json::{Value, json};

use super::wires::{POTENTIAL_LAYER, Seg, WIRE_LAYERS, labels, potentials, wire_segments};
use crate::cmd::{CommandSpec, str_param};
use crate::{Result, Session};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("echeck", "Check Project", run_check)
            .menu(&["Tools", "Electrical", "Check Project"])
            .params(
                "{project? (.ecproj; default: the open project, else this drawing), unconnected?: bool (list pins wired to nothing, default true)} \
                 → {errors, warnings, problems: [{severity: error | warning, kind, sheet, message, handle?}]}",
            )
            .noundo(),
    ]
}

/// What one sheet holds for the project-wide checks, and its own problems.
struct SheetFacts {
    main: Vec<(String, String)>, // (tag, handle)
    contacts: Vec<(String, String)>,
    problems: Vec<Value>,
}

fn problem(severity: &str, kind: &str, sheet: Option<u64>, message: String, handle: Option<String>) -> Value {
    let mut v = json!({ "severity": severity, "kind": kind, "sheet": sheet, "message": message });
    if let Some(h) = handle {
        v["handle"] = json!(h);
    }
    v
}

fn facts(s: &mut Session, sheet: Option<u64>, unconnected: bool) -> Result<SheetFacts> {
    let overrides = HashMap::new();
    let mut f = SheetFacts { main: Vec::new(), contacts: Vec::new(), problems: Vec::new() };
    for d in super::devices(s, None, None)? {
        if d.tag.is_empty() || d.family.as_deref() == Some("TERMINAL") || d.block.to_ascii_uppercase().starts_with(super::titleblock::TITLE_BLOCK) {
            continue;
        }
        if super::is_child(&d, &overrides) {
            f.contacts.push((d.tag.clone(), d.handle.hex()));
        } else {
            f.main.push((d.tag.clone(), d.handle.hex()));
        }
    }

    // Two potential names on one network.
    let (handles, segs) = wire_segments(s, WIRE_LAYERS)?;
    let groups = potentials(handles.len(), &segs, 0.01);
    let names = labels(s, POTENTIAL_LAYER)?;
    for g in &groups {
        let gsegs: Vec<&Seg> = segs.iter().filter(|sg| g.contains(&sg.wire)).collect();
        let on: HashSet<&str> =
            names.iter().filter(|(_, at, v)| !v.is_empty() && gsegs.iter().any(|sg| sg.dist_to(*at) <= 6.0)).map(|(_, _, v)| v.as_str()).collect();
        if on.len() > 1 {
            let mut list: Vec<&str> = on.into_iter().collect();
            list.sort();
            let first = g.first().and_then(|w| handles.get(*w)).map(|h| h.hex());
            f.problems.push(problem("error", "short", sheet, format!("{} are connected: a short circuit", list.join(" and ")), first));
        }
    }

    let rungs = super::wiring::empty_rungs(s)?;
    if !rungs.is_empty() {
        f.problems.push(problem(
            "warning",
            "empty-rungs",
            sheet,
            format!("{} empty ladder rung(s) join the rails directly (Remove Empty Rungs)", rungs.len()),
            rungs.first().map(|r| r.0.hex()),
        ));
    }

    if unconnected {
        let tags = super::terminals::tags_by_handle(s)?;
        let nets = super::wires::networks(s)?;
        let wired: HashSet<(cadcraft_doc::Handle, String)> = nets.iter().flat_map(|n| n.pins.iter().map(|(h, p, _)| (*h, p.clone()))).collect();
        let ix = super::pins::PinIndex::build(s)?;
        let blocks: HashMap<cadcraft_doc::Handle, String> = {
            let d = s.doc()?;
            d.space(&s.space())
                .map(|st| {
                    st.iter()
                        .filter_map(|e| match &e.kind {
                            cadcraft_doc::EntityKind::Insert(i) => Some((e.handle, i.block.to_ascii_uppercase())),
                            _ => None,
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        // Spare PLC points and drive terminals, and cable symbols are not expected to be wired.
        let mut open: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for p in &ix.pins {
            let block = blocks.get(&p.insert).map(String::as_str).unwrap_or("");
            if block.starts_with("PLC_") || block.starts_with("VFD_") || block.starts_with("ECABLE") || wired.contains(&(p.insert, p.number.clone()))
            {
                continue;
            }
            let tag = tags.get(&p.insert).cloned().unwrap_or_else(|| "?".into());
            open.entry(tag).or_default().push(if p.number.is_empty() { "?".into() } else { p.number.clone() });
        }
        for (tag, pins) in open {
            f.problems.push(problem("warning", "unconnected", sheet, format!("{tag}: pin(s) {} wired to nothing", pins.join(", ")), None));
        }
    }
    Ok(f)
}

fn run_check(s: &mut Session, p: &Value) -> Result<Value> {
    let unconnected = p.get("unconnected").and_then(Value::as_bool).unwrap_or(true);
    let project = str_param(p, "project").map(str::to_string).or_else(super::project::current);
    let mut sheets: Vec<(Option<u64>, SheetFacts)> = Vec::new();
    match project {
        Some(path) => {
            let pr = super::project::read_project(&path)?;
            let mut err = None;
            super::project::for_each(s, &path, &pr, false, |s, _, sheet| {
                match facts(s, Some(sheet), unconnected) {
                    Ok(f) => sheets.push((Some(sheet), f)),
                    Err(e) => err = Some(e),
                }
                Ok(Value::Null)
            })?;
            if let Some(e) = err {
                return Err(e);
            }
        }
        None => sheets.push((None, facts(s, None, unconnected)?)),
    }

    let mut problems = Vec::new();
    // Duplicate main tags, across sheets.
    let mut seen: BTreeMap<&str, Vec<(Option<u64>, &str)>> = BTreeMap::new();
    for (sheet, f) in &sheets {
        for (tag, h) in &f.main {
            seen.entry(tag.as_str()).or_default().push((*sheet, h.as_str()));
        }
    }
    for (tag, at) in seen.iter().filter(|(_, at)| at.len() > 1) {
        let wher: Vec<String> = at.iter().map(|(sh, _)| sh.map_or_else(|| "this sheet".to_string(), |n| format!("sheet {n}"))).collect();
        problems.push(problem(
            "error",
            "duplicate-tag",
            at.first().and_then(|a| a.0),
            format!("{tag} is used {} times ({})", at.len(), wher.join(", ")),
            at.first().map(|a| a.1.to_string()),
        ));
    }
    // Contacts with no coil anywhere.
    for (sheet, f) in &sheets {
        for (tag, h) in &f.contacts {
            if !seen.contains_key(tag.as_str()) {
                problems.push(problem("warning", "orphan-contact", *sheet, format!("contact {tag} has no coil or main device"), Some(h.clone())));
            }
        }
    }
    for (_, f) in sheets {
        problems.extend(f.problems);
    }
    let errors = problems.iter().filter(|p| p["severity"] == "error").count();
    let warnings = problems.len() - errors;
    s.log.push(format!("Check: {errors} error(s), {warnings} warning(s)."));
    for p in problems.iter().take(50) {
        let sheet = p["sheet"].as_u64().map(|n| format!("sheet {n}: ")).unwrap_or_default();
        s.log.push(format!("  {} {sheet}{}", if p["severity"] == "error" { "✖" } else { "!" }, p["message"].as_str().unwrap_or("")));
    }
    Ok(json!({ "errors": errors, "warnings": warnings, "problems": problems }))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_check_finds_shorts_duplicates_and_open_pins() {
        let mut s = Session::new();
        s.execute("eladder", &json!({ "top": 200, "bottom": 50, "to": 84, "potentials": ["+24V", "0V"] })).unwrap();
        let r = s.execute("echeck", &json!({ "unconnected": false })).unwrap();
        let kinds: Vec<&str> = r["problems"].as_array().unwrap().iter().filter_map(|p| p["kind"].as_str()).collect();
        assert!(kinds.contains(&"short") && kinds.contains(&"empty-rungs"), "{r}");
        s.execute("esymbol", &json!({ "name": "IEC_COIL", "at": [21, 120] })).unwrap();
        s.execute("eladder.clean", &json!({})).unwrap();
        let r = s.execute("echeck", &json!({})).unwrap();
        assert_eq!(r["errors"], 0, "{r}");
        // A second coil with the first one's tag, a contact with no coil, a lamp wired to nothing.
        s.execute("esymbol", &json!({ "name": "IEC_COIL", "at": [300, 120], "tag": "-K101" })).unwrap();
        s.execute("esymbol", &json!({ "name": "IEC_NO", "at": [300, 220], "tag": "-K999" })).unwrap();
        let r = s.execute("echeck", &json!({})).unwrap();
        let kinds: Vec<&str> = r["problems"].as_array().unwrap().iter().filter_map(|p| p["kind"].as_str()).collect();
        assert!(kinds.contains(&"duplicate-tag") && kinds.contains(&"orphan-contact") && kinds.contains(&"unconnected"), "{r}");
    }
}
