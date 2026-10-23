//! Drawing wires: EWIRE and ELADDER.
//!
//! EWIRE draws a wire between two points, each snapped to the nearest symbol connection
//! point (or onto a wire, making a T junction with a dot) within reach, and routed at right
//! angles: leaving a pin the way its connection point faces. ELADDER lays out a ladder: two
//! horizontal supply rails and a vertical rung in the middle of each column between them, ready
//! for symbols to be dropped on (ESYMBOL cuts the rung at the symbol's pins).
//!
//! A wire drawn over symbols (through their pins, as along a rung) is cut at each symbol's
//! pins, so every pin ends a wire segment, found with the [`PinIndex`]. The wire then joins those
//! pins in the connection graph straight away: like every electrical command EWIRE updates the
//! project database when it finishes, and its result lists the pins it ends on.

use cadcraft_doc::{EntityKind, Handle, Line, LwPolyline};
use cadcraft_geom::{PolyVertex, Vec2, Vec3};
use serde_json::{Value, json};

use super::pins::PinIndex;
use super::wires::{WIRE_LAYERS, wire_layer_matches};
use crate::cmd::{CommandSpec, bad, f64_or, point_req, str_param};
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

const WIRE_LAYER: &str = "WIRE";
const JUNCTION_LAYER: &str = "JUNCTION";
/// Most cutting passes when a wire crosses several symbols.
const MAX_PASSES: usize = 64;
/// Most rungs one ladder may have.
const MAX_RUNGS: usize = 500;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("ewire", "Draw Wire", run_wiredraw)
            .alias(&["ewiredraw", "aewire"])
            .menu(&["Tools", "Electrical", "Draw Wire"])
            .params(
                "{from: [x, y], to: [x, y], snap? (reach, default 3), layer? (default \"WIRE\"), \
                 verticalFirst? (default: from the start pin's direction)} → {handle, from, to, snappedFrom, snappedTo, junctions}",
            )
            .interactive(|_| Ok(Box::new(WireM::default()))),
        CommandSpec::new("eladder", "Insert Ladder", run_ladder)
            .menu(&["Tools", "Electrical", "Insert Ladder"])
            .params(
                "{top: y, bottom: y, from? x (default 0), to? x (default from + 420), spacing? (default 42, a column), \
                 layer? (default \"WIRE\")} → {rails: [handle], rungs: [handle]}",
            )
            .interactive(|_| Ok(Box::new(super::interactive::LadderM::default()))),
        CommandSpec::new("eladder.clean", "Remove Empty Rungs", run_clean).menu(&["Tools", "Electrical", "Remove Empty Rungs"]).params(
            "{dryRun?: bool} → {removed: [handle]} — ladder rungs nothing was placed on: straight vertical wires from \
                 one horizontal wire to another with no symbol, pin or branch on them (each would short its rails)",
        ),
    ]
}

fn run_clean(s: &mut Session, p: &Value) -> Result<Value> {
    let r = run_clean_inner(s, p)?;
    super::sync::auto(s);
    Ok(r)
}

