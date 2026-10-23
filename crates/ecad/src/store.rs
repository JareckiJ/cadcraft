//! Where the graph is kept: a small key–value interface with an in-memory store and, natively,
//! a `redb` database file.
//!
//! Records are JSON in named tables, keyed by string. Changes are applied as a [`Batch`] in one
//! transaction, so a crash leaves either all of a change or none of it.

use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::{EcadError, Result};

/// The tables the graph uses.
pub const TABLES: &[&str] = &["meta", "pages", "devices", "connections", "by_entity"];

/// One change in a [`Batch`].
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    Put { table: &'static str, key: String, value: Vec<u8> },
    Delete { table: &'static str, key: String },
}

/// Changes applied together.
pub type Batch = Vec<Op>;

/// A key–value store of named tables.
pub trait Store: Send + Sync {
    fn get(&self, table: &'static str, key: &str) -> Result<Option<Vec<u8>>>;
    /// Every (key, value) of `table`, by key.
    fn scan(&self, table: &'static str) -> Result<Vec<(String, Vec<u8>)>>;
    /// Apply `batch` atomically.
    fn apply(&self, batch: Batch) -> Result<()>;
}

/// The graph in memory (tests, the web build, drawings outside a project).
#[derive(Debug, Default)]
pub struct MemStore {
    tables: Mutex<BTreeMap<&'static str, BTreeMap<String, Vec<u8>>>>,
}

impl MemStore {
    pub fn new() -> Self {
        MemStore::default()
    }
    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<&'static str, BTreeMap<String, Vec<u8>>>> {
        match self.tables.lock() {
            Ok(g) => g,
            Err(e) => e.into_inner(),
        }
    }
}

impl Store for MemStore {
    fn get(&self, table: &'static str, key: &str) -> Result<Option<Vec<u8>>> {
        Ok(self.lock().get(table).and_then(|t| t.get(key)).cloned())
    }
    fn scan(&self, table: &'static str) -> Result<Vec<(String, Vec<u8>)>> {
        Ok(self.lock().get(table).map(|t| t.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default())
    }
    fn apply(&self, batch: Batch) -> Result<()> {
        let mut g = self.lock();
        for op in batch {
            match op {
                Op::Put { table, key, value } => {
                    g.entry(table).or_default().insert(key, value);
                }
                Op::Delete { table, key } => {
                    if let Some(t) = g.get_mut(table) {
                        t.remove(&key);
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod disk {
    use redb::{Database, ReadableTable, TableDefinition};

    use super::{Batch, Op, Store};
    use crate::{EcadError, Result};

    fn err(e: impl std::fmt::Display) -> EcadError {
        EcadError::Store(e.to_string())
    }

    fn def(table: &'static str) -> TableDefinition<'static, &'static str, &'static [u8]> {
        TableDefinition::new(table)
    }

    /// The graph in a `redb` database file (crash-safe, one writer at a time).
    pub struct RedbStore {
        db: Database,
    }

    impl std::fmt::Debug for RedbStore {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("RedbStore")
        }
    }

    impl RedbStore {
        /// Open the database at `path`, creating it (and its tables) if missing.
        pub fn open(path: &std::path::Path) -> Result<Self> {
            let db = Database::create(path).map_err(err)?;
            let tx = db.begin_write().map_err(err)?;
            for t in super::TABLES {
                tx.open_table(def(t)).map_err(err)?;
            }
            tx.commit().map_err(err)?;
            Ok(RedbStore { db })
        }
    }

    impl Store for RedbStore {
        fn get(&self, table: &'static str, key: &str) -> Result<Option<Vec<u8>>> {
            let tx = self.db.begin_read().map_err(err)?;
            let t = match tx.open_table(def(table)) {
                Ok(t) => t,
                Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
                Err(e) => return Err(err(e)),
            };
            Ok(t.get(key).map_err(err)?.map(|v| v.value().to_vec()))
        }
        fn scan(&self, table: &'static str) -> Result<Vec<(String, Vec<u8>)>> {
            let tx = self.db.begin_read().map_err(err)?;
            let t = match tx.open_table(def(table)) {
                Ok(t) => t,
                Err(redb::TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
                Err(e) => return Err(err(e)),
            };
            let mut out = Vec::new();
            for item in t.iter().map_err(err)? {
                let (k, v) = item.map_err(err)?;
                out.push((k.value().to_string(), v.value().to_vec()));
            }
            Ok(out)
        }
        fn apply(&self, batch: Batch) -> Result<()> {
            let tx = self.db.begin_write().map_err(err)?;
            for op in &batch {
                match op {
                    Op::Put { table, key, value } => {
                        let mut t = tx.open_table(def(table)).map_err(err)?;
                        t.insert(key.as_str(), value.as_slice()).map_err(err)?;
                    }
                    Op::Delete { table, key } => {
                        let mut t = tx.open_table(def(table)).map_err(err)?;
                        t.remove(key.as_str()).map_err(err)?;
                    }
                }
            }
            tx.commit().map_err(err)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use disk::RedbStore;

/// A JSON record.
pub(crate) fn encode<T: serde::Serialize>(v: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(v).map_err(|e| EcadError::Codec(e.to_string()))
}

pub(crate) fn decode<T: serde::de::DeserializeOwned>(b: &[u8]) -> Result<T> {
    serde_json::from_slice(b).map_err(|e| EcadError::Codec(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exercise(s: &dyn Store) {
        s.apply(vec![
            Op::Put { table: "devices", key: "b".into(), value: b"2".to_vec() },
            Op::Put { table: "devices", key: "a".into(), value: b"1".to_vec() },
        ])
        .unwrap();
        assert_eq!(s.get("devices", "a").unwrap(), Some(b"1".to_vec()));
        assert_eq!(s.get("devices", "zz").unwrap(), None);
        assert_eq!(s.get("pages", "a").unwrap(), None);
        let keys: Vec<String> = s.scan("devices").unwrap().into_iter().map(|(k, _)| k).collect();
        assert_eq!(keys, vec!["a", "b"]);
        s.apply(vec![Op::Delete { table: "devices", key: "a".into() }, Op::Delete { table: "pages", key: "none".into() }]).unwrap();
        assert_eq!(s.scan("devices").unwrap().len(), 1);
    }

    #[test]
    fn memory_store() {
        exercise(&MemStore::new());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn redb_store_persists() {
        let path = std::env::temp_dir().join(format!("cadcraft-ecad-store-{}.ecdb", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let s = RedbStore::open(&path).unwrap();
            exercise(&s);
        }
        let s = RedbStore::open(&path).unwrap();
        assert_eq!(s.get("devices", "b").unwrap(), Some(b"2".to_vec()), "survives reopening");
        drop(s);
        let _ = std::fs::remove_file(&path);
    }
}
