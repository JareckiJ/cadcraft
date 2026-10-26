//! Enter (or Space) on a partly typed command name runs the AutoComplete row shown as chosen,
//! as CAD command lines usually do; an exact name or alias runs as typed.

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

/// Types `text` and presses each of `keys` (an egui key name, or `" "` for Space and `"\t"` for
/// Tab, which reach the command line as text), one frame each, then reports the command name the history recorded,
/// checking that the engine started that command (not only that the history shows it).
fn press(app: &mut CadApp, ctx: &egui::Context, text: &str, keys: &[&str]) -> Option<String> {
    app.session.last_command = None;
    app.synthetic.push(egui::Event::Text(text.into()));
    frame(app, ctx);
    for k in keys {
        let ev = match *k {
            " " | "\t" => egui::Event::Text((*k).into()),
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
    if let Some(name) = &ran {
        let started = app.session.last_command.clone();
        let id = cadcraft_engine::cmd::resolve_alias(&name.to_ascii_lowercase());
        assert_eq!(started.as_deref(), Some(id.as_str()), "{name} is in the history, so it must have started");
    }
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
    // Up at the first row goes to the last one; Down there comes back to the first. Tab moves
    // down as Down does.
    let n = cmdline::suggestions_count("dimc");
    let mut downs = vec!["ArrowDown"; n - 1];
    downs.push("Enter");
    let last = press(&mut a, &ctx, "dimc", &downs);
    assert_eq!(press(&mut a, &ctx, "dimc", &["ArrowUp", "Enter"]), last);
    assert_eq!(press(&mut a, &ctx, "dimc", &["ArrowUp", "ArrowDown", "Enter"]).as_deref(), Some(first.as_str()));
    assert_eq!(press(&mut a, &ctx, "dimc", &["\t", "Enter"]).as_deref(), Some(second.as_str()));
}

#[test]
fn the_chosen_row_completes_the_typed_name_until_backspace() {
    let ctx = egui::Context::default();
    let mut a = CadApp::new(Session::new(), Services::default());
    for _ in 0..3 {
        frame(&mut a, &ctx);
    }
    let second = press(&mut a, &ctx, "dimc", &["ArrowDown", "Enter"]).expect("Down then Enter runs a suggestion");
    a.synthetic.push(egui::Event::Text("dimc".into()));
    frame(&mut a, &ctx);
    let shown = cmdline::completion(&a).expect("the first row completes the typed name");
    assert!(!second.ends_with(&shown), "{shown} completes the first row, not {second}");
    // Down: the completion follows the chosen row.
    let key = |k: egui::Key| egui::Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
    a.synthetic.push(key(egui::Key::ArrowDown));
    frame(&mut a, &ctx);
    let rest = cmdline::completion(&a).expect("the second row completes the typed name");
    assert!(second.ends_with(&rest), "{rest} completes {second}");
    // Backspace erases only the completion; the typed name and the chosen row stay.
    a.synthetic.push(key(egui::Key::Backspace));
    frame(&mut a, &ctx);
    assert_eq!(cmdline::completion(&a), None);
    assert_eq!(a.cmd.buffer, "dimc");
    // Tab shows a completion again (the next row's), Up the second row's again.
    a.synthetic.push(egui::Event::Text("\t".into()));
    frame(&mut a, &ctx);
    assert!(cmdline::completion(&a).is_some());
    assert_eq!(press(&mut a, &ctx, "", &["ArrowUp", "Backspace", "Enter"]).as_deref(), Some(second.as_str()));
}

#[test]
fn editing_a_recalled_command_completes_from_the_list_again() {
    let ctx = egui::Context::default();
    let mut a = CadApp::new(Session::new(), Services::default());
    for _ in 0..3 {
        frame(&mut a, &ctx);
    }
    let first = press(&mut a, &ctx, "dimc", &["Enter"]).expect("Enter runs the first suggestion");
    let second = press(&mut a, &ctx, "dimc", &["ArrowDown", "Enter"]).expect("Down then Enter runs a suggestion");
    // Up recalls the last command; erasing it back to `dimc` leaves the history: the list shows
    // again, Enter takes its first row and Down moves in the list, not in the history.
    let back = |name: &str| vec!["Backspace"; name.len() - "dimc".len()];
    let mut keys = vec!["ArrowUp"];
    keys.extend(back(&second));
    keys.push("Enter");
    assert_eq!(press(&mut a, &ctx, "", &keys).as_deref(), Some(first.as_str()));
    let mut keys = vec!["ArrowUp"];
    keys.extend(back(&first));
    keys.extend(["ArrowDown", "Enter"]);
    assert_eq!(press(&mut a, &ctx, "", &keys).as_deref(), Some(second.as_str()));
}

#[test]
fn a_row_chosen_with_down_wins_over_an_exact_alias() {
    let ctx = egui::Context::default();
    let mut a = CadApp::new(Session::new(), Services::default());
    for _ in 0..3 {
        frame(&mut a, &ctx);
    }
    // `l` is LINE's alias: Enter alone runs LINE, but Down picks the next row and Enter runs it.
    assert_eq!(press(&mut a, &ctx, "l", &["Enter"]).as_deref(), Some("l"));
    let chosen = press(&mut a, &ctx, "l", &["ArrowDown", "Enter"]).expect("Down then Enter runs a suggestion");
    assert!(chosen.len() > 1 && !chosen.eq_ignore_ascii_case("line"), "the chosen row runs, not LINE: {chosen}");
    // Down then Up comes back to the first row: the alias runs as typed again.
    assert_eq!(press(&mut a, &ctx, "l", &["ArrowDown", "ArrowUp", "Enter"]).as_deref(), Some("l"));
}
