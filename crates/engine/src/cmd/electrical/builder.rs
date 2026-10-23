//! ESYMBUILD: make a schematic symbol from drawn geometry.
//!
//! Draw the shape, then pick it, its base point and its connection points: ESYMBUILD turns it
//! into a block with the standard symbol attributes (see [`super::symbols`]): the tag (`TAG1`, or
//! `TAG2` for a contact), descriptions, cross-reference, class and family, a pin number and an
//! invisible connection point per pin, facing the side of the shape the pin is on. The symbol is
//! then used like a library one (ESYMBOL), and can be edited like any block (BEDIT, REFEDIT) or
//! written out to a file (WBLOCK) to share.

use std::sync::Arc;

use cadcraft_doc::{Entity, EntityKind, Handle, entity_bounds};
use cadcraft_geom::{Bounds2, Vec2};
use serde_json::{Value, json};

use super::symbols::standard_attdefs;
use crate::cmd::blocks::{Keep, make_block};
use crate::cmd::curves::KW;
use crate::cmd::machines::{SelOutcome, SelectPhase};
use crate::cmd::{CommandSpec, bad, bool_or, point_req, point_value, str_param, targets};
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

const FAMILIES: &[&str] = &["COIL", "NO", "NC", "DEVICE", "TERMINAL"];
/// Most pins on one symbol.
const MAX_PINS: usize = 99;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("esymbuild", "Symbol Builder", run_build)
            .menu(&["Tools", "Electrical", "Symbol Builder"])
            .params(
                "{name, base: [x, y], pins: [{at: [x, y], number?: \"A1\", dir?: \"up\" | \"down\" | \"left\" | \"right\" (default: the side it is on)}], \
                 class: \"K\" (IEC 81346-2 letters), family?: COIL | NO | NC | DEVICE | TERMINAL (default DEVICE), child?: bool (default: NO/NC with class K), \
                 description?, handles? (default: the selection), place?: bool (default true: the geometry becomes the symbol), redefine?: bool} \
                 → {block, handle?, pins}",
            )
            .interactive(|s| Ok(Box::new(BuildM::new(s)))),
    ]
}

/// The direction digit for a `dir` name or digit.
fn dir_param(v: &Value) -> Option<u8> {
    match v {
        Value::Number(n) => n.as_u64().and_then(|d| u8::try_from(d).ok()).filter(|d| matches!(d, 1 | 2 | 4 | 8)),
        Value::String(t) => match t.trim().to_ascii_lowercase().as_str() {
            "up" | "top" | "u" | "1" => Some(1),
            "right" | "r" | "2" => Some(2),
            "down" | "bottom" | "d" | "4" => Some(4),
            "left" | "l" | "8" => Some(8),
            _ => None,
        },
        _ => None,
    }
}

/// The side of `b` nearest `p`: 1 up, 2 right, 4 down, 8 left.
fn side(b: &Bounds2, p: Vec2) -> u8 {
    let d = [(p.y - b.max.y).abs(), (p.x - b.max.x).abs(), (p.y - b.min.y).abs(), (p.x - b.min.x).abs()];
    let dirs = [1u8, 2, 4, 8];
    d.iter().zip(dirs).min_by(|a, b| a.0.total_cmp(b.0)).map(|(_, dir)| dir).unwrap_or(1)
}

fn bounds_of(s: &Session, hs: &[Handle]) -> Result<Bounds2> {
    let d = s.doc()?;
    let mut b = Bounds2::EMPTY;
    for h in hs {
        if let Some(e) = d.entity(*h) {
            b = b.union(&entity_bounds(d, e, 0));
        }
    }
    if b.is_empty() || !b.min.is_finite() || !b.max.is_finite() {
        return Err(EngineError::Other("The selected objects have no extent.".into()));
    }
    Ok(b)
}

