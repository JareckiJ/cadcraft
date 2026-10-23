//! EGENERATE: a schematic from a bill of materials and a netlist.
//!
//! - The **BOM** (CSV with a header row) lists the components: `tag`, `symbol` (a library symbol
//!   such as `IEC_COIL`, or a block in the drawing) or `type` (Relay, Contactor, PushButton…,
//!   mapped to a symbol by the template), and optionally `manufacturer`, `catalog`,
//!   `description`, `location`, and `pins` (the pin numbers in order, separated by spaces or
//!   semicolons, e.g. `23 24` for a second contact). A coil and its contacts share a tag.
//! - The **netlist** (CSV) is either `net,device,pin` rows (one per connection) or
//!   `wire,from,to` rows with `from`/`to` written `tag:pin`.
//! - The **template** (`template.json` in a template folder; every field optional) sets the sheet
//!   layout and the rails: `{"name", "sheetWidth": 420, "columns": 10, "top": 240,
//!   "bottom": 40, "railsTop": ["+24V", "L", "L+"], "railsBottom": ["0V", "N", "M"],
//!   "symbols": {"Relay": "IEC_COIL", …}}`.
//!
//! The layout is a ladder: two rails, and each series path from the top rail to the bottom rail
//! in a column of its own (a *rung*); a path that branches off another starts from it with a
//! T junction. Components with three or more pins (breakers, contactor main contacts, motors)
//! are laid out first, in power columns of three wires each. Columns fill sheets left to right.
//!
//! The plan is a list of engine commands ([`plan`]: EWIRE, ESYMBOL, TEXT…, each with its sheet):
//! EGENERATE runs it in the current drawing with the sheets side by side, `cadcraft-cli
//! generate` builds a project with a drawing per sheet, and the plan can be streamed to a running
//! CADCraft over its control channel. Wire numbers and cross-references follow from the
//! electrical commands.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde_json::{Value, json};

use super::symbols;
use crate::cmd::{CommandSpec, bad, bool_or, str_param};
use crate::{EngineError, Result, Session};

/// Largest BOM or netlist accepted (characters), and most rows.
const MAX_INPUT: usize = 4 << 20;
const MAX_ROWS: usize = 20_000;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec::new("egenerate", "Generate Schematic", run_generate).menu(&["Tools", "Electrical", "Generate Schematic from BOM"]).params(
        "{bom: CSV text, netlist: CSV text, template?: {sheetWidth, columns, top, bottom, railsTop, railsBottom, symbols}, \
                 planOnly?: bool (return the commands without running them)} → {sheets, components, steps, warnings}",
    )]
}

/// The layout settings.
#[derive(Clone, Debug)]
pub struct Template {
    pub name: String,
    pub sheet_width: f64,
    pub columns: usize,
    pub top: f64,
    pub bottom: f64,
    pub rails_top: Vec<String>,
    pub rails_bottom: Vec<String>,
    pub symbols: BTreeMap<String, String>,
}

impl Default for Template {
    fn default() -> Self {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect();
        Template {
            name: "Generated".into(),
            sheet_width: 420.0,
            columns: 10,
            top: 240.0,
            bottom: 40.0,
            rails_top: s(&["+24V", "24V", "+24VDC", "L+", "L", "+"]),
            rails_bottom: s(&["0V", "0VDC", "M", "L-", "N", "-"]),
            symbols: [
                ("relay", "IEC_COIL"),
                ("coil", "IEC_COIL"),
                ("contactor", "IEC_CONTACTOR_COIL"),
                ("no", "IEC_NO"),
                ("contact", "IEC_NO"),
                ("nc", "IEC_NC"),
                ("pushbutton", "IEC_PB_NO"),
                ("start", "IEC_PB_NO"),
                ("stop", "IEC_PB_NC"),
                ("estop", "IEC_ESTOP"),
                ("selector", "IEC_SELECTOR"),
                ("switch", "IEC_SELECTOR"),
                ("lamp", "IEC_LAMP"),
                ("fuse", "IEC_FUSE"),
                ("breaker", "IEC_MCB_1P"),
                ("mcb", "IEC_MCB_1P"),
                ("breaker3p", "IEC_MCB_3P"),
                ("mcb3p", "IEC_MCB_3P"),
                ("maincontacts", "IEC_MAIN_CONTACTS_3P"),
                ("overload", "IEC_OVERLOAD_3P"),
                ("overloadcontact", "IEC_OVERLOAD_NC"),
                ("motor", "IEC_MOTOR_3PH"),
                ("terminal", "IEC_TERMINAL"),
                ("psu", "IEC_PSU"),
                ("powersupply", "IEC_PSU"),
                ("pe", "IEC_PE"),
                ("timer", "IEC_TIMER_ON"),
                ("timeron", "IEC_TIMER_ON"),
                ("timeroff", "IEC_TIMER_OFF"),
                ("timercontact", "IEC_NO_TON"),
                ("limitswitch", "IEC_LIMIT_NO"),
                ("limit", "IEC_LIMIT_NO"),
                ("proximity", "IEC_PROX_IND"),
                ("sensor", "IEC_PROX_IND"),
                ("inductive", "IEC_PROX_IND"),
                ("capacitive", "IEC_PROX_CAP"),
                ("photo", "IEC_PHOTO"),
                ("photoelectric", "IEC_PHOTO"),
                ("pressureswitch", "IEC_PRESSURE"),
                ("thermostat", "IEC_THERMOSTAT"),
                ("levelswitch", "IEC_LEVEL"),
                ("float", "IEC_LEVEL"),
                ("transformer", "IEC_TRANSFORMER"),
                ("vfd", "IEC_VFD"),
                ("drive", "IEC_VFD"),
                ("inverter", "IEC_VFD"),
                ("motor1ph", "IEC_MOTOR_1PH"),
                ("socket", "IEC_SOCKET"),
                ("buzzer", "IEC_BUZZER"),
                ("horn", "IEC_BUZZER"),
                ("heater", "IEC_HEATER"),
                ("resistor", "IEC_RESISTOR"),
                ("diode", "IEC_DIODE"),
            ]
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        }
    }
}

