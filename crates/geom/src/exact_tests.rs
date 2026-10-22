//! Exact ellipse and spline geometry (`exact.rs`, `offset.rs`) against analytic answers:
//! line/ellipse by the quadratic, circle/ellipse and ellipse/ellipse in symmetric cases,
//! splines built from exact conics, tangencies, closest points; offsets measured against the
//! exact curve; hostile and degenerate input. Deterministic xorshift generator, no extra crate.

use crate::*;

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.max(1))
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// A rotated, translated ellipse with semi-axes `a >= b`.
fn ellipse(c: Vec2, a: f64, b: f64, phi: f64) -> Ellipse {
    Ellipse::full(c, Vec2::from_angle(phi) * a, b / a)
}

/// The implicit value (x/a)² + (y/b)² − 1 of `p` for `e`.
fn on_ellipse(e: &Ellipse, p: Vec2) -> f64 {
    let (u, v) = (e.major, e.minor());
    let r = p - e.center;
    let (x, y) = (r.dot(u) / u.len2(), r.dot(v) / v.len2());
    x * x + y * y - 1.0
}

/// Line/ellipse intersections by the quadratic in the ellipse frame.
fn line_ellipse_analytic(e: &Ellipse, p0: Vec2, d: Vec2) -> Vec<Vec2> {
    let (u, v) = (e.major, e.minor());
    let q = p0 - e.center;
    let (qx, qy) = (q.dot(u) / u.len2(), q.dot(v) / v.len2());
    let (kx, ky) = (d.dot(u) / u.len2(), d.dot(v) / v.len2());
    let a = kx * kx + ky * ky;
    let b = 2.0 * (qx * kx + qy * ky);
    let c = qx * qx + qy * qy - 1.0;
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return Vec::new();
    }
    let s = disc.sqrt();
    let k = -0.5 * (b + b.signum() * s);
    let (t1, t2) = (k / a, c / k);
    vec![p0 + d * t1, p0 + d * t2]
}

fn matches(got: &[Vec2], want: &[Vec2], tol: f64) -> bool {
    got.len() == want.len() && want.iter().all(|w| got.iter().any(|g| g.near(*w, tol)))
}

#[test]
fn line_ellipse_matches_the_quadratic() {
    let mut rng = Rng::new(7);
    let mut worst = 0.0f64;
    for case in 0..2000 {
        let a = rng.range(0.5, 50.0);
        let b = a * rng.range(0.05, 1.0);
        let c = Vec2::new(rng.range(-100.0, 100.0), rng.range(-100.0, 100.0));
        let e = ellipse(c, a, b, rng.range(0.0, TAU));
        // A line through a random point near the ellipse, long enough to cross it entirely.
        let p0 = c + Vec2::new(rng.range(-a, a), rng.range(-a, a)) * 1.2;
        let d = Vec2::from_angle(rng.range(0.0, TAU));
        let want = line_ellipse_analytic(&e, p0, d);
        let line = Line::new(p0 - d * (4.0 * a), p0 + d * (4.0 * a));
        let hits: Vec<Vec2> = Curve::line(line).intersect(&Curve::ellipse(&e)).into_iter().map(|h| h.point).collect();
        // Clear crossings only (tangencies are checked separately).
        if want.len() == 2 && want[0].dist(want[1]) > 1e-3 * a {
            assert!(matches(&hits, &want, 1e-9 * a), "case {case}: {hits:?} vs {want:?}");
            for (h, w) in hits.iter().zip(&want) {
                worst = worst.max(h.dist(*w).min(hits.iter().map(|g| g.dist(*w)).fold(f64::INFINITY, f64::min)) / a);
            }
            // The same ellipse as a rational spline (the spline path) gives the same points.
            let s = Spline::from_ellipse(&e).unwrap();
            let sh: Vec<Vec2> = Curve::spline(&s).intersect(&Curve::line(line)).into_iter().map(|h| h.point).collect();
            assert!(matches(&sh, &want, 1e-9 * a), "case {case} (spline): {sh:?} vs {want:?}");
        } else if want.is_empty() {
            assert!(hits.is_empty() || hits.iter().all(|h| on_ellipse(&e, *h).abs() < 1e-8), "case {case}: {hits:?}");
        }
    }
    assert!(worst < 1e-12, "worst relative error {worst:e}");
}

