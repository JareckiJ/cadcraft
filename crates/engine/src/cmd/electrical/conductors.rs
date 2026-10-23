//! Wire types and cables.
//!
//! **Wire types** (EWIRETYPE), AutoCAD Electrical style: a wire's colour and cross-section are its
//! layer, `WIRE_<colour>_<mm²>` (`WIRE_BU_1.5`: blue, 1.5 mm²), coloured like the conductor.
//! EWIRETYPE makes a wire type current (EWIRE draws on it) or changes selected wires to it. The
//! project database and the wire list read colour and size from the layer.
//!
//! **Cables** (ECABLE), EPLAN style: a cable is a definition line drawn across the wires it
//! carries, with its tag (`-W1`) and type (`ÖLFLEX 5G1.5`). The wires it crosses are its cores, in
//! order along the line, given the type's core colours (IEC 60445: brown, black, grey, blue,
//! green-yellow for a 5G cable; numbered cores for a `x` type). The cable is a block, so MOVE and
//! COPY take it along; its cores are worked out wherever it is (the cable list, ECABLES).

use std::sync::Arc;

use cadcraft_doc::{Block, Common, Entity, EntityKind, HAlign, Handle, Line};
use cadcraft_geom::{Vec2, Vec3};
use serde_json::{Value, json};

use super::symbols::{attdef, standard_attdefs};
use super::wires::{networks, wire_layer_matches, wire_segments};
use crate::cmd::{CommandSpec, bad, f64_or, point_req, str_param, targets};
use crate::{Result, Session};

/// Conductor colour codes (IEC 60757) and the layer colour (ACI) drawn for each.
pub(super) const COLOURS: &[(&str, u8, &str)] = &[
    ("BK", 250, "black"),
    ("BN", 34, "brown"),
    ("RD", 1, "red"),
    ("OG", 30, "orange"),
    ("YE", 2, "yellow"),
    ("GN", 3, "green"),
    ("BU", 5, "blue"),
    ("VT", 6, "violet"),
    ("GY", 8, "grey"),
    ("WH", 7, "white"),
    ("PK", 221, "pink"),
    ("GNYE", 92, "green-yellow"),
    ("DBU", 170, "dark blue"),
];

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("ewiretype", "Wire Type", run_wiretype)
            .menu(&["Tools", "Electrical", "Wire Type..."])
            .params("{color: BK | BN | RD | OG | YE | GN | BU | VT | GY | WH | PK | GNYE | DBU, size?: mm² (default 1), handles? (wires to change; default: make it current)} → {layer, changed}"),
        CommandSpec::new("ecable", "Insert Cable", run_cable)
            .menu(&["Tools", "Electrical", "Insert Cable"])
            .params("{from: [x, y], to: [x, y] (the line across the cable's wires), tag?, type? (e.g. \"5G1.5\", \"4x0.75\"), description?} → {handle, tag, cores}"),
        CommandSpec::new("ecables", "Cable List", run_cables)
            .menu(&["Tools", "Electrical", "Reports", "Cable List"])
            .params("{} → {cables: [{tag, type, cores: [{core, colour, wire, from, to}]}]}")
            .noundo(),
    ]
}

/// The layer for colour `code` and cross-section `size`.
pub(super) fn wire_layer(code: &str, size: f64) -> String {
    let size = if size.fract() == 0.0 { format!("{size:.0}") } else { format!("{size}") };
    format!("WIRE_{code}_{size}")
}

/// Colour code and cross-section from a wire layer name (`WIRE_BU_1.5`).
pub(super) fn parse_wire_layer(layer: &str) -> (Option<String>, Option<f64>) {
    let mut parts = layer.split('_').skip(1);
    let colour = parts.next().map(str::to_ascii_uppercase).filter(|c| COLOURS.iter().any(|x| x.0 == c));
    let size = parts.next().and_then(|v| v.replace(',', ".").parse::<f64>().ok()).filter(|v| v.is_finite() && *v > 0.0);
    (colour, size)
}

