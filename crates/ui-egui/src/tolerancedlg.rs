//! Geometric Tolerance dialog (TOLERANCE, -TOLERANCE, TOLERANCE.EDIT).
//!
//! Provides the AutoCAD-standard Geometric Tolerance dialog for building and editing
//! feature control frames with characteristics, tolerances, datums, projected tolerance zones,
//! and datum identifiers.

use cadcraft_doc::{GdtDatum, GdtFrame, GdtLine, GdtSymbol, GdtTol, Handle, compose_frame, deconstruct_frame};
use serde_json::json;

use crate::CadApp;
use crate::theme::Tokens;

/// The 14 standard GD&T geometric characteristics and their symbol glyphs.
pub const CHARACTERISTICS: [(GdtSymbol, &str, &str); 14] = [
    (GdtSymbol::Position, "Position", "⌖"),
    (GdtSymbol::Concentricity, "Concentricity", "◎"),
    (GdtSymbol::Symmetry, "Symmetry", "⌯"),
    (GdtSymbol::Parallelism, "Parallelism", "∥"),
    (GdtSymbol::Perpendicularity, "Perpendicularity", "⊥"),
    (GdtSymbol::Angularity, "Angularity", "∠"),
    (GdtSymbol::Cylindricity, "Cylindricity", "⌭"),
    (GdtSymbol::Flatness, "Flatness", "⏢"),
    (GdtSymbol::Circularity, "Circularity", "○"),
    (GdtSymbol::Straightness, "Straightness", "—"),
    (GdtSymbol::SurfaceProfile, "Surface Profile", "⌓"),
    (GdtSymbol::LineProfile, "Line Profile", "⌒"),
    (GdtSymbol::CircularRunout, "Circular Runout", "↗"),
    (GdtSymbol::TotalRunout, "Total Runout", "⌰"),
];

/// Glyph for a geometric characteristic.
pub fn sym_glyph(s: GdtSymbol) -> &'static str {
    CHARACTERISTICS.iter().find(|(sym, _, _)| *sym == s).map(|(_, _, g)| *g).unwrap_or("⌖")
}

/// The state of the Geometric Tolerance dialog while open.
#[derive(Clone, Debug, PartialEq)]
pub struct ToleranceDlgState {
    pub edit_handle: Option<Handle>,
    pub line1: GdtLine,
    pub line2: GdtLine,
    pub height: String,
    pub projected: bool,
    pub datum_id: String,
    pub location: [f64; 2],
    pub sym_picker_row: Option<usize>,
    pub error: Option<String>,
}

impl Default for ToleranceDlgState {
    fn default() -> Self {
        ToleranceDlgState {
            edit_handle: None,
            line1: GdtLine {
                symbol: Some(GdtSymbol::Position),
                tol1: Some(GdtTol { diameter: false, value: String::new(), material: None }),
                tol2: None,
                datums: Vec::new(),
            },
            line2: GdtLine::default(),
            height: String::new(),
            projected: false,
            datum_id: String::new(),
            location: [0.0, 0.0],
            sym_picker_row: None,
            error: None,
        }
    }
}

/// Open the dialog in create mode.
pub fn open_create(app: &mut CadApp) {
    app.tolerance_dlg = Some(ToleranceDlgState::default());
    app.ui.dialog = Some("tolerance".into());
}

/// Open the dialog in edit mode for an existing tolerance entity.
pub fn open_edit(app: &mut CadApp, h: Handle) {
    let mut st = ToleranceDlgState { edit_handle: Some(h), ..Default::default() };
    let tol = app.session.doc().ok().and_then(|d| d.entity(h)).and_then(|e| match &e.kind {
        cadcraft_doc::EntityKind::Tolerance(t) => Some(t),
        _ => None,
    });
    if let Some(t) = tol {
        st.location = [t.insert.x, t.insert.y];
        let f = deconstruct_frame(&t.text);
        if let Some(l) = f.lines.first() {
            st.line1 = l.clone();
        }
        if let Some(l) = f.lines.get(1) {
            st.line2 = l.clone();
        }
        st.height = f.height;
        st.projected = f.projected;
        st.datum_id = f.datum_id;
    }
    app.tolerance_dlg = Some(st);
    app.ui.dialog = Some("tolerance".into());
}

