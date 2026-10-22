//! Rational Bézier pieces and Bernstein polynomials: the machinery under the exact
//! intersections, closest points and offsets of ellipses and splines (`exact.rs`, `offset.rs`).
//!
//! A spline (or an ellipse, as an exact rational quadratic spline) is cut into its polynomial
//! pieces. Every piece has positive weights, so it lies inside the bounds of its control points:
//! the convex-hull property every subdivision test relies on. A scalar condition along a piece
//! (lying on a conic, perpendicular or tangent from a point) is a polynomial in the piece
//! parameter; in Bernstein form its coefficients bound its values, so roots are isolated by
//! subdivision and then polished by safeguarded Newton steps on the exact function.
//!
//! Textbook algorithms written from the mathematics. Every loop is bounded; degenerate input
//! gives no piece (`None`) or no root, never a panic or NaN.

use crate::nurbs::{H, MAX_EDIT_DEGREE};
use crate::{Bounds2, Spline, Vec2};

/// Most control points one piece has (degree + 1).
pub(crate) const MAX_POINTS: usize = MAX_EDIT_DEGREE + 1;
/// Smallest weight kept after scaling the largest to 1 (a wider spread is hostile input).
const MIN_WEIGHT: f64 = 1e-8;
/// Most subdivision nodes one root search visits.
const MAX_ROOT_NODES: usize = 4000;
/// Deepest subdivision of a root search (the interval is then 2⁻⁴⁸ of the piece).
const MAX_ROOT_DEPTH: u32 = 48;
/// Narrowest interval the root search still splits.
const MIN_ROOT_WIDTH: f64 = 1e-10;
/// Newton / bisection steps when polishing one root.
const POLISH_STEPS: usize = 100;

const ZERO_H: H = (Vec2::ZERO, 0.0);

fn hlerp(a: H, b: H, t: f64) -> H {
    (a.0 * (1.0 - t) + b.0 * t, a.1 * (1.0 - t) + b.1 * t)
}

fn hsub(a: H, b: H) -> H {
    (a.0 - b.0, a.1 - b.1)
}

fn hscale(a: H, k: f64) -> H {
    (a.0 * k, a.1 * k)
}

/// The affine point of a homogeneous one (weights are positive on a piece).
pub(crate) fn proj(h: H) -> Vec2 {
    if h.1.abs() > 1e-300 { h.0 / h.1 } else { h.0 }
}

/// Bounds of the affine control points of a homogeneous net.
pub(crate) fn net_bounds(h: &[H]) -> Bounds2 {
    Bounds2::from_points(h.iter().map(|x| proj(*x)))
}

/// One rational Bézier piece of a curve: homogeneous control points `(w·P, w)` (weights scaled
/// so the largest is 1, all positive) and the source-curve parameters at its ends.
#[derive(Clone, Debug)]
pub(crate) struct Piece {
    pub h: Vec<H>,
    pub t0: f64,
    pub t1: f64,
    pub bounds: Bounds2,
}

impl Piece {
    pub fn new(h: Vec<H>, t0: f64, t1: f64) -> Option<Piece> {
        if h.len() < 2 || h.len() > MAX_POINTS || !(t0.is_finite() && t1.is_finite()) {
            return None;
        }
        let wmax = h.iter().map(|x| x.1).fold(0.0, f64::max);
        if !(wmax.is_finite() && wmax > 0.0) || h.iter().any(|x| !(x.1 > 0.0 && x.0.is_finite())) {
            return None;
        }
        let h: Vec<H> = h.into_iter().map(|x| hscale(x, 1.0 / wmax)).collect();
        if h.iter().any(|x| !(x.1 >= MIN_WEIGHT && x.0.is_finite())) {
            return None;
        }
        let bounds = net_bounds(&h);
        (bounds.min.is_finite() && bounds.max.is_finite()).then_some(Piece { h, t0, t1, bounds })
    }

    /// Source-curve parameter at local parameter `s` in [0, 1].
    pub fn param(&self, s: f64) -> f64 {
        self.t0 + (self.t1 - self.t0) * s
    }

    /// Point at local parameter `s` (clamped to [0, 1]).
    pub fn point(&self, s: f64) -> Vec2 {
        proj(derivs(&self.h, s).0)
    }

    /// Point, first and second derivative with respect to the local parameter.
    pub fn eval3(&self, s: f64) -> (Vec2, Vec2, Vec2) {
        euclid(derivs(&self.h, s))
    }