/// The empty rungs: wires of one vertical segment whose two ends both land on other, horizontal
/// wires, with nothing else on them.
fn empty_rungs(s: &Session) -> Result<Vec<(Handle, Vec2, Vec2)>> {
    const TOL: f64 = 1e-6;
    let (handles, segs) = super::wires::wire_segments(s, WIRE_LAYERS)?;
    let pins = PinIndex::build(s)?;
    let mut per_wire = vec![0usize; handles.len()];
    for g in &segs {
        if let Some(n) = per_wire.get_mut(g.wire) {
            *n += 1;
        }
    }
    let horizontal = |g: &super::wires::Seg| (g.a.y - g.b.y).abs() < TOL && (g.a.x - g.b.x).abs() > TOL;
    let mut out = Vec::new();
    for g in &segs {
        if per_wire.get(g.wire) != Some(&1) || (g.a.x - g.b.x).abs() > TOL || (g.a.y - g.b.y).abs() < TOL {
            continue;
        }
        let Some(&h) = handles.get(g.wire) else { continue };
        let others = || segs.iter().filter(|o| o.wire != g.wire);
        let end_ok = |p: Vec2| {
            let touching: Vec<&super::wires::Seg> = others().filter(|o| o.dist_to(p) < TOL).collect();
            !touching.is_empty() && touching.iter().all(|o| horizontal(o)) && pins.exact(p, TOL).is_none()
        };
        if !end_ok(g.a) || !end_ok(g.b) {
            continue;
        }
        // Nothing branches off it, and no symbol sits across it.
        let branch = others().any(|o| [o.a, o.b].iter().any(|q| !q.near(g.a, TOL) && !q.near(g.b, TOL) && g.dist_to(*q) < TOL));
        if branch || !pins.crossed(g.a, g.b).is_empty() {
            continue;
        }
        out.push((h, g.a, g.b));
    }
    Ok(out)
}

