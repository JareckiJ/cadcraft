//! ARRAYPATH makes an associative array by default (ARRAYASSOCIATIVITY): one block reference whose
//! items sit along the path, turning with it; ARRAYEDIT changes the count and spacing, EXPLODE
//! gives plain objects, and ARRAYASSOCIATIVITY 0 keeps separate copies.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{ArrayKind, ArrayParams, Drawing, Entity, EntityKind};
use cadcraft_engine::geom::{Mat3, Vec2};
use serde_json::{Value, json};

fn handle(v: &Value) -> Value {
    if v["handle"].is_null() { v["handles"][0].clone() } else { v["handle"].clone() }
}

fn model(s: &Session) -> Vec<Entity> {
    s.doc().unwrap().model.iter().map(|e| (**e).clone()).collect()
}

/// The array in model space (the path curve is the other object) and its parameters.
fn the_array(s: &Session) -> (Entity, ArrayParams) {
    let arrays: Vec<(Entity, ArrayParams)> = model(s).into_iter().filter_map(|e| s.doc().unwrap().array_params(&e).map(|p| (e, p))).collect();
    assert_eq!(arrays.len(), 1, "{:?}", model(s));
    arrays.into_iter().next().unwrap()
}

/// World positions of the circles and the end points of the lines `e` draws, through nested blocks.
fn points(d: &Drawing, e: &Entity) -> Vec<Vec2> {
    fn walk(d: &Drawing, e: &Entity, m: Mat3, depth: usize, out: &mut Vec<Vec2>) {
        match &e.kind {
            EntityKind::Circle(c) => out.push(m.apply(c.center.xy())),
            EntityKind::Line(l) => {
                out.push(m.apply(l.a.xy()));
                out.push(m.apply(l.b.xy()));
            }
            EntityKind::Insert(i) if depth < 8 => {
                let b = d.block(&i.block).unwrap();
                let im = m.then_before(i.transform(b.base.xy()));
                for be in b.entities.iter() {
                    walk(d, be, im, depth + 1, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(d, e, Mat3::IDENTITY, 0, &mut out);
    out
}

fn has(pts: &[Vec2], x: f64, y: f64) -> bool {
    pts.iter().any(|p| p.near(Vec2::new(x, y), 1e-6))
}

fn dot(s: &mut Session, x: f64, y: f64) -> Value {
    handle(&s.execute("circle", &json!({"center": [x, y], "radius": 0.25})).unwrap())
}

fn line(s: &mut Session, pts: Value) -> Value {
    handle(&s.execute("line", &json!({"points": pts})).unwrap())
}

#[test]
fn path_array_is_one_associative_array_along_the_path() {
    let mut s = Session::new();
    let o = dot(&mut s, 0.0, 1.0);
    let path = line(&mut s, json!([[0, 0], [10, 0]]));
    let r = s.execute("arraypath", &json!({"handles": [o], "path": path, "count": 6})).unwrap();
    assert_eq!(r["associative"], true);
    assert_eq!(r["created"], 5);
    let (e, p) = the_array(&s);
    assert_eq!(handle(&r), json!(e.handle.hex()));
    assert!(matches!(&p.kind, ArrayKind::Path(k) if k.count == 6 && k.divide && k.align && !k.closed));
    let pts = points(s.doc().unwrap(), &e);
    assert_eq!(pts.len(), 6);
    for x in [0.0, 2.0, 4.0, 6.0, 8.0, 10.0] {
        assert!(has(&pts, x, 1.0), "{x}: {pts:?}");
    }
    // The path curve is not part of the array.
    assert_eq!(model(&s).len(), 2);
}

#[test]
fn items_turn_with_the_path() {
    let mut s = Session::new();
    // A tick pointing along +X at the start of an L-shaped path: the last item points up.
    let tick = line(&mut s, json!([[0, 0], [1, 0]]));
    let path = handle(&s.execute("pline", &json!({"points": [[0, 0], [10, 0], [10, 10]]})).unwrap());
    s.execute("arraypath", &json!({"handles": [tick], "path": path, "count": 5})).unwrap();
    let (e, _) = the_array(&s);
    let pts = points(s.doc().unwrap(), &e);
    // Items 0 and 4: from (0,0) to (1,0), and from (10,10) up to (10,11).
    assert!(has(&pts, 1.0, 0.0) && has(&pts, 10.0, 11.0), "{pts:?}");
    s.execute("arrayedit", &json!({"handle": e.handle.hex(), "align": false})).unwrap();
    let (e, _) = the_array(&s);
    let pts = points(s.doc().unwrap(), &e);
    assert!(has(&pts, 11.0, 10.0) && !has(&pts, 10.0, 11.0), "{pts:?}");
}

#[test]
fn a_circle_path_divides_into_equal_arcs() {
    let mut s = Session::new();
    let o = dot(&mut s, 5.0, 0.0);
    let path = handle(&s.execute("circle", &json!({"center": [0, 0], "radius": 5})).unwrap());
    s.execute("arraypath", &json!({"handles": [o], "path": path, "count": 4})).unwrap();
    let (e, p) = the_array(&s);
    assert!(matches!(&p.kind, ArrayKind::Path(k) if k.closed));
    let pts = points(s.doc().unwrap(), &e);
    assert_eq!(pts.len(), 4);
    for (x, y) in [(5.0, 0.0), (0.0, 5.0), (-5.0, 0.0), (0.0, -5.0)] {
        assert!(has(&pts, x, y), "({x}, {y}): {pts:?}");
    }
}

#[test]
fn spacing_measures_along_the_path_and_arrayedit_changes_it() {
    let mut s = Session::new();
    let o = dot(&mut s, 0.0, 1.0);
    let path = line(&mut s, json!([[0, 0], [10, 0]]));
    let r = s.execute("arraypath", &json!({"handles": [o], "path": path, "spacing": 4})).unwrap();
    assert_eq!(r["created"], 2, "items at 0, 4 and 8");
    let (e, p) = the_array(&s);
    assert!(matches!(&p.kind, ArrayKind::Path(k) if k.count == 3 && !k.divide && k.spacing == 4.0));
    s.execute("arrayedit", &json!({"handle": e.handle.hex(), "items": 2, "spacing": 5})).unwrap();
    let (e, _) = the_array(&s);
    let pts = points(s.doc().unwrap(), &e);
    assert_eq!(pts.len(), 2);
    assert!(has(&pts, 0.0, 1.0) && has(&pts, 5.0, 1.0), "{pts:?}");
    // Back to dividing the path: 3 items go to 0, 5 and 10.
    s.execute("arrayedit", &json!({"handle": e.handle.hex(), "items": 3, "divide": true})).unwrap();
    let (e, _) = the_array(&s);
    assert!(has(&points(s.doc().unwrap(), &e), 10.0, 1.0));
    // The array is still one array after every edit.
    assert_eq!(model(&s).len(), 2);
}

#[test]
fn explode_gives_plain_objects_at_the_items() {
    let mut s = Session::new();
    let o = dot(&mut s, 0.0, 1.0);
    let path = line(&mut s, json!([[0, 0], [10, 0]]));
    s.execute("arraypath", &json!({"handles": [o], "path": path, "count": 3})).unwrap();
    let (e, _) = the_array(&s);
    s.execute("explode", &json!({"handles": [e.handle.hex()]})).unwrap();
    let centers: Vec<Vec2> =
        model(&s).into_iter().filter_map(|e| if let EntityKind::Circle(c) = e.kind { Some(c.center.xy()) } else { None }).collect();
    assert_eq!(centers.len(), 3, "{centers:?}");
    for x in [0.0, 5.0, 10.0] {
        assert!(has(&centers, x, 1.0), "{centers:?}");
    }
}

#[test]
fn associativity_zero_and_the_flag_keep_separate_copies() {
    let mut s = Session::new();
    let o = dot(&mut s, 0.0, 1.0);
    let path = line(&mut s, json!([[0, 0], [10, 0]]));
    let r = s.execute("arraypath", &json!({"handles": [o], "path": path, "count": 3, "associative": false})).unwrap();
    assert_eq!(r["handles"].as_array().unwrap().len(), 2);
    assert_eq!(model(&s).len(), 4);
    s.execute("setvar", &json!({"name": "ARRAYASSOCIATIVITY", "value": 0})).unwrap();
    let r = s.execute("arraypath", &json!({"handles": [o], "path": path, "count": 3})).unwrap();
    assert_eq!(r["handles"].as_array().unwrap().len(), 2);
}

#[test]
fn the_command_line_makes_an_associative_path_array() {
    let mut s = Session::new();
    dot(&mut s, 0.0, 1.0);
    line(&mut s, json!([[0, 0], [10, 0]]));
    for t in ["arraypath", "0.25,1", "", "5,0", "3"] {
        s.cmdline(t).unwrap();
    }
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let (e, p) = the_array(&s);
    assert!(matches!(&p.kind, ArrayKind::Path(k) if k.count == 3));
    assert_eq!(points(s.doc().unwrap(), &e).len(), 3);
}

#[test]
fn hostile_path_arrays_are_refused() {
    let mut s = Session::new();
    let o = dot(&mut s, 0.0, 1.0);
    let path = line(&mut s, json!([[0, 0], [10, 0]]));
    let before = model(&s).len();
    // The path itself, nothing to array, a count past MAXARRAY, a spacing that makes too many items.
    assert!(s.execute("arraypath", &json!({"handles": [path], "path": path})).is_err());
    assert!(s.execute("arraypath", &json!({"handles": [o], "path": path, "count": 4_000_000_000u64})).unwrap_err().to_string().contains("MAXARRAY"));
    assert!(s.execute("arraypath", &json!({"handles": [o], "path": path, "spacing": 1e-9})).is_err());
    assert!(s.execute("arraypath", &json!({"handles": [o], "path": o})).is_err());
    assert_eq!(model(&s).len(), before, "a refused array changes nothing");
    // Editing an array's count past MAXARRAY or to zero is refused too, and keeps the array.
    s.execute("arraypath", &json!({"handles": [o], "path": path, "count": 3})).unwrap();
    let (e, _) = the_array(&s);
    let a = e.handle.hex();
    assert!(s.execute("arrayedit", &json!({"handle": a, "items": 0})).is_err());
    assert!(s.execute("arrayedit", &json!({"handle": a, "items": 10_000_000})).unwrap_err().to_string().contains("MAXARRAY"));
    assert!(s.execute("arrayedit", &json!({"handle": a, "spacing": 0})).is_err());
    let (_, p) = the_array(&s);
    assert!(matches!(&p.kind, ArrayKind::Path(k) if k.count == 3 && k.divide));
}
