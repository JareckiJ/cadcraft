//! Drawings for the Italian cadastre's DOCFA (Agenzia delle Entrate), which imports a plan from
//! DXF or DWG files up to AutoCAD 2000 (see `docs/docfa.md`). The file is a *reduced copy* of
//! the drawing, as DOCFA's guides ask: model space only; block references exploded; dimensions,
//! tables, multilines and leaders as lines and text; multiline text as TEXT lines; ellipses,
//! splines and regions as polylines; hatches, images, wipeouts and objects on layers that are off
//! or frozen left out; every layer continuous and white; colours only on the surface polygons of
//! layer [`LAYER`] (DXF group 62, DOCFA's area classes); text transliterated to ASCII; the drawing
//! scaled to metres and declaring no unit (`$INSUNITS` 0), as the Agenzia's own DXF templates.
//! [`drawing`] makes that copy; the normal DXF and DWG writers write it at AutoCAD 2000.

use std::collections::BTreeMap;

use cadcraft_color::Color;
use cadcraft_doc::*;
use cadcraft_geom::{Bounds2, Mat3, PolyVertex, Polyline, Segment, Vec2, Vec3};

/// The layer DOCFA reads surface polygons (closed polylines, one per area class colour) from.
pub const LAYER: &str = "DOCFA_POLIGONI";

/// Objects in the copy are capped: a drawing beyond this is reported, not written whole.
const MAX_ENTITIES: usize = 2_000_000;
/// Objects visited while exploding, drawn or not: block nestings that fan out (or cycle) without
/// drawing anything still stop here.
const MAX_VISITS: usize = 8_000_000;
/// Nested block references deeper than this are left out (cyclic definitions).
const MAX_DEPTH: usize = cadcraft_doc::MAX_BLOCK_DEPTH;
/// AutoCAD's line spacing: baselines of single-spaced multiline text are 5/3 of the height apart.
const LINE_SPACING: f64 = 5.0 / 3.0;

/// What the copy keeps of the drawing: conversions and omissions, counted by kind, and the
/// DOCFA checks. [`Report::lines`] words it for the user.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    /// "ELLIPSE → POLYLINE" → how many.
    pub converted: BTreeMap<&'static str, usize>,
    /// What was left out → how many.
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
    /// out") and the warnings. Empty when the copy keeps everything.
    pub fn lines(&self) -> Vec<String> {
        let mut out: Vec<String> = self.converted.iter().map(|(what, n)| format!("{n} {what}")).collect();
        out.extend(self.dropped.iter().map(|(what, n)| format!("{n} {what} left out")));
        out.extend(self.warnings.iter().cloned());
        out
    }
}

/// The drawing DOCFA imports, made of `d` (see the module doc), and what that did to it.
/// `scale` takes drawing units to metres (0.001 for a drawing in millimetres); a scale that is
/// not a positive number is 1.
pub fn drawing(d: &Drawing, scale: f64) -> (Drawing, Report) {
    let committed = d.committed();
    let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    let (objects, report) = reduce(&committed, scale);
    let mut out = committed.into_owned();
    out.model = EntityStore::new();
    out.blocks.clear();
    out.groups.clear();
    out.constraints.clear();
    out.views.clear();
    out.ucss.clear();
    out.layer_states.clear();
    out.hidden_objects.clear();
    out.read_notes.clear();
    out.refedit = None;
    out.foreign = Default::default();
    for l in &mut out.layouts {
        l.entities = EntityStore::new();
        l.view = None;
    }
    // Every layer white and continuous; what was on a hidden layer is gone, so nothing hides.
    // The polygons' layer gets the exact name DOCFA looks for.
    for l in &mut out.layers {
        l.color = Color::Index(7);
        l.linetype = "Continuous".into();
        l.on = true;
        l.frozen = false;
        l.transparency = 0;
        if l.name.eq_ignore_ascii_case(LAYER) {
            l.name = LAYER.into();
        }
    }
    for st in &mut out.text_styles {
        st.height *= scale;
    }
    let h = &mut out.header;
    h.set("INSBASE", HVal::Point(Vec3::ZERO));
    h.set_i64("LUNITS", 2);
    h.set_i64("LUPREC", 2);
    // The file is in metres but says no unit, as the Agenzia's templates (which have no such
    // variable): a reader that converts declared units (metres to the millimetres of its own
    // template, say) would otherwise scale the plan.
    h.set_i64("INSUNITS", 0);
    h.set_f64("LTSCALE", 1.0);
    h.set_f64("TEXTSIZE", h.f64("TEXTSIZE", 0.2) * scale);
    for o in objects {
        let common = Common { layer: o.layer, color: o.color, ..Common::default() };
        // A drawing that can't take an object (a full store) loses it.
        let _ = out.add(&Space::Model, common, o.kind);
    }
    (out, report)
}

