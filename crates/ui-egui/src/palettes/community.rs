//! The ECAD Community Hub palette: sign in with GitHub, search the community parts database,
//! use a part on the selected device, and share a symbol's part with others.
//!
//! Network calls (sign-in, search, upload) run on background threads and report into a shared
//! state the palette reads each frame; the UI never blocks.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use cadcraft_ecad::{COMPONENT_TYPES, ManufacturerPart, PART_LICENSE, PinPoint, SymbolGeometry};
use cadcraft_ecad_cloud::{CloudConfig, CloudError, Profile};
use cadcraft_engine::doc::{EntityKind, Handle};
use egui::{RichText, vec2};
use serde_json::{Value, json};

use crate::CadApp;
use crate::theme::Tokens;

/// What the background work reports.
#[derive(Default)]
struct Shared {
    config: CloudConfig,
    loaded: bool,
    profile: Option<Profile>,
    /// While signing in: the sign-in page (shown in case the browser did not open).
    signing_in: Option<String>,
    cancel: Option<Arc<AtomicBool>>,
    busy: bool,
    results: Vec<ManufacturerPart>,
    message: Option<String>,
}

fn shared() -> &'static Mutex<Shared> {
    static S: OnceLock<Mutex<Shared>> = OnceLock::new();
    S.get_or_init(Mutex::default)
}

fn lock() -> std::sync::MutexGuard<'static, Shared> {
    match shared().lock() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    }
}

fn report(r: Result<String, CloudError>) {
    let mut s = lock();
    s.message = Some(match r {
        Ok(m) => m,
        Err(e) => e.to_string(),
    });
    s.busy = false;
}

/// Whether someone is signed in (the Devices palette's Share button asks).
pub fn signed_in() -> bool {
    lock().profile.is_some()
}

/// Read the settings and the saved session once, in the background (a refresh may go online).
fn ensure_loaded() {
    let mut s = lock();
    if s.loaded {
        return;
    }
    s.loaded = true;
    s.config = CloudConfig::load();
    let c = s.config.clone();
    drop(s);
    std::thread::spawn(move || {
        let r = cadcraft_ecad_cloud::current_session(&c);
        let mut s = lock();
        match r {
            Ok(session) => s.profile = session.map(|x| x.profile),
            Err(CloudError::NotConfigured) => {}
            Err(e) => s.message = Some(e.to_string()),
        }
    });
}

fn sign_in() {
    let mut s = lock();
    if s.signing_in.is_some() {
        return;
    }
    let cancel = Arc::new(AtomicBool::new(false));
    s.cancel = Some(cancel.clone());
    s.signing_in = Some(String::new());
    s.message = None;
    let c = s.config.clone();
    drop(s);
    std::thread::spawn(move || {
        let r = cadcraft_ecad_cloud::sign_in(&c, &cancel, |url| {
            lock().signing_in = Some(url.to_string());
            let _ = cadcraft_ecad_cloud::open_in_browser(url);
        });
        let mut s = lock();
        s.signing_in = None;
        s.cancel = None;
        match r {
            Ok(session) => {
                s.message = Some(format!("{} {}.", crate::tl!("Signed in as"), session.profile.github_login));
                s.profile = Some(session.profile);
            }
            Err(e) => s.message = Some(e.to_string()),
        }
    });
}

fn search(query: String) {
    let mut s = lock();
    s.busy = true;
    let c = s.config.clone();
    drop(s);
    std::thread::spawn(move || match cadcraft_ecad_cloud::search(&c, &query, 50) {
        Ok(parts) => {
            let n = parts.len();
            lock().results = parts;
            report(Ok(format!("{n} {}", crate::tl!("parts found."))));
        }
        Err(e) => report(Err(e)),
    });
}

fn upload(part: ManufacturerPart) {
    let mut s = lock();
    s.busy = true;
    let c = s.config.clone();
    drop(s);
    std::thread::spawn(move || {
        let r = cadcraft_ecad_cloud::submit(&c, &part)
            .map(|p| format!("{} {} {} {}", crate::tl!("Shared:"), p.manufacturer, p.part_number, crate::tl!("is now searchable by everyone.")));
        if matches!(r, Err(CloudError::SessionExpired)) {
            lock().profile = None;
        }
        report(r);
    });
}

