//! Save as DXF or DWG compatible with DOCFA (the Italian cadastre): a small window that asks for
//! the drawing's unit (the file is in metres) before the file picker, then `export {docfa: true}`.
//! The export is a reduced copy beside the drawing, which keeps its own file (`docs/docfa.md`).

use egui::{RichText, vec2};
use serde_json::{Value, json};

use crate::CadApp;
use crate::theme::Tokens;

/// DXF or DWG.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Dxf,
    Dwg,
}

impl Format {
    fn ext(self) -> &'static str {
        match self {
            Format::Dxf => "dxf",
            Format::Dwg => "dwg",
        }
    }
}

/// The drawing's unit (the file is in metres).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    M,
    Cm,
    Mm,
}

impl Unit {
    const ALL: [Unit; 3] = [Unit::M, Unit::Cm, Unit::Mm];

    fn as_str(self) -> &'static str {
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

/// The window's choices until Save.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Export {
    pub format: Format,
    pub unit: Unit,
}

impl Export {
    /// The `export` parameters for `path`.
    pub fn params(&self, path: &str) -> Value {
        json!({ "path": path, "docfa": true, "unit": self.unit.as_str() })
    }

    /// The name the picker suggests: the drawing's name with `-docfa`, beside the drawing.
    pub fn suggested_name(&self, title: &str) -> String {
        let stem = std::path::Path::new(title).file_stem().map(|s| s.to_string_lossy().to_string()).filter(|s| !s.trim().is_empty());
        format!("{}-docfa.{}", stem.unwrap_or_else(|| "Drawing".into()), self.format.ext())
    }
}

/// Open the window for the active drawing, on `format`, with the drawing's own unit.
pub fn open(app: &mut CadApp, format: Format) {
    let unit = app.session.doc().map_or(Unit::M, |d| Unit::from_insunits(d.header.i64("INSUNITS", 0)));
    app.docfa = Some(Export { format, unit });
    app.ui.dialog = Some("docfa".into());
}

/// `path` with the format's extension when the picker gave none.
fn with_extension(path: &str, ext: &str) -> String {
    let has = std::path::Path::new(path).extension().is_some_and(|e| !e.is_empty());
    if has { path.to_string() } else { format!("{path}.{ext}") }
}

/// Ask where to save and export. `true` once written; `false` when the picker was cancelled,
/// there is none (said on the command line) or the export failed (said by `app.run`).
fn save(app: &mut CadApp, e: Export) -> bool {
    let title = app.session.state().map(|s| s.title.clone()).unwrap_or_default();
    let Some(pick) = app.services.pick_save.as_ref() else {
        app.session.echo(r#"There is no file dialog here: run export {"path": "plan-docfa.dxf", "docfa": true, "unit": "m"}"#.to_string());
        return false;
    };
    let Some(path) = pick(&e.suggested_name(&title)) else { return false };
    let path = with_extension(&path, e.format.ext());
    app.run("export", e.params(&path)).is_ok()
}

pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let Some(mut e) = app.docfa else {
        *open = false;
        return;
    };
    let (mut go, mut cancel) = (false, false);
    let t = Tokens::get();
    egui::Window::new(crate::tl!("Save as compatible with DOCFA"))
        .id(egui::Id::unique("docfa_export"))
        .open(open)
        .collapsible(false)
        .resizable(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| {
            ui.set_max_width(420.0);
            egui::Grid::new("docfa_grid").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
                ui.label(crate::tl!("File type"));
                ui.horizontal(|ui| {
                    ui.radio_value(&mut e.format, Format::Dxf, "DXF");
                    ui.radio_value(&mut e.format, Format::Dwg, "DWG");
                });
                ui.end_row();
                ui.label(crate::tl!("Drawing unit"));
                ui.horizontal(|ui| {
                    for u in Unit::ALL {
                        ui.radio_value(&mut e.unit, u, u.as_str());
                    }
                });
                ui.end_row();
            });
            let note = |ui: &mut egui::Ui, text: &str| ui.add(egui::Label::new(RichText::new(text).color(t.text_dim)).wrap());
            note(
                ui,
                crate::tl!(
                    "A reduced copy for DOCFA, in metres (AutoCAD 2000): model space only, blocks exploded, no hatches, ASCII text, colours only on the closed polylines of layer DOCFA_POLIGONI (1–8, the area classes). The drawing keeps its own file."
                ),
            );
            if e.format == Format::Dwg {
                note(ui, crate::tl!("DOCFA reads the surface polygons from DXF files only."));
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    go = ui.button(crate::tl!("Save...")).clicked();
                    cancel = ui.button(crate::tl!("Cancel")).clicked();
                });
            });
        });
    app.docfa = Some(e);
    if cancel || (go && save(app, e)) {
        *open = false;
    }
    if !*open {
        app.docfa = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_export_names_the_file_apart_and_carries_the_unit() {
        let e = Export { format: Format::Dwg, unit: Unit::Cm };
        assert_eq!(e.suggested_name("Plan.dwg"), "Plan-docfa.dwg");
        assert_eq!(Export { format: Format::Dxf, ..e }.suggested_name(""), "Drawing-docfa.dxf");
        assert_eq!(e.params("/tmp/p.dwg"), json!({ "path": "/tmp/p.dwg", "docfa": true, "unit": "cm" }));
        assert_eq!(with_extension("/tmp/p", "dxf"), "/tmp/p.dxf");
        assert_eq!(with_extension("/tmp/p.dxf", "dxf"), "/tmp/p.dxf");
        assert_eq!([4, 5, 6, 0, 1].map(Unit::from_insunits), [Unit::Mm, Unit::Cm, Unit::M, Unit::M, Unit::M]);
    }
}
