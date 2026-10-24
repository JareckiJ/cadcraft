//! The sheet title block, AutoCAD Electrical style: one block in every sheet whose attributes
//! the project fills in. Project properties (customer, title, drawn by, job number…) go into
//! every sheet, each sheet keeps its own fields (description, location), and the sheet numbers
//! are the project's. Change a project property and every sheet follows.
//!
//! Title blocks:
//! - `ETITLE`, the compact block in the bottom-right corner;
//! - `ETITLE_A3`, a full-width strip in the style of a consultancy's A3 sheets (company,
//!   designed for, customer, title, description, INST/LOC, drawn/checked/date, modified, drawing
//!   reference, page N of M);
//! - your own: any drawing file (EPROJECT.TITLEBLOCK `file`), its attributes named after the
//!   fields they show (`CUSTOMER1`, `DWGNO`, anything). Such a block is marked with an invisible
//!   `ETB` attribute (AutoCAD Electrical title blocks carry `WD_TB`, which works too).
//!
//! A field's value may refer to other fields: `{JOBNO} / {FILENAME} - {REV}` becomes
//! `7477 / A1 PLC.P4 - B` on each sheet. `SHEET`, `SHEETS`, `PROJECT` and `FILENAME` (the
//! sheet's file name without extension) are always known.

use std::sync::Arc;

use cadcraft_doc::{Attrib, AttribModes, Block, Common, Drawing, Entity, EntityKind, HAlign, Insert, Line, Space, Text, VAlign};
use cadcraft_geom::{Vec2, Vec3};
use serde_json::{Map, Value, json};

use crate::cmd::{CommandSpec, bad, bool_or};
use crate::{Result, Session};

pub(super) const TITLE_BLOCK: &str = "ETITLE";
pub(super) const TITLE_A3: &str = "ETITLE_A3";
/// The attribute that marks an imported block as the title block.
pub(super) const MARKER: &str = "ETB";
/// Fields the project fills in itself.
const AUTO: &[&str] = &["SHEET", "SHEETS"];
/// Longest field value, and most references replaced in one.
const MAX_VALUE: usize = 255;
const MAX_REFS: usize = 32;

/// Width and height of the compact title block (drawing units, mm on an A3 sheet).
const W: f64 = 170.0;
const H: f64 = 30.0;

/// Where a field is edited: on each sheet, for the whole project, or both.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Scope {
    Sheet,
    Project,
    Both,
}

impl Scope {
    fn name(self) -> &'static str {
        match self {
            Scope::Sheet => "sheet",
            Scope::Project => "project",
            Scope::Both => "both",
        }
    }
}

/// The compact block's fields: attribute tag, label shown, cell (x from the block's right edge,
/// y from its bottom, width), text height, scope.
const FIELDS: &[(&str, &str, f64, f64, f64, f64, Scope)] = &[
    ("PROJECT", "Project", -170.0, 20.0, 80.0, 3.0, Scope::Project),
    ("TITLE", "Title", -170.0, 10.0, 80.0, 3.0, Scope::Sheet),
    ("CUSTOMER", "Customer", -170.0, 0.0, 80.0, 2.5, Scope::Project),
    ("DRAWINGNO", "Drawing no.", -90.0, 20.0, 50.0, 3.0, Scope::Sheet),
    ("DATE", "Date", -90.0, 10.0, 25.0, 2.5, Scope::Both),
    ("REV", "Rev.", -65.0, 10.0, 25.0, 2.5, Scope::Both),
    ("DRAWNBY", "Drawn", -90.0, 0.0, 25.0, 2.5, Scope::Both),
    ("CHECKEDBY", "Checked", -65.0, 0.0, 25.0, 2.5, Scope::Both),
    ("SHEET", "Sheet", -40.0, 15.0, 20.0, 5.0, Scope::Sheet),
    ("SHEETS", "of", -20.0, 15.0, 20.0, 5.0, Scope::Sheet),
];