/// The selected symbol (one block reference with connection points), if any.
fn selected_symbol(app: &CadApp) -> Option<Handle> {
    let sel = app.session.selection();
    let [h] = sel.as_slice() else { return None };
    let d = app.session.doc().ok()?;
    match d.entity(*h).map(|e| &e.kind) {
        Some(EntityKind::Insert(i)) if i.attribs.iter().any(|a| a.tag.to_ascii_uppercase().contains("TERM")) => Some(*h),
        _ => None,
    }
}

/// A shareable part from a placed symbol: its block's drawing and pins, its device data.
pub fn part_from_symbol(app: &mut CadApp, h: Handle, component_type: &str) -> Result<ManufacturerPart, String> {
    let dev = app.session.execute("edb.device", &json!({ "handle": h.hex() })).map_err(|e| e.to_string())?;
    let d = app.session.doc().map_err(|e| e.to_string())?;
    let Some(EntityKind::Insert(ins)) = d.entity(h).map(|e| &e.kind) else { return Err(crate::tl!("Select one symbol.").into()) };
    let blk = d.block(&ins.block).ok_or_else(|| crate::tl!("The symbol's block is missing.").to_string())?;
    let base = blk.base.xy();
    let rel = |x: f64, y: f64| [x - base.x, y - base.y];
    let mut g = SymbolGeometry::default();
    let mut numbers: BTreeMap<String, String> = BTreeMap::new();
    let mut points: Vec<(String, f64, f64, u8)> = Vec::new();
    for e in blk.entities.iter() {
        match &e.kind {
            EntityKind::Line(l) => {
                let (a, b) = (rel(l.a.x, l.a.y), rel(l.b.x, l.b.y));
                g.lines.push([a[0], a[1], b[0], b[1]]);
            }
            EntityKind::LwPolyline(pl) => {
                let mut v: Vec<[f64; 2]> = pl.vertices.iter().map(|v| rel(v.p.x, v.p.y)).collect();
                if pl.closed
                    && let Some(f) = v.first().copied()
                {
                    v.push(f);
                }
                for w in v.windows(2) {
                    if let [a, b] = w {
                        g.lines.push([a[0], a[1], b[0], b[1]]);
                    }
                }
            }
            EntityKind::Circle(c) => {
                let p = rel(c.center.x, c.center.y);
                g.circles.push([p[0], p[1], c.radius]);
            }
            EntityKind::Arc(a) => {
                let p = rel(a.center.x, a.center.y);
                g.arcs.push([p[0], p[1], a.radius, a.start.to_degrees(), a.end.to_degrees()]);
            }
            EntityKind::Text(t) => {
                let p = rel(t.insert.x, t.insert.y);
                g.texts.push(([p[0], p[1], t.height], t.value.clone()));
            }
            EntityKind::AttDef(a) => {
                let tag = a.tag.to_ascii_uppercase();
                if let Some(nn) = tag.strip_prefix("TERM") {
                    numbers.insert(nn.to_string(), a.text.value.clone());
                } else if let Some(rest) = tag.strip_prefix('X')
                    && let Some((dir, nn)) = rest.split_once("TERM")
                    && let Ok(dir) = dir.parse::<u8>()
                {
                    let p = rel(a.text.insert.x, a.text.insert.y);
                    points.push((nn.to_string(), p[0], p[1], dir));
                }
            }
            _ => {}
        }
    }
    let pin_map: BTreeMap<String, PinPoint> = points
        .into_iter()
        .map(|(nn, x, y, dir)| {
            let name = numbers.get(&nn).filter(|n| !n.is_empty()).cloned().unwrap_or(nn);
            let dir = match dir {
                2 => "right",
                4 => "down",
                8 => "left",
                _ => "up",
            };
            (name, PinPoint { x, y, dir: dir.into() })
        })
        .collect();
    let s = |v: &Value| v.as_str().unwrap_or("").trim().to_string();
    let attributes: BTreeMap<String, Value> = dev["part"]["ratings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| Some((r["name"].as_str()?.to_string(), json!({ "value": r["value"], "unit": r["unit"] }))))
        .collect();
    let description =
        dev["part"]["description"].as_array().map(|a| a.iter().map(s).filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" ")).unwrap_or_default();
    let class = s(&dev["class"]);
    let part = ManufacturerPart {
        part_number: s(&dev["part"]["catalog"]),
        manufacturer: s(&dev["part"]["manufacturer"]),
        component_type: component_type.to_string(),
        iec_class: (!class.is_empty()).then_some(class),
        description,
        pin_map,
        attributes,
        cad_geometry: g,
        license: PART_LICENSE.into(),
        ..Default::default()
    };
    part.validate()
        .map_err(|e| format!("{e} {}", crate::tl!("Set the manufacturer and catalogue number in the Electrical Devices palette first.")))?;
    Ok(part)
}

