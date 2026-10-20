//! Drawing → AutoCAD Release 12 DXF (`$ACADVER` AC1009), the format of legacy readers: the
//! Italian cadastre's DOCFA, CNC and plotter front ends, old viewers. R12 has no handles,
//! classes, objects, layouts, LWPOLYLINE, MTEXT, ELLIPSE, SPLINE, HATCH, MLINE, LEADER, IMAGE or
//! true colours (DXF Reference, "DXF R12 format"), so the file is a *reduced copy* of the
//! drawing: old-style POLYLINE/VERTEX/SEQEND for polylines, TEXT lines for multiline text,
//! polylines for curves, lines and text for dimensions, tables and multilines, hatch pattern
//! lines for hatches, and model space only. [`notes`] says what became what.
//!
//! The DOCFA profile ([`R12Options::docfa`]) follows what DOCFA's import takes (its guides and
//! practitioners' experience, see `docs/docfa.md`): block references exploded, hatches, images
//! and wipeouts left out, objects on layers that are off or frozen left out, every layer
//! continuous and white, colours only on the surface polygons of layer [`DOCFA_LAYER`] (DXF group
//! 62, DOCFA's area classes), text transliterated to ASCII, and the drawing scaled to metres
//! with [`R12Options::scale`].

use std::collections::{BTreeMap, BTreeSet};

use cadcraft_color::Color;
use cadcraft_doc::*;
use cadcraft_geom::{Bounds2, Mat3, Polyline, Segment, Vec2, Vec3};

/// The layer DOCFA reads surface polygons (closed polylines, one per area class colour) from.
pub const DOCFA_LAYER: &str = "DOCFA_POLIGONI";

/// R12 names (layers, linetypes, styles, blocks) have at most this many characters.
const MAX_NAME: usize = 31;
/// Written entities are capped: a drawing beyond this is reported, not written whole.
const MAX_ENTITIES: usize = 2_000_000;
/// Nested block references deeper than this are left out (cyclic definitions).
const MAX_DEPTH: usize = cadcraft_doc::MAX_BLOCK_DEPTH;
/// Hatch pattern runs per hatch; denser hatches are cut short.
const MAX_HATCH_RUNS: usize = 200_000;
/// AutoCAD's line spacing: baselines of single-spaced multiline text are 5/3 of the height apart.
const LINE_SPACING: f64 = 5.0 / 3.0;

/// How the R12 file is written.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct R12Options {
    /// The DOCFA profile (see the module doc).
    pub docfa: bool,
    /// Every coordinate and size is multiplied by this (1: the drawing's units). DOCFA takes
    /// metres: 0.001 for a drawing in millimetres, 0.01 for one in centimetres.
    pub scale: f64,
}

impl Default for R12Options {
    fn default() -> Self {
        R12Options { docfa: false, scale: 1.0 }
    }
}

/// What the file keeps of the drawing: conversions and omissions, counted by kind, and the
/// DOCFA checks. [`Report::lines`] words it for the user.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    /// "ELLIPSE → POLYLINE" → how many.
    pub converted: BTreeMap<&'static str, usize>,
    /// A DXF type left out → how many.
    pub dropped: BTreeMap<String, usize>,
    /// DOCFA checks and other warnings, one sentence each.
    pub warnings: Vec<String>,
}

impl Report {
    fn convert(&mut self, what: &'static str) {
        let n = self.converted.entry(what).or_insert(0);
        *n = n.saturating_add(1);
    }
    fn leave_out(&mut self, what: &str) {
        let n = self.dropped.entry(what.to_string()).or_insert(0);
        *n = n.saturating_add(1);
    }
    /// The report as sentences: conversions ("2 ELLIPSE → POLYLINE"), omissions ("1 IMAGE left
    /// out") and the warnings. Empty when the file keeps everything.
    pub fn lines(&self) -> Vec<String> {
        let mut out: Vec<String> = self.converted.iter().map(|(what, n)| format!("{n} {what}")).collect();
        out.extend(self.dropped.iter().map(|(what, n)| format!("{n} {what} left out")));
        out.extend(self.warnings.iter().cloned());
        out
    }
}

/// The drawing as R12 ASCII DXF. Non-ASCII text is written as `\U+XXXX` (the DOCFA profile
/// transliterates it to ASCII first).
pub fn write(d: &Drawing, o: &R12Options) -> String {
    cadcraft_dxf::write_ascii_as(&tags(d, o), false)
}

/// The drawing's R12 DXF tags.
pub fn tags(d: &Drawing, o: &R12Options) -> Vec<cadcraft_dxf::Tag> {
    build(d, o).0
}

/// What writing the drawing as R12 with `o` does to it (the same conversion as [`write`]).
pub fn notes(d: &Drawing, o: &R12Options) -> Vec<String> {
    let committed = d.committed();
    let (_, report) = convert_drawing(&committed, o);
    report.lines()
}

/// The tags and the report.
fn build(d: &Drawing, o: &R12Options) -> (Vec<cadcraft_dxf::Tag>, Report) {
    let committed = d.committed();
    let d: &Drawing = &committed;
    let (file, report) = convert_drawing(d, o);
    (emit(d, o, &file), report)
}

// ---------------------------------------------------------------------------------------------
// The reduced drawing
// ---------------------------------------------------------------------------------------------

/// An entity R12 has.
#[derive(Clone, Debug)]
enum Geo {
    Line(Vec3, Vec3),
    Point(Vec3),
    Circle(Vec3, f64),
    /// Centre, radius, start and end angles (radians, counter-clockwise).
    Arc(Vec3, f64, f64, f64),
    /// POLYLINE: vertices (point, bulge, start width, end width), closed, 3D, constant width.
    Poly {
        verts: Vec<(Vec3, f64, f64, f64)>,
        closed: bool,
        three_d: bool,
        width: f64,
    },
    Text(Box<Text>),
    Solid([Vec3; 4]),
    Trace([Vec3; 4]),
    Face([Vec3; 4], u8),
    Insert(Box<Insert>),
    AttDef(Box<Attrib>),
}

/// An entity of the file with the properties R12 keeps.
#[derive(Clone, Debug)]
struct Ent {
    layer: String,
    /// The colour written (group 62): an ACI, 0 for ByBlock; `None` leaves it ByLayer.
    color: Option<i64>,
    /// The linetype written (group 6); `None` leaves it ByLayer.
    linetype: Option<String>,
    geo: Geo,
}

/// A block definition written to the BLOCKS section.
#[derive(Clone, Debug)]
struct BlockDef {
    name: String,
    base: Vec3,
    ents: Vec<Ent>,
}

/// The reduced drawing.
#[derive(Default)]
struct File {
    ents: Vec<Ent>,
    blocks: Vec<BlockDef>,
    /// Original layer, linetype and text style names used, upper case.
    layers: BTreeSet<String>,
    linetypes: BTreeSet<String>,
    styles: BTreeSet<String>,
}

