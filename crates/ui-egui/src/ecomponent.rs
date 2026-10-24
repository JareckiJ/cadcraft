//! Insert / Edit Component: the dialog that follows placing a symbol by hand (and opens for a
//! selected symbol from Edit Component), like AutoCAD Electrical's. A main device gets its tag,
//! description lines, manufacturer and catalogue number, location and installation; a contact
//! picks its coil or main device from the project's list, which shows how many contacts each
//! already has and where. OK writes the data into the project database and the symbol
//! (EDB.DEVICE); Cancel keeps the symbol as placed.

use egui::{RichText, vec2};
use serde_json::{Value, json};

use crate::CadApp;
use crate::theme::Tokens;

#[derive(Clone, Default)]
struct Form {
    loaded_for: String,
    tag: String,
    original_tag: String,
    child: bool,
    family: String,
    block: String,
    reference: String,
    desc: [String; 3],
    manufacturer: String,
    catalog: String,
    location: String,
    installation: String,
    /// Main devices to pick from (contacts): tag, description, reference, contacts used.
    masters: Vec<(String, String, String, String)>,
    filter: String,
    error: Option<String>,
}

fn id() -> egui::Id {
    egui::Id::new("cadcraft-ecomponent")
}

fn s(v: &Value) -> String {
    v.as_str().unwrap_or("").to_string()
}

fn reference(d: &Value) -> String {
    let r = &d["graphic"]["reference"];
    match (r[0].as_u64(), r[1].as_u64()) {
        (Some(a), Some(b)) => format!("/{a}.{b}"),
        _ => String::new(),
    }
}

