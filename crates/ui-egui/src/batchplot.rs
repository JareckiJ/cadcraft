//! Batch Plot: a print manager. The sheets to plot in a list that can be reordered (drag, or the
//! Move buttons), with sheets repeated or removed; sheets of any open drawing can be added; the
//! result is one PDF with every sheet or one PDF per sheet (PUBLISH with `sheets`).

use egui::{Rect, RichText, Sense, Stroke, pos2, vec2};
use serde_json::{Value, json};

use crate::CadApp;
use crate::icons::{self, Icon};
use crate::theme::Tokens;

/// One sheet in the list: the drawing it belongs to (its title) and the layout (or `Model`).
#[derive(Clone, Debug, PartialEq)]
pub struct Sheet {
    pub drawing: String,
    pub layout: String,
}

/// The window's state between frames.
#[derive(Clone, Default)]
struct State {
    sheets: Vec<Sheet>,
    /// The drawing the list was filled from when the window opened.
    filled_for: Option<String>,
    selected: Option<usize>,
    dragging: Option<usize>,
    /// The drawing whose sheets the Add section shows.
    add_from: Option<String>,
    separate: bool,
    note: Option<String>,
}

fn state_id() -> egui::Id {
    egui::Id::new("cc_batch_plot")
}

/// The sheets of drawing `d` in tab order: Model when it has objects, then the layouts.
pub fn sheets_of(d: &cadcraft_doc::Drawing, title: &str) -> Vec<Sheet> {
    let mut layouts: Vec<_> = d.layouts.iter().collect();
    layouts.sort_by_key(|l| l.tab_order);
    let model = (!d.model.is_empty()).then(|| "Model".to_string());
    model.into_iter().chain(layouts.iter().map(|l| l.name.clone())).map(|layout| Sheet { drawing: title.to_string(), layout }).collect()
}

/// Move the sheet at `from` to stand before `slot` (0..=len).
pub fn move_sheet(v: &mut Vec<Sheet>, from: usize, slot: usize) -> Option<usize> {
    if from >= v.len() {
        return None;
    }
    let s = v.remove(from);
    let to = if slot > from { slot - 1 } else { slot }.min(v.len());
    v.insert(to, s);
    Some(to)
}

/// The PUBLISH parameters for `sheets` (drawing titles resolved to open drawings).
pub fn publish_params(app: &CadApp, sheets: &[Sheet], combine: bool, path: Option<&str>) -> Result<Value, String> {
    let list: Vec<Value> = sheets
        .iter()
        .map(|s| {
            let i = app
                .session
                .docs
                .iter()
                .position(|d| d.title == s.drawing)
                .ok_or_else(|| format!("{}: {}", crate::tl!("The drawing is closed"), s.drawing))?;
            Ok(json!({ "drawing": i, "layout": s.layout }))
        })
        .collect::<Result<_, String>>()?;
    let mut p = json!({ "sheets": list, "combine": combine });
    if let Some(path) = path {
        p["path"] = json!(path);
    }
    Ok(p)
}

/// What a click asked for, applied once the list is drawn.
enum Act {
    Select(usize),
    Move(usize, usize),
    Duplicate(usize),
    Remove(usize),
    Add(Sheet),
    AddAll(String),
    Clear,
    Plot,
}

