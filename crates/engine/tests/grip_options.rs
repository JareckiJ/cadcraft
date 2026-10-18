//! Hot-grip options, typed at the command line as AutoCAD's grip modes take them: Base point,
//! Copy, Undo, Reference and eXit; Enter cycles the modes; several grips can be hot at once.
//! A whole grip edit, copies included, is one undo step.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle};
use cadcraft_engine::geom::Vec2;
use serde_json::json;

fn line(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> Handle {
    let r = s.execute("line", &json!({ "points": [a, b] })).unwrap();
    Handle::parse_hex(r["handles"][0].as_str().unwrap()).unwrap()
}

fn lines(s: &Session) -> Vec<(Vec2, Vec2)> {
    let d = s.doc().unwrap();
    d.model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Line(l) => Some((l.a.xy(), l.b.xy())),
            _ => None,
        })
        .collect()
}

fn ends(s: &Session, h: Handle) -> (Vec2, Vec2) {
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::Line(l) => (l.a.xy(), l.b.xy()),
        k => panic!("not a line: {k:?}"),
    }
}

fn near(p: Vec2, x: f64, y: f64) -> bool {
    p.near(Vec2::new(x, y), 1e-9)
}

fn grip_index(s: &Session, h: Handle, x: f64, y: f64) -> usize {
    s.grips_of(h).unwrap().iter().position(|g| near(*g, x, y)).unwrap()
}

fn typed(s: &mut Session, inputs: &[&str]) {
    for t in inputs {
        s.cmdline(t).unwrap();
    }
}

#[test]
fn base_point_copy_and_undo_make_one_undo_step() {
    let mut s = Session::new();
    let a = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    s.set_selection(vec![a]);
    let undo = s.state().unwrap().undo.len();
    s.start_grip_edit(&[(a, grip_index(&s, a, 10.0, 0.0))]).unwrap();
    assert!(s.prompt_text().contains("** STRETCH ** Specify stretch point or [Base point/Copy/Undo/Lengthen/eXit]"), "{}", s.prompt_text());

    // Base point 0,0, then Copy: each point stretches a new copy by (point - base).
    typed(&mut s, &["B", "0,0", "C", "5,5", "0,5", "0,9"]);
    assert_eq!(lines(&s).len(), 4, "the original and three copies");
    // Undo takes back the last copy only.
    typed(&mut s, &["U"]);
    assert_eq!(lines(&s).len(), 3);
    assert!(s.running.is_some(), "Copy mode carries on");
    // Enter after copies ends the edit.
    typed(&mut s, &[""]);
    assert!(s.running.is_none());
    let ls = lines(&s);
    assert!(near(ends(&s, a).1, 10.0, 0.0), "the original stays");
    assert!(ls.iter().any(|(p, q)| near(*p, 0.0, 0.0) && near(*q, 15.0, 5.0)), "{ls:?}");
    assert!(ls.iter().any(|(p, q)| near(*p, 0.0, 0.0) && near(*q, 10.0, 5.0)), "{ls:?}");
    assert_eq!(s.state().unwrap().undo.len(), undo + 1, "one undo step for the whole edit");
    s.undo().unwrap();
    assert_eq!(lines(&s).len(), 1);
}

#[test]
fn enter_cycles_the_modes_and_typed_values_apply() {
    let mut s = Session::new();
    let a = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    s.set_selection(vec![a]);
    s.start_grip_edit(&[(a, 0)]).unwrap();
    typed(&mut s, &[""]);
    assert!(s.prompt_text().contains("** MOVE **"), "{}", s.prompt_text());
    typed(&mut s, &[""]);
    assert!(s.prompt_text().contains("** ROTATE ** Specify rotation angle or [Base point/Copy/Undo/Reference/eXit]"), "{}", s.prompt_text());
    // A typed angle rotates about the grip; without Copy the edit ends.
    typed(&mut s, &["90"]);
    assert!(s.running.is_none());
    let (p, q) = ends(&s, a);
    assert!(near(p, 0.0, 0.0) && near(q, 0.0, 10.0), "{p:?} {q:?}");

    // SCALE takes a typed factor; MIRROR a second point; after MIRROR comes STRETCH again.
    s.start_grip_edit(&[(a, 0)]).unwrap();
    typed(&mut s, &["", "", "", "2"]);
    assert!(near(ends(&s, a).1, 0.0, 20.0), "{:?}", ends(&s, a));
    s.start_grip_edit(&[(a, 0)]).unwrap();
    typed(&mut s, &["", "", "", ""]);
    assert!(s.prompt_text().contains("** MIRROR ** Specify second point"), "{}", s.prompt_text());
    typed(&mut s, &[""]);
    assert!(s.prompt_text().contains("** STRETCH **"), "{}", s.prompt_text());
    // eXit leaves the objects as they are.
    typed(&mut s, &["X"]);
    assert!(s.running.is_none());
    assert!(near(ends(&s, a).1, 0.0, 20.0));
}