#[test]
fn hit_parameters_point_at_the_hit() {
    let e = Ellipse { center: Vec2::new(2.0, -1.0), major: Vec2::new(3.0, 4.0), ratio: 0.4, start: 0.5, end: 4.0 };
    let line = Line::new(Vec2::new(-10.0, 0.0), Vec2::new(10.0, 1.0));
    let hits = Curve::ellipse(&e).intersect(&Curve::line(line));
    assert!(!hits.is_empty());
    for h in &hits {
        assert!(e.at_param(h.t1).near(h.point, 1e-12), "{h:?}");
        assert!(line.at(h.t2).near(h.point, 1e-12), "{h:?}");
        assert!(angle_in_sweep(h.t1, e.start, e.end));
    }
    // An elliptical arc only keeps the crossings on its sweep.
    let full = Curve::ellipse(&Ellipse { start: 0.0, end: TAU, ..e }).intersect(&Curve::line(line));
    assert!(full.len() >= hits.len());
}

#[test]
fn circle_and_ellipse_symmetric_case() {
    // Concentric circle of radius r between the semi-axes: four points with
    // x² = a²(r² − b²)/(a² − b²), y² = r² − x².
    let (a, b, r) = (5.0f64, 2.0f64, 3.5f64);
    let x = (a * a * (r * r - b * b) / (a * a - b * b)).sqrt();
    let y = (r * r - x * x).sqrt();
    for c in [Vec2::ZERO, Vec2::new(1e6, -2e6)] {
        let e = ellipse(c, a, b, 0.0);
        let want: Vec<Vec2> = [(x, y), (-x, y), (-x, -y), (x, -y)].iter().map(|&(x, y)| c + Vec2::new(x, y)).collect();
        let got: Vec<Vec2> = Curve::circle(Circle::new(c, r)).intersect(&Curve::ellipse(&e)).into_iter().map(|h| h.point).collect();
        assert!(matches(&got, &want, 1e-9 * a), "{c:?}: {got:?}");
        // As an arc covering the first quadrant only.
        let arc = Arc::new(c, r, 0.0, PI / 2.0);
        let got: Vec<Vec2> = Curve::arc(arc).intersect(&Curve::ellipse(&e)).into_iter().map(|h| h.point).collect();
        assert!(matches(&got, &want[..1], 1e-9 * a), "{got:?}");
    }
}

#[test]
fn ellipse_ellipse_crossings_and_touches() {
    // Crossed axes: x²/9 + y² = 1 and x² + y²/9 = 1 meet at x² = y² = 9/10.
    let k = (0.9f64).sqrt();
    let want = [Vec2::new(k, k), Vec2::new(-k, k), Vec2::new(-k, -k), Vec2::new(k, -k)];
    let e1 = ellipse(Vec2::ZERO, 3.0, 1.0, 0.0);
    let e2 = ellipse(Vec2::ZERO, 3.0, 1.0, PI / 2.0);
    let got: Vec<Vec2> = Curve::ellipse(&e1).intersect(&Curve::ellipse(&e2)).into_iter().map(|h| h.point).collect();
    assert!(matches(&got, &want, 1e-12), "{got:?}");
    // Same major axis, different minor: they touch at the two vertices.
    let e3 = ellipse(Vec2::ZERO, 3.0, 2.0, 0.0);
    let got: Vec<Vec2> = Curve::ellipse(&e1).intersect(&Curve::ellipse(&e3)).into_iter().map(|h| h.point).collect();
    assert!(matches(&got, &[Vec2::new(3.0, 0.0), Vec2::new(-3.0, 0.0)], 1e-9), "{got:?}");
    // Disjoint: none.
    let far = ellipse(Vec2::new(10.0, 0.0), 3.0, 1.0, 0.3);
    assert!(Curve::ellipse(&e1).intersect(&Curve::ellipse(&far)).is_empty());
    // Ellipse against itself: overlapping, bounded, no isolated points claimed.
    let same = Curve::ellipse(&e1).intersect(&Curve::ellipse(&e1));
    assert!(same.len() <= 512);
}