// ---------------------------------------------------------------------------------------------
// The reduction
// ---------------------------------------------------------------------------------------------

/// An object of the copy: a kind DOCFA reads, on its layer, coloured only on [`LAYER`].
struct Object {
    layer: String,
    color: Color,
    kind: EntityKind,
}

struct Cx<'a> {
    d: &'a Drawing,
    report: Report,
    /// Objects made so far (bounded by [`MAX_ENTITIES`]).
    count: usize,
    /// Objects visited so far (bounded by [`MAX_VISITS`]).
    visits: usize,
}

fn reduce(d: &Drawing, scale: f64) -> (Vec<Object>, Report) {
    let mut cx = Cx { d, report: Report::default(), count: 0, visits: 0 };
    let mut out = Vec::new();
    let base = if (scale - 1.0).abs() > 1e-15 { Some(Mat3::scale(scale, scale)) } else { None };
    for e in d.model.iter() {
        convert(&mut cx, &mut out, e, base.as_ref(), 0);
    }
    let layouts: usize = d.layouts.iter().map(|l| l.entities.len()).sum();
    if layouts > 0 {
        cx.report.warnings.push(format!("{layouts} paper-space objects (layouts) are not written: DOCFA takes model space"));
    }
    checks(&mut cx.report, d, &out);
    (out, cx.report)
}

