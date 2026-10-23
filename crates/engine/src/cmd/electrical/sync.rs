//! The drawing and the project database, kept in step both ways.
//!
//! - Drawing → database (EDB.SYNC, EPROJECT.SYNC): every symbol (a block reference with `TAG1`
//!   or `TAG2`) becomes or updates an [`EcadDevice`]: tag, class, family, part data from its
//!   attributes, pins from its connection points, and a [`GraphicRef`] to the block reference.
//!   Each symbol carries its device's id in the hidden `EID` attribute, given on first sync, so
//!   the link survives saving, moving and renumbering; a copied symbol (a repeated `EID`) gets a
//!   new device. Wire networks become [`EcadConnection`]s between the pins on them. Devices whose
//!   symbols were erased leave the database with their connections.
//! - Database → drawing (EDB.DEVICE with `set`, EDB.PUSH): part data, tag, location and
//!   installation edited in the database are written into the symbol's attributes (`MFG`, `CAT`,
//!   `DESC1`–`DESC3`, `LOC`, `INST`, `TAG1`/`TAG2`), on whichever sheet it is. Data the symbol
//!   has no attribute for (technical ratings, conductor data) lives in the database only.
//!
//! Every electrical command (tagging, numbering, placing symbols, drawing wires…) syncs its
//! drawing when it finishes; EDB.SYNC does it on demand, after ordinary edits.
//!
//! In a project the database is `<project>.ecdb` (redb) next to the project file; a drawing
//! outside a project uses one kept in memory for the session.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::sync::OnceLock;

use cadcraft_doc::{Attrib, AttribModes, EntityKind, HAlign, Handle, Insert, Text, VAlign};
use cadcraft_ecad::{
    ConnectionId, DeviceFamily, DeviceId, EcadConnection, EcadDb, EcadDevice, EcadPage, EcadPin, GraphicRef, PageId, PinDirection, PinRef, Rating,
};
use cadcraft_geom::Vec2;
use serde_json::{Value, json};

use super::project::{Project, current, drawing_path, read_project, same_file, sheet_params};
use super::symbols::connection_dir;
use super::wires::{LABEL_LAYER, WIRE_LAYERS, labels, potentials, wire_segments};
use super::{Grid, class_of, devices, is_child, set_tag};
use crate::cmd::{CommandSpec, bad, str_param};
use crate::{EngineError, Result, Session};

/// Distance within which a pin counts as on a wire.
const PIN_ON_WIRE: f64 = 0.01;

fn db_err(e: cadcraft_ecad::EcadError) -> EngineError {
    EngineError::Other(e.to_string())
}

/// The database for a project file, or the session's in-memory one.
enum Db {
    File(EcadDb),
    Memory(&'static EcadDb),
}

impl std::ops::Deref for Db {
    type Target = EcadDb;
    fn deref(&self) -> &EcadDb {
        match self {
            Db::File(d) => d,
            Db::Memory(d) => d,
        }
    }
}

fn memory_db() -> &'static EcadDb {
    static DB: OnceLock<EcadDb> = OnceLock::new();
    DB.get_or_init(EcadDb::memory)
}

/// The database file of the project at `project`.
fn db_path(project: &str) -> std::path::PathBuf {
    Path::new(project).with_extension("ecdb")
}

fn open_db(project: Option<&str>) -> Result<Db> {
    match project {
        #[cfg(not(target_arch = "wasm32"))]
        Some(p) => Ok(Db::File(EcadDb::open(&db_path(p)).map_err(db_err)?)),
        _ => Ok(Db::Memory(memory_db())),
    }
}

/// Where the active drawing sits: its project (if it is in the current one), page and sheet.
struct Place {
    project: Option<(String, Project)>,
    page: EcadPage,
}

fn page_id(drawing: &str) -> PageId {
    PageId(format!("G-{}", drawing.replace('\\', "/").to_ascii_lowercase()))
}

