//! Exact intersections, closest points, perpendicular feet and tangent points for every curve
//! type: lines, arcs, circles, ellipses and (rational, closed, periodic) splines.
//!
//! Ellipses and splines are worked on as rational Bézier pieces (`bezier.rs`), never as
//! polygons:
//! - a curve against a line, circle, arc or ellipse substitutes the curve into the conic's
//!   implicit equation, a polynomial whose roots are isolated in Bernstein form and polished by
//!   Newton steps (tangencies are double roots, found at the critical point);
//! - a spline against a spline subdivides both with bounding boxes; where the derivative cones
//!   of two pieces are disjoint there is at most one crossing, found by Newton's method on the
//!   two-curve system; where they overlap (near a tangency) the pieces are cut down until tiny
//!   and the closest pair of points decides;
//! - closest points, perpendicular feet and tangent points are the roots of
//!   `(C − p)·C' = 0` and `(C − p)×C' = 0`, polynomials again.
//!
//! Results are accurate to about 1e-9 of the size of the geometry (usually to rounding).
//! Every search is bounded; degenerate or hostile input gives no result, never a panic.

use std::borrow::Cow;

use crate::bezier::{Piece, Roots, badd, bdiff, bmul, derivs, net_bounds, proj, roots, spline_pieces, split};
use crate::nurbs::H;
use crate::{Arc, Bounds2, Circle, EPS, Ellipse, Line, Segment, Spline, TAU, Vec2, angle_in_sweep, intersect_ext, norm_angle};

/// Relative tolerance: a tangency counts when the curves come within this fraction of their
/// size, and coincident results merge.
pub const REL_TOL: f64 = 1e-9;
/// Most nodes one spline/spline piece pair visits.
const MAX_PAIR_NODES: usize = 20_000;
/// Most nodes all piece pairs of one curve pair visit together.
const MAX_CURVE_NODES: usize = 200_000;
/// Most intersections reported for one curve pair (overlapping curves stop here).
const MAX_HITS: usize = 512;
/// Pieces smaller than this fraction of the problem are leaves of the spline/spline search.
const LEAF: f64 = 1e-5;
/// Deepest spline/spline subdivision.
const MAX_PAIR_DEPTH: u32 = 60;
/// Newton / Levenberg–Marquardt steps for one two-curve refinement.
const NEWTON_STEPS: usize = 60;
/// Tessellation tolerance (relative to size) of the fallback for curves without exact pieces.
const FALLBACK_TOL: f64 = 1e-4;

/// A point on a curve with its parameter: 0..1 along a line segment, the angle (radians) on an
/// arc or circle, the ellipse parameter on an ellipse, the spline parameter on a spline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CurvePoint {
    pub t: f64,
    pub point: Vec2,
}

/// An intersection of two curves: the point and its parameter on each (see [`CurvePoint`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CurveHit {
    pub point: Vec2,
    pub t1: f64,
    pub t2: f64,
}

/// The geometry of a [`Curve`].
#[derive(Clone, Debug, PartialEq)]
pub enum CurveKind {
    /// A line segment or circular arc.
    Seg(Segment),
    Circle(Circle),
    Ellipse(Ellipse),
    Spline(Spline),
}

/// A curve prepared for exact queries: an ellipse or spline keeps its rational Bézier pieces,
/// so repeated queries (snaps against many neighbours) decompose it once.
#[derive(Clone, Debug)]
pub struct Curve {
    kind: CurveKind,
    pieces: Vec<Piece>,
    /// Polygon stand-in for an ellipse or spline that has no exact pieces (degenerate axes,
    /// hostile weights, a degree above the editing limit): the old tessellated behaviour.
    fallback: Vec<Segment>,
}

impl Curve {
    pub fn segment(s: Segment) -> Curve {
        Curve { kind: CurveKind::Seg(s), pieces: Vec::new(), fallback: Vec::new() }
    }

    pub fn line(l: Line) -> Curve {
        Curve::segment(Segment::Line(l))
    }

    pub fn arc(a: Arc) -> Curve {
        Curve::segment(Segment::Arc { arc: a, ccw: true })
    }

    pub fn circle(c: Circle) -> Curve {
        Curve { kind: CurveKind::Circle(c), pieces: Vec::new(), fallback: Vec::new() }
    }

    pub fn ellipse(e: &Ellipse) -> Curve {
        let pieces = Spline::from_ellipse(e).and_then(|s| spline_pieces(&s)).unwrap_or_default();
        let fallback = if pieces.is_empty() {
            let mut pts = Vec::new();
            if e.center.is_finite() && e.major.is_finite() && e.ratio.is_finite() && e.start.is_finite() && e.end.is_finite() {
                e.tessellate(e.major.len() * FALLBACK_TOL, &mut pts);
            }
            chords(&pts)
        } else {
            Vec::new()
        };
        Curve { kind: CurveKind::Ellipse(*e), pieces, fallback }
    }

    pub fn spline(s: &Spline) -> Curve {
        let pieces = spline_pieces(s).unwrap_or_default();
        let fallback = if pieces.is_empty() && s.is_valid() {
            let b = Bounds2::from_points(s.control.iter().copied());
            let size = b.width().hypot(b.height());
            if size.is_finite() { chords(&s.tessellate((size * FALLBACK_TOL).max(1e-12))) } else { Vec::new() }
        } else {
            Vec::new()
        };
        Curve { kind: CurveKind::Spline(s.clone()), pieces, fallback }
    }

