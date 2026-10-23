//! FILLET with a full circle (#1559): the circle stays as it is, the other object is trimmed or
//! extended to its tangent point, and the new arc is the short arc that runs on smoothly from
//! the kept part of each object. Also arcs, and hostile radii.

use std::f64::consts::PI;

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle};
use cadcraft_engine::geom::Vec2;
use serde_json::{Value, json};

fn h(r: &Value) -> String {
    r["handle"].as_str().or_else(|| r["handles"][0].as_str()).unwrap().to_string()
}

fn kind(s: &Session, h: &str) -> EntityKind {
    s.doc().unwrap().entity(Handle::parse_hex(h).unwrap()).unwrap().kind.clone()
}

/// Every object in model space, for "nothing changed" checks.
fn model(s: &Session) -> String {
    format!("{:?}", s.doc().unwrap().model.iter().map(|e| (e.handle, e.kind.clone())).collect::<Vec<_>>())
}

fn arcs(s: &Session) -> Vec<(Vec2, f64, f64, f64)> {
    s.doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| if let EntityKind::Arc(a) = &e.kind { Some((a.center.xy(), a.radius, a.start, a.end)) } else { None })
        .collect()
}

/// A circle of radius 50 at the origin and a vertical line at x = 100.
fn circle_and_line(s: &mut Session) -> (String, String) {
    let c = h(&s.execute("circle", &json!({"center": [0, 0], "radius": 50})).unwrap());
    let l = h(&s.execute("line", &json!({"points": [[100, -100], [100, 100]]})).unwrap());
    (c, l)
}

fn assert_circle_unchanged(s: &Session, c: &str) {
    match kind(s, c) {
        EntityKind::Circle(k) => assert!(k.center.xy().near(Vec2::ZERO, 1e-12) && k.radius == 50.0, "{k:?}"),
        k => panic!("the circle became {k:?}"),
    }
}

/// The fillet arc's direction leaving its end at `t`, into the arc.
fn leaving(center: Vec2, start: f64, end: f64, t: Vec2) -> Vec2 {
    let ccw = (t - center).perp().normalized();
    let at_start = (t - center).normalized().dot(Vec2::from_angle(start)) > (t - center).normalized().dot(Vec2::from_angle(end));
    if at_start { ccw } else { -ccw }
}

/// The one fillet arc: radius 30, shorter than a half circle, tangent to the circle and to the
/// line, which now ends at the tangent point and runs on into the arc. Returns its centre.
fn assert_fillet(s: &Session, c: &str, l: &str) -> Vec2 {
    assert_circle_unchanged(s, c);
    let a = arcs(s);
    assert_eq!(a.len(), 1, "{a:?}");
    let (fc, fr, start, end) = a[0];
    assert!((fr - 30.0).abs() < 1e-9, "{fr}");
    let sweep = (end - start).rem_euclid(2.0 * PI);
    assert!(sweep > 1e-6 && sweep < PI, "the fillet arc loops: {sweep}");
    let (s_pt, e_pt) = (fc + Vec2::from_angle(start) * fr, fc + Vec2::from_angle(end) * fr);
    // Tangent to the circle: centres 50 + 30 or 50 - 30 apart, and one arc end on the circle.
    let d = fc.len();
    assert!((d - 80.0).abs() < 1e-6 || (d - 20.0).abs() < 1e-6, "{d}");
    assert!([s_pt, e_pt].iter().any(|p| (p.len() - 50.0).abs() < 1e-6), "{s_pt:?} {e_pt:?}");
    // Tangent to the line, which ends where the arc starts and runs on into it.
    let EntityKind::Line(ln) = kind(s, l) else { panic!("not a line") };
    let (la, lb) = (ln.a.xy(), ln.b.xy());
    let (t, keep) = if [s_pt, e_pt].iter().any(|p| p.near(la, 1e-6)) { (la, lb) } else { (lb, la) };
    assert!(t.near(s_pt, 1e-6) || t.near(e_pt, 1e-6), "line {la:?}-{lb:?} misses the arc {s_pt:?} {e_pt:?}");
    assert!(((t - fc).len() - 30.0).abs() < 1e-6 && (t - fc).dot(keep - t).abs() < 1e-6, "not tangent at {t:?}");
    assert!((t - keep).normalized().dot(leaving(fc, start, end, t)) > 0.999, "the line meets the arc at a cusp");
    fc
}

