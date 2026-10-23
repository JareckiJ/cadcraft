//! The schematic symbol library and ESYMBOL, which places a symbol.
//!
//! Symbols are ordinary blocks carrying AutoCAD Electrical style attributes, so they work in any
//! CAD program and existing AutoCAD Electrical symbols work here:
//! - `TAG1` (a main device: coil, push-button, motor) or `TAG2` (a child carrying its main
//!   device's tag: a contact), `DESC1`–`DESC3`, `MFG`, `CAT`, `LOC`, `INST`, `XREF`;
//! - `TERM01`, `TERM02`, … the pin numbers shown next to the pins (`A1`, `13`);
//! - `X?TERMnn`, invisible, placed exactly on each connection point; the digit is the side a
//!   wire leaves from: 1 up, 2 right, 4 down, 8 left;
//! - `ECLASS` (IEC 81346-2 class letters) and `EFAMILY` (`COIL`, `NO`, `NC`, `DEVICE`,
//!   `TERMINAL`), invisible, for the tagging and cross-reference commands;
//! - `EID`, invisible: the device's id in the project database, linking symbol and record.
//!
//! The built-in shapes are our own drawings of the IEC 60617 symbol forms, drawn on a 2.5 grid
//! with the pins 10 apart, top and bottom, for vertical (top-to-bottom) ladders. Placing a symbol
//! on a wire cuts the wire between its pins, so each pin gets its own wire.

use std::f64::consts::PI;
use std::sync::Arc;

use cadcraft_doc::{Attrib, AttribModes, Block, Circle, Common, Entity, EntityKind, HAlign, Handle, Line, Text, VAlign};
use cadcraft_geom::{Vec2, Vec3};
use serde_json::{Map, Value, json};

use super::wires::wire_layer_matches;
use crate::cmd::{CommandSpec, bad, bool_or, f64_or, point_req, str_param};
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

/// Distance within which a pin counts as lying on a wire.
const ON_WIRE: f64 = 1e-6;