/// The A3 strip: 420 wide, 22 high, its base the bottom-right corner. Each field: tag, label
/// (for the editors), x from the strip's left edge, y, text height, alignment, scope.
const A3_W: f64 = 420.0;
const A3_H: f64 = 22.0;
const A3_FIELDS: &[(&str, &str, f64, f64, f64, HAlign, Scope)] = &[
    ("COMPANY1", "Company", 47.5, 15.5, 5.0, HAlign::Center, Scope::Project),
    ("COMPANY2", "Company line 2", 47.5, 10.5, 2.5, HAlign::Center, Scope::Project),
    ("COMPANY3", "Address", 47.5, 7.0, 1.8, HAlign::Center, Scope::Project),
    ("COMPANY4", "Address line 2", 47.5, 4.5, 1.8, HAlign::Center, Scope::Project),
    ("COMPANY5", "Telephone", 47.5, 2.0, 1.8, HAlign::Center, Scope::Project),
    ("DESIGNEDFOR1", "Designed for", 139.0, 16.5, 2.5, HAlign::Center, Scope::Project),
    ("DESIGNEDFOR2", "Designed for line 2", 139.0, 13.5, 2.2, HAlign::Center, Scope::Project),
    ("NOTE1", "Note line 1", 139.0, 9.0, 1.6, HAlign::Center, Scope::Project),
    ("NOTE2", "Note line 2", 139.0, 6.8, 1.6, HAlign::Center, Scope::Project),
    ("NOTE3", "Note line 3", 139.0, 4.6, 1.6, HAlign::Center, Scope::Project),
    ("NOTE4", "Note line 4", 139.0, 2.4, 1.6, HAlign::Center, Scope::Project),
    ("CUSTOMER1", "Customer", 220.5, 16.5, 2.5, HAlign::Center, Scope::Project),
    ("CUSTOMER2", "Customer line 2", 220.5, 13.5, 2.5, HAlign::Center, Scope::Project),
    ("TITLE", "Title", 220.5, 4.5, 3.0, HAlign::Center, Scope::Project),
    ("DESCRIPTION", "Description", 286.5, 16.5, 2.5, HAlign::Center, Scope::Sheet),
    ("DESCRIPTION2", "Description line 2", 286.5, 12.5, 2.5, HAlign::Center, Scope::Sheet),
    ("INST", "Installation", 268.0, 6.75, 2.0, HAlign::Left, Scope::Sheet),
    ("LOC", "Location", 268.0, 2.25, 2.0, HAlign::Left, Scope::Sheet),
    ("DRAWN", "Drawn", 324.5, 15.0, 2.5, HAlign::Center, Scope::Project),
    ("CHECKED", "Checked", 343.0, 15.0, 2.5, HAlign::Center, Scope::Project),
    ("DATE", "Date", 362.0, 15.0, 2.5, HAlign::Center, Scope::Project),
    ("MODIFIED", "Modified", 324.5, 4.5, 2.5, HAlign::Center, Scope::Project),
    ("MODCHECKED", "Modified, checked", 343.0, 4.5, 2.5, HAlign::Center, Scope::Project),
    ("MODDATE", "Modified, date", 362.0, 4.5, 2.5, HAlign::Center, Scope::Project),
    ("DRAWINGREF", "Drawing ref.", 396.0, 15.0, 2.5, HAlign::Center, Scope::Project),
    ("SHEET", "Page", 393.0, 4.5, 3.0, HAlign::Right, Scope::Sheet),
    ("SHEETS", "of", 399.0, 4.5, 3.0, HAlign::Left, Scope::Sheet),
];
/// Project properties the A3 strip uses through references, not shown in it directly.
const A3_EXTRA: &[(&str, &str)] = &[("JOBNO", "Job no."), ("REV", "Revision")];
/// Values a project gets when it changes to the A3 strip (it keeps any it already has).
const A3_DEFAULTS: &[(&str, &str)] = &[
    ("NOTE1", "THIS DRAWING IS PRIVATE AND CONFIDENTIAL. IT"),
    ("NOTE2", "MUST NOT BE COPIED OR COMMUNICATED TO OTHER"),
    ("NOTE3", "PARTIES WITHOUT WRITTEN CONSENT OF"),
    ("NOTE4", "{COMPANY1}"),
    ("DRAWINGREF", "{JOBNO} / {FILENAME} - {REV}"),
];

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec::new("etitle", "Title Block", run_title).menu(&["Tools", "Electrical", "Title Block..."]).params(
        "{set?: {FIELD: value, …} (any attribute of the title block: TITLE, CUSTOMER1, DESCRIPTION, LOC…; {OTHER} refers to another field), \
             project?: bool (every sheet of the current project, numbering SHEET and SHEETS)} → {sheets, values}",
    )]
}