#[test]
fn fillet_keeps_the_circle_and_follows_the_picks() {
    // Picks above, then below: the fillet sits on the picked side, the line keeps the picked part.
    for (lp, cp, cy, keep) in [([100, 80], [30, 40], 1.0, [100.0, 100.0]), ([100, -80], [30, -40], -1.0, [100.0, -100.0])] {
        for circle_first in [true, false] {
            let mut s = Session::new();
            let (c, l) = circle_and_line(&mut s);
            let p = if circle_first {
                json!({"h1": c, "p1": cp, "h2": l, "p2": lp, "radius": 30})
            } else {
                json!({"h1": l, "p1": lp, "h2": c, "p2": cp, "radius": 30})
            };
            let r = s.execute("fillet", &p).unwrap();
            assert!(r["arc"].is_string(), "{r}");
            let fc = assert_fillet(&s, &c, &l);
            // Centre 30 left of the line and 80 from the circle's centre.
            assert!(fc.near(Vec2::new(70.0, cy * 1500f64.sqrt()), 1e-6), "{fc:?}");
            let EntityKind::Line(ln) = kind(&s, &l) else { panic!() };
            let keep = Vec2::new(keep[0], keep[1]);
            assert!(ln.a.xy().near(keep, 1e-9) || ln.b.xy().near(keep, 1e-9), "{ln:?}");
        }
    }
}

