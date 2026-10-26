//! The Hatch Editor contextual tab's commands: hatch type (solid, gradient, pattern,
//! user-defined), double, background colour, draw order, Match Properties, boundaries removed by
//! a click, region boundaries, Display Boundary Objects, the boundary set and the default origin.

use cadcraft_engine::doc::{EntityKind, Handle, Hatch};
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{Input, Session};
use serde_json::{Value, json};

fn hatch(s: &Session, h: &str) -> Hatch {
    match &s.doc().unwrap().entity(Handle::parse_hex(h).unwrap()).unwrap().kind {
        EntityKind::Hatch(x) => x.clone(),
        k => panic!("not a hatch: {k:?}"),
    }
}

fn handle(v: &Value) -> String {
    v["handle"].as_str().unwrap().to_string()
}

fn rect(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> String {
    handle(&s.execute("rectang", &json!({ "p1": a, "p2": b })).unwrap())
}

/// A 20 x 20 square with a 5 x 5 island, hatched (ANSI31, associative), and the three handles.
fn square_with_island() -> (Session, String, String, String) {
    let mut s = Session::new();
    let outer = rect(&mut s, [0.0, 0.0], [20.0, 20.0]);
    let island = rect(&mut s, [5.0, 5.0], [10.0, 10.0]);
    let h = handle(&s.execute("hatch", &json!({ "points": [[2, 2]], "pattern": "ANSI31" })).unwrap());
    (s, outer, island, h)
}

#[test]
fn the_type_list_turns_a_hatch_solid_user_defined_pattern_or_gradient() {
    let (mut s, _, _, h) = square_with_island();
    s.execute("hatchedit", &json!({ "handles": [h], "type": "solid" })).unwrap();
    assert!(hatch(&s, &h).solid);

    s.execute("hatchedit", &json!({ "handles": [h], "type": "user", "spacing": 2.5 })).unwrap();
    let x = hatch(&s, &h);
    assert!(x.is_user() && !x.solid && !x.is_double());
    assert_eq!((x.pattern.as_str(), x.scale, x.pattern_lines.len()), ("_USER", 2.5, 1));

    s.execute("hatchedit", &json!({ "handles": [h], "double": true })).unwrap();
    assert!(hatch(&s, &h).is_double(), "a second family of lines at 90 degrees");

    s.execute("hatchedit", &json!({ "handles": [h], "type": "pattern" })).unwrap();
    let x = hatch(&s, &h);
    assert_eq!(x.pattern, "ANSI31", "a hatch turned back to a pattern takes the last pattern it had");
    assert!(x.pattern_lines.is_empty() && !x.is_user());

    s.execute("hatchedit", &json!({ "handles": [h], "type": "gradient" })).unwrap();
    assert!(hatch(&s, &h).gradient.is_some());
    assert!(s.execute("hatchedit", &json!({ "handles": [h], "type": "dotted" })).is_err());
}

/// A hatch turned back to "pattern" or "gradient" takes the last pattern or gradient a hatch had,
/// whatever its name: never a name test (a .pat pattern may be called GR_…), and its lines and
/// the gradient's settings come with it.
#[test]
fn a_hatch_turned_back_takes_the_last_pattern_and_the_last_gradient() {
    let (mut s, _, _, h) = square_with_island();
    // A .pat pattern named like a gradient tile, with its lines.
    let pat = "*GR_TEST,Not a gradient\n0,0,0,0,.5\n90,0,0,0,.5\n";
    s.execute("rectang", &json!({ "p1": [30, 0], "p2": [40, 10] })).unwrap();
    s.execute("hatch", &json!({ "points": [[35, 5]], "pattern": "GR_TEST", "patternText": pat })).unwrap();
    s.execute("hatchedit", &json!({ "handles": [h], "pattern": "GR_TEST" })).unwrap();
    assert_eq!(hatch(&s, &h).pattern, "GR_TEST");

    // Pattern GR_TEST → solid → gradient (cylinder, red to yellow, 30°) → solid → pattern.
    s.execute("hatchedit", &json!({ "handles": [h], "type": "solid" })).unwrap();
    s.execute("hatchedit", &json!({ "handles": [h], "gradient": { "name": "CYLINDER", "color1": "1", "color2": "2", "angle": 30 } })).unwrap();
    s.execute("hatchedit", &json!({ "handles": [h], "type": "solid" })).unwrap();
    s.execute("hatchedit", &json!({ "handles": [h], "type": "pattern" })).unwrap();
    let x = hatch(&s, &h);
    assert_eq!(x.pattern, "GR_TEST", "the last pattern, though its name starts with GR_");
    assert_eq!(x.pattern_lines.len(), 2, "with its lines");
    assert!(!x.solid && x.gradient.is_none());

    // Back to a gradient: the last one, with its settings.
    s.execute("hatchedit", &json!({ "handles": [h], "type": "gradient" })).unwrap();
    let g = hatch(&s, &h).gradient.expect("a gradient");
    assert_eq!(g.name, "CYLINDER");
    assert_eq!((g.color1.to_aci(), g.color2.to_aci()), (1, 2));
    assert!((g.angle.to_degrees() - 30.0).abs() < 1e-9);
}

/// With nothing remembered yet, a hatch turned to "pattern" takes ANSI31 (HPNAME, which may hold
/// a gradient tile, plays no part).
#[test]
fn with_nothing_remembered_a_pattern_is_ansi31() {
    let mut s = Session::new();
    rect(&mut s, [0.0, 0.0], [20.0, 20.0]);
    let h = handle(&s.execute("hatch", &json!({ "points": [[2, 2]], "pattern": "SOLID" })).unwrap());
    s.execute("setvar", &json!({ "name": "HPNAME", "value": "GR_LINEAR" })).unwrap();
    s.execute("hatchedit", &json!({ "handles": [h], "type": "pattern" })).unwrap();
    let x = hatch(&s, &h);
    assert_eq!(x.pattern, "ANSI31");
    assert!(x.pattern_lines.is_empty() && !x.solid && x.gradient.is_none());
}

#[test]
fn background_colour_is_set_and_cleared() {
    let (mut s, _, _, h) = square_with_island();
    s.execute("hatchedit", &json!({ "handles": [h], "background": "3" })).unwrap();
    assert!(hatch(&s, &h).background.is_some());
    s.execute("hatchedit", &json!({ "handles": [h], "background": "none" })).unwrap();
    assert!(hatch(&s, &h).background.is_none());
    assert!(s.execute("hatchedit", &json!({ "handles": [h], "background": "nope" })).is_err());
}

#[test]
fn draw_order_sends_the_hatch_behind_or_in_front_of_its_boundary() {
    let (mut s, outer, _, h) = square_with_island();
    let order = |s: &Session| s.doc().unwrap().model.handles().iter().map(|x| x.hex()).collect::<Vec<_>>();
    let pos = |s: &Session, x: &str| order(s).iter().position(|y| y == x).unwrap();
    s.execute("hatchedit", &json!({ "handles": [h], "drawOrder": "behindBoundary" })).unwrap();
    assert!(pos(&s, &h) < pos(&s, &outer));
    s.execute("hatchedit", &json!({ "handles": [h], "drawOrder": "inFrontOfBoundary" })).unwrap();
    assert!(pos(&s, &h) > pos(&s, &outer));
    s.execute("hatchedit", &json!({ "handles": [h], "drawOrder": "back" })).unwrap();
    assert_eq!(pos(&s, &h), 0);
    assert!(s.execute("hatchedit", &json!({ "handles": [h], "drawOrder": "sideways" })).is_err());
}

#[test]
fn match_properties_copies_the_source_hatch_and_keeps_the_origin_unless_asked() {
    let (mut s, _, _, h) = square_with_island();
    let other = rect(&mut s, [30.0, 0.0], [40.0, 10.0]);
    let src = handle(&s.execute("hatch", &json!({ "handles": [other], "pattern": "ANSI37", "scale": 3, "angle": 30, "origin": [7, 7] })).unwrap());
    s.execute("properties.set", &json!({ "handles": [src], "color": "1" })).unwrap();
    let before = hatch(&s, &h).origin;
    s.execute("hatchedit", &json!({ "handles": [h], "inherit": src })).unwrap();
    let x = hatch(&s, &h);
    assert_eq!((x.pattern.as_str(), x.scale), ("ANSI37", 3.0));
    assert!((x.angle.to_degrees() - 30.0).abs() < 1e-9);
    assert_eq!(x.origin, before, "HPINHERIT 0: the current origin stays");
    let e = s.doc().unwrap().entity(Handle::parse_hex(&h).unwrap()).unwrap().common.color;
    assert_eq!(e.to_aci(), 1, "the colour comes along");
    s.execute("hatchedit", &json!({ "handles": [h], "inherit": src, "inheritOrigin": true })).unwrap();
    assert_eq!(hatch(&s, &h).origin, Vec2::new(7.0, 7.0));
    assert!(s.execute("hatchedit", &json!({ "handles": [h], "inherit": other })).is_err(), "the source must be a hatch");
}

#[test]
fn a_click_near_a_boundary_removes_that_loop() {
    let (mut s, _, _, h) = square_with_island();
    assert_eq!(hatch(&s, &h).loops.len(), 2);
    // On the island's right edge.
    s.execute("hatchedit", &json!({ "handles": [h], "removeAt": [[10.1, 7.0]] })).unwrap();
    let x = hatch(&s, &h);
    assert_eq!(x.loops.len(), 1);
    assert!(x.loops[0].outer, "the outer boundary stays");
    assert!(s.execute("hatchedit", &json!({ "handles": [h], "removeAt": [[0.0, 7.0]] })).is_err(), "the last loop stays");
}

#[test]
fn boundaries_are_recreated_as_regions() {
    let (mut s, _, _, h) = square_with_island();
    let r = s.execute("hatchedit", &json!({ "handles": [h], "recreate": "region" })).unwrap();
    let made = r["handles"].as_array().unwrap();
    assert_eq!(made.len(), 2);
    let d = s.doc().unwrap();
    for m in made {
        let e = d.entity(Handle::parse_hex(m.as_str().unwrap()).unwrap()).unwrap();
        assert!(matches!(e.kind, EntityKind::Region(_)));
    }
}

#[test]
fn display_boundary_objects_selects_them_with_the_hatch() {
    let (mut s, outer, island, h) = square_with_island();
    s.set_selection(vec![Handle::parse_hex(&h).unwrap()]);
    s.execute("hatchedit.showboundary", &json!({})).unwrap();
    let sel: Vec<String> = s.selection().iter().map(|x| x.hex()).collect();
    for x in [&h, &outer, &island] {
        assert!(sel.contains(x), "{x} selected");
    }
}

#[test]
fn a_boundary_set_limits_what_pick_points_find() {
    let mut s = Session::new();
    let outer = rect(&mut s, [0.0, 0.0], [20.0, 20.0]);
    let inner = rect(&mut s, [5.0, 5.0], [10.0, 10.0]);
    s.execute("hatchboundaryset", &json!({ "handles": [outer] })).unwrap();
    // The inner square isn't in the set: the point inside it is bounded by the outer square.
    let h = handle(&s.execute("hatch", &json!({ "points": [[7, 7]] })).unwrap());
    let x = hatch(&s, &h);
    assert_eq!(x.loops.len(), 1);
    assert_eq!(x.loops[0].sources, vec![Handle::parse_hex(&outer).unwrap()]);
    s.execute("hatchboundaryset", &json!({ "clear": true })).unwrap();
    let h = handle(&s.execute("hatch", &json!({ "points": [[7, 7]] })).unwrap());
    assert_eq!(hatch(&s, &h).loops[0].sources, vec![Handle::parse_hex(&inner).unwrap()]);
}

#[test]
fn pick_points_from_the_ribbon_keep_adding_until_enter() {
    let (mut s, _, _, h) = square_with_island();
    rect(&mut s, [30.0, 0.0], [40.0, 10.0]);
    rect(&mut s, [50.0, 0.0], [60.0, 10.0]);
    s.set_selection(vec![Handle::parse_hex(&h).unwrap()]);
    s.start("hatchedit.points").unwrap();
    s.input(Input::Point(Vec2::new(35.0, 5.0))).unwrap();
    s.input(Input::Point(Vec2::new(55.0, 5.0))).unwrap();
    assert!(s.current_prompt().is_some(), "still picking");
    s.input(Input::Enter).unwrap();
    assert!(s.current_prompt().is_none());
    assert_eq!(hatch(&s, &h).loops.len(), 4);
}

#[test]
fn set_origin_from_the_ribbon_can_store_it_as_the_default() {
    let (mut s, _, _, h) = square_with_island();
    s.hatch_store_origin = true;
    s.set_selection(vec![Handle::parse_hex(&h).unwrap()]);
    s.start("hatchedit.origin").unwrap();
    s.input(Input::Point(Vec2::new(3.0, 4.0))).unwrap();
    assert_eq!(hatch(&s, &h).origin, Vec2::new(3.0, 4.0));
    let o = s.doc().unwrap().header.point("HPORIGIN").unwrap();
    assert_eq!((o.x, o.y), (3.0, 4.0));
}

#[test]
fn user_defined_double_hatches_are_saved_as_such() {
    let (mut s, _, _, h) = square_with_island();
    s.execute("hatchedit", &json!({ "handles": [h], "type": "user", "spacing": 2, "double": true, "angle": 45 })).unwrap();
    let text = cadcraft_io::write_dxf(s.doc().unwrap());
    let groups: Vec<&str> = text.lines().map(str::trim).collect();
    let has = |code: &str, value: &str| groups.chunks(2).any(|g| g == [code, value]);
    assert!(has("2", "_USER") && has("76", "0") && has("77", "1"));
    let back = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
    let x = back.model.iter().find_map(|e| if let EntityKind::Hatch(x) = &e.kind { Some(x.clone()) } else { None }).unwrap();
    assert!(x.is_double(), "read back as a double user-defined hatch");
    assert!((x.scale - 2.0).abs() < 1e-9);
}

/// HATCH follows the HP* variables the Hatch Creation tab sets while it runs.
#[test]
fn hatch_creation_follows_the_hp_variables_while_it_runs() {
    let mut s = Session::new();
    let outer = rect(&mut s, [0.0, 0.0], [20.0, 20.0]);
    s.start("hatch").unwrap();
    s.input(Input::Point(Vec2::new(5.0, 5.0))).unwrap();
    for (name, value) in
        [("HPNAME", json!("_USER")), ("HPDOUBLE", json!(1)), ("HPSCALE", json!(2.5)), ("HPCOLOR", json!("1")), ("HPBOUNDRETAIN", json!(1))]
    {
        s.execute("setvar", &json!({ "name": name, "value": value })).unwrap();
    }
    s.input(Input::Enter).unwrap();
    let d = s.doc().unwrap();
    let (h, color) = d.model.iter().find_map(|e| if let EntityKind::Hatch(x) = &e.kind { Some((x.clone(), e.common.color)) } else { None }).unwrap();
    assert!(h.is_double() && (h.scale - 2.5).abs() < 1e-9);
    assert_eq!(color.to_aci(), 1);
    // A retained boundary: a new polyline besides the rectangle, the hatch linked to it.
    let polys = d.model.iter().filter(|e| matches!(e.kind, EntityKind::LwPolyline(_))).count();
    assert_eq!(polys, 2);
    assert!(h.loops[0].sources.iter().all(|x| x.hex() != outer));
}

/// Hatch Creation's Remove and Match Properties, through HATCH's keywords.
#[test]
fn hatch_creation_removes_a_boundary_and_matches_a_hatch() {
    let mut s = Session::new();
    rect(&mut s, [0.0, 0.0], [20.0, 20.0]);
    rect(&mut s, [30.0, 0.0], [40.0, 10.0]);
    let other = rect(&mut s, [50.0, 0.0], [60.0, 10.0]);
    let src = handle(&s.execute("hatch", &json!({ "handles": [other], "pattern": "ANSI37", "scale": 3 })).unwrap());
    s.start("hatch").unwrap();
    s.input(Input::Point(Vec2::new(5.0, 5.0))).unwrap();
    s.input(Input::Point(Vec2::new(35.0, 5.0))).unwrap();
    s.input(Input::Keyword("Remove boundaries".into())).unwrap();
    s.input(Input::Point(Vec2::new(40.1, 5.0))).unwrap();
    s.input(Input::Enter).unwrap();
    s.input(Input::Keyword("Inherit properties".into())).unwrap();
    s.input(Input::Pick(vec![Handle::parse_hex(&src).unwrap()])).unwrap();
    s.input(Input::Enter).unwrap();
    let d = s.doc().unwrap();
    let made: Vec<Hatch> = d
        .model
        .iter()
        .filter(|e| e.handle.hex() != src)
        .filter_map(|e| if let EntityKind::Hatch(x) = &e.kind { Some(x.clone()) } else { None })
        .collect();
    assert_eq!(made.len(), 1);
    assert_eq!(made[0].loops.len(), 1, "the second square's boundary was removed");
    assert_eq!((made[0].pattern.as_str(), made[0].scale), ("ANSI37", 3.0));
}

#[test]
fn a_boundary_set_also_limits_the_islands() {
    let mut s = Session::new();
    let outer = rect(&mut s, [0.0, 0.0], [20.0, 20.0]);
    rect(&mut s, [5.0, 5.0], [10.0, 10.0]);
    s.execute("hatchboundaryset", &json!({ "handles": [outer] })).unwrap();
    // Picked between the squares: the inner one is not in the set, so it is no island.
    let h = handle(&s.execute("hatch", &json!({ "points": [[2, 2]] })).unwrap());
    let x = hatch(&s, &h);
    assert_eq!(x.loops.len(), 1, "only the outer square bounds the hatch");
    assert_eq!(x.loops[0].sources, vec![Handle::parse_hex(&outer).unwrap()]);
}

#[test]
fn an_empty_boundary_set_is_refused() {
    let mut s = Session::new();
    rect(&mut s, [0.0, 0.0], [20.0, 20.0]);
    assert!(s.execute("hatchboundaryset", &json!({})).is_err(), "no handles and nothing selected");
    // Pick points still find the square.
    s.execute("hatch", &json!({ "points": [[2, 2]] })).unwrap();
}

#[test]
fn a_boundary_set_belongs_to_its_drawing() {
    let mut s = Session::new();
    let outer = rect(&mut s, [0.0, 0.0], [20.0, 20.0]);
    s.execute("hatchboundaryset", &json!({ "handles": [outer] })).unwrap();
    // Another drawing: its own objects bound its hatches. Handles are numbered per drawing, so
    // its first square may share the set's handle: hatch its second one.
    s.new_drawing(true);
    rect(&mut s, [-50.0, -50.0], [-40.0, -40.0]);
    let second = rect(&mut s, [100.0, 100.0], [120.0, 120.0]);
    assert_ne!(second, outer);
    s.execute("hatch", &json!({ "points": [[110, 110]] })).unwrap();
}

#[test]
fn a_pick_outside_the_boundary_set_says_so() {
    let mut s = Session::new();
    let a = rect(&mut s, [0.0, 0.0], [20.0, 20.0]);
    rect(&mut s, [50.0, 0.0], [70.0, 20.0]);
    s.execute("hatchboundaryset", &json!({ "handles": [a] })).unwrap();
    let e = s.execute("hatch", &json!({ "points": [[60, 10]] })).unwrap_err().to_string();
    assert!(e.contains("boundary set"), "{e}");
}

#[test]
fn the_block_editor_and_the_drawing_keep_their_own_boundary_sets() {
    let mut s = Session::new();
    // Block "Box": a 10 x 10 square. In the drawing, a 20 x 20 square chosen as the boundary set.
    let inner = rect(&mut s, [0.0, 0.0], [10.0, 10.0]);
    s.execute("block", &json!({ "name": "Box", "base": [0, 0], "handles": [inner] })).unwrap();
    let outer = rect(&mut s, [100.0, 100.0], [120.0, 120.0]);
    s.execute("hatchboundaryset", &json!({ "handles": [outer] })).unwrap();
    let host = s.hatch_boundary_set().map(<[Handle]>::to_vec);

    // In the Block Editor the drawing's set doesn't apply: the block's square bounds a hatch.
    s.execute("bedit", &json!({ "name": "Box" })).unwrap();
    assert_eq!(s.hatch_boundary_set(), None);
    s.execute("hatch", &json!({ "points": [[5, 5]] })).unwrap();
    // A set chosen in the editor stays there.
    let square = s.doc().unwrap().model.iter().find(|e| matches!(e.kind, EntityKind::LwPolyline(_))).unwrap().handle.hex();
    s.execute("hatchboundaryset", &json!({ "handles": [square] })).unwrap();
    assert!(s.hatch_boundary_set().is_some());
    s.execute("bclose", &json!({ "save": false })).unwrap();

    // Back in the drawing: its own set again, unchanged; the editor's is gone.
    assert_eq!(s.hatch_boundary_set().map(<[Handle]>::to_vec), host);
    s.execute("hatch", &json!({ "points": [[110, 110]] })).unwrap();
    s.execute("bedit", &json!({ "name": "Box" })).unwrap();
    assert_eq!(s.hatch_boundary_set(), None);
}

fn hx(h: &str) -> Handle {
    Handle::parse_hex(h).unwrap()
}

/// Hatch Creation > Select New Boundary Set takes objects until Enter, then HATCH asks for points
/// again; Use Current Viewport keeps the set aside, and choosing a set again offers it back
/// ("Do you want a new boundary set? [Yes/No] <No>").
#[test]
fn a_boundary_set_takes_several_objects_and_is_kept_by_use_current_viewport() {
    let mut s = Session::new();
    let a = rect(&mut s, [0.0, 0.0], [20.0, 20.0]);
    let b = rect(&mut s, [30.0, 0.0], [50.0, 20.0]);
    let c = rect(&mut s, [60.0, 0.0], [80.0, 20.0]);
    s.start("hatch").unwrap();
    s.input(Input::Keyword("new boundary Set".into())).unwrap();
    assert!(s.current_prompt().unwrap().message.contains("new boundary set"), "no set yet: selection straight away");
    // Two picks: the selection goes on until Enter.
    s.input(Input::Pick(vec![hx(&a)])).unwrap();
    assert!(s.current_prompt().unwrap().message.contains("new boundary set"), "still selecting");
    s.input(Input::Pick(vec![hx(&b)])).unwrap();
    s.input(Input::Enter).unwrap();
    assert_eq!(s.hatch_boundary_set().map(<[Handle]>::len), Some(2));
    assert!(s.current_prompt().unwrap().message.contains("Pick internal point"), "back to HATCH");
    assert!(s.selection().is_empty(), "the set's objects are not left selected");

    // Use Current Viewport: every object again, the set kept.
    s.input(Input::Keyword("use Current viewport".into())).unwrap();
    assert_eq!(s.hatch_boundary_set(), None);
    assert_eq!(s.hatch_boundary_set_kept().map(<[Handle]>::len), Some(2));
    assert!(s.log.iter().any(|l| l.contains("boundary set of 2 object(s) kept")), "{:?}", s.log);

    // Select New Boundary Set again: No (Enter) uses the kept set.
    s.input(Input::Keyword("new boundary Set".into())).unwrap();
    let p = s.current_prompt().unwrap();
    assert!(s.log.last().is_some_and(|l| l.contains("Current boundary set: 2 object(s).")), "{:?}", s.log.last());
    assert!(p.message.contains("Do you want a new boundary set"), "{}", p.message);
    assert_eq!(p.default.as_deref(), Some("No"));
    s.input(Input::Enter).unwrap();
    assert_eq!(s.hatch_boundary_set().map(<[Handle]>::len), Some(2));

    // Yes: a new set replaces it.
    s.input(Input::Keyword("new boundary Set".into())).unwrap();
    s.input(Input::Keyword("Yes".into())).unwrap();
    s.input(Input::Pick(vec![hx(&c)])).unwrap();
    s.input(Input::Enter).unwrap();
    assert_eq!(s.hatch_boundary_set(), Some(&[hx(&c)][..]));
    s.cancel();
}

/// HATCHBOUNDARYSET typed asks the same question whenever a set exists, in use or kept.
#[test]
fn hatchboundaryset_typed_offers_the_set_it_has() {
    let mut s = Session::new();
    let a = rect(&mut s, [0.0, 0.0], [20.0, 20.0]);
    let b = rect(&mut s, [30.0, 0.0], [50.0, 20.0]);
    s.execute("hatchboundaryset", &json!({ "handles": [a, b] })).unwrap();
    s.start("hatchboundaryset").unwrap();
    assert!(s.log.last().is_some_and(|l| l.contains("Current boundary set: 2 object(s).")));
    assert!(s.current_prompt().unwrap().message.contains("Do you want a new boundary set"));
    s.input(Input::Keyword("No".into())).unwrap();
    assert!(s.current_prompt().is_none(), "No ends the command");
    assert_eq!(s.hatch_boundary_set().map(<[Handle]>::len), Some(2));

    // Kept aside, then offered back the same way.
    let r = s.execute("hatchboundaryset", &json!({ "clear": true })).unwrap();
    assert_eq!(r["kept"], json!(2));
    assert_eq!(s.hatch_boundary_set(), None);
    s.start("hatchboundaryset").unwrap();
    s.input(Input::Enter).unwrap();
    assert_eq!(s.hatch_boundary_set().map(<[Handle]>::len), Some(2), "Enter: No, the kept set is used");
    // The JSON form: `reuse` takes the kept set again.
    s.execute("hatchboundaryset", &json!({ "clear": true })).unwrap();
    assert_eq!(s.execute("hatchboundaryset", &json!({ "reuse": true })).unwrap()["objects"], json!(2));
}

/// Esc while choosing a boundary set gives up that choice only: HATCH goes on with the points
/// already picked, the set there was stays, and the objects picked for the new one are not left
/// selected. A second Esc cancels HATCH.
#[test]
fn esc_while_choosing_a_boundary_set_gives_up_the_choice_only() {
    let mut s = Session::new();
    let a = rect(&mut s, [0.0, 0.0], [20.0, 20.0]);
    let b = rect(&mut s, [30.0, 0.0], [50.0, 20.0]);
    s.execute("hatchboundaryset", &json!({ "handles": [a] })).unwrap();
    s.start("hatch").unwrap();
    s.input(Input::Point(Vec2::new(5.0, 5.0))).unwrap();
    s.input(Input::Keyword("new boundary Set".into())).unwrap();
    s.input(Input::Keyword("Yes".into())).unwrap();
    s.input(Input::Pick(vec![hx(&b)])).unwrap();
    assert!(!s.selection().is_empty(), "the picked object shows selected while choosing");
    s.cancel();
    assert!(s.running.is_some(), "HATCH goes on");
    assert!(s.current_prompt().unwrap().message.contains("Pick internal point"));
    assert!(s.selection().is_empty());
    assert_eq!(s.hatch_boundary_set(), Some(&[hx(&a)][..]), "the set there was stays");
    // Enter makes the hatch from the point picked before Esc.
    s.input(Input::Enter).unwrap();
    assert!(s.running.is_none());
    assert!(s.doc().unwrap().model.iter().any(|e| matches!(e.kind, EntityKind::Hatch(_))));

    // A second Esc cancels HATCH.
    s.start("hatch").unwrap();
    s.input(Input::Keyword("new boundary Set".into())).unwrap();
    s.cancel();
    assert!(s.running.is_some());
    s.cancel();
    assert!(s.running.is_none());

    // HATCHBOUNDARYSET typed: Esc ends it without leaving the picked objects selected.
    s.start("hatchboundaryset").unwrap();
    s.input(Input::Keyword("Yes".into())).unwrap();
    s.input(Input::Pick(vec![hx(&b)])).unwrap();
    s.cancel();
    assert!(s.running.is_none());
    assert!(s.selection().is_empty());
    assert_eq!(s.hatch_boundary_set(), Some(&[hx(&a)][..]));
}

/// A Hatch Creation button while a boundary set is being chosen gives up the choice, as Esc does,
/// then does what it does: Close makes the hatch at once, Pick Points picks points again.
#[test]
fn a_hatch_creation_button_gives_up_the_boundary_set_choice_then_acts() {
    let mut s = Session::new();
    let a = rect(&mut s, [0.0, 0.0], [20.0, 20.0]);
    s.start("hatch").unwrap();
    s.input(Input::Point(Vec2::new(5.0, 5.0))).unwrap();
    s.input(Input::Keyword("new boundary Set".into())).unwrap();
    s.input(Input::Pick(vec![hx(&a)])).unwrap();
    s.input(Input::Keyword("picK internal point".into())).unwrap();
    assert!(s.current_prompt().unwrap().message.contains("Pick internal point"));
    assert!(s.selection().is_empty());
    assert_eq!(s.hatch_boundary_set(), None, "no set was chosen");
    s.input(Input::Keyword("new boundary Set".into())).unwrap();
    s.input(Input::Pick(vec![hx(&a)])).unwrap();
    s.input(Input::Keyword("Close".into())).unwrap();
    assert!(s.running.is_none(), "one Close ends HATCH");
    assert_eq!(s.doc().unwrap().model.iter().filter(|e| matches!(e.kind, EntityKind::Hatch(_))).count(), 1);
    assert_eq!(s.hatch_boundary_set(), None);
}

/// The Block Editor's boundary set stays with the editor of its drawing: through BSAVEAS, and
/// apart from another drawing's Block Editor.
#[test]
fn the_block_editor_boundary_set_survives_bsaveas_and_stays_with_its_drawing() {
    let mut s = Session::new();
    let inner = rect(&mut s, [0.0, 0.0], [10.0, 10.0]);
    s.execute("block", &json!({ "name": "Box", "base": [0, 0], "handles": [inner] })).unwrap();
    s.execute("bedit", &json!({ "name": "Box" })).unwrap();
    let square = s.doc().unwrap().model.iter().find(|e| matches!(e.kind, EntityKind::LwPolyline(_))).unwrap().handle;
    s.execute("hatchboundaryset", &json!({ "handles": [square.hex()] })).unwrap();
    s.execute("bsaveas", &json!({ "name": "Box2" })).unwrap();
    assert_eq!(s.hatch_boundary_set(), Some(&[square][..]), "kept through BSAVEAS");

    // Another drawing's Block Editor has its own set; closing it leaves the first one's.
    let first = s.active;
    s.execute("new", &json!({})).unwrap();
    let other = rect(&mut s, [0.0, 0.0], [5.0, 5.0]);
    s.execute("block", &json!({ "name": "Tile", "base": [0, 0], "handles": [other] })).unwrap();
    s.execute("bedit", &json!({ "name": "Tile" })).unwrap();
    assert_eq!(s.hatch_boundary_set(), None);
    let tile = s.doc().unwrap().model.iter().find(|e| matches!(e.kind, EntityKind::LwPolyline(_))).unwrap().handle;
    s.execute("hatchboundaryset", &json!({ "handles": [tile.hex()] })).unwrap();
    s.execute("bclose", &json!({ "save": false })).unwrap();
    s.active = first;
    assert_eq!(s.hatch_boundary_set(), Some(&[square][..]), "the first drawing's editor keeps its set");
}

/// HATCH.TYPE (the Hatch Creation tab's Type list) remembers the pattern or gradient being made
/// and turns back to the last one, with a drawing's own pattern keeping its lines.
#[test]
fn hatch_type_turns_back_to_the_last_pattern_with_its_lines() {
    let mut s = Session::new();
    rect(&mut s, [0.0, 0.0], [20.0, 20.0]);
    s.execute("setvar", &json!({ "name": "HPNAME", "value": "ANSI37" })).unwrap();
    s.execute("hatch.type", &json!({ "type": "solid" })).unwrap();
    assert_eq!(s.doc().unwrap().header.str("HPNAME", ""), "SOLID");
    s.execute("hatch.type", &json!({ "type": "pattern" })).unwrap();
    assert_eq!(s.doc().unwrap().header.str("HPNAME", ""), "ANSI37");
    assert!(s.execute("hatch.type", &json!({ "type": "dots" })).is_err());

    // A drawing's own pattern (not defined by CADCraft), remembered from a hatch that had it.
    let own = cadcraft_engine::doc::library::PatternLine { angle: 30.0, origin: (0.0, 0.0), delta: (0.0, 0.7), dashes: vec![0.5, -0.25] };
    let first = handle(&s.execute("hatch", &json!({ "points": [[2, 2]], "pattern": "ANSI31" })).unwrap());
    let mut h = hatch(&s, &first);
    h.pattern = "MYPAT".into();
    h.pattern_lines = vec![own.clone()];
    // Being solid, the hatch being made doesn't replace it in the memory.
    s.execute("setvar", &json!({ "name": "HPNAME", "value": "SOLID" })).unwrap();
    s.remember_hatch(&h);
    s.execute("hatch.type", &json!({ "type": "pattern" })).unwrap();
    assert_eq!(s.doc().unwrap().header.str("HPNAME", ""), "MYPAT");
    let made = handle(&s.execute("hatch", &json!({ "points": [[2, 2]] })).unwrap());
    assert_eq!(hatch(&s, &made).pattern_lines, vec![own], "drawn with its own lines, not ANSI31");
}