impl Template {
    /// From the template JSON; missing or invalid fields keep their defaults.
    pub fn from_json(v: &Value) -> Template {
        let mut t = Template::default();
        let num = |k: &str, lo: f64, hi: f64| v.get(k).and_then(Value::as_f64).filter(|x| x.is_finite() && *x >= lo && *x <= hi);
        if let Some(n) = v.get("name").and_then(Value::as_str) {
            t.name = n.chars().take(128).collect();
        }
        if let Some(w) = num("sheetWidth", 50.0, 10_000.0) {
            t.sheet_width = w;
        }
        if let Some(c) = num("columns", 1.0, 100.0) {
            t.columns = c as usize;
        }
        if let (Some(top), Some(bottom)) = (num("top", -1e6, 1e6), num("bottom", -1e6, 1e6))
            && top - bottom >= 30.0
        {
            t.top = top;
            t.bottom = bottom;
        }
        let list =
            |k: &str| -> Option<Vec<String>> { Some(v.get(k)?.as_array()?.iter().filter_map(Value::as_str).take(64).map(str::to_string).collect()) };
        if let Some(l) = list("railsTop") {
            t.rails_top = l;
        }
        if let Some(l) = list("railsBottom") {
            t.rails_bottom = l;
        }
        if let Some(m) = v.get("symbols").and_then(Value::as_object) {
            for (k, s) in m.iter().take(256) {
                if let Some(s) = s.as_str() {
                    t.symbols.insert(norm(k), s.to_string());
                }
            }
        }
        t
    }
    fn column_width(&self) -> f64 {
        self.sheet_width / self.columns.max(1) as f64
    }
}

fn norm(s: &str) -> String {
    s.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_lowercase()).collect()
}

// ---------------- CSV ----------------

/// Rows of a CSV text (RFC 4180 quoting; `,` or `;` separated, whichever the header uses).
pub fn parse_csv(text: &str) -> Result<Vec<Vec<String>>> {
    if text.len() > MAX_INPUT {
        return Err(EngineError::Other("The file is too large (over 4 MB).".into()));
    }
    let first = text.lines().next().unwrap_or("");
    let sep = if first.matches(';').count() > first.matches(',').count() { ';' } else { ',' };
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match (quoted, c) {
            (true, '"') if chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            (true, '"') => quoted = false,
            (true, c) => field.push(c),
            (false, '"') if field.trim().is_empty() => {
                field.clear();
                quoted = true;
            }
            (false, c) if c == sep => row.push(std::mem::take(&mut field).trim().to_string()),
            (false, '\r') => {}
            (false, '\n') => {
                row.push(std::mem::take(&mut field).trim().to_string());
                if row.iter().any(|f| !f.is_empty()) {
                    rows.push(std::mem::take(&mut row));
                } else {
                    row.clear();
                }
                if rows.len() > MAX_ROWS {
                    return Err(EngineError::Other(format!("More than {MAX_ROWS} rows.")));
                }
            }
            (false, c) => field.push(c),
        }
    }
    row.push(field.trim().to_string());
    if row.iter().any(|f| !f.is_empty()) {
        rows.push(row);
    }
    Ok(rows)
}

/// Rows as maps from (normalised) header names.
fn records(text: &str) -> Result<Vec<HashMap<String, String>>> {
    let mut rows = parse_csv(text)?.into_iter();
    let Some(header) = rows.next() else { return Ok(Vec::new()) };
    let header: Vec<String> = header.iter().map(|h| norm(h)).collect();
    Ok(rows.map(|r| header.iter().cloned().zip(r).filter(|(_, v)| !v.is_empty()).collect()).collect())
}

fn field<'a>(r: &'a HashMap<String, String>, names: &[&str]) -> Option<&'a str> {
    names.iter().find_map(|n| r.get(*n)).map(String::as_str)
}

// ---------------- the model ----------------

/// A component to place: one BOM row, one symbol.
#[derive(Clone, Debug)]
pub struct Component {
    pub tag: String,
    pub symbol: String,
    /// Pin numbers, in the symbol's pin order.
    pub pins: Vec<String>,
    /// Pin positions relative to the insertion point and direction digits, in pin order.
    geometry: Vec<(f64, f64, u8)>,
    pub values: serde_json::Map<String, Value>,
    pub child: bool,
}

