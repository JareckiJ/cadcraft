//! BOX, CYLINDER and EXTRUDE: solids as groups of 3D faces (gaps.md F24, first step).

use cadcraft_engine::doc::{EntityKind, Handle, LwPolyline};
use cadcraft_engine::geom::{PolyVertex, Vec2, Vec3};
use cadcraft_engine::{Input, Session};
use serde_json::{Value, json};

fn faces(s: &Session) -> Vec<[[f64; 3]; 4]> {
    s.doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Face3d(f) => Some(f.corners.map(|c| [c.x, c.y, c.z])),
            _ => None,
        })
        .collect()
}

fn count(s: &Session) -> usize {
    s.doc().unwrap().model.len()
}

/// Volume by the divergence theorem; positive when the faces wind outwards.
fn volume(fs: &[[[f64; 3]; 4]]) -> f64 {
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let cross = |a: [f64; 3], b: [f64; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    fs.iter()
        .map(|[a, b, c, d]| {
            let mut v = dot(*a, cross(*b, *c)) / 6.0;
            if c != d {
                v += dot(*a, cross(*c, *d)) / 6.0;
            }
            v
        })
        .sum()
}

fn pline(pts: &[(f64, f64, f64)], closed: bool, elevation: f64) -> EntityKind {
    EntityKind::LwPolyline(LwPolyline {
        vertices: pts.iter().map(|&(x, y, b)| PolyVertex::with_bulge(Vec2::new(x, y), b)).collect(),
        closed,
        const_width: 0.0,
        elevation,
        plinegen: false,
    })
}

#[test]
fn box_is_one_group_of_six_outward_faces_and_one_undo_step() {
    let mut s = Session::new();
    let r = s.execute("box", &json!({ "corner": [1, 2, 3], "corner2": [5, 5], "height": 2 })).unwrap();
    assert_eq!(r["faces"], 6);
    assert_eq!(count(&s), 6);
    assert!((volume(&faces(&s)) - 24.0).abs() < 1e-9);
    let d = s.doc().unwrap();
    assert_eq!(d.groups.len(), 1);
    assert_eq!(d.groups[0].members.len(), 6);
    assert!(d.groups[0].selectable);
    s.undo().unwrap();
    assert_eq!(count(&s), 0, "one undo removes the whole box");
    assert!(s.doc().unwrap().groups.is_empty());
}

#[test]
fn box_by_length_and_width_and_negative_height() {
    let mut s = Session::new();
    s.execute("box", &json!({ "corner": [0, 0], "length": 2, "width": 3, "height": -4 })).unwrap();
    assert!((volume(&faces(&s)) - 24.0).abs() < 1e-9);
    assert!(faces(&s).iter().flatten().all(|c| (-4.0..=0.0).contains(&c[2])));
}

#[test]
fn cylinder_volume_and_segments() {
    let mut s = Session::new();
    let r = s.execute("cylinder", &json!({ "center": [0, 0, 1], "radius": 5, "height": 10 })).unwrap();
    let n = r["faces"].as_u64().unwrap();
    assert!(n >= 8 * 3, "{n}");
    let want = std::f64::consts::PI * 25.0 * 10.0;
    assert!((volume(&faces(&s)) - want).abs() / want < 5e-3);
    let mut s2 = Session::new();
    let r2 = s2.execute("cylinder", &json!({ "center": [0, 0], "radius": 1, "height": 1, "segments": 12 })).unwrap();
    assert_eq!(r2["faces"], 12 * 3);
}

#[test]
fn extrude_polyline_with_an_arc_and_a_circle_keep_the_profile_unless_erased() {
    let mut s = Session::new();
    // A D shape: a square with one bulged side.
    let d = s.add_entity(pline(&[(0.0, 0.0, 0.0), (4.0, 0.0, 0.0), (4.0, 4.0, 1.0), (0.0, 4.0, 0.0)], true, 2.0)).unwrap();
    let c = s.execute("circle", &json!({ "center": [20, 0], "radius": 2 })).unwrap();
    let c: Handle = Handle::parse_hex(c["handle"].as_str().unwrap()).unwrap();
    let r = s.execute("extrude", &json!({ "handles": [d.hex(), c.hex()], "height": 3 })).unwrap();
    assert_eq!(r["groups"].as_array().unwrap().len(), 2);
    let model = &s.doc().unwrap().model;
    assert!(model.iter().any(|e| e.handle == d), "the profile stays");
    // All faces of the D sit between its elevation and +3.
    let fs = faces(&s);
    assert!(fs.iter().flatten().all(|p| p[2] >= -1e-9));
    // Area of the D: 16 + half circle of radius 2 (bulge 1) = 16 + 2π.
    let area_d = 16.0 + 2.0 * std::f64::consts::PI;
    let area_c = std::f64::consts::PI * 4.0;
    let want = (area_d + area_c) * 3.0;
    assert!((volume(&fs) - want).abs() / want < 5e-3, "{} vs {want}", volume(&fs));

    s.execute("extrude", &json!({ "handles": [d.hex()], "height": 1, "erase": true })).unwrap();
    assert!(!s.doc().unwrap().model.iter().any(|e| e.handle == d), "erase removes the profile");
}

#[test]
fn extrude_uses_the_selection_and_the_profile_layer() {
    let mut s = Session::new();
    s.execute("layer.new", &json!({ "name": "WALLS" })).unwrap();
    s.execute("layer.current", &json!({ "name": "WALLS" })).unwrap();
    let h = s.add_entity(pline(&[(0.0, 0.0, 0.0), (3.0, 0.0, 0.0), (3.0, 2.0, 0.0), (0.0, 2.0, 0.0)], true, 0.0)).unwrap();
    s.execute("layer.current", &json!({ "name": "0" })).unwrap();
    s.set_selection(vec![h]);
    s.execute("extrude", &json!({ "height": 2.5 })).unwrap();
    let layers: Vec<String> =
        s.doc().unwrap().model.iter().filter(|e| matches!(e.kind, EntityKind::Face3d(_))).map(|e| e.common.layer.clone()).collect();
    assert_eq!(layers.len(), 6);
    assert!(layers.iter().all(|l| l == "WALLS"));
}

#[test]
fn bad_profiles_leave_the_drawing_untouched() {
    let mut s = Session::new();
    let open = s.add_entity(pline(&[(0.0, 0.0, 0.0), (3.0, 0.0, 0.0), (3.0, 2.0, 0.0)], false, 0.0)).unwrap();
    let bow = s.add_entity(pline(&[(0.0, 0.0, 0.0), (2.0, 2.0, 0.0), (2.0, 0.0, 0.0), (0.0, 2.0, 0.0)], true, 0.0)).unwrap();
    let good = s.add_entity(pline(&[(0.0, 0.0, 0.0), (3.0, 0.0, 0.0), (3.0, 2.0, 0.0)], true, 0.0)).unwrap();
    let line = s.add_entity(EntityKind::Line(cadcraft_engine::doc::Line { a: Vec3::ZERO, b: Vec3::new(1.0, 1.0, 0.0) })).unwrap().hex();
    let before = count(&s);
    for (hs, h) in [
        (vec![open.hex()], 1.0),
        (vec![bow.hex()], 1.0),
        (vec![good.hex(), open.hex()], 1.0),
        (vec![line.clone()], 1.0),
        (vec![good.hex()], 0.0),
        (vec![], 1.0),
        (vec!["ZZZZ".into()], 1.0),
    ] {
        let r = s.execute("extrude", &json!({ "handles": hs, "height": h }));
        assert!(r.is_err(), "{hs:?} height {h} must be refused");
        assert_eq!(count(&s), before, "{hs:?}: nothing was added");
    }
    assert!(s.doc().unwrap().groups.is_empty());
}

#[test]
fn hostile_parameters_never_panic() {
    let junk: Vec<Value> = vec![
        json!({}),
        json!({ "corner": "x", "height": 1 }),
        json!({ "corner": [0, 0], "corner2": [0, 5], "height": 1 }),
        json!({ "corner": [0, 0], "corner2": [5, 5], "height": 0 }),
        json!({ "corner": [1e308, 1e308, 1e308], "corner2": [-1e308, -1e308], "height": 1e308 }),
        json!({ "corner": [0, 0], "length": null, "width": 2, "height": 1 }),
        json!({ "center": [0, 0], "radius": 0, "height": 1 }),
        json!({ "center": [0, 0], "radius": -3, "height": 1 }),
        json!({ "center": [0, 0], "radius": 1e300, "height": 1e300, "tolerance": 1e-300 }),
        json!({ "center": [0, 0], "radius": 1, "height": 1, "segments": 2 }),
        json!({ "center": [0, 0], "radius": 1, "height": 1, "segments": 100000 }),
        json!({ "center": [0, 0], "radius": 1, "height": 1, "segments": "many" }),
        json!({ "handles": "all", "height": 1 }),
        json!({ "handles": [1, null, "a"], "height": 1 }),
        json!({ "handles": [], "height": "tall" }),
        json!({ "height": 1e308, "tolerance": 0 }),
    ];
    for cmd in ["box", "cylinder", "extrude"] {
        for p in &junk {
            let mut s = Session::new();
            let _ = s.execute(cmd, p);
        }
    }
    // A polyline with absurd coordinates and tolerances.
    let mut s = Session::new();
    let h = s.add_entity(pline(&[(0.0, 0.0, 1e300), (1e300, 0.0, -1e300), (1e300, 1e300, 0.0)], true, f64::MAX)).unwrap();
    for tol in [json!(null), json!(1e-300), json!(1e300)] {
        let _ = s.execute("extrude", &json!({ "handles": [h.hex()], "height": 1.0, "tolerance": tol }));
    }
}

#[test]
fn the_solid_survives_a_dxf_round_trip() {
    let mut s = Session::new();
    s.execute("box", &json!({ "corner": [0, 0, 0], "corner2": [2, 3], "height": 4 })).unwrap();
    let l = s
        .add_entity(pline(&[(10.0, 0.0, 0.0), (14.0, 0.0, 0.0), (14.0, 1.0, 0.0), (11.0, 1.0, 0.0), (11.0, 3.0, 0.0), (10.0, 3.0, 0.0)], true, 0.0))
        .unwrap();
    s.execute("extrude", &json!({ "handles": [l.hex()], "height": 2 })).unwrap();
    let before = faces(&s);
    let text = cadcraft_io::write_dxf(s.doc().unwrap());
    let back = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
    let after: Vec<[[f64; 3]; 4]> = back
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Face3d(f) => Some(f.corners.map(|c| [c.x, c.y, c.z])),
            _ => None,
        })
        .collect();
    assert_eq!(before.len(), after.len());
    assert!((volume(&before) - volume(&after)).abs() < 1e-9);
    assert_eq!(back.groups.len(), 2, "the groups round trip as well");
}

