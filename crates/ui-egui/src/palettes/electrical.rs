//! The Electrical palette: the project (its sheets, click one to open it, and the whole-project
//! commands), the IEC symbol library as buttons (click one, then click in the drawing), and the
//! electrical tools. A thin face on the engine's commands.

use egui::{RichText, vec2};
use serde_json::{Value, json};

use crate::CadApp;
use crate::theme::Tokens;

/// How often the sheet list is read again while shown (seconds).
const REFRESH_SECONDS: f64 = 3.0;

#[derive(Clone, Default)]
struct State {
    symbols: Vec<(String, String, String, String)>,
    project: Option<Value>,
    read_at: Option<f64>,
    filter: String,
}

fn id() -> egui::Id {
    egui::Id::new("cadcraft-electrical-palette")
}

/// The symbol groups: (title, the symbols it takes by class letters and family).
fn group_of(class: &str, family: &str) -> &'static str {
    match (class.chars().next().unwrap_or(' '), family) {
        (_, "COIL") => "Coils and timers",
        (_, "NO" | "NC") if matches!(class.chars().next(), Some('K' | 'Q' | 'F')) => "Contacts",
        ('S', _) => "Push-buttons and switches",
        ('B', _) => "Sensors",
        ('F', _) | ('Q', _) => "Protection and switching",
        ('M' | 'T', _) => "Motors, drives, supplies",
        ('P', _) => "Lamps and signals",
        ('X', _) => "Terminals and connectors",
        _ => "Other",
    }
}

const GROUPS: &[&str] = &[
    "Coils and timers",
    "Contacts",
    "Push-buttons and switches",
    "Sensors",
    "Protection and switching",
    "Motors, drives, supplies",
    "Lamps and signals",
    "Terminals and connectors",
    "Other",
];

impl State {
    fn read(&mut self, app: &mut CadApp) {
        if self.symbols.is_empty()
            && let Ok(v) = app.session.execute("esymbol", &json!({ "list": true }))
        {
            let s = |x: &Value| x.as_str().unwrap_or("").to_string();
            self.symbols = v["symbols"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|d| (s(&d["name"]), s(&d["description"]), s(&d["class"]), s(&d["family"])))
                .collect();
        }
        self.project = app.session.execute("eproject.list", &json!({})).ok();
    }
}

/// Open the palette (docked on the left the first time, beside the Tool Sets).
pub fn open(app: &mut CadApp) -> Result<Value, String> {
    crate::docking::command(app, false, &json!({ "operation": "open", "panel": crate::docking::Panel::Electrical.id() }))
}

/// A tool button: runs `cmd` as if typed (its prompts follow on the command line).
fn tool(app: &mut CadApp, ui: &mut egui::Ui, label: &str, cmd: &str, hint: &str) {
    if ui.button(label).on_hover_text(hint).clicked() {
        app.start(cmd);
    }
}

/// A whole-project command: runs at once and reports on the command line.
fn project_cmd(app: &mut CadApp, ui: &mut egui::Ui, label: &str, cmd: &str, hint: &str) {
    if ui.button(label).on_hover_text(hint).clicked() {
        let _ = app.run(cmd, json!({}));
    }
}