    pub fn kind(&self) -> &CurveKind {
        &self.kind
    }

    /// The line or arc, for callers that keep their own closed-form segment code.
    pub fn as_segment(&self) -> Option<&Segment> {
        match &self.kind {
            CurveKind::Seg(s) => Some(s),
            _ => None,
        }
    }

    /// True when the curve is worked on exactly (always, except the degenerate fallbacks).
    pub fn is_exact(&self) -> bool {
        matches!(self.kind, CurveKind::Seg(_) | CurveKind::Circle(_)) || !self.pieces.is_empty()
    }

    /// Bounds of the curve (of its control points for a spline: a superset).
    pub fn bounds(&self) -> Bounds2 {
        match &self.kind {
            CurveKind::Seg(s) => s.bounds(),
            CurveKind::Circle(c) => c.bounds(),
            CurveKind::Ellipse(e) if self.pieces.is_empty() => e.bounds(),
            _ => self.pieces.iter().fold(Bounds2::default(), |b, p| b.union(&p.bounds)).union(&fallback_bounds(&self.fallback)),
        }
    }

    /// The parameter of a point lying on the curve (see [`CurvePoint`]).
    fn param_at(&self, p: Vec2, piece_t: Option<f64>) -> f64 {
        match &self.kind {
            CurveKind::Seg(Segment::Line(l)) => l.param_of(p),
            CurveKind::Seg(Segment::Arc { arc, .. }) => norm_angle(arc.center.angle_to(p)),
            CurveKind::Circle(c) => norm_angle(c.center.angle_to(p)),
            CurveKind::Ellipse(e) => ellipse_param(e, p),
            CurveKind::Spline(_) => piece_t.unwrap_or(0.0),
        }
    }

    // ---------------- closest point, perpendiculars, tangents ----------------

    /// The point of the curve nearest `p`, with its parameter. `None` for an empty or
    /// non-finite curve.
    pub fn closest(&self, p: Vec2) -> Option<CurvePoint> {
        if !p.is_finite() {
            return None;
        }
        let c = match &self.kind {
            CurveKind::Seg(s) => s.closest(p),
            CurveKind::Circle(c) => c.closest(p),
            _ if self.pieces.is_empty() => {
                let c = self.fallback.iter().map(|s| s.closest(p)).min_by(|a, b| a.dist(p).total_cmp(&b.dist(p)))?;
                return Some(CurvePoint { t: self.fallback_param(c), point: c });
            }
            _ => return self.closest_on_pieces(p),
        };
        c.is_finite().then(|| CurvePoint { t: self.param_at(c, None), point: c })
    }

    fn fallback_param(&self, c: Vec2) -> f64 {
        match &self.kind {
            CurveKind::Ellipse(e) => ellipse_param(e, c),
            CurveKind::Spline(s) => {
                let (lo, hi) = s.domain();
                let n = self.fallback.len().max(1) as f64;
                let i = self.fallback.iter().position(|sg| sg.closest(c).dist(c) <= 1e-12 * (1.0 + c.len())).unwrap_or(0) as f64;
                lo + (hi - lo) * i / n
            }
            _ => 0.0,
        }
    }

