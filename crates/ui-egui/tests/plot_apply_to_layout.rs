//! The Plot dialog's Apply to Layout (#1563) saves its settings in the page setup of the tab it
//! plots, Model included, without plotting.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services, dialogs, menus};
use egui::vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};

fn plot_dialog() -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::new(), Services::default());
    menus::activate(&mut app, "plot");
    let mut h = Harness::builder().with_size(vec2(1200.0, 900.0)).build_ui_state(|ui, app: &mut CadApp| dialogs::show(app, ui.ctx()), app);
    h.run_steps(3);
    h
}

#[test]
fn apply_to_layout_saves_model_spaces_page_setup() {
    let mut h = plot_dialog();
    assert_eq!(h.state().ui.dialog.as_deref(), Some("plot"));
    assert_eq!(h.state().session.doc().unwrap().model_page, None);
    h.get_by_label("Portrait").click();
    h.run_steps(2);
    h.get_by_label("Apply to Layout").click();
    h.run_steps(2);
    let page = h.state().session.doc().unwrap().model_page.clone().expect("Model's page setup");
    assert_eq!((page.landscape, page.plot_area.as_str(), page.scale_to_fit, page.center), (false, "extents", true, true));
    assert_eq!(h.state().ui.dialog.as_deref(), Some("plot"), "the dialog stays open");
    // One undo step.
    h.state_mut().session.undo().unwrap();
    assert_eq!(h.state().session.doc().unwrap().model_page, None);

    // A plot window can't be kept in a page setup.
    h.get_by_value("Extents").click();
    h.run_steps(2);
    h.get_by_label("Window").click();
    h.run_steps(2);
    assert!(h.query_by_label("Window <").is_some(), "the window area is chosen");
    let apply = h.get_by_label("Apply to Layout");
    assert!(apply.accesskit_node().is_disabled());
    apply.click();
    h.run_steps(2);
    assert_eq!(h.state().session.doc().unwrap().model_page, None);
}