struct Cx<'a> {
    d: &'a Drawing,
    o: &'a R12Options,
    report: Report,
    /// Written entities so far (bounded by [`MAX_ENTITIES`]).
    count: usize,
    /// Block definitions still to convert (plain R12: INSERTs are kept).
    needed_blocks: Vec<String>,
    done_blocks: BTreeSet<String>,
}

fn convert_drawing(d: &Drawing, o: &R12Options) -> (File, Report) {
    let mut cx = Cx { d, o, report: Report::default(), count: 0, needed_blocks: Vec::new(), done_blocks: BTreeSet::new() };
    let mut file = File::default();
    let base = if (o.scale - 1.0).abs() > 1e-15 { Some(Mat3::scale(o.scale, o.scale)) } else { None };
    for e in d.model.iter() {
        convert(&mut cx, &mut file.ents, e, base.as_ref(), 0, false);
    }
    // Paper space has no place in the file (R12's single paper space is not written).
    let layouts: usize = d.layouts.iter().map(|l| l.entities.len()).sum();
    if layouts > 0 {
        cx.report.warnings.push(format!("{layouts} paper-space objects (layouts) are not written: R12 keeps model space"));
    }
    // Block definitions the kept references need, and the ones they need in turn. They are
    // written in block units: the references' scale (transformed above) carries the file's.
    let mut depth = 0;
    while let Some(name) = cx.needed_blocks.pop() {
        let key = name.to_ascii_uppercase();
        if !cx.done_blocks.insert(key) {
            continue;
        }
        depth += 1;
        if depth > MAX_DEPTH.saturating_mul(64) {
            break;
        }
        let Some(b) = d.block(&name) else { continue };
        let mut ents = Vec::new();
        for e in b.entities.iter() {
            convert(&mut cx, &mut ents, e, None, 1, true);
        }
        file.blocks.push(BlockDef { name: b.name.clone(), base: b.base, ents });
    }
    for e in file.ents.iter().chain(file.blocks.iter().flat_map(|b| b.ents.iter())) {
        file.layers.insert(e.layer.to_ascii_uppercase());
        if let Some(lt) = &e.linetype {
            file.linetypes.insert(lt.to_ascii_uppercase());
        }
        match &e.geo {
            Geo::Text(t) => {
                file.styles.insert(t.style.to_ascii_uppercase());
            }
            Geo::AttDef(a) => {
                file.styles.insert(a.text.style.to_ascii_uppercase());
            }
            Geo::Insert(i) => file.styles.extend(i.attribs.iter().map(|a| a.text.style.to_ascii_uppercase())),
            _ => {}
        }
    }
    // Every layer of the drawing is written, used or not, as AutoCAD does.
    file.layers.extend(d.layers.iter().map(|l| l.name.to_ascii_uppercase()));
    if o.docfa {
        docfa_checks(&mut cx.report, d, &file);
    }
    (file, cx.report)
}

