//! Cross-references between a main device (a relay or contactor coil) and its contacts.
//!
//! Contacts are drawn apart from their coil and carry the coil's tag (see
//! [`super::is_child`]). ELINK gives picked contacts the tag of a picked coil. EXREF writes, on
//! the `EXREF` layer, under each coil the contacts it drives (`NO 1.5`: a normally open contact in
//! column 5 of sheet 1) and under each contact where its coil is (`/1.2`). The texts are rebuilt
//! from scratch on every run.

use std::collections::BTreeMap;

use cadcraft_doc::{EntityKind, HAlign, Handle, Text, VAlign, entity_bounds};
use cadcraft_geom::{Vec2, Vec3};
use serde_json::{Value, json};

use super::{Grid, attribute_param, class_overrides, devices, handle_list, is_child, is_numbered, kind_of, set_tag};
use crate::cmd::machines::{SelOutcome, SelectPhase};
use crate::cmd::{CommandSpec, bad, f64_or, str_param};
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

const XREF_LAYER: &str = "EXREF";
/// Most cross-reference lines under one coil.
const MAX_LINES: usize = 64;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("elink", "Link Contacts to Device", run_elink)
            .menu(&["Tools", "Electrical", "Link Contacts"])
            .params("{main: handle (the coil), handles: [handle] (its contacts), attribute? (default: TAG1, TAG2 or TAG)} → {tag, linked}")
            .interactive(|_| Ok(Box::new(ElinkM::default()))),
        CommandSpec::new("exref", "Cross-References", run_exref).menu(&["Tools", "Electrical", "Cross-References"]).params(
            "{attribute? (default: TAG1, TAG2 or TAG), layer? (default \"EXREF\"), height? (default 2.5), origin?, sheetWidth?, columns?, firstSheet?} \
                 → {devices: [{tag, handle, sheet, column, contacts: [{handle, kind, sheet, column}]}], orphans: [handle]}",
        ),
    ]
}

fn run_elink(s: &mut Session, p: &Value) -> Result<Value> {
    let r = run_elink_inner(s, p)?;
    // Keep the project database in step with the drawing.
    super::sync::auto(s);
    Ok(r)
}

fn run_elink_inner(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "elink";
    let attribute = attribute_param(p);
    let main = p
        .get("main")
        .and_then(|v| v.as_str().and_then(Handle::parse_hex).or_else(|| v.as_u64().map(Handle)))
        .ok_or_else(|| bad(CMD, "`main` (the handle of the coil) is required"))?;
    let contacts = handle_list(CMD, p)?.ok_or_else(|| bad(CMD, "`handles` (the contacts) is required"))?;
    let all = devices(s, attribute.as_deref(), None)?;
    let main_dev =
        all.iter().find(|d| d.handle == main).ok_or_else(|| EngineError::Other(format!("{} is not a device with a tag attribute.", main.hex())))?;
    if !is_numbered(&main_dev.tag) {
        return Err(EngineError::Other(format!("The main device has no tag yet ({:?}); run Tag Devices first.", main_dev.tag)));
    }
    let tag = main_dev.tag.clone();
    let mut linked = 0usize;
    for h in contacts.iter().filter(|h| **h != main) {
        if let Some(d) = all.iter().find(|d| d.handle == *h) {
            set_tag(s, *h, &d.attr, &tag)?;
            linked += 1;
        }
    }
    s.log.push(format!("{linked} contact(s) linked to {tag}."));
    Ok(json!({ "tag": tag, "linked": linked }))
}

/// A text for cross-reference `value`, its top centred at `at`.
fn xref_text(value: &str, at: Vec2, height: f64) -> Text {
    let p = Vec3::new(at.x, at.y, 0.0);
    Text {
        insert: p,
        align_pt: Some(p),
        height,
        value: value.to_string(),
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: HAlign::Center,
        valign: VAlign::Top,
    }
}

