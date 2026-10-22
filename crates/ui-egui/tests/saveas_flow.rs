//! Save As per file (gaps FF3): the picker gives the file and so the format (its extension), then
//! the Save As window asks for the version (and binary for DXF) before writing. An export (SVG,
//! PDF…) is written at once, closing a drawing still saves without the window, and with FILEDIA 0
//! SAVEAS asks on the command line instead.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Mutex;

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{FileFormat, IoHooks, SaveAs, set_filedia, set_io, set_write_as};
use cadcraft_engine::doc::Drawing;
use cadcraft_ui_egui::closing::Choice;
use cadcraft_ui_egui::{CadApp, SaveFormat, SaveVersion, Services, dialogs, theme};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::Value;

/// FILEDIA is process-wide: the tests here run one at a time.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn with_options(_: &Drawing, _: &str, o: &SaveAs) -> Result<Vec<u8>, String> {
    Ok(format!("{}|{}", o.version.map(|v| v.year()).unwrap_or("-"), o.binary).into_bytes())
}

fn install() {
    set_io(IoHooks { read: |_, _| Ok(Drawing::new_metric()), write: |_, _| Ok(b"plain".to_vec()), plot: None });
    set_write_as(with_options);
}

fn dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("cadcraft-ui-saveas-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn read(p: &std::path::Path) -> String {
    String::from_utf8(std::fs::read(p).unwrap_or_default()).unwrap_or_default()
}

/// An app whose Save dialog answers `answer` and counts its calls.
fn app(answer: Option<std::path::PathBuf>) -> (CadApp, Rc<RefCell<u32>>) {
    let picks = Rc::new(RefCell::new(0));
    let p = Rc::clone(&picks);
    let services = Services {
        pick_save: Some(Box::new(move |_: &str| {
            *p.borrow_mut() += 1;
            answer.as_ref().map(|a| a.to_string_lossy().to_string())
        })),
        ..Services::default()
    };
    let mut app = CadApp::new(Session::new(), services);
    app.cmdline("circle 0,0 5");
    (app, picks)
}

fn harness(app: CadApp) -> Harness<'static, CadApp> {
    let mut h = Harness::builder().with_size(egui::vec2(900.0, 700.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            theme::apply(ui.ctx(), egui::Theme::Dark, false);
            dialogs::show(app, ui.ctx());
        },
        app,
    );
    h.run_steps(3);
    h
}

#[test]
fn picked_dwg_asks_for_the_version_then_saves() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    install();
    let d = dir("dwg");
    let file = d.join("site.dwg");
    let (mut a, picks) = app(Some(file.clone()));
    a.ui.save_version = "2013".into();
    a.start("saveas");
    assert_eq!(*picks.borrow(), 1);
    assert_eq!(a.ui.dialog.as_deref(), Some("saveas"));
    let dlg = a.save_dlg.clone().unwrap();
    assert_eq!((dlg.format, dlg.version, dlg.binary), (FileFormat::Dwg, SaveVersion::R2013, false), "the default save version");
    assert!(!file.exists(), "nothing is written before Save");

    let mut h = harness(a);
    assert!(h.query_by_label("Binary DXF").is_none(), "binary is a DXF choice");
    h.state_mut().save_dlg.as_mut().unwrap().version = SaveVersion::R2007;
    h.run_steps(2);
    h.get_by_label("Save").click();
    h.run_steps(3);
    assert_eq!(read(&file), "2007|false");
    let a = h.state();
    assert!(a.ui.dialog.is_none() && a.save_dlg.is_none());
    let st = a.session.state().unwrap();
    assert!(!st.is_dirty());
    assert_eq!(st.path.as_deref(), Some(file.to_string_lossy().as_ref()));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn picked_dxf_offers_binary_and_cancel_writes_nothing() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    install();
    let d = dir("dxf");
    let file = d.join("plan.dxf");
    let (mut a, _) = app(Some(file.clone()));
    a.start("ui.saveas");
    let mut h = harness(a);
    // Cancel: no file, the drawing keeps its changes and has no name yet.
    h.get_by_label("Cancel").click();
    h.run_steps(3);
    assert!(!file.exists());
    assert!(h.state().ui.dialog.is_none() && h.state().save_dlg.is_none());
    assert!(h.state().session.state().unwrap().is_dirty());

    // Typed SAVEAS again: binary DXF at 2018 (the default version).
    h.state_mut().cmdline("saveas");
    h.run_steps(3);
    h.get_by_label("Binary DXF").click();
    h.run_steps(2);
    h.get_by_label("Save").click();
    h.run_steps(3);
    assert_eq!(read(&file), "2018|true");
    assert!(h.state().session.state().unwrap().binary_dxf);

    // The drawing has a file now: Save As offers its own version and encoding.
    h.state_mut().start("saveas");
    let dlg = h.state().save_dlg.clone().unwrap();
    assert_eq!((dlg.format, dlg.version, dlg.binary), (FileFormat::Dxf, SaveVersion::R2018, true));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn exports_save_at_once_and_closing_saves_without_the_window() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    install();
    let d = dir("export");
    let svg = d.join("view.svg");
    let (mut a, _) = app(Some(svg.clone()));
    a.start("saveas");
    assert!(a.ui.dialog.is_none(), "an export has no version to choose");
    assert_eq!(read(&svg), "plain");

    // Closing an untitled drawing: Save picks the file and saves before the prompt moves on.
    let file = d.join("closing.dwg");
    let (mut a, picks) = app(Some(file.clone()));
    a.ui.save_format = SaveFormat::Dwg;
    a.ui.save_version = "2010".into();
    a.request_close(None);
    assert!(a.close_prompt().is_some());
    a.answer_close(Choice::Save);
    assert_eq!(*picks.borrow(), 1);
    assert!(a.ui.dialog.is_none());
    assert_eq!(read(&file), "2010|false");
    assert!(a.closing.is_none());
    let _ = std::fs::remove_dir_all(&d);
}

/// Puts FILEDIA back to 1 even when an assertion fails.
struct FiledIa;
impl Drop for FiledIa {
    fn drop(&mut self) {
        set_filedia(true);
    }
}

#[test]
fn filedia_0_asks_on_the_command_line_with_the_default_save_format() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    install();
    let _restore = FiledIa;
    let (mut a, picks) = app(None);
    a.cmdline("filedia 0");
    a.ui.save_format = SaveFormat::Dwg;
    a.ui.save_version = "2004".into();
    for how in ["typed", "menu", "save as", "qsave"] {
        match how {
            "typed" => drop(a.cmdline("saveas")),
            "menu" => a.start("saveas"),
            "save as" => drop(a.run("ui.saveas", Value::Null)),
            _ => a.start("qsave"),
        }
        assert_eq!(*picks.borrow(), 0, "{how}: no file dialog");
        assert!(a.ui.dialog.is_none(), "{how}");
        let p = a.session.prompt_text();
        assert!(p.starts_with("SAVEAS Enter file format") && p.ends_with("<2004>:"), "{how}: {p}");
        a.session.input(cadcraft_engine::Input::Cancel).unwrap();
    }
    // Without a file picker (headless hosts) SAVEAS asks on the command line too.
    a.cmdline("filedia 1");
    let mut b = CadApp::new(Session::new(), Services::default());
    b.start("saveas");
    assert!(b.session.prompt_text().starts_with("SAVEAS Enter file format"), "{}", b.session.prompt_text());
}
