//! Groups (#417), part 2: PICKSTYLE group selection (clicks and "Select objects" prompts pick
//! whole selectable groups; Ctrl+Shift+A toggles it) and the -GROUP options.

use cadcraft_engine::doc::Handle;
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{Input, Session};
use serde_json::json;

/// Four horizontal lines at y = 0, 5, 10, 15; their handles.
fn lines() -> (Session, Vec<Handle>) {
    let mut s = Session::new();
    for y in [0, 5, 10, 15] {
        s.execute("line", &json!({ "points": [[0, y], [10, y]] })).unwrap();
    }
    let hs = s.doc().unwrap().model.iter().map(|e| e.handle).collect();
    (s, hs)
}

fn on(i: usize) -> Vec2 {
    Vec2::new(5.0, 5.0 * i as f64)
}

fn hex(hs: &[Handle]) -> Vec<String> {
    hs.iter().map(|h| h.hex()).collect()
}

fn members(s: &Session, name: &str) -> Vec<Handle> {
    s.doc().unwrap().groups.iter().find(|g| g.name == name).map(|g| g.members.clone()).unwrap_or_default()
}

#[test]
fn clicks_pick_whole_selectable_groups() {
    let (mut s, hs) = lines();
    s.execute("group", &json!({ "name": "A", "handles": hex(&hs[..2]) })).unwrap();
    s.execute("group", &json!({ "name": "Off", "handles": hex(&hs[2..3]), "selectable": false })).unwrap();
    assert_eq!(s.execute("getvar", &json!({ "name": "PICKSTYLE" })).unwrap()["value"], 1);
    s.idle_click(on(1), false).unwrap();
    assert_eq!(s.selection(), vec![hs[1], hs[0]], "the picked object first, then its group");
    // Shift+click takes the whole group out again.
    s.idle_click(on(0), true).unwrap();
    assert!(s.selection().is_empty());
    // An unselectable group's members are picked one by one; a window brings whole groups.
    s.idle_click(on(2), false).unwrap();
    assert_eq!(s.selection(), vec![hs[2]]);
    s.set_selection(Vec::new());
    s.idle_click(Vec2::new(-1.0, -1.0), false).unwrap();
    s.idle_click(Vec2::new(11.0, 1.0), false).unwrap();
    assert_eq!(s.selection(), vec![hs[0], hs[1]]);
    // Ctrl+Shift+A (PICKSTYLE without a value) turns group selection off.
    let r = s.execute("pickstyle", &json!({})).unwrap();
    assert_eq!(r["value"], 0);
    s.set_selection(Vec::new());
    s.idle_click(on(1), false).unwrap();
    assert_eq!(s.selection(), vec![hs[1]]);
    // SETVAR and the typed form; values outside 0-3 are refused.
    s.execute("setvar", &json!({ "name": "PICKSTYLE", "value": 3 })).unwrap();
    assert_eq!(s.settings.pickstyle, 3);
    assert!(s.execute("setvar", &json!({ "name": "PICKSTYLE", "value": 4 })).is_err());
    assert!(s.execute("pickstyle", &json!({ "value": -1 })).is_err());
    s.cmdline("pickstyle").unwrap();
    assert!(s.prompt_text().contains("PICKSTYLE <3>"), "{}", s.prompt_text());
    s.cmdline("7").unwrap();
    assert!(s.running.is_some(), "asked again");
    s.cmdline("1").unwrap();
    assert!(s.running.is_none());
    assert_eq!(s.settings.pickstyle, 1);
}

#[test]
fn select_objects_prompts_pick_groups() {
    let (mut s, hs) = lines();
    s.execute("group", &json!({ "name": "A", "handles": hex(&hs[..2]) })).unwrap();
    s.cmdline("erase").unwrap();
    s.input(Input::Point(on(0))).unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    let left: Vec<Handle> = s.doc().unwrap().model.iter().map(|e| e.handle).collect();
    assert_eq!(left, hs[2..].to_vec(), "both members erased");
    assert!(s.doc().unwrap().groups.is_empty(), "the emptied group is gone");
    s.execute("u", &json!({})).unwrap();
    // GROUPEDIT picks single members while removing.
    s.cmdline("groupedit n a r").unwrap();
    s.input(Input::Point(on(1))).unwrap();
    s.cmdline("").unwrap();
    assert_eq!(members(&s, "A"), vec![hs[0]]);
}