/// Convert one entity (and what it expands to) into `out`, transformed by `m` (block
/// references, the scale), `depth` block levels down. `in_block`: into a block definition
/// (plain R12), where attribute definitions stay.
fn convert(cx: &mut Cx, out: &mut Vec<Ent>, e: &Entity, m: Option<&Mat3>, depth: usize, in_block: bool) {
    if cx.count >= MAX_ENTITIES {
        if cx.count == MAX_ENTITIES {
            cx.report.warnings.push(format!("the file is cut at {MAX_ENTITIES} objects"));
            cx.count = cx.count.saturating_add(1);
        }
        return;
    }
    if depth > MAX_DEPTH {
        cx.report.leave_out("INSERT nested too deep");
        return;
    }
    if !e.common.visible {
        return;
    }
    if cx.o.docfa && !in_block && (!cx.d.is_visible(e) || cx.d.layer(&e.common.layer).is_some_and(|l| !l.visible())) {
        cx.report.leave_out("object on a layer that is off or frozen");
        return;
    }
    // Block contents follow the reference's transform; the scale applies to everything. The
    // original stays for what is drawn from a style (dimensions, tables): their parts are made
    // at the drawing's size and transformed after.
    let original = e;
    let moved;
    let e: &Entity = match m {
        Some(m) => {
            let mut c = e.clone();
            c.kind.transform(m);
            moved = c;
            &moved
        }
        None => e,
    };
    let c = &e.common;
    let push = |cx: &mut Cx, out: &mut Vec<Ent>, geo: Geo| {
        if !finite(&geo) {
            cx.report.leave_out("object with non-finite coordinates");
            return;
        }
        cx.count = cx.count.saturating_add(1);
        out.push(Ent { layer: c.layer.clone(), color: entity_color(cx, c), linetype: entity_linetype(cx, c), geo });
    };
    let d = cx.d;
    match &e.kind {
        EntityKind::Line(l) => push(cx, out, Geo::Line(l.a, l.b)),
        EntityKind::Point(p) => push(cx, out, Geo::Point(p.p)),
        EntityKind::Circle(ci) => push(cx, out, Geo::Circle(ci.center, ci.radius)),
        EntityKind::Arc(a) => push(cx, out, Geo::Arc(a.center, a.radius, a.start, a.end)),
        EntityKind::Ellipse(el) => {
            let ge = cadcraft_geom::Ellipse { center: el.center.xy(), major: el.major.xy(), ratio: el.ratio, start: el.start, end: el.end };
            let mut pts = Vec::new();
            ge.tessellate(tolerance(el.major.len()), &mut pts);
            cx.report.convert("ELLIPSE → POLYLINE");
            push(cx, out, points_poly(&pts, ge.is_full(), el.center.z));
        }
        EntityKind::LwPolyline(p) => {
            let verts = p.vertices.iter().map(|v| (v.p.to3(p.elevation), v.bulge, v.start_width, v.end_width)).collect();
            push(cx, out, Geo::Poly { verts, closed: p.closed, three_d: false, width: p.const_width.max(0.0) });
        }
        EntityKind::Polyline3d(p) => {
            let verts = p.points.iter().map(|q| (*q, 0.0, 0.0, 0.0)).collect();
            push(cx, out, Geo::Poly { verts, closed: p.closed, three_d: true, width: 0.0 });
        }
        EntityKind::Spline(s) => {
            let pts = s.tessellate(tolerance(extent(&Bounds2::from_points(s.control.iter().copied()))));
            cx.report.convert("SPLINE → POLYLINE");
            push(cx, out, points_poly(&pts, s.closed, 0.0));
        }
        EntityKind::Helix(h) => {
            let s = h.spline();
            let pts = s.tessellate(tolerance(extent(&Bounds2::from_points(s.control.iter().copied()))));
            cx.report.convert("HELIX → POLYLINE");
            push(cx, out, points_poly(&pts, false, 0.0));
        }
        EntityKind::Ray(_) => cx.report.leave_out("RAY"),
        EntityKind::XLine(_) => cx.report.leave_out("XLINE"),
        EntityKind::Text(t) => {
            for line in text_lines(cx, d, t) {
                push(cx, out, Geo::Text(Box::new(line)));
            }
        }
        EntityKind::MText(t) => {
            cx.report.convert("MTEXT → TEXT");
            for line in mtext_lines(cx, t) {
                push(cx, out, Geo::Text(Box::new(line)));
            }
        }
        EntityKind::AttDef(a) if in_block => {
            let mut a = a.clone();
            a.text.value = docfa_text(cx, &a.text.value);
            push(cx, out, Geo::AttDef(Box::new(a)));
        }
        EntityKind::AttDef(_) => {}
        EntityKind::Insert(i) => convert_insert(cx, out, e, i, depth, in_block),
        EntityKind::Dimension(_) => {
            cx.report.convert("DIMENSION → lines and text");
            // The dimension as drawn at the drawing's size (its style's text height, arrows and
            // offsets), then moved, rotated and scaled with the rest.
            if let EntityKind::Dimension(dm) = &original.kind {
                for part in crate::dxf_write::dim_block_entities(d, dm, &c.layer) {
                    convert(cx, out, &part, m, depth, in_block);
                }
            }
        }
        EntityKind::Table(_) => {
            cx.report.convert("TABLE → lines and text");
            // The table's block is relative to its own origin: it goes to the insertion point
            // first (the ACAD_TABLE reference does that in the full formats), then with the rest.
            if let EntityKind::Table(t) = &original.kind {
                let to_insert = Mat3::translate(t.insert.xy());
                let placed = match m {
                    Some(m) => to_insert.then(*m),
                    None => to_insert,
                };
                for part in crate::dxf_write::table_block_entities(d, t, &c.layer) {
                    convert(cx, out, &part, Some(&placed), depth, in_block);
                }
            }
        }
        EntityKind::MLine(ml) => {
            cx.report.convert("MLINE → lines and arcs");
            for s in ml.segments() {
                push(cx, out, segment_geo(&s));
            }
        }
        EntityKind::Leader(l) => {
            cx.report.convert("LEADER → POLYLINE");
            let verts = l.vertices.iter().map(|q| (*q, 0.0, 0.0, 0.0)).collect();
            push(cx, out, Geo::Poly { verts, closed: false, three_d: false, width: 0.0 });
        }
        EntityKind::MLeader(ml) => {
            cx.report.convert("MULTILEADER → polylines and text");
            for path in ml.leader_paths() {
                push(cx, out, points_poly(&path, false, 0.0));
            }
            if let Some(t) = &ml.text {
                for line in mtext_lines(cx, t) {
                    push(cx, out, Geo::Text(Box::new(line)));
                }
            }
            if let Some(b) = &ml.block {
                convert_insert(cx, out, e, b, depth, in_block);
            }
        }
        EntityKind::Hatch(_) if cx.o.docfa => cx.report.leave_out("HATCH (DOCFA takes no hatching)"),
        EntityKind::Hatch(h) => convert_hatch(cx, out, h, &push),
        EntityKind::Solid(s) => push(cx, out, Geo::Solid(s.corners)),
        EntityKind::Trace(s) => push(cx, out, Geo::Trace(s.corners)),
        EntityKind::Face3d(f) => push(cx, out, Geo::Face(f.corners, f.hidden_edges)),
        EntityKind::Region(r) => {
            cx.report.convert("REGION → POLYLINE");
            for p in r.polylines() {
                let verts = p.vertices.iter().map(|v| (v.p.to3(0.0), v.bulge, 0.0, 0.0)).collect();
                push(cx, out, Geo::Poly { verts, closed: true, three_d: false, width: 0.0 });
            }
        }
        EntityKind::Viewport(_) => cx.report.leave_out("VIEWPORT"),
        EntityKind::Image(_) => cx.report.leave_out("IMAGE"),
        EntityKind::Wipeout(_) => cx.report.leave_out("WIPEOUT"),
        EntityKind::Tolerance(_) => cx.report.leave_out("TOLERANCE"),
        EntityKind::Unknown(u) => cx.report.leave_out(&u.dxf_type),
    }
}

/// A block reference: kept as INSERT in plain R12 (the definition is written too), exploded
/// into its contents in the DOCFA profile and for blocks R12 can't reference (anonymous ones,
/// external references, missing definitions).
fn convert_insert(cx: &mut Cx, out: &mut Vec<Ent>, e: &Entity, i: &Insert, depth: usize, in_block: bool) {
    let d = cx.d;
    let block = d.block(&i.block);
    let referenceable = block.is_some_and(|b| !b.anonymous && b.xref_path.is_none() && !b.name.starts_with('*'));
    if !cx.o.docfa && referenceable {
        let mut kept = i.clone();
        for a in &mut kept.attribs {
            a.text.value = docfa_text(cx, &a.text.value);
        }
        cx.needed_blocks.push(i.block.clone());
        if finite_insert(&kept) {
            cx.count = cx.count.saturating_add(1);
            out.push(Ent {
                layer: e.common.layer.clone(),
                color: entity_color(cx, &e.common),
                linetype: entity_linetype(cx, &e.common),
                geo: Geo::Insert(Box::new(kept)),
            });
        } else {
            cx.report.leave_out("INSERT with non-finite coordinates");
        }
        return;
    }
    let Some(b) = block else {
        cx.report.leave_out("INSERT of a missing block");
        return;
    };
    if b.xref_path.is_some() {
        cx.report.leave_out("external reference (XREF)");
        return;
    }
    cx.report.convert("INSERT exploded");
    // `i` is in world coordinates already (the entity was transformed); the block's base point
    // is in the block's own units, which `Insert::transform` maps to the world.
    let cols = i.cols.clamp(1, 1000);
    let rows = i.rows.clamp(1, 1000);
    let cells = usize::try_from(cols).unwrap_or(1).saturating_mul(usize::try_from(rows).unwrap_or(1));
    if b.entities.len().saturating_mul(cells) > MAX_ENTITIES {
        cx.report.leave_out("INSERT array too large to explode");
        return;
    }
    let ins = i.transform(b.base.xy());
    for r in 0..rows {
        for cc in 0..cols {
            let off = Vec2::new(f64::from(cc) * i.col_spacing, f64::from(r) * i.row_spacing).rotate(i.rotation);
            let cell = if cc == 0 && r == 0 { ins } else { Mat3::translate(off).then_before(ins) };
            for be in b.entities.iter() {
                // Attribute definitions show through the reference's attributes, written below.
                if matches!(be.kind, EntityKind::AttDef(_)) {
                    continue;
                }
                convert(cx, out, be, Some(&cell), depth + 1, in_block);
            }
        }
    }
    for a in &i.attribs {
        if a.invisible || a.text.value.trim().is_empty() {
            continue;
        }
        let part = Entity {
            handle: Handle(0),
            common: Common { layer: e.common.layer.clone(), ..Common::default() },
            kind: EntityKind::Text(a.text.clone()),
        };
        convert(cx, out, &part, None, depth, in_block);
    }
}