#[test]
fn tangencies_are_found_once_and_exactly() {
    let mut rng = Rng::new(11);
    for _ in 0..300 {
        let a = rng.range(1.0, 20.0);
        let b = a * rng.range(0.1, 1.0);
        let e = ellipse(Vec2::new(rng.range(-50.0, 50.0), rng.range(-50.0, 50.0)), a, b, rng.range(0.0, TAU));
        // The tangent line at a random parameter.
        let t = rng.range(0.0, TAU);
        let (p, d) = (e.at_param(t), e.derivative(t).normalized());
        let line = Line::new(p - d * (3.0 * a), p + d * (5.0 * a));
        let hits = Curve::line(line).intersect(&Curve::ellipse(&e));
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert!(hits[0].point.near(p, 1e-7 * a), "{:?} vs {p:?}", hits[0].point);
        // A line a little outside misses.
        let n = d.perp() * (on_ellipse(&e, p + d.perp() * 1e-3 * a).signum());
        let miss = Line::new(line.a + n * 1e-6 * a, line.b + n * 1e-6 * a);
        assert!(Curve::line(miss).intersect(&Curve::ellipse(&e)).is_empty());
    }
    // A circle inside an ellipse touching both ends of the minor axis.
    let e = ellipse(Vec2::new(3.0, 4.0), 5.0, 2.0, 0.0);
    let got: Vec<Vec2> = Curve::circle(Circle::new(Vec2::new(3.0, 4.0), 2.0)).intersect(&Curve::ellipse(&e)).into_iter().map(|h| h.point).collect();
    assert!(matches(&got, &[Vec2::new(3.0, 6.0), Vec2::new(3.0, 2.0)], 1e-9), "{got:?}");
}

/// Two unit circles as rational splines (from exact conics).
fn circle_spline(c: Vec2, r: f64) -> Spline {
    Spline::from_conic_arc(c, Vec2::X * r, Vec2::Y * r, 0.0, TAU).unwrap()
}

#[test]
fn spline_spline_from_exact_conics() {
    let s1 = circle_spline(Vec2::ZERO, 1.0);
    let s2 = circle_spline(Vec2::new(1.0, 0.0), 1.0);
    let h = 3f64.sqrt() / 2.0;
    let got: Vec<Vec2> = Curve::spline(&s1).intersect(&Curve::spline(&s2)).into_iter().map(|h| h.point).collect();
    assert!(matches(&got, &[Vec2::new(0.5, h), Vec2::new(0.5, -h)], 1e-12), "{got:?}");
    // Touching circles: one point.
    let s3 = circle_spline(Vec2::new(2.0, 0.0), 1.0);
    let got: Vec<Vec2> = Curve::spline(&s1).intersect(&Curve::spline(&s3)).into_iter().map(|h| h.point).collect();
    assert!(matches(&got, &[Vec2::new(1.0, 0.0)], 1e-6), "{got:?}");
    // Far from the origin, elevated to degree 3 (non-trivial weights and knots).
    let o = Vec2::new(3e6, -4e6);
    let a = circle_spline(o, 2.0).elevated(3).unwrap();
    let b = circle_spline(o + Vec2::new(0.0, 2.0), 2.0).elevated(4).unwrap();
    let x = 3f64.sqrt();
    let got: Vec<Vec2> = Curve::spline(&a).intersect(&Curve::spline(&b)).into_iter().map(|h| h.point).collect();
    assert!(matches(&got, &[o + Vec2::new(x, 1.0), o + Vec2::new(-x, 1.0)], 1e-9 * 4.0), "{got:?}");
}

#[test]
fn polynomial_spline_crossings_lie_on_both() {
    let mut rng = Rng::new(3);
    for case in 0..60 {
        let ctrl = |rng: &mut Rng| (0..6).map(|i| Vec2::new(i as f64 * 2.0, rng.range(-4.0, 4.0))).collect::<Vec<_>>();
        let s1 = Spline::from_control(ctrl(&mut rng), 3);
        // The second spline runs crosswise.
        let s2 = Spline::from_control((0..5).map(|i| Vec2::new(rng.range(0.0, 10.0), -6.0 + i as f64 * 3.0)).collect(), 3);
        let hits = Curve::spline(&s1).intersect(&Curve::spline(&s2));
        for h in &hits {
            assert!(s1.eval(h.t1).near(h.point, 1e-9) && s2.eval(h.t2).near(h.point, 1e-9), "case {case}: {h:?}");
        }
        // Against a dense polyline count of crossings (sign changes along s2 of the side of s1
        // are not available; compare with polygon intersections at a fine tolerance).
        let (p1, p2) = (s1.tessellate(1e-4), s2.tessellate(1e-4));
        let mut poly: Vec<Vec2> = Vec::new();
        for w1 in p1.windows(2) {
            for w2 in p2.windows(2) {
                if let Some(x) = line_line(&Line::new(w1[0], w1[1]), &Line::new(w2[0], w2[1]))
                    && !poly.iter().any(|q| q.near(x, 1e-2))
                {
                    poly.push(x);
                }
            }
        }
        // Near-tangent pairs of crossings are where polygon and curve may disagree.
        if poly.iter().enumerate().any(|(i, a)| poly.iter().skip(i + 1).any(|b| a.dist(*b) < 0.1)) {
            continue;
        }
        assert_eq!(hits.len(), poly.len(), "case {case}: {hits:?} vs {poly:?}");
        assert!(poly.iter().all(|q| hits.iter().any(|h| h.point.near(*q, 1e-2))));
    }
}