pub(super) fn is_template(name: &str) -> bool {
    name.eq_ignore_ascii_case(TITLE_BLOCK) || name.eq_ignore_ascii_case(TITLE_A3)
}

/// Whether block reference `ins` is a title block: a built-in one, or one marked as such.
pub(super) fn is_title_block(ins: &Insert) -> bool {
    ins.block.to_ascii_uppercase().starts_with(TITLE_BLOCK)
        || ins.attribs.iter().any(|a| a.tag.eq_ignore_ascii_case(MARKER) || a.tag.eq_ignore_ascii_case("WD_TB"))
}

fn text(at: Vec2, height: f64, value: &str, halign: HAlign) -> Text {
    let p = Vec3::new(at.x, at.y, 0.0);
    Text {
        insert: p,
        align_pt: Some(p),
        height,
        value: value.to_string(),
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign,
        valign: VAlign::Middle,
    }
}

fn attrib(tag: &str, at: Vec2, height: f64, value: &str, halign: HAlign) -> Attrib {
    Attrib {
        tag: tag.into(),
        text: text(at, height, value, halign),
        invisible: false,
        constant: false,
        prompt: tag.into(),
        props: Default::default(),
        multiline: None,
        modes: AttribModes::default(),
    }
}

/// Where a compact field's value goes, relative to the block's base (bottom-right corner).
fn value_at(x: f64, y: f64) -> Vec2 {
    Vec2::new(x + 2.0, y + 4.0)
}

fn line(a: (f64, f64), b: (f64, f64)) -> EntityKind {
    EntityKind::Line(Line { a: Vec3::new(a.0, a.1, 0.0), b: Vec3::new(b.0, b.1, 0.0) })
}

/// The attribute definitions (tag, position, height, alignment) of built-in title block `name`.
fn template_attdefs(name: &str) -> Vec<(&'static str, &'static str, Vec2, f64, HAlign)> {
    if name.eq_ignore_ascii_case(TITLE_A3) {
        A3_FIELDS.iter().map(|(tag, label, x, y, h, al, _)| (*tag, *label, Vec2::new(x - A3_W, *y), *h, *al)).collect()
    } else {
        FIELDS.iter().map(|(tag, label, x, y, _, h, _)| (*tag, *label, value_at(*x, *y), *h, HAlign::Left)).collect()
    }
}

