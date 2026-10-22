//! The OS cursor is hidden only over the drawing area (#1701): over the menu bar, a menu and the
//! docked palettes the normal pointer comes back, so the user can see what they are pointing at.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services};
use egui::accesskit::Role;
use egui::vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

fn harness() -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.ui.in_window_menu = true;
    let mut harness = Harness::builder().with_size(vec2(1280.0, 760.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    harness.run_steps(5);
    harness
}

/// The cursor icon the app asks the platform for in the last frame.
fn cursor(harness: &Harness<'_, CadApp>) -> egui::CursorIcon {
    harness.output().platform_output.cursor_icon
}

fn hover_canvas(harness: &mut Harness<'_, CadApp>) {
    harness.hover_at(egui::pos2(640.0, 400.0));
    harness.run_steps(3);
}

#[test]
fn the_menu_bar_brings_the_os_cursor_back() {
    let mut harness = harness();
    hover_canvas(&mut harness);
    assert_eq!(cursor(&harness), egui::CursorIcon::None, "the drawn crosshair replaces the OS cursor on the canvas");

    // Pointing at a menu title: the crosshair (and the hidden OS cursor) must not leak out of the
    // drawing area.
    harness.get_by_role_and_label(Role::Button, "File").hover();
    harness.run_steps(3);
    assert_ne!(cursor(&harness), egui::CursorIcon::None, "the menu bar keeps a visible OS cursor");
    assert_eq!(cursor(&harness), egui::CursorIcon::Default);
}

#[test]
fn an_open_menu_keeps_the_os_cursor_visible_over_the_drawing() {
    let mut harness = harness();
    let title = harness.get_by_role_and_label(Role::Button, "View");
    title.click();
    harness.run_steps(3);

    // A menu item of the open popup lies over the drawing area.
    harness.get_by_label("Regen All").hover();
    harness.run_steps(3);
    assert_ne!(cursor(&harness), egui::CursorIcon::None, "a menu item over the canvas shows the pointer");

    // The menu's frame (between its items and its border) is over the canvas too, but it is no
    // menu item: the drawing's crosshair must stay under it rather than replace the OS cursor.
    let popup = harness
        .ctx
        .memory(|m| {
            m.areas()
                .visible_layer_ids()
                .into_iter()
                .filter(|layer| layer.order != egui::Order::Background)
                .filter_map(|layer| m.area_rect(layer.id).map(|rect| (layer, rect)))
                .max_by(|(_, a), (_, b)| (a.width() * a.height()).total_cmp(&(b.width() * b.height())))
        })
        .expect("the open menu is an area");
    let frame = egui::pos2(popup.1.right() - 2.0, popup.1.bottom() - 2.0);
    assert!(harness.state().canvas.rect.is_some_and(|canvas| canvas.contains(frame)), "the menu's frame overlaps the drawing area");
    assert_eq!(harness.ctx.layer_id_at(frame), Some(popup.0), "the frame point is over the menu, not the drawing");
    harness.hover_at(frame);
    harness.run_steps(3);
    assert_ne!(cursor(&harness), egui::CursorIcon::None, "the menu's frame keeps the OS cursor visible");
}

#[test]
fn docked_palettes_keep_the_os_cursor_visible() {
    let mut harness = harness();
    hover_canvas(&mut harness);
    assert_eq!(cursor(&harness), egui::CursorIcon::None);

    // The Tool Sets palette docked left of the drawing.
    harness.hover_at(egui::pos2(100.0, 300.0));
    harness.run_steps(3);
    assert_ne!(cursor(&harness), egui::CursorIcon::None, "moving to a palette restores the OS cursor");
}

#[test]
fn show_system_cursor_restores_the_arrow_off_the_canvas() {
    let mut harness = harness();
    harness.state_mut().ui.system_cursor = true;
    hover_canvas(&mut harness);
    assert_eq!(cursor(&harness), egui::CursorIcon::Crosshair);

    harness.get_by_role_and_label(Role::Button, "File").hover();
    harness.run_steps(3);
    assert_eq!(cursor(&harness), egui::CursorIcon::Default, "not the drafting crosshair over the menu bar");
}