#[test]
fn line_and_arc_against_splines() {
    let s = circle_spline(Vec2::new(1.0, 1.0), 2.0);
    let line = Line::new(Vec2::new(-5.0, 1.0), Vec2::new(5.0, 1.0));
    let got: Vec<Vec2> = Curve::line(line).intersect(&Curve::spline(&s)).into_iter().map(|h| h.point).collect();
    assert!(matches(&got, &[Vec2::new(-1.0, 1.0), Vec2::new(3.0, 1.0)], 1e-12), "{got:?}");
    let arc = Arc::new(Vec2::new(3.0, 1.0), 2.0, PI / 2.0, 3.0 * PI / 2.0);
    let x = 2.0f64;
    let got: Vec<Vec2> = Curve::arc(arc).intersect(&Curve::spline(&s)).into_iter().map(|h| h.point).collect();
    // Circles of radius 2 with centres 2 apart meet at x = 2, y = 1 ± √3.
    let want = [Vec2::new(x, 1.0 + 3f64.sqrt()), Vec2::new(x, 1.0 - 3f64.sqrt())];
    assert!(matches(&got, &want, 1e-12), "{got:?}");
    // The window limits the search.
    let w = Bounds2::new(Vec2::new(1.5, 2.0), Vec2::new(2.5, 3.0));
    let got = Curve::arc(arc).intersect_in(&Curve::spline(&s), Some(&w));
    assert_eq!(got.len(), 1);
}

#[test]
fn closest_points_and_feet_are_normals() {
    let mut rng = Rng::new(5);
    for _ in 0..500 {
        let a = rng.range(1.0, 10.0);
        let b = a * rng.range(0.05, 1.0);
        let e = Ellipse {
            start: rng.range(0.0, TAU),
            end: rng.range(0.0, TAU),
            ..ellipse(Vec2::new(rng.range(-9.0, 9.0), 4.0), a, b, rng.range(0.0, 6.0))
        };
        let p = e.center + Vec2::new(rng.range(-2.0 * a, 2.0 * a), rng.range(-2.0 * a, 2.0 * a));
        let q = e.closest(p);
        // No sampled point is nearer.
        let sweep = e.sweep();
        let best = (0..=4000).map(|i| e.at_param(e.start + sweep * i as f64 / 4000.0).dist(p)).fold(f64::INFINITY, f64::min);
        assert!(q.dist(p) <= best + 1e-12 * a, "{} > {best}", q.dist(p));
        assert!(on_ellipse(&e, q).abs() < 1e-12);
        // Interior closest points are feet of normals.
        let t = e.closest_param(p).unwrap();
        if angle_in_sweep(t, e.start + 1e-6, e.end - 1e-6) {
            assert!(e.derivative(t).normalized().dot(q - p).abs() <= 1e-10 * a, "not normal");
        }
        // The same through the spline form.
        let s = Spline::from_ellipse(&e).unwrap();
        let qs = s.closest(p).unwrap();
        assert!((qs.dist(p) - q.dist(p)).abs() <= 1e-11 * a);
        for f in Curve::ellipse(&e).perpendiculars(p) {
            assert!(e.derivative(f.t).normalized().dot(f.point - p).abs() <= 1e-10 * a);
        }
    }
    // A point on the ellipse is its own closest point.
    let e = ellipse(Vec2::ZERO, 4.0, 1.0, 0.2);
    let on = e.at_param(1.3);
    assert!(e.closest(on).near(on, 1e-12));
    // The four normals from inside, as from the old sampled code.
    let e = ellipse(Vec2::ZERO, 4.0, 2.0, 0.0);
    assert_eq!(Curve::ellipse(&e).perpendiculars(Vec2::new(1.0, 0.5)).len(), 4);
    // A circle (as an ellipse or a spline) seen from its centre has no particular foot.
    assert!(Curve::ellipse(&ellipse(Vec2::ZERO, 1.0, 1.0, 0.0)).perpendiculars(Vec2::ZERO).is_empty());
    assert!(Curve::spline(&circle_spline(Vec2::ZERO, 1.0)).perpendiculars(Vec2::ZERO).is_empty());
}