    fn closest_on_pieces(&self, p: Vec2) -> Option<CurvePoint> {
        let scale = self.pieces.iter().map(|pc| far_dist(&pc.bounds, p)).fold(0.0, f64::max);
        if !(scale.is_finite() && scale > 0.0) {
            // The curve is a single point at `p` (or not finite).
            let pc = self.pieces.first()?;
            let c = pc.point(0.0);
            return c.is_finite().then(|| CurvePoint { t: self.param_at(c, Some(pc.t0)), point: c });
        }
        // Visit pieces nearest first; a piece whose bounds are farther than the best so far
        // cannot hold a nearer point.
        let mut order: Vec<(f64, &Piece)> = self.pieces.iter().map(|pc| (near_dist(&pc.bounds, p), pc)).collect();
        order.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut best: Option<(f64, CurvePoint)> = None;
        let consider = |pc: &Piece, s: f64, best: &mut Option<(f64, CurvePoint)>| {
            let c = pc.point(s);
            let d = c.dist(p);
            if c.is_finite() && best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                *best = Some((d, CurvePoint { t: pc.param(s), point: c }));
            }
        };
        for (lb, pc) in order {
            if best.as_ref().is_some_and(|(bd, _)| lb > *bd) {
                break;
            }
            consider(pc, 0.0, &mut best);
            consider(pc, 1.0, &mut best);
            for s in point_roots(pc, p, scale, Cond::Perpendicular).unwrap_or_default() {
                consider(pc, s, &mut best);
            }
        }
        let (_, mut cp) = best?;
        cp.t = self.param_at(cp.point, Some(cp.t));
        Some(cp)
    }

    /// The feet of the perpendiculars from `p` to the curve (points where `p − C` is normal to
    /// the curve), excluding `p` itself. Empty where every point qualifies (a circle seen from
    /// its centre).
    pub fn perpendiculars(&self, p: Vec2) -> Vec<CurvePoint> {
        self.point_condition(p, Cond::Perpendicular)
    }

    /// The points where lines from `p` touch the curve.
    pub fn tangents(&self, p: Vec2) -> Vec<CurvePoint> {
        if let CurveKind::Ellipse(e) = &self.kind
            && !self.pieces.is_empty()
        {
            return ellipse_tangents(e, p).into_iter().map(|q| CurvePoint { t: ellipse_param(e, q), point: q }).collect();
        }
        self.point_condition(p, Cond::Tangent)
    }

    fn point_condition(&self, p: Vec2, cond: Cond) -> Vec<CurvePoint> {
        if !p.is_finite() {
            return Vec::new();
        }
        let pts: Vec<Vec2> = match (&self.kind, cond) {
            (CurveKind::Seg(Segment::Line(l)), Cond::Perpendicular) => {
                let t = l.param_of(p);
                if (-EPS..=1.0 + EPS).contains(&t) { vec![l.at(t)] } else { Vec::new() }
            }
            (CurveKind::Seg(Segment::Line(_)), Cond::Tangent) => Vec::new(),
            (CurveKind::Seg(Segment::Arc { arc, .. }), _) => circle_condition(&Circle::new(arc.center, arc.radius), p, cond)
                .into_iter()
                .filter(|q| arc.contains_angle(arc.center.angle_to(*q)))
                .collect(),
            (CurveKind::Circle(c), _) => circle_condition(c, p, cond),
            _ if self.pieces.is_empty() => return Vec::new(),
            _ => {
                let scale = self.pieces.iter().map(|pc| far_dist(&pc.bounds, p)).fold(0.0, f64::max);
                if !(scale.is_finite() && scale > 0.0) {
                    return Vec::new();
                }
                let mut out: Vec<CurvePoint> = Vec::new();
                for pc in &self.pieces {
                    // `None`: the condition holds along the whole piece (no particular point).
                    let Some(rs) = point_roots(pc, p, scale, cond) else { continue };
                    for s in rs {
                        let q = pc.point(s);
                        if q.is_finite() && q.dist(p) > REL_TOL * scale && !out.iter().any(|o| o.point.near(q, REL_TOL * scale)) {
                            out.push(CurvePoint { t: self.param_at(q, Some(pc.param(s))), point: q });
                        }
                    }
                }
                return out;
            }
        };
        pts.into_iter().filter(|q| q.is_finite()).map(|q| CurvePoint { t: self.param_at(q, None), point: q }).collect()
    }

    // ---------------- intersections ----------------

    /// All intersections with `other`, tangencies included (each once).
    pub fn intersect(&self, other: &Curve) -> Vec<CurveHit> {
        self.intersect_in(other, None)
    }

    /// The intersections with `other` inside `window` (all when `None`). The window also limits
    /// the work: only pieces that reach into it are searched (object snaps near the cursor).
    pub fn intersect_in(&self, other: &Curve, window: Option<&Bounds2>) -> Vec<CurveHit> {
        let mut hits = self.intersect_raw(other, window);
        if let Some(w) = window {
            let pad = REL_TOL * (w.width() + w.height() + w.min.len() + w.max.len());
            let w = w.expand(pad);
            hits.retain(|h| w.contains(h.point));
        }
        hits.retain(|h| h.point.is_finite() && h.t1.is_finite() && h.t2.is_finite());
        let scale = hit_scale(self, other);
        let mut out: Vec<CurveHit> = Vec::new();
        for h in hits {
            if !out.iter().any(|o| o.point.near(h.point, REL_TOL * scale)) {
                out.push(h);
                if out.len() >= MAX_HITS {
                    break;
                }
            }
        }
        out
    }

    fn intersect_raw(&self, other: &Curve, window: Option<&Bounds2>) -> Vec<CurveHit> {
        let (a, b) = (self, other);
        if !a.bounds().expand(REL_TOL * hit_scale(a, b)).intersects(&b.bounds()) {
            return Vec::new();
        }
        let swap = |hs: Vec<CurveHit>| hs.into_iter().map(|h| CurveHit { point: h.point, t1: h.t2, t2: h.t1 }).collect::<Vec<_>>();
        // Fallback polygons: segment by segment, as before exact pieces existed.
        if !a.fallback.is_empty() || !b.fallback.is_empty() {
            return a.polygon_hits(b);
        }
        match (a.as_conic(), b.as_conic()) {
            (Some(_), Some(_)) if a.rank() <= 1 && b.rank() <= 1 => a.segment_hits(b),
            (Some(ca), _) if b.rank() >= a.rank() => conic_hits(a, &ca, b, window),
            (_, Some(cb)) => swap(conic_hits(b, &cb, a, window)),
            _ => spline_hits(a, b, window),
        }
    }

    /// Lines, arcs and circles against each other: the closed forms.
    fn segment_hits(&self, other: &Curve) -> Vec<CurveHit> {
        let (Some(sa), Some(sb)) = (self.seg_like(), other.seg_like()) else { return Vec::new() };
        intersect_ext(&sa, &sb, false).into_iter().map(|p| CurveHit { point: p, t1: self.param_at(p, None), t2: other.param_at(p, None) }).collect()
    }

    fn seg_like(&self) -> Option<Segment> {
        match &self.kind {
            CurveKind::Seg(s) => Some(*s),
            CurveKind::Circle(c) => Some(Segment::Arc { arc: Arc::new(c.center, c.radius, 0.0, 0.0), ccw: true }),
            _ => None,
        }
    }

    /// Fallback: both curves as line/arc segments.
    fn polygon_hits(&self, other: &Curve) -> Vec<CurveHit> {
        let segs = |c: &Curve| if c.fallback.is_empty() { c.seg_like().into_iter().collect::<Vec<_>>() } else { c.fallback.clone() };
        let (sa, sb) = (segs(self), segs(other));
        let mut out = Vec::new();
        for x in &sa {
            for y in &sb {
                for p in intersect_ext(x, y, false) {
                    out.push(CurveHit { point: p, t1: self.loose_param(p), t2: other.loose_param(p) });
                    if out.len() >= MAX_HITS {
                        return out;
                    }
                }
            }
        }
        out
    }

    fn loose_param(&self, p: Vec2) -> f64 {
        if self.fallback.is_empty() { self.param_at(p, None) } else { self.fallback_param(p) }
    }

    /// 0 line, 1 circle or arc, 2 ellipse, 3 spline: the lower one is used implicitly.
    fn rank(&self) -> u8 {
        match &self.kind {
            CurveKind::Seg(Segment::Line(_)) => 0,
            CurveKind::Seg(Segment::Arc { .. }) | CurveKind::Circle(_) => 1,
            CurveKind::Ellipse(_) => 2,
            CurveKind::Spline(_) => 3,
        }
    }

    fn as_conic(&self) -> Option<Conic> {
        match &self.kind {
            CurveKind::Seg(Segment::Line(l)) => {
                let d = l.b - l.a;
                (d.len() > 0.0 && d.is_finite() && l.a.is_finite()).then_some(Conic::Line { a: l.a, dir: d })
            }
            CurveKind::Seg(Segment::Arc { arc, .. }) => Conic::circle(arc.center, arc.radius),
            CurveKind::Circle(c) => Conic::circle(c.center, c.radius),
            CurveKind::Ellipse(e) => {
                let (u, v) = (e.major, e.minor());
                (u.cross(v).abs() > 0.0 && u.is_finite() && v.is_finite() && e.center.is_finite()).then_some(Conic::Ellipse { c: e.center, u, v })
            }
            CurveKind::Spline(_) => None,
        }
    }

    /// The curve's own pieces, or a conic's (a line as one linear piece, an arc or circle as a
    /// rational quadratic spline).
    fn as_pieces(&self) -> Cow<'_, [Piece]> {
        if !self.pieces.is_empty() {
            return Cow::Borrowed(&self.pieces);
        }
        Cow::Owned(match &self.kind {
            CurveKind::Seg(Segment::Line(l)) => Piece::new(vec![(l.a, 1.0), (l.b, 1.0)], 0.0, 1.0).into_iter().collect(),
            CurveKind::Seg(Segment::Arc { arc, .. }) => {
                Spline::from_conic_arc(arc.center, Vec2::X * arc.radius, Vec2::Y * arc.radius, arc.start, arc.sweep())
                    .and_then(|s| spline_pieces(&s))
                    .unwrap_or_default()
            }
            CurveKind::Circle(c) => {
                Spline::from_conic_arc(c.center, Vec2::X * c.radius, Vec2::Y * c.radius, 0.0, TAU).and_then(|s| spline_pieces(&s)).unwrap_or_default()
            }
            _ => Vec::new(),
        })
    }

    /// Whether a point found on the conic's full curve lies on this (bounded) curve.
    fn holds(&self, p: Vec2, t: f64) -> bool {
        match &self.kind {
            CurveKind::Seg(Segment::Line(l)) => {
                let slack = REL_TOL.max(EPS / l.len().max(1e-300));
                (-slack..=1.0 + slack).contains(&t)
            }
            CurveKind::Seg(Segment::Arc { arc, .. }) => arc.contains_angle(arc.center.angle_to(p)),
            CurveKind::Circle(_) => true,
            CurveKind::Ellipse(e) => e.is_full() || angle_in_sweep(t, e.start, e.end) || near_ellipse_end(e, p),
            CurveKind::Spline(_) => true,
        }
    }
}