fn place_of_active(s: &Session) -> Result<Place> {
    let st = s.state()?;
    let path = st.path.clone();
    if let (Some(path), Some(proj)) = (path.as_deref(), current())
        && let Ok(pr) = read_project(&proj)
        && let Some(i) = pr.drawings.iter().position(|d| same_file(&drawing_path(&proj, d), Path::new(path)))
    {
        let rel = pr.drawings.get(i).cloned().unwrap_or_default();
        let page = EcadPage {
            id: page_id(&rel),
            sheet: u32::try_from(i + 1).unwrap_or(u32::MAX),
            drawing: rel,
            title: st.title.clone(),
            description: String::new(),
        };
        return Ok(Place { project: Some((proj, pr)), page });
    }
    let name = path.unwrap_or_else(|| st.title.clone());
    Ok(Place { project: None, page: EcadPage { id: page_id(&name), sheet: 1, drawing: name, title: st.title.clone(), description: String::new() } })
}

fn grid_for(place: &Place) -> Result<Grid> {
    let params = match &place.project {
        Some((_, pr)) => sheet_params(pr, u64::from(place.page.sheet)),
        None => json!({}),
    };
    Grid::from_params("edb.sync", &params)
}

/// The value of attribute `tag` of `ins`, trimmed.
fn attr(ins: &Insert, tag: &str) -> Option<String> {
    ins.attribs.iter().find(|a| a.tag.eq_ignore_ascii_case(tag)).map(|a| a.text.value.trim().to_string())
}

/// The pins of a symbol: its `X?TERMnn` connection points, numbered by its `TERMnn` attributes.
pub(super) fn pins_of(ins: &Insert) -> Vec<EcadPin> {
    let numbers: HashMap<String, String> = ins
        .attribs
        .iter()
        .filter_map(|a| {
            let t = a.tag.to_ascii_uppercase();
            let nn = t.strip_prefix("TERM")?;
            Some((nn.to_string(), a.text.value.trim().to_string()))
        })
        .collect();
    let mut pins: Vec<(String, EcadPin)> = ins
        .attribs
        .iter()
        .filter_map(|a| {
            let dir = connection_dir(&a.tag)?;
            let nn = a.tag.get(6..)?.to_string();
            let p = a.text.justify_point();
            let number = numbers.get(&nn).filter(|n| !n.is_empty()).cloned().unwrap_or_else(|| nn.trim_start_matches('0').to_string());
            Some((nn, EcadPin { number, direction: PinDirection::from_digit(dir).unwrap_or_default(), position: [p.x, p.y] }))
        })
        .collect();
    pins.sort_by(|a, b| a.0.cmp(&b.0));
    pins.into_iter().map(|(_, p)| p).collect()
}

/// Set (or add, invisible) attribute `tag` of block reference `h` to `value`.
fn put_attr(s: &mut Session, h: Handle, tag: &str, value: &str) -> Result<()> {
    s.doc_mut()?.modify_entity(h, |e| {
        if let EntityKind::Insert(ins) = &mut e.kind {
            if let Some(a) = ins.attribs.iter_mut().find(|a| a.tag.eq_ignore_ascii_case(tag)) {
                a.text.value = value.to_string();
            } else {
                let p = ins.insert;
                ins.attribs.push(Attrib {
                    tag: tag.to_string(),
                    text: Text {
                        insert: p,
                        align_pt: None,
                        height: 1.0,
                        value: value.to_string(),
                        rotation: 0.0,
                        width_factor: 1.0,
                        oblique: 0.0,
                        style: "Standard".into(),
                        halign: HAlign::Left,
                        valign: VAlign::Baseline,
                    },
                    invisible: true,
                    constant: false,
                    prompt: String::new(),
                    props: Default::default(),
                    multiline: None,
                    modes: AttribModes::default(),
                });
            }
        }
    })?;
    Ok(())
}