/// A drawing primitive of a symbol, in symbol coordinates.
#[derive(Clone, Copy, Debug)]
enum Prim {
    Line(f64, f64, f64, f64),
    Circle(f64, f64, f64),
    /// Centre, radius, start and end angle in degrees (counter-clockwise).
    Arc(f64, f64, f64, f64, f64),
    /// Centre, height, text.
    Label(f64, f64, f64, &'static str),
}

/// A connection point: where, which side wires leave from (1 up, 2 right, 4 down, 8 left), and
/// the pin number shown.
#[derive(Clone, Copy, Debug)]
struct Pin {
    x: f64,
    y: f64,
    dir: u8,
    number: &'static str,
}

/// A library symbol.
pub(super) struct SymbolDef {
    pub name: &'static str,
    pub description: &'static str,
    /// IEC 81346-2 class letters.
    pub class: &'static str,
    /// `COIL`, `NO`, `NC`, `DEVICE` or `TERMINAL`.
    pub family: &'static str,
    /// A child (contact) carries its main device's tag in `TAG2`.
    pub child: bool,
    prims: fn() -> Vec<Prim>,
    pins: &'static [Pin],
}

macro_rules! pins2 {
    ($a:expr, $b:expr) => {
        &[Pin { x: 0.0, y: 5.0, dir: 1, number: $a }, Pin { x: 0.0, y: -5.0, dir: 4, number: $b }]
    };
}

/// Leads from the pins to ±`inner`.
fn leads(inner: f64) -> Vec<Prim> {
    vec![Prim::Line(0.0, 5.0, 0.0, inner), Prim::Line(0.0, -5.0, 0.0, -inner)]
}

/// A normally open contact: the moving blade from the lower contact leaning left, short of the
/// upper one.
fn no_contact() -> Vec<Prim> {
    let mut v = leads(2.5);
    v.push(Prim::Line(0.0, -2.5, -2.0, 2.5));
    v
}

/// A normally closed contact: the blade reaches past a stub on the upper contact.
fn nc_contact() -> Vec<Prim> {
    let mut v = leads(2.5);
    v.push(Prim::Line(0.0, 2.5, 1.5, 2.5));
    v.push(Prim::Line(0.0, -2.5, 1.8, 3.0));
    v
}

/// A push-button actuator left of a contact: a dashed link to the blade and the button head.
fn push_actuator(blade_x: f64) -> Vec<Prim> {
    vec![
        Prim::Line(blade_x, 0.0, blade_x - 1.0, 0.0),
        Prim::Line(blade_x - 2.0, 0.0, blade_x - 3.0, 0.0),
        Prim::Line(-5.0, 0.0, -4.0, 0.0),
        Prim::Line(-5.0, -1.5, -5.0, 1.5),
        Prim::Line(-5.0, 1.5, -5.75, 1.5),
        Prim::Line(-5.0, -1.5, -5.75, -1.5),
    ]
}

fn coil() -> Vec<Prim> {
    let mut v = leads(3.0);
    v.extend([
        Prim::Line(-4.0, -3.0, 4.0, -3.0),
        Prim::Line(4.0, -3.0, 4.0, 3.0),
        Prim::Line(4.0, 3.0, -4.0, 3.0),
        Prim::Line(-4.0, 3.0, -4.0, -3.0),
    ]);
    v
}

fn pb_no() -> Vec<Prim> {
    let mut v = no_contact();
    v.extend(push_actuator(-1.0));
    v
}

fn pb_nc() -> Vec<Prim> {
    let mut v = nc_contact();
    v.extend(push_actuator(0.9));
    v
}

fn estop() -> Vec<Prim> {
    let mut v = nc_contact();
    v.extend([
        Prim::Line(0.9, 0.0, -0.1, 0.0),
        Prim::Line(-1.1, 0.0, -2.1, 0.0),
        Prim::Line(-3.1, 0.0, -4.0, 0.0),
        // Mushroom head.
        Prim::Arc(-4.0, 0.0, 2.0, 90.0, 270.0),
        Prim::Line(-4.0, -2.0, -4.0, 2.0),
    ]);
    v
}

fn selector() -> Vec<Prim> {
    let mut v = no_contact();
    v.extend([
        Prim::Line(-1.0, 0.0, -2.0, 0.0),
        Prim::Line(-3.0, 0.0, -4.0, 0.0),
        // Turn actuator: a step.
        Prim::Line(-4.0, -1.5, -4.0, 0.0),
        Prim::Line(-4.0, 0.0, -5.5, 0.0),
        Prim::Line(-5.5, 0.0, -5.5, 1.5),
    ]);
    v
}

fn lamp() -> Vec<Prim> {
    let mut v = leads(3.0);
    let d = 3.0 * std::f64::consts::FRAC_1_SQRT_2;
    v.extend([Prim::Circle(0.0, 0.0, 3.0), Prim::Line(-d, -d, d, d), Prim::Line(-d, d, d, -d)]);
    v
}

fn fuse() -> Vec<Prim> {
    let mut v = leads(3.5);
    v.extend([
        Prim::Line(-1.25, -3.5, 1.25, -3.5),
        Prim::Line(1.25, -3.5, 1.25, 3.5),
        Prim::Line(1.25, 3.5, -1.25, 3.5),
        Prim::Line(-1.25, 3.5, -1.25, -3.5),
        Prim::Line(0.0, -3.5, 0.0, 3.5),
    ]);
    v
}

fn breaker() -> Vec<Prim> {
    let mut v = no_contact();
    // The cross on the fixed contact marks a circuit-breaker.
    v.extend([Prim::Line(-0.8, 1.7, 0.8, 3.3), Prim::Line(-0.8, 3.3, 0.8, 1.7)]);
    v
}

fn breaker_3p() -> Vec<Prim> {
    let mut v = Vec::new();
    for x in [-5.0, 0.0, 5.0] {
        v.extend([
            Prim::Line(x, 5.0, x, 2.5),
            Prim::Line(x, -5.0, x, -2.5),
            Prim::Line(x, -2.5, x - 2.0, 2.5),
            Prim::Line(x - 0.8, 1.7, x + 0.8, 3.3),
            Prim::Line(x - 0.8, 3.3, x + 0.8, 1.7),
        ]);
    }
    // Mechanical link (dashed).
    v.extend([Prim::Line(-6.0, 0.0, -5.0, 0.0), Prim::Line(-4.0, 0.0, -2.0, 0.0), Prim::Line(1.0, 0.0, 3.0, 0.0)]);
    v
}

fn contacts_3p() -> Vec<Prim> {
    let mut v = Vec::new();
    for x in [-5.0, 0.0, 5.0] {
        v.extend([Prim::Line(x, 5.0, x, 2.5), Prim::Line(x, -5.0, x, -2.5), Prim::Line(x, -2.5, x - 2.0, 2.5)]);
    }
    v.extend([Prim::Line(-6.0, 0.0, -5.0, 0.0), Prim::Line(-4.0, 0.0, -2.0, 0.0), Prim::Line(1.0, 0.0, 3.0, 0.0)]);
    v
}

fn overload_3p() -> Vec<Prim> {
    let mut v = Vec::new();
    for x in [-5.0, 0.0, 5.0] {
        // Thermal element: a step in the conductor.
        v.extend([
            Prim::Line(x, 5.0, x, 1.5),
            Prim::Line(x, 1.5, x - 1.25, 1.5),
            Prim::Line(x - 1.25, 1.5, x - 1.25, -1.5),
            Prim::Line(x - 1.25, -1.5, x, -1.5),
            Prim::Line(x, -1.5, x, -5.0),
        ]);
    }
    v.extend([
        Prim::Line(-7.0, -3.0, 7.0, -3.0),
        Prim::Line(7.0, -3.0, 7.0, 3.0),
        Prim::Line(7.0, 3.0, -7.0, 3.0),
        Prim::Line(-7.0, 3.0, -7.0, -3.0),
    ]);
    v
}

fn motor_3ph() -> Vec<Prim> {
    vec![
        Prim::Line(-5.0, 5.0, -2.5, 3.0),
        Prim::Line(0.0, 5.0, 0.0, 5.0 - 1.0),
        Prim::Line(5.0, 5.0, 2.5, 3.0),
        Prim::Circle(0.0, 0.0, 4.0),
        Prim::Label(0.0, 0.8, 2.5, "M"),
        Prim::Label(0.0, -1.8, 1.8, "3~"),
    ]
}

fn terminal() -> Vec<Prim> {
    vec![Prim::Line(0.0, 5.0, 0.0, 1.0), Prim::Line(0.0, -5.0, 0.0, -1.0), Prim::Circle(0.0, 0.0, 1.0)]
}

fn psu() -> Vec<Prim> {
    vec![
        Prim::Line(-5.0, 5.0, -5.0, 3.0),
        Prim::Line(5.0, 5.0, 5.0, 3.0),
        Prim::Line(-5.0, -5.0, -5.0, -3.0),
        Prim::Line(5.0, -5.0, 5.0, -3.0),
        Prim::Line(-7.0, -3.0, 7.0, -3.0),
        Prim::Line(7.0, -3.0, 7.0, 3.0),
        Prim::Line(7.0, 3.0, -7.0, 3.0),
        Prim::Line(-7.0, 3.0, -7.0, -3.0),
        Prim::Line(-7.0, -3.0, 7.0, 3.0),
        Prim::Label(-3.5, 1.2, 2.0, "~"),
        Prim::Label(3.5, -1.5, 2.0, "="),
    ]
}

fn ground() -> Vec<Prim> {
    vec![Prim::Line(0.0, 5.0, 0.0, 0.0), Prim::Line(-3.0, 0.0, 3.0, 0.0), Prim::Line(-2.0, -1.0, 2.0, -1.0), Prim::Line(-1.0, -2.0, 1.0, -2.0)]
}

/// Every built-in symbol.
pub(super) fn library() -> &'static [SymbolDef] {
    const L: &[SymbolDef] = &[
        SymbolDef { name: "IEC_COIL", description: "Relay coil", class: "K", family: "COIL", child: false, prims: coil, pins: pins2!("A1", "A2") },
        SymbolDef {
            name: "IEC_CONTACTOR_COIL",
            description: "Contactor coil",
            class: "Q",
            family: "COIL",
            child: false,
            prims: coil,
            pins: pins2!("A1", "A2"),
        },
        SymbolDef {
            name: "IEC_NO",
            description: "Normally open contact",
            class: "K",
            family: "NO",
            child: true,
            prims: no_contact,
            pins: pins2!("13", "14"),
        },
        SymbolDef {
            name: "IEC_NC",
            description: "Normally closed contact",
            class: "K",
            family: "NC",
            child: true,
            prims: nc_contact,
            pins: pins2!("21", "22"),
        },
        SymbolDef {
            name: "IEC_MAIN_CONTACTS_3P",
            description: "Contactor main contacts, 3 poles",
            class: "Q",
            family: "NO",
            child: true,
            prims: contacts_3p,
            pins: &[
                Pin { x: -5.0, y: 5.0, dir: 1, number: "1" },
                Pin { x: 0.0, y: 5.0, dir: 1, number: "3" },
                Pin { x: 5.0, y: 5.0, dir: 1, number: "5" },
                Pin { x: -5.0, y: -5.0, dir: 4, number: "2" },
                Pin { x: 0.0, y: -5.0, dir: 4, number: "4" },
                Pin { x: 5.0, y: -5.0, dir: 4, number: "6" },
            ],
        },
        SymbolDef {
            name: "IEC_PB_NO",
            description: "Push-button, normally open",
            class: "S",
            family: "NO",
            child: false,
            prims: pb_no,
            pins: pins2!("13", "14"),
        },
        SymbolDef {
            name: "IEC_PB_NC",
            description: "Push-button, normally closed",
            class: "S",
            family: "NC",
            child: false,
            prims: pb_nc,
            pins: pins2!("11", "12"),
        },
        SymbolDef {
            name: "IEC_ESTOP",
            description: "Emergency stop, normally closed",
            class: "S",
            family: "NC",
            child: false,
            prims: estop,
            pins: pins2!("11", "12"),
        },
        SymbolDef {
            name: "IEC_SELECTOR",
            description: "Selector switch, normally open",
            class: "S",
            family: "NO",
            child: false,
            prims: selector,
            pins: pins2!("13", "14"),
        },
        SymbolDef {
            name: "IEC_LAMP",
            description: "Indicator lamp",
            class: "P",
            family: "DEVICE",
            child: false,
            prims: lamp,
            pins: pins2!("X1", "X2"),
        },
        SymbolDef { name: "IEC_FUSE", description: "Fuse", class: "F", family: "DEVICE", child: false, prims: fuse, pins: pins2!("1", "2") },
        SymbolDef {
            name: "IEC_MCB_1P",
            description: "Circuit-breaker, 1 pole",
            class: "Q",
            family: "DEVICE",
            child: false,
            prims: breaker,
            pins: pins2!("1", "2"),
        },
        SymbolDef {
            name: "IEC_MCB_3P",
            description: "Circuit-breaker, 3 poles",
            class: "Q",
            family: "DEVICE",
            child: false,
            prims: breaker_3p,
            pins: &[
                Pin { x: -5.0, y: 5.0, dir: 1, number: "1" },
                Pin { x: 0.0, y: 5.0, dir: 1, number: "3" },
                Pin { x: 5.0, y: 5.0, dir: 1, number: "5" },
                Pin { x: -5.0, y: -5.0, dir: 4, number: "2" },
                Pin { x: 0.0, y: -5.0, dir: 4, number: "4" },
                Pin { x: 5.0, y: -5.0, dir: 4, number: "6" },
            ],
        },
        SymbolDef {
            name: "IEC_OVERLOAD_3P",
            description: "Thermal overload relay, 3 poles",
            class: "F",
            family: "DEVICE",
            child: false,
            prims: overload_3p,
            pins: &[
                Pin { x: -5.0, y: 5.0, dir: 1, number: "1" },
                Pin { x: 0.0, y: 5.0, dir: 1, number: "3" },
                Pin { x: 5.0, y: 5.0, dir: 1, number: "5" },
                Pin { x: -5.0, y: -5.0, dir: 4, number: "2" },
                Pin { x: 0.0, y: -5.0, dir: 4, number: "4" },
                Pin { x: 5.0, y: -5.0, dir: 4, number: "6" },
            ],
        },
        SymbolDef {
            name: "IEC_OVERLOAD_NC",
            description: "Overload contact, normally closed",
            class: "F",
            family: "NC",
            child: true,
            prims: nc_contact,
            pins: pins2!("95", "96"),
        },
        SymbolDef {
            name: "IEC_MOTOR_3PH",
            description: "Three-phase motor",
            class: "M",
            family: "DEVICE",
            child: false,
            prims: motor_3ph,
            pins: &[
                Pin { x: -5.0, y: 5.0, dir: 1, number: "U1" },
                Pin { x: 0.0, y: 5.0, dir: 1, number: "V1" },
                Pin { x: 5.0, y: 5.0, dir: 1, number: "W1" },
            ],
        },
        SymbolDef {
            name: "IEC_TERMINAL",
            description: "Terminal",
            class: "X",
            family: "TERMINAL",
            child: false,
            prims: terminal,
            pins: pins2!("1", "1"),
        },
        SymbolDef {
            name: "IEC_PSU",
            description: "Power supply, AC/DC",
            class: "T",
            family: "DEVICE",
            child: false,
            prims: psu,
            pins: &[
                Pin { x: -5.0, y: 5.0, dir: 1, number: "L" },
                Pin { x: 5.0, y: 5.0, dir: 1, number: "N" },
                Pin { x: -5.0, y: -5.0, dir: 4, number: "+" },
                Pin { x: 5.0, y: -5.0, dir: 4, number: "-" },
            ],
        },
        SymbolDef {
            name: "IEC_PE",
            description: "Protective earth",
            class: "X",
            family: "TERMINAL",
            child: false,
            prims: ground,
            pins: &[Pin { x: 0.0, y: 5.0, dir: 1, number: "PE" }],
        },
    ];
    L
}

