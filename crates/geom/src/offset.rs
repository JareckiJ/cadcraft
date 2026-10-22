//! Offsets of ellipses and splines as cubic splines within a tolerance.
//!
//! The offset of an ellipse is not an ellipse (nor any rational curve), and a spline's offset is
//! not a spline, so both are fitted: the exact offset `O = C + δ·N` and its derivative
//! `O' = C'·(1 − δκ)` (κ the signed curvature) give a cubic Hermite piece over each parameter
//! interval; the piece is compared with the exact offset at seven interior parameters and the
//! interval halved until the difference is within the tolerance. Neighbouring pieces share their
//! end point and derivative, so the result is a C1 cubic B-spline with double interior knots
//! (periodic when the source is closed and smooth). A spline's knots where it has a corner are
//! kept as corners, the gap on the outside of the corner bridged by a round arc (fitted the same
//! way), the overlap on the inside by a straight segment.
//!
//! Every loop is bounded (`MAX_SEGMENTS`, `MAX_HALVINGS`); degenerate input gives `None`.

use crate::bezier::spline_pieces;
use crate::{Bounds2, Ellipse, PI, Spline, TAU, Vec2};

/// Default fit tolerance, relative to the size of the curve plus the offset distance.
pub const OFFSET_REL_TOL: f64 = 1e-8;
/// Most cubic pieces one offset produces.
const MAX_SEGMENTS: usize = 4096;
/// Most halvings of one parameter interval.
const MAX_HALVINGS: u32 = 30;
/// Interior parameters at which a fitted piece is checked against the exact offset.
const CHECKS: usize = 7;

/// A cubic Bézier piece of the offset and the parameter span it covers.
#[derive(Clone, Copy, Debug)]
struct Cubic {
    p: [Vec2; 4],
    span: f64,
}

/// Offset point and its derivative with respect to the curve parameter.
type Sample = (Vec2, Vec2);

fn bez(p: &[Vec2; 4], u: f64) -> Vec2 {
    let v = 1.0 - u;
    let [a, b, c, d] = *p;
    a * (v * v * v) + b * (3.0 * v * v * u) + c * (3.0 * v * u * u) + d * (u * u * u)
}

/// The point `δ` to the left of a curve point (derivatives `d1`, `d2`), and its derivative.
/// `vanish`: speeds at or below it count as zero (the tangent is then the second derivative's).
fn offset_point(c: Vec2, d1: Vec2, d2: Vec2, delta: f64, vanish: f64) -> Option<Sample> {
    let speed = d1.len();
    if speed.is_nan() || vanish.is_nan() || speed <= vanish {
        let t = d2.normalized();
        if t == Vec2::ZERO {
            return None;
        }
        let o = c + t.perp() * delta;
        return o.is_finite().then_some((o, Vec2::ZERO));
    }
    let kappa = d1.cross(d2) / (speed * speed * speed);
    let o = c + d1.perp() * (delta / speed);
    let od = d1 * (1.0 - delta * kappa);
    (o.is_finite() && od.is_finite()).then_some((o, od))
}

/// The cubic over [x0, x1] between two offset samples: tangent directions from the samples,
/// handle lengths chosen so the cubic also passes through the exact midpoint (exact to sixth
/// order on a circular arc); Hermite handles (from the derivatives) where that has no good
/// solution (an inflection, a cusp, nearly straight).
fn piece(eval: &dyn Fn(f64) -> Option<Sample>, x0: f64, x1: f64, (p0, d0): Sample, (p3, d3): Sample) -> Option<[Vec2; 4]> {
    let h = x1 - x0;
    let hermite = [p0, p0 + d0 * (h / 3.0), p3 - d3 * (h / 3.0), p3];
    let (t0, t3) = (d0.normalized(), d3.normalized());
    let det = t3.cross(t0);
    if t0 == Vec2::ZERO || t3 == Vec2::ZERO || det.is_nan() || det.abs() <= 1e-6 {
        return Some(hermite);
    }
    let m = eval(0.5 * (x0 + x1))?.0;
    // B(1/2) = (p0 + p3)/2 + 3/8·(α·t0 − β·t3).
    let r = (m - (p0 + p3) * 0.5) * (8.0 / 3.0);
    let alpha = t3.cross(r) / det;
    let beta = t0.cross(r) / det;
    let chord = p0.dist(p3);
    if !(alpha > 0.0 && beta > 0.0 && alpha <= 2.0 * chord && beta <= 2.0 * chord) {
        return Some(hermite);
    }
    Some([p0, p0 + t0 * alpha, p3 - t3 * beta, p3])
}

