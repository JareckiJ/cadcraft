//! Dialogs for the electrical tools that otherwise prompt on the command line: Insert Ladder,
//! Wire Type and PLC I/O Card. Each fills in the command's parameters with clicks; the PLC card
//! is then placed with one click in the drawing (EPLACE). Symbols from the tool bar are placed
//! the same way, click after click until Enter, like AutoCAD Electrical.

use egui::{Color32, RichText, vec2};
use serde_json::json;

use crate::CadApp;
use crate::theme::Tokens;

/// IEC 60757 colour codes with their screen colours.
const COLOURS: &[(&str, &str, [u8; 3])] = &[
    ("BK", "Black", [30, 30, 30]),
    ("BN", "Brown", [140, 85, 40]),
    ("RD", "Red", [210, 40, 40]),
    ("OG", "Orange", [240, 140, 30]),
    ("YE", "Yellow", [235, 210, 40]),
    ("GN", "Green", [50, 160, 70]),
    ("BU", "Blue", [40, 100, 220]),
    ("DBU", "Dark blue", [20, 40, 140]),
    ("VT", "Violet", [140, 70, 190]),
    ("GY", "Grey", [140, 140, 140]),
    ("WH", "White", [240, 240, 240]),
    ("PK", "Pink", [240, 140, 180]),
    ("GNYE", "Green-yellow", [120, 190, 50]),
];
const SIZES: &[&str] = &["0.5", "0.75", "1", "1.5", "2.5", "4", "6", "10", "16", "25"];

#[derive(Clone)]
struct Ladder {
    top: f64,
    bottom: f64,
    from: f64,
    to: f64,
    spacing: f64,
    top_name: String,
    bottom_name: String,
    rungs: bool,
}

impl Default for Ladder {
    fn default() -> Self {
        Ladder { top: 260.0, bottom: 40.0, from: 0.0, to: 420.0, spacing: 42.0, top_name: "+24V".into(), bottom_name: "0V".into(), rungs: true }
    }
}

#[derive(Clone)]
struct WireType {
    colour: String,
    size: String,
    selected_only: bool,
}

impl Default for WireType {
    fn default() -> Self {
        WireType { colour: "BU".into(), size: "1".into(), selected_only: false }
    }
}

#[derive(Clone)]
struct Plc {
    kind: String,
    points: u64,
    address: String,
    description: String,
}

impl Default for Plc {
    fn default() -> Self {
        Plc { kind: "DI".into(), points: 8, address: String::new(), description: String::new() }
    }
}

fn get<T: Clone + Default + Send + Sync + 'static>(ctx: &egui::Context, key: &str) -> T {
    ctx.data_mut(|d| d.get_temp(egui::Id::new(("edialog", key)))).unwrap_or_default()
}

fn put<T: Clone + Send + Sync + 'static>(ctx: &egui::Context, key: &str, v: T) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(("edialog", key)), v));
}

fn number(ui: &mut egui::Ui, label: &str, v: &mut f64) {
    ui.label(crate::i18n::t(label));
    ui.add(egui::DragValue::new(v).speed(1.0).range(-100_000.0..=100_000.0));
    ui.end_row();
}

/// Place symbol `name` with clicks in the drawing (EPLACE), each followed by its dialog.
pub fn place_symbol(app: &mut CadApp, name: &str, label: &str) {
    app.session.place_preset = Some(json!({ "command": "esymbol", "params": { "name": name }, "label": label }));
    app.start("eplace");
}

pub fn ladder(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let mut f: Ladder = get(ctx, "ladder");
    let mut done = false;
    egui::Window::new(crate::tl!("Insert Ladder")).id(egui::Id::new("edialog-ladder")).collapsible(false).resizable(false).open(open).show(
        ctx,
        |ui| {
            egui::Grid::new("ladder-grid").num_columns(2).spacing(vec2(8.0, 4.0)).show(ui, |ui| {
                number(ui, "Top rail (y)", &mut f.top);
                number(ui, "Bottom rail (y)", &mut f.bottom);
                number(ui, "From (x)", &mut f.from);
                number(ui, "To (x)", &mut f.to);
                number(ui, "Rung spacing", &mut f.spacing);
                ui.label(crate::tl!("Top rail name"));
                ui.add(egui::TextEdit::singleline(&mut f.top_name).desired_width(120.0));
                ui.end_row();
                ui.label(crate::tl!("Bottom rail name"));
                ui.add(egui::TextEdit::singleline(&mut f.bottom_name).desired_width(120.0));
                ui.end_row();
            });
            ui.checkbox(&mut f.rungs, crate::tl!("A rung in every column"));
            ui.label(
                RichText::new(crate::tl!("Rungs nothing is placed on can be removed afterwards with Remove Empty Rungs."))
                    .color(Tokens::get().text_dim)
                    .size(11.0),
            );
            ui.horizontal(|ui| {
                if ui.button(crate::tl!("OK")).clicked() {
                    let mut p = json!({ "top": f.top, "bottom": f.bottom, "from": f.from, "to": f.to, "spacing": f.spacing, "rungs": f.rungs });
                    if !f.top_name.trim().is_empty() && !f.bottom_name.trim().is_empty() {
                        p["potentials"] = json!([f.top_name.trim(), f.bottom_name.trim()]);
                    }
                    if app.run("eladder", p).is_ok() {
                        done = true;
                    }
                }
                if ui.button(crate::tl!("Cancel")).clicked() {
                    done = true;
                }
            });
        },
    );
    put(ctx, "ladder", f);
    if done {
        *open = false;
    }
}