/// Render the Geometric Tolerance dialog window.
pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let Some(mut st) = app.tolerance_dlg.clone() else {
        *open = false;
        return;
    };

    let title = if st.edit_handle.is_some() { crate::tl!("Geometric Tolerance - Edit") } else { crate::tl!("Geometric Tolerance") };

    let mut is_open = true;
    let mut cancel = false;
    let mut apply = false;

    egui::Window::new(title).id(egui::Id::new("Geometric Tolerance Window")).open(&mut is_open).resizable(true).default_width(620.0).show(
        ctx,
        |ui| {
            if let Some(err) = &st.error {
                ui.colored_label(Tokens::get().warn, err);
                ui.separator();
            }

            ui.strong(crate::tl!("Tolerance Frame Lines"));
            ui.add_space(4.0);

            // Row 1
            ui.group(|ui| {
                ui.label(crate::tl!("Line 1"));
                render_frame_line(ui, 1, &mut st.line1, &mut st.sym_picker_row);
            });

            ui.add_space(4.0);

            // Row 2
            ui.group(|ui| {
                ui.label(crate::tl!("Line 2"));
                render_frame_line(ui, 2, &mut st.line2, &mut st.sym_picker_row);
            });

            // Symbol picker modal / sub-group if active
            if let Some(row) = st.sym_picker_row {
                ui.separator();
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.strong(crate::tf!("Select Characteristic (Line {row})", row = row));
                        if ui.button(crate::tl!("None")).clicked() {
                            if row == 1 {
                                st.line1.symbol = None;
                            } else {
                                st.line2.symbol = None;
                            }
                            st.sym_picker_row = None;
                        }
                        if ui.button(crate::tl!("Close")).clicked() {
                            st.sym_picker_row = None;
                        }
                    });
                    ui.add_space(4.0);
                    egui::Grid::new("sym_picker_grid").spacing([8.0, 4.0]).show(ui, |ui| {
                        for (idx, (sym, name, glyph)) in CHARACTERISTICS.iter().enumerate() {
                            let label = format!("{glyph} {name}");
                            if ui.button(label).clicked() {
                                if row == 1 {
                                    st.line1.symbol = Some(*sym);
                                } else {
                                    st.line2.symbol = Some(*sym);
                                }
                                st.sym_picker_row = None;
                            }
                            if (idx + 1) % 4 == 0 {
                                ui.end_row();
                            }
                        }
                        ui.end_row();
                    });
                });
            }

            ui.separator();

            // Projected tolerance zone & Datum identifier
            ui.horizontal(|ui| {
                ui.group(|ui| {
                    ui.strong(crate::tl!("Projected Tolerance Zone"));
                    ui.horizontal(|ui| {
                        ui.label(crate::tl!("Height:"));
                        ui.text_edit_singleline(&mut st.height);
                        ui.checkbox(&mut st.projected, "Ⓟ");
                    });
                });

                ui.group(|ui| {
                    ui.strong(crate::tl!("Datum Identifier"));
                    ui.horizontal(|ui| {
                        ui.label(crate::tl!("Identifier:"));
                        ui.text_edit_singleline(&mut st.datum_id);
                    });
                });
            });

            // Location coordinates (if create mode)
            if st.edit_handle.is_none() {
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(crate::tl!("Insertion Point:"));
                    ui.label("X:");
                    ui.add(egui::DragValue::new(&mut st.location[0]).speed(0.1));
                    ui.label("Y:");
                    ui.add(egui::DragValue::new(&mut st.location[1]).speed(0.1));
                });
            }

            ui.separator();

            // Action buttons
            ui.horizontal(|ui| {
                if ui.button(crate::tl!("OK")).clicked() {
                    apply = true;
                }
                if ui.button(crate::tl!("Cancel")).clicked() {
                    cancel = true;
                }
            });
        },
    );

    if cancel || !is_open {
        *open = false;
        app.tolerance_dlg = None;
        if app.ui.dialog.as_deref() == Some("tolerance") {
            app.ui.dialog = None;
        }
        return;
    }

    if apply {
        let mut lines = Vec::new();
        let used = |l: &cadcraft_doc::GdtLine| l.symbol.is_some() || l.tol1.is_some() || l.tol2.is_some() || !l.datums.is_empty();
        if used(&st.line1) {
            lines.push(st.line1.clone());
        }
        if used(&st.line2) {
            lines.push(st.line2.clone());
        }

        let f = GdtFrame { lines, height: st.height.clone(), projected: st.projected, datum_id: st.datum_id.clone() };
        let frame_str = compose_frame(&f);
        if cadcraft_doc::parse_frame(&frame_str).iter().flatten().all(cadcraft_doc::compartment_is_empty) {
            st.error = Some(crate::tl!("The feature control frame is empty.").into());
            app.tolerance_dlg = Some(st);
            return;
        }

        if let Some(h) = st.edit_handle {
            let res = app.run(
                "tolerance",
                json!({
                    "handle": h.hex(),
                    "text": frame_str,
                }),
            );
            if let Err(e) = res {
                st.error = Some(e);
                app.tolerance_dlg = Some(st);
                return;
            }
        } else {
            let res = app.run(
                "tolerance",
                json!({
                    "at": st.location,
                    "text": frame_str,
                }),
            );
            if let Err(e) = res {
                st.error = Some(e);
                app.tolerance_dlg = Some(st);
                return;
            }
        }

        *open = false;
        app.tolerance_dlg = None;
        app.ui.dialog = None;
        return;
    }

    if *open {
        app.tolerance_dlg = Some(st);
    } else {
        app.tolerance_dlg = None;
        if app.ui.dialog.as_deref() == Some("tolerance") {
            app.ui.dialog = None;
        }
    }
}