/// A device as cross-referencing sees it, in drawing `doc` of a set (a project, or just one).
#[derive(Clone, Debug)]
pub(super) struct RefDev {
    pub doc: usize,
    pub handle: Handle,
    pub tag: String,
    pub child: bool,
    pub kind: &'static str,
    /// Sheet and column.
    pub place: Option<(u64, u64)>,
    pub y: f64,
}

/// The devices of the current drawing, numbered `doc`.
pub(super) fn ref_devices(
    s: &Session,
    attribute: Option<&str>,
    grid: &Grid,
    overrides: &std::collections::HashMap<String, String>,
    doc: usize,
) -> Result<Vec<RefDev>> {
    Ok(devices(s, attribute, None)?
        .iter()
        .map(|d| RefDev {
            doc,
            handle: d.handle,
            tag: d.tag.clone(),
            child: is_child(d, overrides),
            kind: kind_of(d),
            place: grid.locate(d.x),
            y: d.y,
        })
        .collect())
}

/// What cross-referencing works out for a set of devices.
pub(super) struct Refs {
    /// The reference lines of each device (by drawing and handle) that has any.
    pub lines: BTreeMap<(usize, Handle), Vec<String>>,
    pub report: Vec<Value>,
    /// Contacts without a single main device of their tag.
    pub orphans: Vec<(usize, Handle)>,
}

fn place_text(p: (u64, u64)) -> String {
    format!("{}{:02}", p.0, p.1)
}

/// Under each main device the contacts it drives (`NO 105`), under each contact where its main
/// device is (`/102`).
pub(super) fn compute(devs: &[RefDev]) -> Refs {
    // Mains with a numbered tag, by tag (a duplicated tag has no single main: its contacts are
    // orphans until the duplicate is resolved).
    let mut mains: BTreeMap<&str, Vec<&RefDev>> = BTreeMap::new();
    for d in devs.iter().filter(|d| !d.child && is_numbered(&d.tag)) {
        mains.entry(d.tag.as_str()).or_default().push(d);
    }
    let mut lines: BTreeMap<(usize, Handle), Vec<String>> = BTreeMap::new();
    let mut report = Vec::new();
    for (tag, ms) in &mains {
        let [main] = ms.as_slice() else { continue };
        let mut contacts: Vec<&RefDev> = devs.iter().filter(|d| d.child && d.tag == *tag).collect();
        contacts.sort_by(|a, b| a.place.cmp(&b.place).then(b.y.total_cmp(&a.y)));
        let ls: Vec<String> = contacts.iter().filter_map(|c| c.place.map(|p| format!("{} {}", c.kind, place_text(p)))).take(MAX_LINES).collect();
        if !ls.is_empty() {
            lines.insert((main.doc, main.handle), ls);
        }
        if let Some(p) = main.place {
            for c in &contacts {
                lines.insert((c.doc, c.handle), vec![format!("/{}", place_text(p))]);
            }
        }
        report.push(json!({
            "tag": tag,
            "handle": main.handle.hex(),
            "drawing": main.doc,
            "sheet": main.place.map(|a| a.0),
            "column": main.place.map(|a| a.1),
            "contacts": contacts.iter().map(|c| json!({
                "handle": c.handle.hex(), "drawing": c.doc, "kind": c.kind, "sheet": c.place.map(|a| a.0), "column": c.place.map(|a| a.1),
            })).collect::<Vec<_>>(),
        }));
    }
    let orphans = devs.iter().filter(|d| d.child && mains.get(d.tag.as_str()).is_none_or(|m| m.len() != 1)).map(|d| (d.doc, d.handle)).collect();
    Refs { lines, report, orphans }
}