    /// The control net mapped by `f` (an affine map of the points); weights unchanged.
    pub fn mapped(&self, f: impl Fn(Vec2) -> Vec2) -> Vec<H> {
        self.h.iter().map(|x| (f(proj(*x)) * x.1, x.1)).collect()
    }
}

/// Euclidean point and first two derivatives from homogeneous ones.
pub(crate) fn euclid((h, d1, d2): (H, H, H)) -> (Vec2, Vec2, Vec2) {
    let w = if h.1.abs() > 1e-300 { h.1 } else { 1.0 };
    let c = h.0 / w;
    let c1 = (d1.0 - c * d1.1) / w;
    let c2 = (d2.0 - c1 * (2.0 * d1.1) - c * d2.1) / w;
    (c, c1, c2)
}

/// Homogeneous point and its first two derivatives at `s` (clamped to [0, 1]), by de Casteljau.
pub(crate) fn derivs(h: &[H], s: f64) -> (H, H, H) {
    let s = if s.is_finite() { s.clamp(0.0, 1.0) } else { 0.0 };
    let n = h.len().min(MAX_POINTS);
    if n == 0 {
        return (ZERO_H, ZERO_H, ZERO_H);
    }
    let p = (n - 1) as f64;
    let mut b = [ZERO_H; MAX_POINTS];
    for (dst, src) in b.iter_mut().zip(h) {
        *dst = *src;
    }
    let at = |b: &[H; MAX_POINTS], i: usize| b.get(i).copied().unwrap_or(ZERO_H);
    let (mut d1, mut d2) = (ZERO_H, ZERO_H);
    let mut len = n;
    while len > 1 {
        if len == 3 {
            let (q0, q1, q2) = (at(&b, 0), at(&b, 1), at(&b, 2));
            d2 = hscale(hsub(hsub(q2, q1), hsub(q1, q0)), p * (p - 1.0));
        }
        if len == 2 {
            d1 = hscale(hsub(at(&b, 1), at(&b, 0)), p);
        }
        for i in 0..len - 1 {
            let next = at(&b, i + 1);
            if let Some(x) = b.get_mut(i) {
                *x = hlerp(*x, next, s);
            }
        }
        len -= 1;
    }
    (at(&b, 0), d1, d2)
}

/// Split a homogeneous control net at `s` into the nets of [0, s] and [s, 1].
pub(crate) fn split(h: &[H], s: f64) -> (Vec<H>, Vec<H>) {
    let n = h.len();
    let mut b = h.to_vec();
    let mut left = Vec::with_capacity(n);
    let mut right = Vec::with_capacity(n);
    for len in (1..=n).rev() {
        left.extend(b.first().copied());
        right.extend(b.get(len - 1).copied());
        for i in 0..len.saturating_sub(1) {
            let next = b.get(i + 1).copied().unwrap_or(ZERO_H);
            if let Some(x) = b.get_mut(i) {
                *x = hlerp(*x, next, s);
            }
        }
    }
    right.reverse();
    (left, right)
}

/// The rational Bézier pieces of a spline (clamped first; periodic splines too). `None` for an
/// invalid spline, a degree above the editing limit, non-positive weights or a curve too big
/// to decompose.
pub(crate) fn spline_pieces(s: &Spline) -> Option<Vec<Piece>> {
    if !s.is_valid() || s.degree > MAX_EDIT_DEGREE || s.control.iter().any(|c| !c.is_finite()) {
        return None;
    }
    if s.weights.iter().any(|w| !(w.is_finite() && *w > 0.0)) {
        return None;
    }
    let (segs, breaks) = s.bezier_pieces()?;
    let mut out = Vec::with_capacity(segs.len());
    for (j, h) in segs.into_iter().enumerate() {
        let (Some(&t0), Some(&t1)) = (breaks.get(j), breaks.get(j + 1)) else { return None };
        if t1 <= t0 {
            continue;
        }
        out.push(Piece::new(h, t0, t1)?);
    }
    (!out.is_empty()).then_some(out)
}

// ---------------- Bernstein polynomials ----------------

/// Binomial coefficient as a float (exact up to well past the degrees used here).
pub(crate) fn binom(n: usize, k: usize) -> f64 {
    if k > n {
        return 0.0;
    }
    let k = k.min(n - k);
    (0..k).fold(1.0, |acc, i| acc * (n - i) as f64 / (i + 1) as f64)
}

