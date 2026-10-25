//! DIMBREAK Auto tracks objects, including edits with no remaining intersection. Gaps are
//! updated by the same command and undo step as the objects, through JSON and prompts.

use std::sync::Arc;

use cadcraft_engine::doc::{AutoDimBreak, DimBreak, Dimension, EntityKind, Handle, Space};
use cadcraft_engine::geom::{Vec2, Vec3};
use cadcraft_engine::render::LineRole;
use cadcraft_engine::{Input, Session};
use serde_json::json;

fn linear(s: &mut Session) -> Handle {
    let r = s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [10, 0], "at": [5, 3] })).unwrap();
    Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap()
}

fn line(s: &mut Session, x: f64) -> Handle {
    s.execute("line", &json!({ "points": [[x, -1], [x, 6]] })).unwrap();
    s.doc().unwrap().space(&s.space()).unwrap().last().unwrap().handle
}

fn dim(s: &Session, h: Handle) -> &Dimension {
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::Dimension(dm) => dm,
        k => panic!("expected dimension, got {k:?}"),
    }
}

fn move_by(s: &mut Session, h: Handle, dx: f64, dy: f64) {
    s.execute("move", &json!({ "handles": [h.hex()], "delta": [dx, dy] })).unwrap();
}

fn auto(s: &mut Session, h: Handle, source: Handle) {
    s.execute("dimbreak", &json!({ "handles": [h.hex()], "objects": [source.hex()], "size": 0.5 })).unwrap();
}

fn assert_gap(s: &Session, h: Handle, x: f64, y: f64) {
    let dm = dim(s, h);
    let gaps: Vec<_> = dm.marks.breaks.iter().filter(|b| b.auto).collect();
    assert_eq!(gaps.len(), 1, "{:?}", dm.marks);
    assert!(gaps[0].at.xy().dist(Vec2::new(x, y)) < 1e-9, "{:?}", gaps[0]);
    let geometry = cadcraft_engine::render::dimension_in(s.doc().unwrap(), dm);
    assert!(!geometry.lines_of(LineRole::Dim).any(|l| {
        l.windows(2).any(|w| (w[0].y - y).abs() < 1e-9 && (w[1].y - y).abs() < 1e-9 && w[0].x.min(w[1].x) < x && x < w[0].x.max(w[1].x))
    }));
}

#[test]
fn source_moves_and_erasure_share_the_commands_undo_steps() {
    let mut s = Session::new();
    let h = linear(&mut s);
    let source = line(&mut s, 4.0);
    auto(&mut s, h, source);
    assert_gap(&s, h, 4.0, 3.0);
    move_by(&mut s, source, 2.0, 0.0);
    assert_gap(&s, h, 6.0, 3.0);
    s.execute("undo", &json!({})).unwrap();
    assert_gap(&s, h, 4.0, 3.0);
    s.execute("redo", &json!({})).unwrap();
    assert_gap(&s, h, 6.0, 3.0);
    s.execute("erase", &json!({ "handles": [source.hex()] })).unwrap();
    assert!(dim(&s, h).marks.breaks.is_empty());
    s.execute("undo", &json!({})).unwrap();
    assert_gap(&s, h, 6.0, 3.0);
    s.execute("redo", &json!({})).unwrap();
    assert!(dim(&s, h).marks.breaks.is_empty());
}

#[test]
fn missing_crossings_return_and_manual_gaps_stay_put() {
    let mut s = Session::new();
    let h = linear(&mut s);
    let source = line(&mut s, 4.0);
    auto(&mut s, h, source);
    s.execute("dimbreak", &json!({ "handles": [h.hex()], "mode": "manual", "first": [7, 3], "second": [8, 3] })).unwrap();
    move_by(&mut s, source, 20.0, 0.0);
    let dm = dim(&s, h);
    assert_eq!(dm.marks.breaks.len(), 1);
    assert!(!dm.marks.breaks[0].auto);
    assert_eq!(dm.marks.breaks[0].at, Vec3::new(7.5, 3.0, 0.0));
    move_by(&mut s, source, -18.0, 0.0);
    assert_gap(&s, h, 6.0, 3.0);
    assert_eq!(dim(&s, h).marks.breaks.len(), 2);
    // Remove also removes the relationship, so later moves cannot recreate gaps.
    s.execute("dimbreak", &json!({ "handles": [h.hex()], "mode": "remove" })).unwrap();
    assert!(dim(&s, h).marks.auto_break.is_none());
    move_by(&mut s, source, -2.0, 0.0);
    assert!(dim(&s, h).marks.breaks.is_empty());
}

