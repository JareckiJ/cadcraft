//! A drawing saved where the user zoomed opens there again: the view on screen goes into the file.

use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::{Session, View};
use cadcraft_geom::Vec2;
use serde_json::json;

fn hooks() {
    set_io(IoHooks {
        read: |bytes, name| cadcraft_io::read(bytes, name).map_err(|e| e.to_string()),
        write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
        plot: None,
    });
}

#[test]
fn the_view_on_screen_is_where_the_drawing_opens_again() {
    for name in ["plan.dxf", "plan.dwg"] {
        opens_where_saved(name);
    }
}

fn opens_where_saved(name: &str) {
    hooks();
    let dir = std::env::temp_dir().join(format!("cadcraft-save-view-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name).to_string_lossy().into_owned();
    let mut s = Session::new();
    s.execute("line", &json!({ "points": [[0, 0], [1000, 600]] })).unwrap();
    s.execute("line", &json!({ "points": [[5000, 5000], [5100, 5060]] })).unwrap();
    // Zoomed in on one corner.
    let zoomed = View { center: Vec2::new(40.0, 30.0), height: 12.0 };
    assert!(s.state_mut().unwrap().set_view(zoomed));
    s.execute("saveas", &json!({ "path": path })).unwrap();
    assert!(!s.state().unwrap().is_dirty(), "keeping the view is no change");
    let bytes = std::fs::read(&path).unwrap();
    // In DXF the header's view goes along, for readers that don't read the VPORT table (a DWG has
    // no such header variables: there the *Active VPORT is the view).
    if name.ends_with(".dxf") {
        let saved = cadcraft_io::read(&bytes, name).unwrap();
        assert!(saved.header.point("VIEWCTR").is_some_and(|c| c.xy().near(zoomed.center, 1e-6)), "{name}: $VIEWCTR");
        assert!(saved.header.f64("VIEWSIZE", 0.0) == zoomed.height, "{name}: $VIEWSIZE");
    }
    let mut t = Session::empty();
    t.open_drawing(cadcraft_io::read(&bytes, name).unwrap(), name, Some(path.clone()));
    let v = t.state().unwrap().view();
    assert!(v.center.near(zoomed.center, 1e-6) && (v.height - zoomed.height).abs() < 1e-6, "{name}: opened at {v:?}");
}

/// With a real drawing: `CADCRAFT_VIEW_PROBE=<file.dwg> cargo test --test save_keeps_view -- --ignored`.
/// Prints only whether the view came back.
#[test]
#[ignore]
fn a_real_drawing_opens_where_saved() {
    let Ok(src) = std::env::var("CADCRAFT_VIEW_PROBE") else { return };
    hooks();
    let mut s = Session::empty();
    let d = cadcraft_io::read(&std::fs::read(&src).unwrap(), &src).unwrap();
    s.open_drawing(d, "probe.dwg", None);
    let v0 = s.state().unwrap().view();
    // A tenth of the opening view, off its centre.
    let zoomed = View { center: v0.center + Vec2::new(v0.height * 0.1, v0.height * 0.05), height: v0.height * 0.1 };
    assert!(s.state_mut().unwrap().set_view(zoomed));
    let dir = std::env::temp_dir().join(format!("cadcraft-view-probe-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for name in ["probe.dwg", "probe.dxf"] {
        let path = dir.join(name).to_string_lossy().into_owned();
        s.execute("saveas", &json!({ "path": path })).unwrap();
        let mut t = Session::empty();
        t.open_drawing(cadcraft_io::read(&std::fs::read(&path).unwrap(), name).unwrap(), name, None);
        let v = t.state().unwrap().view();
        let ok = v.center.near(zoomed.center, zoomed.height * 1e-6) && (v.height - zoomed.height).abs() < zoomed.height * 1e-6;
        println!("{name}: view back {ok}");
        assert!(ok, "{name}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_saved_view_without_header_variables_gets_them() {
    hooks();
    let dir = std::env::temp_dir().join(format!("cadcraft-save-view-header-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("plan.dxf").to_string_lossy().into_owned();
    let mut s = Session::new();
    s.execute("line", &json!({ "points": [[0, 0], [100, 60]] })).unwrap();
    let zoomed = View { center: Vec2::new(40.0, 30.0), height: 12.0 };
    assert!(s.state_mut().unwrap().set_view(zoomed));
    s.execute("saveas", &json!({ "path": path })).unwrap();
    // The view is in the drawing already, the header variables are not (a file from elsewhere).
    let d = std::sync::Arc::make_mut(&mut s.state_mut().unwrap().doc);
    d.header.vars.remove("VIEWCTR");
    d.header.vars.remove("VIEWSIZE");
    s.execute("saveas", &json!({ "path": path })).unwrap();
    let saved = cadcraft_io::read(&std::fs::read(&path).unwrap(), "plan.dxf").unwrap();
    assert!(saved.header.point("VIEWCTR").is_some_and(|c| c.xy().near(zoomed.center, 1e-6)), "$VIEWCTR written again");
    assert!(saved.header.f64("VIEWSIZE", 0.0) == zoomed.height, "$VIEWSIZE written again");
    let _ = std::fs::remove_dir_all(&dir);
}
