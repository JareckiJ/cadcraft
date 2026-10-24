//! The electrical tool bar: one row of icons under the tool bar, like EPLAN's Insert and
//! Connection bars — wiring, the common IEC symbols, numbering and checks, reports and the
//! Project Manager. Each icon runs the same command as its menu item (an icon for a symbol
//! starts Insert Symbol with that symbol: click in the drawing to place it). Whole-project
//! versions run when a project is open. Window ▸ Electrical Tool Bar shows or hides it.

use egui::{Rect, Sense, pos2, vec2};
use serde_json::json;

use crate::CadApp;
use crate::icons::{self, Icon};
use crate::theme::Tokens;

/// What an icon does.
#[derive(Clone, Copy)]
enum Act {
    /// Start a command (its prompts follow on the command line).
    Start(&'static str),
    /// Insert Symbol with this library symbol.
    Symbol(&'static str),
    /// The drawing's command, or the project's when a project is open.
    Scoped(&'static str, &'static str),
    /// Save a report as CSV (kind, suggested file name).
    Report(&'static str, &'static str),
    /// Open a panel (a UI command).
    Ui(&'static str),
}

const GROUPS: &[&[(Icon, Act, &str)]] = &[
    &[
        (Icon::EWire, Act::Start("ewire"), "Draw Wire (EWIRE): click a pin or wire, then the next"),
        (Icon::EWireType, Act::Start("ewiretype"), "Wire Type: colour and cross-section of new wires"),
        (Icon::ELadder, Act::Start("eladder"), "Insert Ladder: supply rails with a rung in every column"),
        (Icon::ECleanRungs, Act::Start("eladder.clean"), "Remove Empty Rungs (they would short the rails)"),
        (Icon::ECable, Act::Start("ecable"), "Insert Cable: draw a line across its wires"),
    ],
    &[
        (Icon::ECoil, Act::Symbol("IEC_COIL"), "Relay coil (-K)"),
        (Icon::ENo, Act::Symbol("IEC_NO"), "Normally open contact"),
        (Icon::ENc, Act::Symbol("IEC_NC"), "Normally closed contact"),
        (Icon::EPbNo, Act::Symbol("IEC_PB_NO"), "Push-button, normally open (-S)"),
        (Icon::EPbNc, Act::Symbol("IEC_PB_NC"), "Push-button, normally closed (-S)"),
        (Icon::ELamp, Act::Symbol("IEC_LAMP"), "Indicator lamp (-P)"),
        (Icon::ETerminal, Act::Symbol("IEC_TERMINAL"), "Terminal (-X)"),
        (Icon::EBreaker, Act::Symbol("IEC_MCB_3P"), "Circuit-breaker, 3 poles (-Q)"),
        (Icon::EMotor, Act::Symbol("IEC_MOTOR_3PH"), "Three-phase motor (-M)"),
        (Icon::EPlc, Act::Start("eplc"), "PLC I/O card (DI, DO, AI, AO)"),
    ],
    &[
        (Icon::ETag, Act::Scoped("etag", "eproject.tag"), "Tag Devices (IEC 81346): whole project when one is open"),
        (Icon::EWireNum, Act::Scoped("ewirenum", "eproject.wires"), "Number Wires: one number per network, at its start"),
        (Icon::EXref, Act::Scoped("exref", "eproject.xref"), "Cross-References: contacts and coils"),
        (Icon::ECheck, Act::Start("echeck"), "Check Project: duplicate tags, shorts, contacts without a coil, open pins"),
    ],
    &[
        (Icon::EReportBom, Act::Report("bom", "parts-list.csv"), "Parts list (CSV)"),
        (Icon::EReportWires, Act::Report("wirelist", "wire-list.csv"), "Wire list (CSV)"),
        (Icon::EReportTerminals, Act::Report("terminals", "terminal-plan.csv"), "Terminal plan (CSV)"),
        (Icon::EReportPlc, Act::Report("plcio", "plc-io.csv"), "PLC I/O list (CSV)"),
        (Icon::EReportCables, Act::Report("cables", "cables.csv"), "Cable list (CSV)"),
    ],
    &[(Icon::EProject, Act::Ui("ui.panel.project"), "Project Manager")],
];

/// The bar's height.
pub const HEIGHT: f32 = 32.0;

fn has_project(app: &mut CadApp) -> bool {
    app.session.execute("eproject.list", &json!({})).is_ok()
}

fn run(app: &mut CadApp, act: Act) {
    match act {
        Act::Start(cmd) => app.start(cmd),
        Act::Symbol(name) => {
            app.cmdline(&format!("esymbol {name}"));
        }
        Act::Scoped(here, project) => {
            if has_project(app) {
                let _ = app.run(project, json!({}));
            } else {
                app.start(here);
            }
        }
        Act::Report(kind, file) => crate::palettes::electrical::save_report(app, kind, file),
        Act::Ui(cmd) => {
            let _ = app.run(cmd, json!({}));
        }
    }
}

/// Whether `act` needs a drawing open.
fn needs_drawing(act: Act) -> bool {
    !matches!(act, Act::Ui(_) | Act::Report(..))
}

pub fn show(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let has_doc = !app.session.docs.is_empty() && !app.ui.start_tab;
    egui::Panel::top("cc_electrical_toolbar").exact_size(HEIGHT).frame(egui::Frame::NONE.fill(t.chrome)).show(ui, |ui| {
        let row = ui.max_rect();
        ui.painter().line_segment([row.left_bottom(), row.right_bottom()], egui::Stroke::new(1.0, t.chrome_dark));
        let mut clicked = None;
        egui::ScrollArea::horizontal().id_salt("etoolbar").show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.add_space(8.0);
                ui.spacing_mut().item_spacing = vec2(2.0, 0.0);
                for (gi, group) in GROUPS.iter().enumerate() {
                    if gi > 0 {
                        ui.add_space(6.0);
                        let (r, _) = ui.allocate_exact_size(vec2(1.0, HEIGHT - 10.0), Sense::hover());
                        ui.painter().rect_filled(Rect::from_min_size(pos2(r.left(), r.top()), r.size()), 0.0, t.text_dim.gamma_multiply(0.4));
                        ui.add_space(6.0);
                    }
                    for (icon, act, tip) in group.iter() {
                        let enabled = has_doc || !needs_drawing(*act);
                        let r = ui.add_enabled_ui(enabled, |ui| icons::button(ui, *icon, 26.0, tip, false)).inner;
                        if r.clicked() {
                            clicked = Some(*act);
                        }
                    }
                }
                ui.add_space(8.0);
            });
        });
        if let Some(act) = clicked {
            run(app, act);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_names_a_real_command_or_symbol() {
        let mut s = cadcraft_engine::Session::new();
        let lib = s.execute("esymbol", &json!({ "list": true })).unwrap();
        let names: Vec<&str> = lib["symbols"].as_array().unwrap().iter().filter_map(|d| d["name"].as_str()).collect();
        let ids: Vec<String> = cadcraft_engine::cmd::command_specs().iter().map(|c| c.id.to_string()).collect();
        for group in GROUPS {
            for (_, act, _) in group.iter() {
                match act {
                    Act::Start(c) => assert!(ids.iter().any(|i| i == c), "{c}"),
                    Act::Scoped(a, b) => assert!(ids.iter().any(|i| i == a) && ids.iter().any(|i| i == b), "{a} {b}"),
                    Act::Symbol(n) => assert!(names.contains(n), "{n}"),
                    Act::Report(..) => {}
                    Act::Ui(c) => assert!(crate::menus::is_ui_command(c), "{c}"),
                }
            }
        }
    }
}