#[test]
fn dimension_edits_and_measured_object_associativity_refresh_breaks() {
    let mut s = Session::new();
    let h = linear(&mut s);
    let source = line(&mut s, 4.0);
    auto(&mut s, h, source);
    move_by(&mut s, h, 0.0, 1.0);
    assert_gap(&s, h, 4.0, 4.0);
    move_by(&mut s, h, 20.0, 0.0);
    assert!(dim(&s, h).marks.breaks.is_empty());
    move_by(&mut s, h, -20.0, 0.0);
    assert_gap(&s, h, 4.0, 4.0);

    let mut s = Session::new();
    s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    let measured = s.doc().unwrap().model.last().unwrap().handle;
    let r = s.execute("dimlinear", &json!({ "object": measured.hex(), "at": [5, 3] })).unwrap();
    let h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    let source = line(&mut s, 4.0);
    auto(&mut s, h, source);
    move_by(&mut s, measured, 0.0, 1.0);
    assert_gap(&s, h, 4.0, 4.0);
}

#[test]
fn prompt_edits_follow_only_the_selected_source() {
    let mut s = Session::new();
    let h = linear(&mut s);
    let source = line(&mut s, 4.0);
    let other = line(&mut s, 8.0);
    s.start("dimbreak").unwrap();
    s.input(Input::Pick(vec![h])).unwrap();
    s.input(Input::Pick(vec![source])).unwrap();
    s.input(Input::Enter).unwrap();
    assert!(s.running.is_none());
    assert_gap(&s, h, 4.0, 3.0);
    s.start("move").unwrap();
    s.input(Input::Pick(vec![source])).unwrap();
    s.input(Input::Enter).unwrap();
    s.input(Input::Point(Vec2::ZERO)).unwrap();
    s.input(Input::Point(Vec2::new(2.0, 0.0))).unwrap();
    assert!(s.running.is_none());
    assert_gap(&s, h, 6.0, 3.0);
    let before = s.doc().unwrap().entity(h).unwrap().clone();
    move_by(&mut s, other, -1.0, 0.0);
    assert!(Arc::ptr_eq(&before, s.doc().unwrap().entity(h).unwrap()), "unrelated edits leave the dimension alone");
    s.execute("undo", &json!({})).unwrap();
    s.execute("undo", &json!({})).unwrap();
    assert_gap(&s, h, 4.0, 3.0);
}

