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