/// Apply a community part to the selected symbol's device: manufacturer, number, description,
/// ratings (written into the symbol too).
fn use_part(app: &mut CadApp, part: &ManufacturerPart) -> Result<(), String> {
    let h = selected_symbol(app).ok_or_else(|| crate::tl!("Select one symbol in the drawing first.").to_string())?;
    let ratings: Vec<Value> = part
        .attributes
        .iter()
        .filter_map(|(k, v)| {
            Some(json!({ "name": k, "value": v.get("value")?.as_f64()?, "unit": v.get("unit").and_then(Value::as_str).unwrap_or("") }))
        })
        .collect();
    app.run(
        "edb.device",
        json!({ "handle": h.hex(), "set": {
            "manufacturer": part.manufacturer, "catalog": part.part_number, "description": [part.description], "ratings": ratings,
        } }),
    )
    .map(|_| ())
}

/// Palette-local state (text being typed).
#[derive(Clone, Default)]
struct Local {
    query: String,
    component_type: usize,
    agreed: bool,
    url: String,
    key: String,
    settings_open: bool,
    filled: bool,
}

fn id() -> egui::Id {
    egui::Id::new("cadcraft-ecad-community")
}

/// Open the palette (floating by the drawing's right edge the first time).
pub fn open(app: &mut CadApp) -> Result<Value, String> {
    let panel = crate::docking::Panel::Community;
    let first = !app.ui.docking.layout.contains(&panel) && !app.ui.docking.hidden.iter().any(|(p, _)| *p == panel);
    let r = crate::docking::command(app, false, &json!({ "operation": "open", "panel": panel.id() }))?;
    if !first {
        return Ok(r);
    }
    let area = app.canvas.rect.unwrap_or(egui::Rect::from_min_size(egui::pos2(220.0, 80.0), vec2(1000.0, 800.0)));
    let (w, h) = (360.0, (area.height() - 40.0).clamp(260.0, 720.0));
    let rect = [area.right() - w - 16.0, area.top() + 24.0, w, h];
    crate::docking::command(app, false, &json!({ "operation": "float", "panel": panel.id(), "rect": rect }))
}

