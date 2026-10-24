//! PLC I/O cards: EPLC places a digital or analog input or output card with any number of
//! points, each a connection point with its terminal number and PLC address (`I0.0`, `I0.1` …
//! `I1.0`; `IW64`, `IW66`; `%IX0.0`), shown on the card. The I/O list (EREPORT plcio) reads which
//! field device each point is wired to.
//!
//! Inputs have their points on top (field devices above, as on a ladder), outputs below. The card
//! block is `PLC_DI8`, `PLC_DO16`…; its addresses are attributes (`ADDR01`…), so one block serves
//! every card of its size.

use std::sync::Arc;

use cadcraft_doc::{Block, Common, Entity, EntityKind, HAlign, Line, Text, VAlign};
use cadcraft_geom::{Vec2, Vec3};
use serde_json::{Value, json};

use super::symbols::{attdef, standard_attdefs};
use super::wires::networks;
use crate::cmd::{CommandSpec, bad, point_req, str_param};
use crate::{Result, Session};

/// Distance between points.
const PITCH: f64 = 10.0;
const MAX_POINTS: u64 = 64;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("eplc", "PLC I/O Card", run_plc)
            .menu(&["Tools", "Electrical", "Insert PLC I/O Card"])
            .params(
                "{kind: DI | DO | AI | AO, points? (default 8, up to 64), address? (first address, default I0.0 / Q0.0 / IW64 / QW64), \
                 at: [x, y], tag?, description?} → {handle, block, tag, addresses}",
            )
            .interactive(|_| Ok(Box::new(super::interactive::PlcM::new()))),
    ]
}

/// The address `n` places after `first`: bit addresses count 0–7 then carry (`I0.7` → `I1.0`),
/// word addresses step by 2 (`IW64` → `IW66`), plain numbers by 1.
pub(super) fn address(first: &str, n: u64, analog: bool) -> String {
    let first = first.trim();
    let split = first.find(|c: char| c.is_ascii_digit()).unwrap_or(first.len());
    let (prefix, rest) = first.split_at(split);
    if let Some((byte, bit)) = rest.split_once('.')
        && let (Ok(byte), Ok(bit)) = (byte.parse::<u64>(), bit.parse::<u64>())
        && bit < 8
    {
        let total = byte.saturating_mul(8).saturating_add(bit).saturating_add(n);
        return format!("{prefix}{}.{}", total / 8, total % 8);
    }
    match rest.parse::<u64>() {
        Ok(v) => format!("{prefix}{}", v.saturating_add(n.saturating_mul(if analog { 2 } else { 1 }))),
        Err(_) => format!("{first}{}", n + 1),
    }
}

fn kind_param(p: &Value) -> Result<(&'static str, bool, bool)> {
    match str_param(p, "kind").map(str::to_ascii_uppercase).as_deref() {
        Some("DI") => Ok(("DI", true, false)),
        Some("DO") => Ok(("DO", false, false)),
        Some("AI") => Ok(("AI", true, true)),
        Some("AO") => Ok(("AO", false, true)),
        _ => Err(bad("eplc", "`kind` must be DI, DO, AI or AO")),
    }
}

/// The card block: a box, a lead and connection point per point, the attributes.
fn card_block(s: &mut Session, name: &str, kind: &str, input: bool, points: u64) -> Result<()> {
    if s.doc()?.block(name).is_some() {
        return Ok(());
    }
    let w = PITCH * points as f64;
    let (x0, x1, y0, y1) = (-PITCH / 2.0, w - PITCH / 2.0, -15.0, 0.0);
    let line = |a: (f64, f64), b: (f64, f64)| EntityKind::Line(Line { a: Vec3::new(a.0, a.1, 0.0), b: Vec3::new(b.0, b.1, 0.0) });
    let mut kinds = vec![line((x0, y0), (x1, y0)), line((x1, y0), (x1, y1)), line((x1, y1), (x0, y1)), line((x0, y1), (x0, y0))];
    let label = Vec3::new(x0 + 2.0, y0 + 3.0, 0.0);
    kinds.push(EntityKind::Text(Text {
        insert: label,
        align_pt: Some(label),
        height: 2.5,
        value: format!("{kind} {points}"),
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: HAlign::Left,
        valign: VAlign::Bottom,
    }));
    let mut pins: Vec<(Vec2, u8, String)> = Vec::new();
    for i in 0..points {
        let x = PITCH * i as f64;
        let (edge, tip, dir) = if input { (y1, y1 + 5.0, 1u8) } else { (y0, y0 - 5.0, 4u8) };
        kinds.push(line((x, edge), (x, tip)));
        pins.push((Vec2::new(x, tip), dir, (i + 1).to_string()));
        // The address inside the card, under (or above) the point.
        let ay = if input { y1 - 4.0 } else { y0 + 9.0 };
        kinds.push(attdef(&format!("ADDR{:02}", i + 1), Vec2::new(x, ay), 1.8, "", false, HAlign::Center));
    }
    kinds.extend(standard_attdefs(false, "KF", "DEVICE", x1, &pins));
    let d = s.doc_mut()?;
    let mut b = Block::new(name);
    b.description = format!("PLC {kind} card, {points} points");
    for k in kinds {
        b.entities.push(Entity { handle: d.new_handle(), common: Common::default(), kind: k });
    }
    d.blocks.insert(name.to_string(), Arc::new(b));
    Ok(())
}