#[test]
fn spline_tangents_match_the_ellipse_closed_form() {
    let e = ellipse(Vec2::new(1.0, 2.0), 4.0, 2.0, 0.4);
    let s = Spline::from_ellipse(&e).unwrap();
    for p in [Vec2::new(9.0, 3.0), Vec2::new(-6.0, -6.0), Vec2::new(1.0, 9.0)] {
        let want: Vec<Vec2> = Curve::ellipse(&e).tangents(p).into_iter().map(|c| c.point).collect();
        let got: Vec<Vec2> = Curve::spline(&s).tangents(p).into_iter().map(|c| c.point).collect();
        assert_eq!(want.len(), 2);
        assert!(matches(&got, &want, 1e-10), "{got:?} vs {want:?}");
    }
    // From inside: none.
    assert!(Curve::spline(&s).tangents(e.center).is_empty());
}

/// The largest deviation of `off` from lying `d` from the source curve (`dist` measures).
fn offset_error(off: &Spline, dist: impl Fn(Vec2) -> f64, d: f64) -> f64 {
    let (lo, hi) = off.domain();
    (0..=4000).map(|i| (dist(off.eval(lo + (hi - lo) * i as f64 / 4000.0)) - d.abs()).abs()).fold(0.0, f64::max)
}

#[test]
fn ellipse_offset_is_a_spline_within_tolerance() {
    let e = ellipse(Vec2::new(3.0, -2.0), 10.0, 4.0, 0.3);
    // Inward offsets up to the smallest radius of curvature (b²/a = 1.6) stay simple curves.
    for d in [2.0f64, -1.5, 0.5, 5.0] {
        let tol = 1e-8 * (10.0 + d.abs());
        let off = e.offset_spline(d, 0.0).unwrap();
        assert!(off.closed && off.is_periodic(), "{off:?}");
        let err = offset_error(&off, |q| e.closest(q).dist(q), d);
        assert!(err <= tol, "d {d}: error {err:e} > {tol:e}");
        // Outward offsets grow, inward ones shrink.
        let r = off.eval(off.domain().0).dist(e.center);
        assert!(if d > 0.0 { r > 10.0 } else { r < 10.0 });
        eprintln!("ellipse offset {d}: {} control points, error {err:e}", off.control.len());
        assert!(off.control.len() < 400, "{} control points", off.control.len());
    }
    // Deeper inward offsets keep their swallowtails (as the exact offset has them).
    let deep = e.offset_spline(-3.9, 0.0).unwrap();
    assert!(deep.control.iter().all(|c| c.is_finite()));
    // An elliptical arc gives an open spline running from the offset start to the offset end.
    let arc = Ellipse { start: 0.5, end: 2.5, ..e };
    let off = arc.offset_spline(1.0, 0.0).unwrap();
    assert!(!off.closed);
    let n = e.derivative(0.5).perp().normalized();
    assert!(off.eval(off.domain().0).near(arc.at_param(0.5) - n * 1.0, 1e-9));
    // Collapsing inward offsets and degenerate ellipses give nothing.
    assert!(e.offset_spline(-4.0, 0.0).is_none());
    assert!(Ellipse::full(Vec2::ZERO, Vec2::ZERO, 0.5).offset_spline(1.0, 0.0).is_none());
    assert!(e.offset_spline(f64::NAN, 0.0).is_none());
}

