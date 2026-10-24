//! The Reference Edit banner on the canvas (REFEDIT): shown while editing a block reference
//! in place, offering the block's name, working set count, and controls to add or remove
//! objects from the working set and save or discard the changes.

use egui::{Rect, RichText, Stroke, vec2};
use serde_json::json;

use super::Xf;
use crate::CadApp;
use crate::theme::Tokens;

fn id() -> egui::Id {
    egui::Id::new("cadcraft-refedit-bar")
}

/// Active in-place reference edit session: (block_name, working_count).
fn editing(app: &CadApp) -> Option<(String, usize)> {
    let st = app.session.state().ok()?;
    let r = st.doc.refedit.as_ref()?;
    Some((r.block.clone(), r.items.len()))
}

/// The Reference Edit bar (over the drawing).
pub(super) fn paint(app: &mut CadApp, ctx: &egui::Context, _p: &egui::Painter, _xf: &Xf, rect: Rect) {
    let Some((name, count)) = editing(app) else { return };
    let t = Tokens::get();
    let mut action: Option<(&str, serde_json::Value)> = None;
    let mut start_cmd: Option<&str> = None;

    egui::Area::new(id()).order(egui::Order::Foreground).pivot(egui::Align2::CENTER_TOP).fixed_pos(rect.center_top() + vec2(0.0, 8.0)).show(
        ctx,
        |ui| {
            egui::Frame::new().fill(t.cmd_bg).stroke(Stroke::new(1.0, t.accent)).corner_radius(4.0).inner_margin(6.0).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(crate::tl!("Edit Reference In-Place")).color(t.accent).strong());
                    ui.label(RichText::new(&name).color(t.text));
                    ui.label(RichText::new(format!("({count})")).color(t.text_dim)).on_hover_text(crate::tl!("Working Set"));
                    ui.separator();
                    if ui.button(crate::tl!("Add to Working Set")).clicked() {
                        let sel = app.session.selection();
                        if sel.is_empty() {
                            start_cmd = Some("refset");
                        } else {
                            let hs: Vec<String> = sel.iter().map(|h| h.hex()).collect();
                            action = Some(("refset", json!({ "add": hs })));
                        }
                    }
                    if ui.button(crate::tl!("Remove")).clicked() {
                        let sel = app.session.selection();
                        if sel.is_empty() {
                            start_cmd = Some("refset");
                        } else {
                            let hs: Vec<String> = sel.iter().map(|h| h.hex()).collect();
                            action = Some(("refset", json!({ "remove": hs })));
                        }
                    }
                    ui.separator();
                    if ui.button(RichText::new(crate::tl!("Save")).color(t.accent)).clicked() {
                        action = Some(("refclose", json!({ "save": true })));
                    }
                    if ui.button(crate::tl!("Discard")).clicked() {
                        action = Some(("refclose", json!({ "save": false })));
                    }
                });
            });
        },
    );

    if let Some(cmd) = start_cmd {
        app.start(cmd);
    } else if let Some((cmd, params)) = action {
        let _ = app.run(cmd, params);
    }
}