impl Component {
    fn is_power(&self) -> bool {
        self.pins.len() >= 3
    }
    /// Indexes of the pins facing up (to the line side) and down.
    fn top_pins(&self) -> Vec<usize> {
        self.geometry.iter().enumerate().filter(|(_, g)| g.2 == 1).map(|(i, _)| i).collect()
    }
    fn bottom_pins(&self) -> Vec<usize> {
        self.geometry.iter().enumerate().filter(|(_, g)| g.2 == 4).map(|(i, _)| i).collect()
    }
}

/// One planned command.
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    /// From 1.
    pub sheet: usize,
    pub command: &'static str,
    pub params: Value,
}

/// The plan and what could not be used.
#[derive(Clone, Debug, Default)]
pub struct Plan {
    pub sheets: usize,
    pub components: usize,
    pub steps: Vec<Step>,
    pub warnings: Vec<String>,
}

/// Read the BOM into components.
fn components(bom: &str, t: &Template, block_exists: &dyn Fn(&str) -> bool, warnings: &mut Vec<String>) -> Result<Vec<Component>> {
    let mut out = Vec::new();
    for (i, r) in records(bom)?.iter().enumerate() {
        let Some(tag) = field(r, &["tag", "bmk", "device", "designation", "reference"]) else {
            warnings.push(format!("BOM row {}: no tag, skipped.", i + 2));
            continue;
        };
        let symbol = field(r, &["symbol", "block"]).map(str::to_string).or_else(|| {
            let ty = field(r, &["type", "componenttype", "kind"])?;
            t.symbols.get(&norm(ty)).cloned()
        });
        let Some(symbol) = symbol else {
            warnings.push(format!("BOM row {} ({tag}): no symbol or known type, skipped.", i + 2));
            continue;
        };
        let lib = symbols::find(&symbol);
        if lib.is_none() && !block_exists(&symbol) {
            warnings.push(format!("BOM row {} ({tag}): no symbol {symbol}, skipped.", i + 2));
            continue;
        }
        let (geometry, mut pins, child) = match lib {
            Some(d) => {
                let l = d.pin_list();
                (l.iter().map(|p| (p.0, p.1, p.2)).collect::<Vec<_>>(), l.iter().map(|p| p.3.to_string()).collect::<Vec<_>>(), d.child)
            }
            // A drawing block: two pins top and bottom assumed until placed.
            None => (vec![(0.0, 5.0, 1), (0.0, -5.0, 4)], vec!["1".into(), "2".into()], false),
        };
        if let Some(p) = field(r, &["pins", "terminals", "connections"]) {
            for (slot, n) in pins.iter_mut().zip(p.split([' ', ';', '/', '|']).filter(|x| !x.is_empty())) {
                *slot = n.to_string();
            }
        }
        let mut values = serde_json::Map::new();
        for (attr, names) in [
            ("MFG", &["manufacturer", "mfg", "maker"][..]),
            ("CAT", &["catalog", "catalogue", "partnumber", "cat", "part"][..]),
            ("DESC1", &["description", "desc", "desc1", "function"][..]),
            ("LOC", &["location", "loc"][..]),
            ("INST", &["installation", "inst"][..]),
        ] {
            if let Some(v) = field(r, names) {
                values.insert(attr.into(), json!(v.chars().take(255).collect::<String>()));
            }
        }
        for (k, n) in pins.iter().enumerate() {
            values.insert(format!("TERM{:02}", k + 1), json!(n));
        }
        out.push(Component { tag: tag.to_string(), symbol, pins, geometry, values, child });
    }
    Ok(out)
}

/// The netlist as nets: name → (component index, pin index).
fn nets(netlist: &str, comps: &[Component], warnings: &mut Vec<String>) -> Result<BTreeMap<String, Vec<(usize, usize)>>> {
    let resolve = |tag: &str, pin: &str| -> Option<(usize, usize)> {
        comps
            .iter()
            .enumerate()
            .filter(|(_, c)| c.tag.eq_ignore_ascii_case(tag))
            .find_map(|(i, c)| c.pins.iter().position(|p| p.eq_ignore_ascii_case(pin)).map(|k| (i, k)))
    };
    let split = |s: &str| -> Option<(String, String)> {
        let (t, p) = s.rsplit_once(':')?;
        Some((t.trim().to_string(), p.trim().to_string()))
    };
    let mut out: BTreeMap<String, Vec<(usize, usize)>> = BTreeMap::new();
    for (i, r) in records(netlist)?.iter().enumerate() {
        let mut problems: Vec<String> = Vec::new();
        let mut add = |net: &str, tag: &str, pin: &str, problems: &mut Vec<String>| match resolve(tag, pin) {
            Some(cp) => {
                let v = out.entry(net.to_string()).or_default();
                if !v.contains(&cp) {
                    v.push(cp);
                }
            }
            None => problems.push(format!("Netlist row {}: no pin {tag}:{pin} in the BOM.", i + 2)),
        };
        if let (Some(net), Some(dev), Some(pin)) =
            (field(r, &["net", "signal", "potential", "wire", "wireno"]), field(r, &["device", "tag", "component"]), field(r, &["pin", "terminal"]))
        {
            add(net, dev, pin, &mut problems);
        } else if let (Some(from), Some(to)) = (field(r, &["from", "source"]), field(r, &["to", "target", "destination"])) {
            let net = field(r, &["wire", "wireno", "net", "number"]).map(str::to_string).unwrap_or_else(|| format!("W{}", i + 1));
            for end in [from, to] {
                match split(end) {
                    Some((t, p)) => add(&net, &t, &p, &mut problems),
                    None => problems.push(format!("Netlist row {}: `{end}` is not tag:pin.", i + 2)),
                }
            }
        } else {
            problems.push(format!("Netlist row {}: needs net, device and pin, or from and to.", i + 2));
        }
        warnings.extend(problems);
    }
    // Nets joined through a shared pin are one net (a `from,to` list chains them).
    let names: Vec<String> = out.keys().cloned().collect();
    let mut merged: BTreeMap<String, Vec<(usize, usize)>> = BTreeMap::new();
    let mut owner: HashMap<(usize, usize), String> = HashMap::new();
    for n in names {
        let pins = out.remove(&n).unwrap_or_default();
        let joined: Vec<String> = pins.iter().filter_map(|p| owner.get(p).cloned()).collect::<HashSet<_>>().into_iter().collect();
        let target = joined.first().cloned().unwrap_or_else(|| n.clone());
        let mut all = merged.remove(&target).unwrap_or_default();
        for j in joined.iter().skip(1) {
            all.extend(merged.remove(j).unwrap_or_default());
        }
        all.extend(pins);
        all.sort();
        all.dedup();
        for p in &all {
            owner.insert(*p, target.clone());
        }
        merged.insert(target, all);
    }
    Ok(merged)
}

