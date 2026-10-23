//! The sheet title block: a block (`ETITLE`) in the bottom-right corner of every sheet with
//! attributes for the project, title, drawing number, customer, date, drawn and checked by,
//! revision, and "sheet N of M". ETITLE fills them in on this sheet, or on every sheet of the
//! project, numbering the sheets as it goes.

use std::sync::Arc;

use cadcraft_doc::{Attrib, AttribModes, Block, Common, Drawing, Entity, EntityKind, HAlign, Insert, Line, Space, Text, VAlign};
use cadcraft_geom::{Vec2, Vec3};
use serde_json::{Map, Value, json};

use crate::cmd::{CommandSpec, bad, bool_or};
use crate::{Result, Session};

pub(super) const TITLE_BLOCK: &str = "ETITLE";
/// Width and height of the title block (drawing units, mm on an A3 sheet).
const W: f64 = 170.0;
const H: f64 = 30.0;

/// The fields: attribute tag, label shown, cell (x from the block's right edge, y from its bottom,
/// width), text height.
const FIELDS: &[(&str, &str, f64, f64, f64, f64)] = &[
    ("PROJECT", "Project", -170.0, 20.0, 80.0, 3.0),
    ("TITLE", "Title", -170.0, 10.0, 80.0, 3.0),
    ("CUSTOMER", "Customer", -170.0, 0.0, 80.0, 2.5),
    ("DRAWINGNO", "Drawing no.", -90.0, 20.0, 50.0, 3.0),
    ("DATE", "Date", -90.0, 10.0, 25.0, 2.5),
    ("REV", "Rev.", -65.0, 10.0, 25.0, 2.5),
    ("DRAWNBY", "Drawn", -90.0, 0.0, 25.0, 2.5),
    ("CHECKEDBY", "Checked", -65.0, 0.0, 25.0, 2.5),
    ("SHEET", "Sheet", -40.0, 15.0, 20.0, 5.0),
    ("SHEETS", "of", -20.0, 15.0, 20.0, 5.0),
];

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec::new("etitle", "Title Block", run_title).menu(&["Tools", "Electrical", "Title Block..."]).params(
        "{set?: {PROJECT, TITLE, CUSTOMER, DRAWINGNO, DATE, REV, DRAWNBY, CHECKEDBY}, project?: bool (every sheet of the current project, \
             numbering SHEET and SHEETS)} → {sheets, values}",
    )]
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

fn attrib(tag: &str, at: Vec2, height: f64, value: &str) -> Attrib {
    Attrib {
        tag: tag.into(),
        text: text(at, height, value, HAlign::Left),
        invisible: false,
        constant: false,
        prompt: tag.into(),
        props: Default::default(),
        multiline: None,
        modes: AttribModes::default(),
    }
}

/// Where a field's value goes, relative to the block's base (bottom-right corner).
fn value_at(x: f64, y: f64) -> Vec2 {
    Vec2::new(x + 2.0, y + 4.0)
}

/// Put the title block into drawing `d`'s model space with its bottom-right corner at `corner`.
pub(super) fn add_to(d: &mut Drawing, corner: Vec2, values: &Map<String, Value>) {
    if d.block(TITLE_BLOCK).is_none() {
        let line = |a: (f64, f64), b: (f64, f64)| EntityKind::Line(Line { a: Vec3::new(a.0, a.1, 0.0), b: Vec3::new(b.0, b.1, 0.0) });
        let mut kinds = vec![
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
        ];
        for (tag, label, x, y, _, th) in FIELDS {
            kinds.push(EntityKind::Text(text(Vec2::new(x + 1.0, y + 8.0), 1.5, label, HAlign::Left)));
            let mut a = attrib(tag, value_at(*x, *y), *th, "");
            a.prompt = (*label).into();
            kinds.push(EntityKind::AttDef(a));
        }
        let mut b = Block::new(TITLE_BLOCK);
        b.description = "Sheet title block".into();
        for k in kinds {
            b.entities.push(Entity { handle: d.new_handle(), common: Common::default(), kind: k });
        }
        d.blocks.insert(TITLE_BLOCK.into(), Arc::new(b));
    }
    let attribs = FIELDS
        .iter()
        .map(|(tag, _, x, y, _, th)| {
            let v = values.get(*tag).and_then(Value::as_str).unwrap_or("");
            attrib(tag, value_at(*x, *y) + corner, *th, v)
        })
        .collect();
    let ins = Insert {
        block: TITLE_BLOCK.into(),
        insert: Vec3::new(corner.x, corner.y, 0.0),
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs,
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    };
    let _ = d.add(&Space::Model, Common { layer: "FRAME".into(), ..Common::default() }, EntityKind::Insert(ins));
}

/// Set fields of every title block in the active drawing; returns how many blocks changed.
pub(super) fn set_fields(s: &mut Session, values: &Map<String, Value>) -> Result<usize> {
    let d = s.doc()?;
    let Some(store) = d.space(&s.space()) else { return Ok(0) };
    let blocks: Vec<cadcraft_doc::Handle> =
        store.iter().filter(|e| matches!(&e.kind, EntityKind::Insert(i) if i.block.eq_ignore_ascii_case(TITLE_BLOCK))).map(|e| e.handle).collect();
    for h in &blocks {
        for (k, v) in values {
            if let Some(v) = v.as_str() {
                super::set_tag(s, *h, k, v)?;
            }
        }
    }
    Ok(blocks.len())
}

/// The `set` parameter: known fields only, values as text.
fn fields_param(p: &Value) -> Result<Map<String, Value>> {
    let mut out = Map::new();
    match p.get("set") {
        None | Some(Value::Null) => {}
        Some(Value::Object(m)) => {
            for (k, v) in m {
                let key = k.to_ascii_uppercase();
                if !FIELDS.iter().any(|f| f.0 == key) || key == "SHEET" || key == "SHEETS" {
                    return Err(bad(
                        "etitle",
                        format!(
                            "`set.{k}`: fields are {}",
                            FIELDS.iter().map(|f| f.0).filter(|f| !f.starts_with("SHEET")).collect::<Vec<_>>().join(", ")
                        ),
                    ));
                }
                let text = match v {
                    Value::String(t) => t.chars().take(255).collect::<String>(),
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
    super::project::for_each(s, &path, &pr, true, |s, _, sheet| {
        let mut v = values.clone();
        v.insert("SHEET".into(), json!(sheet.to_string()));
        v.insert("SHEETS".into(), json!(total.to_string()));
        if v.get("PROJECT").is_none() && !pr.name.is_empty() {
            v.insert("PROJECT".into(), json!(pr.name));
        }
        set_fields(s, &v).map(|n| json!(n))
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
        assert!(fields_param(&json!({ "set": { "COLOUR": "x" } })).is_err());
        assert!(s.execute("etitle", &json!({ "set": { "TITLE": "x" } })).is_ok());
        let mut empty = Session::new();
        assert!(empty.execute("etitle", &json!({ "set": { "TITLE": "x" } })).is_err());
    }
}