/// Convert one entity (and what it expands to) into `out`, transformed by `m` (block
/// references, the scale), `depth` block levels down.
fn convert(cx: &mut Cx, out: &mut Vec<Object>, e: &Entity, m: Option<&Mat3>, depth: usize) {
    if cx.count >= MAX_ENTITIES || cx.visits >= MAX_VISITS {
        if cx.count != usize::MAX {
            cx.report.warnings.push(format!("the file is cut short: more than {MAX_ENTITIES} objects, or block references nested too widely"));
            cx.count = usize::MAX;
        }
        return;
    }
    cx.visits = cx.visits.saturating_add(1);
    if depth > MAX_DEPTH {
        cx.report.leave_out("INSERT nested too deep");
        return;
    }
    if !e.common.visible {
        return;
    }
    if !cx.d.is_visible(e) {
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
    let push = |cx: &mut Cx, out: &mut Vec<Object>, kind: EntityKind| {
        if !finite(&kind) {
            cx.report.leave_out("object with non-finite coordinates");
            return;
        }
        cx.count = cx.count.saturating_add(1);
        out.push(Object { layer: layer_name(&c.layer), color: color(cx, c), kind });
    };
    let d = cx.d;
    match &e.kind {
        EntityKind::Line(_)
        | EntityKind::Point(_)
        | EntityKind::Circle(_)
        | EntityKind::Arc(_)
        | EntityKind::LwPolyline(_)
        | EntityKind::Polyline3d(_)
        | EntityKind::Solid(_)
        | EntityKind::Trace(_)
        | EntityKind::Face3d(_) => push(cx, out, e.kind.clone()),
        EntityKind::Ellipse(el) => {
            let ge = cadcraft_geom::Ellipse { center: el.center.xy(), major: el.major.xy(), ratio: el.ratio, start: el.start, end: el.end };
            let mut pts = Vec::new();
            ge.tessellate(tolerance(el.major.len()), &mut pts);
            cx.report.convert("ELLIPSE → POLYLINE");
            push(cx, out, points_poly(&pts, ge.is_full(), el.center.z));
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
            for line in text_lines(t) {
                push(cx, out, EntityKind::Text(line));
            }
        }
        EntityKind::MText(t) => {
            cx.report.convert("MTEXT → TEXT");
            for line in mtext_lines(t) {
                push(cx, out, EntityKind::Text(line));
            }
        }
        // An attribute definition shows through the references' attributes.
        EntityKind::AttDef(_) => {}
        EntityKind::Insert(i) => explode(cx, out, e, i, depth),
        EntityKind::Dimension(_) => {
            cx.report.convert("DIMENSION → lines and text");
            // The dimension as drawn at the drawing's size (its style's text height, arrows and
            // offsets), then moved, rotated and scaled with the rest.
            if let EntityKind::Dimension(dm) = &original.kind {
                for part in crate::dxf_write::dim_block_entities(d, dm, &c.layer) {
                    convert(cx, out, &part, m, depth + 1);
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
                    convert(cx, out, &part, Some(&placed), depth + 1);
                }
            }
        }
        EntityKind::MLine(ml) => {
            cx.report.convert("MLINE → lines and arcs");
            for s in ml.segments() {
                push(cx, out, segment_kind(&s));
            }
        }
        EntityKind::Leader(l) => {
            cx.report.convert("LEADER → POLYLINE");
            let pts: Vec<Vec2> = l.vertices.iter().map(|q| q.xy()).collect();
            push(cx, out, points_poly(&pts, false, l.vertices.first().map_or(0.0, |q| q.z)));
        }
        EntityKind::MLeader(ml) => {
            cx.report.convert("MULTILEADER → polylines and text");
            for path in ml.leader_paths() {
                push(cx, out, points_poly(&path, false, 0.0));
            }
            if let Some(t) = &ml.text {
                for line in mtext_lines(t) {
                    push(cx, out, EntityKind::Text(line));
                }
            }
            if let Some(b) = &ml.block {
                explode(cx, out, e, b, depth);
            }
        }
        EntityKind::Hatch(_) => cx.report.leave_out("HATCH (DOCFA takes no hatching)"),
        EntityKind::Region(r) => {
            cx.report.convert("REGION → POLYLINE");
            for p in r.polylines() {
                push(cx, out, lwpolyline(p.vertices, true, 0.0));
            }
        }
        EntityKind::Viewport(_) => cx.report.leave_out("VIEWPORT"),
        EntityKind::Image(_) => cx.report.leave_out("IMAGE"),
        EntityKind::Wipeout(_) => cx.report.leave_out("WIPEOUT"),
        EntityKind::Tolerance(_) => cx.report.leave_out("TOLERANCE"),
        EntityKind::Unknown(u) => cx.report.leave_out(&u.dxf_type),
    }
}

/// A block reference as its contents (each cell of an array) and its visible attributes as text.
fn explode(cx: &mut Cx, out: &mut Vec<Object>, e: &Entity, i: &Insert, depth: usize) {
    let Some(b) = cx.d.block(&i.block) else {
        cx.report.leave_out("INSERT of a missing block");
        return;
    };
    if b.xref_path.is_some() {
        cx.report.leave_out("external reference (XREF)");
        return;
    }
    if !(i.insert.x.is_finite() && i.insert.y.is_finite() && i.rotation.is_finite() && i.col_spacing.is_finite() && i.row_spacing.is_finite()) {
        cx.report.leave_out("object with non-finite coordinates");
        return;
    }
    let cols = i.cols.clamp(1, 1000);
    let rows = i.rows.clamp(1, 1000);
    let cells = usize::try_from(cols).unwrap_or(1).saturating_mul(usize::try_from(rows).unwrap_or(1));
    if b.entities.len().saturating_mul(cells) > MAX_ENTITIES {
        cx.report.leave_out("INSERT array too large to explode");
        return;
    }
    cx.report.convert("INSERT exploded");
    // `i` is in world coordinates already (the entity was transformed); the block's base point
    // is in the block's own units, which `Insert::transform` maps to the world.
    let ins = i.transform(b.base.xy());
    for r in 0..rows {
        for cc in 0..cols {
            let off = Vec2::new(f64::from(cc) * i.col_spacing, f64::from(r) * i.row_spacing).rotate(i.rotation);
            let cell = if cc == 0 && r == 0 { ins } else { Mat3::translate(off).then_before(ins) };
            for be in b.entities.iter() {
                convert(cx, out, be, Some(&cell), depth + 1);
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
        convert(cx, out, &part, None, depth + 1);
    }
}

/// A line or arc segment.
fn segment_kind(s: &Segment) -> EntityKind {
    match s {
        Segment::Line(l) => EntityKind::Line(Line { a: l.a.to3(0.0), b: l.b.to3(0.0) }),
        Segment::Arc { arc, .. } => EntityKind::Arc(Arc { center: arc.center.to3(0.0), radius: arc.radius, start: arc.start, end: arc.end }),
    }
}

/// A polyline through `pts` at elevation `z`.
fn points_poly(pts: &[Vec2], closed: bool, z: f64) -> EntityKind {
    let mut vertices: Vec<PolyVertex> = pts.iter().map(|p| PolyVertex::new(*p)).collect();
    // A closed curve's tessellation repeats its first point; the closing segment replaces it.
    if closed && vertices.len() > 2 && vertices.first().map(|v| v.p) == vertices.last().map(|v| v.p) {
        vertices.pop();
    }
    lwpolyline(vertices, closed, z)
}

fn lwpolyline(vertices: Vec<PolyVertex>, closed: bool, elevation: f64) -> EntityKind {
    EntityKind::LwPolyline(LwPolyline { vertices, closed, const_width: 0.0, elevation, plinegen: false })
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

/// Whether every coordinate of an object of the copy is finite (hostile files carry NaN and
/// infinities).
fn finite(k: &EntityKind) -> bool {
    let ok = |p: &Vec3| p.x.is_finite() && p.y.is_finite() && p.z.is_finite();
    match k {
        EntityKind::Line(l) => ok(&l.a) && ok(&l.b),
        EntityKind::Point(p) => ok(&p.p),
        EntityKind::Circle(c) => ok(&c.center) && c.radius.is_finite(),
        EntityKind::Arc(a) => ok(&a.center) && a.radius.is_finite() && a.start.is_finite() && a.end.is_finite(),
        EntityKind::LwPolyline(p) => {
            p.elevation.is_finite()
                && p.const_width.is_finite()
                && p.vertices
                    .iter()
                    .all(|v| v.p.x.is_finite() && v.p.y.is_finite() && v.bulge.is_finite() && v.start_width.is_finite() && v.end_width.is_finite())
        }
        EntityKind::Polyline3d(p) => p.points.iter().all(ok),
        EntityKind::Text(t) => ok(&t.insert) && t.align_pt.is_none_or(|a| ok(&a)) && t.height.is_finite() && t.rotation.is_finite(),
        EntityKind::Solid(s) | EntityKind::Trace(s) => s.corners.iter().all(ok),
        EntityKind::Face3d(f) => f.corners.iter().all(ok),
        _ => false,
    }
}

/// The layer an object goes on: the polygons' layer under the exact name DOCFA looks for.
fn layer_name(layer: &str) -> String {
    if layer.eq_ignore_ascii_case(LAYER) { LAYER.into() } else { layer.to_string() }
}

/// The colour of an object: only the surface polygons of [`LAYER`] carry one (their resolved
/// ACI, DOCFA's area class); everything else is ByLayer (DOCFA draws it in its own colour).
fn color(cx: &Cx, c: &Common) -> Color {
    if !c.layer.eq_ignore_ascii_case(LAYER) {
        return Color::ByLayer;
    }
    let aci = match c.color {
        Color::ByLayer => cx.d.layer(&c.layer).map_or(7, |l| l.color.to_aci()),
        Color::ByBlock => 7,
        other => other.to_aci(),
    };
    Color::Index(u8::try_from(aci.clamp(1, 255)).unwrap_or(7))
}

/// A text entity as one TEXT per line (a multiline attribute's value holds newlines), in ASCII.
fn text_lines(t: &Text) -> Vec<Text> {
    let value = transliterate(&t.value);
    if !value.contains('\n') {
        return vec![Text { value, ..t.clone() }];
    }
    let step = t.height.abs() * LINE_SPACING;
    value
        .split('\n')
        .enumerate()
        .map(|(k, line)| {
            let off = Vec2::new(0.0, -step * k as f64).rotate(t.rotation).to3(0.0);
            Text { value: line.to_string(), insert: t.insert + off, align_pt: t.align_pt.map(|a| a + off), ..t.clone() }
        })
        .collect()
}

/// Multiline text as TEXT lines placed by its attachment point, each line justified like its
/// column of the attachment (left, centre, right), in ASCII. Inline formatting is dropped.
fn mtext_lines(t: &MText) -> Vec<Text> {
    let plain = transliterate(&cadcraft_fonts::plain_mtext(&t.contents));
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
    let halign = match col {
        0 => HAlign::Left,
        1 => HAlign::Center,
        _ => HAlign::Right,
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
                valign: VAlign::Baseline,
            }
        })
        .collect()
}

/// Latin letters with diacritics as their base letters, the degree, plus-minus and diameter signs
/// as TEXT control codes (`%%d`, `%%p`, `%%c`: every AutoCAD font has them), other non-ASCII
/// characters as `?`. Old readers show `\U+00E8` for `è`.
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

/// What DOCFA needs of the file: surface polygons closed, coloured with an area class and on
/// their layer; the sheet at the origin; a plan in metres.
fn checks(r: &mut Report, d: &Drawing, objects: &[Object]) {
    let mut polygons = 0usize;
    let mut open = 0usize;
    let mut other = 0usize;
    let mut odd_color = 0usize;
    for o in objects.iter().filter(|o| o.layer == LAYER) {
        match &o.kind {
            EntityKind::LwPolyline(p) => {
                polygons += 1;
                if !p.closed {
                    open += 1;
                }
                if !matches!(o.color, Color::Index(1..=8)) {
                    odd_color += 1;
                }
            }
            EntityKind::Text(_) => {}
            _ => other += 1,
        }
    }
    if d.layers.iter().any(|l| l.name.eq_ignore_ascii_case(LAYER)) {
        if polygons == 0 {
            r.warnings.push(format!("layer {LAYER} has no polylines: DOCFA makes no surface polygons from this file"));
        } else {
            r.warnings.push(format!("{polygons} surface polygons on {LAYER}"));
        }
    }
    if open > 0 {
        r.warnings.push(format!("{open} polylines on {LAYER} are not closed: DOCFA needs closed polygons"));
    }
    if other > 0 {
        r.warnings.push(format!("{other} objects on {LAYER} are not polylines: DOCFA reads polygons from closed polylines only"));
    }
    if odd_color > 0 {
        r.warnings.push(format!("{odd_color} polygons on {LAYER} have a colour outside 1–8 (DOCFA's area classes A, B, C, D, E, F, G, A2)"));
    }
    let ext = extents(objects);
    if ext.is_empty() {
        return;
    }
    if ext.min.x < -1e-9 || ext.min.y < -1e-9 {
        r.warnings.push("the drawing extends left of or below the origin: DOCFA expects the sheet frame's lower-left corner at 0,0".into());
    }
    let size = extent(&ext);
    if size > 1000.0 {
        r.warnings.push("the drawing spans more than 1000 m: is its unit right? DOCFA takes metres".into());
    } else if size > 0.0 && size < 1.0 {
        r.warnings.push("the drawing spans less than 1 m: is its unit right? DOCFA takes metres".into());
    }
}

/// The objects' bounds.
fn extents(objects: &[Object]) -> Bounds2 {
    let mut b = Bounds2::EMPTY;
    let mut add = |p: Vec3| b = b.union(&Bounds2::new(p.xy(), p.xy()));
    for o in objects {
        match &o.kind {
            EntityKind::Line(l) => {
                add(l.a);
                add(l.b);
            }
            EntityKind::Point(p) => add(p.p),
            EntityKind::Circle(c) => {
                add(c.center - Vec3::new(c.radius, c.radius, 0.0));
                add(c.center + Vec3::new(c.radius, c.radius, 0.0));
            }
            EntityKind::Arc(a) => {
                let ab = cadcraft_geom::Arc { center: a.center.xy(), radius: a.radius, start: a.start, end: a.end }.bounds();
                add(ab.min.to3(0.0));
                add(ab.max.to3(0.0));
            }
            EntityKind::LwPolyline(p) => {
                let pb = Polyline { vertices: p.vertices.clone(), closed: false }.bounds();
                if !pb.is_empty() {
                    add(pb.min.to3(0.0));
                    add(pb.max.to3(0.0));
                }
            }
            EntityKind::Polyline3d(p) => p.points.iter().for_each(|q| add(*q)),
            EntityKind::Text(t) => add(t.insert),
            EntityKind::Solid(s) | EntityKind::Trace(s) => s.corners.iter().for_each(|p| add(*p)),
            EntityKind::Face3d(f) => f.corners.iter().for_each(|p| add(*p)),
            _ => {}
        }
    }
    b
}

#[cfg(test)]
mod tests;