pub fn show(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let mut st: State = ui.ctx().data_mut(|d| d.get_temp(id())).unwrap_or_default();
    let now = ui.input(|i| i.time);
    if st.read_at.is_none_or(|at| now - at > REFRESH_SECONDS) {
        st.read(app);
        st.read_at = Some(now);
    }
    ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(REFRESH_SECONDS));
    egui::ScrollArea::vertical().id_salt("dock-electrical").auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space(4.0);
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);

        // ---------- Project ----------
        egui::CollapsingHeader::new(RichText::new(crate::tl!("Project")).strong()).id_salt("el-project").default_open(true).show(ui, |ui| match &st
            .project
        {
            Some(p) => {
                let name = p["name"].as_str().unwrap_or("");
                ui.label(RichText::new(name).strong());
                let path = p["project"].as_str().unwrap_or("").to_string();
                let dir = std::path::Path::new(&path).parent().map(|d| d.to_path_buf()).unwrap_or_default();
                for d in p["drawings"].as_array().into_iter().flatten() {
                    let sheet = d["sheet"].as_u64().unwrap_or(0);
                    let rel = d["drawing"].as_str().unwrap_or("");
                    let full = if std::path::Path::new(rel).is_absolute() { std::path::PathBuf::from(rel) } else { dir.join(rel) };
                    let full_s = full.to_string_lossy().to_string();
                    let open_at = app.session.docs.iter().position(|x| x.path.as_deref().is_some_and(|q| std::path::Path::new(q) == full));
                    let active = open_at == Some(app.session.active);
                    let label = format!("{sheet:>3}  {rel}");
                    let r = ui.selectable_label(active, RichText::new(label).font(crate::theme::mono()));
                    if r.clicked() {
                        match open_at {
                            Some(i) => {
                                let _ = app.run("document.switch", json!({ "index": i }));
                            }
                            None => app.open_path(&full_s),
                        }
                    }
                }
                ui.horizontal_wrapped(|ui| {
                    project_cmd(app, ui, crate::tl!("Add New Sheet"), "eproject.addsheet", crate::tl!("A framed sheet at the end of the project"));
                    project_cmd(app, ui, crate::tl!("Tag"), "eproject.tag", crate::tl!("Tag new devices across the project"));
                    project_cmd(app, ui, crate::tl!("Wire numbers"), "eproject.wires", crate::tl!("Number new wires across the project"));
                    project_cmd(
                        app,
                        ui,
                        crate::tl!("Cross-references"),
                        "eproject.xref",
                        crate::tl!("Cross-reference coils and contacts across sheets"),
                    );
                    project_cmd(app, ui, crate::tl!("Database"), "eproject.sync", crate::tl!("Update the project database from every sheet"));
                    project_cmd(app, ui, crate::tl!("Check"), "echeck", crate::tl!("Duplicate tags, shorts, contacts without a coil, open pins"));
                });
            }
            None => {
                ui.label(RichText::new(crate::tl!("No project open.")).color(t.text_dim));
                ui.horizontal_wrapped(|ui| {
                    tool(app, ui, crate::tl!("Open Project..."), "eproject.open", crate::tl!("Open an .ecproj project"));
                    tool(app, ui, crate::tl!("New Project..."), "eproject.new", crate::tl!("A new project with a first sheet"));
                });
            }
        });

        // ---------- Symbols ----------
        egui::CollapsingHeader::new(RichText::new(crate::tl!("Symbols")).strong()).id_salt("el-symbols").default_open(true).show(ui, |ui| {
            ui.add(egui::TextEdit::singleline(&mut st.filter).hint_text(crate::tl!("Filter")).desired_width(f32::INFINITY));
            let filter = st.filter.trim().to_lowercase();
            for g in GROUPS {
                let items: Vec<&(String, String, String, String)> = st
                    .symbols
                    .iter()
                    .filter(|(_, _, c, f)| group_of(c, f) == *g)
                    .filter(|(n, d, _, _)| filter.is_empty() || n.to_lowercase().contains(&filter) || d.to_lowercase().contains(&filter))
                    .collect();
                if items.is_empty() {
                    continue;
                }
                ui.label(RichText::new(crate::i18n::t(g)).color(t.text_dim).font(crate::theme::small()));
                ui.horizontal_wrapped(|ui| {
                    for (name, desc, _, _) in items {
                        let short = name.trim_start_matches("IEC_").replace('_', " ");
                        if ui.button(RichText::new(short).font(crate::theme::small())).on_hover_text(desc.as_str()).clicked() {
                            // Insert Symbol with this name: the insertion point is picked next.
                            app.cmdline(&format!("esymbol {name}"));
                        }
                    }
                });
            }
        });

        // ---------- Tools ----------
        egui::CollapsingHeader::new(RichText::new(crate::tl!("Tools")).strong()).id_salt("el-tools").default_open(true).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                tool(app, ui, crate::tl!("Draw Wire"), "ewire", crate::tl!("Click pins or wires; the wire routes at right angles"));
                tool(app, ui, crate::tl!("Wire Type"), "ewiretype", crate::tl!("Colour and cross-section of new wires"));
                tool(app, ui, crate::tl!("Ladder"), "eladder", crate::tl!("Supply rails with a rung in every column"));
                project_cmd(
                    app,
                    ui,
                    crate::tl!("Remove Empty Rungs"),
                    "eladder.clean",
                    crate::tl!("Rungs nothing was placed on (they would short the rails)"),
                );
                tool(app, ui, crate::tl!("Cable"), "ecable", crate::tl!("A cable line across its wires"));
                tool(app, ui, crate::tl!("PLC card"), "eplc", crate::tl!("A digital or analog I/O card"));
                tool(app, ui, crate::tl!("Link Contacts"), "elink", crate::tl!("Give contacts their coil's tag"));
                tool(app, ui, crate::tl!("Symbol Builder"), "esymbuild", crate::tl!("Make a symbol from drawn geometry"));
            });
            ui.horizontal_wrapped(|ui| {
                tool(app, ui, crate::tl!("Tag Devices"), "etag", crate::tl!("IEC 81346 tags from sheet and column"));
                tool(app, ui, crate::tl!("Number Wires"), "ewirenum", crate::tl!("One number per network, at its start"));
                project_cmd(app, ui, crate::tl!("Number Terminals"), "etermnum", crate::tl!("Terminals numbered per strip"));
                project_cmd(app, ui, crate::tl!("Cross-References"), "exref", crate::tl!("Contacts and coils on this drawing"));
                project_cmd(app, ui, crate::tl!("Check Project"), "echeck", crate::tl!("Duplicate tags, shorts, contacts without a coil, open pins"));
            });
            ui.horizontal_wrapped(|ui| {
                if ui.button(crate::tl!("Devices")).on_hover_text(crate::tl!("Edit device data")).clicked() {
                    let _ = crate::palettes::devices::open(app);
                }
                if ui.button(crate::tl!("Community Hub")).on_hover_text(crate::tl!("Shared manufacturer parts")).clicked() {
                    let _ = crate::palettes::community::open(app);
                }
            });
            // Reports: saved as CSV files (open them in Excel).
            ui.label(RichText::new(crate::tl!("Reports (CSV)")).color(t.text_dim).font(crate::theme::small()));
            ui.horizontal_wrapped(|ui| {
                for (kind, label, file) in [
                    ("bom", crate::tl!("Parts list"), "parts-list.csv"),
                    ("wirelist", crate::tl!("Wire list"), "wire-list.csv"),
                    ("terminals", crate::tl!("Terminal plan"), "terminal-plan.csv"),
                    ("plcio", crate::tl!("PLC I/O"), "plc-io.csv"),
                    ("cables", crate::tl!("Cables"), "cables.csv"),
                    ("devices", crate::tl!("Device list"), "devices.csv"),
                ] {
                    if ui.button(label).clicked() {
                        match app.services.pick_save.as_ref().and_then(|f| f(file)) {
                            Some(path) => {
                                if let Ok(r) = app.run("ereport", json!({ "kind": kind, "path": path })) {
                                    app.set_status(format!("{} {} → {path}", r["rows"].as_array().map_or(0, Vec::len), crate::tl!("rows saved")));
                                }
                            }
                            None => {
                                let _ = app.run("ereport", json!({ "kind": kind }));
                            }
                        }
                    }
                }
            });
        });
    });
    ui.ctx().data_mut(|d| d.insert_temp(id(), st));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_library_symbol_has_a_group() {
        let mut s = cadcraft_engine::Session::new();
        let v = s.execute("esymbol", &json!({ "list": true })).unwrap();
        for d in v["symbols"].as_array().unwrap() {
            let g = group_of(d["class"].as_str().unwrap(), d["family"].as_str().unwrap());
            assert!(GROUPS.contains(&g));
        }
        assert_eq!(group_of("K", "COIL"), "Coils and timers");
        assert_eq!(group_of("K", "NO"), "Contacts");
        assert_eq!(group_of("S", "NC"), "Push-buttons and switches");
        assert_eq!(group_of("B", "NO"), "Sensors");
    }
}