#[test]
fn reference_rotate_and_scale() {
    let mut s = Session::new();
    let a = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    s.set_selection(vec![a]);
    // ROTATE Reference: from 45° to 90° turns by 45°.
    s.start_grip_edit(&[(a, 0)]).unwrap();
    typed(&mut s, &["", "", "R", "45", "90"]);
    let q = ends(&s, a).1;
    let r = std::f64::consts::FRAC_1_SQRT_2 * 10.0;
    assert!(near(q, r, r), "{q:?}");

    // SCALE Reference with two points: reference length 5 (two points), new length 10.
    let b = line(&mut s, [0.0, 0.0], [4.0, 0.0]);
    s.set_selection(vec![b]);
    s.start_grip_edit(&[(b, 0)]).unwrap();
    typed(&mut s, &["", "", "", "R", "0,0", "5,0", "10"]);
    assert!(near(ends(&s, b).1, 8.0, 0.0), "{:?}", ends(&s, b));
    assert!(s.running.is_none());
}

#[test]
fn move_with_base_point_and_copy_on_the_selection() {
    let mut s = Session::new();
    let a = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    let b = line(&mut s, [0.0, 5.0], [10.0, 5.0]);
    s.set_selection(vec![a, b]);
    s.start_grip_edit(&[(a, 0)]).unwrap();
    // MOVE, Copy: both selected lines are copied by each displacement.
    typed(&mut s, &["", "C", "0,20", "0,40", "X"]);
    assert!(s.running.is_none());
    assert_eq!(lines(&s).len(), 6);
    assert!(near(ends(&s, a).0, 0.0, 0.0), "the originals stay");
}

#[test]
fn several_hot_grips_stretch_together() {
    let mut s = Session::new();
    s.execute("pline", &json!({ "vertices": [[0, 0], [10, 0], [10, 10], [0, 10]], "closed": true })).unwrap();
    let h = s.doc().unwrap().model.iter().last().unwrap().handle;
    s.set_selection(vec![h]);
    let (i, j) = (grip_index(&s, h, 10.0, 0.0), grip_index(&s, h, 10.0, 10.0));
    s.start_grip_edit(&[(h, i), (h, j)]).unwrap();
    typed(&mut s, &["15,0"]);
    assert!(s.running.is_none());
    let g = s.grips_of(h).unwrap();
    assert!(g.iter().any(|p| near(*p, 15.0, 0.0)) && g.iter().any(|p| near(*p, 15.0, 10.0)), "{g:?}");

    // Typed: GRIP, pick the hot grips, Enter, then the stretch point.
    typed(&mut s, &["GRIP", "0,0", "0,10", "", "-5,0"]);
    assert!(s.running.is_none());
    let g = s.grips_of(h).unwrap();
    assert!(g.iter().any(|p| near(*p, -5.0, 0.0)) && g.iter().any(|p| near(*p, -5.0, 10.0)), "{g:?}");

    // JSON: grip.stretch with several grips.
    let (i, j) = (grip_index(&s, h, 15.0, 0.0), grip_index(&s, h, 15.0, 10.0));
    s.execute("grip.stretch", &json!({ "grips": [[h.hex(), i], [h.hex(), j]], "delta": [5, 0] })).unwrap();
    let g = s.grips_of(h).unwrap();
    assert!(g.iter().any(|p| near(*p, 20.0, 0.0)) && g.iter().any(|p| near(*p, 20.0, 10.0)), "{g:?}");
}