fn run_build(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "esymbuild";
    let name = str_param(p, "name").map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad(CMD, "`name` is required"))?.to_string();
    let base = point_req(CMD, p, "base")?;
    let class = str_param(p, "class").map(|c| c.trim().to_ascii_uppercase()).unwrap_or_default();
    if !super::valid_class(&class) {
        return Err(bad(CMD, "`class` must be IEC 81346-2 class letters such as \"K\", \"S\" or \"QA\""));
    }
    let family = str_param(p, "family").map(|f| f.trim().to_ascii_uppercase()).unwrap_or_else(|| "DEVICE".into());
    if !FAMILIES.contains(&family.as_str()) {
        return Err(bad(CMD, format!("`family` must be one of {}", FAMILIES.join(", "))));
    }
    let child = p.get("child").and_then(Value::as_bool).unwrap_or(matches!(family.as_str(), "NO" | "NC") && class.starts_with('K'));
    let hs = targets(s, p)?;
    if hs.is_empty() {
        return Err(bad(CMD, "select the symbol's geometry (`handles`)"));
    }
    let pins_v = p.get("pins").and_then(Value::as_array).ok_or_else(|| bad(CMD, "`pins` (at least one connection point) is required"))?;
    if pins_v.is_empty() || pins_v.len() > MAX_PINS {
        return Err(bad(CMD, format!("give 1 to {MAX_PINS} pins")));
    }
    if s.doc()?.block(&name).is_some() && !bool_or(p, "redefine", false) {
        return Err(EngineError::Other(format!("Block \"{name}\" already exists; pass redefine: true to replace it.")));
    }
    let b = bounds_of(s, &hs)?;
    let mut pins: Vec<(Vec2, u8, String)> = Vec::with_capacity(pins_v.len());
    for (i, pv) in pins_v.iter().enumerate() {
        let at = pv.get("at").and_then(point_value).ok_or_else(|| bad(CMD, format!("`pins[{i}].at` ([x, y]) is required")))?;
        let dir = match pv.get("dir") {
            None | Some(Value::Null) => side(&b, at),
            Some(v) => dir_param(v).ok_or_else(|| bad(CMD, format!("`pins[{i}].dir` must be up, down, left or right")))?,
        };
        let number = pv.get("number").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| (i + 1).to_string());
        if number.chars().count() > 32 {
            return Err(bad(CMD, format!("`pins[{i}].number` is too long")));
        }
        pins.push((at, dir, number));
    }
    let description = str_param(p, "description").unwrap_or("").to_string();

    make_block(s, &name, base, &hs, Keep::Delete, &description)?;
    // The attributes, in the block's (drawing) coordinates like its geometry.
    let defs = standard_attdefs(child, &class, &family, b.max.x, &pins);
    let d = s.doc_mut()?;
    let key = d.blocks.keys().find(|k| k.eq_ignore_ascii_case(&name)).cloned().ok_or_else(|| EngineError::Other("The block was not made.".into()))?;
    let handles: Vec<Handle> = defs.iter().map(|_| d.new_handle()).collect();
    if let Some(blk) = d.blocks.get_mut(&key) {
        let blk = Arc::make_mut(blk);
        for (k, h) in defs.into_iter().zip(handles) {
            blk.entities.push(Entity { handle: h, common: Default::default(), kind: k });
        }
    }
    let mut out = json!({ "block": key, "pins": pins.len() });
    if bool_or(p, "place", true) {
        let r = super::symbols::run_esymbol(s, &json!({ "name": key, "at": [base.x, base.y], "breakWires": false }))?;
        out["handle"] = r["handle"].clone();
        out["tag"] = r["tag"].clone();
    }
    s.log.push(format!("Symbol {key} made with {} pin(s).", pins.len()));
    Ok(out)
}

/// ESYMBUILD at the command line: objects, base point, pins (Enter ends), name, class, family.
struct BuildM {
    sel: SelectPhase,
    hs: Vec<Handle>,
    base: Option<Vec2>,
    pins: Vec<Vec2>,
    pins_done: bool,
    name: Option<String>,
    class: Option<String>,
}

