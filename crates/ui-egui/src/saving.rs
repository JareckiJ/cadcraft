//! Save As in the interface (gaps FF3): the file picker, then the version to save in.
//!
//! The native file pickers can't say which file type was chosen (rfd returns only the path on
//! every platform; the macOS save panel shows no type list at all, and the web build has no
//! picker), so a list of "2018 DWG", "2013 DWG"… filters could not tell versions apart. The
//! format therefore comes from the extension of the file picked (DXF, DWG, DWT; anything else is
//! an export such as SVG or PDF), and a small Save As window asks for the version (and, for DXF,
//! ASCII or binary) before the file is written. It starts on the version the drawing would be
//! saved in anyway (the Default Save Format version for a drawing without a file, the drawing's
//! own otherwise), so Save keeps today's behaviour.
//!
//! With FILEDIA 0, or a host without a file picker, SAVEAS asks on the command line instead (the
//! engine's prompt sequence, `cadcraft_engine::cmd::file`). JSON calls never get here.

use cadcraft_engine::cmd::file::{FileFormat, drawing_version, filedia, set_untitled_save_default};
use serde_json::{Value, json};

use crate::{CadApp, SaveFormat, SaveVersion};

/// The Save As window while open: the file picked and what to save it as.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveDlg {
    pub path: String,
    pub format: FileFormat,
    pub version: SaveVersion,
    /// Binary DXF (DXF only).
    pub binary: bool,
}

/// SAVEAS typed or from a menu, Save As (`ui.saveas`), and QSAVE of a drawing without a file:
/// the picker and the Save As window, or the command-line prompts. `None` for anything else.
pub fn route(app: &mut CadApp, id: &str, no_path: bool) -> Option<Result<Value, String>> {
    let untitled = app.session.state().is_ok_and(|s| s.path.is_none());
    match id {
        "ui.saveas" | "saveas" if no_path => {}
        "qsave" if no_path && untitled => {}
        _ => return None,
    }
    sync_untitled_default(app);
    if !filedia() || app.services.pick_save.is_none() {
        if let Err(e) = app.session.start("saveas") {
            app.session.echo(e.to_string());
        }
        return Some(Ok(Value::Null));
    }
    Some(pick(app))
}

/// The engine's SAVEAS prompt offers the interface's Default Save Format to untitled drawings.
fn sync_untitled_default(app: &CadApp) {
    let format = match app.ui.save_format {
        SaveFormat::Dxf => FileFormat::Dxf,
        SaveFormat::Dwg => FileFormat::Dwg,
    };
    set_untitled_save_default(format, SaveVersion::parse(&app.ui.save_version).unwrap_or_default());
}

fn picked(app: &CadApp) -> Option<String> {
    let title = app.session.state().map(|s| s.title.clone()).ok();
    let name = app.ui.save_format.suggested_name(title.as_deref());
    app.services.pick_save.as_ref().and_then(|f| f(&name))
}

/// Pick the file; a drawing file opens the Save As window, anything else is exported at once.
fn pick(app: &mut CadApp) -> Result<Value, String> {
    if let Err(e) = app.session.state() {
        return Err(e.to_string());
    }
    let Some(path) = picked(app) else { return Ok(Value::Null) };
    match FileFormat::of_path(&path) {
        Some(format) => {
            open(app, path, format);
            Ok(Value::Null)
        }
        None => app.session.execute("saveas", &json!({ "path": path })).map_err(|e| e.to_string()),
    }
}

/// Pick the file and save at once in the default version, without the Save As window: the
/// "Save changes?" prompt of closing, which needs the drawing saved before it goes on.
pub fn pick_and_save(app: &mut CadApp) -> Result<Value, String> {
    let Some(path) = picked(app) else { return Ok(Value::Null) };
    app.session.execute("saveas", &saveas_params(&app.ui, &app.session, &path)).map_err(|e| e.to_string())
}