/// The library symbol called `name` (any case).
impl SymbolDef {
    /// Its pins: (x, y) in symbol coordinates, direction digit, pin number.
    pub fn pin_list(&self) -> Vec<(f64, f64, u8, &'static str)> {
        self.pins.iter().map(|p| (p.x, p.y, p.dir, p.number)).collect()
    }
}

pub(super) fn find(name: &str) -> Option<&'static SymbolDef> {
    library().iter().find(|d| d.name.eq_ignore_ascii_case(name))
}

fn text(at: Vec2, height: f64, value: &str, halign: HAlign, valign: VAlign) -> Text {
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
        halign,
        valign,
    }
}

/// An attribute definition.
fn attdef(tag: &str, at: Vec2, height: f64, value: &str, invisible: bool, halign: HAlign) -> EntityKind {
    EntityKind::AttDef(Attrib {
        tag: tag.to_string(),
        text: text(at, height, value, halign, VAlign::Middle),
        invisible,
        constant: false,
        prompt: tag.to_string(),
        props: Default::default(),
        multiline: None,
        modes: AttribModes::default(),
    })
}

/// The attribute definitions every symbol carries, for a symbol whose drawing reaches `right` on
/// the right: tag, descriptions and the rest beside it, pin numbers and connection points at the
/// pins.
pub(super) fn standard_attdefs(child: bool, class: &str, family: &str, right: f64, pins: &[(Vec2, u8, String)]) -> Vec<EntityKind> {
    let x = right + 1.5;
    let mut v = vec![
        attdef(if child { "TAG2" } else { "TAG1" }, Vec2::new(x, 1.5), 2.5, "", false, HAlign::Left),
        attdef("DESC1", Vec2::new(x, -1.5), 1.8, "", false, HAlign::Left),
        attdef("DESC2", Vec2::new(x, -3.5), 1.8, "", true, HAlign::Left),
        attdef("DESC3", Vec2::new(x, -5.5), 1.8, "", true, HAlign::Left),
        attdef("XREF", Vec2::new(x, -3.5), 1.8, "", false, HAlign::Left),
        attdef("MFG", Vec2::new(x, -7.5), 1.8, "", true, HAlign::Left),
        attdef("CAT", Vec2::new(x, -9.5), 1.8, "", true, HAlign::Left),
        attdef("LOC", Vec2::new(x, -11.5), 1.8, "", true, HAlign::Left),
        attdef("INST", Vec2::new(x, -13.5), 1.8, "", true, HAlign::Left),
        attdef("ECLASS", Vec2::new(x, -15.5), 1.8, class, true, HAlign::Left),
        attdef("EFAMILY", Vec2::new(x, -17.5), 1.8, family, true, HAlign::Left),
        // The device's id in the project database (filled in by EDB.SYNC).
        attdef("EID", Vec2::new(x, -19.5), 1.0, "", true, HAlign::Left),
    ];
    for (i, (p, dir, number)) in pins.iter().enumerate() {
        let n = i + 1;
        // Pin numbers sit just left of the pin, inside the lead.
        let label_at = match dir {
            1 => Vec2::new(p.x - 0.75, p.y - 1.25),
            4 => Vec2::new(p.x - 0.75, p.y + 1.25),
            2 => Vec2::new(p.x - 1.25, p.y + 1.0),
            _ => Vec2::new(p.x + 1.25, p.y + 1.0),
        };
        v.push(attdef(&format!("TERM{n:02}"), label_at, 1.5, number, false, HAlign::Right));
        // Connection points: left-baseline, so the insertion point is the point itself.
        let mut cp = attdef(&format!("X{dir}TERM{n:02}"), *p, 1.0, "", true, HAlign::Left);
        if let EntityKind::AttDef(a) = &mut cp {
            a.text.align_pt = None;
            a.text.valign = VAlign::Baseline;
        }
        v.push(cp);
    }
    v
}