fn run_wiretype(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "ewiretype";
    let code = str_param(p, "color").or_else(|| str_param(p, "colour")).map(|c| c.trim().to_ascii_uppercase()).unwrap_or_default();
    let Some(&(_, aci, _)) = COLOURS.iter().find(|c| c.0 == code) else {
        return Err(bad(CMD, format!("`color` must be one of {}", COLOURS.iter().map(|c| c.0).collect::<Vec<_>>().join(", "))));
    };
    let size = f64_or(p, "size", 1.0);
    if !(0.01..=1000.0).contains(&size) {
        return Err(bad(CMD, "`size` must be a cross-section in mm², 0.01 to 1000"));
    }
    let layer = wire_layer(&code, size);
    let d = s.doc_mut()?;
    if d.ensure_layer(&layer)
        && let Some(l) = d.layer_mut(&layer)
    {
        l.color = cadcraft_color::Color::Index(aci);
    }
    let changed = if p.get("handles").is_some() || p.get("handle").is_some() {
        let hs = targets(s, p)?;
        let mut n = 0usize;
        for h in hs {
            let is_wire = s
                .doc()?
                .entity(h)
                .is_some_and(|e| matches!(e.kind, EntityKind::Line(_) | EntityKind::LwPolyline(_)) && wire_layer_matches("WIRE*", &e.common.layer));
            if is_wire {
                let l = layer.clone();
                s.doc_mut()?.modify_entity(h, |e| e.common.layer = l)?;
                n += 1;
            }
        }
        n
    } else {
        s.doc_mut()?.header.set_str("CLAYER", &layer);
        0
    };
    s.log.push(if changed > 0 { format!("{changed} wire(s) changed to {layer}.") } else { format!("Wires are now drawn as {layer}.") });
    Ok(json!({ "layer": layer, "changed": changed }))
}

/// The core colours of a cable type: `5G1.5` → BN BK GY BU GNYE; `3G` → BN BU GNYE; a `x` type
/// or anything else → numbered cores.
pub(super) fn core_names(cable_type: &str, count: usize) -> Vec<String> {
    let t = cable_type.to_ascii_uppercase();
    let n: Option<usize> = t.chars().skip_while(|c| !c.is_ascii_digit()).take_while(char::is_ascii_digit).collect::<String>().parse().ok();
    let with_pe = t.contains('G') && n.is_some();
    let colours: &[&str] = match (with_pe, n) {
        (true, Some(2)) => &["BN", "BU"],
        (true, Some(3)) => &["BN", "BU", "GNYE"],
        (true, Some(4)) => &["BN", "BK", "GY", "GNYE"],
        (true, Some(5)) => &["BN", "BK", "GY", "BU", "GNYE"],
        _ => &[],
    };
    (0..count).map(|i| colours.get(i).map(|c| c.to_string()).unwrap_or_else(|| (i + 1).to_string())).collect()
}

fn run_cable(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "ecable";
    let from = point_req(CMD, p, "from")?;
    let to = point_req(CMD, p, "to")?;
    let len = from.dist(to);
    if !(len > 1.0 && len < 1e6) {
        return Err(bad(CMD, "the cable line must be longer than 1"));
    }
    let ty = str_param(p, "type").unwrap_or("").chars().take(64).collect::<String>();
    // One block per cable: its line in block coordinates along +x, its attributes.
    let d = s.doc()?;
    let name = (1..10_000).map(|i| format!("ECABLE_{i}")).find(|n| d.block(n).is_none()).ok_or_else(|| bad(CMD, "too many cables"))?;
    let tick = 2.0;
    let mut kinds = vec![
        EntityKind::Line(Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(len, 0.0, 0.0) }),
        EntityKind::Line(Line { a: Vec3::new(0.0, -tick, 0.0), b: Vec3::new(0.0, tick, 0.0) }),
        EntityKind::Line(Line { a: Vec3::new(len, -tick, 0.0), b: Vec3::new(len, tick, 0.0) }),
    ];
    kinds.extend(standard_attdefs(false, "W", "CABLE", len, &[]));
    kinds.push(attdef("CABLETYPE", Vec2::new(len + 1.5, -1.5), 1.8, &ty, false, HAlign::Left));
    let dm = s.doc_mut()?;
    if dm.ensure_layer("CABLE")
        && let Some(l) = dm.layer_mut("CABLE")
    {
        l.color = cadcraft_color::Color::Index(6);
    }
    let mut b = Block::new(&name);
    b.description = format!("Cable {ty}");
    for k in kinds {
        b.entities.push(Entity { handle: dm.new_handle(), common: Common { layer: "CABLE".into(), ..Common::default() }, kind: k });
    }
    dm.blocks.insert(name.clone(), Arc::new(b));
    let angle = (to.y - from.y).atan2(to.x - from.x).to_degrees();
    let mut values = serde_json::Map::new();
    values.insert("CABLETYPE".into(), json!(ty));
    if let Some(desc) = str_param(p, "description") {
        values.insert("DESC1".into(), json!(desc.chars().take(255).collect::<String>()));
    }
    let mut params = json!({ "name": name, "at": [from.x, from.y], "rotation": angle, "values": values, "breakWires": false });
    if let Some(t) = str_param(p, "tag") {
        params["tag"] = json!(t);
    }
    let r = super::symbols::run_esymbol(s, &params)?;
    let h = r["handle"].as_str().and_then(Handle::parse_hex);
    let cores = h.map(|h| cable_cores(s, h)).transpose()?.unwrap_or_default();
    Ok(json!({ "handle": r["handle"], "tag": r["tag"], "cores": cores }))
}