/// Define built-in title block `name` in `d` when it is missing.
fn define_template(d: &mut Drawing, name: &str) {
    let name = if name.eq_ignore_ascii_case(TITLE_A3) { TITLE_A3 } else { TITLE_BLOCK };
    if d.block(name).is_some() {
        return;
    }
    let mut kinds: Vec<EntityKind> = Vec::new();
    if name == TITLE_A3 {
        let x = |v: f64| v - A3_W;
        kinds.extend([
            line((x(0.0), 0.0), (0.0, 0.0)),
            line((0.0, 0.0), (0.0, A3_H)),
            line((0.0, A3_H), (x(0.0), A3_H)),
            line((x(0.0), A3_H), (x(0.0), 0.0)),
        ]);
        for v in [95.0, 183.0, 258.0, 315.0, 334.0, 352.0, 372.0] {
            kinds.push(line((x(v), 0.0), (x(v), A3_H)));
        }
        kinds.push(line((x(95.0), 11.0), (x(258.0), 11.0)));
        kinds.push(line((x(258.0), 9.0), (x(315.0), 9.0)));
        kinds.push(line((x(258.0), 4.5), (x(315.0), 4.5)));
        kinds.push(line((x(315.0), 11.0), (0.0, 11.0)));
        for (label, lx, ly) in [
            ("DESIGNED FOR", 96.0, 20.5),
            ("CUSTOMER", 184.0, 20.5),
            ("TITLE", 184.0, 9.5),
            ("DESCRIPTION", 259.0, 20.5),
            ("INST", 259.0, 6.75),
            ("LOC", 259.0, 2.25),
            ("DRAWN", 316.0, 20.5),
            ("CHECKED", 335.0, 20.5),
            ("DATE", 353.0, 20.5),
            ("DRAWING REF", 373.0, 20.5),
            ("MODIFIED", 316.0, 9.5),
            ("CHECKED", 335.0, 9.5),
            ("DATE", 353.0, 9.5),
            ("PAGE #", 373.0, 9.5),
        ] {
            kinds.push(EntityKind::Text(text(Vec2::new(x(lx), ly), 1.5, label, HAlign::Left)));
        }
        kinds.push(EntityKind::Text(text(Vec2::new(x(396.0), 4.5), 1.8, "OF", HAlign::Center)));
    } else {
        kinds.extend([
            line((-W, 0.0), (0.0, 0.0)),
            line((0.0, 0.0), (0.0, H)),
            line((0.0, H), (-W, H)),
            line((-W, H), (-W, 0.0)),
            line((-W, 10.0), (-40.0, 10.0)),
            line((-W, 20.0), (-40.0, 20.0)),
            line((-90.0, 0.0), (-90.0, H)),
            line((-65.0, 0.0), (-65.0, 20.0)),
            line((-40.0, 0.0), (-40.0, H)),
            line((-20.0, 0.0), (-20.0, H)),
        ]);
        for (_, label, x, y, _, _, _) in FIELDS {
            kinds.push(EntityKind::Text(text(Vec2::new(x + 1.0, y + 8.0), 1.5, label, HAlign::Left)));
        }
    }
    for (tag, label, at, h, al) in template_attdefs(name) {
        let mut a = attrib(tag, at, h, "", al);
        a.prompt = label.into();
        kinds.push(EntityKind::AttDef(a));
    }
    let mut b = Block::new(name);
    b.description = "Sheet title block".into();
    for k in kinds {
        b.entities.push(Entity { handle: d.new_handle(), common: Common::default(), kind: k });
    }
    d.blocks.insert(name.into(), Arc::new(b));
}

/// The scale a built-in title block gets on a sheet `sheet_width` wide: the A3 strip spans the
/// sheet, the compact block keeps its size.
fn template_scale(name: &str, sheet_width: f64) -> f64 {
    if name.eq_ignore_ascii_case(TITLE_A3) && sheet_width.is_finite() && sheet_width > 0.0 { sheet_width / A3_W } else { 1.0 }
}

/// Put built-in title block `name` into drawing `d`'s model space at the bottom-right corner of a
/// sheet `sheet_width` wide.
pub(super) fn add_template(d: &mut Drawing, name: &str, sheet_width: f64, values: &Map<String, Value>) -> Option<cadcraft_doc::Handle> {
    define_template(d, name);
    let name = if name.eq_ignore_ascii_case(TITLE_A3) { TITLE_A3 } else { TITLE_BLOCK };
    let k = template_scale(name, sheet_width);
    let corner = Vec2::new(sheet_width, 0.0);
    let attribs = template_attdefs(name)
        .into_iter()
        .map(|(tag, _, at, h, al)| {
            let v = values.get(tag).and_then(Value::as_str).unwrap_or("");
            attrib(tag, corner + at * k, h * k, v, al)
        })
        .collect();
    let ins = Insert {
        block: name.into(),
        insert: Vec3::new(corner.x, corner.y, 0.0),
        scale: Vec3::new(k, k, k),
        rotation: 0.0,
        attribs,
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    };
    d.add(&Space::Model, Common { layer: "FRAME".into(), ..Common::default() }, EntityKind::Insert(ins)).ok()
}

