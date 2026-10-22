//! Ellipses and splines are exact in snaps, TRIM, EXTEND and OFFSET (roadmap phase 2, gaps.md
//! top-10 row 3): intersections lie on both curves to rounding, not on a tessellation, and
//! OFFSET of an ellipse gives a spline (as AutoCAD does) within tolerance of the true offset.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle, Space};
use cadcraft_engine::geom::{Ellipse, Spline, Vec2};
use cadcraft_engine::snap::{self, mode};
use serde_json::{Value, json};

fn h(r: &Value) -> String {
    r["handle"].as_str().or_else(|| r["handles"][0].as_str()).unwrap().to_string()
}

fn kind(s: &Session, h: &str) -> EntityKind {
    s.doc().unwrap().entity(Handle::parse_hex(h).unwrap()).unwrap().kind.clone()
}

/// (x/5)² + (y/2)² − 1 for the test ellipse centred at (1, 1).
fn on_test_ellipse(p: Vec2) -> f64 {
    let q = p - Vec2::new(1.0, 1.0);
    (q.x / 5.0).powi(2) + (q.y / 2.0).powi(2) - 1.0
}

fn test_ellipse(s: &mut Session) -> String {
    h(&s.execute("ellipse", &json!({ "center": [1, 1], "major": [5, 0], "ratio": 0.4 })).unwrap())
}

#[test]
fn intersection_snap_on_an_ellipse_is_exact() {
    let mut s = Session::new();
    test_ellipse(&mut s);
    s.execute("line", &json!({ "points": [[-10, 2.3], [10, 2.7]] })).unwrap();
    // Where the line crosses the right side of the ellipse (near (4.07, 2.58)).
    let d = s.doc().unwrap();
    let hit = snap::osnap(d, &Space::Model, Vec2::new(4.1, 2.6), 0.5, mode::INT, None, false).expect("intersection");
    assert_eq!(hit.mode, mode::INT);
    let p = hit.point;
    assert!(on_test_ellipse(p).abs() < 1e-14, "off the ellipse by {}", on_test_ellipse(p));
    assert!((p.y - (2.3 + 0.02 * (p.x + 10.0))).abs() < 1e-13, "off the line");
    // Nearest is on the ellipse too, and perpendicular from a base point is a true normal foot.
    let n = snap::osnap(d, &Space::Model, Vec2::new(1.3, 3.2), 0.5, mode::NEA, None, false).expect("nearest");
    assert!(on_test_ellipse(n.point).abs() < 1e-14);
    let base = Vec2::new(1.5, 8.0);
    let per = snap::osnap(d, &Space::Model, Vec2::new(1.6, 3.0), 0.5, mode::PER, Some(base), false).expect("perpendicular");
    let q = per.point - Vec2::new(1.0, 1.0);
    let tangent = Vec2::new(-q.y * 5.0 / 2.0, q.x * 2.0 / 5.0);
    assert!(tangent.normalized().dot(per.point - base).abs() < 1e-12);
}

#[test]
fn intersection_snap_between_spline_and_ellipse() {
    let mut s = Session::new();
    test_ellipse(&mut s);
    let sp = h(&s.execute("spline", &json!({ "control": [[-6, -2], [-2, 6], [2, -4], [8, 5]] })).unwrap());
    let EntityKind::Spline(spline) = kind(&s, &sp) else { panic!("not a spline") };
    let d = s.doc().unwrap();
    let mut found = 0;
    // Probe along the spline: every intersection snap found lies on both curves.
    let (lo, hi) = spline.domain();
    for i in 0..=200 {
        let c = spline.eval(lo + (hi - lo) * i as f64 / 200.0);
        if let Some(hit) = snap::osnap(d, &Space::Model, c, 0.2, mode::INT, None, false) {
            assert!(on_test_ellipse(hit.point).abs() < 1e-12, "{hit:?}");
            let t = spline.closest_param(hit.point).unwrap();
            assert!(spline.eval(t).dist(hit.point) < 1e-12, "{hit:?}");
            found += 1;
        }
    }
    assert!(found > 0);
}

