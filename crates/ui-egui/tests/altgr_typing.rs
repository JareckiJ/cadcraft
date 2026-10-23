//! AltGr typing (#1551). On a Norwegian layout AltGr+2 types `@`, which starts relative points
//! such as `@10,0`. Each test feeds the events a real AltGr press produces, through the input
//! steps the web shell runs (`apps/cadcraft-web`), and reads the command line and the drawing.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Space};
use cadcraft_engine::geom::Vec2;
use cadcraft_ui_egui::altgr::{self, Typed};
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, Key, Modifiers};

/// AltGr as Windows reports it, and browsers on Windows too: Ctrl and Alt held.
const ALTGR: Modifiers = Modifiers { alt: true, ctrl: true, shift: false, mac_cmd: false, command: true };
const CTRL: Modifiers = Modifiers { alt: false, ctrl: true, shift: false, mac_cmd: false, command: true };

struct Shell {
    app: CadApp,
    ctx: egui::Context,
}

impl Shell {
    fn new() -> Self {
        let ctx = egui::Context::default();
        ctx.set_os(egui::os::OperatingSystem::Windows);
        let mut shell = Shell { app: CadApp::new(Session::new(), Services::default()), ctx };
        for _ in 0..3 {
            shell.frame(Vec::new(), Vec::new());
        }
        assert!(shell.app.canvas.rect.is_some());
        shell
    }

    /// One frame: `typed` is what the web shell's key listener heard since the last one.
    fn frame(&mut self, events: Vec<Event>, typed: Vec<Typed>) {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
        let mut raw = egui::RawInput { screen_rect: Some(screen), events, ..Default::default() };
        altgr::drop_key_names(&self.ctx, &mut raw);
        altgr::restore_typed(&self.ctx, &mut raw, typed);
        self.app.raw_input_hook(&mut raw);
        cadcraft_ui_egui::cmdline::capture_tab(&self.ctx, &mut raw);
        let app = &mut self.app;
        let mut out = self.ctx.run_ui(raw, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        out.textures_delta.clear();
        assert!(!self.ctx.egui_wants_keyboard_input(), "a text field must not hold focus");
    }

    fn type_text(&mut self, text: &str) {
        self.frame(chars(text), Vec::new());
    }

    fn key(&mut self, key: Key, modifiers: Modifiers) {
        self.frame(vec![press(key, modifiers), release(key, modifiers)], Vec::new());
    }

    fn lines(&self) -> Vec<(Vec2, Vec2)> {
        let st = self.app.session.state().unwrap();
        let model = st.doc.space(&Space::Model).unwrap();
        model
            .iter()
            .filter_map(|e| match &e.kind {
                EntityKind::Line(l) => Some((l.a.xy(), l.b.xy())),
                _ => None,
            })
            .collect()
    }
}

fn press(key: Key, modifiers: Modifiers) -> Event {
    Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers }
}

fn release(key: Key, modifiers: Modifiers) -> Event {
    Event::Key { key, physical_key: Some(key), pressed: false, repeat: false, modifiers }
}

fn chars(text: &str) -> Vec<Event> {
    text.chars().map(|c| Event::Text(c.to_string())).collect()
}

/// How AltGr+`key` typing `ch` reaches the app. egui has no key for `@` or `€`, so the key press
/// carries the physical key.
fn altgr_paths(key: Key, ch: &str) -> Vec<(&'static str, Vec<Event>, Vec<Typed>)> {
    let text = Event::Text(ch.into());
    vec![
        // winit leaves Ctrl and Alt out of AltGr's modifiers.
        ("desktop", vec![press(key, Modifiers::NONE), text.clone(), release(key, Modifiers::NONE)], vec![]),
        // The character arrives with Ctrl and Alt held.
        ("text with Ctrl+Alt", vec![press(key, ALTGR), text.clone(), release(key, ALTGR)], vec![]),
        // eframe's web runner drops the text; the shell's listener heard it.
        ("browser on Windows", vec![press(key, ALTGR), release(key, ALTGR)], vec![Typed { physical: Some(key), text: ch.into() }]),
        // AltGr alone arrives as the text "AltGraph"; its character then comes without modifiers.
        ("browser on Linux", vec![Event::Text("AltGraph".into()), text.clone(), press(key, Modifiers::NONE), release(key, Modifiers::NONE)], vec![]),
    ]
}

#[test]
fn altgr_at_sign_types_a_relative_point() {
    for (path, events, typed) in altgr_paths(Key::Num2, "@") {
        let mut s = Shell::new();
        s.type_text("line ");
        assert!(s.app.session.running.as_ref().is_some_and(|r| r.id == "line"), "{path}");
        s.type_text("0,0 ");
        // AltGr+2 and the rest of the point in one frame: the `@` must come first.
        let mut frame = events;
        frame.extend(chars("10,0"));
        s.frame(frame, typed);
        assert_eq!(s.app.cmd.buffer, "@10,0", "{path}");
        s.type_text(" ");
        s.key(Key::Enter, Modifiers::NONE);
        assert!(s.app.session.running.is_none(), "{path}");
        assert_eq!(s.lines(), vec![(Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0))], "{path}");
    }
}

#[test]
fn altgr_types_instead_of_running_the_ctrl_shortcut_on_its_key() {
    // AltGr+E types `€` (Ctrl+E is ISOPLANE); AltGr+3 types `£` on a Norwegian layout (Ctrl+3
    // shows the Tool Sets).
    for (key, ch) in [(Key::E, "€"), (Key::Num3, "£")] {
        for (path, events, typed) in altgr_paths(key, ch) {
            let mut s = Shell::new();
            let plane = s.app.session.settings.isoplane;
            let toolsets = s.app.ui.show_toolsets;
            s.frame(events, typed);
            assert_eq!(s.app.cmd.buffer, ch, "{ch} {path}");
            assert_eq!(s.app.session.settings.isoplane, plane, "{ch} {path}");
            assert_eq!(s.app.ui.show_toolsets, toolsets, "{ch} {path}");
        }
    }
}

#[test]
fn plain_ctrl_shortcuts_still_run() {
    let mut s = Shell::new();
    let plane = s.app.session.settings.isoplane;
    s.key(Key::E, CTRL);
    assert_ne!(s.app.session.settings.isoplane, plane, "Ctrl+E switches the isoplane");
    let toolsets = s.app.ui.show_toolsets;
    s.key(Key::Num3, CTRL);
    assert_ne!(s.app.ui.show_toolsets, toolsets, "Ctrl+3 toggles the Tool Sets");
    assert_eq!(s.app.cmd.buffer, "");
}