/// The block for library symbol `def`, its entities numbered by `new_handle`.
fn build_block(def: &SymbolDef, mut new_handle: impl FnMut() -> Handle) -> Block {
    let mut b = Block::new(def.name);
    b.description = def.description.to_string();
    let mut right: f64 = 0.0;
    let mut kinds: Vec<EntityKind> = Vec::new();
    for p in (def.prims)() {
        match p {
            Prim::Line(x1, y1, x2, y2) => {
                right = right.max(x1).max(x2);
                kinds.push(EntityKind::Line(Line { a: Vec3::new(x1, y1, 0.0), b: Vec3::new(x2, y2, 0.0) }));
            }
            Prim::Circle(x, y, r) => {
                right = right.max(x + r);
                kinds.push(EntityKind::Circle(Circle { center: Vec3::new(x, y, 0.0), radius: r }));
            }
            Prim::Arc(x, y, r, a0, a1) => {
                right = right.max(x + r);
                kinds.push(EntityKind::Arc(cadcraft_doc::Arc {
                    center: Vec3::new(x, y, 0.0),
                    radius: r,
                    start: a0 * PI / 180.0,
                    end: a1 * PI / 180.0,
                }));
            }
            Prim::Label(x, y, h, t) => kinds.push(EntityKind::Text(text(Vec2::new(x, y), h, t, HAlign::Center, VAlign::Middle))),
        }
    }
    let pins: Vec<(Vec2, u8, String)> = def.pins.iter().map(|p| (Vec2::new(p.x, p.y), p.dir, p.number.to_string())).collect();
    kinds.extend(standard_attdefs(def.child, def.class, def.family, right, &pins));
    for k in kinds {
        b.entities.push(Entity { handle: new_handle(), common: Common::default(), kind: k });
    }
    b
}