impl BuildM {
    /// Starts with the objects already selected, if any.
    fn new(s: &Session) -> Self {
        let pre = s.selection();
        let hs = if s.settings.pickfirst { pre } else { Vec::new() };
        BuildM { sel: SelectPhase::default(), hs, base: None, pins: Vec::new(), pins_done: false, name: None, class: None }
    }
}

impl Interactive for BuildM {
    fn name(&self) -> &'static str {
        "ESYMBUILD"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        let point = Accept { point: true, number: false, text: false, select: false, enter: true };
        let text = Accept { point: false, number: false, text: true, select: false, enter: true };
        if self.hs.is_empty() {
            self.sel.prompt()
        } else if self.base.is_none() {
            Prompt::new("Base (insertion) point of the symbol", point)
        } else if !self.pins_done {
            Prompt::new(format!("Connection point {} (Enter when done)", self.pins.len() + 1), point)
        } else if self.name.is_none() {
            Prompt::new("Symbol name", text)
        } else if self.class.is_none() {
            Prompt::new("IEC 81346-2 class letters (K relay, S switch, Q contactor, F fuse, M motor, P lamp, X terminal…)", text).default("K")
        } else {
            Prompt::new("Family", KW).kw(&["Coil", "NO", "NC", "Device", "Terminal"]).default("Device")
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.hs.is_empty() {
            return match self.sel.feed(s, &i)? {
                SelOutcome::More => Ok(Step::Continue),
                SelOutcome::Empty => Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    self.hs = hs;
                    Ok(Step::Continue)
                }
            };
        }
        if self.base.is_none() {
            return Ok(match i {
                Input::Point(p) => {
                    self.base = Some(p);
                    Step::Continue
                }
                Input::Enter | Input::Cancel => Step::Done,
                _ => Step::Continue,
            });
        }
        if !self.pins_done {
            match i {
                Input::Point(p) if self.pins.len() < MAX_PINS => self.pins.push(p),
                Input::Enter if !self.pins.is_empty() => self.pins_done = true,
                Input::Enter => return Err(EngineError::Other("Pick at least one connection point.".into())),
                _ => {}
            }
            return Ok(Step::Continue);
        }
        let text = match i {
            Input::Text(t) | Input::Keyword(t) => t.trim().to_string(),
            Input::Enter => String::new(),
            _ => return Ok(Step::Continue),
        };
        if self.name.is_none() {
            if text.is_empty() {
                return Err(EngineError::Other("Type a name for the symbol.".into()));
            }
            self.name = Some(text);
            return Ok(Step::Continue);
        }
        if self.class.is_none() {
            self.class = Some(if text.is_empty() { "K".into() } else { text.to_ascii_uppercase() });
            return Ok(Step::Continue);
        }
        let family = if text.is_empty() { "DEVICE".to_string() } else { text.to_ascii_uppercase() };
        let pins: Vec<Value> = self.pins.iter().map(|p| json!({ "at": [p.x, p.y] })).collect();
        let hs: Vec<String> = self.hs.iter().map(|h| h.hex()).collect();
        let base = self.base.unwrap_or(Vec2::ZERO);
        run_build(s, &json!({ "name": self.name, "base": [base.x, base.y], "pins": pins, "class": self.class, "family": family, "handles": hs }))?;
        Ok(Step::Done)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// A 10 x 6 box at (100, 100) with leads to pins at (105, 110) and (105, 90).
    fn draw(s: &mut Session) -> Vec<String> {
        s.execute("rectang", &json!({"p1": [100, 97], "p2": [110, 103]})).unwrap();
        let r = s.doc().unwrap().model.last().unwrap().handle.hex();
        s.execute("line", &json!({"points": [[105, 103], [105, 110]]})).unwrap();
        let a = s.doc().unwrap().model.last().unwrap().handle.hex();
        s.execute("line", &json!({"points": [[105, 97], [105, 90]]})).unwrap();
        let b = s.doc().unwrap().model.last().unwrap().handle.hex();
        vec![r, a, b]
    }

