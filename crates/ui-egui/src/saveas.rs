//! The Save As dialog: the file type, the DXF/DWG version and binary DXF, chosen before the
//! host's file picker, which cannot report the type the user picked. "Save..." opens the picker
//! for the chosen type and runs `saveas` (images: an export, as SAVEAS to an image does).
//!
//! Two file types are for the Italian cadastre's DOCFA: "DXF compatible with DOCFA" and "DWG
//! compatible with DOCFA" (`docs/docfa.md`). They are written at the AutoCAD version DOCFA reads,
//! so the dialog asks for no version, only the drawing's unit (the file is in metres), and they
//! are exports (`export {docfa: true}`): the drawing keeps its own file.
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
    /// A DXF file for DOCFA.
    DocfaDxf,
    /// A DWG file for DOCFA.
    DocfaDwg,
    Dwt,
    Svg,
    Png,
    Pdf,
}

impl FileType {
    pub const ALL: [FileType; 8] =
        [FileType::Dxf, FileType::Dwg, FileType::DocfaDxf, FileType::DocfaDwg, FileType::Dwt, FileType::Svg, FileType::Png, FileType::Pdf];

    pub fn ext(self) -> &'static str {
        match self {
            FileType::Dxf | FileType::DocfaDxf => "dxf",
            FileType::Dwg | FileType::DocfaDwg => "dwg",
            FileType::Dwt => "dwt",
            FileType::Svg => "svg",
            FileType::Png => "png",
            FileType::Pdf => "pdf",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            FileType::Dxf => crate::tl!("Drawing (DXF)"),
            FileType::Dwg => crate::tl!("Drawing (DWG)"),
            FileType::DocfaDxf => crate::tl!("DXF compatible with DOCFA"),
            FileType::DocfaDwg => crate::tl!("DWG compatible with DOCFA"),
            FileType::Dwt => crate::tl!("Drawing Template (DWT)"),
            FileType::Svg => "SVG",
            FileType::Png => "PNG",
            FileType::Pdf => "PDF",
        }
    }

    /// The plain type of an extension (a DOCFA type is only ever chosen).
    pub fn from_ext(ext: &str) -> Option<FileType> {
        let ext = ext.trim().trim_start_matches('.');
        FileType::ALL.into_iter().filter(|t| !t.is_docfa()).find(|t| t.ext().eq_ignore_ascii_case(ext))
    }

    /// A drawing file whose version the dialog asks for (DXF, DWG).
    fn versioned(self) -> bool {
        matches!(self, FileType::Dxf | FileType::Dwg)
    }

    /// One of the two DOCFA types.
    pub fn is_docfa(self) -> bool {
        matches!(self, FileType::DocfaDxf | FileType::DocfaDwg)
    }
}

/// The drawing's unit, for the DOCFA types (the file is in metres).
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

    /// The unit the drawing's `$INSUNITS` names (4 mm, 5 cm), else metres.
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
        SaveAsState { file_type, version, binary: st.as_ref().is_some_and(|s| s.binary_dxf) && file_type == FileType::Dxf, unit }
    }

    /// The command and its parameters for `path`: `saveas`, or `export` for the DOCFA types.
    pub fn command(&self, path: &str) -> (&'static str, Value) {
        if self.file_type.is_docfa() {
            return ("export", json!({ "path": path, "docfa": true, "unit": self.unit.as_str() }));
        }
        let mut p = json!({ "path": path });
        if self.file_type.versioned()
            && let Some(o) = p.as_object_mut()
        {
            o.insert("version".into(), json!(self.version.year()));
            if self.binary && self.file_type == FileType::Dxf {
                o.insert("binary".into(), json!(true));
            }
        }
        ("saveas", p)
    }

    /// The file name the picker suggests: the drawing's name with the chosen extension (a DOCFA
    /// export is named apart, beside the drawing).
    pub fn suggested_name(&self, title: &str) -> String {
        let stem = std::path::Path::new(title).file_stem().map(|s| s.to_string_lossy().to_string()).filter(|s| !s.trim().is_empty());
        let stem = stem.unwrap_or_else(|| "Drawing".into());
        if self.file_type.is_docfa() { format!("{stem}-docfa.{}", self.file_type.ext()) } else { format!("{stem}.{}", self.file_type.ext()) }
    }

    /// Binary is for DXF only.
    fn settle(&mut self) {
        if self.file_type != FileType::Dxf {
            self.binary = false;
        }
    }
}

/// Open the dialog for the active drawing.
pub fn open(app: &mut CadApp) {
    app.saveas = Some(SaveAsState::for_app(app));
    app.ui.dialog = Some("saveas".into());
}

