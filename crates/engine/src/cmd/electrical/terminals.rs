//! Terminal strips: ETERMNUM numbers the terminals of each strip, ESTRIP reports a strip (what
//! each terminal connects on either side, with wire numbers) and can draw it as a table.
//!
//! A terminal is a symbol of family `TERMINAL` (IEC_TERMINAL, or one made with ESYMBUILD). Its
//! `TAG1` is its strip (`-X1`); its number is in its pin-number attributes (`TERM01`, `TERM02`),
//! so a wire's from–to reads `-X1:3`. A terminal placed without a strip joins `-X1` with the
//! next free number.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadcraft_doc::{EntityKind, Handle};
use cadcraft_geom::Vec2;
use serde_json::{Value, json};

use super::wires::networks;
use super::{Grid, devices, set_tag};
use crate::cmd::{CommandSpec, bad, point_param, str_param};
use crate::{Result, Session};

/// The strip a terminal without one joins.
pub(super) const DEFAULT_STRIP: &str = "-X1";

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("etermnum", "Number Terminals", run_termnum)
            .menu(&["Tools", "Electrical", "Terminals", "Number Terminals"])
            .params("{mode?: new (default: keep numbered terminals) | all, strip? (only this strip)} → {strips: {strip: count}, numbered}"),
        CommandSpec::new("estrip", "Terminal Strip", run_strip)
            .menu(&["Tools", "Electrical", "Terminals", "Terminal Strip Report"])
            .params("{strip? (default: every strip), draw?: {at: [x, y]} (draw the strip as a table)} → {strips: [{strip, terminals: [{number, top, bottom, wireTop, wireBottom, sheet, column}]}]}")
            .noundo(),
    ]
}

/// A terminal in the active drawing.
#[derive(Clone, Debug)]
pub(super) struct Terminal {
    pub handle: Handle,
    pub strip: String,
    pub number: String,
    pub at: Vec2,
}

fn attr(ins: &cadcraft_doc::Insert, tag: &str) -> Option<String> {
    ins.attribs.iter().find(|a| a.tag.eq_ignore_ascii_case(tag)).map(|a| a.text.value.trim().to_string())
}

/// Every terminal of the current space.
pub(super) fn terminals(s: &Session) -> Result<Vec<Terminal>> {
    let d = s.doc()?;
    let mut out = Vec::new();
    for dev in devices(s, None, None)?.into_iter().filter(|d| d.family.as_deref() == Some("TERMINAL")) {
        let Some(EntityKind::Insert(ins)) = d.entity(dev.handle).map(|e| &e.kind) else { continue };
        let strip = if dev.tag.is_empty() { DEFAULT_STRIP.to_string() } else { dev.tag.clone() };
        out.push(Terminal { handle: dev.handle, strip, number: attr(ins, "TERM01").unwrap_or_default(), at: Vec2::new(dev.x, dev.y) });
    }
    Ok(out)
}

/// Set a terminal's strip (when empty) and number.
fn write_terminal(s: &mut Session, t: &Terminal, number: &str) -> Result<()> {
    let d = s.doc()?;
    let Some(EntityKind::Insert(ins)) = d.entity(t.handle).map(|e| &e.kind) else { return Ok(()) };
    let tag_attr = ["TAG1", "TAG"].iter().find(|a| ins.attribs.iter().any(|x| x.tag.eq_ignore_ascii_case(a))).copied();
    let current_tag = tag_attr.and_then(|a| attr(ins, a)).unwrap_or_default();
    let pin_attrs: Vec<String> = ins.attribs.iter().filter(|a| a.tag.to_ascii_uppercase().starts_with("TERM")).map(|a| a.tag.clone()).collect();
    if let Some(a) = tag_attr
        && current_tag.is_empty()
    {
        set_tag(s, t.handle, a, &t.strip)?;
    }
    for a in pin_attrs {
        set_tag(s, t.handle, &a, number)?;
    }
    Ok(())
}