/// A column of the layout: a series path of components.
#[derive(Clone, Debug)]
struct Rung {
    comps: Vec<usize>,
    /// For a branch: the rung it leaves and the gap (after its `k`th component) it leaves from.
    from: Option<(usize, usize)>,
    power: bool,
    ends_on_bottom: bool,
    /// For a path that ends on another rung (a seal-in contact across a push-button): that
    /// rung and the component (`k`) whose line-side pin it joins: it ends in the gap above `k`.
    to: Option<(usize, usize)>,
}

/// Lay the components out in rungs.
fn rungs(comps: &[Component], nets: &BTreeMap<String, Vec<(usize, usize)>>, t: &Template) -> Vec<Rung> {
    let rail_top: HashSet<&str> = nets.keys().filter(|n| t.rails_top.iter().any(|r| r.eq_ignore_ascii_case(n))).map(String::as_str).collect();
    let rail_bottom: HashSet<&str> = nets.keys().filter(|n| t.rails_bottom.iter().any(|r| r.eq_ignore_ascii_case(n))).map(String::as_str).collect();
    let net_of: HashMap<(usize, usize), &str> = nets.iter().flat_map(|(n, ps)| ps.iter().map(move |p| (*p, n.as_str()))).collect();
    let mut placed: HashMap<usize, (usize, usize)> = HashMap::new(); // component → (rung, position)
    let mut out: Vec<Rung> = Vec::new();

    // Follow a series path down from `start`, through the first unplaced component on each net.
    let follow = |start: usize, placed: &HashMap<usize, (usize, usize)>| -> (Vec<usize>, bool) {
        let mut chain = vec![start];
        let mut seen: HashSet<usize> = HashSet::from([start]);
        while let Some(cur) = chain.last().copied() {
            let c = &comps[cur];
            let bottom = c.bottom_pins();
            let Some(net) = bottom.first().and_then(|b| net_of.get(&(cur, *b))) else { return (chain, false) };
            if rail_bottom.contains(net) {
                return (chain, true);
            }
            let next = nets.get(*net).into_iter().flatten().find(|(i, k)| {
                !seen.contains(i) && !placed.contains_key(i) && comps[*i].is_power() == c.is_power() && comps[*i].top_pins().first() == Some(k)
            });
            match next {
                Some((i, _)) => {
                    seen.insert(*i);
                    chain.push(*i);
                }
                None => return (chain, false),
            }
        }
        (chain, false)
    };

    let starts_top = |power: bool| -> Vec<usize> {
        (0..comps.len())
            .filter(|i| comps[*i].is_power() == power)
            .filter(|i| comps[*i].top_pins().first().and_then(|k| net_of.get(&(*i, *k))).is_none_or(|n| rail_top.contains(n) || power))
            .collect()
    };
    for power in [true, false] {
        // Rungs from the top rail (or, for power, from components fed from nothing placed).
        for s in starts_top(power) {
            if placed.contains_key(&s) {
                continue;
            }
            // A power component fed from another one is not a start.
            if power
                && comps[s]
                    .top_pins()
                    .first()
                    .and_then(|k| net_of.get(&(s, *k)))
                    .is_some_and(|n| nets.get(*n).into_iter().flatten().any(|(j, kk)| *j != s && comps[*j].bottom_pins().contains(kk)))
            {
                continue;
            }
            let (chain, ends) = follow(s, &placed);
            let r = out.len();
            for (k, c) in chain.iter().enumerate() {
                placed.insert(*c, (r, k));
            }
            out.push(Rung { comps: chain, from: None, power, ends_on_bottom: ends, to: None });
        }
        // Branches: components fed from a net between two placed components, again and again.
        loop {
            let mut grew = false;
            let pending: Vec<usize> = (0..comps.len()).filter(|i| comps[*i].is_power() == power && !placed.contains_key(i)).collect();
            for s in pending {
                if placed.contains_key(&s) {
                    continue;
                }
                let Some(net) = comps[s].top_pins().first().and_then(|k| net_of.get(&(s, *k))) else { continue };
                let parent = nets.get(*net).into_iter().flatten().find_map(|(j, kk)| {
                    let (r, k) = *placed.get(j)?;
                    comps[*j].bottom_pins().contains(kk).then_some((r, k))
                });
                let Some(parent) = parent else { continue };
                let (chain, ends) = follow(s, &placed);
                let r = out.len();
                for (k, c) in chain.iter().enumerate() {
                    placed.insert(*c, (r, k));
                }
                out.push(Rung { comps: chain, from: Some(parent), power, ends_on_bottom: ends, to: None });
                grew = true;
            }
            if !grew {
                break;
            }
        }
        // Whatever is left: rungs of their own.
        for s in (0..comps.len()).filter(|i| comps[*i].is_power() == power) {
            if placed.contains_key(&s) {
                continue;
            }
            let (chain, ends) = follow(s, &placed);
            let r = out.len();
            for (k, c) in chain.iter().enumerate() {
                placed.insert(*c, (r, k));
            }
            out.push(Rung { comps: chain, from: None, power, ends_on_bottom: ends, to: None });
        }
    }
    // Where paths that stop short of the bottom rail rejoin another rung.
    for (ri, rung) in out.iter_mut().enumerate() {
        if rung.ends_on_bottom {
            continue;
        }
        let Some(&last) = rung.comps.last() else { continue };
        let Some(net) = comps[last].bottom_pins().first().and_then(|b| net_of.get(&(last, *b))) else { continue };
        let target = nets.get(*net).into_iter().flatten().find_map(|(j, kk)| {
            let (r, k) = *placed.get(j)?;
            (r != ri && k > 0 && comps[*j].top_pins().first() == Some(kk)).then_some((r, k))
        });
        rung.to = target;
    }
    out
}

