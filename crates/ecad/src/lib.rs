//! The electrical project data graph.
//!
//! A schematic is drawings; an electrical project is also a *graph*: physical devices (relays,
//! breakers, motors) with pins, and connections between those pins. This crate holds that graph,
//! independent of how it is drawn:
//!
//! - [`EcadProject`]: the project, its [`EcadPage`]s (one per drawing/sheet), metadata and the
//!   registry of active devices and connections;
//! - [`EcadDevice`]: a piece of hardware — tag, class, part (manufacturer, catalogue number,
//!   technical ratings), [`EcadPin`]s — and the drawing object that shows it ([`GraphicRef`]:
//!   page, entity handle, block), so the drawing and the database stay in step both ways;
//! - [`EcadConnection`]: an edge of the graph, device A pin 1 to device B pin 4, with its wire
//!   number and the wires that draw it.
//!
//! [`EcadDb`] stores the graph in an embedded database ([`store`]: `redb` on disk, or memory)
//! with typed access and graph queries. The crate knows nothing about CAD geometry beyond the ids
//! it keeps; the engine reads and writes drawings and calls it. Depends on nothing else in the
//! workspace.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

mod db;
pub mod store;

pub use db::{EcadDb, GraphStats};
#[cfg(not(target_arch = "wasm32"))]
pub use store::RedbStore;
pub use store::{MemStore, Store};

/// Errors from the data graph and its store.
#[derive(Debug, thiserror::Error)]
pub enum EcadError {
    #[error("database: {0}")]
    Store(String),
    #[error("stored record could not be read: {0}")]
    Codec(String),
    #[error("{0} not found")]
    NotFound(String),
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, EcadError>;

macro_rules! id_type {
    ($(#[$m:meta])* $name:ident, $prefix:literal) => {
        $(#[$m])*
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            /// A new, practically unique id (`D-` and 32 hex digits for a device), mixed from
            /// the process's random hashing keys, a counter and `seed`.
            pub fn new(seed: &str) -> Self {
                $name(format!("{}-{}", $prefix, fresh_hex(seed)))
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                $name(s.to_string())
            }
        }
    };
}

id_type!(
    /// A project.
    ProjectId,
    "P"
);
id_type!(
    /// A page (one drawing, one sheet).
    PageId,
    "G"
);
id_type!(
    /// A device: stored on its symbol as the hidden `EID` attribute.
    DeviceId,
    "D"
);
id_type!(
    /// A connection.
    ConnectionId,
    "C"
);

/// 32 hex digits, different on every call (no clock, no extra crates: wasm-safe).
fn fresh_hex(seed: &str) -> String {
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut parts = [0u64; 2];
    for (i, part) in parts.iter_mut().enumerate() {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write(seed.as_bytes());
        h.write_u64(n);
        h.write_usize(i);
        *part = h.finish();
    }
    format!("{:016x}{:016x}", parts[0], parts[1])
}

/// The project: its pages, metadata and registries of active devices and connections.
///
/// In the database the registries are tables of their own (devices and connections are read and
/// written one by one); [`EcadDb::project`] fills them in for a whole-project view.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EcadProject {
    pub id: ProjectId,
    pub name: String,
    pub description: String,
    /// Pages in sheet order.
    pub pages: Vec<EcadPage>,
    /// Free metadata: customer, job number, revision, author…
    pub metadata: BTreeMap<String, String>,
    /// Active devices, by id.
    pub devices: BTreeMap<DeviceId, EcadDevice>,
    /// Active connections, by id.
    pub connections: BTreeMap<ConnectionId, EcadConnection>,
    /// Bumped on every change, for change detection.
    pub revision: u64,
}

impl Default for ProjectId {
    fn default() -> Self {
        ProjectId(String::new())
    }
}

/// A page: one drawing (a DXF file), one sheet.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EcadPage {
    pub id: PageId,
    pub sheet: u32,
    /// The drawing file, relative to the project file's folder.
    pub drawing: String,
    pub title: String,
    pub description: String,
}

