//! CADCraft in the browser.
//!
//! Runs the same [`cadcraft_ui_egui::CadApp`] as the desktop app through eframe's web runner
//! (wgpu: WebGPU where available, WebGL2 otherwise). Build with `trunk build --release` from this
//! directory (output in `dist/web`). URL flags: `?webgl` forces WebGL2, `?cpu` draws the drawing
//! with egui shapes instead of the GPU canvas, `?sample` opens the sample drawing. Saving downloads
//! the drawing through the browser (`download.rs`). With `?host=parent` (inside an `<iframe>`), a
//! same-origin parent page sends the drawing to open and stores what CADCraft saves instead
//! (`host.rs`).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(target_arch = "wasm32")]
pub mod download;
#[cfg(target_arch = "wasm32")]
mod host;

#[cfg(target_arch = "wasm32")]
mod web {
    use cadcraft_engine::Session;
    use cadcraft_ui_egui::{CadApp, Services};
    use wasm_bindgen::JsCast as _;

    const CANVAS_ID: &str = "cadcraft_canvas";
    const LOADING_ID: &str = "cadcraft_loading";

    /// The app, the files File > Open… and drag and drop read for it in the background, the
    /// page embedding it (`?host=parent`), the browser's `prefers-reduced-motion` (it stays
    /// live, so View ▸ Accessibility ▸ Animation Effects ▸ Use System Setting follows a change),
    /// and the success replies still owed to that page.
    struct Shell(
        CadApp,
        crate::upload::Inbox,
        Option<(crate::host::Bridge, crate::host::DirtyReport)>,
        Option<web_sys::MediaQueryList>,
        crate::PendingOpens,
    );

