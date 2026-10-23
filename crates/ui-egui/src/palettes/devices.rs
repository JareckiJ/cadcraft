//! The Electrical Devices palette: a thin face on the project database. It lists the devices
//! (EDB.QUERY), follows the symbol selected in the drawing, edits a device's tag and part data
//! (EDB.DEVICE, which writes the change back into its symbol on whichever sheet it is) and shows
//! what it is wired to.

use cadcraft_engine::doc::{EntityKind, Handle};
use egui::{RichText, vec2};
use serde_json::{Value, json};

use crate::CadApp;
use crate::theme::Tokens;

/// How often the list is read again while the palette is shown (seconds).
const REFRESH_SECONDS: f64 = 2.0;

/// The device being edited.
#[derive(Clone, Default)]
struct Edit {
    id: String,
    tag: String,
    manufacturer: String,
    catalog: String,
    desc: [String; 3],
    location: String,
    installation: String,
    /// (name, value, unit) as typed.
    ratings: Vec<(String, String, String)>,
    block: String,
    reference: String,
    child: bool,
}

impl Edit {
    fn from_device(d: &Value) -> Self {
        let s = |v: &Value| v.as_str().unwrap_or("").to_string();
        let part = &d["part"];
        let desc = |i: usize| part["description"].get(i).map(s).unwrap_or_default();
        let reference = match (d["graphic"]["reference"][0].as_u64(), d["graphic"]["reference"][1].as_u64()) {
            (Some(sh), Some(c)) => format!("{sh}{c:02}"),
            _ => String::new(),
        };
        Edit {
            id: s(&d["id"]),
            tag: s(&d["tag"]),
            manufacturer: s(&part["manufacturer"]),
            catalog: s(&part["catalog"]),
            desc: [desc(0), desc(1), desc(2)],
            location: s(&d["location"]),
            installation: s(&d["installation"]),
            ratings: part["ratings"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|r| (s(&r["name"]), r["value"].as_f64().map(|v| v.to_string()).unwrap_or_default(), s(&r["unit"])))
                .collect(),
            block: s(&d["graphic"]["block"]),
            reference,
            child: d["child"].as_bool().unwrap_or(false),
        }
    }

    /// The `set` parameter of EDB.DEVICE; `Err` names a rating whose value is not a number.
    fn to_set(&self) -> Result<Value, String> {
        let mut ratings = Vec::new();
        for (name, value, unit) in self.ratings.iter().filter(|r| !r.0.trim().is_empty()) {
            let v: f64 = value.trim().replace(',', ".").parse().map_err(|_| format!("{}: {value}", crate::tl!("Not a number")))?;
            ratings.push(json!({ "name": name.trim(), "value": v, "unit": unit.trim() }));
        }
        let mut desc: Vec<&str> = self.desc.iter().map(String::as_str).collect();
        while desc.last().is_some_and(|d| d.is_empty()) {
            desc.pop();
        }
        Ok(json!({
            "tag": self.tag.trim(),
            "manufacturer": self.manufacturer.trim(),
            "catalog": self.catalog.trim(),
            "description": desc,
            "location": self.location.trim(),
            "installation": self.installation.trim(),
            "ratings": ratings,
        }))
    }
}

#[derive(Clone, Default)]
struct State {
    devices: Vec<Value>,
    filter: String,
    edit: Option<Edit>,
    /// What the selected device is wired to: (its pin, other tag:pin).
    links: Vec<(String, String)>,
    /// When the list was last read (egui time), and the drawing selection then.
    read_at: Option<f64>,
    selection: Vec<Handle>,
    message: Option<String>,
}

fn id() -> egui::Id {
    egui::Id::new("cadcraft-electrical-devices")
}

/// Run an engine command quietly (no command-line echo): the palette shows its own messages.
fn quiet(app: &mut CadApp, cmd: &str, params: Value) -> Result<Value, String> {
    app.session.execute(cmd, &params).map_err(|e| e.to_string())
}

impl State {
    fn read(&mut self, app: &mut CadApp) {
        self.devices = quiet(app, "edb.query", json!({ "what": "devices" })).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
        self.devices.sort_by(|a, b| a["tag"].as_str().cmp(&b["tag"].as_str()));
        if let Some(e) = &self.edit
            && let Some(d) = self.devices.iter().find(|d| d["id"].as_str() == Some(e.id.as_str()))
        {
            // Keep what is being typed; refresh what is shown about it.
            let fresh = Edit::from_device(d);
            if let Some(e) = self.edit.as_mut() {
                e.block = fresh.block;
                e.reference = fresh.reference;
            }
        }
    }