/// Drawing → database for the active drawing. Returns the page and counts.
fn sync_active(s: &mut Session, db: &EcadDb, place: &Place) -> Result<Value> {
    let grid = grid_for(place)?;
    let overrides = HashMap::new();
    let page = &place.page;
    let devs = devices(s, None, None)?;
    let mut seen: HashSet<DeviceId> = HashSet::new();
    let mut records: Vec<EcadDevice> = Vec::with_capacity(devs.len());
    let mut new_ids = 0usize;
    for d in &devs {
        let Some(EntityKind::Insert(ins)) = s.doc()?.entity(d.handle).map(|e| e.kind.clone()) else { continue };
        // The device this symbol shows: its EID, unless that is a copy (seen already, or a device
        // drawn by another symbol that is still on its page elsewhere).
        let eid = attr(&ins, "EID").filter(|e| !e.is_empty()).map(DeviceId);
        let stored = match &eid {
            Some(id) if !seen.contains(id) => db.device(id).map_err(db_err)?,
            _ => None,
        };
        let copy_of_other =
            stored.as_ref().and_then(|o| o.graphic.as_ref()).is_some_and(|g| g.page != page.id && db.page(&g.page).ok().flatten().is_some());
        let id = match (&eid, copy_of_other) {
            (Some(id), false) if !seen.contains(id) => id.clone(),
            _ => {
                new_ids += 1;
                DeviceId::new(&format!("{}/{:X}", page.id, d.handle.0))
            }
        };
        if eid.as_ref() != Some(&id) {
            put_attr(s, d.handle, "EID", id.as_str())?;
        }
        seen.insert(id.clone());
        let old = if eid.as_ref() == Some(&id) { stored } else { None };
        let mut part = old.as_ref().map(|o| o.part.clone()).unwrap_or_default();
        // The symbol's attributes are the truth for what they hold.
        if let Some(v) = attr(&ins, "MFG") {
            part.manufacturer = v;
        }
        if let Some(v) = attr(&ins, "CAT") {
            part.catalog = v;
        }
        let desc: Vec<String> = ["DESC1", "DESC2", "DESC3"].iter().filter_map(|t| attr(&ins, t)).collect();
        if !desc.is_empty() {
            part.description = desc;
            while part.description.last().is_some_and(String::is_empty) {
                part.description.pop();
            }
        }
        let (sheet, column) = grid.locate(d.x).unwrap_or((u64::from(page.sheet), 0));
        records.push(EcadDevice {
            id,
            tag: d.tag.clone(),
            class: class_of(d, &overrides).unwrap_or_default(),
            family: DeviceFamily::parse(d.family.as_deref().unwrap_or("")),
            child: is_child(d, &overrides),
            parent: None,
            part,
            location: attr(&ins, "LOC").or_else(|| old.as_ref().map(|o| o.location.clone())).unwrap_or_default(),
            installation: attr(&ins, "INST").or_else(|| old.as_ref().map(|o| o.installation.clone())).unwrap_or_default(),
            pins: pins_of(&ins),
            graphic: Some(GraphicRef {
                page: page.id.clone(),
                handle: d.handle.0,
                block: d.block.clone(),
                position: [d.x, d.y],
                rotation: ins.rotation,
                reference: (u32::try_from(sheet).unwrap_or(u32::MAX), u32::try_from(column).unwrap_or(u32::MAX)),
            }),
            revision: old.as_ref().map(|o| o.revision).unwrap_or(0),
        });
    }
    // Children point at the main device of their tag (on any page) when there is exactly one.
    let mut mains: BTreeMap<String, Vec<DeviceId>> = BTreeMap::new();
    for d in db.devices().map_err(db_err)?.iter().filter(|d| !d.child && d.graphic.as_ref().is_some_and(|g| g.page != page.id)) {
        mains.entry(d.tag.clone()).or_default().push(d.id.clone());
    }
    for d in records.iter().filter(|d| !d.child) {
        mains.entry(d.tag.clone()).or_default().push(d.id.clone());
    }
    for d in records.iter_mut().filter(|d| d.child && super::is_numbered(&d.tag)) {
        d.parent = mains.get(&d.tag).and_then(|m| if let [one] = m.as_slice() { Some(one.clone()) } else { None });
    }

    // Wire networks → connections between the pins on them.
    let (wire_handles, segs) = wire_segments(s, WIRE_LAYERS)?;
    let groups = potentials(wire_handles.len(), &segs, 0.01);
    let numbers = labels(s, LABEL_LAYER)?;
    let mut connections: Vec<EcadConnection> = Vec::new();
    for (n, g) in groups.iter().enumerate() {
        let gsegs: Vec<_> = segs.iter().filter(|sg| g.contains(&sg.wire)).collect();
        let mut on: Vec<(Vec2, PinRef)> = Vec::new();
        for d in &records {
            for p in &d.pins {
                let at = Vec2::new(p.position[0], p.position[1]);
                if gsegs.iter().any(|sg| sg.dist_to(at) <= PIN_ON_WIRE) {
                    on.push((at, PinRef::new(&d.id, &p.number)));
                }
            }
        }
        if on.len() < 2 {
            continue;
        }
        // The network's number: the label nearest one of its wires.
        let number = numbers
            .iter()
            .filter(|(_, _, v)| !v.is_empty())
            .map(|(_, at, v)| (gsegs.iter().map(|sg| sg.dist_to(*at)).fold(f64::INFINITY, f64::min), v))
            .filter(|(dist, _)| *dist <= 6.0)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, v)| v.clone());
        let network = format!("{}#{}", page.id, number.clone().unwrap_or_else(|| format!("n{n}")));
        let wires: Vec<u64> = g.iter().filter_map(|w| wire_handles.get(*w)).map(|h| h.0).collect();
        // Pins in reading order, chained: the from–to list a wiring person follows.
        on.sort_by(|a, b| a.0.x.total_cmp(&b.0.x).then(b.0.y.total_cmp(&a.0.y)));
        for w in on.windows(2) {
            let [(_, a), (_, b)] = w else { continue };
            connections.push(EcadConnection {
                id: ConnectionId(format!("C-{}|{}:{}|{}:{}", page.id, a.device, a.pin, b.device, b.pin)),
                from: a.clone(),
                to: b.clone(),
                wire_number: number.clone(),
                network: Some(network.clone()),
                page: page.id.clone(),
                wires: wires.clone(),
                ..Default::default()
            });
        }
    }
    // Conductor data set in the database survives re-syncing.
    for c in &mut connections {
        if let Ok(Some(old)) = db.connection(&c.id) {
            c.color = old.color;
            c.cross_section = old.cross_section;
            c.wire_type = old.wire_type;
        }
    }
    let removed = db.devices_on_page(&page.id).map_err(db_err)?.iter().filter(|o| !seen.contains(&o.id)).count();
    db.replace_page(page, &records, &connections).map_err(db_err)?;
    Ok(json!({
        "page": page.id.as_str(),
        "sheet": page.sheet,
        "devices": records.len(),
        "newIds": new_ids,
        "removed": removed,
        "connections": connections.len(),
    }))
}

