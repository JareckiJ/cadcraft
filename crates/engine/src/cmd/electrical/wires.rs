//! EWIRENUM: wire numbers by potential.
//!
//! Wires are lines and polylines on the wire layers (`WIRE*` by default). Wires that touch, end to
//! end or end on another wire (a T junction), form one *potential* (net); wires that only cross
//! stay apart, as on a schematic. Symbols (block references) break potentials: a relay coil's two
//! sides are two potentials.
//!
//! Each network gets one number from the sheet and column where it starts, its leftmost end
//! (`105`, then `105A` for a second network starting in the same column, AutoCAD Electrical
//! style), written as a text on the label layer (`WIRENO`) next to that start. Numbers already on the
//! drawing are kept unless every wire is renumbered.

use std::collections::{HashMap, HashSet};

use cadcraft_doc::{EntityKind, HAlign, Handle, Text, VAlign};
use cadcraft_geom::{Vec2, Vec3};
use serde_json::{Value, json};

use super::{Grid, MAX_FORMAT, MAX_TAG, render, unique};
use crate::cmd::curves::KW;
use crate::cmd::qselect::wildcard;
use crate::cmd::{CommandSpec, bad, bool_or, f64_or, str_param};
use crate::{EngineError, Input, Interactive, Prompt, Result, Session, Step};

pub(super) const WIRE_LAYERS: &str = "WIRE*";
pub(super) const LABEL_LAYER: &str = "WIRENO";
/// Texts naming a potential for good (`+24V`, `0V`, `L1`, `PE`), like EPLAN's potential
/// definition points: the network a text on this layer sits on takes its name as wire number,
/// and numbering never changes it.
pub(super) const POTENTIAL_LAYER: &str = "POTENTIAL";
const FORMAT: &str = "{sheet}{col2}";
/// Most wire segments handled at once.
const MAX_SEGMENTS: usize = 200_000;
/// Most grid cells one segment is filed under; longer segments are checked against everything.
const MAX_CELLS_PER_SEGMENT: i64 = 4_096;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("ewirenum", "Number Wires", run_ewire)
            .menu(&["Tools", "Electrical", "Number Wires"])
            .params(
                "{mode?: \"new\" (default: only potentials without a number) | \"all\" (renumber every potential), \
                 layers? (wire layers, wildcards, default \"WIRE*\"), labelLayer? (default \"WIRENO\"), \
                 format? (default \"{sheet}{col2}\", e.g. 105; placeholders {sheet} {column} {col2} {count}), height? (default 2.5), \
                 tolerance? (default 0.01), origin?, sheetWidth?, columns?, firstSheet?, dryRun? bool} \
                 → {potentials, numbered, kept, wires: [{number, handles: [handle], at: [x, y]}], orphanLabels: [handle]}",
            )
            .interactive(|_| Ok(Box::new(EwireM))),
    ]
}

/// One straight piece of a wire.
#[derive(Clone, Copy, Debug)]
pub(super) struct Seg {
    pub(super) a: Vec2,
    pub(super) b: Vec2,
    /// Index of the wire (entity) it belongs to.
    pub(super) wire: usize,
}

impl Seg {
    pub(super) fn dist_to(&self, p: Vec2) -> f64 {
        let d = self.b - self.a;
        let l2 = d.len2();
        let t = if l2 > 0.0 { ((p - self.a).dot(d) / l2).clamp(0.0, 1.0) } else { 0.0 };
        self.a.lerp(self.b, t).dist(p)
    }
    fn len(&self) -> f64 {
        self.a.dist(self.b)
    }
}

/// Union–find over wires.
struct Sets(Vec<usize>);

impl Sets {
    fn find(&mut self, mut i: usize) -> usize {
        while let Some(&p) = self.0.get(i) {
            if p == i {
                break;
            }
            let gp = self.0.get(p).copied().unwrap_or(p);
            if let Some(slot) = self.0.get_mut(i) {
                *slot = gp;
            }
            i = p;
        }
        i
    }
    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb
            && let Some(slot) = self.0.get_mut(ra.max(rb))
        {
            *slot = ra.min(rb);
        }
    }
}