#[test]
fn fillet_circle_and_line_at_the_command_line() {
    // As reported: radius 30, click the circle, then the line.
    let mut s = Session::new();
    let (c, l) = circle_and_line(&mut s);
    s.script("FILLET R 30 30,40 100,80\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(assert_fillet(&s, &c, &l).y > 0.0);
    // Picking the line between the two possible tangent points used to loop the arc round.
    for lp in ["100,0", "100,10", "100,-10"] {
        let mut s = Session::new();
        let (c, l) = circle_and_line(&mut s);
        s.script(&format!("FILLET R 30 30,40 {lp}\n")).unwrap();
        assert!(s.running.is_none(), "{}", s.prompt_text());
        assert!(assert_fillet(&s, &c, &l).y > 0.0, "the circle pick chooses the side");
    }
    // No trim: the line stays too.
    let mut s = Session::new();
    let (c, l) = circle_and_line(&mut s);
    s.script("FILLET T N R 30 30,40 100,80\n").unwrap();
    assert_circle_unchanged(&s, &c);
    assert!(matches!(kind(&s, &l), EntityKind::Line(x) if x.a.xy() == Vec2::new(100.0, -100.0) && x.b.xy() == Vec2::new(100.0, 100.0)));
    assert_eq!(arcs(&s).len(), 1);
}

#[test]
fn fillet_circle_and_crossing_lines() {
    // A line across the circle, picked outside: the fillet is outside, the line is trimmed back.
    let mut s = Session::new();
    let c = h(&s.execute("circle", &json!({"center": [0, 0], "radius": 50})).unwrap());
    let l = h(&s.execute("line", &json!({"points": [[-100, 20], [100, 20]]})).unwrap());
    s.execute("fillet", &json!({"h1": l, "p1": [80, 20], "h2": c, "p2": [30, 40], "radius": 30})).unwrap();
    let fc = assert_fillet(&s, &c, &l);
    assert!((fc.len() - 80.0).abs() < 1e-6 && fc.x > 0.0 && fc.y > 20.0, "{fc:?}");
    // Picked inside: the fillet is inside the circle (internally tangent).
    let mut s = Session::new();
    let c = h(&s.execute("circle", &json!({"center": [0, 0], "radius": 50})).unwrap());
    let l = h(&s.execute("line", &json!({"points": [[-100, 20], [100, 20]]})).unwrap());
    s.execute("fillet", &json!({"h1": l, "p1": [0, 20], "h2": c, "p2": [40, -30], "radius": 30})).unwrap();
    let fc = assert_fillet(&s, &c, &l);
    assert!((fc.len() - 20.0).abs() < 1e-6 && fc.x > 0.0 && fc.y < 20.0, "{fc:?}");
    assert!(matches!(kind(&s, &l), EntityKind::Line(x) if x.a.xy() == Vec2::new(-100.0, 20.0)));
    // Through the centre.
    let mut s = Session::new();
    let c = h(&s.execute("circle", &json!({"center": [0, 0], "radius": 50})).unwrap());
    let l = h(&s.execute("line", &json!({"points": [[-100, 0], [100, 0]]})).unwrap());
    s.execute("fillet", &json!({"h1": c, "p1": [30, 40], "h2": l, "p2": [80, 0], "radius": 30})).unwrap();
    assert_fillet(&s, &c, &l);
    // Tangent to the circle: no zero-length arc where they touch.
    let mut s = Session::new();
    let c = h(&s.execute("circle", &json!({"center": [0, 0], "radius": 50})).unwrap());
    let l = h(&s.execute("line", &json!({"points": [[-100, 50], [100, 50]]})).unwrap());
    s.execute("fillet", &json!({"h1": c, "p1": [0, 50], "h2": l, "p2": [10, 50], "radius": 30})).unwrap();
    assert_fillet(&s, &c, &l);
}

#[test]
fn fillet_circle_degenerate_radii_and_positions() {
    let hostile = [json!(-1), json!(-30), json!(1e308), json!(f64::MAX), json!("NaN"), json!(null), json!(1e-300)];
    for radius in hostile {
        let mut s = Session::new();
        let (c, l) = circle_and_line(&mut s);
        let before = model(&s);
        let r = s.execute("fillet", &json!({"h1": c, "p1": [30, 40], "h2": l, "p2": [100, 80], "radius": radius}));
        // An error leaves the drawing alone; otherwise the circle is still whole.
        if r.is_err() {
            assert_eq!(model(&s), before, "{radius}");
        }
        assert_circle_unchanged(&s, &c);
        assert!(arcs(&s).iter().all(|a| a.0.is_finite() && a.1.is_finite()), "{radius}");
    }
    // A negative radius is refused, not taken as zero.
    let mut s = Session::new();
    let c = h(&s.execute("circle", &json!({"center": [0, 0], "radius": 50})).unwrap());
    let l = h(&s.execute("line", &json!({"points": [[0, 0], [100, 0]]})).unwrap());
    let before = model(&s);
    assert!(s.execute("fillet", &json!({"h1": c, "p1": [50, 0], "h2": l, "p2": [80, 0], "radius": -1})).is_err());
    assert_eq!(model(&s), before);
    // Zero radius and a line that misses the circle: nothing to meet.
    let mut s = Session::new();
    let (c, l) = circle_and_line(&mut s);
    let before = model(&s);
    assert!(s.execute("fillet", &json!({"h1": c, "p1": [30, 40], "h2": l, "p2": [100, 80], "radius": 0})).is_err());
    assert_eq!(model(&s), before);
    // Zero radius on a crossing line: the line is cut at the circle, the circle stays.
    let mut s = Session::new();
    let c = h(&s.execute("circle", &json!({"center": [0, 0], "radius": 50})).unwrap());
    let l = h(&s.execute("line", &json!({"points": [[0, 0], [100, 0]]})).unwrap());
    s.execute("fillet", &json!({"h1": c, "p1": [50, 0], "h2": l, "p2": [80, 0], "radius": 0})).unwrap();
    assert_circle_unchanged(&s, &c);
    assert!(matches!(kind(&s, &l), EntityKind::Line(x) if x.a.xy().near(Vec2::new(50.0, 0.0), 1e-9) && x.b.x == 100.0));
    assert!(arcs(&s).is_empty());
    // Too far apart for the radius: an error, nothing changed.
    let mut s = Session::new();
    let c = h(&s.execute("circle", &json!({"center": [0, 0], "radius": 50})).unwrap());
    let l = h(&s.execute("line", &json!({"points": [[200, -100], [200, 100]]})).unwrap());
    let before = model(&s);
    assert!(s.execute("fillet", &json!({"h1": c, "p1": [50, 0], "h2": l, "p2": [200, 0], "radius": 30})).is_err());
    assert_eq!(model(&s), before);
    // The radius equal to the circle's.
    let mut s = Session::new();
    let c = h(&s.execute("circle", &json!({"center": [0, 0], "radius": 50})).unwrap());
    let l = h(&s.execute("line", &json!({"points": [[100, -100], [100, 100]]})).unwrap());
    let _ = s.execute("fillet", &json!({"h1": c, "p1": [30, 40], "h2": l, "p2": [100, 80], "radius": 50}));
    assert_circle_unchanged(&s, &c);
    // At the command line a bad radius is refused and the command keeps going.
    let mut s = Session::new();
    let (c, _) = circle_and_line(&mut s);
    let before = model(&s);
    for bad in ["R NaN", "R -5", "R inf"] {
        s.cmdline("FILLET").unwrap();
        let _ = s.script(&format!("{bad}\n"));
        s.cancel();
        assert_eq!(model(&s), before, "{bad}");
    }
    assert_circle_unchanged(&s, &c);
    // A NaN radius stored in the drawing (a hostile file) is refused too.
    s.doc_mut().unwrap().header.set_f64("FILLETRAD", f64::NAN);
    let _ = s.script("FILLET 30,40 100,80\n");
    s.cancel();
    assert_eq!(model(&s), before);
}

/// A line with neither endpoint on a usable side cannot make a short, smooth fillet.
#[test]
fn fillet_refuses_a_short_line_that_cannot_meet_the_short_arc() {
    for (radius, points, cp, lp, fillet_radius) in [
        (50, [[100, -10], [100, 10]], [30, 40], [100, 0], 30),
        // The picked endpoint is already a tangent point. Its arrival direction still matters.
        (5, [[11, 8], [11, 0]], [3, 4], [11, 8], 5),
    ] {
        for circle_first in [true, false] {
            let mut s = Session::new();
            let c = h(&s.execute("circle", &json!({"center": [0, 0], "radius": radius})).unwrap());
            let l = h(&s.execute("line", &json!({"points": points})).unwrap());
            let before = model(&s);
            let params = if circle_first {
                json!({"h1": c, "p1": cp, "h2": l, "p2": lp, "radius": fillet_radius})
            } else {
                json!({"h1": l, "p1": lp, "h2": c, "p2": cp, "radius": fillet_radius})
            };
            // Both endpoints lie between the possible tangent points. Extending either end
            // would require a fillet over 180 degrees or a cusp at the line.
            assert!(s.execute("fillet", &params).is_err());
            assert_eq!(model(&s), before);
        }
    }
}

/// An unchanged arc still has an arrival direction at each endpoint.
#[test]
fn fillet_preserves_arrival_direction_at_an_unchanged_arc_endpoint() {
    let angle = 4.0_f64.atan2(3.0).to_degrees();
    for (start, end, pick, line_pick) in [
        (-angle, 0.0, [3.0, -4.0], [11.0, -15.0]),
        // Pick just beyond the end so the requested retained side has zero length.
        (0.0, angle, [3.0, 4.01], [11.0, 15.0]),
    ] {
        for arc_first in [true, false] {
            let mut s = Session::new();
            let a = h(&s.execute("arc", &json!({"center": [0, 0], "radius": 5, "start": start, "end": end})).unwrap());
            let l = h(&s.execute("line", &json!({"points": [[11, -20], [11, 20]]})).unwrap());
            let before = model(&s);
            let params = if arc_first {
                json!({"h1": a, "p1": pick, "h2": l, "p2": line_pick, "radius": 5})
            } else {
                json!({"h1": l, "p1": line_pick, "h2": a, "p2": pick, "radius": 5})
            };
            // The nearby candidate touches an arc endpoint but would leave a cusp there.
            // The other candidate cannot extend the arc smoothly, so neither is usable.
            assert!(s.execute("fillet", &params).is_err());
            assert_eq!(model(&s), before);
        }
    }
}

/// Fillets with arcs and circles picked between the two possible tangent points: never a loop.
#[test]
fn fillet_arcs_and_circles_run_on_smoothly() {
    let arc = |x: f64, s: f64, e: f64| json!({"center": [x, 0], "radius": 50, "start": s, "end": e});
    let cases: [(&str, Value, [f64; 2], &str, Value, [f64; 2]); 4] = [
        ("arc", arc(0.0, -90.0, 90.0), [50.0, 0.0], "line", json!({"points": [[100, -100], [100, 100]]}), [100.0, 0.0]),
        ("arc", arc(0.0, -90.0, 90.0), [50.0, 0.0], "arc", arc(150.0, 90.0, 270.0), [100.0, 0.0]),
        ("circle", json!({"center": [0, 0], "radius": 50}), [50.0, 0.0], "circle", json!({"center": [150, 0], "radius": 50}), [100.0, 10.0]),
        ("circle", json!({"center": [0, 0], "radius": 50}), [50.0, 0.0], "arc", arc(150.0, 90.0, 270.0), [100.0, 10.0]),
    ];
    for (k1, a1, p1, k2, a2, p2) in cases {
        let mut s = Session::new();
        let h1 = h(&s.execute(k1, &a1).unwrap());
        let h2 = h(&s.execute(k2, &a2).unwrap());
        let r = s.execute("fillet", &json!({"h1": h1, "p1": p1, "h2": h2, "p2": p2, "radius": 30})).unwrap();
        let nh = r["arc"].as_str().unwrap().to_string();
        let EntityKind::Arc(f) = kind(&s, &nh) else { panic!() };
        let (fc, start, end) = (f.center.xy(), f.start, f.end);
        let sweep = (end - start).rem_euclid(2.0 * PI);
        assert!(sweep > 1e-6 && sweep < PI, "{k1}+{k2}: the fillet arc loops: {sweep}");
        for hh in [&h1, &h2] {
            // A trimmed object ends at a fillet arc end and arrives there along the arc.
            let (t, arrive) = match kind(&s, hh) {
                EntityKind::Line(l) => {
                    let (a, b) = (l.a.xy(), l.b.xy());
                    let t = if (a.dist(fc) - 30.0).abs() < 1e-6 { a } else { b };
                    (t, (t - if t == a { b } else { a }).normalized())
                }
                EntityKind::Arc(a) => {
                    let c = a.center.xy();
                    let (sp, ep) = (c + Vec2::from_angle(a.start) * a.radius, c + Vec2::from_angle(a.end) * a.radius);
                    if (ep.dist(fc) - 30.0).abs() < 1e-6 { (ep, (ep - c).perp().normalized()) } else { (sp, -(sp - c).perp().normalized()) }
                }
                EntityKind::Circle(_) => continue,
                k => panic!("{k:?}"),
            };
            assert!((t.dist(fc) - 30.0).abs() < 1e-6, "{k1}+{k2}: {t:?} is not on the fillet arc");
            assert!(arrive.dot(leaving(fc, start, end, t)) > 0.999, "{k1}+{k2}: cusp at {t:?}");
        }
    }
}
