//! The Save As dialog: the file type, the DXF/DWG version and the R12 options (binary DXF, the
//! DOCFA profile with its unit) chosen before the host's file picker, which cannot report the
//! type the user picked. "Save..." opens the picker with the chosen type and runs `saveas`.
//!
//! QSAVE of a drawing that has no file yet skips this dialog and goes straight to the picker
//! (`ui.saveas.pick`): the close prompt's "Save" relies on that save finishing in the same call.

use egui::{RichText, vec2};
use serde_json::{Value, json};

use crate::theme::Tokens;
use crate::{CadApp, SaveFormat, SaveVersion};

/// A file type the dialog saves as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileType {
    Dxf,
    Dwg,
    Dwt,
    Svg,
    Png,
    Pdf,
}

impl FileType {
    pub const ALL: [FileType; 6] = [FileType::Dxf, FileType::Dwg, FileType::Dwt, FileType::Svg, FileType::Png, FileType::Pdf];

    pub fn ext(self) -> &'static str {
        match self {
            FileType::Dxf => "dxf",
            FileType::Dwg => "dwg",
            FileType::Dwt => "dwt",
            FileType::Svg => "svg",
            FileType::Png => "png",
            FileType::Pdf => "pdf",
        }
    }

    /// The label of the type (the file picker's filter names, translated there too).
    fn label(self) -> &'static str {
        match self {
            FileType::Dxf => crate::tl!("Drawing (DXF)"),
            FileType::Dwg => crate::tl!("Drawing (DWG)"),
            FileType::Dwt => crate::tl!("Drawing Template (DWT)"),
            FileType::Svg => crate::tl!("SVG"),
            FileType::Png => crate::tl!("PNG"),
            FileType::Pdf => crate::tl!("PDF"),
        }
    }

    pub fn from_ext(ext: &str) -> Option<FileType> {
        FileType::ALL.into_iter().find(|t| t.ext().eq_ignore_ascii_case(ext.trim().trim_start_matches('.')))
    }

    /// A drawing file (DXF, DWG, DWT) has a version; images don't.
    fn versioned(self) -> bool {
        matches!(self, FileType::Dxf | FileType::Dwg | FileType::Dwt)
    }
}

/// The drawing's unit, for the DOCFA profile (the file is in metres).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    M,
    Cm,
    Mm,
}

impl Unit {
    pub const ALL: [Unit; 3] = [Unit::M, Unit::Cm, Unit::Mm];

    pub fn as_str(self) -> &'static str {
        match self {
            Unit::M => "m",
            Unit::Cm => "cm",
            Unit::Mm => "mm",
        }
    }

    /// The unit the drawing's `$INSUNITS` names (4 mm, 5 cm, 6 m), else metres.
    fn from_insunits(v: i64) -> Unit {
        match v {
            4 => Unit::Mm,
            5 => Unit::Cm,
            _ => Unit::M,
        }
    }
}

/// The dialog's choices until Save.
#[derive(Clone, Debug, PartialEq)]
pub struct SaveAsState {
    pub file_type: FileType,
    pub version: SaveVersion,
    pub binary: bool,
    pub docfa: bool,
    pub unit: Unit,
}

impl SaveAsState {
    /// The choices a drawing starts from: its own file type and version (an untitled drawing:
    /// the default save format and version), its binary flag, its unit.
    pub fn for_app(app: &CadApp) -> SaveAsState {
        let st = app.session.state().ok();
        let untitled = st.as_ref().is_none_or(|s| s.path.is_none());
        let title = st.as_ref().map(|s| s.title.clone()).unwrap_or_default();
        let from_title = std::path::Path::new(&title).extension().and_then(|e| FileType::from_ext(&e.to_string_lossy()));
        let default_type = match app.ui.save_format {
            SaveFormat::Dxf => FileType::Dxf,
            SaveFormat::Dwg => FileType::Dwg,
        };
        let file_type = from_title.filter(|t| t.versioned()).unwrap_or(default_type);
        let doc = app.session.doc().ok();
        let own = doc.and_then(|d| d.header.get("ACADVER").and_then(cadcraft_doc::HVal::as_str).and_then(SaveVersion::for_acadver));
        let version = match (untitled, own) {
            (false, Some(v)) => v,
            _ => SaveVersion::parse(&app.ui.save_version).unwrap_or_default(),
        };
        let unit = doc.map_or(Unit::M, |d| Unit::from_insunits(d.header.i64("INSUNITS", 0)));
        SaveAsState { file_type, version, binary: st.as_ref().is_some_and(|s| s.binary_dxf) && file_type == FileType::Dxf, docfa: false, unit }
    }

