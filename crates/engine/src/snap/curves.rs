//! Snap points on ellipses, elliptical arcs and splines: the midpoint by length, and the
//! perpendicular feet and tangent points from a base point. Feet and tangent points are exact
//! (`cadcraft_geom::Curve`: roots of the conditions on the rational Bézier pieces); the
//! midpoint samples the length, and every loop has a fixed bound.

use cadcraft_geom::{Curve, Ellipse, Spline, Vec2};

/// Parameter samples along a curve (length).
const SAMPLES: usize = 512;
/// The most solutions one curve offers.
const MAX_ROOTS: usize = 16;

/// A curve given by a parameter range and its points.
struct Param<'a> {
    lo: f64,
    hi: f64,
    at: &'a dyn Fn(f64) -> Vec2,
}

impl Param<'_> {
    fn ellipse(e: &Ellipse, f: impl FnOnce(&Param) -> Vec<Vec2>) -> Vec<Vec2> {
        let lo = e.start;
        let hi = e.start + e.sweep();
        f(&Param { lo, hi, at: &|t| e.at_param(t) })
    }

    fn spline(s: &Spline, f: impl FnOnce(&Param) -> Vec<Vec2>) -> Vec<Vec2> {
        let (lo, hi) = s.domain();
        if !(s.is_valid() && lo.is_finite() && hi.is_finite() && hi > lo) {
            return Vec::new();
        }
        f(&Param { lo, hi, at: &|t| s.eval(t) })
    }

    fn param(&self, i: usize) -> f64 {
        self.lo + (self.hi - self.lo) * i as f64 / SAMPLES as f64
    }

    /// The point halfway along the curve's length.
    fn mid(&self) -> Option<Vec2> {
        let pts: Vec<Vec2> = (0..=SAMPLES).map(|i| (self.at)(self.param(i))).collect();
        let lens: Vec<f64> = pts.windows(2).map(|w| if let [a, b] = w { a.dist(*b) } else { 0.0 }).collect();
        let total: f64 = lens.iter().sum();
        if !(total.is_finite() && total > 0.0) {
            return None;
        }
        let mut run = 0.0;
        for (i, l) in lens.iter().enumerate() {
            if run + l >= total / 2.0 && *l > 0.0 {
                let k = (total / 2.0 - run) / l;
                let t = self.param(i) + (self.param(i + 1) - self.param(i)) * k;
                return Some((self.at)(t)).filter(|p| p.is_finite());
            }
            run += l;
        }
        None
    }
}

/// The midpoint (by length) of an elliptical arc; none for a full ellipse.
pub(super) fn ellipse_mid(e: &Ellipse) -> Option<Vec2> {
    if e.is_full() {
        return None;
    }
    Param::ellipse(e, |c| c.mid().into_iter().collect()).first().copied()
}

/// The feet of the perpendiculars from `base` to an ellipse or elliptical arc.
pub(super) fn ellipse_perpendiculars(e: &Ellipse, base: Vec2) -> Vec<Vec2> {
    Curve::ellipse(e).perpendiculars(base).into_iter().map(|c| c.point).take(MAX_ROOTS).collect()
}

/// The points where lines from `base` touch an ellipse or elliptical arc: exact, by mapping the
/// ellipse onto the unit circle (an affine map keeps tangency).
pub(super) fn ellipse_tangents(e: &Ellipse, base: Vec2) -> Vec<Vec2> {
    Curve::ellipse(e).tangents(base).into_iter().map(|c| c.point).take(MAX_ROOTS).collect()
}

/// The midpoint (by length) of an open spline.
pub(super) fn spline_mid(s: &Spline) -> Option<Vec2> {
    if s.closed {
        return None;
    }
    Param::spline(s, |c| c.mid().into_iter().collect()).first().copied()
}

/// The feet of the perpendiculars from `base` to a spline.
pub(super) fn spline_perpendiculars(s: &Spline, base: Vec2) -> Vec<Vec2> {
    Curve::spline(s).perpendiculars(base).into_iter().map(|c| c.point).take(MAX_ROOTS).collect()
}