/// Whether `layer` is one of the wire layers `layers` (comma-separated wildcards).
pub(super) fn wire_layer_matches(layers: &str, layer: &str) -> bool {
    layers.split(',').any(|p| wildcard(p.trim(), layer))
}

/// The wires on layers matching `layers` in the current space, as segments.
pub(super) fn wire_segments(s: &Session, layers: &str) -> Result<(Vec<Handle>, Vec<Seg>)> {
    let d = s.doc()?;
    let Some(store) = d.space(&s.space()) else { return Ok((Vec::new(), Vec::new())) };
    let mut handles = Vec::new();
    let mut segs = Vec::new();
    for e in store.iter() {
        if !wire_layer_matches(layers, &e.common.layer) {
            continue;
        }
        let pts: Vec<Vec2> = match &e.kind {
            EntityKind::Line(l) => vec![l.a.xy(), l.b.xy()],
            EntityKind::LwPolyline(pl) => {
                let mut v: Vec<Vec2> = pl.vertices.iter().map(|v| v.p).collect();
                if pl.closed
                    && let Some(f) = v.first().copied()
                {
                    v.push(f);
                }
                v
            }
            _ => continue,
        };
        if pts.iter().any(|p| !p.is_finite()) {
            continue;
        }
        let wire = handles.len();
        for w in pts.windows(2) {
            if let [a, b] = w {
                if segs.len() >= MAX_SEGMENTS {
                    return Err(EngineError::Other(format!("More than {MAX_SEGMENTS} wire segments; number one area at a time.")));
                }
                segs.push(Seg { a: *a, b: *b, wire });
            }
        }
        handles.push(e.handle);
    }
    Ok((handles, segs))
}

/// The potentials: groups of wire indices, each wire in exactly one.
pub(super) fn potentials(wires: usize, segs: &[Seg], tol: f64) -> Vec<Vec<usize>> {
    let mut sets = Sets((0..wires).collect());
    // File segments in a uniform grid sized to the drawing, so each end is tested only against
    // nearby segments.
    let (mut lo, mut hi) = (Vec2::new(f64::MAX, f64::MAX), Vec2::new(f64::MIN, f64::MIN));
    for s in segs {
        for p in [s.a, s.b] {
            lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
            hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
        }
    }
    let extent = (hi.x - lo.x).max(hi.y - lo.y);
    let n = segs.len().max(1) as f64;
    let cell = (extent / n.sqrt()).max(tol * 4.0).max(1e-9);
    let key = |p: f64, o: f64| -> i64 {
        let k = ((p - o) / cell).floor();
        if k.is_finite() { k.clamp(-1e15, 1e15) as i64 } else { 0 }
    };
    let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    let mut big: Vec<usize> = Vec::new();
    for (i, s) in segs.iter().enumerate() {
        let (x0, x1) = (key(s.a.x.min(s.b.x) - tol, lo.x), key(s.a.x.max(s.b.x) + tol, lo.x));
        let (y0, y1) = (key(s.a.y.min(s.b.y) - tol, lo.y), key(s.a.y.max(s.b.y) + tol, lo.y));
        let cells = x1.saturating_sub(x0).saturating_add(1).saturating_mul(y1.saturating_sub(y0).saturating_add(1));
        if cells > MAX_CELLS_PER_SEGMENT {
            big.push(i);
            continue;
        }
        for x in x0..=x1 {
            for y in y0..=y1 {
                grid.entry((x, y)).or_default().push(i);
            }
        }
    }
    for s in segs {
        for p in [s.a, s.b] {
            let near = grid.get(&(key(p.x, lo.x), key(p.y, lo.y))).into_iter().flatten().chain(big.iter());
            for &j in near {
                if let Some(o) = segs.get(j)
                    && o.wire != s.wire
                    && o.dist_to(p) <= tol
                {
                    sets.union(s.wire, o.wire);
                }
            }
        }
    }
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for w in 0..wires {
        let r = sets.find(w);
        groups.entry(r).or_default().push(w);
    }
    groups.into_values().collect()
}

/// A wire network of the current space: the symbol pins on it and its wire number.
#[derive(Clone, Debug, Default)]
pub(super) struct Network {
    /// (block reference, pin number, position) of every pin on the network.
    pub pins: Vec<(Handle, String, Vec2)>,
    pub number: Option<String>,
    /// The wires (lines and polylines) of the network.
    pub wire_handles: Vec<Handle>,
}