/// Database → drawing for the active drawing: every device of `page` with a symbol here gets
/// its tag and part data written into the symbol's attributes. Returns how many symbols changed.
fn push_active(s: &mut Session, db: &EcadDb, page: &PageId) -> Result<usize> {
    let by_eid: HashMap<String, Handle> = devices(s, None, None)?
        .iter()
        .filter_map(|d| match s.doc().ok()?.entity(d.handle).map(|e| &e.kind) {
            Some(EntityKind::Insert(ins)) => attr(ins, "EID").filter(|e| !e.is_empty()).map(|e| (e, d.handle)),
            _ => None,
        })
        .collect();
    let mut changed = 0usize;
    for dev in db.devices_on_page(page).map_err(db_err)? {
        let Some(h) = by_eid.get(dev.id.as_str()).copied().or_else(|| dev.graphic.as_ref().map(|g| Handle(g.handle))) else { continue };
        let Some(EntityKind::Insert(ins)) = s.doc()?.entity(h).map(|e| e.kind.clone()) else { continue };
        if attr(&ins, "EID").as_deref() != Some(dev.id.as_str()) {
            continue;
        }
        let mut want: Vec<(&str, String)> = vec![
            ("MFG", dev.part.manufacturer.clone()),
            ("CAT", dev.part.catalog.clone()),
            ("LOC", dev.location.clone()),
            ("INST", dev.installation.clone()),
            (if dev.child { "TAG2" } else { "TAG1" }, dev.tag.clone()),
        ];
        for (i, t) in ["DESC1", "DESC2", "DESC3"].iter().enumerate() {
            want.push((t, dev.part.description.get(i).cloned().unwrap_or_default()));
        }
        let mut any = false;
        for (tag, value) in want {
            // Only attributes the symbol has (a block without MFG keeps it in the database).
            if attr(&ins, tag).is_some_and(|v| v != value) {
                set_tag(s, h, tag, &value)?;
                any = true;
            }
        }
        changed += usize::from(any);
    }
    Ok(changed)
}