/// Number the terminals of `strip` (every strip when `None`) in reading order: sheet, column, top
/// to bottom. `all` renumbers from 1; else numbered terminals keep their numbers and the others
/// take the next free ones. Returns per-strip counts and how many were numbered.
pub(super) fn number(s: &mut Session, strip: Option<&str>, all: bool) -> Result<(BTreeMap<String, usize>, usize)> {
    let grid = super::sync::active_grid(s).unwrap_or(Grid::from_params("etermnum", &json!({}))?);
    let mut by_strip: BTreeMap<String, Vec<Terminal>> = BTreeMap::new();
    for t in terminals(s)? {
        if strip.is_none_or(|x| x.eq_ignore_ascii_case(&t.strip)) {
            by_strip.entry(t.strip.clone()).or_default().push(t);
        }
    }
    let mut numbered = 0usize;
    let mut counts = BTreeMap::new();
    for (name, mut ts) in by_strip {
        ts.sort_by(|a, b| grid.locate(a.at.x).cmp(&grid.locate(b.at.x)).then(b.at.y.total_cmp(&a.at.y)).then(a.at.x.total_cmp(&b.at.x)));
        let keep = |t: &Terminal| !all && t.number.parse::<u64>().is_ok_and(|n| n > 0);
        let mut used: HashSet<u64> = ts.iter().filter(|t| keep(t)).filter_map(|t| t.number.parse().ok()).collect();
        let mut next = 1u64;
        for t in &ts {
            if keep(t) {
                continue;
            }
            while used.contains(&next) {
                next += 1;
            }
            used.insert(next);
            write_terminal(s, t, &next.to_string())?;
            numbered += 1;
        }
        counts.insert(name, ts.len());
    }
    Ok((counts, numbered))
}

fn run_termnum(s: &mut Session, p: &Value) -> Result<Value> {
    let all = match str_param(p, "mode").map(str::to_ascii_lowercase).as_deref() {
        None | Some("new") => false,
        Some("all") => true,
        Some(m) => return Err(bad("etermnum", format!("`mode` must be \"new\" or \"all\", not \"{m}\""))),
    };
    let (counts, numbered) = number(s, str_param(p, "strip"), all)?;
    s.log.push(format!("{numbered} terminal(s) numbered on {} strip(s).", counts.len()));
    Ok(json!({ "strips": counts, "numbered": numbered }))
}

/// The tag of every symbol in the current space, by handle.
pub(super) fn tags_by_handle(s: &Session) -> Result<HashMap<Handle, String>> {
    Ok(devices(s, None, None)?.into_iter().map(|d| (d.handle, d.tag)).collect())
}

/// The strip report: for each terminal, what its top and bottom pins connect to.
pub(super) fn strip_report(s: &Session, strip: Option<&str>) -> Result<Vec<Value>> {
    let grid = super::sync::active_grid(s).unwrap_or(Grid::from_params("estrip", &json!({}))?);
    let tags = tags_by_handle(s)?;
    let nets = networks(s)?;
    let d = s.doc()?;
    let mut by_strip: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for t in terminals(s)? {
        if strip.is_some_and(|x| !x.eq_ignore_ascii_case(&t.strip)) {
            continue;
        }
        let Some(EntityKind::Insert(ins)) = d.entity(t.handle).map(|e| &e.kind) else { continue };
        let pins = super::sync::pins_of(ins);
        // Each side: the pins on the same network, other than the terminal's own.
        let mut sides: Vec<(Vec<String>, Option<String>)> = Vec::new();
        for p in pins.iter().take(2) {
            let at = Vec2::new(p.position[0], p.position[1]);
            let net = nets.iter().find(|n| n.pins.iter().any(|(h, _, q)| *h == t.handle && q.near(at, 1e-6)));
            let others: Vec<String> = net
                .map(|n| {
                    n.pins
                        .iter()
                        .filter(|(h, _, _)| *h != t.handle)
                        .map(|(h, pin, _)| format!("{}:{pin}", tags.get(h).map(String::as_str).unwrap_or("?")))
                        .collect()
                })
                .unwrap_or_default();
            sides.push((others, net.and_then(|n| n.number.clone())));
        }
        let side = |i: usize| sides.get(i).cloned().unwrap_or_default();
        let place = grid.locate(t.at.x);
        by_strip.entry(t.strip.clone()).or_default().push(json!({
            "number": t.number,
            "handle": t.handle.hex(),
            "top": side(0).0,
            "wireTop": side(0).1,
            "bottom": side(1).0,
            "wireBottom": side(1).1,
            "sheet": place.map(|p| p.0),
            "column": place.map(|p| p.1),
        }));
    }
    Ok(by_strip
        .into_iter()
        .map(|(name, mut ts)| {
            ts.sort_by_key(|t| t["number"].as_str().and_then(|n| n.parse::<u64>().ok()).unwrap_or(u64::MAX));
            json!({ "strip": name, "terminals": ts })
        })
        .collect())
}

