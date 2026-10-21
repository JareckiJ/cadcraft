//! Enter (or Space) on a partly typed command name runs the AutoComplete row shown as chosen,
//! as AutoCAD does; an exact name or alias runs as typed.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services, cmdline};

fn type_line(app: &mut CadApp, text: &str) {
    app.cmd.buffer = text.into();
    cmdline::submit(app);
}

fn running(app: &CadApp) -> Option<String> {
    app.session.running.as_ref().map(|r| r.id.clone())
}

#[test]
fn enter_on_a_partial_name_runs_the_first_suggestion() {
    let mut a = CadApp::new(Session::new(), Services::default());
    type_line(&mut a, "recta");
    assert_eq!(running(&a).as_deref(), Some("rectang"));
    a.session.cancel();
    // An exact alias runs as typed (L is LINE, not the first command starting with L).
    type_line(&mut a, "l");
    assert_eq!(running(&a).as_deref(), Some("line"));
    a.session.cancel();
    // A system variable name stays a system variable.
    type_line(&mut a, "orthomode");
    assert_eq!(running(&a).as_deref(), Some("setvar"));
    a.session.cancel();
    // Nothing matches: still reported as unknown.
    type_line(&mut a, "zzqx");
    assert_eq!(running(&a), None);
}

fn frame(app: &mut CadApp, ctx: &egui::Context) {
    let mut raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0))), ..Default::default() };
    app.raw_input_hook(&mut raw);
    let mut out = ctx.run_ui(raw, |ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    });
    // No renderer here: drop the texture updates deliberately.
    out.textures_delta.clear();
}

/// Types `text` and presses each of `keys` (an egui key name, or `" "` for Space, which egui
/// delivers as text), one frame each, then reports the command name the history recorded.
fn press(app: &mut CadApp, ctx: &egui::Context, text: &str, keys: &[&str]) -> Option<String> {
    app.synthetic.push(egui::Event::Text(text.into()));
    frame(app, ctx);
    for k in keys {
        let ev = match *k {
            " " => egui::Event::Text(" ".into()),
            name => egui::Event::Key {
                key: egui::Key::from_name(name).unwrap_or_else(|| panic!("no key {name}")),
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            },
        };
        app.synthetic.push(ev);
        frame(app, ctx);
    }
    let ran = app.cmd.history.last().cloned();
    app.session.cancel();
    frame(app, ctx);
    ran
}

#[test]
fn up_and_down_choose_the_suggestion_that_enter_or_space_runs() {
    let ctx = egui::Context::default();
    let mut a = CadApp::new(Session::new(), Services::default());
    for _ in 0..3 {
        frame(&mut a, &ctx);
    }
    // `dimc` has several suggestions (DIMCENTER, DIMCONTINUE…); `re` would not do: it is the alias of REGEN.
    let first = press(&mut a, &ctx, "dimc", &["Enter"]).expect("Enter runs the first suggestion");
    let second = press(&mut a, &ctx, "dimc", &["ArrowDown", "Enter"]).expect("Down then Enter runs a suggestion");
    // The history keeps the full name that ran, not the typed prefix.
    assert!(first.len() > "dimc".len() && second.len() > "dimc".len(), "{first} {second}");
    assert_ne!(first, second, "Down moves the choice to the second row");
    // Up brings the choice back to the first row; Space completes as Enter does.
    assert_eq!(press(&mut a, &ctx, "dimc", &["ArrowDown", "ArrowUp", " "]).as_deref(), Some(first.as_str()));
    assert_eq!(press(&mut a, &ctx, "dimc", &["ArrowDown", " "]).as_deref(), Some(second.as_str()));
    // Up at the first row stays there.
    assert_eq!(press(&mut a, &ctx, "dimc", &["ArrowUp", "Enter"]).as_deref(), Some(first.as_str()));
}
