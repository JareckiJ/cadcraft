//! Typed access to the graph: pages, devices and connections in a [`Store`], with indexes and
//! graph queries.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::store::{Batch, MemStore, Op, Store, decode, encode};
use crate::{ConnectionId, DeviceId, EcadConnection, EcadDevice, EcadError, EcadPage, EcadProject, PageId, PinRef, ProjectId, Result};

/// The project record (the registries live in their own tables).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Meta {
    id: ProjectId,
    name: String,
    description: String,
    metadata: BTreeMap<String, String>,
    revision: u64,
}

/// Counts for a quick look at a graph.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct GraphStats {
    pub pages: usize,
    pub devices: usize,
    pub connections: usize,
    /// Distinct networks (conductors).
    pub networks: usize,
}

/// The electrical project graph in a store.
pub struct EcadDb {
    store: Box<dyn Store>,
}

impl std::fmt::Debug for EcadDb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EcadDb")
    }
}

/// The key of the entity index: a page and a handle.
fn entity_key(page: &PageId, handle: u64) -> String {
    format!("{}/{:X}", page.0, handle)
}

impl EcadDb {
    pub fn new(store: Box<dyn Store>) -> Self {
        EcadDb { store }
    }

    /// A graph kept in memory.
    pub fn memory() -> Self {
        EcadDb::new(Box::new(MemStore::new()))
    }