/// A hatch as its pattern's line runs (a solid fill: its boundary polylines).
fn convert_hatch(cx: &mut Cx, out: &mut Vec<Ent>, h: &Hatch, push: &dyn Fn(&mut Cx, &mut Vec<Ent>, Geo)) {
    let loops: Vec<Vec<Vec2>> = h
        .loops
        .iter()
        .map(|l| {
            let p = Polyline { vertices: l.vertices.clone(), closed: true };
            p.tessellate(tolerance(extent(&p.bounds())))
        })
        .collect();
    if h.solid || h.gradient.is_some() {
        cx.report.convert("solid HATCH → boundary POLYLINE");
        for l in &h.loops {
            let verts = l.vertices.iter().map(|v| (v.p.to3(h.elevation), v.bulge, 0.0, 0.0)).collect();
            push(cx, out, Geo::Poly { verts, closed: true, three_d: false, width: 0.0 });
        }
        return;
    }
    cx.report.convert("HATCH → pattern lines");
    let runs = cadcraft_render::hatch_pattern_lines(cx.d, h, &loops, 0.0, None);
    if runs.len() > MAX_HATCH_RUNS {
        cx.report.warnings.push(format!("a hatch's pattern is cut at {MAX_HATCH_RUNS} lines"));
    }
    for run in runs.iter().take(MAX_HATCH_RUNS) {
        match run.as_slice() {
            [] => {}
            [p] => push(cx, out, Geo::Point(p.to3(h.elevation))),
            [a, .., b] if run.len() == 2 => push(cx, out, Geo::Line(a.to3(h.elevation), b.to3(h.elevation))),
            _ => push(cx, out, points_poly(run, false, h.elevation)),
        }
    }
}

/// A line or arc segment.
fn segment_geo(s: &Segment) -> Geo {
    match s {
        Segment::Line(l) => Geo::Line(l.a.to3(0.0), l.b.to3(0.0)),
        Segment::Arc { arc, .. } => Geo::Arc(arc.center.to3(0.0), arc.radius, arc.start, arc.end),
    }
}

/// A POLYLINE through `pts` at `z`.
fn points_poly(pts: &[Vec2], closed: bool, z: f64) -> Geo {
    let mut verts: Vec<(Vec3, f64, f64, f64)> = pts.iter().map(|p| (p.to3(z), 0.0, 0.0, 0.0)).collect();
    // A closed curve's tessellation repeats its first point; the closing segment replaces it.
    if closed && verts.len() > 2 && verts.first().map(|v| v.0) == verts.last().map(|v| v.0) {
        verts.pop();
    }
    Geo::Poly { verts, closed, three_d: false, width: 0.0 }
}

/// The diagonal of `b` (0 for an empty bounds).
fn extent(b: &Bounds2) -> f64 {
    if b.is_empty() { 0.0 } else { (b.max - b.min).len() }
}

/// The chord tolerance for a curve of about `size` drawing units: fine enough for plots at
/// any scale, bounded for tiny or huge sizes.
fn tolerance(size: f64) -> f64 {
    if size.is_finite() && size > 0.0 { (size * 1e-3).max(1e-9) } else { 1e-4 }
}

/// Whether every coordinate of `g` is finite (hostile files carry NaN and infinities).
fn finite(g: &Geo) -> bool {
    let ok = |p: &Vec3| p.x.is_finite() && p.y.is_finite() && p.z.is_finite();
    match g {
        Geo::Line(a, b) => ok(a) && ok(b),
        Geo::Point(p) => ok(p),
        Geo::Circle(c, r) => ok(c) && r.is_finite(),
        Geo::Arc(c, r, s, e) => ok(c) && r.is_finite() && s.is_finite() && e.is_finite(),
        Geo::Poly { verts, width, .. } => {
            width.is_finite() && verts.iter().all(|(p, b, sw, ew)| ok(p) && b.is_finite() && sw.is_finite() && ew.is_finite())
        }
        Geo::Text(t) => ok(&t.insert) && t.align_pt.is_none_or(|a| ok(&a)) && t.height.is_finite() && t.rotation.is_finite(),
        Geo::Solid(c) | Geo::Trace(c) | Geo::Face(c, _) => c.iter().all(ok),
        Geo::Insert(i) => finite_insert(i),
        Geo::AttDef(a) => ok(&a.text.insert) && a.text.height.is_finite(),
    }
}

fn finite_insert(i: &Insert) -> bool {
    let ok = |p: &Vec3| p.x.is_finite() && p.y.is_finite() && p.z.is_finite();
    ok(&i.insert) && ok(&i.scale) && i.rotation.is_finite() && i.col_spacing.is_finite() && i.row_spacing.is_finite()
}

/// The colour group of an entity: in the DOCFA profile only the surface polygons of
/// [`DOCFA_LAYER`] carry one (their resolved ACI, DOCFA's area class), everything else is
/// ByLayer (DOCFA draws it in its own colour); otherwise the entity's own colour, true colours
/// as the nearest ACI.
fn entity_color(cx: &Cx, c: &Common) -> Option<i64> {
    if cx.o.docfa {
        if !c.layer.eq_ignore_ascii_case(DOCFA_LAYER) {
            return None;
        }
        let aci = match c.color {
            Color::ByLayer => cx.d.layer(&c.layer).map_or(7, |l| l.color.to_aci()),
            Color::ByBlock => 7,
            other => other.to_aci(),
        };
        return Some(i64::from(aci).clamp(1, 255));
    }
    match c.color {
        Color::ByLayer => None,
        Color::ByBlock => Some(0),
        other => Some(i64::from(other.to_aci()).clamp(1, 255)),
    }
}

/// The linetype group of an entity (DOCFA: everything continuous).
fn entity_linetype(cx: &Cx, c: &Common) -> Option<String> {
    let lt = c.linetype.trim();
    if cx.o.docfa || lt.is_empty() || lt.eq_ignore_ascii_case("bylayer") {
        return None;
    }
    Some(lt.to_string())
}