/// Plan the schematic. `side_by_side`: all sheets in one drawing, sheet `n` at x = (n−1) ×
/// sheet width (EGENERATE, the control channel); else every sheet at the origin of its own
/// drawing (a project).
pub fn plan(bom: &str, netlist: &str, t: &Template, side_by_side: bool, block_exists: &dyn Fn(&str) -> bool) -> Result<Plan> {
    let mut warnings = Vec::new();
    let comps = components(bom, t, block_exists, &mut warnings)?;
    if comps.is_empty() {
        return Err(EngineError::Other("The BOM has no usable components.".into()));
    }
    let nets = nets(netlist, &comps, &mut warnings)?;
    let rungs = rungs(&comps, &nets, t);

    // Columns: each rung takes one; a branch stays on its parent's sheet when there is room.
    let cw = t.column_width();
    let mut col_of: Vec<(usize, usize)> = Vec::with_capacity(rungs.len()); // (sheet, column from 0)
    let (mut sheet, mut col) = (1usize, 0usize);
    let mut last_power = rungs.first().is_some_and(|r| r.power);
    for r in &rungs {
        // Control circuits start on a new sheet after the power circuits.
        if last_power && !r.power && col > 0 {
            sheet += 1;
            col = 0;
        }
        last_power = r.power;
        if col >= t.columns {
            sheet += 1;
            col = 0;
        }
        col_of.push((sheet, col));
        col += 1;
    }
    let sheets = col_of.last().map(|c| c.0).unwrap_or(1);
    let ox = |s: usize| if side_by_side { (s - 1) as f64 * t.sheet_width } else { 0.0 };
    let x_of = |s: usize, c: usize| ox(s) + (c as f64 + 0.5) * cw;

    let mut steps: Vec<Step> = Vec::new();
    let step = |sheet: usize, command: &'static str, params: Value| Step { sheet, command, params };
    let control_sheets: Vec<usize> = {
        let mut v: Vec<usize> = rungs.iter().zip(&col_of).filter(|(r, _)| !r.power).map(|(_, c)| c.0).collect();
        v.dedup();
        v
    };
    // Rails across every control sheet, labelled with the rail nets' names.
    let top_name = nets.keys().find(|n| t.rails_top.iter().any(|r| r.eq_ignore_ascii_case(n))).cloned();
    let bottom_name = nets.keys().find(|n| t.rails_bottom.iter().any(|r| r.eq_ignore_ascii_case(n))).cloned();
    for s in &control_sheets {
        let (x0, x1) = (ox(*s) + 4.0, ox(*s) + t.sheet_width - 4.0);
        steps.push(step(*s, "ewire", json!({ "from": [x0, t.top], "to": [x1, t.top], "snap": 0 })));
        steps.push(step(*s, "ewire", json!({ "from": [x0, t.bottom], "to": [x1, t.bottom], "snap": 0 })));
        for (name, y) in [(&top_name, t.top), (&bottom_name, t.bottom)] {
            if let Some(n) = name {
                steps.push(step(*s, "text", json!({ "at": [x0, y + 2.0], "text": n, "height": 2.5 })));
            }
        }
    }

    // Positions, rung by rung (a branch after the rung it leaves): x, and each component's y.
    struct Placed {
        sheet: usize,
        x: f64,
        start_y: f64,
        end_y: f64,
        ys: Vec<f64>,
    }
    let snap = |y: f64| (y / 2.5).round() * 2.5;
    let gap_y = |pl: &Placed, k: usize| -> f64 {
        let below = pl.ys.get(k + 1).copied().unwrap_or(pl.end_y);
        snap((pl.ys[k] + below) / 2.0)
    };
    let mut placed: Vec<Placed> = Vec::with_capacity(rungs.len());
    for (ri, r) in rungs.iter().enumerate() {
        let (s, c) = col_of[ri];
        let x = x_of(s, c);
        let start_y = match r.from {
            Some((pr, pk)) if col_of.get(pr).is_some_and(|pc| pc.0 == s) => placed.get(pr).map(|pl| gap_y(pl, pk)).unwrap_or(t.top),
            _ => t.top,
        };
        let end_y = if r.ends_on_bottom || (!r.power && r.to.is_none()) { t.bottom } else { t.bottom + 20.0 };
        let n = r.comps.len();
        let span = start_y - end_y;
        let ys: Vec<f64> = (0..n).map(|k| snap(start_y - span * (k + 1) as f64 / (n + 1) as f64)).collect();
        placed.push(Placed { sheet: s, x, start_y, end_y, ys });
    }
    // A path rejoining a rung ends at the gap above the component it joins: squeeze it in.
    for (ri, r) in rungs.iter().enumerate() {
        let Some((tr, tk)) = r.to else { continue };
        if col_of.get(tr).map(|c| c.0) != Some(placed[ri].sheet) {
            continue;
        }
        let end = gap_y(&placed[tr], tk - 1);
        let pl = &mut placed[ri];
        if end < pl.start_y - 10.0 {
            let n = pl.ys.len();
            let span = pl.start_y - end;
            pl.end_y = end;
            pl.ys = (0..n).map(|k| snap(pl.start_y - span * (k + 1) as f64 / (n + 1) as f64)).collect();
        }
    }

    // Each rung: its wire(s) from top to end, then its symbols dropped on them (each cuts the
    // wire at its pins).
    let mut connectors: Vec<(usize, [f64; 2], [f64; 2])> = Vec::new();
    for (ri, r) in rungs.iter().enumerate() {
        let pl = &placed[ri];
        let (s, x) = (pl.sheet, pl.x);
        let n = r.comps.len();
        let first = &comps[r.comps[0]];
        let offsets: Vec<f64> = if r.power { first.top_pins().iter().map(|k| first.geometry[*k].0).collect() } else { vec![0.0] };
        let last = &comps[r.comps[n - 1]];
        let rejoins = r.to.is_some_and(|(tr, _)| col_of.get(tr).map(|c| c.0) == Some(s));
        // The wire runs to the bottom rail, the rejoin point, or the last component's last pin.
        // A path that goes nowhere stops at its last pin rather than touch the 0 V rail.
        let bottom = if r.ends_on_bottom || rejoins {
            pl.end_y
        } else if last.bottom_pins().is_empty() {
            pl.ys[n - 1] + 5.0
        } else {
            pl.ys[n - 1] - 5.0
        };
        for dx in &offsets {
            steps.push(step(s, "ewire", json!({ "from": [x + dx, pl.start_y], "to": [x + dx, bottom], "snap": 0 })));
        }
        for (k, ci) in r.comps.iter().enumerate() {
            let comp = &comps[*ci];
            let mut p = json!({ "name": comp.symbol, "at": [x, pl.ys[k]], "values": comp.values });
            p["tag"] = json!(comp.tag);
            steps.push(step(s, "esymbol", p));
        }
        if let Some((pr, _)) = r.from
            && col_of.get(pr).is_some_and(|pc| pc.0 == s)
        {
            connectors.push((s, [placed[pr].x, pl.start_y], [x, pl.start_y]));
        }
        if rejoins && let Some((tr, _)) = r.to {
            connectors.push((s, [x, pl.end_y], [placed[tr].x, pl.end_y]));
        }
    }
    // The branch and rejoin wires last, once every rung is drawn: each end joins a wire with a
    // junction dot.
    for (s, a, b) in connectors {
        steps.push(step(s, "ewire", json!({ "from": a, "to": b, "snap": 0.5 })));
    }
    // Wire numbers: once over all the sheets side by side, else on each sheet's own drawing.
    if side_by_side {
        steps.push(step(
            1,
            "ewirenum",
            json!({ "mode": "all", "firstSheet": 1, "origin": [0, 0], "sheetWidth": t.sheet_width, "columns": t.columns }),
        ));
    } else {
        for s in 1..=sheets {
            steps.push(step(
                s,
                "ewirenum",
                json!({ "mode": "all", "firstSheet": s, "origin": [0, 0], "sheetWidth": t.sheet_width, "columns": t.columns }),
            ));
        }
    }
    if rungs.iter().any(|r| r.comps.len() > 12) {
        warnings.push("Some circuits have more than 12 components in series: their symbols overlap; split them in the netlist.".into());
    }
    Ok(Plan { sheets, components: comps.len(), steps, warnings })
}