/// Main devices with their contacts so far, from the project database.
fn masters(app: &mut CadApp) -> Vec<(String, String, String, String)> {
    let Ok(v) = app.session.execute("edb.query", &json!({ "what": "devices" })) else { return Vec::new() };
    let all: Vec<&Value> = v.as_array().map(|a| a.iter().collect()).unwrap_or_default();
    let mut out: Vec<(String, String, String, String)> = all
        .iter()
        .filter(|d| !d["child"].as_bool().unwrap_or(false) && !matches!(d["family"].as_str(), Some("TERMINAL")) && !s(&d["tag"]).is_empty())
        .map(|d| {
            let tag = s(&d["tag"]);
            let used = |fam: &str| {
                all.iter().filter(|c| c["child"].as_bool().unwrap_or(false) && s(&c["tag"]) == tag && c["family"].as_str() == Some(fam)).count()
            };
            let (no, nc) = (used("NO"), used("NC"));
            let desc = d["part"]["description"].as_array().and_then(|l| l.first()).map(s).unwrap_or_default();
            let desc = if desc.is_empty() { s(&d["graphic"]["block"]).trim_start_matches("IEC_").replace('_', " ") } else { desc };
            (tag, desc, reference(d), format!("{no} NO, {nc} NC"))
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out.dedup_by(|a, b| a.0 == b.0);
    out
}

fn load(app: &mut CadApp, handle: &str) -> Form {
    let mut f = Form { loaded_for: handle.to_string(), ..Form::default() };
    match app.session.execute("edb.device", &json!({ "handle": handle })) {
        Ok(d) => {
            f.tag = s(&d["tag"]);
            f.original_tag = f.tag.clone();
            f.child = d["child"].as_bool().unwrap_or(false);
            f.family = s(&d["family"]);
            f.block = s(&d["graphic"]["block"]);
            f.reference = reference(&d);
            for (i, line) in d["part"]["description"].as_array().into_iter().flatten().take(3).enumerate() {
                if let Some(slot) = f.desc.get_mut(i) {
                    *slot = s(line);
                }
            }
            f.manufacturer = s(&d["part"]["manufacturer"]);
            f.catalog = s(&d["part"]["catalog"]);
            f.location = s(&d["location"]);
            f.installation = s(&d["installation"]);
        }
        Err(e) => f.error = Some(e.to_string()),
    }
    if f.child {
        f.masters = masters(app);
    }
    f
}

fn row(ui: &mut egui::Ui, label: &str, value: &mut String) {
    ui.label(crate::i18n::t(label));
    ui.add(egui::TextEdit::singleline(value).desired_width(260.0));
    ui.end_row();
}

/// The dialog for the symbol `handle`; `open` turns false when it closes.
pub fn dialog(app: &mut CadApp, ctx: &egui::Context, handle: &str, open: &mut bool) {
    let t = Tokens::get();
    let mut f: Form = ctx.data_mut(|d| d.get_temp(id())).unwrap_or_default();
    if f.loaded_for != handle {
        f = load(app, handle);
    }
    let title = if f.child { crate::tl!("Insert / Edit Contact") } else { crate::tl!("Insert / Edit Component") };
    let mut done = false;
    egui::Window::new(title).id(egui::Id::new("ecomponent-window")).collapsible(false).resizable(false).open(open).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(f.block.trim_start_matches("IEC_").replace('_', " ")).strong());
            ui.label(RichText::new(format!("{}  {}", f.family, f.reference)).color(t.text_dim));
        });
        ui.add_space(4.0);
        egui::Grid::new("ecomponent-grid").num_columns(2).spacing(vec2(8.0, 4.0)).show(ui, |ui| {
            row(ui, "Tag", &mut f.tag);
        });
        if f.child {
            ui.add_space(6.0);
            ui.label(RichText::new(crate::tl!("Coil / main device")).strong());
            ui.add(egui::TextEdit::singleline(&mut f.filter).hint_text(crate::tl!("Filter")).desired_width(f32::INFINITY));
            let filter = f.filter.trim().to_lowercase();
            egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                egui::Grid::new("ecomponent-masters").num_columns(4).striped(true).spacing(vec2(10.0, 2.0)).show(ui, |ui| {
                    for (tag, desc, at, used) in f.masters.clone() {
                        if !filter.is_empty() && !tag.to_lowercase().contains(&filter) && !desc.to_lowercase().contains(&filter) {
                            continue;
                        }
                        if ui.selectable_label(f.tag == tag, RichText::new(&tag).font(crate::theme::mono())).clicked() {
                            f.tag = tag.clone();
                        }
                        ui.label(desc);
                        ui.label(RichText::new(at).color(t.text_dim));
                        ui.label(RichText::new(used).color(t.text_dim));
                        ui.end_row();
                    }
                });
                if f.masters.is_empty() {
                    ui.label(RichText::new(crate::tl!("No coils or main devices yet: type the tag.")).color(t.text_dim));
                }
            });
        } else {
            ui.add_space(4.0);
            egui::Grid::new("ecomponent-data").num_columns(2).spacing(vec2(8.0, 4.0)).show(ui, |ui| {
                let [d1, d2, d3] = &mut f.desc;
                row(ui, "Description 1", d1);
                row(ui, "Description 2", d2);
                row(ui, "Description 3", d3);
                row(ui, "Manufacturer", &mut f.manufacturer);
                row(ui, "Catalogue no.", &mut f.catalog);
                row(ui, "Location (+)", &mut f.location);
                row(ui, "Installation (=)", &mut f.installation);
            });
        }
        if let Some(e) = &f.error {
            ui.colored_label(egui::Color32::from_rgb(220, 80, 70), e.as_str());
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.button(crate::tl!("OK")).clicked() {
                let mut set = json!({ "tag": f.tag.trim() });
                if !f.child {
                    set["description"] = json!(f.desc.iter().map(|d| d.trim()).collect::<Vec<_>>());
                    set["manufacturer"] = json!(f.manufacturer.trim());
                    set["catalog"] = json!(f.catalog.trim());
                    set["location"] = json!(f.location.trim());
                    set["installation"] = json!(f.installation.trim());
                }
                match app.run("edb.device", json!({ "handle": handle, "set": set })) {
                    Ok(_) => done = true,
                    Err(e) => f.error = Some(e),
                }
            }
            if ui.button(crate::tl!("Cancel")).clicked() {
                done = true;
            }
        });
    });
    if done {
        *open = false;
    }
    if *open {
        ctx.data_mut(|d| d.insert_temp(id(), f));
    } else {
        ctx.data_mut(|d| d.remove::<Form>(id()));
    }
}

/// Requests queued by the engine: open the component dialog for a symbol just placed.
pub fn take_requests(app: &mut CadApp) {
    if app.session.ui_requests.is_empty() {
        return;
    }
    let requests = std::mem::take(&mut app.session.ui_requests);
    for r in requests {
        if r["dialog"] == "ecomponent"
            && let Some(h) = r["handle"].as_str()
            && app.ui.ecomponent_dialog
        {
            app.ui.dialog = Some(format!("ecomponent:{h}"));
        }
    }
}