/// A text entity as one TEXT per line (a multiline attribute's value holds newlines).
fn text_lines(cx: &Cx, d: &Drawing, t: &Text) -> Vec<Text> {
    let value = docfa_text(cx, &t.value);
    if !value.contains('\n') {
        let mut one = t.clone();
        one.value = value;
        one.insert = crate::dxf_write::text_insert(d, &one);
        return vec![one];
    }
    let step = t.height.abs() * LINE_SPACING;
    let mut out = Vec::new();
    for (k, line) in value.split('\n').enumerate() {
        let mut one = t.clone();
        one.value = line.to_string();
        let off = Vec2::new(0.0, -step * k as f64).rotate(t.rotation);
        one.insert = t.insert + off.to3(0.0);
        one.align_pt = t.align_pt.map(|a| a + off.to3(0.0));
        one.insert = crate::dxf_write::text_insert(d, &one);
        out.push(one);
    }
    out
}

/// Multiline text as TEXT lines placed by its attachment point, each line justified like its
/// column of the attachment (left, centre, right). Inline formatting is dropped.
fn mtext_lines(cx: &Cx, t: &MText) -> Vec<Text> {
    let plain = docfa_text(cx, &cadcraft_fonts::plain_mtext(&t.contents));
    let lines: Vec<&str> = plain.split('\n').collect();
    let n = lines.len().max(1);
    let h = t.height.abs();
    let spacing = if t.line_spacing.is_finite() && t.line_spacing > 0.0 { t.line_spacing } else { 1.0 };
    let step = h * LINE_SPACING * spacing;
    let total = step * (n - 1) as f64 + h;
    let (row, col) = ((t.attach.clamp(1, 9) - 1) / 3, (t.attach.clamp(1, 9) - 1) % 3);
    // The top of the first line in the text's own frame (origin at the attachment point).
    let top = match row {
        0 => 0.0,
        1 => total / 2.0,
        _ => total,
    };
    let (halign, valign) = match col {
        0 => (HAlign::Left, VAlign::Baseline),
        1 => (HAlign::Center, VAlign::Baseline),
        _ => (HAlign::Right, VAlign::Baseline),
    };
    lines
        .iter()
        .enumerate()
        .map(|(k, line)| {
            let local = Vec2::new(0.0, top - h - step * k as f64).rotate(t.rotation);
            let p = t.insert + local.to3(0.0);
            Text {
                insert: p,
                align_pt: if halign == HAlign::Left { None } else { Some(p) },
                height: h,
                value: line.to_string(),
                rotation: t.rotation,
                width_factor: 1.0,
                oblique: 0.0,
                style: t.style.clone(),
                halign,
                valign,
            }
        })
        .collect()
}

/// Text as the profile writes it: the DOCFA profile transliterates it to ASCII (old readers
/// show `\U+00E8` for `è`); plain R12 keeps it (the ASCII writer escapes it).
fn docfa_text(cx: &Cx, s: &str) -> String {
    if cx.o.docfa { transliterate(s) } else { s.to_string() }
}

/// Latin letters with diacritics as their base letters, the degree sign as the `%%d` TEXT
/// control code (every AutoCAD font has it), other non-ASCII characters as `?`.
pub fn transliterate(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii() {
            out.push(c);
            continue;
        }
        let mapped: &str = match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' => "a",
            'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' | 'Ā' => "A",
            'è' | 'é' | 'ê' | 'ë' | 'ē' => "e",
            'È' | 'É' | 'Ê' | 'Ë' | 'Ē' => "E",
            'ì' | 'í' | 'î' | 'ï' | 'ī' => "i",
            'Ì' | 'Í' | 'Î' | 'Ï' | 'Ī' => "I",
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' => "o",
            'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' | 'Ō' => "O",
            'ù' | 'ú' | 'û' | 'ü' | 'ū' => "u",
            'Ù' | 'Ú' | 'Û' | 'Ü' | 'Ū' => "U",
            'ç' => "c",
            'Ç' => "C",
            'ñ' => "n",
            'Ñ' => "N",
            'ß' => "ss",
            'æ' => "ae",
            'Æ' => "AE",
            'œ' => "oe",
            'Œ' => "OE",
            '°' => "%%d",
            '±' => "%%p",
            'Ø' | '⌀' => "%%c",
            '²' => "2",
            '³' => "3",
            '€' => "EUR",
            '£' => "GBP",
            '‘' | '’' | '‚' => "'",
            '“' | '”' | '„' => "\"",
            '–' | '—' => "-",
            '…' => "...",
            '\u{a0}' => " ",
            _ => "?",
        };
        out.push_str(mapped);
    }
    out
}

/// What DOCFA needs of the file: surface polygons closed and on their layer, the sheet at the
/// origin.
fn docfa_checks(r: &mut Report, d: &Drawing, f: &File) {
    let mut polygons = 0usize;
    let mut open = 0usize;
    let mut other = 0usize;
    let mut odd_color = 0usize;
    for e in &f.ents {
        if !e.layer.eq_ignore_ascii_case(DOCFA_LAYER) {
            continue;
        }
        match &e.geo {
            Geo::Poly { closed, .. } => {
                polygons += 1;
                if !closed {
                    open += 1;
                }
                if !matches!(e.color, Some(1..=8)) {
                    odd_color += 1;
                }
            }
            Geo::Text(_) => {}
            _ => other += 1,
        }
    }
    if d.layers.iter().any(|l| l.name.eq_ignore_ascii_case(DOCFA_LAYER)) {
        if polygons == 0 {
            r.warnings.push(format!("layer {DOCFA_LAYER} has no polylines: DOCFA makes no surface polygons from this file"));
        } else {
            r.warnings.push(format!("{polygons} surface polygons on {DOCFA_LAYER}"));
        }
    }
    if open > 0 {
        r.warnings.push(format!("{open} polylines on {DOCFA_LAYER} are not closed: DOCFA needs closed polygons"));
    }
    if other > 0 {
        r.warnings.push(format!("{other} objects on {DOCFA_LAYER} are not polylines: DOCFA reads polygons from closed polylines only"));
    }
    if odd_color > 0 {
        r.warnings.push(format!("{odd_color} polygons on {DOCFA_LAYER} have a colour outside 1–8 (DOCFA's area classes A, B, C, D, E, F, G, A2)"));
    }
    let ext = file_extents(f);
    if !ext.is_empty() && (ext.min.x < -1e-9 || ext.min.y < -1e-9) {
        r.warnings.push("the drawing extends left of or below the origin: DOCFA expects the sheet frame's lower-left corner at 0,0".into());
    }
    if extent(&ext) > 1000.0 {
        r.warnings
            .push("the drawing spans more than 1000 units: DOCFA takes metres (use `scale` for a drawing in millimetres or centimetres)".into());
    }
}