    impl eframe::App for Shell {
        fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
            self.1.take_dropped(ctx);
            // A drawing the page embedding CADCraft sent opens here. Its success reply waits for
            // the UI pass that shows it; failures can be answered at once (#1666).
            let drained = self.1.drain(&mut self.0);
            if let Some((bridge, _)) = &self.2 {
                for reply in &drained.replies {
                    bridge.post_open(reply);
                }
            }
            for (name, uid) in drained.opened {
                self.4.opened(&name, uid);
            }
            self.0.system_animations = self.3.as_ref().map(|reduced| !reduced.matches());
            self.0.logic(ctx);
            if let Some((bridge, reported)) = &self.2 {
                bridge.report_dirty(&self.0, reported);
            }
        }
        fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
            self.0.raw_input_hook(raw);
            cadcraft_ui_egui::cmdline::capture_tab(ctx, raw);
        }
        /// Preferences (interface theme and language) survive reloads; egui state is not kept.
        fn save(&mut self, storage: &mut dyn eframe::Storage) {
            storage.set_string(cadcraft_ui_egui::options::PROFILE_KEY, self.0.profile_json());
            storage.set_string(cadcraft_ui_egui::PREFS_KEY, self.0.prefs_json());
        }
        fn persist_egui_memory(&self) -> bool {
            false
        }
        fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            self.0.ui(ui);
            // The pass just showed the active drawing: answer the upload that opened it, and
            // report an upload a later one superseded before this pass (#1666).
            let shown = self.0.session.state().ok().map(|st| st.uid);
            let replies = self.4.presented(shown);
            if let Some((bridge, _)) = &self.2 {
                for reply in &replies {
                    bridge.post_open(reply);
                }
            }
            if let Some((bridge, reported)) = &self.2 {
                bridge.report_dirty(&self.0, reported);
            }
        }
    }

    fn query() -> String {
        web_sys::window().and_then(|w| w.location().search().ok()).unwrap_or_default()
    }

    fn query_param(key: &str) -> Option<String> {
        web_sys::UrlSearchParams::new_with_str(&query()).ok()?.get(key)
    }

    /// No file picker on the web: Save / Save As from the menu (and the close prompt) download the
    /// drawing under its current name; `saveas {"path": "name.dxf"}` picks another name.
    fn services() -> Services {
        Services { pick_save: Some(Box::new(|name: &str| Some(name.to_string()))), ..Services::default() }
    }

    pub fn start() {
        eframe::WebLogger::init(log::LevelFilter::Info).ok();
        cadcraft_engine::cmd::file::set_io(cadcraft_engine::cmd::file::IoHooks {
            read: |b, name| cadcraft_io::read(b, name).map_err(|e| e.to_string()),
            write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
            plot: Some(|d, space, opts| cadcraft_io::plotter::plot(d, space, opts).map_err(|e| e.to_string())),
        });
        cadcraft_engine::cmd::file::set_write_as(|d, name, o| {
            cadcraft_io::write_with(d, name, &cadcraft_io::SaveOptions { version: o.version, binary: o.binary }).map_err(|e| e.to_string())
        });
        // AutoLISP's entget, entmake and entmod see entities as DXF groups.
        cadcraft_engine::cmd::file::set_entity_codec(cadcraft_engine::cmd::file::EntityCodec {
            groups: cadcraft_io::entity_groups,
            entity: cadcraft_io::entity_from_groups,
        });
        // `?host=parent`: a same-origin page framing CADCraft (such as a Nextcloud app) opens
        // drawings in it and stores what it saves. Otherwise there is no file system here: saved
        // files reach the user as browser downloads.
        let bridge = if query_param("host").as_deref() == Some("parent") { crate::host::Bridge::new() } else { None };
        match &bridge {
            Some(bridge) => {
                bridge.install();
                cadcraft_engine::cmd::file::set_deliver(crate::host::deliver);
            }
            None => cadcraft_engine::cmd::file::set_deliver(crate::download::download),
        }
        let app_bridge = bridge.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let Some(document) = web_sys::window().and_then(|w| w.document()) else { return };
            let Some(canvas) = document.get_element_by_id(CANVAS_ID).and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok()) else {
                log::error!("missing <canvas id=\"{CANVAS_ID}\">");
                return;
            };
            let mut options = eframe::WebOptions::default();
            if query().contains("webgl")
                && let eframe::egui_wgpu::WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup
            {
                create.instance_descriptor.backends = eframe::wgpu::Backends::GL;
            }
            let result = eframe::WebRunner::new()
                .start(
                    canvas,
                    options,
                    Box::new(move |cc| {
                        // Embedded, the parent sends the drawing: start on the Start tab, not Drawing1.
                        let session = if app_bridge.is_some() { Session::empty() } else { Session::new() };
                        let mut app = CadApp::new(session, services());
                        app.system_languages =
                            web_sys::window().map(|w| w.navigator().languages().iter().filter_map(|v| v.as_string()).collect()).unwrap_or_default();
                        if let Some(profile) = cc.storage.and_then(|s| s.get_string(cadcraft_ui_egui::options::PROFILE_KEY)) {
                            app.load_profile(&profile);
                        }
                        if let Some(prefs) = cc.storage.and_then(|s| s.get_string(cadcraft_ui_egui::PREFS_KEY)) {
                            app.load_prefs(&prefs);
                        }
                        // `?cpu`: no render state for the app, so the canvas draws with egui shapes.
                        if !crate::wants_cpu(&query())
                            && let Some(rs) = &cc.wgpu_render_state
                        {
                            // eframe's web runner has no stencil buffer: clipped layout
                            // viewports stay in the layout's display list.
                            app.set_wgpu(rs, None);
                        }
                        if query().contains("sample") {
                            let _ = app.run("ui.sample", serde_json::json!({}));
                        }
                        // No paths on the web: OPEN asks the browser's file picker, which reads the
                        // chosen file in the background (`upload.rs`), so the picker returns none.
                        let inbox = crate::upload::Inbox::new(&cc.egui_ctx);
                        let picker = inbox.clone();
                        app.services.pick_open = Some(Box::new(move || {
                            picker.pick();
                            None
                        }));
                        let host = app_bridge.map(|bridge| {
                            bridge.listen(inbox.clone());
                            bridge.post_ready();
                            (bridge, crate::host::DirtyReport::default())
                        });
                        let reduced_motion = web_sys::window().and_then(|w| w.match_media("(prefers-reduced-motion: reduce)").ok().flatten());
                        Ok(Box::new(Shell(app, inbox, host, reduced_motion, crate::PendingOpens::default())))
                    }),
                )
                .await;
            if let (Err(e), Some(bridge)) = (&result, &bridge) {
                bridge.post_failed(&format!("{e:?}"));
            }
            if let Some(el) = document.get_element_by_id(LOADING_ID) {
                match result {
                    Ok(()) => el.remove(),
                    Err(e) => {
                        el.set_inner_html(&format!("<p>CADCraft failed to start: {e:?}</p><p>A browser with WebGPU or WebGL2 is required.</p>"))
                    }
                }
            }
        });
    }
}

/// True when the URL query (`?cpu&sample`) has a `cpu` flag: the CPU canvas path is requested.
#[cfg(any(target_arch = "wasm32", test))]
fn wants_cpu(query: &str) -> bool {
    query.trim_start_matches('?').split('&').any(|p| p == "cpu" || p.starts_with("cpu="))
}

