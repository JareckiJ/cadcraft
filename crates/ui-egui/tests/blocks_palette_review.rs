//! Regression for a selection menu click arriving as INSERT starts (#2122).
use cadcraft_engine::Session;
use cadcraft_geom::Vec2;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use serde_json::json;

fn release(h: &mut Harness<'_, CadApp>, pos: Pos2) {
    h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
}

#[test]
fn selection_menu_click_cannot_place_a_block_when_insert_starts() {
    let mut session = Session::new();
    let c = session.execute("circle", &json!({"center": [0, 0], "radius": 2})).unwrap();
    session.execute("block", &json!({"name": "Pin", "base": [0, 0], "handles": [c["handle"]], "keep": "delete"})).unwrap();
    for end in [10, 20] {
        session.execute("line", &json!({"points": [[0, 0], [end, 0]]})).unwrap();
    }
    session.execute("setvar", &json!({"name": "SELECTIONCYCLING", "value": 2})).unwrap();
    session.set_selection(Vec::new());
    let mut app = CadApp::new(session, Services::default());
    app.canvas.zoom_pending = true;
    let shots = std::env::var_os("CADCRAFT_BLOCK_SHOTS").map(std::path::PathBuf::from);
    let builder = Harness::builder().with_size(vec2(1600.0, 1000.0));
    let builder = if shots.is_some() { builder.wgpu() } else { builder };
    let mut h = builder.build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    let at = h.state().canvas.xf.unwrap().to_screen(Vec2::new(5.0, 0.0));
    h.event(Event::PointerMoved(at));
    h.run_steps(1);
    release(&mut h, at);
    h.run_steps(2);
    let rows = h.state().canvas.cycle_rows();
    assert_eq!(rows.len(), 2);
    let row = rows[0].0.center();
    h.event(Event::PointerMoved(row));
    h.run_steps(2);
    // Start placement between frames, while the old selection list is still on screen.
    let st = cadcraft_ui_egui::blockmgr::State { repeat: true, ..Default::default() };
    cadcraft_ui_egui::blockmgr::grab_block(h.state_mut(), "Pin", &st);
    release(&mut h, row);
    h.run_steps(2);
    assert_eq!(cadcraft_ui_egui::blockmgr::reference_count(h.state().session.doc().unwrap(), "Pin"), 0);
    assert!(h.state().session.current_prompt().unwrap().accept.point);
    // A subsequent click on bare canvas still places the block.
    let clear = h.state().canvas.xf.unwrap().to_screen(Vec2::new(7.0, 3.0));
    h.event(Event::PointerMoved(clear));
    h.run_steps(1);
    release(&mut h, clear);
    h.run_steps(2);
    assert_eq!(cadcraft_ui_egui::blockmgr::reference_count(h.state().session.doc().unwrap(), "Pin"), 1);
    h.state_mut().session.cancel();
    h.state_mut().start("insert");
    h.run_steps(3);
    if let Some(dir) = shots {
        std::fs::create_dir_all(&dir).unwrap();
        h.render().unwrap().save(dir.join("blocks-palette.png")).unwrap();
    }
}