/// Write the references of drawing `doc` into the current drawing: into each symbol's XREF
/// attribute when it has one, else as texts on `layer` under the symbol. Old references (XREF
/// values, the layer's texts) are cleared first. Returns (texts written, attributes filled).
pub(super) fn write_refs(s: &mut Session, doc: usize, devs: &[RefDev], refs: &Refs, layer: &str, height: f64) -> Result<(usize, usize)> {
    let has_xref = |s: &Session, h: Handle| matches!(s.doc().ok().and_then(|d| d.entity(h)).map(|e| &e.kind), Some(EntityKind::Insert(i)) if i.attribs.iter().any(|a| a.tag.eq_ignore_ascii_case("XREF")));
    let mut in_attributes = 0usize;
    let mut texts: Vec<(Handle, Vec<String>)> = Vec::new();
    for d in devs.iter().filter(|d| d.doc == doc) {
        let lines = refs.lines.get(&(doc, d.handle));
        if has_xref(s, d.handle) {
            let value = lines.map(|l| l.join("  ")).unwrap_or_default();
            in_attributes += usize::from(!value.is_empty());
            set_tag(s, d.handle, "XREF", &value)?;
        } else if let Some(l) = lines {
            texts.push((d.handle, l.clone()));
        }
    }
    let space = s.space();
    let old: Vec<Handle> = s
        .doc()?
        .space(&space)
        .map(|st| {
            st.iter().filter(|e| e.common.layer.eq_ignore_ascii_case(layer) && matches!(e.kind, EntityKind::Text(_))).map(|e| e.handle).collect()
        })
        .unwrap_or_default();
    let anchors: Vec<(Vec2, Vec<String>)> = {
        let d = s.doc()?;
        texts
            .into_iter()
            .filter_map(|(h, lines)| {
                let b = entity_bounds(d, d.entity(h)?, 0);
                if b.is_empty() || !b.min.is_finite() || !b.max.is_finite() {
                    return None;
                }
                Some((Vec2::new(b.center().x, b.min.y - height * 0.6), lines))
            })
            .collect()
    };
    let mut common = s.current_common();
    common.layer = layer.to_string();
    {
        let d = s.doc_mut()?;
        for h in old {
            d.remove_entity(h);
        }
    }
    let mut written = 0usize;
    for (at, lines) in anchors {
        for (i, line) in lines.iter().enumerate() {
            let y = at.y - height * 1.5 * i as f64;
            s.doc_mut()?.add(&space, common.clone(), EntityKind::Text(xref_text(line, Vec2::new(at.x, y), height)))?;
            written += 1;
        }
    }
    Ok((written, in_attributes))
}

/// The `layer` and `height` parameters of EXREF.
pub(super) fn layer_and_height(cmd: &str, p: &Value) -> Result<(String, f64)> {
    let layer = str_param(p, "layer").map(str::trim).filter(|l| !l.is_empty()).unwrap_or(XREF_LAYER).to_string();
    if layer.chars().count() > 255 {
        return Err(bad(cmd, "`layer` is too long"));
    }
    let height = f64_or(p, "height", 2.5);
    if !(height > 0.0 && height < 1e6) {
        return Err(bad(cmd, "`height` must be positive"));
    }
    Ok((layer, height))
}

fn run_exref(s: &mut Session, p: &Value) -> Result<Value> {
    let r = run_exref_inner(s, p)?;
    // Keep the project database in step with the drawing.
    super::sync::auto(s);
    Ok(r)
}

fn run_exref_inner(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "exref";
    let attribute = attribute_param(p);
    let (layer, height) = layer_and_height(CMD, p)?;
    let grid = Grid::from_params(CMD, p)?;
    let overrides = class_overrides(CMD, p)?;
    let devs = ref_devices(s, attribute.as_deref(), &grid, &overrides, 0)?;
    let refs = compute(&devs);
    let (written, in_attributes) = write_refs(s, 0, &devs, &refs, &layer, height)?;
    let orphans: Vec<String> = refs.orphans.iter().map(|(_, h)| h.hex()).collect();
    s.log.push(format!(
        "{} device(s) cross-referenced, {written} text(s), {in_attributes} attribute(s){}.",
        refs.report.len(),
        if orphans.is_empty() { String::new() } else { format!(", {} contact(s) without a single main device", orphans.len()) }
    ));
    Ok(json!({ "devices": refs.report, "orphans": orphans, "written": written, "inAttributes": in_attributes }))
}

