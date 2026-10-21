use serde::{Deserialize, Serialize};

use crate::spline_fit::FitOptions;
use crate::{Bounds2, Vec2};

/// A (possibly rational) B-spline curve, DXF SPLINE style.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Spline {
    pub degree: usize,
    pub knots: Vec<f64>,
    pub control: Vec<Vec2>,
    /// Empty = non-rational (all weights 1).
    #[serde(default)]
    pub weights: Vec<f64>,
    #[serde(default)]
    pub fit: Vec<Vec2>,
    #[serde(default)]
    pub closed: bool,
    /// How the curve was built from `fit` (knot parametrisation, end tangents, fit tolerance), so
    /// edits to the fit points rebuild it the same way.
    #[serde(default)]
    pub fit_opts: FitOptions,
}

impl Spline {
    /// A clamped uniform spline over control points.
    pub fn from_control(control: Vec<Vec2>, degree: usize) -> Spline {
        let n = control.len();
        let degree = degree.clamp(1, n.saturating_sub(1).max(1));
        let knots = clamped_uniform_knots(n, degree);
        Spline { degree, knots, control, weights: Vec::new(), fit: Vec::new(), closed: false, fit_opts: FitOptions::default() }
    }

    /// Cubic interpolation through fit points (chord-length parametrisation, natural-ish ends
    /// via a clamped knot vector: solve the banded system for control points).
    pub fn from_fit_points(fit: &[Vec2]) -> Spline {
        Spline::fit_with(fit, false, FitOptions::default())
    }

    /// Smooth closed (periodic) cubic interpolation through fit points. Chord-length parameters
    /// include the closing chord; the knot vector is the periodic (unclamped) extension and the
    /// first `degree` control points repeat at the end, so the curve has continuous tangent and
    /// curvature at the seam. A repeated closing fit point is dropped.
    pub fn from_fit_points_closed(fit: &[Vec2]) -> Spline {
        Spline::fit_with(fit, true, FitOptions::default())
    }

    /// Interpolation through fit points, open or closed (see `from_fit_points_closed`).
    pub fn from_fit(fit: &[Vec2], closed: bool) -> Spline {
        Spline::fit_with(fit, closed, FitOptions::default())
    }

    /// Closed with an unclamped (periodic) knot vector, as DXF flags it (group 70 bit 2).
    pub fn is_periodic(&self) -> bool {
        self.closed && self.knots.first() != self.knots.get(self.degree)
    }

    pub fn is_valid(&self) -> bool {
        let n = self.control.len();
        n > self.degree && self.degree >= 1 && self.knots.len() == n + self.degree + 1 && self.knots.windows(2).all(|w| w.first() <= w.get(1))
    }

    pub fn domain(&self) -> (f64, f64) {
        let lo = self.knots.get(self.degree).copied().unwrap_or(0.0);
        let hi = self.knots.get(self.control.len()).copied().unwrap_or(1.0);
        (lo, hi)
    }

    /// Evaluate at parameter `t` (de Boor via basis functions).
    pub fn eval(&self, t: f64) -> Vec2 {
        if !self.is_valid() {
            return self.control.first().copied().unwrap_or_default();
        }
        let (lo, hi) = self.domain();
        self.eval_valid(t, lo, hi)
    }

    /// `eval` on a spline `is_valid` has accepted, given its `domain()`, so sampling many points
    /// validates once (`is_valid` walks the knot vector).
    fn eval_valid(&self, t: f64, lo: f64, hi: f64) -> Vec2 {
        let n = self.control.len().saturating_sub(1);
        let t = t.clamp(lo, hi);
        let span = find_span(n, self.degree, t, &self.knots);
        let (mut nb, mut left, mut right) = ([0.0; MAX_STACK_DEGREE + 1], [0.0; MAX_STACK_DEGREE + 1], [0.0; MAX_STACK_DEGREE + 1]);
        let heap;
        let b: &[f64] = if self.degree <= MAX_STACK_DEGREE {
            basis_into(span, t, self.degree, &self.knots, &mut nb, &mut left, &mut right);
            nb.get(..=self.degree).unwrap_or(&[])
        } else {
            heap = basis(span, t, self.degree, &self.knots);
            &heap
        };
        let mut p = Vec2::ZERO;
        let mut wsum = 0.0;
        for (k, nb) in b.iter().enumerate() {
            let idx = span + k - self.degree;
            let w = if self.weights.is_empty() { 1.0 } else { self.weights.get(idx).copied().unwrap_or(1.0) };
            if let Some(c) = self.control.get(idx) {
                p += *c * (nb * w);
                wsum += nb * w;
            }
        }
        if wsum.abs() > 1e-300 { p / wsum } else { p }
    }