/// The wire networks of the current space (on the default wire and label layers), with the
/// symbol pins on each: what a pin is wired to.
pub(super) fn networks(s: &Session) -> Result<Vec<Network>> {
    let (handles, segs) = wire_segments(s, WIRE_LAYERS)?;
    let groups = potentials(handles.len(), &segs, 0.01);
    let numbers = labels(s, LABEL_LAYER)?;
    let fixed = labels(s, POTENTIAL_LAYER)?;
    let ix = super::pins::PinIndex::build(s)?;
    let mut out = Vec::with_capacity(groups.len());
    for g in &groups {
        let gsegs: Vec<&Seg> = segs.iter().filter(|sg| g.contains(&sg.wire)).collect();
        let pins: Vec<(Handle, String, Vec2)> =
            ix.pins.iter().filter(|p| gsegs.iter().any(|sg| sg.dist_to(p.at) <= 0.01)).map(|p| (p.insert, p.number.clone(), p.at)).collect();
        let number = network_number(&gsegs, &numbers, &fixed);
        out.push(Network { pins, number, wire_handles: g.iter().filter_map(|w| handles.get(*w)).copied().collect() });
    }
    Ok(out)
}

/// A network's number: the fixed potential name on it, else the wire number label nearest one
/// of its wires (within reach).
pub(super) fn network_number(gsegs: &[&Seg], numbers: &[(Handle, Vec2, String)], fixed: &[(Handle, Vec2, String)]) -> Option<String> {
    let near = |list: &[(Handle, Vec2, String)]| {
        list.iter()
            .filter(|(_, _, v)| !v.is_empty())
            .map(|(_, at, v)| (gsegs.iter().map(|sg| sg.dist_to(*at)).fold(f64::INFINITY, f64::min), v))
            .filter(|(d, _)| *d <= 6.0)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, v)| v.clone())
    };
    near(fixed).or_else(|| near(numbers))
}

/// Texts on the label layer in the current space: handle, insertion point, value.
pub(super) fn labels(s: &Session, layer: &str) -> Result<Vec<(Handle, Vec2, String)>> {
    let d = s.doc()?;
    let Some(store) = d.space(&s.space()) else { return Ok(Vec::new()) };
    Ok(store
        .iter()
        .filter(|e| e.common.layer.eq_ignore_ascii_case(layer))
        .filter_map(|e| match &e.kind {
            EntityKind::Text(t) => Some((e.handle, t.justify_point(), t.value.trim().to_string())),
            _ => None,
        })
        .collect())
}

/// Where a network starts and where its number goes: its leftmost end (the topmost of equals),
/// the label point a little along the wire from there, and whether that wire is vertical.
fn anchor(segs: &[&Seg], height: f64) -> Option<(Vec2, Vec2, bool)> {
    let start = segs.iter().flat_map(|s| [s.a, s.b]).min_by(|p, q| p.x.total_cmp(&q.x).then(q.y.total_cmp(&p.y)))?;
    // The wire leaving the start: horizontal ones first (numbers read along them), then longest.
    let seg = segs.iter().filter(|s| s.a.near(start, 1e-9) || s.b.near(start, 1e-9)).max_by(|a, b| {
        let flat = |s: &Seg| (s.b.x - s.a.x).abs() >= (s.b.y - s.a.y).abs();
        flat(a).cmp(&flat(b)).then(a.len().total_cmp(&b.len()))
    })?;
    let other = if seg.a.near(start, 1e-9) { seg.b } else { seg.a };
    let len = seg.len();
    let along = (len * 0.5).min(height * 3.0);
    let at = if len > 0.0 { start.lerp(other, along / len) } else { start };
    let d = other - start;
    Some((start, at, d.y.abs() > d.x.abs()))
}

/// The text for number `value` of a potential anchored at `at`.
fn label_text(value: &str, at: Vec2, vertical: bool, height: f64) -> Text {
    let gap = height * 0.4;
    let (p, rotation) = if vertical { (Vec2::new(at.x - gap, at.y), std::f64::consts::FRAC_PI_2) } else { (Vec2::new(at.x, at.y + gap), 0.0) };
    let p3 = Vec3::new(p.x, p.y, 0.0);
    Text {
        insert: p3,
        align_pt: Some(p3),
        height,
        value: value.to_string(),
        rotation,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: HAlign::Center,
        valign: VAlign::Bottom,
    }
}