#[test]
fn dash_group_options_typed() {
    let (mut s, hs) = lines();
    s.cmdline("-group").unwrap();
    assert!(s.prompt_text().contains("[?/Order/Add/Remove/Explode/REName/Selectable/Create] <Create>"), "{}", s.prompt_text());
    s.cmdline("").unwrap();
    s.cmdline("Doors").unwrap();
    s.cmdline("front doors").unwrap();
    s.input(Input::Pick(hs[..3].to_vec())).unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    let g = s.doc().unwrap().groups.first().cloned().unwrap();
    assert_eq!((g.name.as_str(), g.description.as_str(), g.members.clone()), ("Doors", "front doors", hs[..3].to_vec()));
    // Order: move the object at position 2 to the front.
    s.cmdline("-group o doors 2 0").unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(members(&s, "Doors"), vec![hs[2], hs[0], hs[1]]);
    // Add and Remove pick single objects.
    s.cmdline("-group a doors").unwrap();
    s.input(Input::Point(on(3))).unwrap();
    s.cmdline("").unwrap();
    assert_eq!(members(&s, "Doors").len(), 4);
    s.cmdline("-group r doors").unwrap();
    s.input(Input::Point(on(0))).unwrap();
    s.cmdline("").unwrap();
    assert_eq!(members(&s, "Doors"), vec![hs[2], hs[1], hs[3]]);
    // Selectable: Enter accepts the toggle.
    s.cmdline("-group s doors").unwrap();
    assert!(s.prompt_text().contains("is selectable. Make it unselectable?"), "{}", s.prompt_text());
    s.cmdline("").unwrap();
    assert!(!s.doc().unwrap().groups[0].selectable);
    // REName, ?, Explode.
    s.cmdline("-group ren doors Gates").unwrap();
    assert_eq!(s.doc().unwrap().groups[0].name, "Gates");
    s.cmdline("-group ? g*").unwrap();
    assert!(s.log.last().is_some_and(|l| l.starts_with("Gates: front doors, 3 object(s), not selectable")), "{:?}", s.log.last());
    s.cmdline("-group e gates").unwrap();
    assert!(s.doc().unwrap().groups.is_empty());
    // Create with `*` makes an unnamed group.
    s.cmdline("-group c *").unwrap();
    s.cmdline("").unwrap();
    s.input(Input::Pick(vec![hs[0]])).unwrap();
    s.cmdline("").unwrap();
    assert_eq!(s.doc().unwrap().groups[0].name, "*A1");
}

#[test]
fn dash_group_json() {
    let (mut s, hs) = lines();
    let r = s.execute("-group", &json!({ "name": "G", "handles": hex(&hs[..3]) })).unwrap();
    assert_eq!(r["name"], "G");
    let r = s.execute("-group", &json!({ "option": "order", "name": "g", "from": 1, "to": 0, "count": 2 })).unwrap();
    assert_eq!(r["members"], json!(hex(&[hs[1], hs[2], hs[0]])));
    for bad in
        [json!({ "from": 3, "to": 0 }), json!({ "from": 2, "to": 0, "count": 2 }), json!({ "from": 0, "to": 9 }), json!({ "from": -1, "to": 0 })]
    {
        let mut p = bad.clone();
        p["option"] = json!("order");
        p["name"] = json!("G");
        assert!(s.execute("-group", &p).is_err(), "{p}");
    }
    assert_eq!(s.execute("-group", &json!({ "option": "add", "name": "G", "handles": [hs[3].hex()] })).unwrap()["added"], 1);
    assert_eq!(s.execute("-group", &json!({ "option": "selectable", "name": "G" })).unwrap()["selectable"], false);
    assert_eq!(s.execute("-group", &json!({ "option": "?", "name": "x*" })).unwrap()["groups"], json!([]));
    assert_eq!(s.execute("-group", &json!({ "option": "rename", "name": "G", "newName": "H" })).unwrap()["name"], "H");
    let r = s.execute("-group", &json!({ "option": "remove", "name": "H", "handles": hex(&hs) })).unwrap();
    assert_eq!(r["deleted"], true);
    assert!(s.execute("-group", &json!({ "option": "explode", "name": "H" })).is_err());
    assert!(s.execute("-group", &json!({ "option": "bogus" })).is_err());
    assert!(s.running.is_none());
}