/// Put the compact title block into drawing `d` with its bottom-right corner at `corner`.
pub(super) fn add_to(d: &mut Drawing, corner: Vec2, values: &Map<String, Value>) {
    let _ = add_template(d, TITLE_BLOCK, corner.x, values);
}

/// The fields of the active drawing's (first) title block, or `None` when it has none.
pub(super) fn values(s: &Session) -> Result<Option<Map<String, Value>>> {
    let d = s.doc()?;
    let Some(store) = d.space(&s.space()) else { return Ok(None) };
    Ok(store.iter().find_map(|e| match &e.kind {
        EntityKind::Insert(i) if is_title_block(i) => Some(
            i.attribs
                .iter()
                .filter(|a| !a.tag.eq_ignore_ascii_case(MARKER) && !a.tag.eq_ignore_ascii_case("WD_TB"))
                .map(|a| (a.tag.to_ascii_uppercase(), json!(a.text.value)))
                .collect(),
        ),
        _ => None,
    }))
}

/// The title blocks of the active drawing.
fn title_blocks(s: &Session) -> Result<Vec<cadcraft_doc::Handle>> {
    let d = s.doc()?;
    let Some(store) = d.space(&s.space()) else { return Ok(Vec::new()) };
    Ok(store.iter().filter(|e| matches!(&e.kind, EntityKind::Insert(i) if is_title_block(i))).map(|e| e.handle).collect())
}

/// Set fields of every title block in the active drawing (fields a block lacks are skipped);
/// returns how many blocks there are.
pub(super) fn set_fields(s: &mut Session, values: &Map<String, Value>) -> Result<usize> {
    let blocks = title_blocks(s)?;
    for h in &blocks {
        s.doc_mut()?.modify_entity(*h, |e| {
            if let EntityKind::Insert(ins) = &mut e.kind {
                for a in ins.attribs.iter_mut() {
                    if let Some(v) = values.get(&a.tag.to_ascii_uppercase()).and_then(Value::as_str) {
                        a.text.value = v.to_string();
                    }
                }
            }
        })?;
    }
    Ok(blocks.len())
}