/// Make sure block `name` exists in the drawing, adding it from the library when it is a
/// library symbol. Returns the block's name as defined.
pub(super) fn ensure_block(s: &mut Session, name: &str) -> Result<String> {
    if let Some(b) = s.doc()?.block(name) {
        return Ok(b.name.clone());
    }
    let def = find(name).ok_or_else(|| EngineError::Other(format!("No symbol \"{name}\" in the drawing or the library.")))?;
    let d = s.doc_mut()?;
    let b = build_block(def, || d.new_handle());
    d.blocks.insert(def.name.to_string(), Arc::new(b));
    Ok(def.name.to_string())
}

/// The connection points of block reference `h`: (world point, direction digit).
pub(super) fn connection_points(s: &Session, h: Handle) -> Vec<(Vec2, u8)> {
    let Ok(d) = s.doc() else { return Vec::new() };
    let Some(EntityKind::Insert(ins)) = d.entity(h).map(|e| &e.kind) else { return Vec::new() };
    insert_connection_points(ins)
}

/// The connection points of a block reference: its `X?TERMnn` attributes.
pub(super) fn insert_connection_points(ins: &cadcraft_doc::Insert) -> Vec<(Vec2, u8)> {
    ins.attribs.iter().filter_map(|a| connection_dir(&a.tag).map(|dir| (a.text.justify_point(), dir))).filter(|(p, _)| p.is_finite()).collect()
}