struct Potential {
    wires: Vec<usize>,
    at: Vec2,
    /// Where its number goes.
    label_at: Vec2,
    vertical: bool,
    sheet: u64,
    column: u64,
    /// Label already on the drawing (handle, value).
    label: Option<(Handle, String)>,
    /// Its fixed name (a text on the POTENTIAL layer).
    fixed: Option<String>,
}

pub(super) fn run_ewire(s: &mut Session, p: &Value) -> Result<Value> {
    let r = run_ewire_inner(s, p)?;
    // Keep the project database in step with the drawing.
    super::sync::auto(s);
    Ok(r)
}

fn run_ewire_inner(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "ewirenum";
    let renumber_all = match str_param(p, "mode").map(str::to_ascii_lowercase).as_deref() {
        None | Some("new") => false,
        Some("all") => true,
        Some(m) => return Err(bad(CMD, format!("`mode` must be \"new\" or \"all\", not \"{m}\""))),
    };
    let layers = str_param(p, "layers").map(str::trim).filter(|l| !l.is_empty()).unwrap_or(WIRE_LAYERS).to_string();
    let label_layer = str_param(p, "labelLayer").map(str::trim).filter(|l| !l.is_empty()).unwrap_or(LABEL_LAYER).to_string();
    if label_layer.chars().count() > 255 || layers.chars().count() > 1024 {
        return Err(bad(CMD, "`layers` or `labelLayer` is too long"));
    }
    let format = str_param(p, "format").unwrap_or(FORMAT);
    if format.chars().count() > MAX_FORMAT || !["{sheet}", "{column}", "{col2}", "{count}"].iter().any(|k| format.contains(k)) {
        return Err(bad(CMD, format!("`format` must contain {{sheet}}, {{column}}, {{col2}} or {{count}} and be at most {MAX_FORMAT} characters")));
    }
    let counted = format.contains("{count}");
    let height = f64_or(p, "height", 2.5);
    let tol = f64_or(p, "tolerance", 0.01);
    if !(height > 0.0 && height < 1e6) || !(0.0..1e6).contains(&tol) {
        return Err(bad(CMD, "`height` must be positive and `tolerance` zero or more"));
    }
    let grid = Grid::from_params(CMD, p)?;
    let dry_run = bool_or(p, "dryRun", false);

    let (handles, segs) = wire_segments(s, &layers)?;
    let groups = potentials(handles.len(), &segs, tol);
    let mut by_wire: Vec<Vec<&Seg>> = vec![Vec::new(); handles.len()];
    for sg in &segs {
        if let Some(v) = by_wire.get_mut(sg.wire) {
            v.push(sg);
        }
    }

    // Existing labels belong to the potential with a wire nearest them (within reach).
    let reach = height * 2.0 + tol;
    let existing = labels(s, &label_layer)?;
    let mut owner: HashMap<usize, usize> = HashMap::new(); // label index -> potential index
    for (li, (_, at, _)) in existing.iter().enumerate() {
        let best = groups
            .iter()
            .enumerate()
            .flat_map(|(gi, g)| g.iter().flat_map(|w| by_wire.get(*w).into_iter().flatten()).map(move |sg| (gi, sg.dist_to(*at))))
            .filter(|(_, d)| *d <= reach)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((gi, _)) = best {
            owner.insert(li, gi);
        }
    }

    // Fixed potential names.
    let names = labels(s, POTENTIAL_LAYER)?;
    let mut named: HashMap<usize, String> = HashMap::new(); // potential index -> name
    for (_, at, v) in names.iter().filter(|(_, _, v)| !v.is_empty()) {
        let best = groups
            .iter()
            .enumerate()
            .flat_map(|(gi, g)| g.iter().flat_map(|w| by_wire.get(*w).into_iter().flatten()).map(move |sg| (gi, sg.dist_to(*at))))
            .filter(|(_, d)| *d <= reach)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((gi, _)) = best {
            named.entry(gi).or_insert_with(|| v.clone());
        }
    }

    let mut pots: Vec<Potential> = Vec::new();
    let mut outside = 0usize;
    for (gi, g) in groups.iter().enumerate() {
        let gsegs: Vec<&Seg> = g.iter().flat_map(|w| by_wire.get(*w).into_iter().flatten().copied()).collect();
        let Some((at, label_at, vertical)) = anchor(&gsegs, height) else { continue };
        let Some((sheet, column)) = grid.locate(at.x) else {
            outside += 1;
            continue;
        };
        let label = owner
            .iter()
            .filter(|(_, owner_g)| **owner_g == gi)
            .filter_map(|(li, _)| existing.get(*li))
            .min_by_key(|(h, ..)| *h)
            .map(|(h, _, v)| (*h, v.clone()));
        pots.push(Potential { wires: g.clone(), at, label_at, vertical, sheet, column, label, fixed: named.get(&gi).cloned() });
    }
    pots.sort_by(|a, b| (a.sheet, a.column).cmp(&(b.sheet, b.column)).then(b.at.y.total_cmp(&a.at.y)).then(a.at.x.total_cmp(&b.at.x)));

    // Numbers kept (new mode) are reserved first.
    let keep = |pt: &Potential| !renumber_all && pt.label.as_ref().is_some_and(|(_, v)| !v.is_empty());
    let mut used: HashSet<String> = pots.iter().filter(|pt| keep(pt)).filter_map(|pt| pt.label.as_ref().map(|(_, v)| v.clone())).collect();
    used.extend(pots.iter().filter_map(|pt| pt.fixed.clone()));
    // Numbers taken elsewhere (other drawings of a project).
    used.extend(super::reserved_param(p));
    let mut counters: HashMap<String, u64> = HashMap::new();
    let mut plan: Vec<(usize, String)> = Vec::new();
    for (i, pt) in pots.iter().enumerate() {
        if keep(pt) || pt.fixed.is_some() {
            continue;
        }
        let number = if counted {
            let n = counters.entry(render(format, "", pt.sheet, pt.column, None)).or_insert(0);
            let mut found = None;
            for _ in 0..MAX_SEGMENTS {
                *n = n.saturating_add(1);
                let t = render(format, "", pt.sheet, pt.column, Some(*n));
                if !used.contains(&t) {
                    found = Some(t);
                    break;
                }
            }
            found
        } else {
            unique(&render(format, "", pt.sheet, pt.column, None), &used)
        };
        let Some(number) = number.filter(|t| t.len() <= MAX_TAG) else {
            return Err(EngineError::Other("Could not make a unique wire number.".into()));
        };
        used.insert(number.clone());
        plan.push((i, number));
    }

    // Labels not near any wire are reported, and removed when renumbering everything.
    let orphans: Vec<Handle> = existing.iter().enumerate().filter(|(li, _)| !owner.contains_key(li)).map(|(_, (h, ..))| *h).collect();

    let mut numbered = 0usize;
    if !dry_run {
        let space = s.space();
        let mut common = s.current_common();
        common.layer = label_layer.clone();
        if renumber_all {
            let d = s.doc_mut()?;
            for (h, ..) in &existing {
                d.remove_entity(*h);
            }
        }
        // A named potential needs no number of its own: its name is its label.
        if !renumber_all {
            let stale: Vec<Handle> = pots.iter().filter(|pt| pt.fixed.is_some()).filter_map(|pt| pt.label.as_ref().map(|(h, _)| *h)).collect();
            let d = s.doc_mut()?;
            for h in stale {
                d.remove_entity(h);
            }
        }
        for (i, number) in &plan {
            let Some(pt) = pots.get(*i) else { continue };
            match (&pt.label, renumber_all) {
                // A label with an empty value: fill it in place.
                (Some((h, _)), false) => {
                    let n = number.clone();
                    s.doc_mut()?.modify_entity(*h, |e| {
                        if let EntityKind::Text(t) = &mut e.kind {
                            t.value = n;
                        }
                    })?;
                }
                _ => {
                    let t = label_text(number, pt.label_at, pt.vertical, height);
                    s.doc_mut()?.add(&space, common.clone(), EntityKind::Text(t))?;
                }
            }
            numbered += 1;
        }
        s.log.push(format!(
            "{} potential(s): {numbered} numbered, {} kept{}{}.",
            pots.len(),
            pots.len().saturating_sub(plan.len()),
            if outside == 0 { String::new() } else { format!(", {outside} outside the sheets") },
            if orphans.is_empty() || renumber_all { String::new() } else { format!(", {} label(s) not on a wire", orphans.len()) },
        ));
    }

    let numbers: HashMap<usize, &String> = plan.iter().map(|(i, n)| (*i, n)).collect();
    let list: Vec<Value> = pots
        .iter()
        .enumerate()
        .map(|(i, pt)| {
            let number = pt.fixed.clone().or_else(|| numbers.get(&i).map(|n| n.to_string())).or_else(|| pt.label.as_ref().map(|(_, v)| v.clone()));
            let hs: Vec<String> = pt.wires.iter().filter_map(|w| handles.get(*w)).map(|h| h.hex()).collect();
            json!({ "number": number, "handles": hs, "at": [pt.at.x, pt.at.y], "sheet": pt.sheet, "column": pt.column })
        })
        .collect();
    let orphan_hex: Vec<String> = orphans.iter().map(|h| h.hex()).collect();
    Ok(json!({
        "potentials": pots.len(),
        "numbered": numbered,
        "kept": pots.len().saturating_sub(plan.len()),
        "outside": outside,
        "wires": list,
        "orphanLabels": orphan_hex,
        "dryRun": dry_run,
    }))
}