/// Fit `eval` over [a, b] with cubic pieces within `tol`, appending to `out`; neighbouring
/// pieces share their end point and tangent direction.
fn fit(eval: &dyn Fn(f64) -> Option<Sample>, a: f64, b: f64, tol: f64, out: &mut Vec<Cubic>) -> Option<()> {
    if a.is_nan() || b.is_nan() || b <= a {
        return None;
    }
    let mut stack: Vec<(f64, f64, Sample, Sample, u32)> = vec![(a, b, eval(a)?, eval(b)?, 0)];
    while let Some((x0, x1, s0, s3, depth)) = stack.pop() {
        let h = x1 - x0;
        let p = piece(eval, x0, x1, s0, s3)?;
        let mut err = 0.0f64;
        for k in 1..=CHECKS {
            let u = k as f64 / (CHECKS + 1) as f64;
            let q = bez(&p, u);
            let (o, od) = eval(x0 + h * u)?;
            // Distance to the exact offset near there: one Newton step along it removes the
            // difference in parametrisation, which is not an error of the shape.
            let mut e = q.dist(o);
            let l2 = od.len2();
            if l2 > 0.0 {
                let t = (x0 + h * u + (q - o).dot(od) / l2).clamp(x0, x1);
                e = e.min(q.dist(eval(t)?.0));
            }
            err = err.max(e);
        }
        if !err.is_finite() {
            return None;
        }
        let mid = 0.5 * (x0 + x1);
        if err <= tol || depth >= MAX_HALVINGS || out.len() + stack.len() >= MAX_SEGMENTS || !(mid > x0 && mid < x1) {
            out.push(Cubic { p, span: h });
            continue;
        }
        let em = eval(mid)?;
        stack.push((mid, x1, em, s3, depth + 1));
        stack.push((x0, mid, s0, em, depth + 1));
    }
    Some(())
}

/// Lengths of the start and end handles of a cubic.
fn handles(c: &Cubic) -> (f64, f64) {
    (c.p[1].dist(c.p[0]), c.p[3].dist(c.p[2]))
}

/// Where a cubic B-spline with a double knot puts the joint of `a` and `b` (spans `da`, `db`).
fn knot_point(a: &Cubic, b: &Cubic, da: f64, db: f64) -> Vec2 {
    (a.p[2] * db + b.p[1] * da) / (da + db)
}

/// Cubic pieces (with whether each joint to the next is smooth) as a B-spline starting at `t0`.
/// Spans are chosen in proportion to the handle lengths, so a smooth joint is C1 and takes a
/// double knot (the curve passes the joint within `tol`); other joints are corners with triple
/// knots. Periodic when `periodic` asks for it and the seam can be smooth too.
fn assemble(cubics: &[Cubic], c1: &[bool], t0: f64, closed: bool, periodic: bool, tol: f64) -> Option<Spline> {
    let n = cubics.len();
    if n == 0 || c1.len() + 1 != n {
        return None;
    }
    let (first, last) = (cubics.first()?, cubics.last()?);
    let mut flags = c1.to_vec();
    let mut spans = vec![if first.span > 0.0 { first.span } else { 1.0 }];
    for i in 0..n - 1 {
        let (a, b) = (cubics.get(i)?, cubics.get(i + 1)?);
        let prev = *spans.last()?;
        let mut next = if b.span > 0.0 { b.span } else { prev };
        if flags.get(i).copied().unwrap_or(false) {
            let ((_, beta), (alpha, _)) = (handles(a), handles(b));
            let ratio = alpha / beta;
            let ok = ratio.is_finite() && (1e-4..=1e4).contains(&ratio) && knot_point(a, b, prev, prev * ratio).dist(a.p[3]) <= tol;
            if ok {
                next = prev * ratio;
            } else if let Some(f) = flags.get_mut(i) {
                *f = false;
            }
        }
        spans.push(next);
    }
    let mut periodic_spans = None;
    if periodic && n >= 2 && flags.iter().all(|f| *f) {
        // Close the chain of span ratios round the seam: spread the mismatch evenly.
        let ((_, beta), (alpha, _)) = (handles(last), handles(first));
        let need = spans.last()? * alpha / beta;
        let r = need / spans.first()?;
        if r.is_finite() && r > 0.0 {
            let sp: Vec<f64> = spans.iter().enumerate().map(|(i, s)| s * r.powf(-(i as f64) / n as f64)).collect();
            let joints_ok = (0..n).all(|i| {
                let j = (i + 1) % n;
                match (cubics.get(i), cubics.get(j), sp.get(i), sp.get(j)) {
                    (Some(a), Some(b), Some(da), Some(db)) => knot_point(a, b, *da, *db).dist(a.p[3]) <= tol,
                    _ => false,
                }
            });
            if joints_ok && sp.iter().all(|s| s.is_finite() && *s > 0.0) {
                periodic_spans = Some(sp);
            }
        }
    }
    let periodic = periodic_spans.is_some();
    let spans = periodic_spans.unwrap_or(spans);
    let mut u = vec![t0];
    for s in &spans {
        let last = *u.last()?;
        u.push(last + s);
    }
    let (mut control, mut knots);
    if periodic {
        let period = u.last()? - t0;
        control = vec![last.p[2]];
        for c in cubics {
            control.push(c.p[1]);
            control.push(c.p[2]);
        }
        control.push(first.p[1]);
        let before = u.get(n - 1)? - period;
        let after = u.get(1)? + period;
        knots = vec![before, before];
        for x in &u {
            knots.push(*x);
            knots.push(*x);
        }
        knots.push(after);
        knots.push(after);
    } else {
        control = vec![first.p[0]];
        knots = vec![t0; 4];
        for (i, c) in cubics.iter().enumerate() {
            control.push(c.p[1]);
            control.push(c.p[2]);
            let ui = *u.get(i + 1)?;
            if i + 1 == n {
                control.push(c.p[3]);
                knots.extend([ui; 4]);
            } else if flags.get(i).copied().unwrap_or(false) {
                knots.extend([ui; 2]);
            } else {
                control.push(c.p[3]);
                knots.extend([ui; 3]);
            }
        }
    }
    let s = Spline { degree: 3, knots, control, closed, ..Spline::default() };
    (s.is_valid() && s.control.iter().all(|c| c.is_finite())).then_some(s)
}