    fn select(&mut self, app: &mut CadApp, device: &Value) {
        let e = Edit::from_device(device);
        self.links = quiet(app, "edb.query", json!({ "what": "neighbours", "device": e.id }))
            .ok()
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
            .iter()
            .map(|l| {
                let other = l["to"]["device"].as_str().unwrap_or("");
                let tag = self.devices.iter().find(|d| d["id"].as_str() == Some(other)).and_then(|d| d["tag"].as_str()).unwrap_or("?");
                (l["pin"].as_str().unwrap_or("").to_string(), format!("{tag}:{}", l["to"]["pin"].as_str().unwrap_or("")))
            })
            .collect();
        self.edit = Some(e);
        self.message = None;
    }

    /// Follow a single symbol selected in the drawing.
    fn follow_selection(&mut self, app: &mut CadApp) {
        let sel = app.session.selection();
        if sel == self.selection {
            return;
        }
        self.selection = sel.clone();
        if let [h] = sel.as_slice()
            && let Ok(d) = quiet(app, "edb.device", json!({ "handle": h.hex() }))
        {
            self.select(app, &d);
        }
    }

    /// Select the symbol of the device in the drawing, when it is in the active one.
    fn show_in_drawing(&mut self, app: &mut CadApp, device: &Value) {
        let Some(h) = device["graphic"]["handle"].as_u64().map(Handle) else { return };
        let id = device["id"].as_str().unwrap_or("");
        let here = app.session.doc().ok().and_then(|d| d.entity(h)).is_some_and(
            |e| matches!(&e.kind, EntityKind::Insert(i) if i.attribs.iter().any(|a| a.tag.eq_ignore_ascii_case("EID") && a.text.value == id)),
        );
        if here {
            app.session.set_selection(vec![h]);
            self.selection = vec![h];
        }
    }

    fn apply(&mut self, app: &mut CadApp) {
        let Some(e) = self.edit.clone() else { return };
        match e.to_set().and_then(|set| app.run("edb.device", json!({ "id": e.id, "set": set }))) {
            Ok(d) => {
                self.message = Some(crate::tl!("Saved to the database and the drawing.").to_string());
                self.read(app);
                self.select(app, &d);
            }
            Err(err) => self.message = Some(err),
        }
    }
}

/// Open the palette, floating by the drawing's right edge the first time.
pub fn open(app: &mut CadApp) -> Result<Value, String> {
    let panel = crate::docking::Panel::Devices;
    let first = !app.ui.docking.layout.contains(&panel) && !app.ui.docking.hidden.iter().any(|(p, _)| *p == panel);
    let r = crate::docking::command(app, false, &json!({ "operation": "open", "panel": panel.id() }))?;
    if !first {
        return Ok(r);
    }
    let area = app.canvas.rect.unwrap_or(egui::Rect::from_min_size(egui::pos2(220.0, 80.0), vec2(1000.0, 800.0)));
    let (w, h) = (340.0, (area.height() - 40.0).clamp(240.0, 720.0));
    let rect = [area.right() - w - 16.0, area.top() + 24.0, w, h];
    crate::docking::command(app, false, &json!({ "operation": "float", "panel": panel.id(), "rect": rect }))
}

fn field(ui: &mut egui::Ui, label: &str, value: &mut String) {
    ui.label(RichText::new(label).font(crate::theme::small()));
    ui.add(egui::TextEdit::singleline(value).desired_width(f32::INFINITY));
    ui.end_row();
}