/// EWIRENUM at the command line: `Number wires [New/All] <New>:`.
struct EwireM;

impl Interactive for EwireM {
    fn name(&self) -> &'static str {
        "EWIRENUM"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new("Number wires", KW).kw(&["New", "All"]).default("New")
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let mode = match i {
            Input::Enter => "new",
            Input::Keyword(k) | Input::Text(k) => match k.trim().to_ascii_lowercase().as_str() {
                "new" | "n" | "" => "new",
                "all" | "a" => "all",
                _ => return Err(EngineError::Other("Invalid option keyword.".into())),
            },
            _ => return Ok(Step::Continue),
        };
        run_ewire(s, &json!({ "mode": mode }))?;
        Ok(Step::Done)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn session() -> Session {
        let mut s = Session::new();
        s.execute("layer.new", &json!({ "name": "WIRE" })).unwrap();
        s
    }

    fn wire(s: &mut Session, pts: &[[f64; 2]]) -> String {
        if pts.len() == 2 {
            s.execute("line", &json!({ "points": pts })).unwrap();
        } else {
            s.execute("pline", &json!({ "points": pts })).unwrap();
        }
        let h = s.doc().unwrap().model.last().unwrap().handle.hex();
        s.execute("properties.set", &json!({ "handles": [h], "layer": "WIRE" })).unwrap();
        h
    }

