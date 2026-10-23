//! A spatial index of the symbols' connection points (pins) and outlines in the current space.
//!
//! Pins are filed in a uniform grid, so finding the pin under a click, the pin a wire ends on, or
//! the symbols a wire passes over costs a few cell lookups, not a walk over the drawing. The index
//! is built from the drawing when needed (it is cheap: one pass over the block references) and
//! never stored, so it cannot go stale.

use std::collections::HashMap;

use cadcraft_doc::{EntityKind, Handle, entity_bounds};
use cadcraft_ecad::PinDirection;
use cadcraft_geom::{Bounds2, Vec2};

use super::sync::pins_of;
use crate::{Result, Session};

/// Grid cell size: symbols' pins are 5–10 apart.
const CELL: f64 = 5.0;
/// Most cells searched around a point before falling back to a scan.
const MAX_RING: i64 = 64;

/// A pin of a symbol.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct PinRec {
    pub at: Vec2,
    /// Side wires leave from: 1 up, 2 right, 4 down, 8 left.
    pub dir: u8,
    /// The pin number (`A1`, `13`).
    pub number: String,
    /// The block reference it belongs to.
    pub insert: Handle,
}

/// A symbol: its outline and pins.
#[derive(Clone, Debug)]
pub(super) struct DeviceBox {
    pub insert: Handle,
    pub bounds: Bounds2,
    /// Indexes into [`PinIndex::pins`].
    pub pins: Vec<usize>,
}

#[derive(Debug, Default)]
pub(super) struct PinIndex {
    pub pins: Vec<PinRec>,
    pub devices: Vec<DeviceBox>,
    grid: HashMap<(i64, i64), Vec<usize>>,
}

fn cell_of(p: Vec2) -> Option<(i64, i64)> {
    let (x, y) = ((p.x / CELL).floor(), (p.y / CELL).floor());
    (x.is_finite() && y.is_finite() && x.abs() < 1e15 && y.abs() < 1e15).then_some((x as i64, y as i64))
}

impl PinIndex {
    /// The pins and outlines of every symbol (block reference with connection points) in the
    /// current space.
    pub fn build(s: &Session) -> Result<Self> {
        let d = s.doc()?;
        let mut ix = PinIndex::default();
        let Some(store) = d.space(&s.space()) else { return Ok(ix) };
        for e in store.iter() {
            let EntityKind::Insert(ins) = &e.kind else { continue };
            let pins = pins_of(ins);
            if pins.is_empty() {
                continue;
            }
            let mut own = Vec::with_capacity(pins.len());
            for p in &pins {
                let at = Vec2::new(p.position[0], p.position[1]);
                let Some(c) = cell_of(at) else { continue };
                let k = ix.pins.len();
                let dir = match p.direction {
                    PinDirection::Up => 1,
                    PinDirection::Right => 2,
                    PinDirection::Down => 4,
                    PinDirection::Left => 8,
                };
                ix.pins.push(PinRec { at, dir, number: p.number.clone(), insert: e.handle });
                ix.grid.entry(c).or_default().push(k);
                own.push(k);
            }
            let bounds = entity_bounds(d, e, 0);
            if !bounds.is_empty() && bounds.min.is_finite() && bounds.max.is_finite() {
                ix.devices.push(DeviceBox { insert: e.handle, bounds, pins: own });
            }
        }
        Ok(ix)
    }

    /// The pin nearest `p` within `reach`.
    pub fn nearest(&self, p: Vec2, reach: f64) -> Option<&PinRec> {
        let ring = (reach / CELL).ceil();
        let candidates: Box<dyn Iterator<Item = &PinRec>> = match cell_of(p) {
            Some((cx, cy)) if ring.is_finite() && (ring as i64) <= MAX_RING => {
                let r = ring as i64;
                Box::new(
                    (cx - r..=cx + r)
                        .flat_map(move |x| (cy - r..=cy + r).map(move |y| (x, y)))
                        .filter_map(|c| self.grid.get(&c))
                        .flatten()
                        .filter_map(|i| self.pins.get(*i)),
                )
            }
            _ => Box::new(self.pins.iter()),
        };
        candidates.map(|q| (q.at.dist(p), q)).filter(|(d, _)| *d <= reach).min_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, q)| q)
    }

    /// The pin exactly at `p` (within `tol`): a wire end that terminates on it.
    pub fn exact(&self, p: Vec2, tol: f64) -> Option<&PinRec> {
        self.nearest(p, tol)
    }

    /// The symbols whose outline the segment `a`–`b` passes through or touches.
    pub fn crossed(&self, a: Vec2, b: Vec2) -> Vec<&DeviceBox> {
        self.devices.iter().filter(|d| segment_hits_box(a, b, &d.bounds)).collect()
    }
}

/// Whether segment `a`–`b` meets box `bx` (Liang–Barsky clipping).
pub(super) fn segment_hits_box(a: Vec2, b: Vec2, bx: &Bounds2) -> bool {
    let d = b - a;
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [(-d.x, a.x - bx.min.x), (d.x, bx.max.x - a.x), (-d.y, a.y - bx.min.y), (d.y, bx.max.y - a.y)] {
        if p == 0.0 {
            if q < 0.0 {
                return false;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                t0 = t0.max(r);
            } else {
                t1 = t1.min(r);
            }
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn index_finds_pins_and_crossed_symbols() {
        let mut s = Session::new();
        s.execute("esymbol", &json!({ "name": "IEC_COIL", "at": [50, 100] })).unwrap();
        s.execute("esymbol", &json!({ "name": "IEC_MCB_3P", "at": [200, 100] })).unwrap();
        let ix = PinIndex::build(&s).unwrap();
        assert_eq!(ix.pins.len(), 2 + 6);
        assert_eq!(ix.devices.len(), 2);
        let p = ix.nearest(Vec2::new(51.0, 106.0), 3.0).unwrap();
        assert_eq!((p.number.as_str(), p.dir), ("A1", 1));
        assert!(ix.nearest(Vec2::new(51.0, 106.0), 0.5).is_none());
        assert_eq!(ix.exact(Vec2::new(205.0, 95.0), 1e-6).map(|p| p.number.as_str()), Some("6"));
        // A huge reach falls back to scanning everything.
        assert!(ix.nearest(Vec2::new(1e6, 1e6), 1e9).is_some());
        assert_eq!(ix.crossed(Vec2::new(50.0, 200.0), Vec2::new(50.0, 0.0)).len(), 1);
        assert_eq!(ix.crossed(Vec2::new(0.0, 100.0), Vec2::new(300.0, 100.0)).len(), 2);
        assert!(ix.crossed(Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)).is_empty());
    }

    #[test]
    fn box_clipping() {
        let b = Bounds2 { min: Vec2::new(0.0, 0.0), max: Vec2::new(10.0, 10.0) };
        assert!(segment_hits_box(Vec2::new(-5.0, 5.0), Vec2::new(15.0, 5.0), &b));
        assert!(segment_hits_box(Vec2::new(5.0, 5.0), Vec2::new(5.0, 6.0), &b), "inside");
        assert!(segment_hits_box(Vec2::new(10.0, 20.0), Vec2::new(10.0, 10.0), &b), "touching the edge");
        assert!(!segment_hits_box(Vec2::new(-5.0, 11.0), Vec2::new(15.0, 11.0), &b));
        assert!(!segment_hits_box(Vec2::new(11.0, -5.0), Vec2::new(11.0, 15.0), &b));
    }
}
