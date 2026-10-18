//! MTEXT frame editing changes wrapping/column height, not character size.
use cadcraft_engine::doc::{EntityKind, Handle, MText};
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::grips::{GripMode, stretch_entity_grip};
use cadcraft_engine::{Input, Session};
use serde_json::json;

fn setup() -> (Session, Handle) {
    let mut s = Session::new();
    let r = s.execute("mtext", &json!({"at":[0,5],"text":"AAAA AAAA AAAA","height":1,"width":5})).unwrap();
    let h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    (s, h)
}
fn text(s: &Session, h: Handle) -> MText {
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::MText(t) => t.clone(),
        _ => panic!("not MTEXT"),
    }
}
#[test]
fn width_wraps_and_insertion_grip_still_moves() {
    let (mut s, h) = setup();
    assert_eq!(s.grips_of(h).unwrap().len(), 3);
    let before = text(&s, h);
    let old_lines = cadcraft_render::layout_mtext_entity(s.doc().unwrap(), &before).lines.len();
    s.grip_edit(h, 1, Vec2::new(9.0, 50.0), GripMode::Stretch).unwrap();
    let wide = text(&s, h);
    assert_eq!(wide.width, 9.0);
    assert_eq!(wide.height, before.height);
    assert_eq!(wide.contents, before.contents);
    assert_eq!(wide.insert, before.insert);
    assert!(cadcraft_render::layout_mtext_entity(s.doc().unwrap(), &wide).lines.len() < old_lines);
    s.undo().unwrap();
    assert_eq!(text(&s, h), before);
    s.redo().unwrap();
    assert_eq!(text(&s, h), wide);
    s.grip_edit(h, 0, Vec2::new(3.0, 7.0), GripMode::Stretch).unwrap();
    assert_eq!(text(&s, h).insert.xy(), Vec2::new(3.0, 7.0));
    assert_eq!(text(&s, h).width, 9.0);
}
#[test]
fn shorter_box_flows_into_columns_and_can_be_undone() {
    let (mut s, h) = setup();
    let before = text(&s, h);
    s.grip_edit(h, 2, Vec2::new(2.5, 3.0), GripMode::Stretch).unwrap();
    let short = text(&s, h);
    assert_eq!(short.height, 1.0);
    assert_eq!(short.contents, before.contents);
    assert_eq!(short.columns.as_ref().unwrap().height, 2.0);
    let layout = cadcraft_render::layout_mtext_entity(s.doc().unwrap(), &short);
    assert!(layout.bounds.width() > 5.0);
    assert_eq!(layout.lines.concat(), "AAAAAAAAAAAA");
    s.undo().unwrap();
    assert_eq!(text(&s, h), before);
    s.redo().unwrap();
    assert_eq!(text(&s, h), short);
}
#[test]
fn rotated_bottom_right_attachment_keeps_top_left_fixed() {
    let (mut s, h) = setup();
    s.doc_mut()
        .unwrap()
        .modify_entity(h, |e| {
            if let EntityKind::MText(t) = &mut e.kind {
                t.rotation = std::f64::consts::FRAC_PI_2;
                t.attach = 9;
            }
        })
        .unwrap();
    let rotation = text(&s, h).rotation;
    let top = s.grips_of(h).unwrap()[1] - Vec2::new(5.0, 0.0).rotate(rotation);
    s.grip_edit(h, 1, top + Vec2::new(8.0, 3.0).rotate(rotation), GripMode::Stretch).unwrap();
    assert!((text(&s, h).width - 8.0).abs() < 1e-9);
    let after_top = s.grips_of(h).unwrap()[1] - Vec2::new(8.0, 0.0).rotate(rotation);
    assert!(after_top.near(top, 1e-9));
}
#[test]
fn preview_cancel_and_hostile_sizes_leave_drawing_unchanged() {
    let (mut s, h) = setup();
    let before = text(&s, h);
    s.start_grip_edit(&[(h, 1)]).unwrap();
    assert!(s.preview(Vec2::new(9.0, 5.0)).iter().any(|e| matches!(&e.kind, EntityKind::MText(t) if t.width == 9.0)));
    assert_eq!(text(&s, h), before);
    s.cancel();
    assert_eq!(text(&s, h), before);
    for (i, p) in [
        (1, Vec2::new(-1.0, 5.0)),
        (2, Vec2::new(0.0, 6.0)),
        (1, Vec2::new(f64::NAN, 0.0)),
        (2, Vec2::new(0.0, f64::NEG_INFINITY)),
        (usize::MAX, Vec2::ZERO),
    ] {
        assert!(stretch_entity_grip(s.doc().unwrap(), &EntityKind::MText(before.clone()), i, p).is_none());
        assert_eq!(text(&s, h), before);
    }
    s.start_grip_edit(&[(h, 1)]).unwrap();
    s.input(Input::Point(Vec2::new(8.0, 5.0))).unwrap();
    assert_eq!(text(&s, h).width, 8.0);
    s.undo().unwrap();
    assert_eq!(text(&s, h), before);
}