    fn numbers(s: &Session) -> Vec<String> {
        let mut v: Vec<String> = s
            .doc()
            .unwrap()
            .model
            .iter()
            .filter(|e| e.common.layer == "WIRENO")
            .filter_map(|e| if let EntityKind::Text(t) = &e.kind { Some(t.value.clone()) } else { None })
            .collect();
        v.sort();
        v
    }

    #[test]
    fn touching_wires_share_a_number_crossing_wires_do_not() {
        let mut s = session();
        // Potential 1: an L (end to end) plus a T branch, in column 1.
        wire(&mut s, &[[0.0, 100.0], [30.0, 100.0]]);
        wire(&mut s, &[[30.0, 100.0], [30.0, 60.0]]);
        wire(&mut s, &[[30.0, 80.0], [40.0, 80.0]]); // T on the vertical
        // Potential 2: crosses the vertical without a junction, in column 1 too.
        wire(&mut s, &[[20.0, 70.0], [40.0, 70.0]]);
        // Potential 3: sheet 2.
        wire(&mut s, &[[430.0, 10.0], [500.0, 10.0]]);
        let r = s.execute("ewirenum", &json!({})).unwrap();
        assert_eq!(r["potentials"], 3, "{r}");
        assert_eq!(r["numbered"], 3);
        assert_eq!(numbers(&s), vec!["101", "101A", "201"]);
        let first = r["wires"].as_array().unwrap().iter().find(|w| w["handles"].as_array().unwrap().len() == 3).unwrap();
        assert_eq!(first["number"], "101", "the higher potential comes first");
    }