#[test]
fn bad_grips_are_errors_not_panics() {
    let mut s = Session::new();
    let a = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    s.set_selection(vec![a]);
    assert!(s.start_grip_edit(&[]).is_err());
    assert!(s.start_grip_edit(&[(a, 99)]).is_err());
    assert!(s.start_grip_edit(&[(Handle(0xFFFF), 0)]).is_err());
    for p in [
        json!({ "grips": "x" }),
        json!({ "grips": [] , "delta": [1, 0] }),
        json!({ "grips": [[a.hex()]], "delta": [1, 0] }),
        json!({ "grips": [[a.hex(), -1]], "delta": [1, 0] }),
        json!({ "grips": [[a.hex(), 0]], "delta": [f64::MAX, 0] }),
        json!({ "grips": [[a.hex(), 0]] }),
    ] {
        let _ = s.execute("grip.stretch", &p);
    }
    assert!(s.execute("grip", &json!({})).is_err());
    // Typed GRIP away from any grip re-prompts; nonsense at the mode prompt too.
    typed(&mut s, &["GRIP", "50,50"]);
    assert!(s.running.is_some());
    typed(&mut s, &["10,0", "", "foo", "1e400,0"]);
    s.cancel();
    assert!(s.running.is_none());
    let (p, q) = ends(&s, a);
    assert!(p.is_finite() && q.is_finite());
}

// ---------------- multifunctional grips ----------------

fn pline(s: &mut Session, pts: &[[f64; 2]], closed: bool) -> Handle {
    s.execute("pline", &json!({ "vertices": pts, "closed": closed })).unwrap();
    s.doc().unwrap().model.iter().last().unwrap().handle
}

fn vertices(s: &Session, h: Handle) -> Vec<(Vec2, f64)> {
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::LwPolyline(p) => p.vertices.iter().map(|v| (v.p, v.bulge)).collect(),
        k => panic!("not a polyline: {k:?}"),
    }
}

#[test]
fn polyline_vertex_options() {
    let mut s = Session::new();
    let h = pline(&mut s, &[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]], false);
    s.set_selection(vec![h]);
    let undo = s.state().unwrap().undo.len();
    s.start_grip_edit(&[(h, grip_index(&s, h, 10.0, 0.0))]).unwrap();
    assert!(s.prompt_text().contains("[Base point/Copy/Undo/Add vertex/Remove vertex/eXit]"), "{}", s.prompt_text());
    // Add vertex after the hot vertex.
    typed(&mut s, &["A"]);
    assert!(s.prompt_text().contains("** ADD VERTEX ** Specify new vertex point"), "{}", s.prompt_text());
    typed(&mut s, &["15,5"]);
    assert!(s.running.is_none());
    let v: Vec<Vec2> = vertices(&s, h).into_iter().map(|v| v.0).collect();
    assert_eq!(v.len(), 4);
    assert!(near(v[2], 15.0, 5.0), "{v:?}");
    assert_eq!(s.state().unwrap().undo.len(), undo + 1);

    // Remove vertex acts at once.
    s.start_grip_edit(&[(h, grip_index(&s, h, 15.0, 5.0))]).unwrap();
    typed(&mut s, &["R"]);
    assert!(s.running.is_none());
    assert_eq!(vertices(&s, h).len(), 3);

    // Add vertex from a segment midpoint: the new vertex goes on that segment.
    s.start_grip_edit(&[(h, grip_index(&s, h, 5.0, 0.0))]).unwrap();
    typed(&mut s, &["A", "5,-3"]);
    let v: Vec<Vec2> = vertices(&s, h).into_iter().map(|v| v.0).collect();
    assert!(v.len() == 4 && near(v[1], 5.0, -3.0), "{v:?}");

    // Enter at an option prompt goes back to STRETCH.
    s.start_grip_edit(&[(h, 0)]).unwrap();
    typed(&mut s, &["A", ""]);
    assert!(s.prompt_text().contains("** STRETCH **"), "{}", s.prompt_text());
    typed(&mut s, &["X"]);
    assert_eq!(vertices(&s, h).len(), 4);
}