/// Update the database from the active drawing after an electrical command. A failure (no
/// database, a locked file) never fails the command: the next sync catches up.
pub(super) fn auto(s: &mut Session) {
    let r = place_of_active(s).and_then(|place| {
        let db = open_db(place.project.as_ref().map(|(p, _)| p.as_str()))?;
        sync_active(s, &db, &place)
    });
    if let Err(e) = r {
        log::warn!("project database not updated: {e}");
    }
}

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("edb.sync", "Update Project Database", run_sync)
            .menu(&["Tools", "Electrical", "Database", "Update from Drawing"])
            .params("{} → {page, sheet, devices, newIds, removed, connections}"),
        CommandSpec::new("edb.push", "Update Drawing from Database", run_push)
            .menu(&["Tools", "Electrical", "Database", "Update Drawing"])
            .params("{} → {changed}"),
        CommandSpec::new("edb.device", "Device Data", run_device).menu(&["Tools", "Electrical", "Database", "Device Data"]).params(
            "{id? | handle? (default: the selected symbol), set?: {tag, manufacturer, catalog, description: [..3], location, installation, \
                 ratings: [{name, value, unit}]}} → the device (after the change, written to its symbol)",
        ),
        CommandSpec::new("edb.query", "Query Project Database", run_query)
            .menu(&["Tools", "Electrical", "Database", "Report"])
            .params(
                "{what: \"project\" | \"devices\" | \"connections\" | \"wirelist\" | \"bom\" | \"stats\" | \"conductor\" | \"neighbours\", \
                 device? (id), pin?} → the answer",
            )
            .noundo(),
        CommandSpec::new("eproject.sync", "Update Project Database (Whole Project)", run_project_sync)
            .menu(&["Tools", "Electrical", "Project", "Update Database (Whole Project)"])
            .params("{project?} → {drawings}")
            .enabled(crate::cmd::always)
            .noundo(),
    ]
}

fn run_sync(s: &mut Session, _p: &Value) -> Result<Value> {
    let place = place_of_active(s)?;
    let db = open_db(place.project.as_ref().map(|(p, _)| p.as_str()))?;
    let r = sync_active(s, &db, &place)?;
    s.log.push(format!(
        "Database updated: {} device(s) ({} new), {} connection(s), {} removed.",
        r["devices"], r["newIds"], r["connections"], r["removed"]
    ));
    Ok(r)
}

fn run_push(s: &mut Session, _p: &Value) -> Result<Value> {
    let place = place_of_active(s)?;
    let db = open_db(place.project.as_ref().map(|(p, _)| p.as_str()))?;
    let changed = push_active(s, &db, &place.page.id)?;
    s.log.push(format!("{changed} symbol(s) updated from the database."));
    Ok(json!({ "changed": changed }))
}

/// The device a command means: `id`, else the device of `handle` or the selected symbol.
fn target_device(s: &Session, db: &EcadDb, place: &Place, p: &Value) -> Result<EcadDevice> {
    if let Some(id) = str_param(p, "id") {
        return db.device(&DeviceId::from(id)).map_err(db_err)?.ok_or_else(|| EngineError::Other(format!("No device {id} in the database.")));
    }
    let h = crate::cmd::targets(s, p)?.first().copied().ok_or_else(|| bad("edb.device", "give `id`, or `handle` / select a symbol"))?;
    let eid = match s.doc()?.entity(h).map(|e| &e.kind) {
        Some(EntityKind::Insert(ins)) => attr(ins, "EID").filter(|e| !e.is_empty()),
        _ => None,
    };
    let found = match eid {
        Some(e) => db.device(&DeviceId(e)).map_err(db_err)?,
        None => db.device_by_entity(&place.page.id, h.0).map_err(db_err)?,
    };
    found.ok_or_else(|| EngineError::Other("That symbol is not in the database yet: run Update from Drawing (EDB.SYNC) first.".into()))
}

fn text_list(v: &Value, max: usize) -> Option<Vec<String>> {
    let a = v.as_array()?;
    Some(a.iter().take(max).map(|x| x.as_str().unwrap_or("").chars().take(255).collect()).collect())
}

