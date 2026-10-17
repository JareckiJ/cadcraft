//! Property Settings dialog for MATCHPROP: AutoCAD-standard property picker checkboxes.

use cadcraft_engine::cmd::MatchProps;
use egui::{RichText, vec2};

use crate::CadApp;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MatchPropDlgState {
    pub props: MatchProps,
}

pub fn open(app: &mut CadApp) {
    app.matchprop_dlg = Some(MatchPropDlgState { props: app.session.last_used.matchprop });
    app.ui.dialog = Some("matchprop".into());
}

pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let Some(mut st) = app.matchprop_dlg.clone() else {
        *open = false;
        return;
    };

    let mut is_open = true;
    let mut confirmed = false;
    let mut cancelled = false;

    egui::Window::new(crate::tl!("Property Settings"))
        .id(egui::Id::new("matchprop_property_settings_window"))
        .open(&mut is_open)
        .resizable(false)
        .default_width(420.0)
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 6.0);

            ui.horizontal(|ui| {
                // Column 1: Basic Properties
                ui.group(|ui| {
                    ui.set_min_width(190.0);
                    ui.label(RichText::new(crate::tl!("Basic Properties")).strong());
                    ui.separator();
                    ui.checkbox(&mut st.props.color, crate::tl!("Color"));
                    ui.checkbox(&mut st.props.layer, crate::tl!("Layer"));
                    ui.checkbox(&mut st.props.linetype, crate::tl!("Linetype"));
                    ui.checkbox(&mut st.props.ltscale, crate::tl!("Linetype Scale"));
                    ui.checkbox(&mut st.props.lineweight, crate::tl!("Lineweight"));
                    ui.checkbox(&mut st.props.thickness, crate::tl!("Thickness"));
                    ui.checkbox(&mut st.props.plot_style, crate::tl!("Plot style"));
                    ui.checkbox(&mut st.props.transparency, crate::tl!("Transparency"));
                    ui.checkbox(&mut st.props.material, crate::tl!("Material"));
                    ui.checkbox(&mut st.props.shadow, crate::tl!("Shadow display"));
                });

                // Column 2: Special Properties
                ui.group(|ui| {
                    ui.set_min_width(190.0);
                    ui.label(RichText::new(crate::tl!("Special Properties")).strong());
                    ui.separator();
                    ui.checkbox(&mut st.props.dimension, crate::tl!("Dimension"));
                    ui.checkbox(&mut st.props.text, crate::tl!("Text"));
                    ui.checkbox(&mut st.props.hatch, crate::tl!("Hatch"));
                    ui.checkbox(&mut st.props.polyline, crate::tl!("Polyline"));
                    ui.checkbox(&mut st.props.viewport, crate::tl!("Viewport"));
                    ui.checkbox(&mut st.props.table, crate::tl!("Table"));
                    ui.checkbox(&mut st.props.multileader, crate::tl!("Multileader"));
                    ui.checkbox(&mut st.props.center, crate::tl!("Center Object"));
                });
            });

            ui.add_space(8.0);
            ui.separator();

            ui.horizontal(|ui| {
                if ui.button(crate::tl!("Select All")).clicked() {
                    st.props = MatchProps::all(true);
                }
                if ui.button(crate::tl!("Clear All")).clicked() {
                    st.props = MatchProps::all(false);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(crate::tl!("Cancel")).clicked() {
                        cancelled = true;
                    }
                    if ui.button(RichText::new(crate::tl!("OK")).strong()).clicked() {
                        confirmed = true;
                    }
                });
            });
        });

    if confirmed {
        app.session.last_used.matchprop = st.props;
        app.matchprop_dlg = None;
        *open = false;
    } else if cancelled || !is_open {
        app.matchprop_dlg = None;
        *open = false;
    } else {
        app.matchprop_dlg = Some(st);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_engine::Session;

    #[test]
    fn open_and_save_matchprop_dialog() {
        let mut app = CadApp::new(Session::new(), crate::Services::default());
        open(&mut app);
        assert!(app.matchprop_dlg.is_some());
        assert_eq!(app.ui.dialog.as_deref(), Some("matchprop"));
    }
}
