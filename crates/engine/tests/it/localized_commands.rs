//! Polish command names at the command line: OKRĄG starts CIRCLE while the interface is Polish,
//! `_` still asks for the English name, and English stays the default.

use cadcraft_engine::Session;
use cadcraft_engine::cmd::localized::set_language;

fn started(s: &mut Session, typed: &str) -> String {
    s.cancel();
    let from = s.log.len();
    s.cmdline(typed).unwrap();
    s.log.get(from..).unwrap_or_default().iter().find(|l| l.starts_with("Command: ")).cloned().unwrap_or_default()
}

#[test]
fn polish_names_and_shortcuts_start_their_commands() {
    set_language("pl");
    let mut s = Session::new();
    for (typed, id) in [
        ("OKRĄG", "CIRCLE"),
        ("okrag", "CIRCLE"),
        ("o", "CIRCLE"),
        ("ŁUK", "ARC"),
        ("Ł", "ARC"),
        ("LINIA", "LINE"),
        ("prostokąt", "RECTANG"),
        ("PL", "PLINE"),
        ("KRESKUJ", "HATCH"),
        ("WTEKST", "MTEXT"),
    ] {
        assert_eq!(started(&mut s, typed), format!("Command: {id}"), "{typed}");
    }
    set_language("en");
}

#[test]
fn an_underscore_and_english_names_keep_working_in_polish() {
    set_language("pl");
    let mut s = Session::new();
    assert_eq!(started(&mut s, "_O"), "Command: OFFSET");
    assert_eq!(started(&mut s, "_PR"), "Command: PROPERTIES");
    assert_eq!(started(&mut s, "CIRCLE"), "Command: CIRCLE");
    assert_eq!(started(&mut s, "_LINE"), "Command: LINE");
    set_language("en");
}

#[test]
fn english_interfaces_do_not_take_polish_names() {
    set_language("en");
    let mut s = Session::new();
    assert_eq!(started(&mut s, "o"), "Command: OFFSET");
    s.cancel();
    assert!(s.cmdline("OKRĄG").is_err());
}
