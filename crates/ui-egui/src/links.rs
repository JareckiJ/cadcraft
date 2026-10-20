//! Community and project links opened from the Help menu and native application menu.
//! URLs follow the crafting-app pattern (`getartcraft.com/apps/<app>`, `github.com/storytold/<app>`).

use serde_json::{Value, json};

use crate::CadApp;

pub const DISCORD: &str = "https://discord.gg/artcraft";
pub const ARTCRAFT_WEBSITE: &str = "https://getartcraft.com";
pub const APP_PAGE: &str = "https://getartcraft.com/apps/cadcraft";
pub const GITHUB: &str = "https://github.com/storytold/cadcraft";
pub const ISSUES: &str = "https://github.com/storytold/cadcraft/issues";

/// Help-menu link commands: (id, url). Labels live in `menus::UI_COMMANDS`.
pub const COMMANDS: &[(&str, &str)] = &[
    ("help.discord", DISCORD),
    ("help.website", APP_PAGE),
    ("help.artcraftWebsite", ARTCRAFT_WEBSITE),
    ("help.github", GITHUB),
    ("help.reportIssue", ISSUES),
];

/// Resolve a menu link command ID to its URL.
pub fn url_for(id: &str) -> Option<&'static str> {
    match id {
        // Keep the legacy ID working for callers using the original Discord command.
        "ui.discord" => Some(DISCORD),
        other => COMMANDS.iter().find(|command| command.0 == other).map(|command| command.1),
    }
}

/// Open `url` in the system browser and note it in the status bar.
/// Prefers the platform `open_url` service; falls back to queuing
/// `ui.pending_url` for `ctx.open_url`.
pub fn open(app: &mut CadApp, url: &str) -> Value {
    if app.services.open_url.as_ref().is_some_and(|f| f(url).is_ok()) {
        app.ui.pending_url = None;
    } else {
        app.ui.pending_url = Some(url.to_string());
    }

    app.set_status(format!("Opened {url}"));
    json!({ "url": url })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_engine::Session;
    use std::sync::{Arc, Mutex};

    #[test]
    fn url_for_resolves_help_links_and_legacy_discord_id() {
        for (id, url) in COMMANDS {
            assert_eq!(url_for(id), Some(*url), "{id}");
        }

        assert_eq!(url_for("ui.discord"), Some(DISCORD));
        assert!(url_for("help.appPage").is_none());
        assert!(url_for("unknown").is_none());
    }

    #[test]
    fn links_open_through_the_platform_service() {
        let opened: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let rec = opened.clone();

        let services = crate::Services {
            open_url: Some(Box::new(move |url: &str| {
                rec.lock().unwrap().push(url.to_string());
                Ok(())
            })),
            ..Default::default()
        };

        let mut app = CadApp::new(Session::empty(), services);

        // Every Help-menu link command reaches the service with its URL.
        for (id, url) in COMMANDS {
            let result = app.run(id, json!({})).unwrap();

            assert_eq!(result["url"], *url, "{id}");
            assert_eq!(opened.lock().unwrap().last().map(String::as_str), Some(*url), "{id}");
            assert_eq!(app.status.as_ref().map(|(status, _)| status.as_str()), Some(format!("Opened {url}").as_str()));
        }

        // Direct opening also clears any stale pending URL after success.
        app.ui.pending_url = Some("https://example.com/stale".to_string());

        open(&mut app, DISCORD);

        assert_eq!(opened.lock().unwrap().last().map(String::as_str), Some(DISCORD));
        assert_eq!(app.ui.pending_url, None);
    }

    #[test]
    fn help_menu_lists_destinations_in_exact_order() {
        let app = CadApp::new(Session::empty(), Default::default());
        let tree = crate::menus::tree(&app);

        let help = tree.into_iter().find(|(menu, _)| menu == "Help").map(|(_, entries)| entries).unwrap_or_default();

        let items: Vec<(String, String)> = help
            .into_iter()
            .filter_map(|entry| match entry {
                crate::menus::Entry::Item { label, id, .. } => Some((label, id)),
                _ => None,
            })
            .collect();

        let expected = vec![
            ("Open Sample Drawing".to_string(), "ui.sample".to_string()),
            ("Join the CADCraft Discord...".to_string(), "help.discord".to_string()),
            ("CADCraft Website".to_string(), "help.website".to_string()),
            ("ArtCraft Website".to_string(), "help.artcraftWebsite".to_string()),
            ("CADCraft on GitHub".to_string(), "help.github".to_string()),
            ("Report an Issue...".to_string(), "help.reportIssue".to_string()),
            ("About CADCraft".to_string(), "ui.dialog.about".to_string()),
            ("Command Reference".to_string(), "ui.dialog.commands".to_string()),
        ];

        assert_eq!(items, expected);
    }

    #[test]
    fn link_commands_fall_back_to_pending_url_without_service() {
        let mut app = CadApp::new(Session::empty(), Default::default());

        assert_eq!(app.ui.pending_url, None);

        let result = app.run("help.website", json!({})).unwrap();

        assert_eq!(result["url"], APP_PAGE);
        assert_eq!(app.ui.pending_url.as_deref(), Some(APP_PAGE));
        assert_eq!(app.status.as_ref().map(|(status, _)| status.as_str()), Some(format!("Opened {APP_PAGE}").as_str()));

        // When the platform service fails, queue the URL for fallback.
        let failing_services = crate::Services { open_url: Some(Box::new(|_| Err("failed".to_string()))), ..Default::default() };

        let mut app_fail = CadApp::new(Session::empty(), failing_services);

        open(&mut app_fail, GITHUB);

        assert_eq!(app_fail.ui.pending_url.as_deref(), Some(GITHUB));
    }
}