fn run_generate(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "egenerate";
    let bom = str_param(p, "bom").ok_or_else(|| bad(CMD, "`bom` (CSV text) is required"))?;
    let netlist = str_param(p, "netlist").unwrap_or("");
    let t = p.get("template").map(Template::from_json).unwrap_or_default();
    let doc = s.doc()?;
    let exists = |n: &str| doc.block(n).is_some();
    let plan = plan(bom, netlist, &t, true, &exists)?;
    let steps: Vec<Value> = plan.steps.iter().map(|st| json!({ "sheet": st.sheet, "command": st.command, "params": st.params })).collect();
    if bool_or(p, "planOnly", false) {
        return Ok(json!({ "sheets": plan.sheets, "components": plan.components, "steps": steps, "warnings": plan.warnings }));
    }
    // Sheet frames, side by side.
    let pr = super::project::Project { name: t.name.clone(), sheet_width: t.sheet_width, columns: t.columns as u64, ..Default::default() };
    let space = s.space();
    for n in 1..=plan.sheets {
        let frame = super::project::sheet_drawing(&pr, n as u64, plan.sheets as u64);
        // The title block's definition comes along with the frame.
        for (name, b) in &frame.blocks {
            if s.doc()?.block(name).is_none() {
                s.doc_mut()?.blocks.insert(name.clone(), b.clone());
            }
        }
        let dx = (n - 1) as f64 * t.sheet_width;
        for e in frame.model.iter() {
            let mut k = e.kind.clone();
            k.transform(&cadcraft_geom::Mat3::translate(cadcraft_geom::Vec2::new(dx, 0.0)));
            s.doc_mut()?.add(&space, e.common.clone(), k)?;
        }
    }
    let mut failed = Vec::new();
    for st in &plan.steps {
        if let Err(e) = s.execute(st.command, &st.params) {
            failed.push(format!("{} {}: {e}", st.command, st.params.get("tag").and_then(Value::as_str).unwrap_or("")));
        }
    }
    let mut warnings = plan.warnings;
    warnings.extend(failed);
    s.log.push(format!(
        "Schematic generated: {} component(s) on {} sheet(s){}.",
        plan.components,
        plan.sheets,
        if warnings.is_empty() { String::new() } else { format!(", {} warning(s)", warnings.len()) }
    ));
    Ok(json!({ "sheets": plan.sheets, "components": plan.components, "steps": steps.len(), "warnings": warnings }))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use cadcraft_doc::EntityKind;

    const BOM: &str = "tag,type,manufacturer,catalog,description,pins\n\
        -S1,Stop,Acme,PB-R,Stop,\n\
        -S2,Start,Acme,PB-G,Start,\n\
        -K1,Relay,Acme,R-24,Pump relay,\n\
        -K1,Contact,,,,23 24\n\
        -H1,Lamp,Acme,L-G,Pump running,\n\
        -Q1,Breaker3P,Acme,MCB-C16,,\n\
        -M1,Motor,Acme,M-1.5kW,Pump,\n";

    const NETS: &str = "net,device,pin\n\
        +24V,-S1,11\n\
        1,-S1,12\n1,-S2,13\n\
        2,-S2,14\n2,-K1,A1\n\
        0V,-K1,A2\n\
        +24V,-K1,23\n\
        3,-K1,24\n3,-H1,X1\n\
        0V,-H1,X2\n\
        L1,-Q1,1\nL2,-Q1,3\nL3,-Q1,5\n\
        U,-Q1,2\nU,-M1,U1\nV,-Q1,4\nV,-M1,V1\nW,-Q1,6\nW,-M1,W1\n";

    #[test]
    fn csv_parsing() {
        let rows = parse_csv("a,b\n\"x, y\",\"he said \"\"hi\"\"\"\n\n1;2\n").unwrap();
        assert_eq!(rows[1], vec!["x, y", "he said \"hi\""]);
        assert_eq!(rows.len(), 3);
        let semi = parse_csv("a;b\n1;2").unwrap();
        assert_eq!(semi[1], vec!["1", "2"]);
    }

    #[test]
    fn plan_lays_out_power_then_control() {
        let t = Template::default();
        let p = plan(BOM, NETS, &t, true, &|_| false).unwrap();
        assert_eq!(p.components, 7);
        assert!(p.warnings.is_empty(), "{:?}", p.warnings);
        assert_eq!(p.sheets, 2, "power on sheet 1, control on sheet 2");
        let syms: Vec<(usize, &str, &str)> = p
            .steps
            .iter()
            .filter(|s| s.command == "esymbol")
            .map(|s| (s.sheet, s.params["name"].as_str().unwrap(), s.params["tag"].as_str().unwrap()))
            .collect();
        assert_eq!(syms[0], (1, "IEC_MCB_3P", "-Q1"));
        assert_eq!(syms[1], (1, "IEC_MOTOR_3PH", "-M1"));
        // Stop then start then the coil in one rung; the contact and lamp in another.
        assert_eq!(syms[2..5].iter().map(|s| s.2).collect::<Vec<_>>(), vec!["-S1", "-S2", "-K1"]);
        assert!(syms.iter().any(|s| s.1 == "IEC_NO" && s.2 == "-K1"));
        // The second contact's pins from the BOM.
        let contact = p.steps.iter().find(|s| s.params["name"] == "IEC_NO").unwrap();
        assert_eq!(contact.params["values"]["TERM01"], "23");
        // Side by side: sheet 2's columns are past sheet 1.
        let x = p.steps.iter().find(|s| s.command == "esymbol" && s.sheet == 2).unwrap().params["at"][0].as_f64().unwrap();
        assert!(x > 420.0);
    }

    #[test]
    fn a_seal_in_contact_bridges_the_start_button() {
        let bom = "tag,type,pins\n-S1,Stop,\n-S2,Start,\n-K1,Relay,\n-K1,Contact,13 14\n";
        let nets = "net,device,pin\n+24V,-S1,11\n1,-S1,12\n1,-S2,13\n1,-K1,13\n2,-S2,14\n2,-K1,14\n2,-K1,A1\n0V,-K1,A2\n";
        let mut s = Session::new();
        let a = s.active;
        if let Some(d) = s.docs.get_mut(a) {
            d.title = "generate-seal-in".into();
        }
        let r = s.execute("egenerate", &json!({ "bom": bom, "netlist": nets })).unwrap();
        assert_eq!(r["warnings"], json!([]), "{r}");
        // The contact is across the start button: its pins are on the button's two nets, and
        // nothing joins it to 0 V.
        let c = s.execute("edb.query", &json!({ "what": "wirelist" })).unwrap();
        let pairs: Vec<String> =
            c.as_array().unwrap().iter().map(|w| format!("{} {}", w["from"].as_str().unwrap(), w["to"].as_str().unwrap())).collect();
        let joined = |a: &str, b: &str| pairs.iter().any(|p| p == &format!("{a} {b}") || p == &format!("{b} {a}"));
        assert!(joined("-S1:12", "-S2:13") || joined("-S1:12", "-K1:13"), "{pairs:?}");
        assert!(!pairs.iter().any(|p| p.contains("-K1:14") && p.contains("-K1:A2")), "seal-in shorted to 0 V: {pairs:?}");
        let cond = s.execute("edb.query", &json!({ "what": "devices" })).unwrap();
        let contact = cond.as_array().unwrap().iter().find(|d| d["child"] == true).unwrap()["id"].as_str().unwrap().to_string();
        let k14 = s.execute("edb.query", &json!({ "what": "conductor", "device": contact, "pin": "14" })).unwrap();
        let on: Vec<String> = k14.as_array().unwrap().iter().map(|p| p["pin"].as_str().unwrap().to_string()).collect();
        assert!(on.contains(&"A1".to_string()) && on.contains(&"14".to_string()), "14 joins the coil's A1: {on:?}");
    }

    #[test]
    fn from_to_netlists_and_bad_rows() {
        let nets = "wire,from,to\n101,-S1:12,-S2:13\n,-S2:14,-K1:A1\n,-K9:1,-K1:A2\n,broken,-K1:A2\n";
        let p = plan(BOM, nets, &Template::default(), false, &|_| false).unwrap();
        assert!(p.warnings.iter().any(|w| w.contains("-K9:1")), "{:?}", p.warnings);
        assert!(p.warnings.iter().any(|w| w.contains("broken")));
        assert!(plan("tag\n", "", &Template::default(), false, &|_| false).is_err(), "no components");
        let bad = plan("tag,symbol\n-X1,NOPE\n-K1,IEC_COIL\n", "", &Template::default(), false, &|_| false).unwrap();
        assert!(bad.warnings.iter().any(|w| w.contains("NOPE")));
    }

    #[test]
    fn generate_builds_a_wired_tagged_schematic() {
        let mut s = Session::new();
        let a = s.active;
        if let Some(d) = s.docs.get_mut(a) {
            d.title = "generate-test".into();
        }
        let r = s.execute("egenerate", &json!({ "bom": BOM, "netlist": NETS })).unwrap();
        assert_eq!(r["warnings"], json!([]), "{r}");
        let tags: Vec<String> = s
            .doc()
            .unwrap()
            .model
            .iter()
            .filter_map(|e| match &e.kind {
                EntityKind::Insert(i) => i.attribs.iter().find(|a| a.tag == "TAG1" || a.tag == "TAG2").map(|a| a.text.value.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(tags.len(), 7);
        assert!(tags.contains(&"-K1".to_string()) && tags.contains(&"-M1".to_string()));
        // The graph joins -S2:14 to -K1:A1, as the netlist says.
        let c = s.execute("edb.query", &json!({ "what": "wirelist" })).unwrap();
        let pairs: Vec<(String, String)> =
            c.as_array().unwrap().iter().map(|w| (w["from"].as_str().unwrap().to_string(), w["to"].as_str().unwrap().to_string())).collect();
        assert!(pairs.iter().any(|(a, b)| (a == "-S2:14" && b == "-K1:A1") || (a == "-K1:A1" && b == "-S2:14")), "{pairs:?}");
        // The contact is cross-referenced to its coil.
        let xref: Vec<String> = s
            .doc()
            .unwrap()
            .model
            .iter()
            .filter_map(|e| match &e.kind {
                EntityKind::Insert(i) if i.block == "IEC_NO" => i.attribs.iter().find(|a| a.tag == "XREF").map(|a| a.text.value.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(xref.len(), 1);
        assert!(xref[0].starts_with("/2."), "{xref:?}");
        assert!(s.execute("egenerate", &json!({})).is_err());
    }
}
