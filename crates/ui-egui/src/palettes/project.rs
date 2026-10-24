//! The Project Manager: the open project's sheets in order (click to select, double-click to
//! open), moving them up and down, renaming and removing them, each sheet's title block fields,
//! the project's own properties (shared by every sheet's title block) and recent projects.
//! After the sheet order changes it offers to renumber tags and wire numbers for the new order.
//! A thin face on the EPROJECT.* commands.

use egui::{RichText, vec2};
use serde_json::{Map, Value, json};

use crate::CadApp;
use crate::theme::Tokens;

/// How often the open project is looked up (seconds). The sheet list itself is read again only
/// when the project changes, after an action, or on Refresh (it reads every sheet's title block).
const POLL_SECONDS: f64 = 2.0;

/// Title block fields edited per sheet, and those the project shares.
const SHEET_FIELDS: &[(&str, &str)] =
    &[("TITLE", "Title"), ("DRAWINGNO", "Drawing no."), ("DATE", "Date"), ("REV", "Rev."), ("DRAWNBY", "Drawn"), ("CHECKEDBY", "Checked")];
const PROJECT_FIELDS: &[(&str, &str)] =
    &[("CUSTOMER", "Customer"), ("DRAWNBY", "Drawn"), ("CHECKEDBY", "Checked"), ("DATE", "Date"), ("REV", "Rev.")];

#[derive(Clone, Default)]
struct Sheet {
    sheet: u64,
    drawing: String,
    path: String,
    exists: bool,
    fields: Map<String, Value>,
}

#[derive(Clone, Default)]
struct State {
    project: Option<String>,
    name: String,
    sheets: Vec<Sheet>,
    selected: Option<usize>,
    polled_at: Option<f64>,
    stale: bool,
    /// Edits in progress.
    sheet_edit: Map<String, Value>,
    sheet_edit_for: Option<usize>,
    project_edit: Map<String, Value>,
    project_name: String,
    rename: Option<String>,
    confirm_remove: bool,
    /// Offer to renumber after the order changed.
    offer_renumber: bool,
    error: Option<String>,
}

fn id() -> egui::Id {
    egui::Id::new("cadcraft-project-manager")
}

pub fn open(app: &mut CadApp) -> Result<Value, String> {
    crate::docking::command(app, false, &json!({ "operation": "open", "panel": crate::docking::Panel::Projects.id() }))
}

fn text(v: &Value) -> String {
    v.as_str().unwrap_or("").to_string()
}

impl State {
    fn poll(&mut self, app: &mut CadApp) {
        let current = app.session.execute("eproject.list", &json!({ "quiet": true })).ok().and_then(|v| v["project"].as_str().map(str::to_string));
        if current != self.project {
            self.project = current;
            self.selected = None;
            self.sheet_edit_for = None;
            self.offer_renumber = false;
            self.stale = true;
        }
        if let Some(p) = self.project.clone() {
            app.remember_project(&p);
        }
    }