fn chords(pts: &[Vec2]) -> Vec<Segment> {
    pts.windows(2).filter_map(|w| Some(Segment::Line(Line::new(*w.first()?, *w.get(1)?)))).collect()
}

fn fallback_bounds(segs: &[Segment]) -> Bounds2 {
    segs.iter().fold(Bounds2::default(), |b, s| b.union(&s.bounds()))
}

/// Size of the problem two curves pose (for relative tolerances).
fn hit_scale(a: &Curve, b: &Curve) -> f64 {
    let size = |c: &Curve| {
        let bb = c.bounds();
        if bb.is_empty() { 0.0 } else { bb.width().hypot(bb.height()) }
    };
    let s = size(a).max(size(b));
    if s.is_finite() && s > 0.0 { s } else { 1.0 }
}

/// Distance from `p` to the farthest corner of `b`.
fn far_dist(b: &Bounds2, p: Vec2) -> f64 {
    b.corners().iter().map(|c| c.dist(p)).fold(0.0, f64::max)
}

/// Distance from `p` to the box `b` (zero inside).
fn near_dist(b: &Bounds2, p: Vec2) -> f64 {
    let dx = (b.min.x - p.x).max(0.0).max(p.x - b.max.x);
    let dy = (b.min.y - p.y).max(0.0).max(p.y - b.max.y);
    dx.hypot(dy)
}