    #[test]
    fn new_mode_keeps_numbers_all_mode_renumbers() {
        let mut s = session();
        wire(&mut s, &[[0.0, 0.0], [30.0, 0.0]]);
        s.execute("ewirenum", &json!({})).unwrap();
        assert_eq!(numbers(&s), vec!["101"]);
        // A second run adds nothing; a new wire gets the next number.
        wire(&mut s, &[[5.0, 50.0], [35.0, 50.0]]);
        let r = s.execute("ewirenum", &json!({})).unwrap();
        assert_eq!(r["kept"], 1);
        assert_eq!(r["numbered"], 1);
        assert_eq!(numbers(&s), vec!["101", "101A"]);
        // Renumbering everything puts the upper wire first.
        let r = s.execute("ewirenum", &json!({ "mode": "all" })).unwrap();
        assert_eq!(r["numbered"], 2);
        assert_eq!(numbers(&s), vec!["101", "101A"]);
        let upper = r["wires"].as_array().unwrap().iter().find(|w| w["at"][1].as_f64().unwrap() > 40.0).unwrap();
        assert_eq!(upper["number"], "101");
    }

    #[test]
    fn polylines_sequential_format_and_dry_run() {
        let mut s = session();
        wire(&mut s, &[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]]);
        wire(&mut s, &[[10.0, 10.0], [20.0, 10.0]]);
        wire(&mut s, &[[100.0, 0.0], [120.0, 0.0]]);
        let r = s.execute("ewirenum", &json!({ "format": "W{count}", "dryRun": true })).unwrap();
        assert_eq!(r["potentials"], 2);
        assert!(numbers(&s).is_empty(), "dry run draws nothing");
        s.execute("ewirenum", &json!({ "format": "{count}" })).unwrap();
        assert_eq!(numbers(&s), vec!["1", "2"]);
    }

    #[test]
    fn other_layers_are_not_wires_and_undo_works() {
        let mut s = session();
        s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
        let r = s.execute("ewirenum", &json!({})).unwrap();
        assert_eq!(r["potentials"], 0);
        wire(&mut s, &[[0.0, 5.0], [10.0, 5.0]]);
        s.execute("ewirenum", &json!({})).unwrap();
        assert_eq!(numbers(&s).len(), 1);
        s.execute("undo", &json!({})).unwrap();
        assert!(numbers(&s).is_empty());
    }

    #[test]
    fn command_line_form() {
        let mut s = session();
        wire(&mut s, &[[0.0, 0.0], [10.0, 0.0]]);
        s.cmdline("ewirenum").unwrap();
        s.cmdline("").unwrap();
        assert!(s.running.is_none());
        assert_eq!(numbers(&s), vec!["101"]);
    }

    #[test]
    fn hostile_params_are_refused() {
        let mut s = session();
        wire(&mut s, &[[1e300, 0.0], [-1e300, 0.0]]);
        for p in [
            json!({"height": 0}),
            json!({"height": -1}),
            json!({"tolerance": -1}),
            json!({"format": "plain"}),
            json!({"format": "x".repeat(100) + "{count}"}),
            json!({"mode": "x"}),
            json!({"sheetWidth": 0}),
            json!({"layers": "x".repeat(5000)}),
        ] {
            assert!(s.execute("ewirenum", &p).is_err(), "{p}");
        }
        // A huge wire is handled (or reported outside), never a panic.
        let _ = s.execute("ewirenum", &json!({})).unwrap();
    }

    #[test]
    fn union_find_and_distance() {
        let mut u = Sets((0..5).collect());
        u.union(0, 3);
        u.union(3, 4);
        assert_eq!(u.find(4), u.find(0));
        assert_ne!(u.find(1), u.find(0));
        let sg = Seg { a: Vec2::new(0.0, 0.0), b: Vec2::new(10.0, 0.0), wire: 0 };
        assert!((sg.dist_to(Vec2::new(5.0, 3.0)) - 3.0).abs() < 1e-12);
        assert!((sg.dist_to(Vec2::new(-4.0, 3.0)) - 5.0).abs() < 1e-12);
    }
}