fn render_frame_line(ui: &mut egui::Ui, row_idx: usize, line: &mut GdtLine, picker_row: &mut Option<usize>) {
    ui.horizontal(|ui| {
        // Characteristic Symbol button
        let sym_text = match line.symbol {
            Some(s) => format!("{} {}", sym_glyph(s), s.name()),
            None => crate::tl!("(Sym)").into(),
        };
        if ui.button(sym_text).clicked() {
            *picker_row = Some(row_idx);
        }

        ui.separator();

        // Tolerance 1
        ui.label(crate::tl!("Tol 1:"));
        let mut t1 = line.tol1.clone().unwrap_or_default();
        let mut t1_changed = false;
        if ui.checkbox(&mut t1.diameter, "Ø").changed() {
            t1_changed = true;
        }
        if ui.add(egui::TextEdit::singleline(&mut t1.value).desired_width(60.0)).changed() {
            t1_changed = true;
        }
        if render_material_combo(ui, format!("t1_mat_{row_idx}"), &mut t1.material) {
            t1_changed = true;
        }
        if t1_changed {
            line.tol1 = if t1.value.trim().is_empty() && !t1.diameter && t1.material.is_none() { None } else { Some(t1) };
        }

        ui.separator();

        // Tolerance 2
        ui.label(crate::tl!("Tol 2:"));
        let mut t2 = line.tol2.clone().unwrap_or_default();
        let mut t2_changed = false;
        if ui.checkbox(&mut t2.diameter, "Ø").changed() {
            t2_changed = true;
        }
        if ui.add(egui::TextEdit::singleline(&mut t2.value).desired_width(60.0)).changed() {
            t2_changed = true;
        }
        if render_material_combo(ui, format!("t2_mat_{row_idx}"), &mut t2.material) {
            t2_changed = true;
        }
        if t2_changed {
            line.tol2 = if t2.value.trim().is_empty() && !t2.diameter && t2.material.is_none() { None } else { Some(t2) };
        }
    });

    ui.horizontal(|ui| {
        // Datums 1..3
        for d_idx in 0..3 {
            ui.label(crate::tf!("Datum {n}:", n = d_idx + 1));
            let mut datum = line.datums.get(d_idx).cloned().unwrap_or_default();
            let mut d_changed = false;
            if ui.add(egui::TextEdit::singleline(&mut datum.letter).desired_width(30.0)).changed() {
                d_changed = true;
            }
            if render_material_combo(ui, format!("d{d_idx}_mat_{row_idx}"), &mut datum.material) {
                d_changed = true;
            }
            if d_changed {
                while line.datums.len() <= d_idx {
                    line.datums.push(GdtDatum::default());
                }
                if let Some(slot) = line.datums.get_mut(d_idx) {
                    *slot = datum;
                }
            }
        }
    });
}

fn render_material_combo(ui: &mut egui::Ui, id_salt: String, mat: &mut Option<String>) -> bool {
    let mut changed = false;
    let label = match mat.as_deref() {
        Some("M") => "(M)",
        Some("L") => "(L)",
        Some("S") => "(S)",
        _ => "-",
    };
    egui::ComboBox::from_id_salt(id_salt).selected_text(label).width(45.0).show_ui(ui, |ui| {
        if ui.selectable_label(mat.is_none(), "-").clicked() {
            *mat = None;
            changed = true;
        }
        if ui.selectable_label(mat.as_deref() == Some("M"), "(M)").clicked() {
            *mat = Some("M".into());
            changed = true;
        }
        if ui.selectable_label(mat.as_deref() == Some("L"), "(L)").clicked() {
            *mat = Some("L".into());
            changed = true;
        }
        if ui.selectable_label(mat.as_deref() == Some("S"), "(S)").clicked() {
            *mat = Some("S".into());
            changed = true;
        }
    });
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Services;
    use cadcraft_engine::Session;

    fn app_with_drawing() -> CadApp {
        CadApp::new(Session::new(), Services::default())
    }

    #[test]
    fn open_create_and_edit_tolerance_dialog() {
        let mut app = app_with_drawing();
        open_create(&mut app);
        assert_eq!(app.ui.dialog.as_deref(), Some("tolerance"));
        let st = app.tolerance_dlg.clone().expect("dialog state");
        assert!(st.edit_handle.is_none());
        assert_eq!(st.line1.symbol, Some(GdtSymbol::Position));

        // Create a tolerance in the drawing
        let r = app
            .session
            .execute(
                "tolerance",
                &json!({
                    "at": [10, 20],
                    "symbol": "flatness",
                    "tol1": "0.05",
                    "datums": ["A"],
                }),
            )
            .unwrap();
        let h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();

        // Open edit dialog
        open_edit(&mut app, h);
        let st_edit = app.tolerance_dlg.clone().expect("dialog state");
        assert_eq!(st_edit.edit_handle, Some(h));
        assert_eq!(st_edit.line1.symbol, Some(GdtSymbol::Flatness));
        assert_eq!(st_edit.line1.tol1.as_ref().map(|t| t.value.as_str()), Some("0.05"));
        assert_eq!(st_edit.line1.datums.first().map(|d| d.letter.as_str()), Some("A"));
    }
}
