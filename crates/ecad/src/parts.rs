//! Manufacturer parts: the reusable definition of a product (a relay type, a breaker type), as
//! shared in the community parts database and cached locally.
//!
//! A [`ManufacturerPart`] carries what a schematic needs to place the product: its symbol drawing,
//! its pins (names to symbol coordinates) and its ratings. Field names match the database columns
//! (`manufacturer_parts`), so records travel as JSON unchanged.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::store::{Op, Store, decode, encode};
use crate::{EcadError, Result};

/// Where a pin sits on the symbol, and which way wires leave it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PinPoint {
    pub x: f64,
    pub y: f64,
    /// `up`, `right`, `down` or `left`.
    pub dir: String,
}

/// A symbol drawing in symbol coordinates (base point at the origin).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SymbolGeometry {
    /// `[x1, y1, x2, y2]`.
    pub lines: Vec<[f64; 4]>,
    /// `[x, y, r]`.
    pub circles: Vec<[f64; 3]>,
    /// `[x, y, r, start°, end°]`, counter-clockwise.
    pub arcs: Vec<[f64; 5]>,
    /// `[x, y, height]` and the text.
    pub texts: Vec<([f64; 3], String)>,
}

impl SymbolGeometry {
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty() && self.circles.is_empty() && self.arcs.is_empty() && self.texts.is_empty()
    }
}

/// A manufacturer's product, as shared.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ManufacturerPart {
    /// The database id (absent until uploaded).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub part_number: String,
    pub manufacturer: String,
    /// Relay, Contactor, Breaker, VFD, Fuse, Motor, Terminal, PSU, PLC, Sensor, Lamp, Other.
    pub component_type: String,
    /// IEC 81346-2 class letters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iec_class: Option<String>,
    pub description: String,
    /// Pin name → position on the symbol.
    pub pin_map: BTreeMap<String, PinPoint>,
    /// Ratings and other data: `{"coilVoltage": {"value": 24, "unit": "V DC"}}`, footprint paths…
    pub attributes: BTreeMap<String, Value>,
    pub cad_geometry: SymbolGeometry,
    /// Always CC BY 4.0 in the community database.
    pub license: String,
    /// Set by the server: the contributor's GitHub login, their user id, last change.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub github_login: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// The licence of shared parts.
pub const PART_LICENSE: &str = "CC-BY-4.0";

/// The component types offered when sharing a part.
pub const COMPONENT_TYPES: &[&str] =
    &["Relay", "Contactor", "Breaker", "Fuse", "Overload", "VFD", "Motor", "PSU", "PLC", "Sensor", "Switch", "Lamp", "Terminal", "Other"];

impl ManufacturerPart {
    /// The cache key: manufacturer and part number, any case.
    pub fn key(&self) -> String {
        format!("{}|{}", self.manufacturer.trim().to_lowercase(), self.part_number.trim().to_lowercase())
    }

    /// Whether the part can be shared: named, with pins, and not oversized.
    pub fn validate(&self) -> Result<()> {
        let bad = |m: &str| Err(EcadError::Invalid(m.to_string()));
        if self.part_number.trim().is_empty() || self.part_number.chars().count() > 128 {
            return bad("A part needs a part number (up to 128 characters).");
        }
        if self.manufacturer.trim().is_empty() || self.manufacturer.chars().count() > 128 {
            return bad("A part needs a manufacturer (up to 128 characters).");
        }
        if self.pin_map.is_empty() {
            return bad("A part needs at least one pin.");
        }
        if self.iec_class.as_deref().is_some_and(|c| c.is_empty() || c.len() > 3 || !c.chars().all(|ch| ch.is_ascii_uppercase())) {
            return bad("The IEC class must be one to three capital letters.");
        }
        let size = encode(self)?.len();
        if size >= 1 << 20 {
            return bad("The part is too large to share (over 1 MB).");
        }
        Ok(())
    }
}

/// Parts cached locally (downloaded from the community database or made here), by
/// manufacturer and part number, for offline use.
pub struct PartsCache<'a> {
    store: &'a dyn Store,
}