#[test]
fn typed_box_cylinder_and_extrude_follow_the_prompts() {
    let mut s = Session::new();
    s.start("box").unwrap();
    assert!(s.prompt_text().contains("first corner"), "{}", s.prompt_text());
    s.input(Input::Point(Vec2::new(0.0, 0.0))).unwrap();
    s.input(Input::Point(Vec2::new(2.0, 3.0))).unwrap();
    assert!(s.prompt_text().contains("height"), "{}", s.prompt_text());
    s.input(Input::Text("2".into())).unwrap();
    assert!(s.running.is_none());
    assert!((volume(&faces(&s)) - 12.0).abs() < 1e-9);

    s.start("cylinder").unwrap();
    s.input(Input::Point(Vec2::new(10.0, 10.0))).unwrap();
    s.input(Input::Text("1".into())).unwrap();
    let _ = s.input(Input::Text("0".into()));
    assert!(s.running.is_some() && s.prompt_text().contains("height"), "a zero height is refused and the prompt stays: {}", s.prompt_text());
    s.input(Input::Text("3".into())).unwrap();
    assert!(s.running.is_none());

    let h = s.add_entity(pline(&[(20.0, 0.0, 0.0), (23.0, 0.0, 0.0), (23.0, 2.0, 0.0), (20.0, 2.0, 0.0)], true, 0.0)).unwrap();
    let before = count(&s);
    s.start("extrude").unwrap();
    assert!(s.prompt_text().contains("Select objects"), "{}", s.prompt_text());
    s.input(Input::Pick(vec![h])).unwrap();
    s.input(Input::Enter).unwrap();
    assert!(s.prompt_text().contains("height of extrusion"), "{}", s.prompt_text());
    s.input(Input::Text("1.5".into())).unwrap();
    assert!(s.running.is_none());
    assert_eq!(count(&s), before + 6);
}
