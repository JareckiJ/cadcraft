//! The electrical tool bar and the Project Manager in the real app: the bar's icons are there,
//! the manager lists a project's sheets with their titles and its buttons reorder them. With
//! CRAFT_UI_VISUAL_DIR set, screenshots are saved there for review.

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_ui_egui::{CadApp, Services};
use egui::vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

fn save(h: &mut Harness<'static, CadApp>, name: &str) {
    if let Ok(directory) = std::env::var("CRAFT_UI_VISUAL_DIR") {
        std::fs::create_dir_all(&directory).unwrap();
        h.render().unwrap().save(format!("{directory}/{name}.png")).unwrap();
    }
}

#[test]
fn electrical_tool_bar_and_project_manager() {
    set_io(IoHooks {
        read: |b, name| cadcraft_io::read(b, name).map_err(|e| e.to_string()),
        write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
        plot: None,
    });
    let dir = std::env::temp_dir().join(format!("cadcraft-ui-eproject-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let project = dir.join("Plant.ecproj").to_string_lossy().to_string();

    let mut app = CadApp::new(Session::default(), Services::default());
    app.run("eproject.new", json!({ "path": project, "name": "Pump station", "sheets": 3 })).unwrap();
    for (n, title) in [(1, "Power distribution"), (2, "Motor control"), (3, "PLC inputs")] {
        app.run("eproject.sheet", json!({ "project": project, "sheet": n, "set": { "TITLE": title } })).unwrap();
    }
    app.run("eproject.open", json!({ "path": project })).unwrap();
    app.run("eladder", json!({ "top": 250, "bottom": 60, "potentials": ["+24V", "0V"] })).unwrap();
    app.run("esymbol", json!({ "name": "IEC_PB_NO", "at": [21, 200] })).unwrap();
    app.run("esymbol", json!({ "name": "IEC_COIL", "at": [21, 110] })).unwrap();
    app.run("eladder.clean", json!({})).unwrap();
    app.run("zoom", json!({ "extents": true })).ok();
    app.ui.start_tab = false;
    app.ui.in_window_menu = true;
    app.run("ui.panel.project", json!({})).unwrap();

    let mut h = Harness::builder().with_size(vec2(1400.0, 860.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(8);
    save(&mut h, "electrical-1-project-manager");

    // The tool bar's icons, by their tooltips' labels.
    for label in ["Draw Wire (EWIRE): click a pin or wire, then the next", "Relay coil (-K)", "Parts list (CSV)", "Project Manager"] {
        assert!(h.query_all_by_label(label).next().is_some(), "tool bar icon {label}");
    }
    // The sheets with their titles.
    for title in ["Power distribution", "Motor control", "PLC inputs"] {
        assert!(h.query_all_by_label_contains(title).next().is_some(), "sheet {title}");
    }
    // Select sheet 3 and move it up: the order changes and the renumber offer appears.
    h.get_by_label_contains("PLC inputs").click();
    h.run_steps(3);
    save(&mut h, "electrical-2-sheet-selected");
    h.get_by_label("▲").click();
    h.run_steps(6);
    assert!(h.query_by_label_contains("Renumber device tags").is_some(), "renumber offer");
    save(&mut h, "electrical-3-moved");
    let list = h.state_mut().session.execute("eproject.sheets", &json!({ "project": project })).unwrap();
    assert_eq!(list["sheets"][1]["fields"]["TITLE"], "PLC inputs");
    h.get_by_label("Keep tags").click();
    h.run_steps(3);
    assert!(h.query_by_label_contains("Renumber device tags").is_none());

    // Placing a contact by hand opens Insert / Edit Contact with the coils to pick from.
    h.state_mut().cmdline("esymbol IEC_NO");
    h.run_steps(2);
    h.state_mut().cmdline("63,200");
    h.run_steps(6);
    save(&mut h, "electrical-6-insert-contact");
    assert!(h.query_by_label_contains("Coil / main device").is_some(), "the contact dialog");
    let coil = h.query_all_by_label_contains("-K3").next().map(|n| n.label().unwrap_or_default());
    assert!(coil.is_some(), "the coil is listed");
    h.get_all_by_label_contains("-K3").next().unwrap().click();
    h.run_steps(2);
    h.get_by_label("OK").click();
    h.run_steps(4);
    assert!(h.query_by_label_contains("Coil / main device").is_none(), "closed");

    // The electrical workspace: Tool Sets, Layers and Properties go; Project Manager and the
    // Electrical palette share the left side.
    h.state_mut().run("ui.workspace.electrical", json!({})).unwrap();
    h.run_steps(6);
    save(&mut h, "electrical-4-workspace");
    assert!(!h.state().ui.show_toolsets && !h.state().ui.show_palettes);
    // The palette's close button closes it.
    h.get_by_label("Close panel").click();
    h.run_steps(4);
    assert!(!h.state().ui.docking.layout.contains(&cadcraft_ui_egui::docking::Panel::Projects));
    save(&mut h, "electrical-5-closed");
}