#[test]
fn trim_and_extend_against_an_ellipse_land_on_it() {
    let mut s = Session::new();
    let e = test_ellipse(&mut s);
    let l = h(&s.execute("line", &json!({ "points": [[-10, 1.7], [10, 1.2]] })).unwrap());
    s.execute("trim", &json!({ "handle": l, "pick": [9, 1.225], "edges": [e] })).unwrap();
    let EntityKind::Line(line) = s.doc().unwrap().model.iter().last().unwrap().kind.clone() else { panic!("not a line") };
    assert!(on_test_ellipse(line.b.xy()).abs() < 1e-14, "{line:?}");
    // EXTEND a short line up to the ellipse.
    let k = h(&s.execute("line", &json!({ "points": [[0.3, 0.1], [0.9, 0.4]] })).unwrap());
    s.execute("extend", &json!({ "handle": k, "pick": [0.85, 0.38], "edges": [e] })).unwrap();
    let EntityKind::Line(line) = kind(&s, &k) else { panic!("not a line") };
    assert!(on_test_ellipse(line.b.xy()).abs() < 1e-14, "{line:?}");
    // A spline trimmed by the ellipse ends exactly on it.
    let sp = h(&s.execute("spline", &json!({ "control": [[-8, 0], [-3, 4], [3, -3], [9, 2]] })).unwrap());
    s.execute("trim", &json!({ "handle": sp, "pick": [-7.9, 0.1], "edges": [e] })).unwrap();
    let EntityKind::Spline(rest) = s.doc().unwrap().model.iter().last().unwrap().kind.clone() else { panic!("not a spline") };
    let (lo, _) = rest.domain();
    assert!(on_test_ellipse(rest.eval(lo)).abs() < 1e-12, "{:?}", rest.eval(lo));
}

#[test]
fn offset_of_an_ellipse_is_a_spline_within_tolerance() {
    let mut s = Session::new();
    let e = test_ellipse(&mut s);
    s.execute("offset", &json!({ "handle": e, "distance": 1.5, "side": [9, 1] })).unwrap();
    let EntityKind::Spline(off) = s.doc().unwrap().model.iter().last().unwrap().kind.clone() else { panic!("not a spline") };
    assert!(off.closed);
    let g = Ellipse::full(Vec2::new(1.0, 1.0), Vec2::new(5.0, 0.0), 0.4);
    let (lo, hi) = off.domain();
    for i in 0..=2000 {
        let p = off.eval(lo + (hi - lo) * i as f64 / 2000.0);
        let d = g.closest(p).dist(p);
        assert!((d - 1.5).abs() < 1e-6, "{p:?}: {d}");
    }
    // Inward from inside.
    s.execute("offset", &json!({ "handle": e, "distance": 0.5, "side": [1, 1.2] })).unwrap();
    let EntityKind::Spline(inner) = s.doc().unwrap().model.iter().last().unwrap().kind.clone() else { panic!("not a spline") };
    let p = inner.eval(inner.domain().0);
    assert!((g.closest(p).dist(p) - 0.5).abs() < 1e-6 && p.dist(Vec2::new(1.0, 1.0)) < 5.0);
}

#[test]
fn offset_of_a_spline_stays_at_the_distance() {
    let mut s = Session::new();
    let sp = h(&s.execute("spline", &json!({ "control": [[0, 0], [3, 3], [6, -3], [9, 0]] })).unwrap());
    let EntityKind::Spline(src) = kind(&s, &sp) else { panic!("not a spline") };
    s.execute("offset", &json!({ "handle": sp, "distance": 0.5, "side": [1, 2] })).unwrap();
    let EntityKind::Spline(off) = s.doc().unwrap().model.iter().last().unwrap().kind.clone() else { panic!("not a spline") };
    let (lo, hi) = off.domain();
    for i in 0..=1000 {
        let p = off.eval(lo + (hi - lo) * i as f64 / 1000.0);
        let d = src.closest(p).unwrap().dist(p);
        assert!((d - 0.5).abs() < 1e-6, "{p:?}: {d}");
    }
    // On the picked side: (1, 2) is above the start of the curve, which heads up-right.
    let start = off.eval(lo);
    assert!(start.y > 0.0 && start.x < 0.0, "{start:?}");
    let _ = Spline::default();
}