/// The written entities' bounds (model space; block contents through their references are
/// approximated by the insertion points).
fn file_extents(f: &File) -> Bounds2 {
    let mut b = Bounds2::EMPTY;
    let mut add = |p: Vec3| b = b.union(&Bounds2::new(p.xy(), p.xy()));
    for e in &f.ents {
        match &e.geo {
            Geo::Line(a, c) => {
                add(*a);
                add(*c);
            }
            Geo::Point(p) => add(*p),
            Geo::Circle(c, r) => {
                add(*c - Vec3::new(*r, *r, 0.0));
                add(*c + Vec3::new(*r, *r, 0.0));
            }
            Geo::Arc(c, r, s, e) => {
                let arc = cadcraft_geom::Arc { center: c.xy(), radius: *r, start: *s, end: *e };
                let ab = arc.bounds();
                add(ab.min.to3(0.0));
                add(ab.max.to3(0.0));
            }
            Geo::Poly { verts, .. } => {
                let p = Polyline {
                    vertices: verts.iter().map(|(q, b, _, _)| cadcraft_geom::PolyVertex::with_bulge(q.xy(), *b)).collect(),
                    closed: false,
                };
                let pb = p.bounds();
                if !pb.is_empty() {
                    add(pb.min.to3(0.0));
                    add(pb.max.to3(0.0));
                }
            }
            Geo::Text(t) => add(t.insert),
            Geo::Solid(c) | Geo::Trace(c) | Geo::Face(c, _) => c.iter().for_each(|p| add(*p)),
            Geo::Insert(i) => add(i.insert),
            Geo::AttDef(a) => add(a.text.insert),
        }
    }
    b
}

// ---------------------------------------------------------------------------------------------
// Tags
// ---------------------------------------------------------------------------------------------

struct W {
    t: Vec<cadcraft_dxf::Tag>,
    /// Original upper-case name → the R12 name written.
    names: BTreeMap<String, String>,
    used_names: BTreeSet<String>,
}

impl W {
    fn s(&mut self, c: i32, v: impl Into<String>) {
        self.t.push(cadcraft_dxf::Tag::s(c, v));
    }
    fn f(&mut self, c: i32, v: f64) {
        self.t.push(cadcraft_dxf::Tag::f(c, if v.is_finite() { v } else { 0.0 }));
    }
    fn i(&mut self, c: i32, v: i64) {
        self.t.push(cadcraft_dxf::Tag::i(c, v));
    }
    fn p(&mut self, c: i32, p: Vec3) {
        self.f(c, p.x);
        self.f(c + 10, p.y);
        self.f(c + 20, p.z);
    }
    /// The R12 form of a symbol name: upper case, letters, digits, `$`, `-` and `_` only, at
    /// most 31 characters, unique (a clash gets a numeric suffix). The same original name
    /// always maps to the same R12 name.
    fn name(&mut self, original: &str) -> String {
        let key = original.trim().to_ascii_uppercase();
        if let Some(n) = self.names.get(&key) {
            return n.clone();
        }
        let mut base: String = key.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '$' | '-' | '_') { c } else { '_' }).collect();
        if base.is_empty() {
            base = "0".into();
        }
        base.truncate(MAX_NAME);
        let mut candidate = base.clone();
        let mut n = 1;
        while self.used_names.contains(&candidate) {
            n += 1;
            let suffix = format!("_{n}");
            let keep = MAX_NAME.saturating_sub(suffix.len());
            candidate = format!("{}{suffix}", base.chars().take(keep).collect::<String>());
        }
        self.used_names.insert(candidate.clone());
        self.names.insert(key, candidate.clone());
        candidate
    }
}

