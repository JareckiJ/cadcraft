use cadcraft_engine::Session;
use cadcraft_geom::Vec3;
use serde_json::json;

#[test]
fn screen_up_is_applied_before_fitting_the_cad_view() {
    let mut s = Session::new();
    s.execute("line", &json!({"points":[[0,0],[20,0],[20,2],[0,2]]})).unwrap();
    let document = s.state().unwrap().doc.clone();
    let undo = s.state().unwrap().undo.len();
    s.execute("vpoint", &json!({"direction":[0,0,1],"up":[-1,0,0]})).unwrap();
    assert!((s.state().unwrap().view().twist + std::f64::consts::FRAC_PI_2).abs() < 1e-6);
    assert!(s.state().unwrap().view().height > 20.0, "fit uses the final rotated extents");
    assert_eq!(document.model, s.state().unwrap().doc.model);
    assert_eq!(document.layers, s.state().unwrap().doc.layers);
    assert_eq!(s.state().unwrap().undo.len(), undo);
    s.execute("vpoint", &json!({"direction":[0,0,1],"up":[1e308,0,0]})).unwrap();
    assert!(s.state().unwrap().view().twist.is_finite(), "large finite vectors are normalized safely");
}

#[test]
fn invalid_up_vectors_preserve_the_complete_camera_and_drawing() {
    let mut s = Session::new();
    s.execute("vpoint", &json!({"preset":"seiso"})).unwrap();
    s.execute("dview", &json!({"twist":25})).unwrap();
    let view = s.state().unwrap().view();
    let direction = s.view_direction();
    let document = s.state().unwrap().doc.clone();
    let undo = s.state().unwrap().undo.len();
    for up in [json!([0, 0, 0]), json!([0, 0, 2]), json!([1, 2]), json!([null, 1, 0]), json!(null)] {
        assert!(s.execute("vpoint", &json!({"direction":[0,0,1],"up":up})).is_err());
        assert_eq!(s.state().unwrap().view(), view);
        assert_eq!(s.view_direction(), direction);
        assert_eq!(s.state().unwrap().doc.model, document.model);
        assert_eq!(s.state().unwrap().undo.len(), undo);
    }
}

#[test]
fn ucs_view_follows_the_camera_twist_and_preserves_its_origin() {
    let mut session = Session::new();
    session.execute("ucs.origin", &json!({"origin":[12,24]})).unwrap();
    session.execute("dview", &json!({"twist":30})).unwrap();
    let view = session.state().unwrap().view();
    session.execute("ucs.view", &json!({})).unwrap();
    let angle = 30f64.to_radians();
    assert!((session.ucs().x - Vec3::new(angle.cos(), -angle.sin(), 0.0)).len() < 1e-9);
    assert_eq!(session.ucs().origin, Vec3::new(12.0, 24.0, 0.0));
    assert_eq!(session.state().unwrap().view(), view);
    session.execute("vpoint", &json!({"preset":"front"})).unwrap();
    session.execute("ucs.view", &json!({})).unwrap();
    assert!((session.ucs().x - Vec3::new(1.0, 0.0, 0.0)).len() < 1e-9);
    assert!((session.ucs().y - Vec3::Z).len() < 1e-9);
    assert!(session.input_ucs().is_world(), "tilted input keeps the documented WCS fallback");
}

#[test]
fn invalid_camera_roll_leaves_the_view_unchanged() {
    let mut session = Session::new();
    let before = session.state().unwrap().doc.clone();
    let view = session.state().unwrap().view();
    for angle in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(session.roll_view(angle).is_err());
        assert!(std::sync::Arc::ptr_eq(&before, &session.state().unwrap().doc));
        assert_eq!(view, session.state().unwrap().view());
    }
}