/// The direction digit of a connection point attribute tag (`X4TERM02` → 4).
pub(super) fn connection_dir(tag: &str) -> Option<u8> {
    let t = tag.to_ascii_uppercase();
    let rest = t.strip_prefix('X')?;
    let (d, tail) = rest.split_at_checked(1)?;
    let dir: u8 = d.parse().ok()?;
    (tail.starts_with("TERM") && matches!(dir, 1 | 2 | 4 | 8)).then_some(dir)
}

/// Cut every wire that runs through two or more of `pins`: the part between the outermost pins
/// on it goes, so each pin ends a wire. Returns how many wires were cut.
pub(super) fn break_wires(s: &mut Session, pins: &[Vec2], layers: &str) -> Result<usize> {
    let space = s.space();
    let d = s.doc()?;
    let Some(store) = d.space(&space) else { return Ok(0) };
    // (handle, segment index, the cut: from the pin nearest the start to the farthest).
    let mut cuts: Vec<(Handle, usize, Vec2, Vec2)> = Vec::new();
    for e in store.iter().filter(|e| wire_layer_matches(layers, &e.common.layer)) {
        let pts: Vec<Vec2> = match &e.kind {
            EntityKind::Line(l) => vec![l.a.xy(), l.b.xy()],
            EntityKind::LwPolyline(pl) if !pl.closed => pl.vertices.iter().map(|v| v.p).collect(),
            _ => continue,
        };
        for (i, w) in pts.windows(2).enumerate() {
            let [a, b] = w else { continue };
            let len2 = (*b - *a).len2();
            if len2.is_nan() || len2 <= 0.0 {
                continue;
            }
            let mut on: Vec<f64> = pins
                .iter()
                .filter_map(|p| {
                    let t = (*p - *a).dot(*b - *a) / len2;
                    let foot = a.lerp(*b, t);
                    (t > 1e-9 && t < 1.0 - 1e-9 && foot.dist(*p) <= ON_WIRE.max(len2.sqrt() * 1e-12)).then_some(t)
                })
                .collect();
            if on.len() >= 2 {
                on.sort_by(f64::total_cmp);
                let (t0, t1) = (on.first().copied().unwrap_or(0.0), on.last().copied().unwrap_or(1.0));
                cuts.push((e.handle, i, a.lerp(*b, t0), a.lerp(*b, t1)));
                break;
            }
        }
    }
    let n = cuts.len();
    for (h, i, p0, p1) in cuts {
        let Some(e) = s.doc()?.entity(h).map(|e| (**e).clone()) else { continue };
        let second = match &e.kind {
            EntityKind::Line(l) => {
                let tail = EntityKind::Line(Line { a: Vec3::new(p1.x, p1.y, l.a.z), b: l.b });
                s.doc_mut()?.modify_entity(h, |e| {
                    if let EntityKind::Line(l) = &mut e.kind {
                        l.b = Vec3::new(p0.x, p0.y, l.b.z);
                    }
                })?;
                tail
            }
            EntityKind::LwPolyline(pl) => {
                let mut head = pl.clone();
                let mut tail = pl.clone();
                let split = i + 1;
                head.vertices.truncate(split);
                let mut v0 = head.vertices.last().copied().unwrap_or_default();
                v0.p = p0;
                v0.bulge = 0.0;
                head.vertices.push(v0);
                if let Some(last) = head.vertices.get_mut(split.saturating_sub(1)) {
                    last.bulge = 0.0;
                }
                let mut v1 = v0;
                v1.p = p1;
                tail.vertices = std::iter::once(v1).chain(pl.vertices.iter().skip(split).copied()).collect();
                s.doc_mut()?.modify_entity(h, |e| e.kind = EntityKind::LwPolyline(head))?;
                EntityKind::LwPolyline(tail)
            }
            _ => continue,
        };
        s.doc_mut()?.add(&space, e.common.clone(), second)?;
    }
    Ok(n)
}

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("esymbol", "Insert Symbol", run_esymbol)
            .menu(&["Tools", "Electrical", "Insert Symbol"])
            .params(
                "{name (library symbol such as IEC_COIL, or any block), at: [x, y], rotation? (degrees), values? {DESC1: \"...\"}, \
                 tag? (default: tagged automatically for main devices), breakWires? (default true), layers? (wire layers, default WIRE*)} \
                 → {handle, block, tag, wiresCut, pins: [[x, y]]} · {list: true} → {symbols: [{name, description, class, family, child}]}",
            )
            .interactive(|_| Ok(Box::new(EsymbolM::default()))),
    ]
}