    pub fn tessellate(&self, tol: f64) -> Vec<Vec2> {
        if !self.is_valid() {
            return self.control.clone();
        }
        self.samples(self.sample_count(tol)).collect()
    }

    /// How many segments a tessellation at `tol` takes: the control-polygon length guides it,
    /// capped at `MAX_SAMPLES`.
    fn sample_count(&self, tol: f64) -> usize {
        let poly: f64 = self.control.windows(2).map(|w| w.first().zip(w.get(1)).map(|(a, b)| a.dist(*b)).unwrap_or(0.0)).sum();
        let n = ((poly / tol.max(1e-9)).sqrt() * 2.0).clamp(8.0, 2000.0) as usize * self.degree.max(1);
        n.min(MAX_SAMPLES)
    }

    /// `n + 1` points evenly spaced in parameter over the domain (the spline must be valid).
    fn samples(&self, n: usize) -> impl Iterator<Item = Vec2> + '_ {
        let (lo, hi) = self.domain();
        (0..=n).map(move |i| self.eval_valid(lo + (hi - lo) * i as f64 / n as f64, lo, hi))
    }

    /// The box round the curve, from samples like `tessellate`'s at a tolerance relative to the
    /// spline's own size (1e-3 of its control box's diagonal), and at least four per knot span so
    /// a dense spline's wiggles are seen. The box can fall short of an extreme by the sag between
    /// two samples, far under 1% of the spline's size, whatever the drawing's units (zoom extents
    /// and selection pre-filtering tolerate that; the sampled box was never exact). A fixed
    /// absolute tolerance sampled the 4000-point cap on every spline of a drawing in millimetres,
    /// a third of the time a large drawing took to open (#1076); a typical spline now takes a few
    /// hundred evaluations. A degree-1 spline is its control polygon, so its control box is exact;
    /// a degenerate spline gets its control box, which contains the curve (the box of the finite
    /// control points when one is not finite: `hull` skips it, but it would poison every sample).
    pub fn bounds(&self) -> Bounds2 {
        let hull = Bounds2::from_points(self.control.iter().copied());
        let diag = hull.min.dist(hull.max);
        let finite = self.control.iter().all(|c| c.is_finite()) && self.weights.iter().all(|w| w.is_finite());
        if !self.is_valid() || self.degree == 1 || !finite || !(diag.is_finite() && diag > 0.0) {
            return hull;
        }
        let spans = self.control.len().saturating_sub(self.degree);
        let n = self.sample_count(diag * 1e-3).max(spans.saturating_mul(4)).min(MAX_SAMPLES);
        Bounds2::from_points(self.samples(n))
    }
}

/// Most segments `tessellate` and `bounds` evaluate for one spline (#268: a high degree on many
/// control points took minutes).
const MAX_SAMPLES: usize = 4000;

pub(crate) fn clamped_uniform_knots(n: usize, degree: usize) -> Vec<f64> {
    let mut k = vec![0.0; degree + 1];
    let inner = n.saturating_sub(degree + 1);
    for i in 1..=inner {
        k.push(i as f64 / (inner + 1) as f64);
    }
    k.extend(std::iter::repeat_n(1.0, degree + 1));
    k
}