fn emit(d: &Drawing, o: &R12Options, f: &File) -> Vec<cadcraft_dxf::Tag> {
    let mut w = W { t: Vec::new(), names: BTreeMap::new(), used_names: BTreeSet::new() };
    // Fixed names first so nothing else takes them.
    for fixed in ["0", "CONTINUOUS", "STANDARD", "BYLAYER", "BYBLOCK", DOCFA_LAYER] {
        w.name(fixed);
    }
    let ext = file_extents(f);
    let (emin, emax) = if ext.is_empty() { (Vec3::ZERO, Vec3::ZERO) } else { (ext.min.to3(0.0), ext.max.to3(0.0)) };
    let h = &d.header;

    // ---------------- HEADER ----------------
    w.s(0, "SECTION");
    w.s(2, "HEADER");
    w.s(9, "$ACADVER");
    w.s(1, cadcraft_dxf::Version::R12.code());
    w.s(9, "$INSBASE");
    w.p(10, if o.docfa { Vec3::ZERO } else { h.point("INSBASE").unwrap_or(Vec3::ZERO) * o.scale });
    w.s(9, "$EXTMIN");
    w.p(10, emin);
    w.s(9, "$EXTMAX");
    w.p(10, emax);
    let (lmin, lmax) = if o.docfa {
        (Vec3::ZERO, Vec3::new(emax.x.max(0.0), emax.y.max(0.0), 0.0))
    } else {
        (h.point("LIMMIN").unwrap_or(Vec3::ZERO) * o.scale, h.point("LIMMAX").unwrap_or(Vec3::new(12.0, 9.0, 0.0)) * o.scale)
    };
    w.s(9, "$LIMMIN");
    w.f(10, lmin.x);
    w.f(20, lmin.y);
    w.s(9, "$LIMMAX");
    w.f(10, lmax.x);
    w.f(20, lmax.y);
    w.s(9, "$ORTHOMODE");
    w.i(70, 0);
    w.s(9, "$REGENMODE");
    w.i(70, 1);
    w.s(9, "$FILLMODE");
    w.i(70, 1);
    w.s(9, "$QTEXTMODE");
    w.i(70, 0);
    w.s(9, "$MIRRTEXT");
    w.i(70, h.i64("MIRRTEXT", 0).clamp(0, 1));
    w.s(9, "$LTSCALE");
    w.f(40, if o.docfa { 1.0 } else { h.f64("LTSCALE", 1.0) * o.scale });
    w.s(9, "$ATTMODE");
    w.i(70, 1);
    w.s(9, "$TEXTSIZE");
    w.f(40, h.f64("TEXTSIZE", 0.2) * o.scale);
    w.s(9, "$PDMODE");
    w.i(70, h.i64("PDMODE", 0));
    w.s(9, "$PDSIZE");
    w.f(40, h.f64("PDSIZE", 0.0) * o.scale);
    let clayer = w.name(&h.str("CLAYER", "0"));
    w.s(9, "$CLAYER");
    w.s(8, clayer);
    w.s(9, "$LUNITS");
    w.i(70, if o.docfa { 2 } else { h.i64("LUNITS", 2) });
    w.s(9, "$LUPREC");
    w.i(70, if o.docfa { 2 } else { h.i64("LUPREC", 4) });
    w.s(9, "$AUNITS");
    w.i(70, h.i64("AUNITS", 0));
    w.s(9, "$AUPREC");
    w.i(70, h.i64("AUPREC", 0));
    w.s(0, "ENDSEC");

    // ---------------- TABLES ----------------
    w.s(0, "SECTION");
    w.s(2, "TABLES");
    // VPORT: the active viewport shows the whole drawing.
    w.s(0, "TABLE");
    w.s(2, "VPORT");
    w.i(70, 1);
    w.s(0, "VPORT");
    w.s(2, "*ACTIVE");
    w.i(70, 0);
    w.f(10, 0.0);
    w.f(20, 0.0);
    w.f(11, 1.0);
    w.f(21, 1.0);
    let (vc, vh, aspect) = if ext.is_empty() {
        (Vec2::new(6.0, 4.5), 10.0, 1.5)
    } else {
        let size = ext.max - ext.min;
        let h = if size.y > 0.0 { size.y * 1.1 } else { size.x.max(1.0) };
        (ext.center(), h.max(1e-9), if size.y > 0.0 { (size.x / size.y).clamp(0.1, 10.0) } else { 1.5 })
    };
    w.f(12, vc.x);
    w.f(22, vc.y);
    w.f(13, 0.0);
    w.f(23, 0.0);
    w.f(14, 1.0);
    w.f(24, 1.0);
    w.f(15, 0.0);
    w.f(25, 0.0);
    w.f(16, 0.0);
    w.f(26, 0.0);
    w.f(36, 1.0);
    w.f(17, 0.0);
    w.f(27, 0.0);
    w.f(37, 0.0);
    w.f(40, vh);
    w.f(41, aspect);
    w.f(42, 50.0);
    w.f(43, 0.0);
    w.f(44, 0.0);
    w.f(50, 0.0);
    w.f(51, 0.0);
    w.i(71, 0);
    w.i(72, 100);
    w.i(73, 1);
    w.i(74, 3);
    w.i(75, 0);
    w.i(76, 0);
    w.i(77, 0);
    w.i(78, 0);
    w.s(0, "ENDTAB");
    // LTYPE: CONTINUOUS and the linetypes entities use (none in the DOCFA profile).
    let mut ltypes: Vec<&Linetype> = Vec::new();
    for name in &f.linetypes {
        if name == "CONTINUOUS" || name == "BYLAYER" || name == "BYBLOCK" {
            continue;
        }
        if let Some(lt) = d.linetype(name) {
            ltypes.push(lt);
        }
    }
    w.s(0, "TABLE");
    w.s(2, "LTYPE");
    w.i(70, 1 + ltypes.len() as i64);
    w.s(0, "LTYPE");
    w.s(2, "CONTINUOUS");
    w.i(70, 0);
    w.s(3, "Solid line");
    w.i(72, 65);
    w.i(73, 0);
    w.f(40, 0.0);
    for lt in ltypes {
        let name = w.name(&lt.name);
        w.s(0, "LTYPE");
        w.s(2, name);
        w.i(70, 0);
        w.s(3, lt.description.chars().take(47).collect::<String>());
        w.i(72, 65);
        w.i(73, lt.pattern.len() as i64);
        w.f(40, lt.pattern_length() * o.scale);
        for el in &lt.pattern {
            w.f(49, el.length * o.scale);
        }
    }
    w.s(0, "ENDTAB");
    // LAYER: every layer of the drawing (DOCFA: white and continuous).
    let layers: Vec<&Layer> = f.layers.iter().filter_map(|n| d.layers.iter().find(|l| l.name.eq_ignore_ascii_case(n))).collect();
    let mut extra: Vec<String> = f.layers.iter().filter(|n| !d.layers.iter().any(|l| l.name.eq_ignore_ascii_case(n))).cloned().collect();
    if !layers.iter().any(|l| l.name == "0") && !extra.iter().any(|n| n == "0") {
        extra.insert(0, "0".into());
    }
    w.s(0, "TABLE");
    w.s(2, "LAYER");
    w.i(70, (layers.len() + extra.len()) as i64);
    for l in &layers {
        let name = w.name(&l.name);
        w.s(0, "LAYER");
        w.s(2, name);
        w.i(70, if l.frozen { 1 } else { 0 } | if l.locked { 4 } else { 0 });
        let aci = if o.docfa { 7 } else { i64::from(l.color.to_aci()).clamp(1, 255) };
        w.i(62, if l.on { aci } else { -aci });
        let lt = if o.docfa { "CONTINUOUS".to_string() } else { w.name(&l.linetype) };
        w.s(6, lt);
    }
    for n in &extra {
        let name = w.name(n);
        w.s(0, "LAYER");
        w.s(2, name);
        w.i(70, 0);
        w.i(62, 7);
        w.s(6, "CONTINUOUS");
    }
    w.s(0, "ENDTAB");
    // STYLE: STANDARD and the styles texts use.
    let styles: Vec<&TextStyle> = f.styles.iter().filter(|n| *n != "STANDARD").filter_map(|n| d.text_style(n)).collect();
    w.s(0, "TABLE");
    w.s(2, "STYLE");
    w.i(70, 1 + styles.len() as i64);
    let standard = d.text_style("Standard");
    style_record(&mut w, "STANDARD", standard, o);
    for s in styles {
        let name = w.name(&s.name);
        style_record(&mut w, &name, Some(s), o);
    }
    w.s(0, "ENDTAB");
    w.s(0, "ENDSEC");

    // ---------------- BLOCKS ----------------
    w.s(0, "SECTION");
    w.s(2, "BLOCKS");
    for b in &f.blocks {
        let name = w.name(&b.name);
        let has_attdefs = b.ents.iter().any(|e| matches!(e.geo, Geo::AttDef(_)));
        w.s(0, "BLOCK");
        w.s(8, "0");
        w.s(2, name.clone());
        w.i(70, if has_attdefs { 2 } else { 0 });
        w.p(10, b.base);
        w.s(3, name);
        for e in &b.ents {
            entity(&mut w, e);
        }
        w.s(0, "ENDBLK");
        w.s(8, "0");
    }
    w.s(0, "ENDSEC");

    // ---------------- ENTITIES ----------------
    w.s(0, "SECTION");
    w.s(2, "ENTITIES");
    for e in &f.ents {
        entity(&mut w, e);
    }
    w.s(0, "ENDSEC");
    w.s(0, "EOF");
    w.t
}

/// A STYLE record: the built-in font goes out as `txt`, which every reader has.
fn style_record(w: &mut W, name: &str, s: Option<&TextStyle>, o: &R12Options) {
    let default = TextStyle::default();
    let s = s.unwrap_or(&default);
    w.s(0, "STYLE");
    w.s(2, name);
    w.i(70, 0);
    w.f(40, s.height * o.scale);
    w.f(41, if s.width_factor.is_finite() && s.width_factor > 0.0 { s.width_factor } else { 1.0 });
    w.f(50, s.oblique.to_degrees());
    w.i(71, if s.backwards { 2 } else { 0 } | if s.upside_down { 4 } else { 0 });
    w.f(42, 0.2 * o.scale);
    let font = s.font.trim();
    let legacy = font.is_empty() || o.docfa || !font.to_ascii_lowercase().ends_with(".shx");
    w.s(3, if legacy { "txt" } else { font });
    w.s(4, if o.docfa { "" } else { s.big_font.as_str() });
}

