//! macOS: CADCraft's menu tree as the native system menu bar. Items dispatch through the same
//! command path as the in-window menus; enablement is refreshed a few times per second.

use std::collections::HashMap;
use std::str::FromStr;

use cadcraft_ui_egui::menus::{self, Entry};
use cadcraft_ui_egui::{CadApp, i18n};
use muda::accelerator::Accelerator;
use muda::{CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};

/// A menu row: a plain command, or a switch / one choice of a group that shows a check mark.
enum Item {
    Plain(MenuItem),
    Check(CheckMenuItem),
}

impl Item {
    /// Sets the label of the row.
    fn set_text(&self, text: &str) {
        match self {
            Self::Plain(i) => i.set_text(text),
            Self::Check(i) => i.set_text(text),
        }
    }

    /// Enables or greys out the row.
    fn set_enabled(&self, enabled: bool) {
        match self {
            Self::Plain(i) => i.set_enabled(enabled),
            Self::Check(i) => i.set_enabled(enabled),
        }
    }

    /// The row as the type `Submenu::append` takes.
    fn as_menu_item(&self) -> &dyn IsMenuItem {
        match self {
            Self::Plain(i) => i,
            Self::Check(i) => i,
        }
    }
}

pub struct NativeMenu {
    _menu: Menu,
    items: HashMap<String, (String, String, Item)>,
    submenus: Vec<(String, Submenu)>,
    predefined: Vec<(&'static str, PredefinedMenuItem)>,
    language: &'static str,
    last_refresh: f64,
    /// The recent files File ▸ Open Recent lists now (it is rebuilt when they change).
    recent: Vec<String>,
    counter: usize,
}

/// Accelerator for "Cmd+Shift+S" (modifier-less shortcuts stay in the app so typing works).
fn accel(sc: &str) -> Option<Accelerator> {
    if !(sc.contains("Cmd") || sc.contains("Ctrl") || sc.contains("Alt")) {
        return None;
    }
    Accelerator::from_str(&sc.replace("Cmd", "CMD").replace("Alt", "ALT").replace("Shift", "SHIFT").replace("Ctrl", "CTRL")).ok()
}

impl NativeMenu {
    /// Builds the system menu bar from the menu tree and installs it for the app.
    pub fn install(app: &mut CadApp) -> Self {
        let menu = Menu::new();
        let mut items = HashMap::new();
        let language = app.language();
        let mut submenus = Vec::new();
        let mut counter = 0usize;
        let app_menu = Submenu::new("CADCraft", true);
        let about = MenuItem::with_id("cc-about", i18n::tr(language, "About CADCraft"), true, None);
        let discord = MenuItem::with_id("cc-discord", i18n::tr(language, "Join the ArtCraft Discord…"), true, None);
        let predefined = vec![
            ("Services", PredefinedMenuItem::services(Some(i18n::tr(language, "Services")))),
            ("Hide CADCraft", PredefinedMenuItem::hide(Some(i18n::tr(language, "Hide CADCraft")))),
            ("Hide Others", PredefinedMenuItem::hide_others(Some(i18n::tr(language, "Hide Others")))),
            ("Show All", PredefinedMenuItem::show_all(Some(i18n::tr(language, "Show All")))),
            ("Quit CADCraft", PredefinedMenuItem::quit(Some(i18n::tr(language, "Quit CADCraft")))),
        ];
        // The Mac place for preferences: CADCraft ▸ Settings… (⌘,) opens the Options dialog.
        let settings = MenuItem::with_id("cc-settings", i18n::tr(language, "Settings..."), true, accel("Cmd+Comma"));
        let _ = app_menu.append_items(&[&about, &discord, &PredefinedMenuItem::separator(), &settings, &PredefinedMenuItem::separator()]);
        for (label, item) in &predefined {
            let _ = app_menu.append(item);
            if ["Services", "Show All"].contains(label) {
                let _ = app_menu.append(&PredefinedMenuItem::separator());
            }
        }
        items.insert("cc-about".into(), ("ui.dialog.about".into(), "About CADCraft".into(), Item::Plain(about)));
        items.insert("cc-discord".into(), ("ui.discord".into(), "Join the ArtCraft Discord…".into(), Item::Plain(discord)));
        items.insert("cc-settings".into(), ("options".into(), "Settings...".into(), Item::Plain(settings)));
        let _ = menu.append(&app_menu);
        for (title, entries) in menus::tree(app) {
            let sub = Submenu::new(i18n::tr_menu(language, &title), true);
            build(&sub, &entries, &mut items, &mut submenus, &mut counter, language, &|id| menus::checked(app, id));
            let _ = menu.append(&sub);
            submenus.push((title, sub));
        }
        menu.init_for_nsapp();
        Self { _menu: menu, items, submenus, predefined, language, last_refresh: 0.0, recent: app.ui.recent_files.clone(), counter }
    }

