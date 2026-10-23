//! `cadcraft-cli generate`: a schematic project from a bill of materials and a netlist.
//!
//! ```text
//! cadcraft-cli generate --bom BOM.csv --netlist NETS.csv [--template DIR] (--out DIR [--name NAME] | --emit FILE|-)
//! ```
//!
//! `--out` builds a project in DIR: `NAME.ecproj` with one DXF drawing per sheet, symbols wired,
//! tagged, wire-numbered and cross-referenced, and the project database. `--emit` writes the
//! plan instead, as control-channel requests (JSON lines), to drive a running CADCraft
//! (`cadcraft --control PORT`): every sheet side by side in its current drawing.

use std::path::Path;

use cadcraft_engine::Session;
use cadcraft_engine::cmd::electrical::generate::{Template, plan};
use serde_json::{Value, json};

use crate::OptionSpec;

pub const OPTIONS: &[OptionSpec] = &[
    OptionSpec { name: "--bom", takes_value: true },
    OptionSpec { name: "--netlist", takes_value: true },
    OptionSpec { name: "--template", takes_value: true },
    OptionSpec { name: "--out", takes_value: true },
    OptionSpec { name: "--name", takes_value: true },
    OptionSpec { name: "--emit", takes_value: true },
];

/// Largest input file read.
const MAX_FILE: u64 = 4 << 20;

fn value<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(String::as_str)
}

fn read(path: &str) -> Result<String, String> {
    let len = std::fs::metadata(path).map_err(|e| format!("{path}: {e}"))?.len();
    if len > MAX_FILE {
        return Err(format!("{path}: larger than 4 MB"));
    }
    std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))
}

/// The template from DIR/template.json (or a .json file named directly), else the defaults.
fn template(dir: Option<&str>) -> Result<Template, String> {
    let Some(dir) = dir else { return Ok(Template::default()) };
    let p = Path::new(dir);
    let file = if p.is_dir() { p.join("template.json") } else { p.to_path_buf() };
    if !file.exists() {
        return Err(format!("{}: no template.json", p.display()));
    }
    let text = read(&file.to_string_lossy())?;
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(Template::from_json(&v))
}

pub fn generate(args: &[String]) -> Result<(), String> {
    crate::validate_options("generate", args, OPTIONS)?;
    let bom = read(value(args, "--bom").ok_or("generate: --bom BOM.csv is required")?)?;
    let netlist = match value(args, "--netlist") {
        Some(p) => read(p)?,
        None => String::new(),
    };
    let t = template(value(args, "--template"))?;
    if let Some(target) = value(args, "--emit") {
        return emit(&bom, &netlist, &t, target);
    }
    let out = value(args, "--out").ok_or("generate: --out DIR or --emit FILE is required")?;
    build(&bom, &netlist, &t, out, value(args, "--name"))
}

/// The plan as control-channel requests, one JSON object per line.
fn emit(bom: &str, netlist: &str, t: &Template, target: &str) -> Result<(), String> {
    let p = plan(bom, netlist, t, true, &|_| false).map_err(|e| e.to_string())?;
    let mut lines = String::new();
    for (i, st) in p.steps.iter().enumerate() {
        let req = json!({ "id": i + 1, "method": "engine.execute", "params": { "command": st.command, "params": st.params } });
        lines.push_str(&req.to_string());
        lines.push('\n');
    }
    for w in &p.warnings {
        eprintln!("warning: {w}");
    }
    if target == "-" {
        outln!("{}", lines.trim_end());
    } else {
        std::fs::write(target, &lines).map_err(|e| format!("{target}: {e}"))?;
        eprintln!("wrote {target}: {} request(s) for {} sheet(s)", p.steps.len(), p.sheets);
    }
    Ok(())
}

/// A project in `out`: plan per sheet, run, save, then number and cross-reference it all.
fn build(bom: &str, netlist: &str, t: &Template, out: &str, name: Option<&str>) -> Result<(), String> {
    let p = plan(bom, netlist, t, false, &|_| false).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(out).map_err(|e| format!("{out}: {e}"))?;
    let name = name.unwrap_or(&t.name);
    let stem: String = name.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    let project = Path::new(out).join(format!("{stem}.ecproj")).to_string_lossy().to_string();
    let mut s = Session::empty();
    let x = |s: &mut Session, id: &str, v: Value| s.execute(id, &v).map_err(|e| format!("{id}: {e}"));
    x(&mut s, "eproject.new", json!({ "path": project, "name": name, "sheets": p.sheets, "overwrite": true }))?;
    x(&mut s, "eproject.open", json!({ "path": project }))?;
    let mut failed = 0usize;
    for sheet in 1..=p.sheets {
        let file = format!("{stem}-{sheet:02}.dxf");
        let i = s.docs.iter().position(|d| d.path.as_deref().is_some_and(|q| q.ends_with(&file))).ok_or(format!("sheet {sheet} did not open"))?;
        x(&mut s, "document.switch", json!({ "index": i }))?;
        for st in p.steps.iter().filter(|st| st.sheet == sheet) {
            if let Err(e) = s.execute(st.command, &st.params) {
                failed += 1;
                eprintln!("sheet {sheet}: {} failed: {e}", st.command);
            }
        }
        x(&mut s, "qsave", json!({}))?;
    }
    for id in ["eproject.sync", "eproject.xref"] {
        x(&mut s, id, json!({ "project": project }))?;
    }
    for w in &p.warnings {
        eprintln!("warning: {w}");
    }
    eprintln!(
        "wrote {project}: {} component(s) on {} sheet(s){}",
        p.components,
        p.sheets,
        if failed > 0 { format!(", {failed} step(s) failed") } else { String::new() }
    );
    Ok(())
}
