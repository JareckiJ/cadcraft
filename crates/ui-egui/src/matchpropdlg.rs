//! The Property Settings dialog of MATCHPROP: while the command waits at its Settings prompt, the
//! basic and special properties it copies as check boxes. OK keeps the ticks, Cancel the settings
//! as they were; both go back to "Select destination object(s)". The typed keywords keep working.

use cadcraft_engine::Input;
use cadcraft_engine::cmd::MatchProps;
use egui::vec2;

use crate::CadApp;

/// Open the dialog when MATCHPROP reaches its Settings prompt; close it when the command leaves
/// that prompt some other way (Enter or keywords typed at the command line, Esc).
pub fn sync(app: &mut CadApp) {
    let wanted = app.session.settings_dialog() == Some("matchprop");
    let shown = app.ui.dialog.as_deref() == Some("matchprop");
    if wanted && app.ui.dialog.is_none() {
        app.matchprop_dlg = Some(app.session.last_used.matchprop);
        app.ui.dialog = Some("matchprop".into());
    } else if !wanted && shown {
        app.matchprop_dlg = None;
        app.ui.dialog = None;
    }
}

/// OK: the ticked properties are the ones MATCHPROP copies from now on.
pub fn ok(app: &mut CadApp) {
    if let Some(m) = app.matchprop_dlg.take() {
        app.session.last_used.matchprop = m;
    }
    back_to_destinations(app);
}

/// Cancel: the settings stay as they were.
pub fn cancel(app: &mut CadApp) {
    app.matchprop_dlg = None;
    back_to_destinations(app);
}

fn back_to_destinations(app: &mut CadApp) {
    app.ui.dialog = None;
    if app.session.settings_dialog() == Some("matchprop")
        && let Err(e) = app.session.input(Input::Enter)
    {
        app.session.echo(e.to_string());
    }
}

pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let Some(mut m) = app.matchprop_dlg else {
        *open = false;
        return;
    };
    let (mut done_ok, mut done_cancel) = (false, false);
    egui::Window::new(crate::tl!("Property Settings")).id(egui::Id::new("matchprop_settings")).open(open).resizable(false).collapsible(false).show(
        ctx,
        |ui| {
            let MatchProps {
                color,
                layer,
                linetype,
                ltscale,
                lineweight,
                transparency,
                thickness,
                plot_style,
                dimension,
                text,
                hatch,
                polyline,
                viewport,
                table,
                multileader,
            } = &mut m;
            ui.group(|ui| {
                ui.strong(crate::tl!("Basic Properties"));
                ui.checkbox(color, crate::tl!("Color"));
                ui.checkbox(layer, crate::tl!("Layer"));
                ui.checkbox(linetype, crate::tl!("Linetype"));
                ui.checkbox(ltscale, crate::tl!("Linetype Scale"));
                ui.checkbox(lineweight, crate::tl!("Lineweight"));
                ui.checkbox(transparency, crate::tl!("Transparency"));
                ui.checkbox(thickness, crate::tl!("Thickness"));
                ui.checkbox(plot_style, crate::tl!("Plot style"));
            });
            ui.group(|ui| {
                ui.strong(crate::tl!("Special Properties"));
                egui::Grid::new("matchprop_special").num_columns(3).spacing(vec2(16.0, 4.0)).show(ui, |ui| {
                    ui.checkbox(dimension, crate::tl!("Dimension"));
                    ui.checkbox(text, crate::tl!("Text"));
                    ui.checkbox(hatch, crate::tl!("Hatch"));
                    ui.end_row();
                    ui.checkbox(polyline, crate::tl!("Polyline"));
                    ui.checkbox(viewport, crate::tl!("Viewport"));
                    ui.checkbox(table, crate::tl!("Table"));
                    ui.end_row();
                    ui.checkbox(multileader, crate::tl!("Multileader"));
                    ui.end_row();
                });
            });
            ui.horizontal(|ui| {
                done_ok = ui.button(crate::tl!("OK")).clicked();
                done_cancel = ui.button(crate::tl!("Cancel")).clicked();
            });
        },
    );
    app.matchprop_dlg = Some(m);
    if done_ok {
        ok(app);
    } else if done_cancel || !*open {
        cancel(app);
    }
}
