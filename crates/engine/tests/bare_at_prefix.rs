use cadcraft_engine::prompt::parse_point_input;
use cadcraft_engine::PointInput;
use cadcraft_geom::Vec2;

#[test]
fn bare_at_is_a_relative_zero_offset() {
    let last = Vec2::new(3.0, 4.0);
    let pi = parse_point_input("@", last, &Default::default()).expect("@ alone must parse");
    match pi {
        PointInput::RelativeCartesian { at, delta } => {
            assert_eq!(at, last, "@ alone must be relative to the last point");
            assert_eq!(delta, Vec2::ZERO, "@ alone must be a zero offset");
        }
        other => panic!("@ alone must stay relative, got {other:?}"),
    }
}

#[test]
fn at_star_is_also_a_relative_zero_offset() {
    // `@*` and `*@` are the international forms; the `*` prefix marks a keyword elsewhere.
    let last = Vec2::new(-1.0, 2.0);
    let pi = parse_point_input("@*", last, &Default::default()).expect("@* must parse");
    assert!(matches!(pi, PointInput::RelativeCartesian { delta, .. } if delta == Vec2::ZERO));
}

#[test]
fn at_with_numbers_is_still_a_real_offset() {
    let last = Vec2::new(1.0, 1.0);
    let pi = parse_point_input("@2,3", last, &Default::default()).unwrap();
    assert_eq!(pi.point(), Vec2::new(3.0, 4.0));
    assert!(matches!(pi, PointInput::RelativeCartesian { delta, .. } if delta == Vec2::new(2.0, 3.0)));
}

#[test]
fn hash_forces_absolute() {
    let last = Vec2::new(9.0, 9.0);
    let pi = parse_point_input("#2,3", last, &Default::default()).unwrap();
    assert!(matches!(pi, PointInput::Absolute(_)), "# must force absolute");
    assert_eq!(pi.point(), Vec2::new(2.0, 3.0));
}