pub(super) fn run_esymbol(s: &mut Session, p: &Value) -> Result<Value> {
    let r = run_esymbol_inner(s, p)?;
    // Keep the project database in step with the drawing.
    super::sync::auto(s);
    Ok(r)
}

fn run_esymbol_inner(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "esymbol";
    if bool_or(p, "list", false) {
        let list: Vec<Value> = library()
            .iter()
            .map(|d| json!({ "name": d.name, "description": d.description, "class": d.class, "family": d.family, "child": d.child }))
            .collect();
        return Ok(json!({ "symbols": list }));
    }
    let name = str_param(p, "name").map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad(CMD, "`name` is required"))?;
    let at = point_req(CMD, p, "at")?;
    let rotation = f64_or(p, "rotation", 0.0);
    if rotation.abs() > 1e6 {
        return Err(bad(CMD, "`rotation` is out of range"));
    }
    let layers = str_param(p, "layers").map(str::trim).filter(|l| !l.is_empty()).unwrap_or(super::wires::WIRE_LAYERS).to_string();
    if layers.chars().count() > 1024 {
        return Err(bad(CMD, "`layers` is too long"));
    }
    let mut values: Map<String, Value> = p.get("values").and_then(Value::as_object).cloned().unwrap_or_default();
    if let Some(t) = str_param(p, "tag") {
        values.insert("TAG1".into(), json!(t));
        values.insert("TAG2".into(), json!(t));
    }
    let block = ensure_block(s, name)?;
    let h = crate::cmd::blocks::insert(s, &block, at, 1.0, rotation.to_radians(), &values)?;
    let pins: Vec<Vec2> = connection_points(s, h).into_iter().map(|(p, _)| p).collect();
    let cut = if bool_or(p, "breakWires", true) { break_wires(s, &pins, &layers)? } else { 0 };
    // Main devices get their tag straight away, like AutoCAD Electrical's insert-and-tag.
    let has_tag1 =
        matches!(s.doc()?.entity(h).map(|e| &e.kind), Some(EntityKind::Insert(i)) if i.attribs.iter().any(|a| a.tag.eq_ignore_ascii_case("TAG1")));
    if has_tag1 && str_param(p, "tag").is_none() {
        super::run_etag(s, &json!({ "handles": [h.hex()] }))?;
    }
    let tag = match s.doc()?.entity(h).map(|e| &e.kind) {
        Some(EntityKind::Insert(i)) => {
            i.attribs.iter().find(|a| a.tag.eq_ignore_ascii_case("TAG1") || a.tag.eq_ignore_ascii_case("TAG2")).map(|a| a.text.value.clone())
        }
        _ => None,
    };
    let pins_json: Vec<Value> = pins.iter().map(|p| json!([p.x, p.y])).collect();
    Ok(json!({ "handle": h.hex(), "block": block, "tag": tag, "wiresCut": cut, "pins": pins_json }))
}

/// ESYMBOL at the command line: the symbol's name, then where it goes.
#[derive(Default)]
struct EsymbolM {
    name: Option<String>,
}