pub fn wire_type(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let t = Tokens::get();
    let mut f: WireType = get(ctx, "wiretype");
    let selection: Vec<String> = app.session.state().map(|st| st.selection.iter().map(|h| h.hex()).collect()).unwrap_or_default();
    let mut done = false;
    egui::Window::new(crate::tl!("Wire Type")).id(egui::Id::new("edialog-wiretype")).collapsible(false).resizable(false).open(open).show(ctx, |ui| {
        ui.label(RichText::new(crate::tl!("Colour")).strong());
        egui::Grid::new("wire-colours").num_columns(4).spacing(vec2(4.0, 4.0)).show(ui, |ui| {
            for (i, (code, name, rgb)) in COLOURS.iter().enumerate() {
                let selected = f.colour == *code;
                let r = ui
                    .add(
                        egui::Button::new(RichText::new(format!("● {code}")).color(Color32::from_rgb(rgb[0], rgb[1], rgb[2])))
                            .selected(selected)
                            .min_size(vec2(64.0, 22.0)),
                    )
                    .on_hover_text(crate::i18n::t(name));
                if r.clicked() {
                    f.colour = (*code).to_string();
                }
                if i % 4 == 3 {
                    ui.end_row();
                }
            }
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(crate::tl!("Cross-section (mm²)")).strong());
            egui::ComboBox::from_id_salt("wire-size").selected_text(f.size.clone()).show_ui(ui, |ui| {
                for s in SIZES {
                    ui.selectable_value(&mut f.size, (*s).to_string(), *s);
                }
            });
        });
        ui.add_enabled_ui(!selection.is_empty(), |ui| {
            ui.checkbox(&mut f.selected_only, format!("{} ({})", crate::tl!("Change the selected wires"), selection.len()));
        });
        ui.label(RichText::new(crate::tl!("Otherwise new wires are drawn with this type.")).color(t.text_dim).size(11.0));
        ui.horizontal(|ui| {
            if ui.button(crate::tl!("OK")).clicked() {
                let size: f64 = f.size.parse().unwrap_or(1.0);
                let mut p = json!({ "color": f.colour, "size": size });
                if f.selected_only && !selection.is_empty() {
                    p["handles"] = json!(selection);
                }
                if app.run("ewiretype", p).is_ok() {
                    done = true;
                }
            }
            if ui.button(crate::tl!("Cancel")).clicked() {
                done = true;
            }
        });
    });
    put(ctx, "wiretype", f);
    if done {
        *open = false;
    }
}

pub fn plc(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let mut f: Plc = get(ctx, "plc");
    let mut done = false;
    egui::Window::new(crate::tl!("PLC I/O Card")).id(egui::Id::new("edialog-plc")).collapsible(false).resizable(false).open(open).show(ctx, |ui| {
        ui.horizontal(|ui| {
            for (k, label) in [("DI", "Digital in"), ("DO", "Digital out"), ("AI", "Analog in"), ("AO", "Analog out")] {
                if ui.selectable_label(f.kind == k, format!("{k}  {}", crate::i18n::t(label))).clicked() {
                    f.kind = k.to_string();
                }
            }
        });
        egui::Grid::new("plc-grid").num_columns(2).spacing(vec2(8.0, 4.0)).show(ui, |ui| {
            ui.label(crate::tl!("Points"));
            ui.add(egui::DragValue::new(&mut f.points).range(1..=64));
            ui.end_row();
            ui.label(crate::tl!("First address"));
            let hint = match f.kind.as_str() {
                "DI" => "I0.0",
                "DO" => "Q0.0",
                "AI" => "IW64",
                _ => "QW64",
            };
            ui.add(egui::TextEdit::singleline(&mut f.address).hint_text(hint).desired_width(120.0));
            ui.end_row();
            ui.label(crate::tl!("Description"));
            ui.add(egui::TextEdit::singleline(&mut f.description).desired_width(200.0));
            ui.end_row();
        });
        ui.horizontal(|ui| {
            if ui.button(crate::tl!("Place")).on_hover_text(crate::tl!("Then click where the card goes")).clicked() {
                let mut params = json!({ "kind": f.kind, "points": f.points });
                if !f.address.trim().is_empty() {
                    params["address"] = json!(f.address.trim());
                }
                if !f.description.trim().is_empty() {
                    params["description"] = json!(f.description.trim());
                }
                app.session.place_preset = Some(json!({ "command": "eplc", "params": params, "label": format!("{} {}", f.kind, f.points) }));
                app.start("eplace");
                done = true;
            }
            if ui.button(crate::tl!("Cancel")).clicked() {
                done = true;
            }
        });
    });
    put(ctx, "plc", f);
    if done {
        *open = false;
    }
}

/// Whether `v` is one of these dialogs' names.
pub fn is_dialog(name: &str) -> bool {
    matches!(name, "eladder" | "ewiretype" | "eplc")
}