/// File ▸ Batch Plot: the print manager window.
pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let mut st = ctx.data(|d| d.get_temp::<State>(state_id())).unwrap_or_default();
    let current = app.session.state().map(|s| s.title.clone()).unwrap_or_default();
    // Opened for a drawing: its sheets, in tab order (others are added by hand).
    if st.filled_for.as_deref() != Some(current.as_str()) {
        st.sheets = app.session.doc().map(|d| sheets_of(d, &current)).unwrap_or_default();
        st.filled_for = Some(current.clone());
        st.selected = None;
        st.add_from = Some(current.clone());
        st.note = None;
    }
    let titles: Vec<String> = app.session.docs.iter().map(|d| d.title.clone()).collect();
    let several = st.sheets.iter().any(|s| s.drawing != current) || titles.len() > 1;
    let paper = |s: &Sheet| -> String {
        let Some(d) = app.session.docs.iter().find(|d| d.title == s.drawing) else { return crate::tl!("closed").to_string() };
        if s.layout == "Model" {
            return String::new();
        }
        d.doc.layout(&s.layout).map(|l| l.page.paper.clone()).unwrap_or_default()
    };
    let rows: Vec<(String, String, String)> = st.sheets.iter().map(|s| (s.layout.clone(), s.drawing.clone(), paper(s))).collect();
    let can_plot = app.services.pick_save.is_some();
    let mut act: Option<Act> = None;
    let mut close = false;
    egui::Window::new(crate::tl!("Batch Plot"))
        .id(egui::Id::new("cc_batch_plot_window"))
        .open(open)
        .default_size(vec2(680.0, 500.0))
        .min_size(vec2(520.0, 380.0))
        .resizable(true)
        .show(ctx, |ui| {
            let t = Tokens::get();
            ui.label(RichText::new(crate::tl!("Sheets to plot, in order")).strong());
            ui.horizontal_top(|ui| {
                let list_w = (ui.available_width() - 190.0).max(260.0);
                let list_h = (ui.available_height() - 190.0).max(140.0);
                ui.allocate_ui_with_layout(vec2(list_w, list_h), egui::Layout::top_down(egui::Align::LEFT), |ui| {
                    egui::Frame::NONE.stroke(Stroke::new(1.0, t.border)).inner_margin(2.0).show(ui, |ui| {
                        egui::ScrollArea::vertical().id_salt("cc_batch_list").auto_shrink([false, false]).max_height(list_h - 6.0).show(ui, |ui| {
                            let w = list_w - 10.0;
                            ui.set_width(w);
                            let mut rects: Vec<Rect> = Vec::new();
                            for (i, (layout, drawing, paper)) in rows.iter().enumerate() {
                                let (r, resp) = ui.allocate_exact_size(vec2(w, 24.0), Sense::click_and_drag());
                                rects.push(r);
                                let on = st.selected == Some(i);
                                if on {
                                    ui.painter().rect_filled(r, 2.0, t.accent.gamma_multiply(0.35));
                                } else if resp.hovered() {
                                    ui.painter().rect_filled(r, 2.0, t.control_hover.gamma_multiply(0.6));
                                }
                                let p = ui.painter().with_clip_rect(r);
                                let y = r.center().y;
                                // A grip that shows rows can be dragged.
                                for dy in [-4.0, 0.0, 4.0] {
                                    p.hline((r.left() + 5.0)..=(r.left() + 13.0), y + dy, Stroke::new(1.0, t.text_faint));
                                }
                                p.text(pos2(r.left() + 20.0, y), egui::Align2::LEFT_CENTER, format!("{}.", i + 1), crate::theme::body(), t.text_dim);
                                p.text(pos2(r.left() + 48.0, y), egui::Align2::LEFT_CENTER, crate::i18n::t(layout), crate::theme::body(), t.text);
                                if several {
                                    // Hebrew names read right to left, as in the file manager.
                                    let job = crate::i18n::job(drawing, crate::theme::small(), t.text_dim);
                                    let g = ui.ctx().fonts_mut(|f| f.layout_job(job));
                                    p.galley(pos2(r.left() + w * 0.45, y - g.size().y / 2.0), g, t.text_dim);
                                }
                                p.text(pos2(r.right() - 6.0, y), egui::Align2::RIGHT_CENTER, paper, crate::theme::small(), t.text_dim);
                                if resp.clicked() || resp.drag_started() {
                                    act = Some(Act::Select(i));
                                }
                                if resp.drag_started() {
                                    st.dragging = Some(i);
                                }
                                resp.context_menu(|ui| {
                                    if ui.button(crate::tl!("Duplicate")).clicked() {
                                        act = Some(Act::Duplicate(i));
                                        ui.close();
                                    }
                                    if ui.button(crate::tl!("Delete")).clicked() {
                                        act = Some(Act::Remove(i));
                                        ui.close();
                                    }
                                });
                            }
                            if rows.is_empty() {
                                ui.label(RichText::new(crate::tl!("No sheets. Add sheets below.")).color(t.text_dim));
                            }
                            // Dragging a row: a line where it will land.
                            if let Some(from) = st.dragging {
                                let pointer = ui.ctx().pointer_latest_pos();
                                let slot = pointer.map(|p| rects.iter().position(|r| p.y < r.center().y).unwrap_or(rects.len()));
                                if let Some(slot) = slot {
                                    let y = rects.get(slot).map_or_else(|| rects.last().map_or(0.0, |r| r.bottom()), |r| r.top());
                                    if let Some(first) = rects.first() {
                                        ui.painter().hline(first.left()..=first.right(), y, Stroke::new(2.0, t.accent));
                                    }
                                }
                                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                                if ui.input(|i| i.pointer.any_released()) {
                                    if let Some(slot) = slot
                                        && slot != from
                                        && slot != from + 1
                                    {
                                        act = Some(Act::Move(from, slot));
                                    }
                                    st.dragging = None;
                                }
                            }
                        });
                    });
                });
                // The order and list buttons.
                ui.vertical(|ui| {
                    ui.set_width(180.0);
                    let sel = st.selected.filter(|i| *i < st.sheets.len());
                    let n = st.sheets.len();
                    let row = |ui: &mut egui::Ui, icon: Icon, label: &str, enabled: bool| -> bool {
                        ui.add_enabled_ui(enabled, |ui| {
                            ui.horizontal(|ui| {
                                let r = ui.add_sized(vec2(176.0, 26.0), egui::Button::new(""));
                                let ir = Rect::from_center_size(pos2(r.rect.left() + 15.0, r.rect.center().y), vec2(18.0, 18.0));
                                icons::paint(ui.painter(), ir, icon, !enabled);
                                let col = if enabled { Tokens::get().text } else { Tokens::get().text_faint };
                                ui.painter().text(
                                    pos2(r.rect.left() + 30.0, r.rect.center().y),
                                    egui::Align2::LEFT_CENTER,
                                    label,
                                    crate::theme::body(),
                                    col,
                                );
                                r.clicked()
                            })
                            .inner
                        })
                        .inner
                    };
                    if row(ui, Icon::MoveTop, crate::tl!("Move to Top"), sel.is_some_and(|i| i > 0))
                        && let Some(i) = sel
                    {
                        act = Some(Act::Move(i, 0));
                    }
                    if row(ui, Icon::MoveUp, crate::tl!("Move Higher"), sel.is_some_and(|i| i > 0))
                        && let Some(i) = sel
                    {
                        act = Some(Act::Move(i, i.saturating_sub(1)));
                    }
                    if row(ui, Icon::MoveDown, crate::tl!("Move Lower"), sel.is_some_and(|i| i + 1 < n))
                        && let Some(i) = sel
                    {
                        act = Some(Act::Move(i, i + 2));
                    }
                    if row(ui, Icon::MoveBottom, crate::tl!("Move to Bottom"), sel.is_some_and(|i| i + 1 < n))
                        && let Some(i) = sel
                    {
                        act = Some(Act::Move(i, n));
                    }
                    ui.add_space(6.0);
                    if row(ui, Icon::Copy, crate::tl!("Duplicate"), sel.is_some())
                        && let Some(i) = sel
                    {
                        act = Some(Act::Duplicate(i));
                    }
                    if row(ui, Icon::Erase, crate::tl!("Delete"), sel.is_some())
                        && let Some(i) = sel
                    {
                        act = Some(Act::Remove(i));
                    }
                    if row(ui, Icon::Close, crate::tl!("Clear List"), n > 0) {
                        act = Some(Act::Clear);
                    }
                });
            });
            ui.add_space(6.0);
            ui.separator();
            // Adding sheets: from any open drawing.
            ui.horizontal(|ui| {
                ui.label(crate::tl!("Add sheets from"));
                let from = st.add_from.clone().filter(|f| titles.contains(f)).unwrap_or_else(|| current.clone());
                let shown = crate::i18n::widget(ui, &from, egui::TextStyle::Body);
                egui::ComboBox::from_id_salt("cc_batch_from").selected_text(shown).width(220.0).show_ui(ui, |ui| {
                    for title in &titles {
                        let name = crate::i18n::widget(ui, title, egui::TextStyle::Body);
                        if ui.selectable_label(*title == from, name).clicked() {
                            st.add_from = Some(title.clone());
                        }
                    }
                });
                if ui.button(crate::tl!("Add All")).clicked() {
                    act = Some(Act::AddAll(from.clone()));
                }
            });
            let from = st.add_from.clone().filter(|f| titles.contains(f)).unwrap_or_else(|| current.clone());
            let available = app.session.docs.iter().find(|d| d.title == from).map(|d| sheets_of(&d.doc, &from)).unwrap_or_default();
            egui::ScrollArea::horizontal().id_salt("cc_batch_add").max_height(34.0).show(ui, |ui| {
                ui.horizontal(|ui| {
                    for s in available {
                        if ui
                            .button(format!("+ {}", crate::i18n::t(&s.layout)))
                            .on_hover_text(crate::tl!("Add this sheet to the end of the list"))
                            .clicked()
                        {
                            act = Some(Act::Add(s));
                        }
                    }
                });
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.radio_value(&mut st.separate, false, crate::tl!("One PDF file with all sheets"));
                ui.radio_value(&mut st.separate, true, crate::tl!("A separate PDF file for each sheet"));
            });
            if let Some(n) = &st.note {
                ui.label(RichText::new(n).color(t.text_dim));
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let enabled = can_plot && !st.sheets.is_empty();
                let r = ui.add_enabled(enabled, egui::Button::new(crate::tl!("Plot...")));
                let r = if can_plot { r } else { r.on_disabled_hover_text(crate::tl!("There is no file dialog here")) };
                if r.clicked() {
                    act = Some(Act::Plot);
                }
                if ui.button(crate::tl!("Close")).clicked() {
                    close = true;
                }
                ui.label(RichText::new(format!("{} {}", st.sheets.len(), crate::tl!("sheets"))).color(t.text_dim));
            });
        });
    match act {
        Some(Act::Select(i)) => st.selected = Some(i),
        Some(Act::Move(from, slot)) => st.selected = move_sheet(&mut st.sheets, from, slot),
        Some(Act::Duplicate(i)) => {
            if let Some(s) = st.sheets.get(i).cloned() {
                st.sheets.insert(i + 1, s);
                st.selected = Some(i + 1);
            }
        }
        Some(Act::Remove(i)) => {
            if i < st.sheets.len() {
                st.sheets.remove(i);
                st.selected = if st.sheets.is_empty() { None } else { Some(i.min(st.sheets.len() - 1)) };
            }
        }
        Some(Act::Add(s)) => {
            st.sheets.push(s);
            st.selected = Some(st.sheets.len() - 1);
        }
        Some(Act::AddAll(title)) => {
            if let Some(d) = app.session.docs.iter().find(|d| d.title == title) {
                st.sheets.extend(sheets_of(&d.doc, &title));
            }
        }
        Some(Act::Clear) => {
            st.sheets.clear();
            st.selected = None;
        }
        Some(Act::Plot) => {
            let stem = std::path::Path::new(&current)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "Drawing".into());
            let picked = app.services.pick_save.as_ref().and_then(|pick| pick(&format!("{stem}.pdf")));
            if let Some(path) = picked {
                match publish_params(app, &st.sheets, !st.separate, Some(&path)).and_then(|p| app.run("publish", p)) {
                    Ok(r) => {
                        let msg = r["message"].as_str().unwrap_or("").to_string();
                        app.session.echo(msg);
                        close = true;
                    }
                    Err(e) => st.note = Some(e),
                }
            }
        }
        None => {}
    }
    if close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) && st.dragging.is_none() {
        *open = false;
    }
    if *open {
        ctx.data_mut(|d| d.insert_temp(state_id(), st));
    } else {
        // Opened again later: filled anew from the drawing then current.
        ctx.data_mut(|d| d.remove::<State>(state_id()));
    }
}

#[cfg(test)]
mod tests {
    use super::{Sheet, move_sheet};

    fn s(n: &str) -> Sheet {
        Sheet { drawing: "A.dwg".into(), layout: n.into() }
    }

    #[test]
    fn sheets_move_to_a_slot() {
        let mut v = vec![s("1"), s("2"), s("3"), s("4")];
        assert_eq!(move_sheet(&mut v, 3, 0), Some(0));
        assert_eq!(v.iter().map(|x| x.layout.as_str()).collect::<Vec<_>>(), ["4", "1", "2", "3"]);
        assert_eq!(move_sheet(&mut v, 0, 4), Some(3));
        assert_eq!(v.iter().map(|x| x.layout.as_str()).collect::<Vec<_>>(), ["1", "2", "3", "4"]);
        assert_eq!(move_sheet(&mut v, 1, 3), Some(2));
        assert_eq!(v.iter().map(|x| x.layout.as_str()).collect::<Vec<_>>(), ["1", "3", "2", "4"]);
        assert_eq!(move_sheet(&mut v, 9, 0), None);
    }
}
