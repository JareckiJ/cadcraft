//! One background update check at startup, the "Update and Restart" prompt, and Help ▸ Check for
//! Updates… (desktop only; the web build is always the latest).

use std::sync::mpsc::{Receiver, channel};

use craft_update::suite::CADCRAFT;
use craft_update::{Release, StartupPrompt, StartupPromptResponse};

use crate::CadApp;

/// The version this build reports to the update check.
const VERSION: &str = env!("CARGO_PKG_VERSION");

type CheckResult = Result<Option<Release>, String>;

/// What the update check and prompt are doing between frames.
#[derive(Default)]
pub struct Runtime {
    /// Whether the host asked for a startup check (see [`CadApp::enable_update_check`]).
    enabled: bool,
    /// Whether the startup check has started.
    started: bool,
    /// Whether the running check came from the menu, so an answer of "up to date" is told.
    manual: bool,
    check_rx: Option<Receiver<CheckResult>>,
    prompt: Option<StartupPrompt>,
    update_rx: Option<Receiver<Result<(), String>>>,
}

impl CadApp {
    /// Turn on the startup update check (the desktop host does; tests and the web build do not).
    pub fn enable_update_check(&mut self) {
        self.updates_rt.enabled = true;
    }
}

fn spawn_check(app: &mut CadApp, ctx: &egui::Context) {
    if app.updates_rt.check_rx.is_some() {
        return;
    }
    let (tx, rx) = channel();
    app.updates_rt.check_rx = Some(rx);
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let result = craft_update::Http::new(&format!("CADCraft/{VERSION}"))
            .map_err(|e| format!("could not initialize HTTP: {e}"))
            .and_then(|http| craft_update::check_for_update(&CADCRAFT, &http, VERSION));
        let _ = tx.send(result);
        ctx.request_repaint();
    });
}

/// Help ▸ Check for Updates…: check now, and say so when there is nothing newer.
pub fn check_now(app: &mut CadApp) {
    if app.updates_rt.check_rx.is_none() {
        app.updates_rt.manual = true;
        // Started by the next frame's `tick`, which has the egui context.
        app.updates_rt.started = false;
        app.updates_rt.enabled = true;
    }
}

/// Start the startup check once, and collect the answer.
pub fn tick(app: &mut CadApp, ctx: &egui::Context) {
    if app.updates_rt.enabled && !app.updates_rt.started {
        app.updates_rt.started = true;
        spawn_check(app, ctx);
    }
    let Some(rx) = &app.updates_rt.check_rx else {
        return;
    };
    match rx.try_recv() {
        Ok(result) => {
            app.updates_rt.check_rx = None;
            let manual = std::mem::take(&mut app.updates_rt.manual);
            match result {
                Ok(Some(release)) => app.updates_rt.prompt = Some(StartupPrompt::new(&CADCRAFT, release, VERSION)),
                Ok(None) if manual => app.session.echo(crate::tl!("CADCraft is up to date.")),
                Ok(None) => {}
                Err(e) => {
                    log::warn!("CADCraft update check failed: {e}");
                    if manual {
                        app.session.echo(crate::tl!("Could not check for updates."));
                    }
                }
            }
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
        Err(std::sync::mpsc::TryRecvError::Disconnected) => app.updates_rt.check_rx = None,
    }
}

/// Draw the prompt while there is one and act on its answer.
pub fn show(app: &mut CadApp, ctx: &egui::Context) {
    let Some(prompt) = &mut app.updates_rt.prompt else {
        return;
    };
    if let Some(result) = app.updates_rt.update_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
        app.updates_rt.update_rx = None;
        match result {
            // The guarded close path asks about unsaved changes; the restart helper relaunches
            // once the process has exited.
            Ok(()) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Err(e) => {
                prompt.status_message = Some(e);
                prompt.is_installing = false;
            }
        }
    }
    match prompt.show(ctx) {
        StartupPromptResponse::Pending => {}
        StartupPromptResponse::UpdateAndRestart => {
            prompt.is_installing = true;
            if let Some(install) = craft_update::self_update::current(&CADCRAFT) {
                let release = prompt.release.clone();
                let (tx, rx) = channel();
                app.updates_rt.update_rx = Some(rx);
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    let result = install_release(&install, &release);
                    if let Err(e) = &result {
                        log::warn!("CADCraft self-update failed: {e}");
                    }
                    let _ = tx.send(result);
                    ctx.request_repaint();
                });
            } else {
                prompt.status_message = Some("Cannot auto-update this installation type. Please download manually.".to_string());
                prompt.is_installing = false;
            }
        }
        StartupPromptResponse::Download => {
            let url = prompt.release.html_url.clone();
            app.updates_rt.prompt = None;
            ctx.open_url(egui::OpenUrl::new_tab(url));
        }
        StartupPromptResponse::Later => app.updates_rt.prompt = None,
    }
}

fn install_release(install: &craft_update::self_update::Install, release: &Release) -> Result<(), String> {
    let http = craft_update::Http::new(&format!("CADCraft/{VERSION}")).map_err(|e| format!("could not initialize HTTP: {e}"))?;
    let release = if release.assets.is_empty() {
        craft_update::check_for_update(&CADCRAFT, &http, VERSION).ok().flatten().unwrap_or_else(|| release.clone())
    } else {
        release.clone()
    };
    let paths = craft_update::Paths::for_app(&CADCRAFT).map_err(|e| format!("could not find paths: {e}"))?;
    let platform = craft_update::Platform::current().ok_or("unsupported platform for self update")?;
    craft_update::self_update::update(
        &CADCRAFT,
        install,
        &release,
        platform,
        &http,
        &paths,
        &craft_update::Stop::default(),
        &mut |_| {},
        &|| Ok(()),
        Some(VERSION),
    )
    .map_err(|e| e.to_string())
}