/// Open the dialog on a DOCFA type (File ▸ Save as … compatible with DOCFA, and Options ▸ Open
/// and Save).
pub fn open_docfa(app: &mut CadApp, file_type: FileType) {
    let mut st = SaveAsState::for_app(app);
    st.file_type = file_type;
    app.saveas = Some(st);
    app.ui.dialog = Some("saveas".into());
}

/// `path` with the chosen extension when the picker gave none.
fn with_extension(path: &str, ext: &str) -> String {
    let has = std::path::Path::new(path).extension().is_some_and(|e| !e.is_empty());
    if has { path.to_string() } else { format!("{path}.{ext}") }
}

/// Ask the host where to save and run the dialog's choices. `Some(true)` once saved,
/// `Some(false)` when the picker was cancelled or the save failed (the dialog stays), `None`
/// without a picker (said on the command line).
pub fn save(app: &mut CadApp) -> Option<bool> {
    let st = app.saveas.clone().unwrap_or_else(|| SaveAsState::for_app(app));
    let title = app.session.state().map(|s| s.title.clone()).unwrap_or_default();
    let Some(pick) = app.services.pick_save.as_ref() else {
        app.session
            .echo(r#"SAVEAS: there is no file dialog here; run it with parameters, e.g. saveas {"path": "plan.dxf", "version": 2018}"#.to_string());
        return None;
    };
    let Some(path) = pick(&st.suggested_name(&title)) else { return Some(false) };
    let (command, params) = st.command(&with_extension(&path, st.file_type.ext()));
    Some(app.run(command, params).is_ok())
}

/// A dimmed, wrapping note.
fn note(ui: &mut egui::Ui, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).color(Tokens::get().text_dim)).wrap());
}

pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    if app.saveas.is_none() {
        app.saveas = Some(SaveAsState::for_app(app));
    }
    let Some(mut st) = app.saveas.clone() else { return };
    let (mut save_clicked, mut cancel) = (false, false);
    egui::Window::new(crate::tl!("Save As"))
        .id(egui::Id::unique("saveas_dialog"))
        .open(open)
        .collapsible(false)
        .resizable(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| {
            ui.set_max_width(440.0);
            egui::Grid::new("saveas_grid").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
                ui.label(crate::tl!("File type"));
                egui::ComboBox::from_id_salt("saveas_type").selected_text(st.file_type.label()).width(260.0).show_ui(ui, |ui| {
                    for ft in FileType::ALL {
                        ui.selectable_value(&mut st.file_type, ft, ft.label());
                    }
                });
                ui.end_row();
                if st.file_type.versioned() {
                    ui.label(crate::tl!("Version"));
                    egui::ComboBox::from_id_salt("saveas_version").selected_text(version_label(st.version)).width(260.0).show_ui(ui, |ui| {
                        for v in SaveVersion::ALL {
                            ui.selectable_value(&mut st.version, v, version_label(v));
                        }
                    });
                    ui.end_row();
                }
                if st.file_type == FileType::Dxf {
                    ui.label("");
                    ui.checkbox(&mut st.binary, crate::tl!("Binary DXF"));
                    ui.end_row();
                }
                if st.file_type.is_docfa() {
                    ui.label(crate::tl!("Drawing unit"));
                    ui.horizontal(|ui| {
                        for u in Unit::ALL {
                            ui.radio_value(&mut st.unit, u, u.as_str());
                        }
                    });
                    ui.end_row();
                }
            });
            st.settle();
            if st.file_type.is_docfa() {
                note(
                    ui,
                    crate::tl!(
                        "A reduced copy for DOCFA, in metres (AutoCAD 2000): model space only, blocks exploded, no hatches, ASCII text, colours only on the closed polylines of layer DOCFA_POLIGONI (1–8, the area classes). The drawing keeps its own file."
                    ),
                );
                if st.file_type == FileType::DocfaDwg {
                    note(ui, crate::tl!("DOCFA reads the surface polygons from DXF files only."));
                }
            } else if !st.file_type.versioned() && st.file_type != FileType::Dwt {
                note(ui, crate::tl!("An export of the drawing as shown: the drawing keeps its own file."));
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    save_clicked = ui.button(crate::tl!("Save...")).clicked();
                    cancel = ui.button(crate::tl!("Cancel")).clicked();
                });
            });
        });
    app.saveas = Some(st);
    // Closed once saved, or when there is no picker to ask (the command line says how to save).
    if cancel || (save_clicked && save(app) != Some(false)) {
        *open = false;
    }
    if !*open {
        app.saveas = None;
    }
}

/// A version's label: "AutoCAD 2018".
pub fn version_label(v: SaveVersion) -> String {
    format!("AutoCAD {}", v.year())
}

#[cfg(test)]
mod tests;