#[test]
fn polyline_segment_converts_to_arc_and_back() {
    let mut s = Session::new();
    let h = pline(&mut s, &[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]], false);
    s.set_selection(vec![h]);
    let mid = grip_index(&s, h, 5.0, 0.0);
    s.start_grip_edit(&[(h, mid)]).unwrap();
    assert!(s.prompt_text().contains("Add vertex/conVert to arc/eXit"), "{}", s.prompt_text());
    typed(&mut s, &["V", "5,3"]);
    let b = vertices(&s, h)[0].1;
    assert!(b.abs() > 0.1, "an arc segment: bulge {b}");
    // The arc's midpoint grip now offers Convert to Line.
    let g = s.grips_of(h).unwrap();
    assert!(near(g[mid], 5.0, 3.0), "{g:?}");
    s.start_grip_edit(&[(h, mid)]).unwrap();
    assert!(s.prompt_text().contains("conVert to line"), "{}", s.prompt_text());
    typed(&mut s, &["V"]);
    assert!(s.running.is_none());
    assert_eq!(vertices(&s, h)[0].1, 0.0);
}

#[test]
fn line_and_arc_lengthen_and_radius() {
    let mut s = Session::new();
    let a = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    s.set_selection(vec![a]);
    s.start_grip_edit(&[(a, 2)]).unwrap();
    assert!(s.prompt_text().contains("Undo/Lengthen/eXit"), "{}", s.prompt_text());
    // A picked point is projected onto the line.
    typed(&mut s, &["L", "15,3"]);
    assert!(near(ends(&s, a).1, 15.0, 0.0), "{:?}", ends(&s, a));
    // A typed value is the change in length; from the start grip.
    s.start_grip_edit(&[(a, 0)]).unwrap();
    typed(&mut s, &["L", "-5"]);
    assert!(near(ends(&s, a).0, 5.0, 0.0), "{:?}", ends(&s, a));
    // Shortening past the other end is refused (re-prompts).
    s.start_grip_edit(&[(a, 2)]).unwrap();
    typed(&mut s, &["L", "-50"]);
    assert!(s.running.is_some());
    s.cancel();
    assert!(near(ends(&s, a).1, 15.0, 0.0));

    // Arc: Radius from the midpoint grip, Lengthen from an end grip.
    s.execute("arc", &json!({ "center": [0, 0], "radius": 5, "start": 0, "end": 90 })).unwrap();
    let arc = s.doc().unwrap().model.iter().last().unwrap().handle;
    s.set_selection(vec![arc]);
    s.start_grip_edit(&[(arc, 1)]).unwrap();
    assert!(s.prompt_text().contains("Undo/Radius/eXit"), "{}", s.prompt_text());
    typed(&mut s, &["R", "8"]);
    s.start_grip_edit(&[(arc, 2)]).unwrap();
    typed(&mut s, &["L", "-1,0.0001"]);
    match &s.doc().unwrap().entity(arc).unwrap().kind {
        EntityKind::Arc(k) => {
            assert!((k.radius - 8.0).abs() < 1e-9, "{k:?}");
            assert!((k.end.to_degrees() - 180.0).abs() < 0.01, "{k:?}");
        }
        k => panic!("{k:?}"),
    }
}

#[test]
fn grip_option_json_form() {
    let mut s = Session::new();
    let h = pline(&mut s, &[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]], false);
    s.execute("grip.option", &json!({ "handle": h.hex(), "index": 0, "option": "addVertex", "to": [-5, 0] })).unwrap();
    assert_eq!(vertices(&s, h).len(), 4);
    s.execute("grip.option", &json!({ "handle": h.hex(), "index": 0, "option": "removeVertex" })).unwrap();
    assert_eq!(vertices(&s, h).len(), 3);
    // Options that don't apply, unknown options and hostile values are errors, not panics.
    for p in [
        json!({ "handle": h.hex(), "index": 0, "option": "radius", "value": 3 }),
        json!({ "handle": h.hex(), "index": 0, "option": "explode" }),
        json!({ "handle": h.hex(), "index": 99, "option": "addVertex", "to": [1, 1] }),
        json!({ "handle": h.hex(), "index": 1, "option": "toArc", "to": [1e308, 1e308] }),
        json!({ "handle": h.hex(), "index": 1, "option": "toArc" }),
        json!({ "index": 1, "option": "toLine" }),
    ] {
        assert!(s.execute("grip.option", &p).is_err(), "{p}");
    }
}