impl Default for PageId {
    fn default() -> Self {
        PageId(String::new())
    }
}

/// The IEC role of a device's symbol.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DeviceFamily {
    Coil,
    /// Normally open contact.
    No,
    /// Normally closed contact.
    Nc,
    #[default]
    Device,
    Terminal,
}

impl DeviceFamily {
    /// From an `EFAMILY` attribute value (`COIL`, `NO`, …); anything else is a plain device.
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_uppercase().as_str() {
            "COIL" => DeviceFamily::Coil,
            "NO" => DeviceFamily::No,
            "NC" => DeviceFamily::Nc,
            "TERMINAL" => DeviceFamily::Terminal,
            _ => DeviceFamily::Device,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            DeviceFamily::Coil => "COIL",
            DeviceFamily::No => "NO",
            DeviceFamily::Nc => "NC",
            DeviceFamily::Device => "DEVICE",
            DeviceFamily::Terminal => "TERMINAL",
        }
    }
}

/// A technical rating: rated voltage, current, power, coil voltage, IP rating…
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Rating {
    /// What is rated (`ratedVoltage`, `ratedCurrent`, `coilVoltage`, `power`, `ip`).
    pub name: String,
    pub value: f64,
    /// SI unit or note (`V`, `V AC`, `A`, `kW`, `mm²`).
    pub unit: String,
}

/// The part a device is: manufacturer data for ordering and the parts list.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PartSpec {
    pub manufacturer: String,
    /// Catalogue / order number.
    pub catalog: String,
    /// Up to three description lines (`DESC1`–`DESC3`).
    pub description: Vec<String>,
    pub ratings: Vec<Rating>,
}

/// Which way a wire leaves a pin (the AutoCAD Electrical `X?TERM` digit).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PinDirection {
    #[default]
    Up,
    Right,
    Down,
    Left,
}

impl PinDirection {
    /// From the digit of an `X?TERMnn` attribute: 1 up, 2 right, 4 down, 8 left.
    pub fn from_digit(d: u8) -> Option<Self> {
        match d {
            1 => Some(PinDirection::Up),
            2 => Some(PinDirection::Right),
            4 => Some(PinDirection::Down),
            8 => Some(PinDirection::Left),
            _ => None,
        }
    }
}

/// A pin (connection point) of a device.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EcadPin {
    /// The pin number printed on the device (`A1`, `13`, `U1`).
    pub number: String,
    pub direction: PinDirection,
    /// Where it is on its page, as last synchronised.
    pub position: [f64; 2],
}

/// The drawing object that shows a device: kept so either side can find the other.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GraphicRef {
    pub page: PageId,
    /// The block reference's handle in its drawing (a hint: copying changes it; the `EID`
    /// attribute on the symbol is what identifies the device).
    pub handle: u64,
    /// The block (symbol) name.
    pub block: String,
    /// Insertion point and rotation (radians) as last synchronised.
    pub position: [f64; 2],
    pub rotation: f64,
    /// Sheet and column of the symbol (`(1, 5)` is reference `105`).
    pub reference: (u32, u32),
}

/// A device: a piece of hardware, as the parts list and the wiring see it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EcadDevice {
    pub id: DeviceId,
    /// IEC 81346 reference designation (`-K105`).
    pub tag: String,
    /// IEC 81346-2 class letters (`K`, `QA`).
    pub class: String,
    pub family: DeviceFamily,
    /// A child component (a contact) carrying a main device's tag (`TAG2`).
    pub child: bool,
    /// For a child: the main device it belongs to, once known.
    pub parent: Option<DeviceId>,
    pub part: PartSpec,
    /// Location (`LOC`) and installation (`INST`).
    pub location: String,
    pub installation: String,
    pub pins: Vec<EcadPin>,
    /// The symbol that shows it.
    pub graphic: Option<GraphicRef>,
    /// Bumped on every change to the device.
    pub revision: u64,
}

impl Default for DeviceId {
    fn default() -> Self {
        DeviceId(String::new())
    }
}