fn run_device(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "edb.device";
    let place = place_of_active(s)?;
    let db = open_db(place.project.as_ref().map(|(p, _)| p.as_str()))?;
    let mut dev = target_device(s, &db, &place, p)?;
    if let Some(set) = p.get("set") {
        let set = set.as_object().ok_or_else(|| bad(CMD, "`set` must be an object"))?;
        let text = |k: &str| set.get(k).and_then(Value::as_str).map(|v| v.chars().take(255).collect::<String>());
        if let Some(v) = text("tag") {
            dev.tag = v;
        }
        if let Some(v) = text("manufacturer") {
            dev.part.manufacturer = v;
        }
        if let Some(v) = text("catalog") {
            dev.part.catalog = v;
        }
        if let Some(v) = text("location") {
            dev.location = v;
        }
        if let Some(v) = text("installation") {
            dev.installation = v;
        }
        if let Some(v) = set.get("description") {
            dev.part.description = text_list(v, 3).ok_or_else(|| bad(CMD, "`description` must be a list of up to 3 lines"))?;
        }
        if let Some(v) = set.get("ratings") {
            let a = v.as_array().ok_or_else(|| bad(CMD, "`ratings` must be a list of {name, value, unit}"))?;
            dev.part.ratings = a
                .iter()
                .take(64)
                .map(|r| {
                    Ok(Rating {
                        name: r.get("name").and_then(Value::as_str).ok_or_else(|| bad(CMD, "each rating needs a `name`"))?.chars().take(64).collect(),
                        value: r
                            .get("value")
                            .and_then(Value::as_f64)
                            .filter(|x| x.is_finite())
                            .ok_or_else(|| bad(CMD, "each rating needs a numeric `value`"))?,
                        unit: r.get("unit").and_then(Value::as_str).unwrap_or("").chars().take(32).collect(),
                    })
                })
                .collect::<Result<Vec<_>>>()?;
        }
        db.put_device(&dev).map_err(db_err)?;
        // Database → drawing: the symbol shows the change, wherever it is.
        let home = dev.graphic.as_ref().map(|g| g.page.clone());
        if home.as_ref() == Some(&place.page.id) {
            push_active(s, &db, &place.page.id)?;
        } else if let (Some(home), Some((proj, pr))) = (home, &place.project) {
            drop(db);
            let target = pr.drawings.iter().position(|d| page_id(d) == home);
            let only = Project { drawings: target.and_then(|i| pr.drawings.get(i).cloned()).into_iter().collect(), ..pr.clone() };
            super::project::for_each(s, proj, &only, true, |s, _, _| {
                let db = open_db(Some(proj))?;
                push_active(s, &db, &home).map(|n| json!(n))
            })?;
            let db = open_db(Some(proj))?;
            dev = db.device(&dev.id).map_err(db_err)?.unwrap_or(dev);
        }
        s.log.push(format!("{} updated.", dev.tag));
    }
    serde_json::to_value(&dev).map_err(|e| EngineError::Other(e.to_string()))
}

fn run_query(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "edb.query";
    let place = place_of_active(s)?;
    let db = open_db(place.project.as_ref().map(|(p, _)| p.as_str()))?;
    let what = str_param(p, "what").unwrap_or("stats").to_ascii_lowercase();
    Ok(match what.as_str() {
        "project" => to_json(&db.project().map_err(db_err)?),
        "devices" => to_json(&db.devices().map_err(db_err)?),
        "connections" => to_json(&db.connections().map_err(db_err)?),
        "wirelist" => {
            let list: Vec<Value> = db.wire_list().map_err(db_err)?.into_iter().map(|(n, a, b)| json!({ "wire": n, "from": a, "to": b })).collect();
            json!(list)
        }
        "bom" => {
            // Parts list: main devices grouped by part.
            let mut rows: BTreeMap<(String, String), (Vec<String>, String)> = BTreeMap::new();
            for d in db.devices().map_err(db_err)?.into_iter().filter(|d| !d.child) {
                let r =
                    rows.entry((d.part.manufacturer.clone(), d.part.catalog.clone())).or_insert_with(|| (Vec::new(), d.part.description.join(" ")));
                r.0.push(d.tag);
            }
            let list: Vec<Value> = rows
                .into_iter()
                .map(|((m, c), (mut tags, desc))| {
                    tags.sort();
                    json!({ "manufacturer": m, "catalog": c, "description": desc, "quantity": tags.len(), "tags": tags })
                })
                .collect();
            json!(list)
        }
        "stats" => to_json(&db.stats().map_err(db_err)?),
        "conductor" | "neighbours" => {
            let dev = str_param(p, "device").ok_or_else(|| bad(CMD, "`device` (id) is required"))?;
            let id = DeviceId::from(dev);
            if what == "neighbours" {
                let list: Vec<Value> = db.neighbours(&id).map_err(db_err)?.into_iter().map(|(a, b)| json!({ "pin": a.pin, "to": b })).collect();
                json!(list)
            } else {
                let pin = str_param(p, "pin").ok_or_else(|| bad(CMD, "`pin` is required"))?;
                to_json(&db.conductor(&PinRef::new(&id, pin)).map_err(db_err)?)
            }
        }
        _ => return Err(bad(CMD, "`what` must be project, devices, connections, wirelist, bom, stats, conductor or neighbours")),
    })
}