/// Whether `k` can name a field: letters, digits and `_`, at most 32.
fn valid_field(k: &str) -> bool {
    (1..=32).contains(&k.len()) && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The `set` parameter: field names in capitals, values as text.
pub(super) fn fields_param(p: &Value) -> Result<Map<String, Value>> {
    let mut out = Map::new();
    match p.get("set") {
        None | Some(Value::Null) => {}
        Some(Value::Object(m)) => {
            if m.len() > 256 {
                return Err(bad("etitle", "`set`: too many fields"));
            }
            for (k, v) in m {
                let key = k.trim().to_ascii_uppercase();
                if AUTO.contains(&key.as_str()) {
                    return Err(bad("etitle", format!("`set.{k}`: the sheet numbers are the project's")));
                }
                if !valid_field(&key) || key == MARKER || key == "WD_TB" {
                    return Err(bad("etitle", format!("`set.{k}`: a field name is letters, digits and _ (at most 32)")));
                }
                let text = match v {
                    Value::String(t) => t.chars().take(MAX_VALUE).collect::<String>(),
                    Value::Number(n) => n.to_string(),
                    _ => return Err(bad("etitle", format!("`set.{k}` must be text"))),
                };
                out.insert(key, json!(text));
            }
        }
        Some(_) => return Err(bad("etitle", "`set` must be an object")),
    }
    Ok(out)
}

/// `values` with `{FIELD}` references replaced from `ctx` (unknown fields become empty).
pub(super) fn resolve(values: &Map<String, Value>, ctx: &Map<String, Value>) -> Map<String, Value> {
    values.iter().map(|(k, v)| (k.clone(), json!(resolve_one(v.as_str().unwrap_or(""), ctx)))).collect()
}

fn resolve_one(v: &str, ctx: &Map<String, Value>) -> String {
    let mut out = String::new();
    let mut rest = v;
    for _ in 0..MAX_REFS {
        let Some(open) = rest.find('{') else { break };
        let Some(close) = rest.get(open..).and_then(|r| r.find('}')).map(|c| open + c) else { break };
        let key = rest.get(open + 1..close).unwrap_or("").trim().to_ascii_uppercase();
        if !valid_field(&key) {
            out.push_str(rest.get(..=open).unwrap_or(""));
            rest = rest.get(open + 1..).unwrap_or("");
            continue;
        }
        out.push_str(rest.get(..open).unwrap_or(""));
        // One level: a referenced value is used as it is.
        out.push_str(ctx.get(&key).and_then(Value::as_str).unwrap_or(""));
        rest = rest.get(close + 1..).unwrap_or("");
    }
    out.push_str(rest);
    out.chars().take(MAX_VALUE).collect()
}

/// Write the project's fields into the active drawing's title block for sheet `sheet` of
/// `total`, its file called `file`: the project properties (references resolved against this
/// sheet), the project's name and the sheet numbers. Returns how many title blocks there are.
pub(super) fn write_project_fields(
    s: &mut Session,
    name: &str,
    properties: &std::collections::BTreeMap<String, String>,
    sheet: u64,
    total: usize,
    file: &str,
) -> Result<usize> {
    let stem = std::path::Path::new(file).file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
    let mut fixed = Map::new();
    fixed.insert("SHEET".into(), json!(sheet.to_string()));
    fixed.insert("SHEETS".into(), json!(total.to_string()));
    fixed.insert("FILENAME".into(), json!(stem));
    if !name.is_empty() {
        fixed.insert("PROJECT".into(), json!(name));
    }
    let props: Map<String, Value> = properties.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
    let mut ctx = values(s)?.unwrap_or_default();
    ctx.extend(props.clone());
    ctx.extend(fixed.clone());
    let mut out = resolve(&props, &ctx);
    out.extend(fixed);
    set_fields(s, &out)
}

/// The fields of the title block `template` (a built-in name, or the block of the active
/// drawing's title block for one of your own): tag, label, scope, and whether the block shows it.
pub(super) fn field_list(s: &Session, template: &str) -> Vec<Value> {
    if is_template(template) || template.is_empty() {
        let mut v: Vec<Value> = if template.eq_ignore_ascii_case(TITLE_A3) {
            A3_FIELDS
                .iter()
                .filter(|f| !AUTO.contains(&f.0))
                .map(|(tag, label, _, _, _, _, sc)| json!({ "tag": tag, "label": label, "scope": sc.name(), "shown": true }))
                .collect()
        } else {
            FIELDS
                .iter()
                .filter(|f| !AUTO.contains(&f.0) && f.0 != "PROJECT")
                .map(|(tag, label, _, _, _, _, sc)| json!({ "tag": tag, "label": label, "scope": sc.name(), "shown": true }))
                .collect()
        };
        if template.eq_ignore_ascii_case(TITLE_A3) {
            v.extend(A3_EXTRA.iter().map(|(tag, label)| json!({ "tag": tag, "label": label, "scope": "project", "shown": false })));
        }
        return v;
    }
    // Your own block: its attribute definitions, labelled by their prompts.
    let Ok(d) = s.doc() else { return Vec::new() };
    let Some(store) = d.space(&s.space()) else { return Vec::new() };
    let Some(block) = store.iter().find_map(|e| match &e.kind {
        EntityKind::Insert(i) if is_title_block(i) => d.block(&i.block).cloned(),
        _ => None,
    }) else {
        return Vec::new();
    };
    block
        .entities
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::AttDef(a) if !a.constant => Some(a),
            _ => None,
        })
        .map(|a| a.tag.to_ascii_uppercase())
        .filter(|t| valid_field(t) && !AUTO.contains(&t.as_str()) && t != MARKER && t != "WD_TB")
        .map(|t| {
            let label = block
                .entities
                .iter()
                .find_map(|e| match &e.kind {
                    EntityKind::AttDef(a) if a.tag.eq_ignore_ascii_case(&t) && !a.prompt.is_empty() => Some(a.prompt.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| t.clone());
            json!({ "tag": t, "label": label, "scope": guess_scope(&t).name(), "shown": true })
        })
        .collect()
}

/// Where a field of your own title block is most likely edited, from its name.
pub(super) fn guess_scope(tag: &str) -> Scope {
    let t = tag.to_ascii_uppercase();
    let sheet = ["DESC", "SHEET", "PAGE", "DWGNAME", "DRAWINGNAME", "LOC", "INST", "SUBTITLE"];
    if sheet.iter().any(|w| t.starts_with(w)) { Scope::Sheet } else { Scope::Project }
}

/// Defaults a project gets with title block `template`.
pub(super) fn template_defaults(template: &str) -> &'static [(&'static str, &'static str)] {
    if template.eq_ignore_ascii_case(TITLE_A3) { A3_DEFAULTS } else { &[] }
}