    fn reload(&mut self, app: &mut CadApp) {
        self.stale = false;
        let Some(project) = self.project.clone() else {
            self.sheets.clear();
            return;
        };
        match app.session.execute("eproject.sheets", &json!({ "project": project })) {
            Ok(v) => {
                self.name = text(&v["name"]);
                self.project_name = self.name.clone();
                self.project_edit = v["properties"].as_object().cloned().unwrap_or_default();
                self.sheets = v["sheets"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|x| Sheet {
                        sheet: x["sheet"].as_u64().unwrap_or(0),
                        drawing: text(&x["drawing"]),
                        path: text(&x["path"]),
                        exists: x["exists"].as_bool().unwrap_or(false),
                        fields: x["fields"].as_object().cloned().unwrap_or_default(),
                    })
                    .collect();
                if self.selected.is_some_and(|i| i >= self.sheets.len()) {
                    self.selected = None;
                }
                self.sheet_edit_for = None;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    /// Run a project command; on success the list is read again.
    fn act(&mut self, app: &mut CadApp, cmd: &str, mut params: Value) -> Option<Value> {
        if let Some(p) = &self.project {
            params["project"] = json!(p);
        }
        match app.run(cmd, params) {
            Ok(v) => {
                self.error = None;
                self.stale = true;
                Some(v)
            }
            Err(e) => {
                self.error = Some(e);
                None
            }
        }
    }
}

/// Open sheet `path` (or switch to it when it is open).
fn open_sheet(app: &mut CadApp, path: &str) {
    let full = std::path::Path::new(path);
    match app.session.docs.iter().position(|d| d.path.as_deref().is_some_and(|q| std::path::Path::new(q) == full)) {
        Some(i) => {
            let _ = app.run("document.switch", json!({ "index": i }));
        }
        None => app.open_path(path),
    }
}

/// Text boxes for `fields` of `values`.
fn field_grid(ui: &mut egui::Ui, salt: &str, fields: &[(&str, &str)], values: &mut Map<String, Value>) {
    egui::Grid::new(salt).num_columns(2).spacing(vec2(6.0, 3.0)).show(ui, |ui| {
        for (key, label) in fields {
            ui.label(crate::i18n::t(label));
            let mut v = values.get(*key).and_then(Value::as_str).unwrap_or("").to_string();
            if ui.add(egui::TextEdit::singleline(&mut v).desired_width(f32::INFINITY)).changed() {
                values.insert((*key).to_string(), json!(v));
            }
            ui.end_row();
        }
    });
}

pub fn show(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let mut st: State = ui.ctx().data_mut(|d| d.get_temp(id())).unwrap_or_default();
    let now = ui.input(|i| i.time);
    if st.polled_at.is_none_or(|at| now - at > POLL_SECONDS) {
        st.poll(app);
        st.polled_at = Some(now);
    }
    if st.stale {
        st.reload(app);
    }
    ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(POLL_SECONDS));

    egui::ScrollArea::vertical().id_salt("dock-project").auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space(4.0);
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        ui.horizontal_wrapped(|ui| {
            if ui.button(crate::tl!("Open Project...")).clicked() {
                app.start("eproject.open");
                st.polled_at = None;
            }
            if ui.button(crate::tl!("New Project...")).clicked() {
                app.start("eproject.new");
                st.polled_at = None;
            }
            let recent: Vec<String> = app.ui.recent_projects.clone();
            ui.add_enabled_ui(!recent.is_empty(), |ui| {
                ui.menu_button(crate::tl!("Recent"), |ui| {
                    for p in &recent {
                        let label = std::path::Path::new(p).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_else(|| p.clone());
                        if ui.button(label).on_hover_text(p.as_str()).clicked() {
                            let _ = app.run("eproject.open", json!({ "path": p }));
                            st.polled_at = None;
                            ui.close();
                        }
                    }
                });
            });
        });
        if let Some(e) = &st.error {
            ui.colored_label(egui::Color32::from_rgb(220, 80, 70), e.as_str());
        }
        let Some(_) = st.project.clone() else {
            ui.add_space(8.0);
            ui.label(RichText::new(crate::tl!("No project open.")).color(t.text_dim));
            return;
        };

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(st.name.as_str()).strong().size(15.0));
            if ui.small_button("⟳").on_hover_text(crate::tl!("Refresh")).clicked() {
                st.stale = true;
            }
        });

        // ---------- Renumber offer ----------
        if st.offer_renumber {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.label(crate::tl!("The sheet order changed. Renumber device tags and wire numbers for the new sheet numbers?"));
                ui.horizontal(|ui| {
                    if ui.button(crate::tl!("Renumber")).clicked() {
                        st.offer_renumber = false;
                        if st.act(app, "eproject.tag", json!({ "mode": "all" })).is_some()
                            && st.act(app, "eproject.wires", json!({ "mode": "all" })).is_some()
                        {
                            let _ = st.act(app, "eproject.xref", json!({}));
                        }
                    }
                    if ui.button(crate::tl!("Keep tags")).clicked() {
                        st.offer_renumber = false;
                    }
                });
            });
        }

        // ---------- Sheets ----------
        egui::CollapsingHeader::new(RichText::new(crate::tl!("Sheets")).strong()).id_salt("pm-sheets").default_open(true).show(ui, |ui| {
            let mut open_path = None;
            let rows = st.sheets.clone();
            for (i, sh) in rows.iter().enumerate() {
                let title = sh.fields.get("TITLE").and_then(Value::as_str).unwrap_or("");
                let open =
                    app.session.docs.iter().any(|d| d.path.as_deref().is_some_and(|q| std::path::Path::new(q) == std::path::Path::new(&sh.path)));
                let label =
                    format!("{:>3}  {}{}", sh.sheet, if title.is_empty() { sh.drawing.as_str() } else { title }, if open { "  ●" } else { "" });
                let mut text = RichText::new(label).font(crate::theme::mono());
                if !sh.exists {
                    text = text.color(t.text_dim).strikethrough();
                }
                let r = ui.selectable_label(st.selected == Some(i), text).on_hover_text(format!("{}\n{}", sh.drawing, sh.path));
                if r.clicked() {
                    st.selected = Some(i);
                    st.rename = None;
                    st.confirm_remove = false;
                }
                if r.double_clicked() && sh.exists {
                    open_path = Some(sh.path.clone());
                }
            }
            if let Some(p) = open_path {
                open_sheet(app, &p);
            }
            ui.horizontal_wrapped(|ui| {
                let sel = st.selected;
                let n = st.sheets.len();
                if ui.add_enabled(sel.is_some_and(|i| i > 0), egui::Button::new("▲")).on_hover_text(crate::tl!("Move Up")).clicked()
                    && let Some(i) = sel
                    && st.act(app, "eproject.move", json!({ "sheet": i + 1, "to": i })).is_some()
                {
                    st.selected = Some(i - 1);
                    st.offer_renumber = true;
                }
                if ui.add_enabled(sel.is_some_and(|i| i + 1 < n), egui::Button::new("▼")).on_hover_text(crate::tl!("Move Down")).clicked()
                    && let Some(i) = sel
                    && st.act(app, "eproject.move", json!({ "sheet": i + 1, "to": i + 2 })).is_some()
                {
                    st.selected = Some(i + 1);
                    st.offer_renumber = true;
                }
                if ui.add_enabled(sel.is_some(), egui::Button::new(crate::tl!("Open"))).clicked()
                    && let Some(sh) = sel.and_then(|i| st.sheets.get(i))
                {
                    let p = sh.path.clone();
                    open_sheet(app, &p);
                }
                if ui.add_enabled(sel.is_some(), egui::Button::new(crate::tl!("Rename..."))).clicked()
                    && let Some(sh) = sel.and_then(|i| st.sheets.get(i))
                {
                    let stem = std::path::Path::new(&sh.drawing).file_stem().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
                    st.rename = Some(stem);
                    st.confirm_remove = false;
                }
                if ui.add_enabled(sel.is_some(), egui::Button::new(crate::tl!("Remove..."))).clicked() {
                    st.confirm_remove = true;
                    st.rename = None;
                }
                if ui.button(crate::tl!("Add New Sheet")).clicked() {
                    let _ = st.act(app, "eproject.addsheet", json!({}));
                }
            });
            if let (Some(i), Some(mut name)) = (st.selected, st.rename.clone()) {
                ui.horizontal(|ui| {
                    ui.label(crate::tl!("New file name"));
                    let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(140.0));
                    st.rename = Some(name.clone());
                    let go = ui.button(crate::tl!("Rename")).clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                    if go && st.act(app, "eproject.rename", json!({ "sheet": i + 1, "name": name })).is_some() {
                        st.rename = None;
                    }
                    if ui.button(crate::tl!("Cancel")).clicked() {
                        st.rename = None;
                    }
                });
            }
            if st.confirm_remove
                && let Some(i) = st.selected
            {
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!("{} {}? {}", crate::tl!("Remove sheet"), i + 1, crate::tl!("The file is kept on disk.")));
                    if ui.button(crate::tl!("Remove")).clicked() {
                        st.confirm_remove = false;
                        if let Some(v) = st.act(app, "eproject.remove", json!({ "sheet": i + 1 })) {
                            st.selected = None;
                            st.offer_renumber = v["renumberSuggested"].as_bool().unwrap_or(false);
                        }
                    }
                    if ui.button(crate::tl!("Cancel")).clicked() {
                        st.confirm_remove = false;
                    }
                });
            }
        });

        // ---------- Sheet properties ----------
        if let Some(i) = st.selected
            && let Some(sh) = st.sheets.get(i).cloned()
        {
            egui::CollapsingHeader::new(RichText::new(format!("{} {}", crate::tl!("Sheet"), sh.sheet)).strong())
                .id_salt("pm-sheet-props")
                .default_open(true)
                .show(ui, |ui| {
                    if st.sheet_edit_for != Some(i) {
                        st.sheet_edit = sh.fields.clone();
                        st.sheet_edit_for = Some(i);
                    }
                    if sh.fields.is_empty() {
                        ui.label(RichText::new(crate::tl!("This sheet has no title block.")).color(t.text_dim));
                        return;
                    }
                    let mut edit = std::mem::take(&mut st.sheet_edit);
                    field_grid(ui, "pm-sheet-grid", SHEET_FIELDS, &mut edit);
                    let changed: Map<String, Value> = SHEET_FIELDS
                        .iter()
                        .filter(|(k, _)| edit.get(*k) != sh.fields.get(*k) && edit.get(*k).is_some())
                        .filter_map(|(k, _)| edit.get(*k).map(|v| ((*k).to_string(), v.clone())))
                        .collect();
                    st.sheet_edit = edit;
                    if ui.add_enabled(!changed.is_empty(), egui::Button::new(crate::tl!("Apply"))).clicked() {
                        let _ = st.act(app, "eproject.sheet", json!({ "sheet": i + 1, "set": changed }));
                    }
                });
        }

        // ---------- Project properties ----------
        egui::CollapsingHeader::new(RichText::new(crate::tl!("Project properties")).strong()).id_salt("pm-project-props").default_open(false).show(
            ui,
            |ui| {
                ui.horizontal(|ui| {
                    ui.label(crate::tl!("Name"));
                    ui.add(egui::TextEdit::singleline(&mut st.project_name).desired_width(f32::INFINITY));
                });
                let mut edit = std::mem::take(&mut st.project_edit);
                field_grid(ui, "pm-project-grid", PROJECT_FIELDS, &mut edit);
                st.project_edit = edit;
                ui.label(RichText::new(crate::tl!("Written into every sheet's title block.")).color(t.text_dim).font(crate::theme::small()));
                if ui.button(crate::tl!("Apply")).clicked() {
                    let set: Map<String, Value> =
                        PROJECT_FIELDS.iter().map(|(k, _)| ((*k).to_string(), st.project_edit.get(*k).cloned().unwrap_or(json!("")))).collect();
                    let name = st.project_name.trim().to_string();
                    let mut params = json!({ "set": set });
                    if !name.is_empty() && name != st.name {
                        params["name"] = json!(name);
                    }
                    let _ = st.act(app, "eproject.props", params);
                }
            },
        );
    });
    ui.ctx().data_mut(|d| d.insert_temp(id(), st));
}
