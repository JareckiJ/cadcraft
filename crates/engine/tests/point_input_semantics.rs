//! Semantic behavior of `PointInput`: the variants must reach the commands as
//! the user meant them, not collapsed into a bare coordinate.

use cadcraft_doc::{EntityKind, Space};
use cadcraft_engine::{Input, PointInput, Session};
use cadcraft_geom::Vec2;
use std::f64::consts::FRAC_PI_4;

/// The `b` of the last line drawn in model space.
fn last_line_b(s: &Session) -> Vec2 {
    let doc = s.doc().unwrap();
    let store = doc.space(&Space::Model).expect("no model space");
    let line = store
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Line(l) => Some(l),
            _ => None,
        })
        .last()
        .expect("no line drawn");
    Vec2::new(line.b.x, line.b.y)
}

fn draw_to(s: &mut Session, from: Vec2, to: PointInput) {
    s.cmdline("line").unwrap();
    s.input(Input::Point(PointInput::Absolute(from))).unwrap();
    s.input(Input::Point(to)).unwrap();
    s.input(Input::Enter).unwrap();
}

#[test]
fn relative_polar_reaches_the_command_as_its_point() {
    let mut s = Session::new();
    draw_to(&mut s, Vec2::ZERO, PointInput::RelativePolar { at: Vec2::ZERO, dist: 10.0, angle: FRAC_PI_4 });
    let expected = Vec2::new(10.0 * FRAC_PI_4.cos(), 10.0 * FRAC_PI_4.sin());
    assert!(last_line_b(&s).near(expected, 1e-9), "RelativePolar is not measured from its base");
}

#[test]
fn relative_polar_is_measured_from_its_own_base() {
    let mut s = Session::new();
    let base = Vec2::new(5.0, 7.0);
    draw_to(&mut s, base, PointInput::RelativePolar { at: base, dist: 4.0, angle: 0.0 });
    assert!(last_line_b(&s).near(Vec2::new(9.0, 7.0), 1e-9), "RelativePolar ignored its own base");
}

#[test]
fn direct_distance_reaches_the_command_as_its_point() {
    let mut s = Session::new();
    draw_to(&mut s, Vec2::ZERO, PointInput::DirectDistance { base: Vec2::ZERO, dist: 5.0, dir: Vec2::Y });
    assert!(last_line_b(&s).near(Vec2::new(0.0, 5.0), 1e-9), "DirectDistance is not measured along its direction");
}

#[test]
fn absolute_and_picked_land_where_they_say() {
    let mut s = Session::new();
    draw_to(&mut s, Vec2::ZERO, PointInput::Absolute(Vec2::new(2.0, 3.0)));
    assert!(last_line_b(&s).near(Vec2::new(2.0, 3.0), 1e-9));

    draw_to(&mut s, Vec2::ZERO, PointInput::Picked(Vec2::new(-4.0, 1.0)));
    assert!(last_line_b(&s).near(Vec2::new(-4.0, 1.0), 1e-9));
}

#[test]
fn from_vec2_is_absolute_not_a_pick() {
    // The UI passes PointInput::Picked explicitly; scripts, the API and tests hand over
    // coordinates, which are absolute points, not clicks on the canvas.
    let p = Vec2::new(3.0, 4.0);
    let pi: PointInput = p.into();
    assert!(matches!(pi, PointInput::Absolute(_)), "From<Vec2> must not be a pick");
    assert_eq!(pi.point(), p);
}