    /// The `saveas` parameters for `path`.
    pub fn params(&self, path: &str) -> Value {
        let mut p = json!({ "path": path });
        if let Some(o) = p.as_object_mut() {
            if self.docfa && self.docfa_file() {
                o.insert("docfa".into(), json!(true));
                o.insert("version".into(), json!(self.version.year()));
                o.insert("unit".into(), json!(self.unit.as_str()));
            } else if self.file_type.versioned() {
                o.insert("version".into(), json!(self.version.year()));
                if self.binary && self.file_type == FileType::Dxf && self.version.lossless() {
                    o.insert("binary".into(), json!(true));
                }
            }
        }
        p
    }

    /// Whether the chosen file type can take the DOCFA profile (DXF and DWG).
    fn docfa_file(&self) -> bool {
        matches!(self.file_type, FileType::Dxf | FileType::Dwg)
    }

    /// The versions the file type can be saved in with the current choices: with the DOCFA
    /// profile R12 (DXF only), 2000 and 2004; otherwise every one the format has.
    pub fn versions(&self) -> Vec<SaveVersion> {
        SaveVersion::ALL
            .into_iter()
            .filter(|v| *v != SaveVersion::R12 || self.file_type == FileType::Dxf)
            .filter(|v| !self.docfa || cadcraft_io_docfa_versions().contains(v))
            .collect()
    }

    /// The file name the picker suggests: the drawing's name with the chosen extension (a DOCFA
    /// export is named apart, beside the drawing).
    pub fn suggested_name(&self, title: &str) -> String {
        let stem = std::path::Path::new(title).file_stem().map(|s| s.to_string_lossy().to_string()).filter(|s| !s.trim().is_empty());
        let stem = stem.unwrap_or_else(|| "Drawing".into());
        if self.docfa && self.docfa_file() { format!("{stem}-docfa.{}", self.file_type.ext()) } else { format!("{stem}.{}", self.file_type.ext()) }
    }

    /// Choices that don't go together are settled: the DOCFA profile is for DXF and DWG at R12
    /// (DXF only), 2000 or 2004, DWG has no R12, binary DXF is 2000 or later and not for DOCFA.
    fn settle(&mut self) {
        if !self.docfa_file() {
            self.docfa = false;
        }
        if self.file_type != FileType::Dxf {
            self.binary = false;
        }
        if self.docfa && !self.versions().contains(&self.version) {
            self.version = if self.file_type == FileType::Dxf { SaveVersion::R12 } else { SaveVersion::R2000 };
        }
        if self.file_type != FileType::Dxf && self.version == SaveVersion::R12 {
            self.version = SaveVersion::R2000;
        }
        if !self.version.lossless() || self.docfa {
            self.binary = false;
        }
    }
}

/// The versions the DOCFA profile writes (R12, 2000, 2004).
fn cadcraft_io_docfa_versions() -> [SaveVersion; 3] {
    cadcraft_engine::cmd::file::DOCFA_VERSIONS
}

/// Open the dialog for the active drawing.
pub fn open(app: &mut CadApp) {
    app.saveas = Some(SaveAsState::for_app(app));
    app.ui.dialog = Some("saveas".into());
}

/// `path` with the chosen extension when the picker gave none (or a bare name).
fn with_extension(path: &str, ext: &str) -> String {
    let has = std::path::Path::new(path).extension().is_some_and(|e| !e.is_empty());
    if has { path.to_string() } else { format!("{path}.{ext}") }
}