impl Interactive for EsymbolM {
    fn name(&self) -> &'static str {
        "ESYMBOL"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match &self.name {
            None => {
                Prompt::new("Symbol name or [?]", Accept { point: false, number: false, text: true, select: false, enter: true }).default("IEC_COIL")
            }
            Some(n) => {
                Prompt::new(format!("Insertion point for {n}"), Accept { point: true, number: false, text: false, select: false, enter: false })
            }
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match (&self.name, i) {
            (None, Input::Text(t) | Input::Keyword(t)) if t.trim() == "?" => {
                let names: Vec<&str> = library().iter().map(|d| d.name).collect();
                s.echo(format!("Symbols: {}", names.join(", ")));
                Ok(Step::Continue)
            }
            (None, Input::Text(t) | Input::Keyword(t)) => {
                let t = t.trim();
                let name = if t.is_empty() { "IEC_COIL" } else { t };
                if find(name).is_none() && s.doc()?.block(name).is_none() {
                    return Err(EngineError::Other(format!("No symbol \"{name}\"; type ? for the list.")));
                }
                self.name = Some(name.to_string());
                Ok(Step::Continue)
            }
            (None, Input::Enter) => {
                self.name = Some("IEC_COIL".into());
                Ok(Step::Continue)
            }
            (Some(n), Input::Point(p)) => {
                let n = n.clone();
                run_esymbol(s, &json!({ "name": n, "at": [p.x, p.y] }))?;
                Ok(Step::Done)
            }
            _ => Ok(Step::Continue),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn wire_count(s: &Session) -> usize {
        s.doc().unwrap().model.iter().filter(|e| e.common.layer == "WIRE").count()
    }

    #[test]
    fn every_library_symbol_builds_with_its_attributes() {
        let mut s = Session::new();
        for (i, def) in library().iter().enumerate() {
            let r = s.execute("esymbol", &json!({ "name": def.name, "at": [20.0 * i as f64, 100.0] })).unwrap();
            assert_eq!(r["pins"].as_array().unwrap().len(), def.pins.len(), "{}", def.name);
            let h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
            let Some(EntityKind::Insert(ins)) = s.doc().unwrap().entity(h).map(|e| &e.kind) else { panic!() };
            let has = |t: &str| ins.attribs.iter().any(|a| a.tag == t);
            assert!(has(if def.child { "TAG2" } else { "TAG1" }), "{}", def.name);
            assert!(has("DESC1") && has("XREF") && has("ECLASS") && has("TERM01"), "{}", def.name);
            if !def.child {
                assert!(r["tag"].as_str().unwrap().starts_with(&format!("-{}", def.class)), "{}: {r}", def.name);
            }
        }
    }

    #[test]
    fn inserting_on_a_wire_cuts_it_between_the_pins() {
        let mut s = Session::new();
        s.execute("layer.new", &json!({ "name": "WIRE", "current": true })).unwrap();
        s.execute("line", &json!({ "points": [[50, 200], [50, 20]] })).unwrap();
        s.execute("pline", &json!({ "points": [[100, 200], [100, 20], [150, 20]] })).unwrap();
        s.execute("layer.current", &json!({ "name": "0" })).unwrap();
        let r = s.execute("esymbol", &json!({ "name": "IEC_COIL", "at": [50, 100] })).unwrap();
        assert_eq!(r["wiresCut"], 1);
        assert_eq!(r["tag"], "-K102");
        let r = s.execute("esymbol", &json!({ "name": "IEC_PB_NO", "at": [100, 150] })).unwrap();
        assert_eq!(r["wiresCut"], 1);
        assert_eq!(wire_count(&s), 4);
        // Each pin now ends a wire: the line stops at 105 and restarts at 95.
        let ends: Vec<(f64, f64)> = s
            .doc()
            .unwrap()
            .model
            .iter()
            .filter_map(|e| match &e.kind {
                EntityKind::Line(l) => Some((l.a.y, l.b.y)),
                _ => None,
            })
            .collect();
        assert!(ends.contains(&(200.0, 105.0)) && ends.contains(&(95.0, 20.0)), "{ends:?}");
        // Wire numbering now sees four networks.
        let w = s.execute("ewirenum", &json!({})).unwrap();
        assert_eq!(w["potentials"], 4);
    }

    #[test]
    fn rotated_symbol_pins_and_bad_params() {
        let mut s = Session::new();
        let r = s.execute("esymbol", &json!({ "name": "IEC_LAMP", "at": [0, 0], "rotation": 90 })).unwrap();
        let pins = r["pins"].as_array().unwrap();
        let x0 = pins[0][0].as_f64().unwrap();
        assert!((x0 + 5.0).abs() < 1e-9, "top pin turns to the left: {pins:?}");
        assert!(s.execute("esymbol", &json!({ "name": "NOPE", "at": [0, 0] })).is_err());
        assert!(s.execute("esymbol", &json!({ "name": "IEC_LAMP" })).is_err());
        assert!(s.execute("esymbol", &json!({ "name": "IEC_LAMP", "at": [0, 0], "rotation": 1e300 })).is_err());
        assert_eq!(s.execute("esymbol", &json!({ "list": true })).unwrap()["symbols"].as_array().unwrap().len(), library().len());
        assert_eq!(connection_dir("X4TERM02"), Some(4));
        assert_eq!(connection_dir("x1term01"), Some(1));
        assert_eq!(connection_dir("X3TERM01"), None);
        assert_eq!(connection_dir("XTERM"), None);
        assert_eq!(connection_dir("TERM01"), None);
    }

    #[test]
    fn command_line_form() {
        let mut s = Session::new();
        s.cmdline("esymbol").unwrap();
        s.cmdline("IEC_FUSE").unwrap();
        s.input(Input::Point(Vec2::new(10.0, 10.0))).unwrap();
        assert!(s.running.is_none());
        assert!(s.doc().unwrap().block("IEC_FUSE").is_some());
    }
}