pub fn show(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    ensure_loaded();
    let mut st: Local = ui.ctx().data_mut(|d| d.get_temp(id())).unwrap_or_default();
    let (profile, signing_in, busy, message, results, config) = {
        let s = lock();
        (s.profile.clone(), s.signing_in.clone(), s.busy, s.message.clone(), s.results.clone(), s.config.clone())
    };
    if !st.filled {
        st.url = config.url.clone();
        st.key = config.anon_key.clone();
        st.settings_open = config.validated().is_err();
        st.filled = true;
    }
    if signing_in.is_some() || busy {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
    }
    egui::ScrollArea::vertical().id_salt("dock-community").auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space(6.0);
        // Account.
        match (&profile, &signing_in) {
            (Some(p), _) => {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter().circle_filled(r.center(), 4.5, egui::Color32::from_rgb(46, 160, 67));
                    ui.label(RichText::new(format!("{} {}", crate::tl!("Logged in as"), p.github_login)).strong());
                    if ui.small_button(crate::tl!("Sign out")).clicked() {
                        let _ = cadcraft_ecad_cloud::sign_out();
                        lock().profile = None;
                    }
                });
            }
            (None, Some(url)) => {
                ui.label(crate::tl!("Waiting for GitHub in your browser…"));
                if !url.is_empty() {
                    ui.label(RichText::new(crate::tl!("If no browser opened, open this address:")).color(t.text_dim));
                    let mut u = url.clone();
                    ui.add(egui::TextEdit::singleline(&mut u).desired_width(f32::INFINITY));
                }
                if ui.button(crate::tl!("Cancel")).clicked()
                    && let Some(c) = lock().cancel.clone()
                {
                    c.store(true, Ordering::Relaxed);
                }
            }
            (None, None) => {
                ui.label(crate::tl!("Log in to contribute parts to the community database."));
                let ready = config.validated().is_ok();
                let b = ui.add_enabled(ready, egui::Button::new(RichText::new(crate::tl!("Link GitHub Account")).strong()));
                if b.clicked() {
                    sign_in();
                }
                if !ready {
                    b.on_disabled_hover_text(crate::tl!("Set the database address and public key below first."));
                }
            }
        }
        if let Some(m) = &message {
            ui.label(RichText::new(m).color(t.text_dim));
        }
        ui.separator();

        // Search.
        ui.label(RichText::new(crate::tl!("Community parts")).strong());
        ui.horizontal(|ui| {
            let r = ui.add(
                egui::TextEdit::singleline(&mut st.query)
                    .hint_text(crate::tl!("Part number, maker or type"))
                    .desired_width(ui.available_width() - 70.0),
            );
            let go = ui.add_enabled(!busy && config.validated().is_ok(), egui::Button::new(crate::tl!("Search")));
            if go.clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                search(st.query.clone());
            }
        });
        let mut chosen: Option<ManufacturerPart> = None;
        egui::ScrollArea::vertical().id_salt("community-results").max_height(220.0).show(ui, |ui| {
            for p in &results {
                ui.horizontal(|ui| {
                    let by = p.github_login.as_deref().map(|l| format!(" · @{l}")).unwrap_or_default();
                    ui.label(RichText::new(format!("{} · {} · {}{by}", p.part_number, p.manufacturer, p.component_type)).font(crate::theme::small()));
                    if ui.small_button(crate::tl!("Use")).on_hover_text(crate::tl!("Apply to the selected symbol")).clicked() {
                        chosen = Some(p.clone());
                    }
                });
            }
        });
        if let Some(p) = chosen {
            lock().message = Some(match use_part(app, &p) {
                Ok(()) => format!("{} {}.", crate::tl!("Applied"), p.part_number),
                Err(e) => e,
            });
        }
        ui.separator();

        // Share.
        ui.label(RichText::new(crate::tl!("Share a part")).strong());
        let symbol = selected_symbol(app);
        ui.label(
            RichText::new(if symbol.is_some() {
                crate::tl!("Shares the selected symbol: its drawing, pins, maker, number and ratings.")
            } else {
                crate::tl!("Select one symbol in the drawing to share its part.")
            })
            .color(t.text_dim),
        );
        egui::ComboBox::from_id_salt("community-type").selected_text(COMPONENT_TYPES.get(st.component_type).copied().unwrap_or("Other")).show_ui(
            ui,
            |ui| {
                for (i, ty) in COMPONENT_TYPES.iter().enumerate() {
                    ui.selectable_value(&mut st.component_type, i, *ty);
                }
            },
        );
        ui.checkbox(&mut st.agreed, crate::tl!("I drew this symbol myself and share it under CC BY 4.0."));
        let can = profile.is_some() && symbol.is_some() && st.agreed && !busy;
        let share = ui.add_enabled(can, egui::Button::new(RichText::new(crate::tl!("Share Part with Community")).strong()));
        let share = if profile.is_none() { share.on_disabled_hover_text(crate::tl!("Sign in with GitHub to share parts.")) } else { share };
        if share.clicked()
            && let Some(h) = symbol
        {
            let ty = COMPONENT_TYPES.get(st.component_type).copied().unwrap_or("Other");
            match part_from_symbol(app, h, ty) {
                Ok(part) => upload(part),
                Err(e) => lock().message = Some(e),
            }
        }
        ui.separator();

        // Settings.
        egui::CollapsingHeader::new(crate::tl!("Database settings")).id_salt("community-settings").open(st.settings_open.then_some(true)).show(
            ui,
            |ui| {
                st.settings_open = false;
                ui.label(RichText::new(crate::tl!("Supabase project URL")).font(crate::theme::small()));
                ui.add(egui::TextEdit::singleline(&mut st.url).hint_text("https://….supabase.co").desired_width(f32::INFINITY));
                ui.label(RichText::new(crate::tl!("Public (anon) key")).font(crate::theme::small()));
                ui.add(egui::TextEdit::singleline(&mut st.key).password(true).desired_width(f32::INFINITY));
                if ui.button(crate::tl!("Save")).clicked() {
                    let c = CloudConfig { url: st.url.trim().into(), anon_key: st.key.trim().into() };
                    let r = c.validated().and_then(|_| c.save()).map(|()| crate::tl!("Settings saved.").to_string());
                    if r.is_ok() {
                        let mut s = lock();
                        s.config = c;
                        s.loaded = false;
                    }
                    report(r);
                }
            },
        );
    });
    ui.ctx().data_mut(|d| d.insert_temp(id(), st));
}
