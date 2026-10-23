//! A drawing saved with a view opens there: the interface doesn't fit it to the drawing area.

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::{Drawing, VportTile};
use cadcraft_engine::geom::Vec2;
use cadcraft_ui_egui::{CadApp, Services};

#[test]
fn a_saved_view_is_not_replaced_by_zoom_extents() {
    // "view" files carry a saved model view; others don't.
    set_io(IoHooks {
        read: |_, name| {
            let mut d = Drawing::new_metric();
            if name.starts_with("view") {
                d.model_view =
                    Some(VportTile { min: Vec2::ZERO, max: Vec2::new(1.0, 1.0), center: Vec2::new(40.0, 30.0), height: 12.0, aspect: 1.5 });
            }
            Ok(d)
        },
        write: |_, _| Ok(Vec::new()),
        plot: None,
    });
    let mut app = CadApp::new(Session::empty(), Services::default());
    app.open_bytes("view.dxf", b"0\nSECTION\n");
    assert!(!app.canvas.zoom_pending, "opens at its saved view");
    let v = app.session.state().unwrap().view();
    assert_eq!((v.center, v.height), (Vec2::new(40.0, 30.0), 12.0));
    app.open_bytes("plain.dxf", b"0\nSECTION\n");
    assert!(app.canvas.zoom_pending, "without a saved view: fitted to the drawing area");
}