/// The common groups of an entity: layer, colour, linetype.
fn common(w: &mut W, e: &Ent) {
    let layer = w.name(&e.layer);
    w.s(8, layer);
    if let Some(lt) = &e.linetype {
        let lt = if lt.eq_ignore_ascii_case("byblock") { "BYBLOCK".to_string() } else { w.name(lt) };
        w.s(6, lt);
    }
    if let Some(c) = e.color {
        w.i(62, c);
    }
}

fn entity(w: &mut W, e: &Ent) {
    match &e.geo {
        Geo::Line(a, b) => {
            w.s(0, "LINE");
            common(w, e);
            w.p(10, *a);
            w.p(11, *b);
        }
        Geo::Point(p) => {
            w.s(0, "POINT");
            common(w, e);
            w.p(10, *p);
        }
        Geo::Circle(c, r) => {
            w.s(0, "CIRCLE");
            common(w, e);
            w.p(10, *c);
            w.f(40, *r);
        }
        Geo::Arc(c, r, s, en) => {
            w.s(0, "ARC");
            common(w, e);
            w.p(10, *c);
            w.f(40, *r);
            w.f(50, s.to_degrees());
            w.f(51, en.to_degrees());
        }
        Geo::Poly { verts, closed, three_d, width } => {
            w.s(0, "POLYLINE");
            common(w, e);
            w.i(66, 1);
            let z = if *three_d { 0.0 } else { verts.first().map_or(0.0, |v| v.0.z) };
            w.p(10, Vec3::new(0.0, 0.0, z));
            w.i(70, i64::from(*closed) | if *three_d { 8 } else { 0 });
            if *width > 0.0 {
                w.f(40, *width);
                w.f(41, *width);
            }
            let layer = w.name(&e.layer);
            for (p, bulge, sw, ew) in verts {
                w.s(0, "VERTEX");
                w.s(8, layer.clone());
                w.p(10, if *three_d { *p } else { Vec3::new(p.x, p.y, z) });
                if !*three_d && (*sw != 0.0 || *ew != 0.0) {
                    w.f(40, *sw);
                    w.f(41, *ew);
                }
                if !*three_d && *bulge != 0.0 {
                    w.f(42, *bulge);
                }
                if *three_d {
                    w.i(70, 32);
                }
            }
            w.s(0, "SEQEND");
            w.s(8, layer);
        }
        Geo::Text(t) => {
            w.s(0, "TEXT");
            common(w, e);
            text_tags(w, t);
        }
        Geo::Solid(c) | Geo::Trace(c) => {
            w.s(0, if matches!(e.geo, Geo::Solid(_)) { "SOLID" } else { "TRACE" });
            common(w, e);
            for (k, p) in c.iter().enumerate() {
                w.p(10 + k as i32, *p);
            }
        }
        Geo::Face(c, hidden) => {
            w.s(0, "3DFACE");
            common(w, e);
            for (k, p) in c.iter().enumerate() {
                w.p(10 + k as i32, *p);
            }
            if *hidden != 0 {
                w.i(70, i64::from(*hidden));
            }
        }
        Geo::Insert(i) => {
            let attribs: Vec<&Attrib> = i.attribs.iter().filter(|a| !a.constant).collect();
            w.s(0, "INSERT");
            common(w, e);
            if !attribs.is_empty() {
                w.i(66, 1);
            }
            let name = w.name(&i.block);
            w.s(2, name);
            w.p(10, i.insert);
            if i.scale != Vec3::new(1.0, 1.0, 1.0) {
                w.f(41, i.scale.x);
                w.f(42, i.scale.y);
                w.f(43, i.scale.z);
            }
            if i.rotation != 0.0 {
                w.f(50, i.rotation.to_degrees());
            }
            if i.cols > 1 || i.rows > 1 {
                w.i(70, i64::from(i.cols.max(1)));
                w.i(71, i64::from(i.rows.max(1)));
                w.f(44, i.col_spacing);
                w.f(45, i.row_spacing);
            }
            let layer = w.name(&e.layer);
            for a in &attribs {
                w.s(0, "ATTRIB");
                w.s(8, layer.clone());
                text_tags(w, &a.text);
                w.s(2, a.tag.chars().take(MAX_NAME).collect::<String>());
                w.i(70, attrib_flags(a));
            }
            if !attribs.is_empty() {
                w.s(0, "SEQEND");
                w.s(8, layer);
            }
        }
        Geo::AttDef(a) => {
            w.s(0, "ATTDEF");
            common(w, e);
            text_tags(w, &a.text);
            w.s(3, a.prompt.clone());
            w.s(2, a.tag.chars().take(MAX_NAME).collect::<String>());
            w.i(70, attrib_flags(a));
        }
    }
}

/// ATTRIB/ATTDEF flags (DXF Reference group 70): invisible, constant, verify, preset.
fn attrib_flags(a: &Attrib) -> i64 {
    i64::from(a.invisible) | if a.constant { 2 } else { 0 } | if a.modes.verify { 4 } else { 0 } | if a.modes.preset { 8 } else { 0 }
}

/// The TEXT groups shared by TEXT, ATTRIB and ATTDEF (group 72/73 justification, 11 alignment).
fn text_tags(w: &mut W, t: &Text) {
    w.p(10, t.insert);
    w.f(40, t.height.abs());
    w.s(1, t.value.replace(['\r', '\n'], " "));
    if t.rotation != 0.0 {
        w.f(50, t.rotation.to_degrees());
    }
    if (t.width_factor - 1.0).abs() > 1e-12 && t.width_factor.is_finite() && t.width_factor > 0.0 {
        w.f(41, t.width_factor);
    }
    if t.oblique != 0.0 {
        w.f(51, t.oblique.to_degrees());
    }
    let style = w.name(&t.style);
    w.s(7, style);
    let h = match t.halign {
        HAlign::Left => 0,
        HAlign::Center => 1,
        HAlign::Right => 2,
        HAlign::Aligned => 3,
        HAlign::Middle => 4,
        HAlign::Fit => 5,
    };
    let v = match t.valign {
        VAlign::Baseline => 0,
        VAlign::Bottom => 1,
        VAlign::Middle => 2,
        VAlign::Top => 3,
    };
    if h != 0 {
        w.i(72, h);
    }
    if let Some(a) = t.align_pt.filter(|_| h != 0 || v != 0) {
        w.p(11, a);
    }
    if v != 0 {
        w.i(73, v);
    }
}

#[cfg(test)]
mod tests;