#[test]
fn spline_offset_is_within_tolerance() {
    // A circle as a spline offsets to the concentric circle.
    let c = Spline { closed: true, ..circle_spline(Vec2::new(1.0, 1.0), 3.0) };
    let off = c.offset(-1.0, 0.0).unwrap();
    assert!(off.closed);
    let err = offset_error(&off, |q| (q.dist(Vec2::new(1.0, 1.0)) - 3.0).abs(), 1.0);
    assert!(err <= 1e-8 * (2.0 * 6f64.hypot(6.0) + 1.0), "{err:e}");
    // A periodic fit spline stays closed and periodic.
    let fit = [Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)];
    let p = Spline::from_fit(&fit, true);
    let off = p.offset(1.0, 0.0).unwrap();
    assert!(off.closed && off.is_periodic());
    let (lo, hi) = off.domain();
    assert!(off.eval(lo).near(off.eval(hi), 1e-9));
    let err = offset_error(&off, |q| p.closest(q).unwrap().dist(q), 1.0);
    assert!(err <= 1e-8 * 16.0, "{err:e}");
    // An open cubic.
    let s = Spline::from_control(vec![Vec2::new(0.0, 0.0), Vec2::new(3.0, 5.0), Vec2::new(6.0, -3.0), Vec2::new(9.0, 2.0), Vec2::new(12.0, 0.0)], 3);
    for d in [0.5, -0.5] {
        let off = s.offset(d, 0.0).unwrap();
        let err = offset_error(&off, |q| s.closest(q).unwrap().dist(q), d);
        assert!(err <= 1e-8 * 15.0, "{d}: {err:e}");
        // The left side for positive distances.
        let (lo, _) = s.domain();
        let n = s.derivative(lo).perp().normalized();
        assert!(off.eval(off.domain().0).near(s.eval(lo) + n * d, 1e-9));
    }
    // A corner (a degree-1 spline): the outside is bridged by a round arc.
    let v = Spline::from_control(vec![Vec2::new(0.0, 0.0), Vec2::new(5.0, 0.0), Vec2::new(5.0, 5.0)], 1);
    let off = v.offset(-1.0, 0.0).unwrap();
    let (lo, hi) = off.domain();
    assert!(off.eval(lo).near(Vec2::new(0.0, -1.0), 1e-9) && off.eval(hi).near(Vec2::new(6.0, 5.0), 1e-9));
    let err = offset_error(&off, |q| v.closest(q).unwrap().dist(q), 1.0);
    assert!(err <= 1e-7, "{err:e}");
}

#[test]
fn hostile_input_never_panics() {
    let nan = f64::NAN;
    let ellipses = [
        Ellipse::full(Vec2::ZERO, Vec2::ZERO, 0.5),
        Ellipse::full(Vec2::ZERO, Vec2::X, 0.0),
        Ellipse::full(Vec2::new(nan, 0.0), Vec2::X, 0.5),
        Ellipse::full(Vec2::ZERO, Vec2::new(1e300, 1e300), 1e-300),
        Ellipse { center: Vec2::ZERO, major: Vec2::X, ratio: 0.5, start: nan, end: 1.0 },
        Ellipse::full(Vec2::new(1e15, 1e15), Vec2::new(1e-6, 0.0), 0.5),
    ];
    let mut splines = vec![
        Spline::from_control(vec![Vec2::ZERO; 4], 3),
        Spline::from_control(vec![Vec2::ZERO, Vec2::ZERO, Vec2::X, Vec2::X], 3),
        Spline { degree: 3, knots: vec![0.0; 8], control: vec![Vec2::X; 4], ..Spline::default() },
        Spline { degree: 40, knots: vec![], control: vec![Vec2::X; 2], ..Spline::default() },
        Spline::from_control(vec![Vec2::new(1e300, 0.0), Vec2::new(-1e300, 1e300), Vec2::new(0.0, -1e300)], 2),
    ];
    let mut w = Spline::from_control(vec![Vec2::ZERO, Vec2::X, Vec2::Y, Vec2::new(1.0, 1.0)], 3);
    w.weights = vec![1.0, -1.0, 0.0, nan];
    splines.push(w);
    let mut big = Spline::from_control((0..200).map(|i| Vec2::new(i as f64, (i % 2) as f64)).collect(), 3);
    big.weights = (0..200).map(|i| 1.0 + (i % 3) as f64).collect();
    splines.push(big);
    let mut curves: Vec<Curve> = ellipses.iter().map(Curve::ellipse).collect();
    curves.extend(splines.iter().map(Curve::spline));
    curves.push(Curve::line(Line::new(Vec2::ZERO, Vec2::ZERO)));
    curves.push(Curve::line(Line::new(Vec2::new(-1e8, 0.5), Vec2::new(1e8, 0.5))));
    curves.push(Curve::circle(Circle::new(Vec2::ZERO, 0.0)));
    curves.push(Curve::circle(Circle::new(Vec2::ZERO, nan)));
    for a in &curves {
        for p in [Vec2::ZERO, Vec2::new(nan, 1.0), Vec2::new(1e300, -1e300), Vec2::new(0.5, 0.5)] {
            let _ = a.closest(p);
            let _ = a.perpendiculars(p);
            let _ = a.tangents(p);
        }
        for b in &curves {
            let hits = a.intersect(b);
            assert!(hits.len() <= 512);
            assert!(hits.iter().all(|h| h.point.is_finite()));
        }
    }
    for e in &ellipses {
        let _ = e.offset_spline(1.0, 0.0);
        let _ = e.offset_spline(-1.0, 1e-300);
        let _ = e.closest(Vec2::new(0.3, 0.2));
    }
    for s in &splines {
        let _ = s.offset(1.0, 0.0);
        let _ = s.offset(1e300, 0.0);
        let _ = s.closest(Vec2::new(0.3, 0.2));
    }
}