/// Replace the active drawing's title block with `template` (a built-in name or a drawing file),
/// keeping the values of the fields both have. A drawing file is placed at the origin (it
/// usually holds the whole border, so the sheet's own frame goes unless `keep_border`).
pub(super) fn replace(s: &mut Session, template: &str, sheet_width: f64, keep_border: bool) -> Result<()> {
    let old = values(s)?.unwrap_or_default();
    for h in title_blocks(s)? {
        s.doc_mut()?.remove_entity(h);
    }
    if is_template(template) {
        let d = s.doc_mut()?;
        add_template(d, template, sheet_width, &old);
    } else {
        if !keep_border {
            let space = s.space();
            let frame: Vec<cadcraft_doc::Handle> = s
                .doc()?
                .space(&space)
                .map(|st| {
                    st.iter()
                        .filter(|e| e.common.layer.eq_ignore_ascii_case("FRAME") && !matches!(e.kind, EntityKind::Insert(_)))
                        .map(|e| e.handle)
                        .collect()
                })
                .unwrap_or_default();
            for h in frame {
                s.doc_mut()?.remove_entity(h);
            }
        }
        let r = s.execute("insert", &json!({ "file": template, "at": [0, 0] }))?;
        let h = r["handle"]
            .as_str()
            .and_then(cadcraft_doc::Handle::parse_hex)
            .ok_or_else(|| bad("eproject.titleblock", "the title block file could not be inserted"))?;
        s.doc_mut()?.modify_entity(h, |e| {
            e.common.layer = "FRAME".into();
            if let EntityKind::Insert(ins) = &mut e.kind
                && !ins.attribs.iter().any(|a| a.tag.eq_ignore_ascii_case(MARKER))
            {
                let p = ins.insert;
                let mut a = attrib(MARKER, Vec2::new(p.x, p.y), 1.0, "1", HAlign::Left);
                a.invisible = true;
                ins.attribs.push(a);
            }
        })?;
        s.doc_mut()?.ensure_layer("FRAME");
    }
    set_fields(s, &old)?;
    Ok(())
}