#[test]
fn relationships_survive_dxf_even_without_a_current_gap() {
    let mut s = Session::new();
    let h = linear(&mut s);
    let source = line(&mut s, 24.0);
    auto(&mut s, h, source);
    assert!(dim(&s, h).marks.breaks.is_empty());
    let text = cadcraft_io::write_dxf(s.doc().unwrap());
    let d = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
    s.state_mut().unwrap().doc = Arc::new(d);
    assert_eq!(dim(&s, h).marks.auto_break, Some(AutoDimBreak { objects: vec![source], size: 0.5 }));
    move_by(&mut s, source, -20.0, 0.0);
    assert_gap(&s, h, 4.0, 3.0);
    let text = cadcraft_io::write_dxf(s.doc().unwrap());
    s.state_mut().unwrap().doc = Arc::new(cadcraft_io::read_dxf(text.as_bytes()).unwrap());
    move_by(&mut s, source, 2.0, 0.0);
    assert_gap(&s, h, 6.0, 3.0);
    assert!(dim(&s, h).block.is_none(), "the saved dimension block must be regenerated");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn relationships_survive_dwg_and_continue_following() {
    let mut s = Session::new();
    let h = linear(&mut s);
    let source = line(&mut s, 4.0);
    auto(&mut s, h, source);
    let bytes = cadcraft_io::write(s.doc().unwrap(), "breaks.dwg").unwrap();
    s.state_mut().unwrap().doc = Arc::new(cadcraft_io::read(&bytes, "breaks.dwg").unwrap());
    assert_eq!(dim(&s, h).marks.auto_break, Some(AutoDimBreak { objects: vec![source], size: 0.5 }));
    move_by(&mut s, source, 2.0, 0.0);
    assert_gap(&s, h, 6.0, 3.0);
}

#[test]
fn scaling_and_copying_keep_the_gap_size_and_source_relationships() {
    let mut s = Session::new();
    let h = linear(&mut s);
    let source = line(&mut s, 4.0);
    auto(&mut s, h, source);
    s.execute("scale", &json!({ "handles": [h.hex(), source.hex()], "base": [0, 0], "factor": 2 })).unwrap();
    assert_gap(&s, h, 8.0, 6.0);
    assert_eq!(dim(&s, h).marks.auto_break.as_ref().unwrap().size, 1.0);
    assert_eq!(dim(&s, h).marks.breaks[0].size, 1.0);
    let r = s.execute("copy", &json!({ "handles": [h.hex(), source.hex()], "delta": [30, 0] })).unwrap();
    let copied_dim = Handle::parse_hex(r["handles"][0].as_str().unwrap()).unwrap();
    let copied_source = Handle::parse_hex(r["handles"][1].as_str().unwrap()).unwrap();
    assert_eq!(dim(&s, copied_dim).marks.auto_break.as_ref().unwrap().objects, vec![copied_source]);
    assert_gap(&s, copied_dim, 38.0, 6.0);
    move_by(&mut s, copied_source, 1.0, 0.0);
    assert_gap(&s, copied_dim, 39.0, 6.0);
    assert_gap(&s, h, 8.0, 6.0);
}

#[test]
fn pasted_dimensions_cannot_follow_an_unrelated_handle_in_another_drawing() {
    let mut s = Session::new();
    let h = linear(&mut s);
    let source = line(&mut s, 4.0);
    auto(&mut s, h, source);
    s.execute("copyclip", &json!({ "handles": [h.hex()] })).unwrap();
    let mut other = Session::new();
    linear(&mut other);
    let unrelated = line(&mut other, 7.0);
    assert_eq!(unrelated, source, "independent drawings reuse handle numbers");
    let b = s.open_drawing((*other.doc().unwrap()).clone(), "other", None);
    s.active = b;
    let r = s.execute("pasteorig", &json!({})).unwrap();
    let pasted = Handle::parse_hex(r["handles"][0].as_str().unwrap()).unwrap();
    assert!(dim(&s, pasted).marks.auto_break.is_none());
    let before = dim(&s, pasted).marks.breaks.clone();
    move_by(&mut s, unrelated, 1.0, 0.0);
    assert_eq!(dim(&s, pasted).marks.breaks, before);
}

#[test]
fn default_auto_follows_existing_objects_and_remove_clears_an_invisible_relationship() {
    let mut s = Session::new();
    let h = linear(&mut s);
    let source = line(&mut s, 24.0);
    s.execute("dimbreak", &json!({ "handles": [h.hex()] })).unwrap();
    assert!(dim(&s, h).marks.breaks.is_empty());
    move_by(&mut s, source, -20.0, 0.0);
    assert_gap(&s, h, 4.0, 3.0);
    let unrelated = line(&mut s, 8.0);
    move_by(&mut s, unrelated, -1.0, 0.0);
    assert_gap(&s, h, 4.0, 3.0);
    move_by(&mut s, source, 20.0, 0.0);
    assert!(dim(&s, h).marks.breaks.is_empty());
    assert_eq!(s.execute("dimbreak", &json!({ "handles": [h.hex()], "mode": "remove" })).unwrap()["changed"], 1);
    move_by(&mut s, source, -20.0, 0.0);
    assert!(dim(&s, h).marks.breaks.is_empty());
}

#[test]
fn extension_line_breaks_follow_the_crossing_object() {
    let mut s = Session::new();
    let h = linear(&mut s);
    s.execute("line", &json!({ "points": [[8, 1], [12, 1]] })).unwrap();
    let source = s.doc().unwrap().model.last().unwrap().handle;
    auto(&mut s, h, source);
    assert_eq!(dim(&s, h).marks.breaks[0].at, Vec3::new(10.0, 1.0, 0.0));
    move_by(&mut s, source, 0.0, 1.0);
    assert_eq!(dim(&s, h).marks.breaks[0].at, Vec3::new(10.0, 2.0, 0.0));
    let g = cadcraft_engine::render::dimension_in(s.doc().unwrap(), dim(&s, h));
    assert!(!g.lines_of(LineRole::Ext).any(|l| {
        l.windows(2).any(|w| (w[0].x - 10.0).abs() < 1e-9 && (w[1].x - 10.0).abs() < 1e-9 && w[0].y.min(w[1].y) < 2.0 && 2.0 < w[0].y.max(w[1].y))
    }));
}

#[test]
fn model_and_paper_dimensions_only_follow_objects_in_their_own_space() {
    let mut s = Session::new();
    let model_dim = linear(&mut s);
    let model_line = line(&mut s, 4.0);
    s.state_mut().unwrap().space = Space::Paper("Layout1".into());
    let paper_dim = linear(&mut s);
    let paper_line = line(&mut s, 8.0);
    s.execute("dimbreak", &json!({ "handles": [model_dim.hex(), paper_dim.hex()] })).unwrap();
    assert_gap(&s, model_dim, 4.0, 3.0);
    assert_gap(&s, paper_dim, 8.0, 3.0);
    move_by(&mut s, paper_line, -2.0, 0.0);
    assert_gap(&s, paper_dim, 6.0, 3.0);
    assert_gap(&s, model_dim, 4.0, 3.0);
    s.state_mut().unwrap().space = Space::Model;
    move_by(&mut s, model_line, -1.0, 0.0);
    assert_gap(&s, model_dim, 3.0, 3.0);
    assert_gap(&s, paper_dim, 6.0, 3.0);
}

#[test]
fn too_many_sources_are_refused_without_changing_the_existing_breaks() {
    let mut s = Session::new();
    let h = linear(&mut s);
    let source = line(&mut s, 4.0);
    auto(&mut s, h, source);
    let d = s.doc_mut().unwrap();
    let template = (**d.entity(source).unwrap()).clone();
    for _ in 0..cadcraft_engine::doc::MAX_DIM_BREAK_OBJECTS {
        let mut e = template.clone();
        e.handle = d.new_handle();
        d.model.push(e);
    }
    let before = s.state().unwrap().doc.clone();
    let err = s.execute("dimbreak", &json!({ "handles": [h.hex()] })).unwrap_err();
    assert!(err.to_string().contains("source objects"), "{err}");
    assert!(Arc::ptr_eq(&before, &s.state().unwrap().doc));
    assert_gap(&s, h, 4.0, 3.0);
    s.execute("dimbreak", &json!({ "handles": [h.hex()], "objects": [source.hex()] })).unwrap();
    assert_gap(&s, h, 4.0, 3.0);
}

#[test]
fn sources_in_other_spaces_and_legacy_gaps_do_not_get_followed() {
    let mut s = Session::new();
    let h = linear(&mut s);
    let source = line(&mut s, 4.0);
    auto(&mut s, h, source);
    let d = s.doc_mut().unwrap();
    let e = d.model.remove(source).unwrap();
    d.layouts.first_mut().unwrap().entities.push((*e).clone());
    // A change to the dimension forces refresh after the source left its space.
    move_by(&mut s, h, 0.0, 1.0);
    assert!(dim(&s, h).marks.breaks.is_empty());
    assert!(s.doc().unwrap().space(&Space::Model).unwrap().get(source).is_none());
    s.doc_mut()
        .unwrap()
        .modify_entity(h, |e| {
            if let EntityKind::Dimension(dm) = &mut e.kind {
                dm.marks.auto_break = None;
                dm.marks.breaks = vec![DimBreak { at: Vec3::new(4.0, 4.0, 0.0), size: 0.5, auto: true }];
            }
        })
        .unwrap();
    // Old files have auto-marked points but no relationship. Keep those points as before.
    s.execute("setvar", &json!({ "name": "DIMSCALE", "value": 2 })).unwrap();
    assert_eq!(dim(&s, h).marks.breaks.len(), 1);
}

#[test]
fn array_and_explode_remap_auto_break_handles() {
    let mut s = Session::new();
    let h = linear(&mut s);
    let source = line(&mut s, 4.0);
    auto(&mut s, h, source);
    assert_gap(&s, h, 4.0, 3.0);

    // Stage 1: Make an associative array: 1 row, 2 cols, colSpacing 20.
    let arr = s.execute("arrayrect", &json!({ "handles": [h.hex(), source.hex()], "rows": 1, "cols": 2, "colSpacing": 20.0 })).unwrap();
    assert_eq!(arr["associative"], true);
    let arr_h = Handle::parse_hex(arr["handle"].as_str().unwrap()).unwrap();

    // Verify stage 1: in the source block, the dimension's auto_break object was remapped
    // to the cloned line handle inside the source block.
    let src_line_handle = {
        let d = s.doc().unwrap();
        let p = d.array_params(d.entity(arr_h).unwrap()).unwrap();
        let src_blk = d.block(&p.source).unwrap();
        let src_line = src_blk.entities.iter().find(|e| matches!(e.kind, EntityKind::Line(_))).unwrap();
        let src_dim = src_blk
            .entities
            .iter()
            .find_map(|e| match &e.kind {
                EntityKind::Dimension(dm) => Some(dm),
                _ => None,
            })
            .unwrap();
        assert_ne!(src_line.handle, source);
        assert_eq!(src_dim.marks.auto_break.as_ref().unwrap().objects, vec![src_line.handle]);
        src_line.handle
    };

    // Stage 2: Explode the array into top-level entities.
    let exp = s.execute("explode", &json!({ "handles": [arr_h.hex()] })).unwrap();
    let new_hs: Vec<Handle> = exp["handles"].as_array().unwrap().iter().map(|v| Handle::parse_hex(v.as_str().unwrap()).unwrap()).collect();
    assert_eq!(new_hs.len(), 4); // 2 items * 2 entities

    // Filter into dimensions and lines in model space.
    let (dim_hs, line_hs) = {
        let d = s.doc().unwrap();
        let dims: Vec<Handle> = new_hs.iter().copied().filter(|h| matches!(&d.entity(*h).unwrap().kind, EntityKind::Dimension(_))).collect();
        let lines: Vec<Handle> = new_hs.iter().copied().filter(|h| matches!(&d.entity(*h).unwrap().kind, EntityKind::Line(_))).collect();
        (dims, lines)
    };
    assert_eq!(dim_hs.len(), 2);
    assert_eq!(line_hs.len(), 2);

    for (dim_h, line_h) in dim_hs.iter().zip(line_hs.iter()) {
        let dm = dim(&s, *dim_h);
        assert_ne!(dm.marks.auto_break.as_ref().unwrap().objects, vec![source]);
        assert_ne!(dm.marks.auto_break.as_ref().unwrap().objects, vec![src_line_handle]);
        assert_eq!(dm.marks.auto_break.as_ref().unwrap().objects, vec![*line_h]);
    }

    // Both exploded dimensions have their gaps active.
    let gap0_x = dim(&s, dim_hs[0]).marks.breaks[0].at.x;
    let gap1_x = dim(&s, dim_hs[1]).marks.breaks[0].at.x;
    assert!((gap0_x - 4.0).abs() < 1e-9 || (gap0_x - 24.0).abs() < 1e-9);
    assert!((gap1_x - 4.0).abs() < 1e-9 || (gap1_x - 24.0).abs() < 1e-9);
    assert!((gap0_x - gap1_x).abs() > 10.0);
}