impl EcadDevice {
    /// Whether this is a child component (a contact) carrying a main device's tag.
    pub fn is_child(&self) -> bool {
        self.child
    }
    pub fn pin(&self, number: &str) -> Option<&EcadPin> {
        self.pins.iter().find(|p| p.number == number)
    }
}

/// One end of a connection: a device's pin.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PinRef {
    pub device: DeviceId,
    pub pin: String,
}

impl PinRef {
    pub fn new(device: &DeviceId, pin: &str) -> Self {
        PinRef { device: device.clone(), pin: pin.to_string() }
    }
}

/// An edge of the graph: device A pin 1 wired to device B pin 4.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EcadConnection {
    pub id: ConnectionId,
    pub from: PinRef,
    pub to: PinRef,
    /// The wire number of the network it is part of (`105`).
    pub wire_number: Option<String>,
    /// The network (potential) it belongs to: connections with the same network are the same
    /// conductor.
    pub network: Option<String>,
    pub page: PageId,
    /// Handles of the wires that draw it.
    pub wires: Vec<u64>,
    /// Conductor data: colour, cross-section (mm²), type.
    pub color: String,
    pub cross_section: Option<f64>,
    pub wire_type: String,
}

impl Default for ConnectionId {
    fn default() -> Self {
        ConnectionId(String::new())
    }
}

impl EcadConnection {
    /// The other end of the connection from `pin`, if `pin` is one of its ends.
    pub fn other_end(&self, pin: &PinRef) -> Option<&PinRef> {
        if &self.from == pin {
            Some(&self.to)
        } else if &self.to == pin {
            Some(&self.from)
        } else {
            None
        }
    }
    /// Whether `device` is at either end.
    pub fn touches(&self, device: &DeviceId) -> bool {
        &self.from.device == device || &self.to.device == device
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_fresh_and_prefixed() {
        let a = DeviceId::new("x");
        let b = DeviceId::new("x");
        assert_ne!(a, b);
        assert!(a.as_str().starts_with("D-") && a.as_str().len() == 34);
        assert!(ConnectionId::new("").as_str().starts_with("C-"));
    }

    #[test]
    fn records_round_trip_as_json() {
        let d = EcadDevice {
            id: DeviceId::from("D-1"),
            tag: "-K105".into(),
            class: "K".into(),
            family: DeviceFamily::Coil,
            part: PartSpec {
                manufacturer: "Acme".into(),
                catalog: "R-24".into(),
                description: vec!["Motor contactor".into()],
                ratings: vec![Rating { name: "coilVoltage".into(), value: 24.0, unit: "V DC".into() }],
            },
            pins: vec![EcadPin { number: "A1".into(), direction: PinDirection::Up, position: [63.0, 105.0] }],
            ..Default::default()
        };
        let s = serde_json::to_string(&d).unwrap();
        assert!(s.contains("\"family\":\"COIL\"") && s.contains("\"tag\":\"-K105\""));
        let back: EcadDevice = serde_json::from_str(&s).unwrap();
        assert_eq!(back, d);
        // Old records missing newer fields still read.
        let old: EcadDevice = serde_json::from_str(r#"{"id": "D-2", "tag": "-S101"}"#).unwrap();
        assert_eq!(old.family, DeviceFamily::Device);
    }

    #[test]
    fn connection_ends() {
        let a = PinRef::new(&DeviceId::from("D-a"), "13");
        let b = PinRef::new(&DeviceId::from("D-b"), "A1");
        let c = EcadConnection { from: a.clone(), to: b.clone(), ..Default::default() };
        assert_eq!(c.other_end(&a), Some(&b));
        assert_eq!(c.other_end(&b), Some(&a));
        assert!(c.touches(&DeviceId::from("D-b")));
        assert_eq!(c.other_end(&PinRef::default()), None);
        assert_eq!(DeviceFamily::parse("nc"), DeviceFamily::Nc);
        assert_eq!(PinDirection::from_digit(8), Some(PinDirection::Left));
        assert_eq!(PinDirection::from_digit(3), None);
    }
}