/// Product of two polynomials in Bernstein form on [0, 1].
pub(crate) fn bmul(a: &[f64], b: &[f64]) -> Vec<f64> {
    let (Some(m), Some(n)) = (a.len().checked_sub(1), b.len().checked_sub(1)) else { return Vec::new() };
    let ca: Vec<f64> = (0..=m).map(|i| binom(m, i)).collect();
    let cb: Vec<f64> = (0..=n).map(|j| binom(n, j)).collect();
    (0..=m + n)
        .map(|k| {
            let lo = k.saturating_sub(n);
            let hi = k.min(m);
            let sum: f64 = (lo..=hi)
                .map(|i| {
                    let j = k - i;
                    ca.get(i).copied().unwrap_or(0.0)
                        * cb.get(j).copied().unwrap_or(0.0)
                        * a.get(i).copied().unwrap_or(0.0)
                        * b.get(j).copied().unwrap_or(0.0)
                })
                .sum();
            sum / binom(m + n, k)
        })
        .collect()
}

/// Coefficient-wise `a + k·b` (same degree).
pub(crate) fn badd(a: &[f64], b: &[f64], k: f64) -> Vec<f64> {
    a.iter().zip(b).map(|(x, y)| x + k * y).collect()
}

/// Derivative of a Bernstein polynomial (one degree lower; a constant for degree 0).
pub(crate) fn bdiff(a: &[f64]) -> Vec<f64> {
    let p = a.len().saturating_sub(1) as f64;
    if a.len() < 2 {
        return vec![0.0];
    }
    a.windows(2).map(|w| if let [x, y] = w { p * (y - x) } else { 0.0 }).collect()
}

/// Split a scalar Bernstein polynomial at `s`.
fn bsplit(c: &[f64], s: f64) -> (Vec<f64>, Vec<f64>) {
    let n = c.len();
    let mut b = c.to_vec();
    let mut left = Vec::with_capacity(n);
    let mut right = Vec::with_capacity(n);
    for len in (1..=n).rev() {
        left.extend(b.first().copied());
        right.extend(b.get(len - 1).copied());
        for i in 0..len.saturating_sub(1) {
            let next = b.get(i + 1).copied().unwrap_or(0.0);
            if let Some(x) = b.get_mut(i) {
                *x = *x * (1.0 - s) + next * s;
            }
        }
    }
    right.reverse();
    (left, right)
}

fn sign_changes(c: &[f64]) -> usize {
    let mut last = 0.0f64;
    let mut n = 0;
    for &x in c {
        if x != 0.0 {
            if last != 0.0 && (x > 0.0) != (last > 0.0) {
                n += 1;
            }
            last = x;
        }
    }
    n
}

/// What the root search reports for one piece.
pub(crate) enum Roots {
    /// The roots in [0, 1], sorted.
    Some(Vec<f64>),
    /// The function vanishes on the whole piece (within the tolerance): no particular root.
    Everywhere,
}

/// Roots in [0, 1] of a scalar function `g` along a piece. `coef` is `g` in Bernstein form (its
/// coefficients bound it, which isolates the roots); `eval` gives the exact `g` and `g'` at a
/// parameter, which polishes them. Values within `eps` of zero count as zero, so a touch (a
/// double root, as at a tangency) is found as a critical point where `|g| <= eps`.
pub(crate) fn roots(coef: &[f64], eps: f64, eval: &dyn Fn(f64) -> (f64, f64)) -> Roots {
    if coef.is_empty() || coef.iter().any(|c| !c.is_finite()) || !(eps.is_finite() && eps >= 0.0) {
        return Roots::Some(Vec::new());
    }
    if coef.iter().all(|c| c.abs() <= eps) {
        return Roots::Everywhere;
    }
    let mut out: Vec<f64> = Vec::new();
    let mut stack: Vec<(Vec<f64>, f64, f64, u32)> = vec![(coef.to_vec(), 0.0, 1.0, 0)];
    let mut budget = MAX_ROOT_NODES;
    let near_zero = |s: f64| eval(s).0.abs() <= eps;
    while let Some((c, a, b, depth)) = stack.pop() {
        if budget == 0 {
            break;
        }
        budget -= 1;
        let (mn, mx) = c.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), x| (lo.min(*x), hi.max(*x)));
        if mn > eps || mx < -eps {
            continue;
        }
        let (Some(&c0), Some(&cn)) = (c.first(), c.last()) else { continue };
        if sign_changes(&c) == 1
            && c0 * cn < 0.0
            && let Some(r) = polish_bracket(a, b, eval)
        {
            out.push(r);
            continue;
        }
        // The derivative's sign pattern: monotone (at most one root, at a sign change or a
        // piece end) or one extremum (a touch, two close roots, or none).
        let d = bdiff(&c);
        let (Some(&d0), Some(&dn)) = (d.first(), d.last()) else { continue };
        let dchanges = sign_changes(&d);
        let dzero = d.contains(&0.0);
        if dchanges == 0 && !dzero {
            if let Some(r) = polish_bracket(a, b, eval) {
                out.push(r);
            } else {
                // An end on the zero set (within the tolerance): a piece end, or a touch or
                // crossing right at a split point.
                out.extend([a, b].into_iter().filter(|e| near_zero(*e)).filter_map(|e| polish_touch(e, e, eps, eval)));
            }
            continue;
        }
        if dchanges == 1 && d0 * dn < 0.0 && !dzero {
            out.extend(one_extremum(a, b, eps, eval));
            continue;
        }
        // A stretch lying on the zero set (an overlap) has no isolated root.
        if depth > 0 && mn >= -eps && mx <= eps {
            continue;
        }
        if b - a <= MIN_ROOT_WIDTH || depth >= MAX_ROOT_DEPTH {
            out.extend(polish_touch(a, b, eps, eval));
            continue;
        }
        let (l, r) = bsplit(&c, 0.5);
        let m = 0.5 * (a + b);
        stack.push((r, m, b, depth + 1));
        stack.push((l, a, m, depth + 1));
    }
    out.retain(|r| r.is_finite());
    out.sort_by(f64::total_cmp);
    out.dedup_by(|x, y| (*x - *y).abs() <= 1e-12);
    Roots::Some(out)
}