pub(super) fn run_plc(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "eplc";
    let (kind, input, analog) = kind_param(p)?;
    let points = match p.get("points") {
        None | Some(Value::Null) => 8,
        Some(v) => v.as_u64().filter(|n| (1..=MAX_POINTS).contains(n)).ok_or_else(|| bad(CMD, format!("`points` must be 1 to {MAX_POINTS}")))?,
    };
    let at = point_req(CMD, p, "at")?;
    let default_first = match kind {
        "DI" => "I0.0",
        "DO" => "Q0.0",
        "AI" => "IW64",
        _ => "QW64",
    };
    let first = str_param(p, "address").map(str::trim).filter(|a| !a.is_empty()).unwrap_or(default_first);
    if first.chars().count() > 32 {
        return Err(bad(CMD, "`address` is too long"));
    }
    let name = format!("PLC_{kind}{points}");
    card_block(s, &name, kind, input, points)?;
    let addresses: Vec<String> = (0..points).map(|i| address(first, i, analog)).collect();
    let mut values = serde_json::Map::new();
    for (i, a) in addresses.iter().enumerate() {
        values.insert(format!("ADDR{:02}", i + 1), json!(a));
    }
    if let Some(d) = str_param(p, "description") {
        values.insert("DESC1".into(), json!(d.chars().take(255).collect::<String>()));
    }
    let mut params = json!({ "name": name, "at": [at.x, at.y], "values": values, "breakWires": false });
    if let Some(t) = str_param(p, "tag") {
        params["tag"] = json!(t);
    }
    let r = super::symbols::run_esymbol(s, &params)?;
    Ok(json!({ "handle": r["handle"], "block": name, "tag": r["tag"], "addresses": addresses }))
}

/// One I/O point and what it is wired to.
#[derive(Clone, Debug)]
pub(super) struct IoPoint {
    pub address: String,
    pub card: String,
    pub point: String,
    pub connected: Vec<String>,
    pub wire: Option<String>,
}

/// Every point of every PLC card in the current space, in address order per card.
pub(super) fn io_list(s: &Session) -> Result<Vec<IoPoint>> {
    let tags = super::terminals::tags_by_handle(s)?;
    let nets = networks(s)?;
    let d = s.doc()?;
    let Some(store) = d.space(&s.space()) else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    for e in store.iter() {
        let EntityKind::Insert(ins) = &e.kind else { continue };
        if !ins.block.to_ascii_uppercase().starts_with("PLC_") {
            continue;
        }
        let card = tags.get(&e.handle).cloned().unwrap_or_default();
        let pins = super::sync::pins_of(ins);
        for (i, pin) in pins.iter().enumerate() {
            let addr = ins
                .attribs
                .iter()
                .find(|a| a.tag.eq_ignore_ascii_case(&format!("ADDR{:02}", i + 1)))
                .map(|a| a.text.value.clone())
                .unwrap_or_default();
            // A manufacturer part's supply and common terminals have no address.
            if addr.is_empty() && ins.attribs.iter().any(|a| a.tag.eq_ignore_ascii_case("EPART")) {
                continue;
            }
            let at = Vec2::new(pin.position[0], pin.position[1]);
            let net = nets.iter().find(|n| n.pins.iter().any(|(h, _, q)| *h == e.handle && q.near(at, 1e-6)));
            let connected = net
                .map(|n| {
                    n.pins
                        .iter()
                        .filter(|(h, _, _)| *h != e.handle)
                        .map(|(h, p, _)| format!("{}:{p}", tags.get(h).map(String::as_str).unwrap_or("?")))
                        .collect()
                })
                .unwrap_or_default();
            out.push(IoPoint { address: addr, card: card.clone(), point: pin.number.clone(), connected, wire: net.and_then(|n| n.number.clone()) });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn addresses_count_like_a_plc() {
        assert_eq!(address("I0.0", 0, false), "I0.0");
        assert_eq!(address("I0.0", 7, false), "I0.7");
        assert_eq!(address("I0.0", 8, false), "I1.0");
        assert_eq!(address("%IX2.6", 3, false), "%IX3.1");
        assert_eq!(address("IW64", 2, true), "IW68");
        assert_eq!(address("X10", 5, false), "X15");
        assert_eq!(address("IN", 0, false), "IN1");
    }

    #[test]
    fn a_card_reads_its_field_devices() {
        let mut s = Session::new();
        let a = s.active;
        if let Some(d) = s.docs.get_mut(a) {
            d.title = "plc-test".into();
        }
        let card = s.execute("eplc", &json!({ "kind": "DI", "points": 16, "at": [50, 50] })).unwrap();
        assert_eq!(card["addresses"][15], "I1.7");
        assert!(card["tag"].as_str().unwrap().starts_with("-KF"), "{card}");
        // A push-button wired to the card's third input.
        s.execute("esymbol", &json!({ "name": "IEC_PB_NO", "at": [70, 120] })).unwrap();
        s.execute("ewire", &json!({ "from": [70, 115], "to": [70, 55] })).unwrap();
        s.execute("ewirenum", &json!({})).unwrap();
        let io = s.execute("ereport", &json!({ "kind": "plcio" })).unwrap();
        let rows = io["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 16);
        let third = &rows[2];
        assert_eq!(third[0], "I0.2");
        assert!(third[3].as_str().unwrap().ends_with(":14"), "{third:?}");
        // An output card has its points below.
        let out = s.execute("eplc", &json!({ "kind": "do", "points": 4, "at": [50, 0], "address": "Q4.0" })).unwrap();
        assert_eq!(out["addresses"][3], "Q4.3");
        for bad in [
            json!({ "kind": "XX", "at": [0, 0] }),
            json!({ "kind": "DI", "points": 0, "at": [0, 0] }),
            json!({ "kind": "DI", "points": 65, "at": [0, 0] }),
            json!({ "kind": "DI" }),
        ] {
            assert!(s.execute("eplc", &bad).is_err(), "{bad}");
        }
    }
}