/// The points where lines from `base` touch a spline.
pub(super) fn spline_tangents(s: &Spline, base: Vec2) -> Vec<Vec2> {
    Curve::spline(s).tangents(base).into_iter().map(|c| c.point).take(MAX_ROOTS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_geom::PI;

    fn length(f: impl Fn(f64) -> Vec2, lo: f64, hi: f64) -> f64 {
        let n = 20_000;
        (0..n).map(|i| f(lo + (hi - lo) * i as f64 / n as f64).dist(f(lo + (hi - lo) * (i + 1) as f64 / n as f64))).sum()
    }

    #[test]
    fn ellipse_mid_halves_the_length() {
        let e = Ellipse { center: Vec2::new(1.0, 2.0), major: Vec2::new(4.0, 0.0), ratio: 0.5, start: 0.0, end: PI / 2.0 };
        let m = ellipse_mid(&e).unwrap();
        let t = e.param_of(m);
        let (l1, l2) = (length(|t| e.at_param(t), 0.0, t), length(|t| e.at_param(t), t, PI / 2.0));
        assert!((l1 - l2).abs() < 1e-4, "{l1} vs {l2}");
        assert!(ellipse_mid(&Ellipse::full(Vec2::ZERO, Vec2::X, 0.5)).is_none());
    }

    #[test]
    fn ellipse_tangents_touch_and_stay_on_the_arc() {
        let full = Ellipse::full(Vec2::ZERO, Vec2::new(4.0, 0.0), 0.5);
        let base = Vec2::new(0.0, 5.0);
        let ts = ellipse_tangents(&full, base);
        assert_eq!(ts.len(), 2);
        for p in &ts {
            // On the ellipse, and the line from the base touches it: (x/a)² + (y/b)² = 1 and
            // the gradient is perpendicular to the line.
            assert!(((p.x / 4.0).powi(2) + (p.y / 2.0).powi(2) - 1.0).abs() < 1e-9);
            let grad = Vec2::new(p.x / 16.0, p.y / 4.0);
            assert!(grad.dot(*p - base).abs() < 1e-9);
        }
        // Only the right half of the upper arc: one tangent point.
        let arc = Ellipse { start: 0.0, end: PI / 2.0, ..full };
        assert_eq!(ellipse_tangents(&arc, base).len(), 1);
        // From inside: none.
        assert!(ellipse_tangents(&full, Vec2::new(1.0, 0.5)).is_empty());
    }

    #[test]
    fn ellipse_perpendiculars_are_normals() {
        let e = Ellipse::full(Vec2::ZERO, Vec2::new(4.0, 0.0), 0.5);
        let base = Vec2::new(1.0, 0.5);
        let ps = ellipse_perpendiculars(&e, base);
        // From a point inside near the centre: the four normals of an ellipse.
        assert_eq!(ps.len(), 4, "{ps:?}");
        for p in &ps {
            let tan = Vec2::new(-p.y * 4.0 / 2.0, p.x * 2.0 / 4.0);
            assert!(tan.dot(*p - base).abs() < 1e-6, "{p:?}");
        }
        // A circle seen from its centre has no particular perpendicular.
        assert!(ellipse_perpendiculars(&Ellipse::full(Vec2::ZERO, Vec2::X, 1.0), Vec2::ZERO).is_empty());
    }

    #[test]
    fn spline_snaps() {
        let s = Spline::from_control(vec![Vec2::new(0.0, 0.0), Vec2::new(1.0, 3.0), Vec2::new(3.0, 3.0), Vec2::new(4.0, 0.0)], 3);
        // Symmetric about x = 2: the midpoint is on the axis, so is the perpendicular from
        // straight below, and the tangents from far above are symmetric.
        let m = spline_mid(&s).unwrap();
        assert!((m.x - 2.0).abs() < 1e-6 && (m.y - 2.25).abs() < 1e-6, "{m:?}");
        let ps = spline_perpendiculars(&s, Vec2::new(2.0, -5.0));
        assert!(ps.iter().any(|p| p.near(m, 1e-6)), "{ps:?}");
        // Every tangent of the arch passes x = 2 between y = 2.25 and 6: none from (2, 10) or
        // from below the arch, two from (2, 4).
        assert!(spline_tangents(&s, Vec2::new(2.0, 10.0)).is_empty());
        assert!(spline_tangents(&s, Vec2::new(2.0, -1.0)).is_empty());
        let ts = spline_tangents(&s, Vec2::new(2.0, 4.0));
        assert_eq!(ts.len(), 2, "{ts:?}");
        assert!((ts[0].x - 2.0 + ts[1].x - 2.0).abs() < 1e-6 && (ts[0].y - ts[1].y).abs() < 1e-6, "{ts:?}");
    }
}