/// The root of `g` in [a, b] where `g` changes sign: Newton steps kept inside the shrinking
/// bracket, bisection when a step leaves it. `None` when the ends do not bracket a root.
pub(crate) fn polish_bracket(a: f64, b: f64, eval: &dyn Fn(f64) -> (f64, f64)) -> Option<f64> {
    let (ga, _) = eval(a);
    let (gb, _) = eval(b);
    if !(ga.is_finite() && gb.is_finite()) {
        return None;
    }
    if ga == 0.0 {
        return Some(a);
    }
    if gb == 0.0 {
        return Some(b);
    }
    if (ga > 0.0) == (gb > 0.0) {
        return None;
    }
    // `neg`: where g < 0, `pos`: where g > 0.
    let (mut neg, mut pos) = if ga < 0.0 { (a, b) } else { (b, a) };
    let mut x = 0.5 * (a + b);
    for _ in 0..POLISH_STEPS {
        let (g, dg) = eval(x);
        if !g.is_finite() {
            return None;
        }
        if g == 0.0 {
            return Some(x);
        }
        if g < 0.0 {
            neg = x;
        } else {
            pos = x;
        }
        let (lo, hi) = (neg.min(pos), neg.max(pos));
        let step = x - g / dg;
        let next = if step.is_finite() && step > lo && step < hi { step } else { 0.5 * (lo + hi) };
        if (next - x).abs() <= 4.0 * f64::EPSILON * x.abs().max(1e-300) || hi - lo <= 4.0 * f64::EPSILON * hi.abs().max(1e-300) {
            return Some(next);
        }
        x = next;
    }
    Some(x)
}

/// The critical point of `g` in [a, b], where `g'` changes sign (bisection on `g'`).
fn critical(a: f64, b: f64, eval: &dyn Fn(f64) -> (f64, f64)) -> Option<f64> {
    let (_, da) = eval(a);
    let (_, db) = eval(b);
    if !(da.is_finite() && db.is_finite()) || (da > 0.0) == (db > 0.0) || da == 0.0 || db == 0.0 {
        return None;
    }
    let (mut lo, mut hi, mut dlo) = (a, b, da);
    for _ in 0..POLISH_STEPS {
        let m = 0.5 * (lo + hi);
        if !(m > lo && m < hi) {
            break;
        }
        let (_, dm) = eval(m);
        if !dm.is_finite() {
            return None;
        }
        if dm == 0.0 {
            return Some(m);
        }
        if (dm > 0.0) == (dlo > 0.0) {
            lo = m;
            dlo = dm;
        } else {
            hi = m;
        }
    }
    Some(0.5 * (lo + hi))
}