pub(crate) fn find_span(n: usize, p: usize, u: f64, knots: &[f64]) -> usize {
    let kn = knots.get(n + 1).copied().unwrap_or(1.0);
    if u >= kn {
        return n;
    }
    let mut low = p;
    let mut high = n + 1;
    let mut mid = (low + high) / 2;
    let mut guard = 0;
    while guard < 200 {
        guard += 1;
        let km = knots.get(mid).copied().unwrap_or(0.0);
        let km1 = knots.get(mid + 1).copied().unwrap_or(1.0);
        if u < km {
            high = mid;
        } else if u >= km1 {
            low = mid;
        } else {
            break;
        }
        mid = (low + high) / 2;
    }
    mid
}

/// Highest degree `eval` works out on the stack. The DXF reader and the SPLINE command cap the
/// degree at 10; a higher one (a programmatic spline) takes a heap buffer.
pub(crate) const MAX_STACK_DEGREE: usize = 10;

/// The `p + 1` non-zero basis functions of degree `p` at `u` in `span`.
pub(crate) fn basis(span: usize, u: f64, p: usize, knots: &[f64]) -> Vec<f64> {
    let mut n = vec![0.0; p + 1];
    let mut left = vec![0.0; p + 1];
    let mut right = vec![0.0; p + 1];
    basis_into(span, u, p, knots, &mut n, &mut left, &mut right);
    n
}

/// `basis` into `n[..=p]`, with `left` and `right` as scratch; each must hold `p + 1` values or
/// `n` stays as it was.
fn basis_into(span: usize, u: f64, p: usize, knots: &[f64], n: &mut [f64], left: &mut [f64], right: &mut [f64]) {
    if n.len() <= p || left.len() <= p || right.len() <= p {
        return;
    }
    if let Some(x) = n.get_mut(0) {
        *x = 1.0;
    }
    for j in 1..=p {
        let k = |i: isize| knots.get(i.max(0) as usize).copied().unwrap_or(0.0);
        left[j] = u - k(span as isize + 1 - j as isize);
        right[j] = k(span as isize + j as isize) - u;
        let mut saved = 0.0;
        for r in 0..j {
            let den = right[r + 1] + left[j - r];
            let tmp = if den.abs() > 1e-300 { n[r] / den } else { 0.0 };
            n[r] = saved + right[r + 1] * tmp;
            saved = left[j - r] * tmp;
        }
        n[j] = saved;
    }
}

