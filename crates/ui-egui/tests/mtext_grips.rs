//! MTEXT frame grips support press-drag-release without changing character size.
use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle};
use cadcraft_engine::geom::Vec2;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, Modifiers, PointerButton, vec2};
use egui_kittest::Harness;
use serde_json::json;

fn frame_harness() -> (Harness<'static, CadApp>, Handle) {
    let mut app = CadApp::new(Session::new(), Services::default());
    let result = app.run("mtext", json!({"at": [0, 5], "text": "AAAA AAAA AAAA", "height": 1, "width": 5})).unwrap();
    let handle = Handle::parse_hex(result["handle"].as_str().unwrap()).unwrap();
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    h.state_mut().session.execute("view.set", &json!({"center": [2, 4], "height": 20})).unwrap();
    h.state_mut().canvas.zoom_pending = false;
    h.run_steps(3);
    (h, handle)
}

#[test]
fn dragging_mtext_frame_grips_changes_width_and_column_height() {
    for (index, to) in [(1, Vec2::new(8.0, 5.0)), (2, Vec2::new(2.5, 3.0))] {
        let (mut h, m) = frame_harness();
        {
            let s = &mut h.state_mut().session;
            s.settings.osmode = 0;
            s.settings.orthomode = false;
            s.settings.polarmode = false;
            s.settings.otrack = false;
            s.settings.snapmode = false;
            s.settings.dynmode = false;
            s.set_selection(vec![m]);
        }
        h.run_steps(3);
        let from = h.state().session.grips_of(m).unwrap()[index];
        let xf = h.state().canvas.xf.unwrap();
        let start = xf.to_screen(from);
        let end = xf.to_screen(to);
        h.event(Event::PointerMoved(start));
        h.run_steps(2);
        h.event(Event::PointerButton { pos: start, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
        h.run_steps(2);
        h.event(Event::PointerMoved(end));
        h.run_steps(2);
        assert_eq!(h.state().session.running.as_ref().map(|r| r.id.as_str()), Some("grip"));
        h.event(Event::PointerButton { pos: end, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
        h.run_steps(3);
        let EntityKind::MText(t) = &h.state().session.doc().unwrap().entity(m).unwrap().kind else { panic!("not MTEXT") };
        assert_eq!(t.height, 1.0);
        if index == 1 {
            assert!((t.width - 8.0).abs() < 1e-6);
        } else {
            assert!((t.columns.as_ref().unwrap().height - 2.0).abs() < 1e-6);
        }
        assert!(h.state().session.running.is_none());
    }
}
