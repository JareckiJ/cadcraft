//! The default save version (#421): File > Default Save Format > Version …, kept in the
//! preferences, applied by the Save dialog to drawings that have no file yet.

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, SaveAs, set_io, set_write_as};
use cadcraft_engine::doc::Drawing;
use cadcraft_ui_egui::{CadApp, Services};
use serde_json::{Value, json};

fn picked() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("cadcraft-ui-save-version-{}.dxf", std::process::id()))
}

fn with_options(_: &Drawing, _: &str, o: &SaveAs) -> Result<Vec<u8>, String> {
    Ok(format!("{}|{}", o.version.map(|v| v.year()).unwrap_or("-"), o.binary).into_bytes())
}

#[test]
fn default_save_version_applies_to_new_drawings_and_persists() {
    set_io(IoHooks { read: |_, _| Ok(Drawing::new_metric()), write: |_, _| Ok(b"plain".to_vec()), plot: None });
    set_write_as(with_options);
    let services = Services { pick_save: Some(Box::new(|_: &str| Some(picked().to_string_lossy().to_string()))), ..Services::default() };
    let mut app = CadApp::new(Session::new(), services);
    assert_eq!(app.run("ui.saveversion", json!({})).ok(), Some(json!({ "saveVersion": "2018" })), "2018 by default");
    assert!(app.run("ui.saveversion", json!({ "version": "2005" })).is_err());
    assert_eq!(app.run("ui.saveversion", json!({ "version": 2010 })).ok(), Some(json!({ "saveVersion": "2010" })));

    // The Save dialog's version starts on the default version for an untitled drawing …
    app.run("ui.saveas", Value::Null).unwrap();
    assert_eq!(app.save_dlg.as_ref().map(|d| d.version.year()), Some("2010"));
    cadcraft_ui_egui::saving::confirm(&mut app).unwrap();
    assert_eq!(std::fs::read(picked()).unwrap(), b"2010|false");
    // … and on the drawing's own for a drawing that has a file (2010 now, not the default).
    app.run("ui.saveversion", json!({ "version": 2018 })).unwrap();
    app.run("ui.saveas", Value::Null).unwrap();
    assert_eq!(app.save_dlg.as_ref().map(|d| d.version.year()), Some("2010"));
    cadcraft_ui_egui::saving::confirm(&mut app).unwrap();
    assert_eq!(std::fs::read(picked()).unwrap(), b"2010|false");
    // A save without a version (QSAVE) writes the drawing's own through the plain writer.
    app.run("qsave", Value::Null).unwrap();
    assert_eq!(std::fs::read(picked()).unwrap(), b"plain");
    app.run("ui.saveversion", json!({ "version": 2010 })).unwrap();
    let _ = std::fs::remove_file(picked());

    // Menu items, preferences (bad values ignored) and resetting the palettes.
    app.start("ui.saveversion.2004");
    assert_eq!(app.ui.save_version, "2004");
    let mut b = CadApp::new(Session::empty(), Services::default());
    b.load_prefs(&app.prefs_json());
    assert_eq!(b.ui.save_version, "2004");
    b.load_prefs(r#"{"saveVersion": "R14"}"#);
    assert_eq!(b.ui.save_version, "2004");
    b.start("ui.resetpalettes");
    assert_eq!(b.ui.save_version, "2004");
}