/// The parameter of a point on an ellipse: its angle in the frame of the axes.
pub(crate) fn ellipse_param(e: &Ellipse, p: Vec2) -> f64 {
    let (u, v) = (e.major, e.minor());
    let (uu, vv) = (u.len2(), v.len2());
    if !(uu > 0.0 && vv > 0.0) {
        return 0.0;
    }
    let r = p - e.center;
    norm_angle((r.dot(v) / vv).atan2(r.dot(u) / uu))
}

/// A point within tolerance of one end of an elliptical arc (a hit at the end).
fn near_ellipse_end(e: &Ellipse, p: Vec2) -> bool {
    let tol = REL_TOL * e.major.len().max(1e-300);
    e.at_param(e.start).near(p, tol) || e.at_param(e.end).near(p, tol)
}

/// The points where lines from `p` touch an ellipse arc: exact, by mapping the ellipse onto the
/// unit circle (an affine map keeps tangency).
fn ellipse_tangents(e: &Ellipse, p: Vec2) -> Vec<Vec2> {
    let (u, v) = (e.major, e.minor());
    let (a2, b2) = (u.len2(), v.len2());
    if !(a2 > 0.0 && b2 > 0.0 && a2.is_finite() && b2.is_finite()) {
        return Vec::new();
    }
    let r = p - e.center;
    let local = Vec2::new(r.dot(u) / a2, r.dot(v) / b2);
    Circle::new(Vec2::ZERO, 1.0)
        .tangent_points(local)
        .into_iter()
        .map(|q| q.angle())
        .filter(|t| t.is_finite() && angle_in_sweep(*t, e.start, e.end))
        .map(|t| e.at_param(t))
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cond {
    Perpendicular,
    Tangent,
}

/// Perpendicular feet (radial points) or tangent points of a circle from `p`.
fn circle_condition(c: &Circle, p: Vec2, cond: Cond) -> Vec<Vec2> {
    match cond {
        Cond::Tangent => c.tangent_points(p),
        Cond::Perpendicular => {
            let d = p - c.center;
            if d.len() <= EPS * (1.0 + c.radius) {
                return Vec::new();
            }
            let u = d.normalized();
            vec![c.center + u * c.radius, c.center - u * c.radius]
        }
    }
}

/// Local parameters on `pc` where `(C − p)·C'` (perpendicular) or `(C − p)×C'` (tangent) is
/// zero. Works in a frame centred on `p` and scaled by `scale`. `None` when the condition holds
/// along the whole piece.
fn point_roots(pc: &Piece, p: Vec2, scale: f64, cond: Cond) -> Option<Vec<f64>> {
    let local = pc.mapped(|q| (q - p) / scale);
    let xs: Vec<f64> = local.iter().map(|h| h.0.x).collect();
    let ys: Vec<f64> = local.iter().map(|h| h.0.y).collect();
    let ws: Vec<f64> = local.iter().map(|h| h.1).collect();
    // C' ∝ X'W − XW' (the rational derivative's numerator), degree 2p − 1.
    let dw = bdiff(&ws);
    let bx = badd(&bmul(&bdiff(&xs), &ws), &bmul(&xs, &dw), -1.0);
    let by = badd(&bmul(&bdiff(&ys), &ws), &bmul(&ys, &dw), -1.0);
    let coef = match cond {
        Cond::Perpendicular => badd(&bmul(&xs, &bx), &bmul(&ys, &by), 1.0),
        Cond::Tangent => badd(&bmul(&xs, &by), &bmul(&ys, &bx), -1.0),
    };
    let mag = |v: &[f64]| v.iter().fold(0.0f64, |m, x| m.max(x.abs()));
    let gscale = mag(&xs).max(mag(&ys)) * mag(&bx).max(mag(&by));
    if !(gscale.is_finite() && gscale > 0.0) {
        return Some(Vec::new());
    }
    let eval = |s: f64| {
        let (h, h1, h2) = derivs(&local, s);
        let (x, w, x1, w1, x2, w2) = (h.0, h.1, h1.0, h1.1, h2.0, h2.1);
        let b = x1 * w - x * w1;
        let b1 = x2 * w - x * w2;
        match cond {
            Cond::Perpendicular => (x.dot(b), x1.dot(b) + x.dot(b1)),
            Cond::Tangent => (x.cross(b), x1.cross(b) + x.cross(b1)),
        }
    };
    match roots(&coef, 1e-12 * gscale, &eval) {
        Roots::Some(v) => Some(v),
        Roots::Everywhere => None,
    }
}

/// A line, circle or ellipse as an implicit equation.
#[derive(Clone, Copy, Debug)]
enum Conic {
    /// The infinite line through `a` along `dir`.
    Line { a: Vec2, dir: Vec2 },
    /// `c + u cos θ + v sin θ` with `u ⟂ v` (a circle has |u| = |v|).
    Ellipse { c: Vec2, u: Vec2, v: Vec2 },
}

impl Conic {
    fn circle(c: Vec2, r: f64) -> Option<Conic> {
        (c.is_finite() && r.is_finite() && r > 0.0).then(|| Conic::Ellipse { c, u: Vec2::X * r, v: Vec2::Y * r })
    }

    /// A map into the frame where the conic is `y = 0` (a line, scaled by `scale`) or the unit
    /// circle, and the equation value and derivative from a homogeneous point there.
    fn frame(&self, near: Vec2, scale: f64) -> impl Fn(Vec2) -> Vec2 + use<> {
        let me = *self;
        move |q: Vec2| match me {
            Conic::Line { a, dir } => {
                let d = dir.normalized();
                // Measured from the point of the line nearest the piece, so a long line far
                // away does not cost precision.
                let o = a + d * (near - a).dot(d);
                let r = q - o;
                Vec2::new(r.dot(d), r.cross(d)) / scale
            }
            Conic::Ellipse { c, u, v } => {
                let r = q - c;
                Vec2::new(r.dot(u) / u.len2(), r.dot(v) / v.len2())
            }
        }
    }

    fn is_line(&self) -> bool {
        matches!(self, Conic::Line { .. })
    }
}

/// Intersections of `curve` (any kind; its pieces) with the implicit `conic` of `implicit`.
fn conic_hits(implicit: &Curve, conic: &Conic, curve: &Curve, window: Option<&Bounds2>) -> Vec<CurveHit> {
    let pieces = curve.as_pieces();
    let mut out = Vec::new();
    let reach = implicit.bounds();
    for pc in pieces.iter() {
        if window.is_some_and(|w| !pc.bounds.intersects(w)) {
            continue;
        }
        let tol = REL_TOL * pc.bounds.width().hypot(pc.bounds.height()).max(1e-300);
        if !conic.is_line() && !pc.bounds.expand(tol).intersects(&reach.expand(REL_TOL * reach.width().hypot(reach.height()))) {
            continue;
        }
        for s in piece_on_conic(pc, conic) {
            let p = pc.point(s);
            let t_curve = curve.param_at(p, Some(pc.param(s)));
            let t_imp = implicit.param_at(p, None);
            if implicit.holds(p, t_imp) && curve.holds(p, t_curve) {
                out.push(CurveHit { point: p, t1: t_imp, t2: t_curve });
                if out.len() >= MAX_HITS {
                    return out;
                }
            }
        }
    }
    out
}

/// Local parameters where a piece meets a conic: roots of the conic's equation along it.
fn piece_on_conic(pc: &Piece, conic: &Conic) -> Vec<f64> {
    let size = pc.bounds.width().hypot(pc.bounds.height());
    let scale = if size.is_finite() && size > 0.0 { size } else { 1.0 };
    let local = pc.mapped(conic.frame(pc.bounds.center(), scale));
    if local.iter().any(|h| !h.0.is_finite()) {
        return Vec::new();
    }
    let ys: Vec<f64> = local.iter().map(|h| h.0.y).collect();
    let (coef, eps): (Vec<f64>, f64) = match conic {
        // y = 0 in the line's frame: the signed distance over the piece size, times W.
        Conic::Line { .. } => (ys, REL_TOL),
        // x² + y² − W² = 0 on the unit circle: about twice the relative distance, times W².
        Conic::Ellipse { .. } => {
            let xs: Vec<f64> = local.iter().map(|h| h.0.x).collect();
            let ws: Vec<f64> = local.iter().map(|h| h.1).collect();
            (badd(&badd(&bmul(&xs, &xs), &bmul(&ys, &ys), 1.0), &bmul(&ws, &ws), -1.0), 2.0 * REL_TOL)
        }
    };
    let is_line = conic.is_line();
    let eval = |s: f64| {
        let (h, h1, _) = derivs(&local, s);
        if is_line { (h.0.y, h1.0.y) } else { (h.0.len2() - h.1 * h.1, 2.0 * (h.0.dot(h1.0) - h.1 * h1.1)) }
    };
    match roots(&coef, eps, &eval) {
        Roots::Some(v) => v,
        // The piece lies on the conic: overlapping curves have no isolated crossing.
        Roots::Everywhere => Vec::new(),
    }
}

// ---------------- spline against spline ----------------

fn spline_hits(a: &Curve, b: &Curve, window: Option<&Bounds2>) -> Vec<CurveHit> {
    let (pa, pb) = (a.as_pieces(), b.as_pieces());
    let scale = hit_scale(a, b);
    let mut budget = MAX_CURVE_NODES;
    let mut out: Vec<CurveHit> = Vec::new();
    let tol = REL_TOL * scale;
    for x in pa.iter() {
        if window.is_some_and(|w| !x.bounds.expand(tol).intersects(w)) {
            continue;
        }
        for y in pb.iter() {
            if !x.bounds.expand(tol).intersects(&y.bounds) || window.is_some_and(|w| !y.bounds.expand(tol).intersects(w)) {
                continue;
            }
            for (s, t) in piece_pair(x, y, scale, window, &mut budget) {
                let p = x.point(s);
                out.push(CurveHit { point: p, t1: a.param_at(p, Some(x.param(s))), t2: b.param_at(p, Some(y.param(t))) });
            }
            if budget == 0 || out.len() >= MAX_HITS {
                return out;
            }
        }
    }
    out
}

/// One node of the spline/spline search: the two sub-nets and their local parameter ranges.
struct Node {
    a: Vec<H>,
    a0: f64,
    a1: f64,
    b: Vec<H>,
    b0: f64,
    b1: f64,
    depth: u32,
}

/// Intersections of two pieces as (local parameter on `x`, on `y`).
fn piece_pair(x: &Piece, y: &Piece, scale: f64, window: Option<&Bounds2>, budget: &mut usize) -> Vec<(f64, f64)> {
    // Work in a frame centred between the pieces and scaled to the problem: tolerances are then
    // relative, and large coordinates keep their precision.
    let o = x.bounds.union(&y.bounds).center();
    let to_local = |q: Vec2| (q - o) / scale;
    let (la, lb) = (x.mapped(to_local), y.mapped(to_local));
    let win = window.map(|w| Bounds2::new(to_local(w.min), to_local(w.max)).expand(REL_TOL));
    let mut out: Vec<(f64, f64)> = Vec::new();
    let mut stack = vec![Node { a: la.clone(), a0: 0.0, a1: 1.0, b: lb.clone(), b0: 0.0, b1: 1.0, depth: 0 }];
    let mut nodes = 0usize;
    while let Some(n) = stack.pop() {
        if *budget == 0 || nodes >= MAX_PAIR_NODES || out.len() >= MAX_HITS {
            break;
        }
        *budget -= 1;
        nodes += 1;
        let (ba, bb) = (net_bounds(&n.a), net_bounds(&n.b));
        if !ba.expand(REL_TOL).intersects(&bb) || win.is_some_and(|w| !ba.intersects(&w) || !bb.intersects(&w)) {
            continue;
        }
        let (sa, sb) = (ba.width().hypot(ba.height()), bb.width().hypot(bb.height()));
        if !(sa.is_finite() && sb.is_finite()) {
            continue;
        }
        let leaf = (sa <= LEAF && sb <= LEAF) || n.depth >= MAX_PAIR_DEPTH;
        let seed = (0.5 * (n.a0 + n.a1), 0.5 * (n.b0 + n.b1));
        // Disjoint derivative cones: at most one crossing here, and Newton's method is well
        // conditioned on it.
        if transversal(&n.a, &n.b)
            && let Some((s, t)) = newton(&la, &lb, seed, (n.a0, n.a1), (n.b0, n.b1))
        {
            push_pair(&mut out, &la, s, t);
            continue;
        }
        if leaf {
            // Tiny pieces with overlapping cones: a tangency (parallel tangents at a common
            // point), else a crossing, else the nearest points if they touch.
            let found =
                touch_newton(&la, &lb, seed).or_else(|| newton(&la, &lb, seed, (n.a0, n.a1), (n.b0, n.b1))).or_else(|| closest_pair(&la, &lb, seed));
            if let Some((s, t)) = found {
                push_pair(&mut out, &la, s, t);
            }
            continue;
        }
        // Split the larger net in two.
        if sa >= sb {
            let (l, r) = split(&n.a, 0.5);
            let m = 0.5 * (n.a0 + n.a1);
            stack.push(Node { a: r, a0: m, a1: n.a1, b: n.b.clone(), b0: n.b0, b1: n.b1, depth: n.depth + 1 });
            stack.push(Node { a: l, a0: n.a0, a1: m, b: n.b, b0: n.b0, b1: n.b1, depth: n.depth + 1 });
        } else {
            let (l, r) = split(&n.b, 0.5);
            let m = 0.5 * (n.b0 + n.b1);
            stack.push(Node { a: n.a.clone(), a0: n.a0, a1: n.a1, b: r, b0: m, b1: n.b1, depth: n.depth + 1 });
            stack.push(Node { a: n.a, a0: n.a0, a1: n.a1, b: l, b0: n.b0, b1: m, depth: n.depth + 1 });
        }
    }
    out
}

fn push_pair(out: &mut Vec<(f64, f64)>, la: &[H], s: f64, t: f64) {
    let p = proj(derivs(la, s).0);
    if !out.iter().any(|(s0, _)| proj(derivs(la, *s0).0).near(p, REL_TOL)) {
        out.push((s, t));
    }
}

/// The derivative numerator `X'W − XW'` of a net in Bernstein form (vector coefficients).
fn hodograph(h: &[H]) -> Vec<Vec2> {
    let xs: Vec<f64> = h.iter().map(|q| q.0.x).collect();
    let ys: Vec<f64> = h.iter().map(|q| q.0.y).collect();
    let ws: Vec<f64> = h.iter().map(|q| q.1).collect();
    let dw = bdiff(&ws);
    let bx = badd(&bmul(&bdiff(&xs), &ws), &bmul(&xs, &dw), -1.0);
    let by = badd(&bmul(&bdiff(&ys), &ws), &bmul(&ys, &dw), -1.0);
    bx.into_iter().zip(by).map(|(x, y)| Vec2::new(x, y)).collect()
}

/// True when every tangent direction of `a` crosses every one of `b` on the same side: the
/// pieces are never parallel, so they cross at most once.
fn transversal(a: &[H], b: &[H]) -> bool {
    let (da, db) = (hodograph(a), hodograph(b));
    let mut sign = 0.0f64;
    for u in &da {
        for v in &db {
            let c = u.cross(*v);
            let lim = 1e-12 * u.len() * v.len();
            if c.is_nan() || lim.is_nan() || c.abs() <= lim || (sign != 0.0 && (c > 0.0) != (sign > 0.0)) {
                return false;
            }
            sign = c;
        }
    }
    sign != 0.0
}

/// Newton's method on `A(s) = B(t)`, kept in [0, 1]²; accepted when it converges inside the
/// node's ranges (slightly widened).
fn newton(la: &[H], lb: &[H], seed: (f64, f64), ra: (f64, f64), rb: (f64, f64)) -> Option<(f64, f64)> {
    let (mut s, mut t) = seed;
    for _ in 0..NEWTON_STEPS {
        let (pa, da, _) = crate::bezier::euclid(derivs(la, s));
        let (pb, db, _) = crate::bezier::euclid(derivs(lb, t));
        let f = pa - pb;
        let det = db.cross(da);
        if !(det.abs() > 1e-300 && f.is_finite()) {
            return None;
        }
        // Solve [da, −db]·(ds, dt) = −f.
        let ds = f.cross(db) / det;
        let dt = f.cross(da) / det;
        let (ns, nt) = ((s + ds).clamp(0.0, 1.0), (t + dt).clamp(0.0, 1.0));
        if !(ns.is_finite() && nt.is_finite()) {
            return None;
        }
        let small = (ns - s).abs() <= 1e-15 && (nt - t).abs() <= 1e-15;
        (s, t) = (ns, nt);
        if small {
            break;
        }
    }
    let gap = proj(derivs(la, s).0).dist(proj(derivs(lb, t).0));
    let wa = (ra.1 - ra.0) * 1e-6 + 1e-12;
    let wb = (rb.1 - rb.0) * 1e-6 + 1e-12;
    (gap <= REL_TOL * 1e-3 && s >= ra.0 - wa && s <= ra.1 + wa && t >= rb.0 - wb && t <= rb.1 + wb).then_some((s, t))
}

/// A tangency of two pieces near `seed`: Newton's method on "the tangents are parallel and
/// `B(t)` is the foot of `A(s)`", which stays well conditioned where `A(s) = B(t)` does not.
/// Kept when the two points coincide within the tolerance.
fn touch_newton(la: &[H], lb: &[H], seed: (f64, f64)) -> Option<(f64, f64)> {
    let (mut s, mut t) = seed;
    for _ in 0..NEWTON_STEPS {
        let (pa, da, dda) = crate::bezier::euclid(derivs(la, s));
        let (pb, db, ddb) = crate::bezier::euclid(derivs(lb, t));
        let f = pa - pb;
        let (g1, g2) = (da.cross(db), f.dot(db));
        let (j11, j12) = (dda.cross(db), da.cross(ddb));
        let (j21, j22) = (da.dot(db), f.dot(ddb) - db.len2());
        let det = j11 * j22 - j12 * j21;
        if !(det.is_finite() && det.abs() > 1e-300) {
            return None;
        }
        let ds = -(g1 * j22 - j12 * g2) / det;
        let dt = -(j11 * g2 - g1 * j21) / det;
        let (ns, nt) = ((s + ds).clamp(0.0, 1.0), (t + dt).clamp(0.0, 1.0));
        if !(ns.is_finite() && nt.is_finite()) {
            return None;
        }
        let small = (ns - s).abs() <= 1e-15 && (nt - t).abs() <= 1e-15;
        (s, t) = (ns, nt);
        if small {
            break;
        }
    }
    let gap = proj(derivs(la, s).0).dist(proj(derivs(lb, t).0));
    (gap <= REL_TOL).then_some((s, t))
}

/// The closest pair of points of two pieces near `seed` (damped Gauss–Newton on the distance);
/// kept when they touch within the tolerance.
fn closest_pair(la: &[H], lb: &[H], seed: (f64, f64)) -> Option<(f64, f64)> {
    let (mut s, mut t) = seed;
    let mut lambda = 1e-12;
    let dist2 = |s: f64, t: f64| (proj(derivs(la, s).0) - proj(derivs(lb, t).0)).len2();
    let mut cur = dist2(s, t);
    for _ in 0..NEWTON_STEPS {
        let (pa, da, _) = crate::bezier::euclid(derivs(la, s));
        let (pb, db, _) = crate::bezier::euclid(derivs(lb, t));
        let f = pa - pb;
        // J = [da, −db]; normal equations (JᵀJ + λ·diag) δ = −Jᵀf.
        let (j11, j12, j22) = (da.len2(), -da.dot(db), db.len2());
        let (g1, g2) = (da.dot(f), -db.dot(f));
        let (a11, a22) = (j11 * (1.0 + lambda) + 1e-300, j22 * (1.0 + lambda) + 1e-300);
        let det = a11 * a22 - j12 * j12;
        if !(det.is_finite() && det.abs() > 0.0) {
            break;
        }
        let ds = -(a22 * g1 - j12 * g2) / det;
        let dt = -(a11 * g2 - j12 * g1) / det;
        let (ns, nt) = ((s + ds).clamp(0.0, 1.0), (t + dt).clamp(0.0, 1.0));
        let next = dist2(ns, nt);
        if !next.is_finite() {
            break;
        }
        if next <= cur {
            let done = (ns - s).abs() <= 1e-16 && (nt - t).abs() <= 1e-16;
            (s, t, cur) = (ns, nt, next);
            lambda = (lambda * 0.1).max(1e-15);
            if done || cur == 0.0 {
                break;
            }
        } else {
            lambda *= 10.0;
            if lambda > 1e12 {
                break;
            }
        }
    }
    (cur.sqrt() <= REL_TOL).then_some((s, t))
}

impl Spline {
    /// The parameter of the point of the spline nearest `p` (exact: the roots of the
    /// perpendicular condition on each Bézier piece). `None` for an invalid spline.
    pub fn closest_param(&self, p: Vec2) -> Option<f64> {
        Curve::spline(self).closest(p).map(|c| c.t)
    }

    /// The point of the spline nearest `p`.
    pub fn closest(&self, p: Vec2) -> Option<Vec2> {
        Curve::spline(self).closest(p).map(|c| c.point)
    }
}