/// Where segments `a`–`b` and `c`–`d` cross: the parameter along the first, if they do.
fn crossing(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> Option<f64> {
    let r = b - a;
    let q = d - c;
    let den = r.x * q.y - r.y * q.x;
    if den.abs() < 1e-12 {
        return None;
    }
    let ac = c - a;
    let t = (ac.x * q.y - ac.y * q.x) / den;
    let u = (ac.x * r.y - ac.y * r.x) / den;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some(t)
}

/// The cores of cable `h`: the wire networks its line crosses, in order along it.
pub(super) fn cable_cores(s: &Session, h: Handle) -> Result<Vec<Value>> {
    let d = s.doc()?;
    let Some(EntityKind::Insert(ins)) = d.entity(h).map(|e| &e.kind) else { return Ok(Vec::new()) };
    let Some(blk) = d.block(&ins.block) else { return Ok(Vec::new()) };
    let line = blk.entities.iter().find_map(|e| match &e.kind {
        EntityKind::Line(l) if (l.b.x - l.a.x).abs() > 1.0 => Some((l.a.xy(), l.b.xy())),
        _ => None,
    });
    let Some((a, b)) = line else { return Ok(Vec::new()) };
    let m = ins.transform(blk.base.xy());
    let (a, b) = (m.apply(a), m.apply(b));
    let ty = ins.attribs.iter().find(|x| x.tag.eq_ignore_ascii_case("CABLETYPE")).map(|x| x.text.value.clone()).unwrap_or_default();
    let (handles, segs) = wire_segments(s, "WIRE*")?;
    // Each crossed wire, once, by where along the cable it is crossed.
    let mut crossed: Vec<(f64, Handle)> = Vec::new();
    for sg in &segs {
        if let Some(t) = crossing(a, b, sg.a, sg.b)
            && let Some(wh) = handles.get(sg.wire)
            && !crossed.iter().any(|(_, x)| x == wh)
        {
            crossed.push((t, *wh));
        }
    }
    crossed.sort_by(|x, y| x.0.total_cmp(&y.0));
    let nets = networks(s)?;
    let tags = super::terminals::tags_by_handle(s)?;
    let names = core_names(&ty, crossed.len());
    let wire_layers: std::collections::HashMap<Handle, String> =
        d.space(&s.space()).map(|st| st.iter().map(|e| (e.handle, e.common.layer.clone())).collect()).unwrap_or_default();
    let (all_handles, all_segs) = (handles, segs);
    let mut out = Vec::new();
    for (i, (_, wh)) in crossed.iter().enumerate() {
        // The network this wire belongs to: the one with a pin on a wire segment of the same
        // potential (found by the wire's own end points).
        let ends: Vec<Vec2> = all_segs.iter().filter(|sg| all_handles.get(sg.wire) == Some(wh)).flat_map(|sg| [sg.a, sg.b]).collect();
        let net = nets
            .iter()
            .find(|n| n.wire_handles.contains(wh))
            .or_else(|| nets.iter().find(|n| n.pins.iter().any(|(_, _, q)| ends.iter().any(|e| e.near(*q, 0.01)))));
        let pins: Vec<String> = net
            .map(|n| n.pins.iter().map(|(ph, pin, _)| format!("{}:{pin}", tags.get(ph).map(String::as_str).unwrap_or("?"))).collect())
            .unwrap_or_default();
        let (colour, size) = parse_wire_layer(wire_layers.get(wh).map(String::as_str).unwrap_or(""));
        out.push(json!({
            "core": names.get(i).cloned().unwrap_or_else(|| (i + 1).to_string()),
            "wire": net.and_then(|n| n.number.clone()),
            "from": pins.first(),
            "to": pins.get(1..).map(|r| r.join(", ")),
            "wireColour": colour,
            "size": size,
        }));
    }
    Ok(out)
}

/// Every cable in the current space with its cores.
pub(super) fn cables(s: &Session) -> Result<Vec<Value>> {
    let d = s.doc()?;
    let Some(store) = d.space(&s.space()) else { return Ok(Vec::new()) };
    let mut list: Vec<(String, String, Handle)> = Vec::new();
    for e in store.iter() {
        if let EntityKind::Insert(ins) = &e.kind
            && ins.attribs.iter().any(|a| a.tag.eq_ignore_ascii_case("EFAMILY") && a.text.value.eq_ignore_ascii_case("CABLE"))
        {
            let get = |t: &str| ins.attribs.iter().find(|a| a.tag.eq_ignore_ascii_case(t)).map(|a| a.text.value.clone()).unwrap_or_default();
            list.push((get("TAG1"), get("CABLETYPE"), e.handle));
        }
    }
    list.sort();
    list.into_iter().map(|(tag, ty, h)| Ok(json!({ "tag": tag, "type": ty, "handle": h.hex(), "cores": cable_cores(s, h)? }))).collect()
}

fn run_cables(s: &mut Session, _p: &Value) -> Result<Value> {
    let list = cables(s)?;
    s.log.push(format!("{} cable(s).", list.len()));
    Ok(json!({ "cables": list }))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn wire_layers_and_core_colours() {
        assert_eq!(wire_layer("BU", 1.5), "WIRE_BU_1.5");
        assert_eq!(wire_layer("GNYE", 2.0), "WIRE_GNYE_2");
        assert_eq!(parse_wire_layer("WIRE_BU_1.5"), (Some("BU".into()), Some(1.5)));
        assert_eq!(parse_wire_layer("WIRE"), (None, None));
        assert_eq!(parse_wire_layer("WIRE_XX_1"), (None, Some(1.0)));
        assert_eq!(core_names("5G1.5", 5), vec!["BN", "BK", "GY", "BU", "GNYE"]);
        assert_eq!(core_names("ÖLFLEX 3G0.75", 2), vec!["BN", "BU"]);
        assert_eq!(core_names("7x0.5", 3), vec!["1", "2", "3"]);
        assert_eq!(crossing(Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(5.0, -1.0), Vec2::new(5.0, 1.0)), Some(0.5));
        assert_eq!(crossing(Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(0.0, 1.0), Vec2::new(10.0, 1.0)), None);
    }

    #[test]
    fn wire_types_and_a_cable_across_three_wires() {
        let mut s = Session::new();
        let a = s.active;
        if let Some(d) = s.docs.get_mut(a) {
            d.title = "cables-test".into();
        }
        let r = s.execute("ewiretype", &json!({ "color": "bu", "size": 1.5 })).unwrap();
        assert_eq!(r["layer"], "WIRE_BU_1.5");
        // Three motor wires from a breaker to a motor, on the current wire type.
        s.execute("esymbol", &json!({ "name": "IEC_MCB_3P", "at": [60, 200] })).unwrap();
        s.execute("esymbol", &json!({ "name": "IEC_MOTOR_3PH", "at": [60, 80] })).unwrap();
        for x in [55, 60, 65] {
            let w = s.execute("ewire", &json!({ "from": [x, 195], "to": [x, 85] })).unwrap();
            let h = Handle::parse_hex(w["handle"].as_str().unwrap()).unwrap();
            assert_eq!(s.doc().unwrap().entity(h).unwrap().common.layer, "WIRE_BU_1.5");
        }
        let c = s.execute("ecable", &json!({ "from": [45, 140], "to": [75, 140], "type": "4G1.5", "tag": "-W1" })).unwrap();
        assert_eq!(c["tag"], "-W1");
        let cores = c["cores"].as_array().unwrap();
        assert_eq!(cores.len(), 3, "{c}");
        assert_eq!(cores.iter().map(|x| x["core"].as_str().unwrap()).collect::<Vec<_>>(), vec!["BN", "BK", "GY"]);
        assert_eq!(cores[0]["wireColour"], "BU");
        assert!(cores[0]["from"].as_str().unwrap().ends_with(":2") || cores[0]["from"].as_str().unwrap().ends_with(":U1"), "{cores:?}");
        // Moving the cable off the wires leaves it with no cores; the list follows.
        let h = c["handle"].as_str().unwrap().to_string();
        s.execute("move", &json!({ "handles": [h], "delta": [100, 0] })).unwrap();
        let l = s.execute("ecables", &json!({})).unwrap();
        assert_eq!(l["cables"][0]["cores"], json!([]));
        // Changing a wire's type.
        let w = s.doc().unwrap().model.iter().find(|e| e.common.layer == "WIRE_BU_1.5").unwrap().handle.hex();
        let r = s.execute("ewiretype", &json!({ "color": "GNYE", "size": 2.5, "handles": [w] })).unwrap();
        assert_eq!(r["changed"], 1);
        for bad in [json!({ "color": "PURPLE" }), json!({ "color": "BU", "size": 0 })] {
            assert!(s.execute("ewiretype", &bad).is_err());
        }
        assert!(s.execute("ecable", &json!({ "from": [0, 0], "to": [0, 0.5] })).is_err());
    }
}