fn default_tol(tol: f64, size: f64, d: f64) -> Option<f64> {
    let t = if tol.is_finite() && tol > 0.0 { tol } else { OFFSET_REL_TOL * (size + d.abs()) };
    (t.is_finite() && t > 0.0).then_some(t)
}

impl Ellipse {
    /// The offset of the ellipse or elliptical arc by `d` (positive outward, negative inward) as
    /// a cubic spline within `tol` of the exact offset (`tol <= 0`: [`OFFSET_REL_TOL`] of the
    /// size). A full ellipse gives a closed periodic spline. `None` when an inward offset
    /// reaches the minor axis (the curve collapses) or the input is degenerate.
    pub fn offset_spline(&self, d: f64, tol: f64) -> Option<Spline> {
        let (u, v) = (self.major, self.minor());
        let (a, b) = (u.len(), v.len());
        if !(self.center.is_finite() && a.is_finite() && b.is_finite() && a > 0.0 && b > 0.0 && d.is_finite()) {
            return None;
        }
        if !(self.start.is_finite() && self.end.is_finite()) || -d >= a.min(b) {
            return None;
        }
        let tol = default_tol(tol, a, d)?;
        // N is the left normal: inward on a counter-clockwise ellipse.
        let delta = if u.cross(v) > 0.0 { -d } else { d };
        let vanish = 1e-12 * b.min(a);
        let eval = |t: f64| {
            let (c, s) = (t.cos(), t.sin());
            offset_point(self.center + u * c + v * s, v * c - u * s, -(u * c + v * s), delta, vanish)
        };
        let sweep = self.sweep();
        let full = self.is_full();
        let parts = ((sweep / TAU) * 8.0).ceil().clamp(1.0, 8.0) as usize;
        let mut cubics = Vec::new();
        for i in 0..parts {
            let t0 = self.start + sweep * i as f64 / parts as f64;
            let t1 = if i + 1 == parts { self.start + sweep } else { self.start + sweep * (i + 1) as f64 / parts as f64 };
            fit(&eval, t0, t1, tol * 0.5, &mut cubics)?;
        }
        let c1 = vec![true; cubics.len().saturating_sub(1)];
        assemble(&cubics, &c1, self.start, full, full, tol * 0.5)
    }
}