#[test]
fn huge_coordinates_keep_relative_accuracy() {
    // A small ellipse at survey coordinates: the line crossings stay exact relative to its size.
    let o = Vec2::new(4.5e6, 5.2e6);
    let e = ellipse(o, 3.0, 1.0, 0.7);
    let d = Vec2::from_angle(0.2);
    let want = line_ellipse_analytic(&e, o, d);
    let got: Vec<Vec2> = Curve::line(Line::new(o - d * 10.0, o + d * 10.0)).intersect(&Curve::ellipse(&e)).into_iter().map(|h| h.point).collect();
    assert!(matches(&got, &want, 1e-9 * 3.0 + 4.0 * f64::EPSILON * o.len()), "{got:?} vs {want:?}");
    let q = e.closest(o + Vec2::new(5.0, 1.0));
    assert!(on_ellipse(&e, q).abs() < 1e-8);
}

// ---------- old (tessellated) snap computations, for the timing comparison ----------

fn old_chords_ellipse(e: &Ellipse) -> Vec<Segment> {
    let mut pts = Vec::new();
    e.tessellate(e.major.len() * 1e-3, &mut pts);
    pts.windows(2).map(|w| Segment::Line(Line::new(w[0], w[1]))).collect()
}

fn old_chords_spline(s: &Spline) -> Vec<Segment> {
    s.tessellate(1e-3).windows(2).map(|w| Segment::Line(Line::new(w[0], w[1]))).collect()
}

/// The old sampled root search of `snap/curves.rs` (512 samples, 60 bisections per root).
fn old_solve(at: &dyn Fn(f64) -> Vec2, d: &dyn Fn(f64) -> Vec2, lo: f64, hi: f64, base: Vec2, cond: fn(Vec2, Vec2) -> f64) -> Vec<Vec2> {
    let n = 512;
    let par = |i: usize| lo + (hi - lo) * i as f64 / n as f64;
    let g = |t: f64| cond(at(t) - base, d(t));
    let vals: Vec<f64> = (0..=n).map(|i| g(par(i))).collect();
    let mut out = Vec::new();
    for i in 0..n {
        let (ga, gb) = (vals[i], vals[i + 1]);
        if (ga > 0.0) == (gb > 0.0) && ga != 0.0 {
            continue;
        }
        let (mut a, mut b, mut fa) = (par(i), par(i + 1), ga);
        for _ in 0..60 {
            let m = 0.5 * (a + b);
            let fm = g(m);
            if (fm > 0.0) == (fa > 0.0) {
                (a, fa) = (m, fm);
            } else {
                b = m;
            }
        }
        out.push(at(0.5 * (a + b)));
    }
    out
}