/// Ask the host where to save and run `saveas` with the dialog's choices. `Some(true)` once
/// saved, `Some(false)` when the picker was cancelled or the save failed (the dialog stays),
/// `None` without a picker (reported on the command line).
pub fn save(app: &mut CadApp) -> Option<bool> {
    let st = app.saveas.clone().unwrap_or_else(|| SaveAsState::for_app(app));
    let title = app.session.state().map(|s| s.title.clone()).unwrap_or_default();
    let Some(pick) = app.services.pick_save.as_ref() else {
        app.session
            .echo(r#"SAVEAS: there is no file dialog here; run it with parameters, e.g. saveas {"path": "plan.dxf", "version": "R12"}"#.to_string());
        return None;
    };
    let Some(path) = pick(&st.suggested_name(&title)) else { return Some(false) };
    let path = with_extension(&path, st.file_type.ext());
    match app.run("saveas", st.params(&path)) {
        Ok(r) => {
            if let Some(p) = r.get("path").and_then(Value::as_str)
                && !st.docfa
                && st.file_type.versioned()
            {
                app.session.echo(format!("Saved {p}"));
            }
            Some(true)
        }
        Err(_) => Some(false),
    }
}

pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let t = Tokens::get();
    if app.saveas.is_none() {
        app.saveas = Some(SaveAsState::for_app(app));
    }
    let Some(mut st) = app.saveas.clone() else { return };
    let (mut save, mut cancel) = (false, false);
    let center = ctx.content_rect().center();
    egui::Window::new(crate::tl!("Save As"))
        .id(egui::Id::new("Save As"))
        .open(open)
        .collapsible(false)
        .resizable(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(center)
        .show(ctx, |ui| {
            egui::Grid::new("saveas_grid").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
                ui.label(crate::tl!("File type"));
                egui::ComboBox::from_id_salt("saveas_type").selected_text(st.file_type.label()).width(220.0).show_ui(ui, |ui| {
                    for ft in FileType::ALL {
                        ui.selectable_value(&mut st.file_type, ft, ft.label());
                    }
                });
                ui.end_row();
                if st.file_type.versioned() {
                    ui.label(crate::tl!("Version"));
                    let versions = st.versions();
                    egui::ComboBox::from_id_salt("saveas_version").selected_text(version_label(st.version)).width(260.0).show_ui(ui, |ui| {
                        for v in versions {
                            ui.selectable_value(&mut st.version, v, version_label(v));
                        }
                    });
                    ui.end_row();
                }
                if st.file_type == FileType::Dxf {
                    ui.label("");
                    ui.add_enabled_ui(st.version.lossless() && !st.docfa, |ui| ui.checkbox(&mut st.binary, crate::tl!("Binary DXF")));
                    ui.end_row();
                }
                if st.docfa_file() {
                    ui.label("");
                    ui.checkbox(&mut st.docfa, crate::tl!("DOCFA profile (Italian cadastre)")).on_hover_text(crate::tl!(
                        "DXF or DWG as DOCFA imports it: model space only, block references exploded, no hatches, colours only on the DOCFA_POLIGONI polygons, ASCII text, in metres. The drawing keeps its own file."
                    ));
                    ui.end_row();
                    if st.docfa {
                        ui.label(crate::tl!("Drawing unit"));
                        ui.horizontal(|ui| {
                            for u in Unit::ALL {
                                ui.radio_value(&mut st.unit, u, u.as_str());
                            }
                        });
                        ui.end_row();
                    }
                }
            });
            st.settle();
            if st.docfa {
                ui.add(
                    egui::Label::new(
                        RichText::new(crate::tl!(
                            "An export for DOCFA: a reduced copy of the drawing, which keeps its own file. Closed polylines on layer DOCFA_POLIGONI are the surface polygons; their colour (1–8) is the area class."
                        ))
                        .color(t.text_dim),
                    )
                    .wrap(),
                );
                if st.file_type == FileType::Dwg {
                    ui.add(egui::Label::new(RichText::new(crate::tl!("DWG has no polygon layer for DOCFA: it computes the surfaces inside DOCFA.")).color(t.text_dim)).wrap());
                }
            } else if st.file_type.versioned() && st.version == SaveVersion::R12 {
                ui.add(
                    egui::Label::new(
                        RichText::new(crate::tl!(
                            "R12 is a reduced copy of the drawing: curves, hatches, multiline text, dimensions and layouts become lines, polylines and text."
                        ))
                        .color(t.text_dim),
                    )
                    .wrap(),
                );
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    save = ui.button(crate::tl!("Save...")).clicked();
                    cancel = ui.button(crate::tl!("Cancel")).clicked();
                });
            });
        });
    app.saveas = Some(st);
    if cancel {
        *open = false;
    }
    if save && save_from_dialog(app) {
        *open = false;
    }
}

