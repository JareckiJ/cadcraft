//! The native macOS menu marks switches and the current choice of a group with a check mark.
//! `menus::checked` is what it asks: `Some(on)` for those items, `None` for ordinary commands.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services, menus};

/// A fresh app with an empty session.
fn app() -> CadApp {
    CadApp::new(Session::new(), Services::default())
}

/// Collects the command id of every item in `entries`, submenus included.
fn ids(entries: &[menus::Entry], out: &mut Vec<String>) {
    for e in entries {
        match e {
            menus::Entry::Item { id, .. } => out.push(id.clone()),
            menus::Entry::Sub { children, .. } => ids(children, out),
        }
    }
}

/// Choosing one item of a group leaves exactly that item checked.
#[test]
fn exactly_the_chosen_item_of_a_group_is_checked() {
    let mut a = app();
    let groups: [(&str, &[&str]); 4] = [
        ("ui.theme.", &["system", "light", "dark"]),
        ("ui.animations.", &["system", "on", "off"]),
        ("ui.saveformat.", &["dxf", "dwg"]),
        ("ui.saveversion.", &["2018", "2013", "2010", "2007", "2004", "2000"]),
    ];
    for (prefix, choices) in groups {
        for chosen in choices {
            menus::activate(&mut a, &format!("{prefix}{chosen}"));
            for choice in choices {
                assert_eq!(menus::checked(&a, &format!("{prefix}{choice}")), Some(choice == chosen), "{prefix}{choice} after choosing {chosen}");
            }
        }
    }
}

/// A switch is checked while it is on, and flips with each click.
#[test]
fn a_switch_is_checked_while_it_is_on_and_follows_the_menu() {
    let mut a = app();
    for (id, on) in [
        ("ui.toggle.toolbar", (|a: &CadApp| a.ui.show_toolbar) as fn(&CadApp) -> bool),
        ("ui.toggle.statusbar", |a| a.ui.show_status_bar),
        ("ui.toggle.viewcube", |a| a.ui.show_viewcube),
        ("ui.toggle.ucsicon", |a| a.ui.show_ucs_icon),
        ("ui.toggle.systemcursor", |a| a.ui.system_cursor),
    ] {
        let before = on(&a);
        assert_eq!(menus::checked(&a, id), Some(before), "{id} at the start");
        menus::activate(&mut a, id);
        assert_eq!(on(&a), !before, "{id} flips");
        assert_eq!(menus::checked(&a, id), Some(!before), "{id} after the click");
        menus::activate(&mut a, id);
        assert_eq!(menus::checked(&a, id), Some(before), "{id} back again");
    }
}

/// Commands that are neither a switch nor a choice report no state.
#[test]
fn ordinary_commands_have_no_check_mark() {
    let a = app();
    for id in ["line", "ui.dialog.about", "ui.saveformat", "ui.theme", "ui.dock.reset"] {
        assert_eq!(menus::checked(&a, id), None, "{id}");
    }
}

/// Every switch and choice in the menu tree has a state to show.
#[test]
fn every_switch_and_choice_in_the_menu_tree_reports_a_state() {
    let a = app();
    let mut all = Vec::new();
    for (_, entries) in menus::tree(&a) {
        ids(&entries, &mut all);
    }
    let marked: Vec<&String> = all
        .iter()
        .filter(|id| ["ui.toggle.", "ui.theme.", "ui.animations.", "ui.saveformat.", "ui.saveversion."].iter().any(|p| id.starts_with(p)))
        .collect();
    assert!(marked.len() >= 20, "the menu has switches and choices, found {marked:?}");
    for id in marked {
        assert!(menus::checked(&a, id).is_some(), "{id} is in a menu as a switch or choice but has no state");
    }
}