    #[test]
    fn geometry_becomes_a_smart_symbol() {
        let mut s = Session::new();
        let hs = draw(&mut s);
        let r = s
            .execute(
                "esymbuild",
                &json!({"name": "MY_TIMER", "base": [105, 100], "handles": hs, "class": "K", "family": "COIL",
                        "pins": [{"at": [105, 110], "number": "A1"}, {"at": [105, 90], "number": "A2"}]}),
            )
            .unwrap();
        assert_eq!(r["block"], "MY_TIMER");
        assert_eq!(r["tag"], "-K103", "placed and tagged where it was drawn");
        assert_eq!(s.doc().unwrap().model.len(), 1, "the geometry became the symbol");
        // Pins face the side they are on, so wires snap and leave the right way.
        let w = s.execute("ewiredraw", &json!({"from": [105.4, 110.3], "to": [150, 150]})).unwrap();
        assert_eq!(w["snappedFrom"], "pin");
        assert_eq!(w["from"], json!([105.0, 110.0]));
        // And it is placed again like a library symbol.
        let again = s.execute("esymbol", &json!({"name": "MY_TIMER", "at": [300, 100]})).unwrap();
        assert_eq!(again["pins"].as_array().unwrap().len(), 2);
        assert_eq!(again["tag"], "-K108");
    }

    #[test]
    fn refuses_bad_input() {
        let mut s = Session::new();
        let hs = draw(&mut s);
        let ok = json!({"name": "X1", "base": [0, 0], "handles": hs.clone(), "class": "K", "pins": [{"at": [105, 110]}]});
        for (k, v) in [
            ("class", json!("ZZ")),
            ("family", json!("BANANA")),
            ("pins", json!([])),
            ("pins", json!([{"at": "nowhere"}])),
            ("pins", json!([{"at": [0, 0], "dir": "sideways"}])),
            ("name", json!("")),
            ("handles", json!([])),
        ] {
            let mut p = ok.clone();
            p[k] = v;
            assert!(s.execute("esymbuild", &p).is_err(), "{k}");
        }
        s.execute("esymbuild", &ok).unwrap();
        let hs = draw(&mut s);
        let mut again = ok.clone();
        again["handles"] = json!(hs);
        assert!(s.execute("esymbuild", &again).is_err(), "no silent redefinition");
        again["redefine"] = json!(true);
        s.execute("esymbuild", &again).unwrap();
    }

    #[test]
    fn sides_and_directions() {
        let b = Bounds2 { min: Vec2::new(0.0, 0.0), max: Vec2::new(10.0, 10.0) };
        assert_eq!(side(&b, Vec2::new(5.0, 11.0)), 1);
        assert_eq!(side(&b, Vec2::new(10.0, 5.0)), 2);
        assert_eq!(side(&b, Vec2::new(5.0, -1.0)), 4);
        assert_eq!(side(&b, Vec2::new(-1.0, 5.0)), 8);
        assert_eq!(dir_param(&json!("Down")), Some(4));
        assert_eq!(dir_param(&json!(8)), Some(8));
        assert_eq!(dir_param(&json!(3)), None);
    }

    #[test]
    fn command_line_form() {
        let mut s = Session::new();
        let hs: Vec<Handle> = draw(&mut s).iter().map(|h| Handle::parse_hex(h).unwrap()).collect();
        s.set_selection(hs);
        s.start("esymbuild").unwrap();
        s.input(Input::Point(Vec2::new(105.0, 100.0))).unwrap();
        s.input(Input::Point(Vec2::new(105.0, 110.0))).unwrap();
        s.input(Input::Point(Vec2::new(105.0, 90.0))).unwrap();
        s.input(Input::Enter).unwrap();
        s.input(Input::Text("LIMIT_SW".into())).unwrap();
        s.input(Input::Text("B".into())).unwrap();
        s.input(Input::Keyword("NO".into())).unwrap();
        assert!(s.running.is_none());
        assert!(s.doc().unwrap().block("LIMIT_SW").is_some());
    }
}