/// Save from the dialog: closed once saved, or when there is no picker to ask.
fn save_from_dialog(app: &mut CadApp) -> bool {
    save(app) != Some(false)
}

/// A version's label; the ones DOCFA reads say so.
pub fn version_label(v: SaveVersion) -> String {
    let docfa = cadcraft_io_docfa_versions().contains(&v) && v != SaveVersion::R2004;
    let name = match v {
        SaveVersion::R12 => format!("R12 ({})", crate::tl!("AutoCAD Release 12")),
        other => other.year().to_string(),
    };
    if docfa { format!("{name} · {}", crate::tl!("DOCFA compatible")) } else { name }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use cadcraft_engine::Session;
    use serde_json::json;

    use super::*;
    use crate::Services;

    fn app_with_picker(answer: Option<String>) -> (CadApp, Rc<RefCell<Vec<String>>>) {
        let asked = Rc::new(RefCell::new(Vec::new()));
        let log = Rc::clone(&asked);
        let services = Services {
            pick_open: None,
            pick_save: Some(Box::new(move |name: &str| {
                log.borrow_mut().push(name.to_string());
                answer.clone()
            })),
            ..Services::default()
        };
        (CadApp::new(Session::new(), services), asked)
    }

    #[test]
    fn save_as_opens_the_dialog_with_the_drawings_own_type_and_version() {
        let (mut app, asked) = app_with_picker(Some("x.dxf".into()));
        app.start("ui.saveas");
        assert_eq!(app.ui.dialog.as_deref(), Some("saveas"));
        assert!(asked.borrow().is_empty(), "the picker waits for Save...");
        let st = app.saveas.clone().unwrap();
        assert_eq!((st.file_type, st.version, st.docfa, st.binary), (FileType::Dxf, SaveVersion::R2018, false, false));
        // A drawing opened as a 2000 DWG in millimetres.
        let mut d = cadcraft_doc::Drawing::new_metric();
        d.header.set_str("ACADVER", "AC1015");
        d.header.set_i64("INSUNITS", 4);
        app.session.open_drawing(d, "plan.dwg", Some("/tmp/plan.dwg".into()));
        open(&mut app);
        let st = app.saveas.clone().unwrap();
        assert_eq!((st.file_type, st.version, st.unit), (FileType::Dwg, SaveVersion::R2000, Unit::Mm));
        assert_eq!(st.suggested_name("plan.dwg"), "plan.dwg");
        assert_eq!(st.params("/tmp/plan.dwg"), json!({ "path": "/tmp/plan.dwg", "version": "2000" }));
    }

    #[test]
    fn choices_settle_and_become_saveas_parameters() {
        let mut st = SaveAsState { file_type: FileType::Dxf, version: SaveVersion::R2018, binary: true, docfa: true, unit: Unit::Mm };
        st.settle();
        assert_eq!(st.version, SaveVersion::R12, "DOCFA is R12");
        assert!(!st.binary, "R12 is ASCII");
        assert_eq!(st.params("/tmp/plan-docfa.dxf"), json!({ "path": "/tmp/plan-docfa.dxf", "docfa": true, "version": "R12", "unit": "mm" }));
        assert_eq!(st.suggested_name("Plan.dwg"), "Plan-docfa.dxf");
        // DOCFA reads DXF and DWG 2000 too; 2018 is outside the profile.
        st.version = SaveVersion::R2000;
        st.settle();
        assert_eq!((st.version, st.params("a.dxf")["version"].as_str()), (SaveVersion::R2000, Some("2000")));
        st.version = SaveVersion::R2018;
        st.settle();
        assert_eq!(st.version, SaveVersion::R12, "DXF falls back to R12");
        st.file_type = FileType::Dwg;
        st.settle();
        assert!(st.docfa && st.version == SaveVersion::R2000, "DWG falls back to 2000: {st:?}");
        assert_eq!(st.versions(), vec![SaveVersion::R2000, SaveVersion::R2004]);
        assert_eq!(st.params("/tmp/plan-docfa.dwg"), json!({ "path": "/tmp/plan-docfa.dwg", "docfa": true, "version": "2000", "unit": "mm" }));
        assert_eq!(st.suggested_name("Plan.dwg"), "Plan-docfa.dwg");
        // Not for images or templates.
        st.file_type = FileType::Pdf;
        st.settle();
        assert!(!st.docfa);
        // R12 plain, then DWG: DWG has no R12.
        let mut st = SaveAsState { file_type: FileType::Dxf, version: SaveVersion::R12, binary: false, docfa: false, unit: Unit::M };
        assert_eq!(st.params("a.dxf"), json!({ "path": "a.dxf", "version": "R12" }));
        st.file_type = FileType::Dwg;
        st.settle();
        assert_eq!(st.version, SaveVersion::R2000);
        // Images carry no version; binary DXF only at 2000 and later.
        let st = SaveAsState { file_type: FileType::Pdf, version: SaveVersion::R2018, binary: true, docfa: false, unit: Unit::M };
        assert_eq!(st.params("a.pdf"), json!({ "path": "a.pdf" }));
        let st = SaveAsState { file_type: FileType::Dxf, version: SaveVersion::R2004, binary: true, docfa: false, unit: Unit::M };
        assert_eq!(st.params("a.dxf"), json!({ "path": "a.dxf", "version": "2004", "binary": true }));
        assert_eq!(with_extension("/tmp/plan", "dxf"), "/tmp/plan.dxf");
        assert_eq!(with_extension("/tmp/plan.DXF", "dxf"), "/tmp/plan.DXF");
    }

    #[test]
    fn save_asks_the_picker_with_the_chosen_type_and_keeps_the_dialog_when_cancelled() {
        let (mut app, asked) = app_with_picker(None);
        open(&mut app);
        if let Some(st) = app.saveas.as_mut() {
            st.file_type = FileType::Svg;
        }
        assert_eq!(save(&mut app), Some(false));
        assert_eq!(*asked.borrow(), vec!["Drawing1.svg".to_string()]);
        // DOCFA names the export apart.
        if let Some(st) = app.saveas.as_mut() {
            st.file_type = FileType::Dxf;
            st.docfa = true;
        }
        assert_eq!(save(&mut app), Some(false));
        assert_eq!(asked.borrow().last().map(String::as_str), Some("Drawing1-docfa.dxf"));
        // Without a picker the command line says how to save.
        let mut app = CadApp::new(Session::new(), Services::default());
        assert_eq!(save(&mut app), None);
        assert!(app.session.log.iter().any(|l| l.contains("no file dialog")));
    }

    #[test]
    fn qsave_of_an_untitled_drawing_goes_straight_to_the_picker() {
        let (mut app, asked) = app_with_picker(None);
        app.start("qsave");
        assert_eq!(*asked.borrow(), vec!["Drawing1.dxf".to_string()]);
        assert!(app.ui.dialog.is_none());
    }

    #[test]
    fn the_dialog_renders_its_choices() {
        let (mut app, _) = app_with_picker(None);
        open(&mut app);
        let ctx = egui::Context::default();
        let input =
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0))), ..Default::default() };
        for _ in 0..2 {
            let mut output = ctx.run_ui(input.clone(), |ui| app.ui(ui));
            output.textures_delta.clear();
        }
        assert_eq!(app.ui.dialog.as_deref(), Some("saveas"));
        assert!(app.saveas.is_some());
    }
}