/// Gaussian elimination with partial pivoting.
#[allow(clippy::needless_range_loop)]
pub(crate) fn solve(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Option<Vec<f64>> {
    let n = b.len();
    for c in 0..n {
        let piv = (c..n).max_by(|&i, &j| a[i][c].abs().total_cmp(&a[j][c].abs()))?;
        if a[piv][c].abs() < 1e-14 {
            return None;
        }
        a.swap(c, piv);
        b.swap(c, piv);
        for r in c + 1..n {
            let f = a[r][c] / a[c][c];
            if f != 0.0 {
                for k in c..n {
                    a[r][k] -= f * a[c][k];
                }
                b[r] -= f * b[c];
            }
        }
    }
    let mut x = vec![0.0; n];
    for r in (0..n).rev() {
        let s: f64 = (r + 1..n).map(|k| a[r][k] * x[k]).sum();
        x[r] = (b[r] - s) / a[r][r];
    }
    Some(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `basis` before it evaluated on the stack, kept as the reference.
    fn basis_ref(span: usize, u: f64, p: usize, knots: &[f64]) -> Vec<f64> {
        let mut n = vec![0.0; p + 1];
        let mut left = vec![0.0; p + 1];
        let mut right = vec![0.0; p + 1];
        n[0] = 1.0;
        for j in 1..=p {
            let k = |i: isize| knots.get(i.max(0) as usize).copied().unwrap_or(0.0);
            left[j] = u - k(span as isize + 1 - j as isize);
            right[j] = k(span as isize + j as isize) - u;
            let mut saved = 0.0;
            for r in 0..j {
                let den = right[r + 1] + left[j - r];
                let tmp = if den.abs() > 1e-300 { n[r] / den } else { 0.0 };
                n[r] = saved + right[r + 1] * tmp;
                saved = left[j - r] * tmp;
            }
            n[j] = saved;
        }
        n
    }

    /// `eval` before it evaluated on the stack, kept as the reference.
    fn eval_ref(s: &Spline, t: f64) -> Vec2 {
        let n = s.control.len() - 1;
        let (lo, hi) = s.domain();
        let t = t.clamp(lo, hi);
        let span = find_span(n, s.degree, t, &s.knots);
        let b = basis_ref(span, t, s.degree, &s.knots);
        let mut p = Vec2::ZERO;
        let mut wsum = 0.0;
        for (k, nb) in b.iter().enumerate() {
            let idx = span + k - s.degree;
            let w = if s.weights.is_empty() { 1.0 } else { s.weights[idx] };
            p += s.control[idx] * (nb * w);
            wsum += nb * w;
        }
        if wsum.abs() > 1e-300 { p / wsum } else { p }
    }

    /// Deterministic pseudo-random numbers in `0..1`.
    fn noise(seed: &mut u64) -> f64 {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (*seed >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Splines of degree 1..=11 over 14 control points: clamped and unclamped (uniform) knots,
    /// unit and random weights.
    fn samples() -> Vec<Spline> {
        let mut seed = 7;
        let mut out = Vec::new();
        for degree in 1..=11 {
            let control: Vec<Vec2> = (0..14).map(|i| Vec2::new(i as f64 * 3.0 + noise(&mut seed), 10.0 * noise(&mut seed) - 5.0)).collect();
            let weights: Vec<f64> = (0..14).map(|_| 0.5 + 1.5 * noise(&mut seed)).collect();
            let clamped = Spline::from_control(control.clone(), degree);
            let uniform = Spline { knots: (0..control.len() + degree + 1).map(|i| i as f64 * 0.25).collect(), ..clamped.clone() };
            for base in [clamped, uniform] {
                out.push(Spline { weights: weights.clone(), ..base.clone() });
                out.push(base);
            }
        }
        out
    }

    #[test]
    fn stack_basis_matches_the_heap_basis_bit_for_bit() {
        for s in samples() {
            assert!(s.is_valid());
            let (lo, hi) = s.domain();
            for i in -20..=1020 {
                let t = lo + (hi - lo) * i as f64 / 1000.0;
                let got = s.eval(t);
                let want = eval_ref(&s, t);
                assert_eq!(
                    (got.x.to_bits(), got.y.to_bits()),
                    (want.x.to_bits(), want.y.to_bits()),
                    "degree {} at {t}: {got:?} vs {want:?}",
                    s.degree
                );
                let span = find_span(s.control.len() - 1, s.degree, t.clamp(lo, hi), &s.knots);
                let b = basis(span, t.clamp(lo, hi), s.degree, &s.knots);
                let r = basis_ref(span, t.clamp(lo, hi), s.degree, &s.knots);
                assert!(b.iter().zip(&r).all(|(a, b)| a.to_bits() == b.to_bits()) && b.len() == r.len(), "degree {}", s.degree);
            }
        }
    }

    #[test]
    fn basis_into_needs_room_for_every_function() {
        let knots = clamped_uniform_knots(6, 3);
        let (mut n, mut left, mut right) = ([0.5; 3], [0.0; 4], [0.0; 4]);
        basis_into(3, 0.5, 3, &knots, &mut n, &mut left, &mut right);
        assert_eq!(n, [0.5; 3]);
    }

    /// A cubic through 300 points of a sine wave, open or closed.
    fn wiggly(closed: bool) -> Spline {
        let fit: Vec<Vec2> = (0..300)
            .map(|i| {
                let x = i as f64 / 299.0;
                Vec2::new(x * 100.0, 10.0 * (x * 60.0).sin())
            })
            .collect();
        Spline::from_fit(&fit, closed)
    }

    /// The box from 4000 evaluations, which `bounds` approximates.
    fn fine_box(s: &Spline) -> Bounds2 {
        let (lo, hi) = s.domain();
        Bounds2::from_points((0..=4000).map(|i| s.eval(lo + (hi - lo) * i as f64 / 4000.0)))
    }

    fn scaled(s: &Spline, k: f64) -> Spline {
        Spline { control: s.control.iter().map(|c| *c * k).collect(), fit: s.fit.iter().map(|c| *c * k).collect(), ..s.clone() }
    }

    #[test]
    fn bounds_stay_within_a_small_margin_of_the_finely_sampled_box() {
        let mut all = samples();
        all.push(wiggly(false));
        all.push(wiggly(true));
        all.push(Spline::from_fit_points_closed(&[Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)]));
        for s in all {
            // Whatever the drawing's units: the same relative margin at any scale.
            for k in [1e-6, 1.0, 1e6] {
                let s = scaled(&s, k);
                let (b, want) = (s.bounds(), fine_box(&s));
                let diag = want.min.dist(want.max);
                let err = [b.min.x - want.min.x, b.min.y - want.min.y, b.max.x - want.max.x, b.max.y - want.max.y]
                    .into_iter()
                    .map(f64::abs)
                    .fold(0.0, f64::max);
                assert!(err <= 1e-3 * diag, "degree {} with {} control points at scale {k}: {b:?} vs {want:?}", s.degree, s.control.len());
                // The curve lies in its control points' convex hull (positive weights), up to rounding.
                assert!(Bounds2::from_points(s.control.iter().copied()).expand(1e-9 * diag).contains_box(&b), "degree {} at scale {k}", s.degree);
            }
        }
    }

    #[test]
    fn degenerate_splines_get_their_control_box() {
        let point = Spline::from_control(vec![Vec2::new(3.0, 4.0); 4], 3);
        assert_eq!(point.bounds(), Bounds2::new(Vec2::new(3.0, 4.0), Vec2::new(3.0, 4.0)));
        let invalid = Spline { knots: vec![0.0; 3], ..Spline::from_control(vec![Vec2::ZERO, Vec2::new(1.0, 2.0), Vec2::new(5.0, -1.0)], 2) };
        assert!(!invalid.is_valid());
        assert_eq!(invalid.bounds(), Bounds2::new(Vec2::new(0.0, -1.0), Vec2::new(5.0, 2.0)));
        let hostile = Spline::from_control(vec![Vec2::new(f64::NAN, 0.0), Vec2::new(1.0, f64::INFINITY), Vec2::new(2.0, 2.0)], 2);
        assert_eq!(hostile.bounds(), Bounds2::new(Vec2::new(2.0, 2.0), Vec2::new(2.0, 2.0)));
        // A non-finite control point among finite ones poisons every sample: the box of the
        // finite control points, not the empty box.
        for bad in [Vec2::new(f64::NAN, 5.0), Vec2::new(5.0, f64::NAN), Vec2::new(f64::INFINITY, 5.0), Vec2::new(5.0, f64::NEG_INFINITY)] {
            let s = Spline::from_control(vec![Vec2::ZERO, bad, Vec2::new(10.0, 10.0), Vec2::new(20.0, 0.0)], 3);
            assert!(s.is_valid());
            assert_eq!(s.bounds(), Bounds2::new(Vec2::ZERO, Vec2::new(20.0, 10.0)), "{bad:?}");
        }
        // A non-finite weight poisons the samples the same way.
        for bad in [f64::NAN, f64::INFINITY] {
            let mut s = Spline::from_control(vec![Vec2::ZERO, Vec2::new(5.0, 5.0), Vec2::new(10.0, 10.0), Vec2::new(20.0, 0.0)], 3);
            s.weights = vec![1.0, bad, 1.0, 1.0];
            assert!(s.is_valid());
            assert_eq!(s.bounds(), Bounds2::new(Vec2::ZERO, Vec2::new(20.0, 10.0)), "weight {bad:?}");
        }
        assert!(Spline::default().bounds().is_empty());
    }
}