fn run_title(s: &mut Session, p: &Value) -> Result<Value> {
    let values = fields_param(p)?;
    if !bool_or(p, "project", false) {
        let n = set_fields(s, &values)?;
        if n == 0 {
            return Err(bad("etitle", "this drawing has no title block (sheets made by New Project or Add New Sheet have one)"));
        }
        return Ok(json!({ "sheets": 1, "values": values }));
    }
    let path = super::project::current().ok_or_else(|| bad("etitle", "no project is open: open one, or leave `project` out"))?;
    let pr = super::project::read_project(&path)?;
    let total = pr.drawings.len();
    let files = pr.drawings.clone();
    super::project::for_each(s, &path, &pr, true, |s, i, sheet| {
        set_fields(s, &values)?;
        let file = files.get(i).map(String::as_str).unwrap_or("");
        write_project_fields(s, &pr.name, &pr.properties, sheet, total, file).map(|n| json!(n))
    })?;
    s.log.push(format!("Title blocks updated on {total} sheet(s)."));
    Ok(json!({ "sheets": total, "values": values }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_block_fields() {
        let mut d = Drawing::new_metric();
        let mut v = Map::new();
        v.insert("TITLE".into(), json!("Pump starter"));
        v.insert("SHEET".into(), json!("2"));
        add_to(&mut d, Vec2::new(420.0, 0.0), &v);
        let EntityKind::Insert(i) = &d.model.iter().last().unwrap().kind else { panic!() };
        let get = |t: &str| i.attribs.iter().find(|a| a.tag == t).map(|a| a.text.value.clone()).unwrap();
        assert_eq!((get("TITLE").as_str(), get("SHEET").as_str(), get("REV").as_str()), ("Pump starter", "2", ""));
        // In the sheet's corner.
        assert!(i.attribs.iter().all(|a| a.text.insert.x > 420.0 - W && a.text.insert.x < 420.0));
        let mut s = Session::new();
        s.open_drawing(d, "titled", None);
        assert_eq!(set_fields(&mut s, &fields_param(&json!({ "set": { "drawnby": "EG", "rev": 3 } })).unwrap()).unwrap(), 1);
        assert!(fields_param(&json!({ "set": { "SHEET": "9" } })).is_err(), "sheet numbers are the project's");
        assert!(fields_param(&json!({ "set": { "bad name!": "x" } })).is_err());
        assert!(fields_param(&json!({ "set": { "CUSTOMER1": "Warburtons" } })).is_ok(), "any field of your own block");
        assert!(s.execute("etitle", &json!({ "set": { "TITLE": "x" } })).is_ok());
        let mut empty = Session::new();
        assert!(empty.execute("etitle", &json!({ "set": { "TITLE": "x" } })).is_err());
    }

    #[test]
    fn references_and_the_a3_strip() {
        let mut ctx = Map::new();
        ctx.insert("JOBNO".into(), json!("7477"));
        ctx.insert("FILENAME".into(), json!("A1 PLC.P4"));
        ctx.insert("REV".into(), json!("B"));
        assert_eq!(resolve_one("{JOBNO} / {FILENAME} - {REV}", &ctx), "7477 / A1 PLC.P4 - B");
        assert_eq!(resolve_one("{nope}x{ a b }{", &ctx), "x{ a b }{");
        let long = resolve_one(&"{REV}".repeat(100), &ctx);
        assert!(long.starts_with("BBB") && long.chars().count() <= MAX_VALUE, "references are capped");

        let mut d = Drawing::new_metric();
        add_template(&mut d, TITLE_A3, 420.0, &Map::new());
        let mut s = Session::new();
        s.open_drawing(d, "a3", None);
        let mut props = std::collections::BTreeMap::new();
        for (k, v) in A3_DEFAULTS {
            props.insert((*k).to_string(), (*v).to_string());
        }
        props.insert("JOBNO".into(), "7477".into());
        props.insert("REV".into(), "B".into());
        props.insert("COMPANY1".into(), "ACME CONTROLS".into());
        assert_eq!(write_project_fields(&mut s, "CP5", &props, 25, 162, "A1 PLC.P4.dxf").unwrap(), 1);
        let v = values(&s).unwrap().unwrap();
        assert_eq!(v["DRAWINGREF"], "7477 / A1 PLC.P4 - B");
        assert_eq!(v["NOTE4"], "ACME CONTROLS");
        assert_eq!((v["SHEET"].as_str(), v["SHEETS"].as_str()), (Some("25"), Some("162")));
        // Changing to the compact block keeps the fields both have.
        s.execute("etitle", &json!({ "set": { "DATE": "02/02/26" } })).unwrap();
        replace(&mut s, TITLE_BLOCK, 420.0, true).unwrap();
        let v = values(&s).unwrap().unwrap();
        assert_eq!(v["DATE"], "02/02/26");
        assert_eq!(v["SHEET"], "25");
        assert!(field_list(&s, TITLE_A3).iter().any(|f| f["tag"] == "JOBNO" && f["shown"] == false));
        assert_eq!(guess_scope("DESCRIPTION2"), Scope::Sheet);
        assert_eq!(guess_scope("CUSTOMER1"), Scope::Project);
    }
}