impl Spline {
    /// The offset by `d` to the left of the direction of travel (negative: to the right) as a
    /// cubic spline within `tol` of the exact offset (`tol <= 0`: [`OFFSET_REL_TOL`] of the
    /// size), starting at this spline's start parameter. A closed smooth spline gives a closed
    /// periodic spline. `None` on invalid input.
    pub fn offset(&self, d: f64, tol: f64) -> Option<Spline> {
        if !d.is_finite() {
            return None;
        }
        let pieces = spline_pieces(self)?;
        let bounds = pieces.iter().fold(Bounds2::default(), |b, p| b.union(&p.bounds));
        let size = bounds.width().hypot(bounds.height());
        let tol = default_tol(tol, size, d)?;
        if d == 0.0 {
            return Some(Spline { fit: Vec::new(), fit_opts: Default::default(), ..self.clone() });
        }
        // One run of cubics per Bézier piece (smooth inside), with its end samples.
        struct Run {
            cubics: Vec<Cubic>,
            start: Sample,
            end: Sample,
            corner: Vec2,
            tan_in: Vec2,
            tan_out: Vec2,
        }
        let mut runs: Vec<Run> = Vec::with_capacity(pieces.len());
        for pc in &pieces {
            let h = pc.t1 - pc.t0;
            if !(h > 0.0 && h.is_finite()) {
                continue;
            }
            let vanish = 1e-12 * pc.bounds.width().hypot(pc.bounds.height()).max(1e-300) / h;
            let eval = |t: f64| {
                let (c, d1, d2) = pc.eval3((t - pc.t0) / h);
                offset_point(c, d1 / h, d2 / (h * h), d, vanish)
            };
            let mut cubics = Vec::new();
            fit(&eval, pc.t0, pc.t1, tol * 0.5, &mut cubics)?;
            let (_, t_start, _) = pc.eval3(0.0);
            let (_, t_end, _) = pc.eval3(1.0);
            runs.push(Run { cubics, start: eval(pc.t0)?, end: eval(pc.t1)?, corner: pc.point(1.0), tan_in: t_start, tan_out: t_end });
            if runs.iter().map(|r| r.cubics.len()).sum::<usize>() > MAX_SEGMENTS {
                return None;
            }
        }
        let typical = (self.domain().1 - self.domain().0) / pieces.len().max(1) as f64;
        // Join run i to run j: C1, a corner, or a bridge across the gap at a corner.
        let joint = |a: &Run, b: &Run, out: &mut Vec<Cubic>, c1: &mut Vec<bool>| -> Option<()> {
            let ((pa, da), (pb, db)) = (a.end, b.start);
            let smooth_d = (da - db).len() <= 1e-9 * (da.len() + db.len()) && da.len() + db.len() > 0.0;
            if pa.dist(pb) <= tol {
                c1.push(smooth_d);
                return Some(());
            }
            // A corner of the source: round on the outside, straight across on the inside.
            let turn = a.tan_out.cross(b.tan_in);
            let k = a.corner;
            let mut bridge = Vec::new();
            if turn * d < 0.0 {
                let r = d.abs();
                let (fa, fb) = ((pa - k).angle(), (pb - k).angle());
                let mut sweep = fb - fa;
                while sweep > PI {
                    sweep -= TAU;
                }
                while sweep <= -PI {
                    sweep += TAU;
                }
                let arc = |x: f64| {
                    let f = fa + sweep * x;
                    Some((k + Vec2::from_angle(f) * r, Vec2::new(-f.sin(), f.cos()) * (r * sweep)))
                };
                fit(&arc, 0.0, 1.0, tol * 0.5, &mut bridge)?;
            } else {
                bridge.push(Cubic { p: [pa, pa.lerp(pb, 1.0 / 3.0), pa.lerp(pb, 2.0 / 3.0), pb], span: 1.0 });
            }
            c1.push(false);
            let m = bridge.len();
            for (i, mut c) in bridge.into_iter().enumerate() {
                c.span *= typical.max(1e-12);
                out.push(c);
                c1.push(i + 1 < m);
            }
            // The last bridge piece joins the next run at a corner.
            if let Some(l) = c1.last_mut() {
                *l = false;
            }
            Some(())
        };
        let mut cubics: Vec<Cubic> = Vec::new();
        let mut c1: Vec<bool> = Vec::new();
        for i in 0..runs.len() {
            let r = runs.get(i)?;
            for (k, c) in r.cubics.iter().enumerate() {
                if k > 0 {
                    c1.push(true);
                }
                cubics.push(*c);
            }
            if let Some(next) = runs.get(i + 1) {
                joint(r, next, &mut cubics, &mut c1)?;
            }
        }
        let mut periodic = false;
        if self.closed
            && let (Some(last), Some(first)) = (runs.last(), runs.first())
        {
            let ((pa, da), (pb, db)) = (last.end, first.start);
            if pa.dist(pb) <= tol {
                periodic = (da - db).len() <= 1e-9 * (da.len() + db.len()) && c1.iter().all(|x| *x);
            } else {
                // Close the gap at the seam's corner.
                joint(last, first, &mut cubics, &mut c1)?;
                c1.pop();
            }
        }
        // Make corner joints meet exactly.
        for i in 0..c1.len() {
            if !c1.get(i).copied().unwrap_or(true)
                && let Some(p) = cubics.get(i).map(|c| c.p[3])
                && let Some(next) = cubics.get_mut(i + 1)
            {
                next.p[0] = p;
            }
        }
        let t0 = self.domain().0;
        assemble(&cubics, &c1, t0, self.closed, periodic, tol * 0.5)
    }
}