/// ELINK at the command line: pick the coil, then its contacts.
#[derive(Default)]
struct ElinkM {
    main: Option<Handle>,
    sel: SelectPhase,
}

impl Interactive for ElinkM {
    fn name(&self) -> &'static str {
        "ELINK"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.main.is_none() { Prompt::new("Select main device (coil)", Accept::SELECT) } else { Prompt::new("Select contacts", Accept::SELECT) }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.main.is_none() && !self.sel.single {
            self.sel = SelectPhase::single();
        }
        match self.sel.feed(s, &i)? {
            SelOutcome::More => Ok(Step::Continue),
            SelOutcome::Empty => Ok(Step::Done),
            SelOutcome::Done(hs) => match self.main {
                None => {
                    self.main = hs.first().copied();
                    self.sel = SelectPhase::default();
                    Ok(if self.main.is_some() { Step::Continue } else { Step::Done })
                }
                Some(m) => {
                    let handles: Vec<String> = hs.iter().map(|h| h.hex()).collect();
                    run_elink(s, &json!({ "main": m.hex(), "handles": handles }))?;
                    Ok(Step::Done)
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn session() -> Session {
        let mut s = Session::new();
        for name in ["RELAY_COIL", "CONTACT_NO", "CONTACT_NC", "MOTOR"] {
            s.execute("rectang", &json!({"p1": [-5, -5], "p2": [5, 5]})).unwrap();
            let r = s.doc().unwrap().model.last().unwrap().handle;
            s.execute("attdef", &json!({"tag": "TAG", "at": [7, 0], "default": "", "height": 2.5})).unwrap();
            let a = s.doc().unwrap().model.last().unwrap().handle;
            s.execute("block", &json!({"name": name, "base": [0, 0], "handles": [r.hex(), a.hex()], "keep": "delete"})).unwrap();
        }
        s
    }

    fn put(s: &mut Session, block: &str, at: [f64; 2], tag: &str) -> String {
        s.execute("insert", &json!({"name": block, "at": at, "attribs": {"TAG": tag}})).unwrap();
        s.doc().unwrap().model.last().unwrap().handle.hex()
    }

    fn tag_of(s: &Session, h: &str) -> String {
        let h = Handle::parse_hex(h).unwrap();
        match &s.doc().unwrap().entity(h).unwrap().kind {
            EntityKind::Insert(i) => i.attribs[0].text.value.clone(),
            _ => panic!("not an insert"),
        }
    }

    fn xref_texts(s: &Session) -> Vec<String> {
        let mut v: Vec<String> = s
            .doc()
            .unwrap()
            .model
            .iter()
            .filter(|e| e.common.layer == "EXREF")
            .filter_map(|e| if let EntityKind::Text(t) = &e.kind { Some(t.value.clone()) } else { None })
            .collect();
        v.sort();
        v
    }

    #[test]
    fn contacts_are_not_tagged_and_follow_their_coil() {
        let mut s = session();
        let k = put(&mut s, "RELAY_COIL", [63.0, 100.0], "");
        let c = put(&mut s, "CONTACT_NO", [200.0, 100.0], "");
        let r = s.execute("etag", &json!({})).unwrap();
        assert_eq!(r["auxiliary"], 1);
        assert_eq!(tag_of(&s, &k), "-K102");
        assert_eq!(tag_of(&s, &c), "", "a contact gets no tag of its own");
        s.execute("elink", &json!({"main": k, "handles": [c]})).unwrap();
        assert_eq!(tag_of(&s, &c), "-K102");
        // A new coil above it in the same column takes -K1.2 when renumbering everything; the
        // contact follows its own coil to the new tag.
        let k2 = put(&mut s, "RELAY_COIL", [50.0, 150.0], "");
        let r = s.execute("etag", &json!({"mode": "all"})).unwrap();
        assert_eq!(tag_of(&s, &k2), "-K102");
        assert_eq!(tag_of(&s, &k), "-K102A");
        assert_eq!(tag_of(&s, &c), "-K102A");
        assert_eq!(r["contactsFollowed"], 1);
        // Shared tags of contacts are not duplicates.
        let r = s.execute("edevices", &json!({})).unwrap();
        assert_eq!(r["duplicates"], json!({}));
    }

    #[test]
    fn cross_references_both_ways() {
        let mut s = session();
        put(&mut s, "RELAY_COIL", [63.0, 100.0], "-K1.2");
        put(&mut s, "CONTACT_NO", [200.0, 150.0], "-K1.2");
        put(&mut s, "CONTACT_NC", [500.0, 150.0], "-K1.2");
        let orphan = put(&mut s, "CONTACT_NO", [300.0, 50.0], "-K9.9");
        put(&mut s, "MOTOR", [100.0, 20.0], "-M1.3");
        let r = s.execute("exref", &json!({})).unwrap();
        assert_eq!(xref_texts(&s), vec!["/102", "/102", "NC 202", "NO 105"]);
        assert_eq!(r["orphans"], json!([orphan]));
        // Running again rebuilds rather than piling up.
        s.execute("exref", &json!({})).unwrap();
        assert_eq!(xref_texts(&s).len(), 4);
        s.execute("undo", &json!({})).unwrap();
        assert_eq!(xref_texts(&s).len(), 4);
    }

    #[test]
    fn library_symbols_show_references_in_their_xref_attribute() {
        let mut s = Session::new();
        let k = s.execute("esymbol", &json!({"name": "IEC_COIL", "at": [63, 100]})).unwrap();
        let c = s.execute("esymbol", &json!({"name": "IEC_NO", "at": [200, 150]})).unwrap();
        let (k, c) = (k["handle"].as_str().unwrap().to_string(), c["handle"].as_str().unwrap().to_string());
        s.execute("elink", &json!({"main": k, "handles": [c]})).unwrap();
        let r = s.execute("exref", &json!({})).unwrap();
        assert_eq!(r["inAttributes"], 2);
        assert!(xref_texts(&s).is_empty());
        let xref = |h: &str| match &s.doc().unwrap().entity(Handle::parse_hex(h).unwrap()).unwrap().kind {
            EntityKind::Insert(i) => i.attribs.iter().find(|a| a.tag == "XREF").unwrap().text.value.clone(),
            _ => panic!(),
        };
        assert_eq!(xref(&k), "NO 105");
        assert_eq!(xref(&c), "/102");
    }

    #[test]
    fn elink_refuses_untagged_main_and_bad_params() {
        let mut s = session();
        let k = put(&mut s, "RELAY_COIL", [63.0, 100.0], "");
        let c = put(&mut s, "CONTACT_NO", [200.0, 100.0], "");
        assert!(s.execute("elink", &json!({"main": k, "handles": [c]})).is_err());
        assert!(s.execute("elink", &json!({"handles": [c]})).is_err());
        assert!(s.execute("elink", &json!({"main": "zz", "handles": [c]})).is_err());
        assert!(s.execute("elink", &json!({"main": k})).is_err());
        assert!(s.execute("exref", &json!({"height": 0})).is_err());
        assert!(s.execute("exref", &json!({"layer": "x".repeat(300)})).is_err());
    }

    #[test]
    fn elink_command_line() {
        let mut s = session();
        let k = put(&mut s, "RELAY_COIL", [63.0, 100.0], "-K1.2");
        let c = put(&mut s, "CONTACT_NO", [200.0, 100.0], "");
        s.cmdline("elink").unwrap();
        s.input(Input::Pick(vec![Handle::parse_hex(&k).unwrap()])).unwrap();
        s.input(Input::Pick(vec![Handle::parse_hex(&c).unwrap()])).unwrap();
        s.input(Input::Enter).unwrap();
        assert!(s.running.is_none());
        assert_eq!(tag_of(&s, &c), "-K1.2");
    }
}