pub fn show(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let mut st: State = ui.ctx().data_mut(|d| d.get_temp(id())).unwrap_or_default();
    let now = ui.input(|i| i.time);
    if st.read_at.is_none_or(|at| now - at > REFRESH_SECONDS) {
        st.read(app);
        st.read_at = Some(now);
    }
    st.follow_selection(app);
    ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(REFRESH_SECONDS));

    egui::ScrollArea::vertical().id_salt("dock-devices").auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if ui.button(crate::tl!("Update from Drawing")).clicked() {
                if let Err(e) = app.run("edb.sync", json!({})) {
                    st.message = Some(e);
                }
                st.read(app);
            }
            if ui.button(crate::tl!("Update Drawing")).clicked()
                && let Err(e) = app.run("edb.push", json!({}))
            {
                st.message = Some(e);
            }
        });
        ui.add(egui::TextEdit::singleline(&mut st.filter).hint_text(crate::tl!("Filter")).desired_width(f32::INFINITY));
        let filter = st.filter.trim().to_ascii_lowercase();
        let shown: Vec<Value> = st
            .devices
            .iter()
            .filter(|d| {
                filter.is_empty()
                    || [&d["tag"], &d["graphic"]["block"], &d["part"]["catalog"], &d["part"]["manufacturer"]]
                        .iter()
                        .any(|v| v.as_str().is_some_and(|s| s.to_ascii_lowercase().contains(&filter)))
            })
            .cloned()
            .collect();
        if st.devices.is_empty() {
            ui.label(RichText::new(crate::tl!("No devices yet: place symbols, or run Update from Drawing.")).color(t.text_dim));
        }
        let mut picked: Option<Value> = None;
        egui::ScrollArea::vertical().id_salt("dock-devices-list").max_height(200.0).show(ui, |ui| {
            for d in &shown {
                let tag = d["tag"].as_str().filter(|s| !s.is_empty()).unwrap_or("—");
                let block = d["graphic"]["block"].as_str().unwrap_or("");
                let cat = d["part"]["catalog"].as_str().unwrap_or("");
                let on = st.edit.as_ref().is_some_and(|e| d["id"].as_str() == Some(e.id.as_str()));
                let text = RichText::new(format!("{tag}   {block}   {cat}")).font(crate::theme::mono());
                if ui.selectable_label(on, text).clicked() {
                    picked = Some(d.clone());
                }
            }
        });
        if let Some(d) = picked {
            st.select(app, &d);
            st.show_in_drawing(app, &d);
        }
        ui.separator();
        let Some(mut e) = st.edit.clone() else {
            ui.label(RichText::new(crate::tl!("Select a device in the list or a symbol in the drawing.")).color(t.text_dim));
            return;
        };
        let kind = if e.child { crate::tl!("Contact") } else { crate::tl!("Device") };
        ui.label(RichText::new(format!("{kind} · {} · {}", e.block, e.reference)).color(t.text_dim).font(crate::theme::small()));
        egui::Grid::new("device-fields").num_columns(2).spacing(vec2(8.0, 4.0)).show(ui, |ui| {
            field(ui, crate::tl!("Tag"), &mut e.tag);
            field(ui, crate::tl!("Manufacturer"), &mut e.manufacturer);
            field(ui, crate::tl!("Catalogue number"), &mut e.catalog);
            for (i, d) in e.desc.iter_mut().enumerate() {
                field(ui, &format!("{} {}", crate::tl!("Description"), i + 1), d);
            }
            field(ui, crate::tl!("Location"), &mut e.location);
            field(ui, crate::tl!("Installation"), &mut e.installation);
        });
        ui.label(RichText::new(crate::tl!("Ratings")).strong());
        let mut remove = None;
        for (i, (name, value, unit)) in e.ratings.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(name).hint_text(crate::tl!("Name")).desired_width(110.0));
                ui.add(egui::TextEdit::singleline(value).hint_text(crate::tl!("Value")).desired_width(60.0));
                ui.add(egui::TextEdit::singleline(unit).hint_text(crate::tl!("Unit")).desired_width(50.0));
                if ui.small_button("×").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove
            && i < e.ratings.len()
        {
            e.ratings.remove(i);
        }
        if ui.small_button(crate::tl!("Add rating")).clicked() {
            e.ratings.push(Default::default());
        }
        st.edit = Some(e);
        ui.add_space(4.0);
        if ui.button(crate::tl!("Apply")).clicked() {
            st.apply(app);
        }
        if let Some(m) = &st.message {
            ui.label(RichText::new(m).color(t.text_dim));
        }
        if !st.links.is_empty() {
            ui.separator();
            ui.label(RichText::new(crate::tl!("Wired to")).strong());
            for (pin, other) in &st.links {
                ui.label(RichText::new(format!("{pin} → {other}")).font(crate::theme::mono()));
            }
        }
    });
    ui.ctx().data_mut(|d| d.insert_temp(id(), st));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_json_round_trips_through_the_editor() {
        let d = json!({
            "id": "D-1", "tag": "-K105", "child": false, "location": "+CAB1", "installation": "=A1",
            "part": { "manufacturer": "Acme", "catalog": "R-24", "description": ["Pump", "relay"],
                      "ratings": [{ "name": "coilVoltage", "value": 24.0, "unit": "V DC" }] },
            "graphic": { "block": "IEC_COIL", "reference": [1, 5] },
        });
        let e = Edit::from_device(&d);
        assert_eq!((e.reference.as_str(), e.desc[1].as_str(), e.ratings.len()), ("105", "relay", 1));
        let set = e.to_set().unwrap();
        assert_eq!(set["description"], json!(["Pump", "relay"]));
        assert_eq!(set["ratings"][0]["value"], 24.0);
        let mut bad = e.clone();
        bad.ratings.push(("current".into(), "ten".into(), "A".into()));
        assert!(bad.to_set().is_err());
        bad.ratings.last_mut().unwrap().1 = "2,5".into();
        assert_eq!(bad.to_set().unwrap()["ratings"][1]["value"], 2.5);
    }
}