impl<'a> PartsCache<'a> {
    pub fn new(store: &'a dyn Store) -> Self {
        PartsCache { store }
    }
    pub fn put(&self, parts: &[ManufacturerPart]) -> Result<()> {
        let batch = parts.iter().map(|p| Ok(Op::Put { table: "parts", key: p.key(), value: encode(p)? })).collect::<Result<Vec<_>>>()?;
        self.store.apply(batch)
    }
    pub fn get(&self, manufacturer: &str, part_number: &str) -> Result<Option<ManufacturerPart>> {
        let key = format!("{}|{}", manufacturer.trim().to_lowercase(), part_number.trim().to_lowercase());
        self.store.get("parts", &key)?.map(|b| decode(&b)).transpose()
    }
    /// Cached parts whose part number, manufacturer, type or description contains every word of
    /// `query` (any case), at most `limit`.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<ManufacturerPart>> {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        let mut out = Vec::new();
        for (_, v) in self.store.scan("parts")? {
            let p: ManufacturerPart = decode(&v)?;
            let hay = format!("{} {} {} {}", p.part_number, p.manufacturer, p.component_type, p.description).to_lowercase();
            if words.iter().all(|w| hay.contains(w.as_str())) {
                out.push(p);
                if out.len() >= limit {
                    break;
                }
            }
        }
        Ok(out)
    }
    pub fn remove(&self, manufacturer: &str, part_number: &str) -> Result<()> {
        let key = format!("{}|{}", manufacturer.trim().to_lowercase(), part_number.trim().to_lowercase());
        self.store.apply(vec![Op::Delete { table: "parts", key }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemStore;

    fn relay() -> ManufacturerPart {
        ManufacturerPart {
            part_number: "R-24DC".into(),
            manufacturer: "Acme".into(),
            component_type: "Relay".into(),
            iec_class: Some("K".into()),
            description: "Interface relay 24 V DC".into(),
            pin_map: BTreeMap::from([("A1".into(), PinPoint { x: 0.0, y: 5.0, dir: "up".into() })]),
            attributes: BTreeMap::from([("coilVoltage".into(), serde_json::json!({ "value": 24, "unit": "V DC" }))]),
            cad_geometry: SymbolGeometry { lines: vec![[-4.0, -3.0, 4.0, -3.0]], ..Default::default() },
            license: PART_LICENSE.into(),
            ..Default::default()
        }
    }

    #[test]
    fn parts_serialise_as_database_rows() {
        let v = serde_json::to_value(relay()).unwrap();
        assert_eq!(v["part_number"], "R-24DC");
        assert_eq!(v["pin_map"]["A1"]["dir"], "up");
        assert!(v.get("id").is_none() && v.get("created_by").is_none(), "server fields are not sent");
        let back: ManufacturerPart = serde_json::from_value(serde_json::json!({
            "id": "0b6e…", "part_number": "R-24DC", "manufacturer": "Acme", "status": "published", "github_login": "elliott"
        }))
        .unwrap();
        assert_eq!(back.github_login.as_deref(), Some("elliott"));
    }

    #[test]
    fn validation() {
        assert!(relay().validate().is_ok());
        let mut p = relay();
        p.pin_map.clear();
        assert!(p.validate().is_err());
        let mut p = relay();
        p.part_number = " ".into();
        assert!(p.validate().is_err());
        let mut p = relay();
        p.iec_class = Some("k".into());
        assert!(p.validate().is_err());
    }

    #[test]
    fn cache_finds_parts_offline() {
        let store = MemStore::new();
        let cache = PartsCache::new(&store);
        let mut other = relay();
        other.part_number = "MCB-C10".into();
        other.component_type = "Breaker".into();
        other.description = "Circuit-breaker C10".into();
        cache.put(&[relay(), other]).unwrap();
        assert_eq!(cache.search("acme relay", 10).unwrap().len(), 1);
        assert_eq!(cache.search("", 10).unwrap().len(), 2);
        assert!(cache.get("ACME", "r-24dc").unwrap().is_some());
        cache.remove("Acme", "R-24DC").unwrap();
        assert!(cache.get("Acme", "R-24DC").unwrap().is_none());
    }
}