fn run_strip(s: &mut Session, p: &Value) -> Result<Value> {
    let strips = strip_report(s, str_param(p, "strip"))?;
    if let Some(at) = p.get("draw").and_then(|d| point_param(d, "at")) {
        let mut y = at.y;
        for st in &strips {
            let rows: Vec<Vec<String>> = st["terminals"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|t| {
                    let list =
                        |k: &str| t[k].as_array().map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")).unwrap_or_default();
                    let wire = |k: &str| t[k].as_str().unwrap_or("").to_string();
                    vec![wire("wireTop"), list("top"), t["number"].as_str().unwrap_or("").to_string(), list("bottom"), wire("wireBottom")]
                })
                .collect();
            let title = format!("Terminal strip {}", st["strip"].as_str().unwrap_or(""));
            let h = super::reports::draw_table(s, Vec2::new(at.x, y), &title, &["Wire", "Target", "Terminal", "Target", "Wire"], &rows)?;
            y -= h + 10.0;
        }
    }
    let n: usize = strips.iter().map(|st| st["terminals"].as_array().map_or(0, Vec::len)).sum();
    s.log.push(format!("{} strip(s), {n} terminal(s).", strips.len()));
    Ok(json!({ "strips": strips }))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn terminals_join_a_strip_numbered_and_reported() {
        let mut s = Session::new();
        let a = s.active;
        if let Some(d) = s.docs.get_mut(a) {
            d.title = "terminals-test".into();
        }
        // A lamp fed through two terminals: -X1 above and below it.
        s.execute("ewire", &json!({ "from": [63, 250], "to": [63, 30] })).unwrap();
        let t1 = s.execute("esymbol", &json!({ "name": "IEC_TERMINAL", "at": [63, 200] })).unwrap();
        s.execute("esymbol", &json!({ "name": "IEC_LAMP", "at": [63, 140] })).unwrap();
        let t2 = s.execute("esymbol", &json!({ "name": "IEC_TERMINAL", "at": [63, 80] })).unwrap();
        // Placed without a strip, they joined -X1 as terminals 1 and 2, untouched by ETAG.
        assert_eq!(t1["tag"], "-X1");
        assert_eq!(t2["tag"], "-X1");
        let d = s.execute("edevices", &json!({})).unwrap();
        assert_eq!(d["duplicates"], json!({}), "terminals of a strip are not duplicates");
        s.execute("ewirenum", &json!({})).unwrap();
        let r = s.execute("estrip", &json!({ "draw": { "at": [300, 250] } })).unwrap();
        let strip = &r["strips"][0];
        assert_eq!(strip["strip"], "-X1");
        let ts = strip["terminals"].as_array().unwrap();
        assert_eq!(ts.len(), 2);
        assert_eq!((ts[0]["number"].as_str(), ts[1]["number"].as_str()), (Some("1"), Some("2")));
        // Terminal 1's bottom side reaches the lamp's X1; terminal 2's top side its X2.
        assert!(ts[0]["bottom"].as_array().unwrap().iter().any(|v| v.as_str().unwrap().ends_with(":X1")), "{ts:?}");
        assert!(ts[1]["top"].as_array().unwrap().iter().any(|v| v.as_str().unwrap().ends_with(":X2")), "{ts:?}");
        assert!(ts[0]["wireBottom"].is_string());
        // Renumbering from the bottom up is not what reading order gives: all → 1, 2 again.
        let r = s.execute("etermnum", &json!({ "mode": "all" })).unwrap();
        assert_eq!(r["strips"]["-X1"], 2);
        assert!(s.execute("etermnum", &json!({ "mode": "sideways" })).is_err());
        // The drawn table is on the REPORT layer.
        assert!(s.doc().unwrap().model.iter().any(|e| e.common.layer == "REPORT"));
    }
}