fn to_json<T: serde::Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

fn run_project_sync(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "eproject.sync";
    let proj = str_param(p, "project")
        .map(str::to_string)
        .or_else(current)
        .ok_or_else(|| bad(CMD, "no project is open: open one (EPROJECT.OPEN) or pass `project`"))?;
    let pr = read_project(&proj)?;
    let results = super::project::for_each(s, &proj, &pr, true, |s, i, sheet| {
        let rel = pr.drawings.get(i).cloned().unwrap_or_default();
        let place = Place {
            project: Some((proj.clone(), pr.clone())),
            page: EcadPage { id: page_id(&rel), sheet: u32::try_from(sheet).unwrap_or(u32::MAX), drawing: rel, ..Default::default() },
        };
        let db = open_db(Some(&proj))?;
        sync_active(s, &db, &place)
    })?;
    // Pages no longer in the project leave the database.
    let db = open_db(Some(&proj))?;
    let listed: HashSet<PageId> = pr.drawings.iter().map(|d| page_id(d)).collect();
    for g in db.pages().map_err(db_err)? {
        if !listed.contains(&g.id) {
            db.remove_page(&g.id).map_err(db_err)?;
        }
    }
    let stats = db.stats().map_err(db_err)?;
    s.log.push(format!("Project database: {} device(s), {} connection(s) on {} sheet(s).", stats.devices, stats.connections, stats.pages));
    Ok(json!({ "drawings": results, "stats": serde_json::to_value(stats).unwrap_or(Value::Null) }))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// A coil wired to a push-button above it, a lamp in parallel with the coil.
    /// A drawing of its own in the session's in-memory database (tests run side by side).
    fn session(name: &str) -> Session {
        let mut s = Session::new();
        let a = s.active;
        if let Some(d) = s.docs.get_mut(a) {
            d.title = format!("sync-test-{name}");
        }
        s
    }

    fn circuit(s: &mut Session) -> (String, String, String) {
        s.execute("eladder", &json!({ "top": 250, "bottom": 50, "from": 0, "to": 84 })).unwrap();
        let pb = s.execute("esymbol", &json!({ "name": "IEC_PB_NO", "at": [21, 200] })).unwrap()["handle"].as_str().unwrap().to_string();
        let k = s.execute("esymbol", &json!({ "name": "IEC_COIL", "at": [21, 120] })).unwrap()["handle"].as_str().unwrap().to_string();
        let h = s.execute("esymbol", &json!({ "name": "IEC_LAMP", "at": [63, 120] })).unwrap()["handle"].as_str().unwrap().to_string();
        (pb, k, h)
    }

    fn eid(s: &Session, h: &str) -> String {
        match &s.doc().unwrap().entity(Handle::parse_hex(h).unwrap()).unwrap().kind {
            EntityKind::Insert(i) => attr(i, "EID").unwrap_or_default(),
            _ => String::new(),
        }
    }

    #[test]
    fn drawing_to_database_and_back() {
        let mut s = session("both-ways");
        let (pb, k, _) = circuit(&mut s);
        // Placing the symbols already put them in the database (each electrical command syncs).
        let k_id = eid(&s, &k);
        let r = s.execute("edb.sync", &json!({})).unwrap();
        assert_eq!(r["devices"], 3);
        assert_eq!(r["newIds"], 0);
        assert!(k_id.starts_with("D-"), "the symbol now carries its device id");
        // Pins and the graph: the button's 14 and the coil's A1 are wired.
        let dev = s.execute("edb.device", &json!({ "id": k_id })).unwrap();
        assert_eq!(dev["tag"], "-K101");
        assert_eq!(dev["pins"].as_array().unwrap().len(), 2);
        assert_eq!(dev["graphic"]["block"], "IEC_COIL");
        let n = s.execute("edb.query", &json!({ "what": "neighbours", "device": k_id })).unwrap();
        let pb_id = eid(&s, &pb);
        assert!(n.as_array().unwrap().iter().any(|c| c["pin"] == "A1" && c["to"]["device"] == pb_id.as_str() && c["to"]["pin"] == "14"), "{n}");
        // Re-syncing keeps the ids.
        let r = s.execute("edb.sync", &json!({})).unwrap();
        assert_eq!(r["newIds"], 0);
        assert_eq!(eid(&s, &k), k_id);
        // Database → drawing: part data lands in the symbol's attributes.
        s.execute(
            "edb.device",
            &json!({ "handle": k, "set": { "manufacturer": "Acme", "catalog": "R-24DC", "description": ["Pump relay"],
                     "ratings": [{ "name": "coilVoltage", "value": 24, "unit": "V DC" }] } }),
        )
        .unwrap();
        let Some(EntityKind::Insert(ins)) = s.doc().unwrap().entity(Handle::parse_hex(&k).unwrap()).map(|e| e.kind.clone()) else { panic!() };
        assert_eq!(attr(&ins, "MFG").as_deref(), Some("Acme"));
        assert_eq!(attr(&ins, "CAT").as_deref(), Some("R-24DC"));
        assert_eq!(attr(&ins, "DESC1").as_deref(), Some("Pump relay"));
        // Drawing → database again keeps the ratings the symbol has no attribute for.
        s.execute("edb.sync", &json!({})).unwrap();
        let dev = s.execute("edb.device", &json!({ "id": k_id })).unwrap();
        assert_eq!(dev["part"]["ratings"][0]["value"], 24.0);
        assert_eq!(dev["part"]["manufacturer"], "Acme");
        // And editing the attribute in the drawing goes the other way.
        set_tag(&mut s, Handle::parse_hex(&k).unwrap(), "CAT", "R-24DC-B").unwrap();
        s.execute("edb.sync", &json!({})).unwrap();
        assert_eq!(s.execute("edb.device", &json!({ "id": k_id })).unwrap()["part"]["catalog"], "R-24DC-B");
        let bom = s.execute("edb.query", &json!({ "what": "bom" })).unwrap();
        assert!(bom.as_array().unwrap().iter().any(|r| r["catalog"] == "R-24DC-B" && r["quantity"] == 1), "{bom}");
    }

    #[test]
    fn erased_and_copied_symbols() {
        let mut s = session("erase-copy");
        let (_, k, h) = circuit(&mut s);
        s.execute("edb.sync", &json!({})).unwrap();
        // A copy carries the same EID: it becomes a device of its own.
        s.execute("copy", &json!({ "handles": [k], "from": [0, 0], "to": [42, 0] })).unwrap();
        let r = s.execute("edb.sync", &json!({})).unwrap();
        assert_eq!((r["devices"].as_u64(), r["newIds"].as_u64()), (Some(4), Some(1)), "{r}");
        // Erasing a symbol removes its device and connections.
        s.execute("erase", &json!({ "handles": [h] })).unwrap();
        let r = s.execute("edb.sync", &json!({})).unwrap();
        assert_eq!(r["removed"], 1);
        let stats = s.execute("edb.query", &json!({ "what": "stats" })).unwrap();
        assert!(stats["devices"].as_u64().unwrap() >= 3);
        assert!(s.execute("edb.query", &json!({ "what": "nonsense" })).is_err());
        assert!(s.execute("edb.device", &json!({ "id": "D-missing" })).is_err());
        assert!(s.execute("edb.device", &json!({ "handle": k, "set": { "ratings": [{ "value": 1 }] } })).is_err());
    }
}