#[test]
fn copying_whole_group_creates_new_anonymous_group() {
    let (mut s, hs) = lines();
    s.execute("group", &json!({ "name": "Original", "handles": hex(&hs[..2]) })).unwrap();
    assert_eq!(s.doc().unwrap().groups.len(), 1);

    // Copying both members together creates a new anonymous group (*A1) for the copies.
    let r = s.execute("copy", &json!({ "handles": hex(&hs[..2]), "delta": [0, 20] })).unwrap();
    let copies: Vec<String> = r["handles"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
    assert_eq!(copies.len(), 2);
    assert_eq!(s.doc().unwrap().groups.len(), 2);
    let g2 = &s.doc().unwrap().groups[1];
    assert_eq!(g2.name, "*A1");
    assert_eq!(hex(&g2.members), copies);

    // Copying only one member of "Original" does NOT create a new group.
    s.execute("copy", &json!({ "handles": [hs[0].hex()], "delta": [0, 40] })).unwrap();
    assert_eq!(s.doc().unwrap().groups.len(), 2);
}

#[test]
fn pickstyle_associative_hatch_selection() {
    let mut s = Session::new();
    // Create a closed rectangular polyline boundary.
    s.execute("pline", &json!({ "points": [[0, 0], [10, 0], [10, 10], [0, 10]], "closed": true })).unwrap();
    let rect_h = s.doc().unwrap().model.iter().last().unwrap().handle;

    // Create associative hatch with rect_h as boundary.
    let r = s.execute("hatch", &json!({ "handles": [rect_h.hex()] })).unwrap();
    let hatch_h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();

    // PICKSTYLE 0 (no group, no hatch expansion):
    s.execute("setvar", &json!({ "name": "PICKSTYLE", "value": 0 })).unwrap();
    s.idle_click(Vec2::new(0.0, 5.0), false).unwrap();
    assert_eq!(s.selection(), vec![rect_h]);

    // PICKSTYLE 1 (group only):
    s.set_selection(Vec::new());
    s.execute("setvar", &json!({ "name": "PICKSTYLE", "value": 1 })).unwrap();
    s.idle_click(Vec2::new(0.0, 5.0), false).unwrap();
    assert_eq!(s.selection(), vec![rect_h]);

    // PICKSTYLE 2 (associative hatch selection):
    s.set_selection(Vec::new());
    s.execute("setvar", &json!({ "name": "PICKSTYLE", "value": 2 })).unwrap();
    s.idle_click(Vec2::new(0.0, 5.0), false).unwrap();
    assert_eq!(s.selection(), vec![rect_h, hatch_h]);

    // Clicking the hatch also expands to select the boundary:
    s.set_selection(Vec::new());
    s.idle_click(Vec2::new(5.0, 5.0), false).unwrap();
    assert_eq!(s.selection(), vec![hatch_h, rect_h]);
}

#[test]
fn groupdisplaymode_system_variable() {
    let mut s = Session::new();
    // Default is 2 (bounding box only).
    assert_eq!(s.execute("getvar", &json!({ "name": "GROUPDISPLAYMODE" })).unwrap()["value"], 2);

    s.execute("setvar", &json!({ "name": "GROUPDISPLAYMODE", "value": 0 })).unwrap();
    assert_eq!(s.settings.groupdisplaymode, 0);

    s.execute("setvar", &json!({ "name": "GROUPDISPLAYMODE", "value": 1 })).unwrap();
    assert_eq!(s.settings.groupdisplaymode, 1);

    s.execute("setvar", &json!({ "name": "GROUPDISPLAYMODE", "value": 2 })).unwrap();
    assert_eq!(s.settings.groupdisplaymode, 2);

    assert!(s.execute("setvar", &json!({ "name": "GROUPDISPLAYMODE", "value": -1 })).is_err());
    assert!(s.execute("setvar", &json!({ "name": "GROUPDISPLAYMODE", "value": 3 })).is_err());
}