/// The answer owed to the page embedding CADCraft for a drawing it sent (`?host=parent`): the
/// drawing opened, or it didn't and why (#1666). The hub waits for this instead of guessing.
///
/// The message shapes are pure so they can be tested without a browser; [`crate::host::Bridge`]
/// turns the fields into a `postMessage`.
#[cfg(any(target_arch = "wasm32", test))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenReply {
    /// The drawing's name, as the parent sent it (made safe to open).
    name: String,
    /// Why it couldn't be opened; `None` once it is loaded and showing.
    error: Option<String>,
}

#[cfg(any(target_arch = "wasm32", test))]
impl OpenReply {
    /// What to report for the drawing `name`: opened (`error` `None`) or failed with `error`.
    pub fn new(name: &str, error: Option<&str>) -> Self {
        Self { name: name.to_string(), error: error.map(str::to_string) }
    }

    /// The message's `type`: `cadcraft:opened`, or `cadcraft:open-failed` with a reason.
    pub fn kind(&self) -> &'static str {
        if self.error.is_some() { "cadcraft:open-failed" } else { "cadcraft:opened" }
    }

    /// The message's fields, in posting order: `type`, `name`, and `error` when it failed.
    pub fn fields(&self) -> Vec<(&'static str, &str)> {
        let mut fields = vec![("type", self.kind()), ("name", self.name.as_str())];
        if let Some(error) = &self.error {
            fields.push(("error", error.as_str()));
        }
        fields
    }
}

/// Why an upload whose drawing never reached a UI pass is reported as not opened: another upload
/// draining before that pass became the drawing shown (#1666).
#[cfg(any(target_arch = "wasm32", test))]
const SUPERSEDED_OPEN: &str = "Open: superseded by another drawing before it was shown";

/// Success replies owed to the page embedding CADCraft for drawings it sent, each held until the
/// UI pass that presents its drawing (#1666).
///
/// `logic` runs before `ui` and can run while the UI is hidden, and several uploads can drain
/// before one UI pass. Only the last of those drawings is shown by that pass, so the others are
/// answered as superseded instead of claiming to have opened.
#[cfg(any(target_arch = "wasm32", test))]
#[derive(Default)]
pub struct PendingOpens {
    /// `(name, document uid)` per successful host open not yet answered, in drain order.
    waiting: Vec<(String, u64)>,
}

#[cfg(any(target_arch = "wasm32", test))]
impl PendingOpens {
    /// Hold the success reply for `name`, whose drawing is `uid`, until a UI pass shows it.
    pub fn opened(&mut self, name: &str, uid: u64) {
        self.waiting.push((name.to_string(), uid));
    }

    /// Whether a success reply is still owed (none has been shown yet).
    pub fn is_empty(&self) -> bool {
        self.waiting.is_empty()
    }

    /// Answer every held success after a UI pass showed the drawing `shown`: `cadcraft:opened`
    /// for that drawing, `cadcraft:open-failed` for any earlier one the pass superseded.
    pub fn presented(&mut self, shown: Option<u64>) -> Vec<OpenReply> {
        self.waiting
            .drain(..)
            .map(|(name, uid)| if shown == Some(uid) { OpenReply::new(&name, None) } else { OpenReply::new(&name, Some(SUPERSEDED_OPEN)) })
            .collect()
    }
}

/// What one drain of the upload inbox produced: replies ready to post now (uploads that failed),
/// and successful host opens whose reply waits for the UI pass that shows their drawings (#1666).
#[cfg(any(target_arch = "wasm32", test))]
#[derive(Default)]
pub struct Drain {
    /// Failures to post at once: no drawing opened, so no UI pass is involved.
    pub replies: Vec<OpenReply>,
    /// `(name, drawing uid)` per host upload that opened a drawing, in drain order.
    pub opened: Vec<(String, u64)>,
}

/// Why a drawing the page embedding CADCraft sent is refused before it reaches the app, if it is:
/// that channel only opens drawings, while a picked or dropped `.lsp` loads as APPLOAD loads it
/// (#1666).
#[cfg(any(target_arch = "wasm32", test))]
pub fn host_open_error(name: &str) -> Option<String> {
    cadcraft_ui_egui::is_lisp_file(name).then(|| format!("Open: {name} is a LISP file, not a drawing"))
}

#[cfg(target_arch = "wasm32")]
mod upload;