/// The roots of `g` on [a, b] where it has exactly one extremum: one crossing when the ends
/// differ in sign; else two crossings either side of the extremum, or one touch there when
/// `|g| <= eps` at it (two roots closer than the tolerance count as a tangency).
fn one_extremum(a: f64, b: f64, eps: f64, eval: &dyn Fn(f64) -> (f64, f64)) -> Vec<f64> {
    if let Some(r) = polish_bracket(a, b, eval) {
        return vec![r];
    }
    let Some(m) = critical(a, b, eval) else {
        return polish_touch(a, b, eps, eval).into_iter().collect();
    };
    let (gm, _) = eval(m);
    let (ga, _) = eval(a);
    if !(gm.is_finite() && ga.is_finite()) {
        return Vec::new();
    }
    if gm.abs() <= eps {
        return vec![m];
    }
    if (gm > 0.0) != (ga > 0.0) {
        return [polish_bracket(a, m, eval), polish_bracket(m, b, eval)].into_iter().flatten().collect();
    }
    // No crossing; an end may still lie on the zero set.
    [a, b].into_iter().filter(|e| eval(*e).0.abs() <= eps).filter_map(|e| polish_touch(e, e, eps, eval)).collect()
}

/// A touching root in or next to [a, b]: the critical point of `g` (where `g'` changes sign),
/// kept when `|g| <= eps` there. A sign change of `g` itself is polished as a simple root.
fn polish_touch(a: f64, b: f64, eps: f64, eval: &dyn Fn(f64) -> (f64, f64)) -> Option<f64> {
    let w = (b - a).max(1e-12);
    let (a, b) = ((a - w).max(0.0), (b + w).min(1.0));
    let (ga, _) = eval(a);
    let (gb, _) = eval(b);
    if !(ga.is_finite() && gb.is_finite()) {
        return None;
    }
    if (ga < 0.0 && gb > 0.0) || (ga > 0.0 && gb < 0.0) {
        return polish_bracket(a, b, eval);
    }
    // The extremum; with none inside, only a piece end can touch (an interior point of a
    // monotone stretch is never a root of its own).
    let s = match critical(a, b, eval) {
        Some(m) => m,
        None => [a, b].into_iter().filter(|e| *e == 0.0 || *e == 1.0).min_by(|x, y| eval(*x).0.abs().total_cmp(&eval(*y).0.abs()))?,
    };
    let (g, _) = eval(s);
    (g.is_finite() && g.abs() <= eps).then_some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bernstein_product_and_roots() {
        // (s - 1/4)(s - 3/4) as a product of two lines in Bernstein form.
        let a = [-0.25, 0.75];
        let b = [-0.75, 0.25];
        let c = bmul(&a, &b);
        let eval = |s: f64| ((s - 0.25) * (s - 0.75), 2.0 * s - 1.0);
        let Roots::Some(r) = roots(&c, 1e-14, &eval) else { panic!("vanishes") };
        assert_eq!(r.len(), 2, "{r:?}");
        assert!((r[0] - 0.25).abs() < 1e-15 && (r[1] - 0.75).abs() < 1e-15, "{r:?}");
        // A double root (s - 1/3)²: found as a touch.
        let d = bmul(&[-1.0 / 3.0, 2.0 / 3.0], &[-1.0 / 3.0, 2.0 / 3.0]);
        let eval = |s: f64| ((s - 1.0 / 3.0).powi(2), 2.0 * (s - 1.0 / 3.0));
        let Roots::Some(r) = roots(&d, 1e-12, &eval) else { panic!("vanishes") };
        assert_eq!(r.len(), 1, "{r:?}");
        assert!((r[0] - 1.0 / 3.0).abs() < 1e-12, "{r:?}");
        assert!(matches!(roots(&[0.0, 0.0, 0.0], 1e-12, &|_| (0.0, 0.0)), Roots::Everywhere));
        assert!(matches!(roots(&[f64::NAN], 1e-12, &|_| (0.0, 0.0)), Roots::Some(v) if v.is_empty()));
    }

    #[test]
    fn pieces_evaluate_like_the_spline() {
        let mut s =
            Spline::from_control(vec![Vec2::new(0.0, 0.0), Vec2::new(3.0, 3.0), Vec2::new(6.0, -3.0), Vec2::new(9.0, 0.0), Vec2::new(12.0, 2.0)], 3);
        s.weights = vec![1.0, 2.0, 0.5, 1.0, 1.5];
        let ps = spline_pieces(&s).unwrap();
        assert_eq!(ps.len(), 2);
        for p in &ps {
            for i in 0..=10 {
                let u = i as f64 / 10.0;
                let (c, d1, _) = p.eval3(u);
                assert!(c.near(s.eval(p.param(u)), 1e-12));
                assert!((d1 / (p.t1 - p.t0)).near(s.derivative(p.param(u)), 1e-9));
            }
        }
        let mut bad = s.clone();
        bad.weights[2] = -1.0;
        assert!(spline_pieces(&bad).is_none());
    }
}