fn run_clean_inner(s: &mut Session, p: &Value) -> Result<Value> {
    let rungs = empty_rungs(s)?;
    let removed: Vec<String> = rungs.iter().map(|r| r.0.hex()).collect();
    if p.get("dryRun").and_then(Value::as_bool).unwrap_or(false) || rungs.is_empty() {
        return Ok(json!({ "removed": removed }));
    }
    // The junction dots at their ends go too: the rail just runs on there.
    let ends: Vec<Vec2> = rungs.iter().flat_map(|r| [r.1, r.2]).collect();
    let dots: Vec<Handle> = {
        let d = s.doc()?;
        d.space(&s.space())
            .map(|store| {
                store
                    .iter()
                    .filter(|e| e.common.layer.eq_ignore_ascii_case(JUNCTION_LAYER))
                    .filter_map(|e| match &e.kind {
                        EntityKind::LwPolyline(pl) if !pl.vertices.is_empty() => {
                            let n = pl.vertices.len() as f64;
                            let c = pl.vertices.iter().fold(Vec2::new(0.0, 0.0), |acc, v| acc + v.p) * (1.0 / n);
                            ends.iter().any(|q| q.near(c, 1e-3)).then_some(e.handle)
                        }
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let d = s.doc_mut()?;
    for (h, _, _) in &rungs {
        d.remove_entity(*h);
    }
    for h in dots {
        d.remove_entity(h);
    }
    s.log.push(format!("{} empty rung(s) removed.", rungs.len()));
    Ok(json!({ "removed": removed }))
}

/// What a point snapped to.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Snap {
    /// A symbol's connection point, with its direction digit.
    Pin(Vec2, u8),
    /// The middle of a wire (a T junction).
    Wire(Vec2),
    /// The end of a wire.
    WireEnd(Vec2),
    Free(Vec2),
}

impl Snap {
    fn point(self) -> Vec2 {
        match self {
            Snap::Pin(p, _) | Snap::Wire(p) | Snap::WireEnd(p) | Snap::Free(p) => p,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Snap::Pin(..) => "pin",
            Snap::Wire(_) => "wire",
            Snap::WireEnd(_) => "wire end",
            Snap::Free(_) => "none",
        }
    }
}

/// `p` snapped within `reach`: a connection point first, then a wire end, then onto a wire.
fn snap(s: &Session, p: Vec2, reach: f64, layers: &str) -> Snap {
    let Ok(d) = s.doc() else { return Snap::Free(p) };
    let Some(store) = d.space(&s.space()) else { return Snap::Free(p) };
    if let Ok(ix) = PinIndex::build(s)
        && let Some(pin) = ix.nearest(p, reach)
    {
        return Snap::Pin(pin.at, pin.dir);
    }
    let mut best_end: Option<(f64, Vec2)> = None;
    let mut best_on: Option<(f64, Vec2)> = None;
    for e in store.iter().filter(|e| wire_layer_matches(layers, &e.common.layer)) {
        let pts: Vec<Vec2> = match &e.kind {
            EntityKind::Line(l) => vec![l.a.xy(), l.b.xy()],
            EntityKind::LwPolyline(pl) => pl.vertices.iter().map(|v| v.p).collect(),
            _ => continue,
        };
        for q in [pts.first(), pts.last()].into_iter().flatten() {
            let dist = q.dist(p);
            if dist <= reach && best_end.is_none_or(|b| dist < b.0) {
                best_end = Some((dist, *q));
            }
        }
        for w in pts.windows(2) {
            let [a, b] = w else { continue };
            let l2 = (*b - *a).len2();
            if l2.is_nan() || l2 <= 0.0 {
                continue;
            }
            let t = ((p - *a).dot(*b - *a) / l2).clamp(0.0, 1.0);
            let foot = a.lerp(*b, t);
            let dist = foot.dist(p);
            if dist <= reach && best_on.is_none_or(|b| dist < b.0) {
                best_on = Some((dist, foot));
            }
        }
    }
    match (best_end, best_on) {
        (Some((_, q)), _) => Snap::WireEnd(q),
        (None, Some((_, q))) => Snap::Wire(q),
        _ => Snap::Free(p),
    }
}

/// Whether a wire on `layer` ends at `p`.
fn wire_end_at(s: &Session, layer: &str, p: Vec2) -> bool {
    let Ok(d) = s.doc() else { return false };
    let Some(store) = d.space(&s.space()) else { return false };
    store.iter().filter(|e| wire_layer_matches(layer, &e.common.layer)).any(|e| match &e.kind {
        EntityKind::Line(l) => l.a.xy().near(p, 1e-6) || l.b.xy().near(p, 1e-6),
        EntityKind::LwPolyline(pl) => [pl.vertices.first(), pl.vertices.last()].into_iter().flatten().any(|v| v.p.near(p, 1e-6)),
        _ => false,
    })
}

/// The corner points of a right-angled route from `a` to `b`.
fn route(a: Vec2, b: Vec2, vertical_first: bool) -> Vec<Vec2> {
    if (a.x - b.x).abs() < 1e-9 || (a.y - b.y).abs() < 1e-9 {
        return vec![a, b];
    }
    let corner = if vertical_first { Vec2::new(a.x, b.y) } else { Vec2::new(b.x, a.y) };
    vec![a, corner, b]
}

/// Add layer `name` with colour `color` (ACI) if the drawing hasn't got it.
fn ensure_layer(s: &mut Session, name: &str, color: u8) -> Result<()> {
    let d = s.doc_mut()?;
    if d.ensure_layer(name)
        && let Some(l) = d.layer_mut(name)
    {
        l.color = cadcraft_color::Color::Index(color);
    }
    Ok(())
}

/// Add a wire through `pts` on `layer`: a line for two points, else a polyline.
fn add_wire(s: &mut Session, pts: &[Vec2], layer: &str) -> Result<Handle> {
    ensure_layer(s, layer, 1)?;
    let mut common = s.current_common();
    common.layer = layer.to_string();
    let kind = match pts {
        [a, b] => EntityKind::Line(Line { a: Vec3::new(a.x, a.y, 0.0), b: Vec3::new(b.x, b.y, 0.0) }),
        _ => EntityKind::LwPolyline(LwPolyline {
            vertices: pts.iter().map(|p| PolyVertex { p: *p, ..Default::default() }).collect(),
            closed: false,
            const_width: 0.0,
            elevation: 0.0,
            plinegen: false,
        }),
    };
    let space = s.space();
    Ok(s.doc_mut()?.add(&space, common, kind)?)
}

/// A junction dot at `p`.
fn add_dot(s: &mut Session, p: Vec2) -> Result<()> {
    ensure_layer(s, JUNCTION_LAYER, 1)?;
    let mut common = s.current_common();
    common.layer = JUNCTION_LAYER.to_string();
    let space = s.space();
    // A filled dot: a donut, a closed polyline of two half-circles as wide as its radius.
    let r = 0.5;
    let dot = EntityKind::LwPolyline(LwPolyline {
        vertices: vec![
            PolyVertex { p: Vec2::new(p.x - r / 2.0, p.y), bulge: 1.0, ..Default::default() },
            PolyVertex { p: Vec2::new(p.x + r / 2.0, p.y), bulge: 1.0, ..Default::default() },
        ],
        closed: true,
        const_width: r,
        elevation: 0.0,
        plinegen: false,
    });
    s.doc_mut()?.add(&space, common, dot)?;
    Ok(())
}

/// The `layer` parameter, else the current layer when it is a wire type (EWIRETYPE), else WIRE.
fn layer_param(s: &Session, cmd: &str, p: &Value) -> Result<String> {
    let current = s.doc().map(|d| d.header.str("CLAYER", "0")).unwrap_or_default();
    let default = if wire_layer_matches(WIRE_LAYERS, &current) { current } else { WIRE_LAYER.to_string() };
    let l = str_param(p, "layer").map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).unwrap_or(default);
    if l.chars().count() > 255 { Err(bad(cmd, "`layer` is too long")) } else { Ok(l) }
}

fn run_wiredraw(s: &mut Session, p: &Value) -> Result<Value> {
    let r = run_wiredraw_inner(s, p)?;
    // Keep the project database in step with the drawing.
    super::sync::auto(s);
    Ok(r)
}

fn run_wiredraw_inner(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "ewire";
    let from = point_req(CMD, p, "from")?;
    let to = point_req(CMD, p, "to")?;
    let reach = f64_or(p, "snap", 3.0);
    if !(0.0..1e6).contains(&reach) {
        return Err(bad(CMD, "`snap` must be zero or more"));
    }
    let layer = layer_param(s, CMD, p)?;
    let layers = format!("{WIRE_LAYERS},{layer}");
    let a = snap(s, from, reach, &layers);
    let b = snap(s, to, reach, &layers);
    let (pa, pb) = (a.point(), b.point());
    if pa.near(pb, 1e-9) {
        return Err(bad(CMD, "the wire's ends are the same point"));
    }
    let vertical_first = match p.get("verticalFirst").and_then(Value::as_bool) {
        Some(v) => v,
        None => match a {
            Snap::Pin(_, dir) => matches!(dir, 1 | 4),
            _ => matches!(b, Snap::Pin(_, 2 | 8)),
        },
    };
    let pts = route(pa, pb, vertical_first);
    let h = add_wire(s, &pts, &layer)?;
    // Over a symbol the wire stops at its pins: cut it between them, pass by pass (each cut can
    // leave a tail that crosses the next symbol).
    let ix = PinIndex::build(s)?;
    let mut crossed: Vec<Handle> = Vec::new();
    // Each crossed symbol's pins, apart: cutting between the outermost pins of all of them at
    // once would take out the wire running from one symbol to the next.
    let mut per_symbol: Vec<Vec<Vec2>> = Vec::new();
    for w in pts.windows(2) {
        let [a0, b0] = w else { continue };
        for d in ix.crossed(*a0, *b0) {
            if !crossed.contains(&d.insert) {
                crossed.push(d.insert);
                per_symbol.push(d.pins.iter().filter_map(|i| ix.pins.get(*i)).map(|p| p.at).collect());
            }
        }
    }
    let mut cut = 0usize;
    for symbol_pins in per_symbol.iter().take(MAX_PASSES) {
        cut += super::symbols::break_wires(s, symbol_pins, &layer)?;
    }
    let pins: Vec<Vec2> = per_symbol.into_iter().flatten().collect();
    // Which pins the wire now ends on (its ends, and the pins it was cut at).
    let ix = PinIndex::build(s)?;
    let mut on_pins: Vec<Value> = Vec::new();
    for at in [pa, pb].into_iter().chain(pins.iter().copied()) {
        if let Some(pin) = ix.exact(at, 1e-6)
            && wire_end_at(s, &layer, at)
        {
            let v = json!({ "symbol": pin.insert.hex(), "pin": pin.number, "at": [pin.at.x, pin.at.y] });
            if !on_pins.contains(&v) {
                on_pins.push(v);
            }
        }
    }
    let mut junctions = 0;
    for sn in [a, b] {
        if let Snap::Wire(q) = sn {
            add_dot(s, q)?;
            junctions += 1;
        }
    }
    Ok(json!({
        "handle": h.hex(),
        "from": [pa.x, pa.y],
        "to": [pb.x, pb.y],
        "snappedFrom": a.label(),
        "snappedTo": b.label(),
        "junctions": junctions,
        "symbolsCrossed": crossed.len(),
        "cuts": cut,
        "pins": on_pins,
    }))
}

fn run_ladder(s: &mut Session, p: &Value) -> Result<Value> {
    let r = run_ladder_inner(s, p)?;
    // Keep the project database in step with the drawing.
    super::sync::auto(s);
    Ok(r)
}

fn run_ladder_inner(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "eladder";
    let top = p.get("top").and_then(Value::as_f64).filter(|v| v.is_finite()).ok_or_else(|| bad(CMD, "`top` (y of the upper rail) is required"))?;
    let bottom =
        p.get("bottom").and_then(Value::as_f64).filter(|v| v.is_finite()).ok_or_else(|| bad(CMD, "`bottom` (y of the lower rail) is required"))?;
    if top <= bottom {
        return Err(bad(CMD, "`top` must be above `bottom`"));
    }
    let x0 = f64_or(p, "from", 0.0);
    let x1 = f64_or(p, "to", x0 + 420.0);
    let spacing = f64_or(p, "spacing", 42.0);
    if x1 <= x0 || spacing <= 0.0 || (x1 - x0) / spacing > MAX_RUNGS as f64 {
        return Err(bad(CMD, format!("`to` must be right of `from`, `spacing` positive, and at most {MAX_RUNGS} rungs")));
    }
    let layer = layer_param(s, CMD, p)?;
    let rails = vec![
        add_wire(s, &[Vec2::new(x0, top), Vec2::new(x1, top)], &layer)?.hex(),
        add_wire(s, &[Vec2::new(x0, bottom), Vec2::new(x1, bottom)], &layer)?.hex(),
    ];
    let mut rungs = Vec::new();
    let mut x = x0 + spacing / 2.0;
    while x < x1 && rungs.len() < MAX_RUNGS {
        rungs.push(add_wire(s, &[Vec2::new(x, top), Vec2::new(x, bottom)], &layer)?.hex());
        add_dot(s, Vec2::new(x, top))?;
        add_dot(s, Vec2::new(x, bottom))?;
        x += spacing;
    }
    Ok(json!({ "rails": rails, "rungs": rungs }))
}

/// EWIRE at the command line: a start point, then the next points; each wire leg ends the
/// command when it lands on a pin or a wire, else the next leg starts where it ended.
#[derive(Default)]
struct WireM {
    last: Option<Vec2>,
}

impl Interactive for WireM {
    fn name(&self) -> &'static str {
        "EWIRE"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        let accept = Accept { point: true, number: false, text: false, select: false, enter: true };
        if self.last.is_none() { Prompt::new("Wire start point", accept) } else { Prompt::new("Wire end point", accept) }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let Input::Point(p) = i else {
            return Ok(if matches!(i, Input::Enter | Input::Cancel) { Step::Done } else { Step::Continue });
        };
        let reach = s.pixel_size() * 12.0;
        match self.last {
            None => {
                self.last = Some(snap(s, p, reach, WIRE_LAYERS).point());
                Ok(Step::Continue)
            }
            Some(a) => {
                let r = run_wiredraw(s, &json!({ "from": [a.x, a.y], "to": [p.x, p.y], "snap": reach.max(1e-9) }))?;
                if r["snappedTo"] == "pin" || r["snappedTo"] == "wire" || r["snappedTo"] == "wire end" {
                    return Ok(Step::Done);
                }
                let to = r["to"].as_array().and_then(|t| Some(Vec2::new(t.first()?.as_f64()?, t.get(1)?.as_f64()?)));
                self.last = to;
                Ok(Step::Continue)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn empty_rungs_are_removed_and_the_rails_part() {
        let mut s = Session::new();
        s.execute("eladder", &json!({ "top": 200, "bottom": 50, "to": 168 })).unwrap();
        // Four rungs at 21, 63, 105, 147; a coil on the first, a branch off the third.
        s.execute("esymbol", &json!({ "name": "IEC_COIL", "at": [21, 120] })).unwrap();
        let branch = s.execute("ewire", &json!({ "from": [105, 150], "to": [130, 150] })).unwrap();
        let dry = s.execute("eladder.clean", &json!({ "dryRun": true })).unwrap();
        assert_eq!(dry["removed"].as_array().unwrap().len(), 2, "the branched rung stays: {dry}");
        s.execute("erase", &json!({ "handles": [branch["handle"]] })).unwrap();
        let r = s.execute("eladder.clean", &json!({})).unwrap();
        assert_eq!(r["removed"].as_array().unwrap().len(), 3, "{r}");
        // The rails are two potentials now, joined only through the coil.
        let n = s.execute("ewirenum", &json!({})).unwrap();
        assert_eq!(n["potentials"], 2, "{n}");
        let again = s.execute("eladder.clean", &json!({})).unwrap();
        assert!(again["removed"].as_array().unwrap().is_empty());
        let dots = s.doc().unwrap().model.iter().filter(|e| e.common.layer == JUNCTION_LAYER).count();
        assert!(dots <= 3, "the removed rungs' dots are gone: {dots}");
    }

    #[test]
    fn wires_snap_to_pins_and_route_at_right_angles() {
        let mut s = Session::new();
        let k = s.execute("esymbol", &json!({"name": "IEC_COIL", "at": [50, 100]})).unwrap();
        let pb = s.execute("esymbol", &json!({"name": "IEC_PB_NO", "at": [80, 150]})).unwrap();
        let k_top = k["pins"][0].clone();
        let pb_bottom = pb["pins"][1].clone();
        // Clicks near the pins snap onto them; the coil's top pin faces up, so the wire leaves upwards.
        let r = s.execute("ewire", &json!({"from": [50.8, 105.5], "to": [79.0, 144.2]})).unwrap();
        assert_eq!(r["snappedFrom"], "pin");
        assert_eq!(r["snappedTo"], "pin");
        assert_eq!(r["from"], k_top);
        assert_eq!(r["to"], pb_bottom);
        let h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
        let Some(EntityKind::LwPolyline(pl)) = s.doc().unwrap().entity(h).map(|e| &e.kind) else { panic!("a polyline") };
        assert_eq!(pl.vertices.len(), 3);
        assert_eq!(pl.vertices[1].p, Vec2::new(50.0, 145.0), "vertical first, out of the coil's top");
        // Numbering sees one network joining the two pins.
        let w = s.execute("ewirenum", &json!({})).unwrap();
        assert_eq!(w["potentials"], 1);
    }

    #[test]
    fn t_junction_onto_a_wire_gets_a_dot() {
        let mut s = Session::new();
        s.execute("ewire", &json!({"from": [0, 100], "to": [200, 100]})).unwrap();
        let r = s.execute("ewire", &json!({"from": [60.5, 101.0], "to": [60.5, 20.0]})).unwrap();
        assert_eq!(r["snappedFrom"], "wire");
        assert_eq!(r["junctions"], 1);
        assert_eq!(r["from"], json!([60.5, 100.0]));
        assert_eq!(s.execute("ewirenum", &json!({})).unwrap()["potentials"], 1);
    }

    #[test]
    fn ladder_then_symbols_make_rungs() {
        let mut s = Session::new();
        let r = s.execute("eladder", &json!({"top": 270, "bottom": 30, "from": 0, "to": 210})).unwrap();
        assert_eq!(r["rungs"].as_array().unwrap().len(), 5);
        // One network: both rails joined through the rungs.
        assert_eq!(s.execute("ewirenum", &json!({})).unwrap()["potentials"], 1);
        // A coil on the second rung splits it: top rail side and bottom rail side.
        let c = s.execute("esymbol", &json!({"name": "IEC_COIL", "at": [63, 150]})).unwrap();
        assert_eq!(c["wiresCut"], 1);
        assert_eq!(s.execute("ewirenum", &json!({"mode": "all"})).unwrap()["potentials"], 1, "still joined by the other rungs");
        assert!(s.execute("eladder", &json!({"top": 0, "bottom": 10})).is_err());
        assert!(s.execute("eladder", &json!({"top": 10, "bottom": 0, "spacing": 0})).is_err());
        assert!(s.execute("eladder", &json!({"top": 10, "bottom": 0, "to": 1e9, "spacing": 1})).is_err());
    }

    #[test]
    fn a_wire_over_symbols_is_cut_at_their_pins_and_joins_them() {
        let mut s = Session::new();
        let a = s.active;
        if let Some(d) = s.docs.get_mut(a) {
            d.title = "ewire-through".into();
        }
        s.execute("esymbol", &json!({"name": "IEC_LAMP", "at": [50, 160]})).unwrap();
        s.execute("esymbol", &json!({"name": "IEC_COIL", "at": [50, 100]})).unwrap();
        let r = s.execute("ewire", &json!({"from": [50, 250], "to": [50, 0]})).unwrap();
        assert_eq!(r["symbolsCrossed"], 2, "{r}");
        assert_eq!(r["cuts"], 2);
        let numbers: Vec<&str> = r["pins"].as_array().unwrap().iter().filter_map(|p| p["pin"].as_str()).collect();
        for n in ["X1", "X2", "A1", "A2"] {
            assert!(numbers.contains(&n), "{n} in {numbers:?}");
        }
        let wires = s.doc().unwrap().model.iter().filter(|e| e.common.layer == "WIRE").count();
        assert_eq!(wires, 3, "top, between the symbols, bottom");
        // The graph has the lamp's X2 wired to the coil's A1 already: no manual update.
        let c = s.execute("edb.query", &json!({"what": "connections"})).unwrap();
        let pairs: Vec<(String, String)> = c
            .as_array()
            .unwrap()
            .iter()
            .map(|c| (c["from"]["pin"].as_str().unwrap().to_string(), c["to"]["pin"].as_str().unwrap().to_string()))
            .collect();
        assert!(pairs.iter().any(|(a, b)| (a == "X2" && b == "A1") || (a == "A1" && b == "X2")), "{pairs:?}");
    }

    #[test]
    fn interactive_wire_ends_on_a_pin() {
        let mut s = Session::new();
        s.execute("esymbol", &json!({"name": "IEC_LAMP", "at": [100, 100]})).unwrap();
        s.start("ewire").unwrap();
        s.input(Input::Point(Vec2::new(0.0, 105.0))).unwrap();
        s.input(Input::Point(Vec2::new(100.0, 105.0))).unwrap();
        assert!(s.running.is_none(), "landing on the lamp's pin ends the command");
        assert!(s.execute("ewire", &json!({"from": [0, 0], "to": [0, 0]})).is_err());
        assert!(s.execute("ewire", &json!({"from": [0, 0]})).is_err());
    }
}