/// Timing of snap-like queries on a large drawing of ellipses and splines: the old tessellated
/// computations against the exact ones. Run with `cargo test -p cadcraft-geom --lib -- --ignored
/// bench_snap --nocapture`.
#[test]
#[ignore]
fn bench_snap_queries() {
    use std::time::Instant;
    let mut rng = Rng::new(99);
    let mut ellipses = Vec::new();
    let mut splines = Vec::new();
    for _ in 0..300 {
        let c = Vec2::new(rng.range(0.0, 1000.0), rng.range(0.0, 1000.0));
        let a = rng.range(5.0, 60.0);
        ellipses.push(ellipse(c, a, a * rng.range(0.2, 1.0), rng.range(0.0, TAU)));
        let fit: Vec<Vec2> = (0..8).map(|i| c + Vec2::new(i as f64 * 12.0, rng.range(-30.0, 30.0))).collect();
        splines.push(Spline::from_fit_points(&fit));
    }
    // Cursors on the curves, aperture 2 units; bases for PER/TAN off to the side.
    let cursors: Vec<(Vec2, Vec2)> = (0..400)
        .map(|i| {
            let p = if i % 2 == 0 {
                let e = &ellipses[i % ellipses.len()];
                e.at_param(rng.range(0.0, TAU))
            } else {
                let s = &splines[i % splines.len()];
                let (lo, hi) = s.domain();
                s.eval(rng.range(lo, hi))
            };
            (p, p + Vec2::new(rng.range(-50.0, 50.0), rng.range(-50.0, 50.0)))
        })
        .collect();
    let aperture = 2.0;
    let near = |p: Vec2| -> (Vec<&Ellipse>, Vec<&Spline>) {
        let probe = Bounds2::new(p, p).expand(aperture);
        (
            ellipses.iter().filter(|e| e.bounds().expand(aperture).intersects(&probe)).collect(),
            splines.iter().filter(|s| s.bounds().expand(aperture).intersects(&probe)).collect(),
        )
    };
    let gathered: Vec<_> = cursors.iter().map(|(p, b)| (*p, *b, near(*p))).collect();

    let t = Instant::now();
    let mut old_count = 0usize;
    for (p, base, (es, ss)) in &gathered {
        let probe = Bounds2::new(*p, *p).expand(aperture);
        let mut segs: Vec<Vec<Segment>> = es.iter().map(|e| old_chords_ellipse(e)).chain(ss.iter().map(|s| old_chords_spline(s))).collect();
        for v in &mut segs {
            v.retain(|s| s.bounds().intersects(&probe));
        }
        for i in 0..segs.len() {
            for j in i + 1..segs.len() {
                for a in &segs[i] {
                    for b in &segs[j] {
                        old_count += intersect_ext(a, b, false).into_iter().filter(|x| x.dist(*p) <= aperture).count();
                    }
                }
            }
        }
        for e in es {
            let (u, v) = (e.major, e.minor());
            let at = |t: f64| e.at_param(t);
            let d = |t: f64| v * t.cos() - u * t.sin();
            old_count += old_solve(&at, &d, e.start, e.start + e.sweep(), *base, |r, d| r.dot(d)).len();
            let mut pts = Vec::new();
            e.tessellate(e.major.len() * 1e-3, &mut pts);
            old_count += pts.windows(2).map(|w| Line::new(w[0], w[1]).closest(*p).dist(*p)).fold(f64::INFINITY, f64::min).is_finite() as usize;
        }
        for s in ss {
            let (lo, hi) = s.domain();
            let h = (hi - lo) * 1e-7;
            let at = |t: f64| s.eval(t);
            let d = |t: f64| (s.eval((t + h).min(hi)) - s.eval((t - h).max(lo))) / (2.0 * h);
            old_count += old_solve(&at, &d, lo, hi, *base, |r, d| r.dot(d)).len();
            old_count += old_solve(&at, &d, lo, hi, *base, |r, d| r.cross(d)).len();
            let pts = s.tessellate(1e-3);
            old_count += pts.windows(2).map(|w| Line::new(w[0], w[1]).closest(*p).dist(*p)).fold(f64::INFINITY, f64::min).is_finite() as usize;
        }
    }
    let old = t.elapsed();

    let t = Instant::now();
    let mut new_count = 0usize;
    for (p, base, (es, ss)) in &gathered {
        let probe = Bounds2::new(*p, *p).expand(aperture);
        let curves: Vec<Curve> = es.iter().map(|e| Curve::ellipse(e)).chain(ss.iter().map(|s| Curve::spline(s))).collect();
        for i in 0..curves.len() {
            for j in i + 1..curves.len() {
                new_count += curves[i].intersect_in(&curves[j], Some(&probe)).into_iter().filter(|h| h.point.dist(*p) <= aperture).count();
            }
        }
        for (k, c) in curves.iter().enumerate() {
            new_count += c.perpendiculars(*base).len();
            if k >= es.len() {
                new_count += c.tangents(*base).len();
            }
            new_count += c.closest(*p).is_some() as usize;
        }
    }
    let new = t.elapsed();
    eprintln!(
        "{} snap queries over {} ellipses and {} splines: old tessellated {:?} ({} results), exact {:?} ({} results), speed-up {:.1}x",
        gathered.len(),
        ellipses.len(),
        splines.len(),
        old,
        old_count,
        new,
        new_count,
        old.as_secs_f64() / new.as_secs_f64().max(1e-9)
    );
}