/// SAVEAS parameters for a path picked in the Save dialog: a drawing that has no file yet is saved
/// in the default save version; one opened from a file keeps that file's version.
pub fn saveas_params(ui: &crate::UiState, session: &cadcraft_engine::Session, path: &str) -> Value {
    let untitled = session.state().is_ok_and(|s| s.path.is_none());
    match SaveVersion::parse(&ui.save_version) {
        Some(v) if untitled => json!({ "path": path, "version": v.year() }),
        _ => json!({ "path": path }),
    }
}

/// Open the Save As window for `path`, on the version and encoding the drawing would get anyway.
pub fn open(app: &mut CadApp, path: String, format: FileFormat) {
    let st = app.session.state().ok();
    let untitled = st.is_some_and(|s| s.path.is_none());
    let version = if untitled { SaveVersion::parse(&app.ui.save_version).unwrap_or_default() } else { drawing_version(&app.session, &path) };
    let binary = format == FileFormat::Dxf && st.is_some_and(|s| s.binary_dxf);
    app.save_dlg = Some(SaveDlg { path, format, version, binary });
    app.ui.dialog = Some("saveas".into());
}

/// Save: write the file with the window's choices. Errors are reported on the command line and
/// the status bar; the drawing keeps its unsaved changes.
pub fn confirm(app: &mut CadApp) -> Result<Value, String> {
    let Some(d) = app.save_dlg.take() else { return Ok(Value::Null) };
    if app.ui.dialog.as_deref() == Some("saveas") {
        app.ui.dialog = None;
    }
    let binary = d.format == FileFormat::Dxf && d.binary;
    let r = app.run("saveas", json!({ "path": d.path, "version": d.version.year(), "binary": binary }));
    if let Err(e) = &r {
        app.set_status(e.clone());
    }
    r
}

fn version_label(v: SaveVersion) -> &'static str {
    match v {
        SaveVersion::R2000 => crate::tl!("Version 2000"),
        SaveVersion::R2004 => crate::tl!("Version 2004"),
        SaveVersion::R2007 => crate::tl!("Version 2007"),
        SaveVersion::R2010 => crate::tl!("Version 2010"),
        SaveVersion::R2013 => crate::tl!("Version 2013"),
        SaveVersion::R2018 => crate::tl!("Version 2018"),
    }
}

/// The Save As window: the file, its format, the version and (DXF) binary; Save or Cancel.
pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let Some(d) = app.save_dlg.as_mut() else {
        *open = false;
        return;
    };
    let file = std::path::Path::new(&d.path).file_name().map_or_else(|| d.path.clone(), |n| n.to_string_lossy().to_string());
    let format = match d.format {
        FileFormat::Dxf => crate::tl!("Drawing (DXF)"),
        FileFormat::Dwg => crate::tl!("Drawing (DWG)"),
        FileFormat::Template => crate::tl!("Drawing Template (DWT)"),
    };
    let (mut save, mut cancel) = (false, false);
    egui::Window::new(crate::tl!("Save As")).id(egui::Id::new("saveas_dialog")).open(open).collapsible(false).resizable(false).show(ctx, |ui| {
        egui::Grid::new("saveas_grid").num_columns(2).show(ui, |ui| {
            ui.label(crate::tl!("File"));
            ui.label(file);
            ui.end_row();
            ui.label(crate::tl!("Format"));
            ui.label(format);
            ui.end_row();
            ui.label(crate::tl!("Version"));
            egui::ComboBox::from_id_salt("saveas_version").selected_text(version_label(d.version)).show_ui(ui, |ui| {
                for v in SaveVersion::ALL.iter().rev() {
                    ui.selectable_value(&mut d.version, *v, version_label(*v));
                }
            });
            ui.end_row();
        });
        if d.format == FileFormat::Dxf {
            ui.checkbox(&mut d.binary, crate::tl!("Binary DXF"));
        }
        ui.separator();
        ui.horizontal(|ui| {
            save = ui.button(crate::tl!("Save")).clicked();
            cancel = ui.button(crate::tl!("Cancel")).clicked();
        });
    });
    if save {
        let _ = confirm(app);
        *open = false;
    }
    if cancel {
        *open = false;
    }
    if !*open {
        app.save_dlg = None;
    }
}