/// Centre and radius of the polyline segment that starts at vertex `i`.
fn segment_circle(s: &Session, h: Handle, i: usize) -> (Vec2, f64) {
    let v = vertices(s, h);
    let (a, bulge) = v[i];
    let b = v[(i + 1) % v.len()].0;
    let (arc, _) = cadcraft_engine::geom::bulge_to_arc(a, b, bulge).unwrap_or_else(|| panic!("segment {i} is straight: {v:?}"));
    (arc.center, arc.radius)
}

/// An open polyline 0,0 → 10,0 whose segment is turned into a half circle through `through`.
fn half_circle(s: &mut Session, through: [f64; 2]) -> Handle {
    let h = pline(s, &[[0.0, 0.0], [10.0, 0.0]], false);
    let mid = grip_index(s, h, 5.0, 0.0);
    s.execute("grip.option", &json!({ "handle": h.hex(), "index": mid, "option": "toArc", "to": through })).unwrap();
    h
}

#[test]
fn add_vertex_on_an_arc_segment_keeps_two_arcs_on_its_circle() {
    // Clockwise above the chord, counterclockwise below it; the vertex goes 60° from the end,
    // away from the arc's midpoint so that splitting the bulge in halves would not do.
    let y = 5.0 * 60f64.to_radians().sin();
    for (through, at) in [([5.0, 5.0], [7.5, y]), ([5.0, -5.0], [7.5, -y])] {
        let mut s = Session::new();
        let h = half_circle(&mut s, through);
        let sign = vertices(&s, h)[0].1.signum();
        s.set_selection(vec![h]);
        s.start_grip_edit(&[(h, grip_index(&s, h, through[0], through[1]))]).unwrap();
        typed(&mut s, &["A", &format!("{},{}", at[0], at[1])]);
        let v = vertices(&s, h);
        assert_eq!(v.len(), 3, "{v:?}");
        assert!(near(v[1].0, at[0], at[1]), "{v:?}");
        for i in 0..2 {
            assert_eq!(v[i].1.signum(), sign, "segment {i} turns the same way: {v:?}");
            let (c, r) = segment_circle(&s, h, i);
            assert!(c.near(Vec2::new(5.0, 0.0), 1e-9) && (r - 5.0).abs() < 1e-9, "segment {i}: centre {c:?} radius {r}");
        }
    }
}

#[test]
fn add_vertex_off_the_arc_keeps_the_picked_point() {
    // The vertex goes where it is picked, not onto the old arc; both pieces then lie on the one
    // circle through the segment's ends and the new vertex.
    let mut s = Session::new();
    let h = half_circle(&mut s, [5.0, 5.0]);
    let mid = grip_index(&s, h, 5.0, 5.0);
    s.execute("grip.option", &json!({ "handle": h.hex(), "index": mid, "option": "addVertex", "to": [8.0, 8.0] })).unwrap();
    let v = vertices(&s, h);
    assert!(v.len() == 3 && near(v[1].0, 8.0, 8.0), "{v:?}");
    let circle = cadcraft_engine::geom::Arc::from_3_points(v[0].0, v[1].0, v[2].0).unwrap();
    for i in 0..2 {
        let (c, r) = segment_circle(&s, h, i);
        assert!(c.near(circle.center, 1e-9) && (r - circle.radius).abs() < 1e-9, "segment {i}: centre {c:?} radius {r}, want {circle:?}");
    }
}

#[test]
fn add_vertex_on_an_arc_is_one_undo_step() {
    let mut s = Session::new();
    let h = half_circle(&mut s, [5.0, 5.0]);
    let before = vertices(&s, h);
    s.set_selection(vec![h]);
    let undo = s.state().unwrap().undo.len();
    s.start_grip_edit(&[(h, grip_index(&s, h, 5.0, 5.0))]).unwrap();
    typed(&mut s, &["A", "0.5,3"]);
    assert!(s.running.is_none());
    assert_eq!(vertices(&s, h).len(), 3);
    assert_eq!(s.state().unwrap().undo.len(), undo + 1);
    s.undo().unwrap();
    assert_eq!(vertices(&s, h), before, "one Undo brings the whole arc back");
}
