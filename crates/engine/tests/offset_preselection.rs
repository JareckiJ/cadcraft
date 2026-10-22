//! OFFSET uses a single object selected before the command (issue #1610): drawing a rectangle,
//! selecting it and starting OFFSET used to ask for the object again. Now the preselected object
//! goes straight to the side prompt; a multiple selection still asks which object to offset.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle};
use serde_json::json;

/// Every line as ((ax, ay), (bx, by)), sorted by the first y.
fn lines(s: &Session) -> Vec<((f64, f64), (f64, f64))> {
    let mut v: Vec<_> = s
        .doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| if let EntityKind::Line(l) = &e.kind { Some(((l.a.x, l.a.y), (l.b.x, l.b.y))) } else { None })
        .collect();
    v.sort_by(|a, b| a.0.1.total_cmp(&b.0.1).then(a.0.0.total_cmp(&b.0.0)));
    v
}

fn two_lines() -> (Session, Vec<Handle>) {
    let mut s = Session::new();
    s.script("LINE 0,0 10,0\n\nLINE 0,5 10,5\n\n").unwrap();
    let hs: Vec<_> = s.doc().unwrap().model.iter().map(|e| e.handle).collect();
    (s, hs)
}

fn prompt(s: &Session) -> String {
    s.current_prompt().map(|p| p.message.clone()).unwrap_or_default()
}

#[test]
fn one_preselected_object_goes_straight_to_the_side_prompt() {
    let (mut s, hs) = two_lines();
    s.set_selection(vec![hs[0]]);
    s.cmdline("OFFSET").unwrap();
    // The distance still comes first (as without a selection)...
    assert!(prompt(&s).contains("Specify offset distance"), "{}", s.prompt_text());
    s.cmdline("2").unwrap();
    // ...then the preselected line is taken and only the side is asked for.
    assert!(prompt(&s).contains("Specify point on side to offset"), "{}", s.prompt_text());
    s.cmdline("5,1").unwrap();
    assert_eq!(lines(&s), vec![((0.0, 0.0), (10.0, 0.0)), ((0.0, 2.0), (10.0, 2.0)), ((0.0, 5.0), (10.0, 5.0))]);
    // The command keeps its object loop for further objects.
    assert!(prompt(&s).contains("Select object to offset"), "{}", s.prompt_text());
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
}

#[test]
fn two_preselected_objects_still_ask_to_select_one() {
    let (mut s, hs) = two_lines();
    s.set_selection(hs);
    s.cmdline("OFFSET").unwrap();
    s.cmdline("2").unwrap();
    assert!(prompt(&s).contains("Select object to offset"), "{}", s.prompt_text());
    s.cmdline("5,0").unwrap();
    assert!(prompt(&s).contains("Specify point on side to offset"), "{}", s.prompt_text());
    s.cmdline("5,2").unwrap();
    assert_eq!(lines(&s), vec![((0.0, 0.0), (10.0, 0.0)), ((0.0, 2.0), (10.0, 2.0)), ((0.0, 5.0), (10.0, 5.0))]);
    assert!(prompt(&s).contains("Select object to offset"), "{}", s.prompt_text());
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
}

#[test]
fn one_preselected_non_offsettable_object_still_asks_to_select() {
    let mut s = Session::new();
    s.execute("text", &json!({ "at": [0, 0], "text": "NOTE", "height": 2 })).unwrap();
    let h = s.doc().unwrap().model.iter().next().unwrap().handle;
    s.set_selection(vec![h]);
    s.cmdline("OFFSET").unwrap();
    s.cmdline("2").unwrap();
    assert!(prompt(&s).contains("Select object to offset"), "{}", s.prompt_text());
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
}