    /// The graph in the database file at `path` (created if missing).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open(path: &std::path::Path) -> Result<Self> {
        Ok(EcadDb::new(Box::new(crate::store::RedbStore::open(path)?)))
    }

    fn meta(&self) -> Result<Meta> {
        Ok(match self.store.get("meta", "project")? {
            Some(b) => decode(&b)?,
            None => Meta::default(),
        })
    }

    /// The batch op that bumps the project revision.
    fn bump(&self) -> Result<Op> {
        let mut m = self.meta()?;
        m.revision = m.revision.saturating_add(1);
        Ok(Op::Put { table: "meta", key: "project".into(), value: encode(&m)? })
    }

    fn scan<T: serde::de::DeserializeOwned>(&self, table: &'static str) -> Result<Vec<T>> {
        self.store.scan(table)?.iter().map(|(_, v)| decode(v)).collect()
    }

    // ---------------- project ----------------

    /// Set the project's identity and metadata (a new id when it has none).
    pub fn set_project(&self, name: &str, description: &str, metadata: BTreeMap<String, String>) -> Result<ProjectId> {
        let mut m = self.meta()?;
        if m.id.0.is_empty() {
            m.id = ProjectId::new(name);
        }
        m.name = name.to_string();
        m.description = description.to_string();
        m.metadata = metadata;
        m.revision = m.revision.saturating_add(1);
        let id = m.id.clone();
        self.store.apply(vec![Op::Put { table: "meta", key: "project".into(), value: encode(&m)? }])?;
        Ok(id)
    }

    /// The whole project: identity, pages in sheet order, every device and connection.
    pub fn project(&self) -> Result<EcadProject> {
        let m = self.meta()?;
        Ok(EcadProject {
            id: m.id,
            name: m.name,
            description: m.description,
            pages: self.pages()?,
            metadata: m.metadata,
            devices: self.devices()?.into_iter().map(|d| (d.id.clone(), d)).collect(),
            connections: self.connections()?.into_iter().map(|c| (c.id.clone(), c)).collect(),
            revision: m.revision,
        })
    }

    pub fn revision(&self) -> Result<u64> {
        Ok(self.meta()?.revision)
    }

    // ---------------- pages ----------------

    pub fn put_page(&self, page: &EcadPage) -> Result<()> {
        if page.id.0.is_empty() {
            return Err(EcadError::Invalid("a page needs an id".into()));
        }
        self.store.apply(vec![Op::Put { table: "pages", key: page.id.0.clone(), value: encode(page)? }, self.bump()?])
    }

    /// Pages in sheet order.
    pub fn pages(&self) -> Result<Vec<EcadPage>> {
        let mut v: Vec<EcadPage> = self.scan("pages")?;
        v.sort_by(|a, b| a.sheet.cmp(&b.sheet).then_with(|| a.drawing.cmp(&b.drawing)));
        Ok(v)
    }

    pub fn page(&self, id: &PageId) -> Result<Option<EcadPage>> {
        self.store.get("pages", &id.0)?.map(|b| decode(&b)).transpose()
    }

    /// The page showing drawing `drawing` (as listed in the project), if any.
    pub fn page_by_drawing(&self, drawing: &str) -> Result<Option<EcadPage>> {
        let norm = |s: &str| s.replace('\\', "/").to_ascii_lowercase();
        Ok(self.pages()?.into_iter().find(|p| norm(&p.drawing) == norm(drawing)))
    }

    /// Remove a page with its devices and connections.
    pub fn remove_page(&self, id: &PageId) -> Result<()> {
        let mut batch: Batch = vec![Op::Delete { table: "pages", key: id.0.clone() }];
        for d in self.devices_on_page(id)? {
            batch.extend(self.device_removal(&d)?);
        }
        for c in self.connections()?.into_iter().filter(|c| &c.page == id) {
            batch.push(Op::Delete { table: "connections", key: c.id.0 });
        }
        batch.push(self.bump()?);
        self.store.apply(batch)
    }

    // ---------------- devices ----------------

    /// The ops that store `d` (revision bumped) and keep the entity index right.
    fn device_put(&self, d: &EcadDevice) -> Result<Batch> {
        if d.id.0.is_empty() {
            return Err(EcadError::Invalid("a device needs an id".into()));
        }
        let old = self.device(&d.id)?;
        let mut d = d.clone();
        d.revision = old.as_ref().map(|o| o.revision).unwrap_or(0).saturating_add(1);
        let mut batch = Vec::new();
        if let Some(g) = old.as_ref().and_then(|o| o.graphic.as_ref()) {
            batch.push(Op::Delete { table: "by_entity", key: entity_key(&g.page, g.handle) });
        }
        if let Some(g) = &d.graphic {
            batch.push(Op::Put { table: "by_entity", key: entity_key(&g.page, g.handle), value: d.id.0.as_bytes().to_vec() });
        }
        batch.push(Op::Put { table: "devices", key: d.id.0.clone(), value: encode(&d)? });
        Ok(batch)
    }

    /// Add or update a device.
    pub fn put_device(&self, d: &EcadDevice) -> Result<()> {
        let mut batch = self.device_put(d)?;
        batch.push(self.bump()?);
        self.store.apply(batch)
    }

    pub fn device(&self, id: &DeviceId) -> Result<Option<EcadDevice>> {
        self.store.get("devices", &id.0)?.map(|b| decode(&b)).transpose()
    }

    /// Every device, by id.
    pub fn devices(&self) -> Result<Vec<EcadDevice>> {
        self.scan("devices")
    }

    pub fn devices_on_page(&self, page: &PageId) -> Result<Vec<EcadDevice>> {
        Ok(self.devices()?.into_iter().filter(|d| d.graphic.as_ref().is_some_and(|g| &g.page == page)).collect())
    }

    /// The devices tagged `tag` (a main device and its contacts share a tag).
    pub fn devices_tagged(&self, tag: &str) -> Result<Vec<EcadDevice>> {
        Ok(self.devices()?.into_iter().filter(|d| d.tag == tag).collect())
    }

    /// The device shown by the block reference `handle` on `page`.
    pub fn device_by_entity(&self, page: &PageId, handle: u64) -> Result<Option<EcadDevice>> {
        match self.store.get("by_entity", &entity_key(page, handle))? {
            Some(id) => self.device(&DeviceId(String::from_utf8_lossy(&id).to_string())),
            None => Ok(None),
        }
    }

    /// The ops that remove `d`, its index entry and its connections.
    fn device_removal(&self, d: &EcadDevice) -> Result<Batch> {
        let mut batch = vec![Op::Delete { table: "devices", key: d.id.0.clone() }];
        if let Some(g) = &d.graphic {
            batch.push(Op::Delete { table: "by_entity", key: entity_key(&g.page, g.handle) });
        }
        for c in self.connections_of(&d.id)? {
            batch.push(Op::Delete { table: "connections", key: c.id.0 });
        }
        Ok(batch)
    }

    /// Remove a device and every connection to it.
    pub fn remove_device(&self, id: &DeviceId) -> Result<()> {
        let d = self.device(id)?.ok_or_else(|| EcadError::NotFound(format!("device {id}")))?;
        let mut batch = self.device_removal(&d)?;
        batch.push(self.bump()?);
        self.store.apply(batch)
    }

    // ---------------- connections ----------------

    fn check_ends(&self, c: &EcadConnection) -> Result<()> {
        for end in [&c.from, &c.to] {
            if self.device(&end.device)?.is_none() {
                return Err(EcadError::NotFound(format!("device {} of connection {}", end.device, c.id)));
            }
        }
        if c.from == c.to {
            return Err(EcadError::Invalid(format!("connection {} joins a pin to itself", c.id)));
        }
        Ok(())
    }

    /// Add or update a connection; both ends must be stored devices.
    pub fn put_connection(&self, c: &EcadConnection) -> Result<()> {
        if c.id.0.is_empty() {
            return Err(EcadError::Invalid("a connection needs an id".into()));
        }
        self.check_ends(c)?;
        self.store.apply(vec![Op::Put { table: "connections", key: c.id.0.clone(), value: encode(c)? }, self.bump()?])
    }

    pub fn connection(&self, id: &ConnectionId) -> Result<Option<EcadConnection>> {
        self.store.get("connections", &id.0)?.map(|b| decode(&b)).transpose()
    }

    pub fn connections(&self) -> Result<Vec<EcadConnection>> {
        self.scan("connections")
    }

    /// The connections at any pin of `device`.
    pub fn connections_of(&self, device: &DeviceId) -> Result<Vec<EcadConnection>> {
        Ok(self.connections()?.into_iter().filter(|c| c.touches(device)).collect())
    }

    pub fn remove_connection(&self, id: &ConnectionId) -> Result<()> {
        self.store.apply(vec![Op::Delete { table: "connections", key: id.0.clone() }, self.bump()?])
    }

    // ---------------- synchronising a page ----------------

    /// Make `page` hold exactly `devices` and `connections` (as read from its drawing), in one
    /// transaction: devices of the page not listed go, with their connections; the page's
    /// connections are replaced. Devices keep their part data unless the new record sets it.
    pub fn replace_page(&self, page: &EcadPage, devices: &[EcadDevice], connections: &[EcadConnection]) -> Result<()> {
        let mut batch: Batch = vec![Op::Put { table: "pages", key: page.id.0.clone(), value: encode(page)? }];
        let keep: BTreeSet<&DeviceId> = devices.iter().map(|d| &d.id).collect();
        for old in self.devices_on_page(&page.id)? {
            if !keep.contains(&old.id) {
                batch.extend(self.device_removal(&old)?);
            }
        }
        for c in self.connections()?.into_iter().filter(|c| c.page == page.id) {
            batch.push(Op::Delete { table: "connections", key: c.id.0 });
        }
        for d in devices {
            batch.extend(self.device_put(d)?);
        }
        let ids: BTreeSet<&DeviceId> = devices.iter().map(|d| &d.id).collect();
        for c in connections {
            let known = |id: &DeviceId| -> Result<bool> { Ok(ids.contains(id) || self.device(id)?.is_some()) };
            if known(&c.from.device)? && known(&c.to.device)? && c.from != c.to {
                batch.push(Op::Put { table: "connections", key: c.id.0.clone(), value: encode(c)? });
            }
        }
        batch.push(self.bump()?);
        self.store.apply(batch)
    }

    // ---------------- graph queries ----------------

    /// Every pin wired to `pin` through connections (the conductor it is on), `pin` included.
    /// Devices do not conduct here: a coil's A1 and A2 are separate conductors.
    pub fn conductor(&self, pin: &PinRef) -> Result<BTreeSet<PinRef>> {
        let all = self.connections()?;
        let mut seen: BTreeSet<PinRef> = BTreeSet::new();
        let mut queue: VecDeque<PinRef> = VecDeque::from([pin.clone()]);
        while let Some(p) = queue.pop_front() {
            if !seen.insert(p.clone()) {
                continue;
            }
            for c in &all {
                if let Some(o) = c.other_end(&p)
                    && !seen.contains(o)
                {
                    queue.push_back(o.clone());
                }
            }
        }
        Ok(seen)
    }

    /// The devices directly wired to `device`: (its pin, the other device's pin).
    pub fn neighbours(&self, device: &DeviceId) -> Result<Vec<(PinRef, PinRef)>> {
        let mut out = Vec::new();
        for c in self.connections_of(device)? {
            if &c.from.device == device {
                out.push((c.from, c.to));
            } else {
                out.push((c.to, c.from));
            }
        }
        out.sort();
        Ok(out)
    }

    /// The from–to wire list, in wire-number order: for each connection the two pins as
    /// tag:pin, with its wire number.
    pub fn wire_list(&self) -> Result<Vec<(String, String, String)>> {
        let devices: BTreeMap<DeviceId, EcadDevice> = self.devices()?.into_iter().map(|d| (d.id.clone(), d)).collect();
        let name = |p: &PinRef| format!("{}:{}", devices.get(&p.device).map(|d| d.tag.as_str()).unwrap_or("?"), p.pin);
        let mut v: Vec<(String, String, String)> =
            self.connections()?.iter().map(|c| (c.wire_number.clone().unwrap_or_default(), name(&c.from), name(&c.to))).collect();
        v.sort();
        Ok(v)
    }

    pub fn stats(&self) -> Result<GraphStats> {
        let connections = self.connections()?;
        let networks: BTreeSet<&str> = connections.iter().filter_map(|c| c.network.as_deref()).collect();
        Ok(GraphStats { pages: self.pages()?.len(), devices: self.devices()?.len(), connections: connections.len(), networks: networks.len() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DeviceFamily, GraphicRef, PartSpec};

    fn page(n: u32) -> EcadPage {
        EcadPage { id: PageId(format!("G-{n}")), sheet: n, drawing: format!("sheet{n:02}.dxf"), ..Default::default() }
    }

    fn device(id: &str, tag: &str, page: &EcadPage, handle: u64) -> EcadDevice {
        EcadDevice {
            id: DeviceId::from(id),
            tag: tag.into(),
            class: "K".into(),
            family: DeviceFamily::Coil,
            graphic: Some(GraphicRef { page: page.id.clone(), handle, block: "IEC_COIL".into(), ..Default::default() }),
            ..Default::default()
        }
    }

    fn conn(id: &str, a: (&str, &str), b: (&str, &str), net: &str) -> EcadConnection {
        EcadConnection {
            id: ConnectionId::from(id),
            from: PinRef::new(&DeviceId::from(a.0), a.1),
            to: PinRef::new(&DeviceId::from(b.0), b.1),
            network: Some(net.into()),
            wire_number: Some(net.into()),
            page: PageId::from("G-1"),
            ..Default::default()
        }
    }

    fn sample(db: &EcadDb) {
        let p1 = page(1);
        db.put_page(&p1).unwrap();
        db.put_page(&page(2)).unwrap();
        for (id, tag, h) in [("D-s", "-S101", 0x10), ("D-k", "-K102", 0x11), ("D-h", "-P103", 0x12)] {
            db.put_device(&device(id, tag, &p1, h)).unwrap();
        }
        db.put_connection(&conn("C-1", ("D-s", "14"), ("D-k", "A1"), "101")).unwrap();
        db.put_connection(&conn("C-2", ("D-k", "A1"), ("D-h", "X1"), "101")).unwrap();
        db.put_connection(&conn("C-3", ("D-k", "A2"), ("D-h", "X2"), "102")).unwrap();
    }

    #[test]
    fn devices_pages_and_index() {
        let db = EcadDb::memory();
        let id = db.set_project("Plant", "Line 3", BTreeMap::from([("customer".to_string(), "Acme".to_string())])).unwrap();
        sample(&db);
        let p = db.project().unwrap();
        assert_eq!(p.id, id);
        assert_eq!(p.pages.iter().map(|g| g.sheet).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(p.devices.len(), 3);
        assert_eq!(p.connections.len(), 3);
        assert_eq!(p.metadata.get("customer").map(String::as_str), Some("Acme"));
        // The drawing finds its device by handle, and moving the symbol's handle moves the index.
        let g1 = PageId::from("G-1");
        assert_eq!(db.device_by_entity(&g1, 0x11).unwrap().unwrap().tag, "-K102");
        let mut k = db.device(&DeviceId::from("D-k")).unwrap().unwrap();
        assert_eq!(k.revision, 1);
        k.part = PartSpec { manufacturer: "Acme".into(), catalog: "R24".into(), ..Default::default() };
        if let Some(g) = k.graphic.as_mut() {
            g.handle = 0x99;
        }
        db.put_device(&k).unwrap();
        assert!(db.device_by_entity(&g1, 0x11).unwrap().is_none());
        let k = db.device_by_entity(&g1, 0x99).unwrap().unwrap();
        assert_eq!((k.revision, k.part.catalog.as_str()), (2, "R24"));
        assert_eq!(db.page_by_drawing("SHEET02.DXF").unwrap().map(|p| p.sheet), Some(2));
    }

    #[test]
    fn graph_queries() {
        let db = EcadDb::memory();
        sample(&db);
        let a1 = PinRef::new(&DeviceId::from("D-k"), "A1");
        let conductor = db.conductor(&a1).unwrap();
        assert_eq!(conductor.len(), 3, "S:14, K:A1, H:X1 are one conductor; A2 is not");
        assert!(!conductor.contains(&PinRef::new(&DeviceId::from("D-k"), "A2")));
        assert_eq!(db.neighbours(&DeviceId::from("D-k")).unwrap().len(), 3);
        let wl = db.wire_list().unwrap();
        assert_eq!(wl.first().map(|w| (w.0.as_str(), w.1.as_str())), Some(("101", "-K102:A1")));
        assert_eq!(db.stats().unwrap(), GraphStats { pages: 2, devices: 3, connections: 3, networks: 2 });
    }

    #[test]
    fn removing_a_device_removes_its_connections() {
        let db = EcadDb::memory();
        sample(&db);
        db.remove_device(&DeviceId::from("D-h")).unwrap();
        assert_eq!(db.connections().unwrap().len(), 1);
        assert!(db.remove_device(&DeviceId::from("D-h")).is_err());
        // A connection to a device that isn't stored is refused.
        assert!(db.put_connection(&conn("C-9", ("D-s", "13"), ("D-gone", "1"), "x")).is_err());
        assert!(db.put_connection(&conn("C-9", ("D-s", "13"), ("D-s", "13"), "x")).is_err());
    }

    #[test]
    fn replacing_a_page_syncs_it() {
        let db = EcadDb::memory();
        sample(&db);
        let p1 = page(1);
        // The drawing now has S and K only, wired once.
        let devices = vec![device("D-s", "-S101", &p1, 0x10), device("D-k", "-K102", &p1, 0x11)];
        let c = conn("C-new", ("D-s", "14"), ("D-k", "A1"), "101");
        db.replace_page(&p1, &devices, std::slice::from_ref(&c)).unwrap();
        assert_eq!(db.devices().unwrap().len(), 2);
        assert_eq!(db.connections().unwrap().iter().map(|c| c.id.0.as_str()).collect::<Vec<_>>(), vec!["C-new"]);
        assert!(db.revision().unwrap() > 0);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn on_disk_graph_survives_reopening() {
        let path = std::env::temp_dir().join(format!("cadcraft-ecad-db-{}.ecdb", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let db = EcadDb::open(&path).unwrap();
            db.set_project("Plant", "", BTreeMap::new()).unwrap();
            sample(&db);
        }
        let db = EcadDb::open(&path).unwrap();
        let p = db.project().unwrap();
        assert_eq!((p.name.as_str(), p.devices.len(), p.connections.len()), ("Plant", 3, 3));
        drop(db);
        let _ = std::fs::remove_file(&path);
    }
}