    /// Rebuild File ▸ Open Recent after a drawing was opened or the list was cleared: the native
    /// menu is built once, and its items name the files by position.
    fn refresh_recent(&mut self, app: &CadApp) {
        if self.recent == app.ui.recent_files {
            return;
        }
        self.recent = app.ui.recent_files.clone();
        let Some(sub) = self.submenus.iter().find(|(label, _)| label == menus::OPEN_RECENT).map(|(_, s)| s.clone()) else { return };
        for _ in sub.items() {
            sub.remove_at(0);
        }
        let ids: Vec<String> = self.items.iter().filter(|(_, (cmd, _, _))| cmd.starts_with("ui.recent.")).map(|(id, _)| id.clone()).collect();
        for id in ids {
            self.items.remove(&id);
        }
        build(&sub, &menus::recent_entries(app), &mut self.items, &mut self.submenus, &mut self.counter, self.language, &|id| {
            menus::checked(app, id)
        });
    }

    /// Runs the commands chosen in the menu, then keeps enablement and check marks current.
    pub fn poll(&mut self, app: &mut CadApp, ctx: &egui::Context) {
        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            if let Some((cmd, _, _)) = self.items.get(ev.id.as_ref()) {
                if cmd == "ui.discord" {
                    ctx.open_url(egui::OpenUrl::new_tab("https://discord.gg/artcraft"));
                } else {
                    menus::activate(app, &cmd.clone());
                }
                // Show the new state of switches and choices right away, not after the next refresh.
                self.last_refresh = f64::NEG_INFINITY;
                ctx.request_repaint();
            }
        }
        self.refresh_recent(app);
        let language = app.language();
        if self.language != language {
            self.language = language;
            for (_, label, item) in self.items.values() {
                item.set_text(i18n::tr(language, label));
            }
            for (label, item) in &self.predefined {
                item.set_text(i18n::tr(language, label));
            }
            for (label, submenu) in &self.submenus {
                submenu.set_text(i18n::tr_menu(language, label));
            }
        }
        let now = cadcraft_ui_egui::now_ms();
        if now - self.last_refresh < 300.0 {
            return;
        }
        self.last_refresh = now;
        for (cmd, _, item) in self.items.values() {
            let en = cadcraft_engine::find_command(cmd).is_none_or(|c| (c.enabled)(&app.session).is_ok());
            item.set_enabled(en);
        }
        for (cmd, _, item) in self.items.values() {
            if let (Item::Check(check), Some(on)) = (item, menus::checked(app, cmd)) {
                check.set_checked(on);
            }
        }
    }
}

/// Appends `entries` to `parent`; rows for which `checked` reports a state become check items.
fn build(
    parent: &Submenu,
    entries: &[Entry],
    items: &mut HashMap<String, (String, String, Item)>,
    submenus: &mut Vec<(String, Submenu)>,
    counter: &mut usize,
    language: &str,
    checked: &dyn Fn(&str) -> Option<bool>,
) {
    for e in entries {
        match e {
            Entry::Sub { label, children } => {
                let sub = Submenu::new(i18n::tr_menu(language, label), true);
                build(&sub, children, items, submenus, counter, language, checked);
                let _ = parent.append(&sub);
                submenus.push((label.clone(), sub));
            }
            Entry::Item { label, id, shortcut, enabled } => {
                *counter += 1;
                let mid = format!("cc{counter}");
                let (text, accelerator) = (i18n::tr(language, label), shortcut.as_deref().and_then(accel));
                let i = match checked(id) {
                    Some(on) => Item::Check(CheckMenuItem::with_id(mid.clone(), text, *enabled, on, accelerator)),
                    None => Item::Plain(MenuItem::with_id(mid.clone(), text, *enabled, accelerator)),
                };
                let _ = parent.append(i.as_menu_item());
                items.insert(mid, (id.clone(), label.clone(), i));
            }
        }
    }
}