#[cfg(target_arch = "wasm32")]
fn main() {
    web::start();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("cadcraft-web only runs in the browser: build it with `trunk build --release` in apps/cadcraft-web");
}

#[cfg(test)]
mod tests {
    use super::{OpenReply, PendingOpens, SUPERSEDED_OPEN, host_open_error, wants_cpu};

    /// #1666: the page embedding CADCraft hears `cadcraft:opened` once the drawing it sent is
    /// loaded and showing, or `cadcraft:open-failed` with the reason.
    #[test]
    fn host_open_replies_name_the_drawing_and_the_failure() {
        let opened = OpenReply::new("Bracket.dxf", None);
        assert_eq!(opened.fields(), vec![("type", "cadcraft:opened"), ("name", "Bracket.dxf")]);

        let failed = OpenReply::new("Bracket.dxf", Some("Open: not a drawing"));
        assert_eq!(failed.fields(), vec![("type", "cadcraft:open-failed"), ("name", "Bracket.dxf"), ("error", "Open: not a drawing")]);
    }

    /// #1666: a drawing the parent sent is answered `cadcraft:opened` only once a UI pass has
    /// shown it. `logic` can run with the UI hidden, so the success reply waits.
    #[test]
    fn a_host_drawing_is_answered_opened_once_a_ui_pass_shows_it() {
        let mut pending = PendingOpens::default();
        pending.opened("Bracket.dxf", 7);
        assert!(!pending.is_empty(), "no UI pass yet: the reply is still owed");

        let replies = pending.presented(Some(7));
        assert_eq!(replies, vec![OpenReply::new("Bracket.dxf", None)]);
        assert!(pending.is_empty());

        // One reply per open, not one per UI pass.
        assert!(pending.presented(Some(7)).is_empty());
    }

    /// #1666: two uploads can drain before one UI pass. Only the last drawing is shown, so the
    /// first must not claim `cadcraft:opened`; it reports why it was not shown instead.
    #[test]
    fn uploads_drained_before_one_ui_pass_report_only_the_shown_drawing_as_opened() {
        let mut pending = PendingOpens::default();
        pending.opened("First.dxf", 1);
        pending.opened("Second.dxf", 2);

        let replies = pending.presented(Some(2));
        let fields: Vec<_> = replies.iter().map(OpenReply::fields).collect();
        assert_eq!(
            fields,
            vec![
                vec![("type", "cadcraft:open-failed"), ("name", "First.dxf"), ("error", SUPERSEDED_OPEN)],
                vec![("type", "cadcraft:opened"), ("name", "Second.dxf")],
            ]
        );
    }

    /// #1666: each success stays associated with the upload that opened it, across drains and
    /// UI passes; a drawing shown by its own pass is not reported again later.
    #[test]
    fn each_drained_upload_keeps_its_own_reply() {
        let mut pending = PendingOpens::default();
        pending.opened("First.dxf", 1);
        assert_eq!(pending.presented(Some(1)), vec![OpenReply::new("First.dxf", None)]);

        pending.opened("Second.dxf", 2);
        pending.opened("Third.dxf", 3);
        let replies = pending.presented(Some(3));
        assert_eq!(replies.first(), Some(&OpenReply::new("Second.dxf", Some(SUPERSEDED_OPEN))));
        assert_eq!(replies.get(1), Some(&OpenReply::new("Third.dxf", None)));
    }

    /// #1666: the host channel opens drawings only. A `.lsp` name is refused like a failed open
    /// instead of being loaded as LISP and answered `cadcraft:opened`.
    #[test]
    fn host_supplied_lisp_files_are_refused_as_drawings() {
        assert_eq!(host_open_error("Bracket.dxf"), None);
        assert_eq!(host_open_error("Bracket.DWG"), None);
        assert_eq!(host_open_error("helpers.lsp"), Some("Open: helpers.lsp is a LISP file, not a drawing".into()));
        assert_eq!(host_open_error("HELPERS.LSP"), Some("Open: HELPERS.LSP is a LISP file, not a drawing".into()));
    }

    #[test]
    fn cpu_flag_is_detected() {
        assert!(wants_cpu("?cpu"));
        assert!(wants_cpu("?cpu&sample"));
        assert!(wants_cpu("?sample&cpu"));
        assert!(!wants_cpu(""));
        assert!(!wants_cpu("?sample"));
        assert!(!wants_cpu("?webgl&sample"));
        assert!(!wants_cpu("?gpucpu"));
    }
}
