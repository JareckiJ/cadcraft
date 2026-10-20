//! Associative arrays: the parameters an array keeps, and where they place its items.
//!
//! An associative array is a block reference to an anonymous block (`*U…`, the *array block*)
//! that holds one block reference per item. Each item refers to the array's *source block*
//! (another anonymous block holding the source objects) or, for items replaced with ARRAYEDIT,
//! to another block. Other programs see an ordinary block reference; CADCraft keeps the
//! parameters below on the array block ([`Block::array`](crate::Block)) so the items can be
//! regenerated when the array is edited.
//!
//! All positions are in the array block's coordinates: its origin is the array's base point,
//! which is where the array's block reference is inserted.

use cadcraft_geom::{Segment, Vec2, Vec3, segments_at_length};
use serde::{Deserialize, Serialize};

use crate::{Common, Drawing, Entity, EntityKind, Insert, Lineweight};

/// Most items one array may have (items × rows × levels), whatever MAXARRAY says.
pub const MAX_ARRAY_ITEMS: u64 = 10_000_000;

fn one() -> u32 {
    1
}

fn true_fn() -> bool {
    true
}

/// The parameters of an associative array.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArrayParams {
    /// The source block the items refer to.
    pub source: String,
    pub kind: ArrayKind,
    /// Levels: copies of the whole array along Z.
    #[serde(default = "one")]
    pub levels: u32,
    #[serde(default)]
    pub level_spacing: f64,
    /// Items that refer to another block than the source (ARRAYEDIT REPlace).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub replaced: Vec<ReplacedItem>,
    /// Items erased/deleted from the array (ARRAYEDIT ERase).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub erased: Vec<(u32, u32, u32)>,
    /// The source objects are out for editing (ARRAYEDIT Source, until ARRAYCLOSE). Not saved:
    /// a drawing saved meanwhile keeps the objects as separate objects.
    #[serde(skip)]
    pub editing: Option<SourceEdit>,
}

/// ARRAYEDIT Source in progress: the array's source objects placed in the drawing for editing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SourceEdit {
    /// The array (block reference) being edited.
    pub array: crate::Handle,
    /// The item the objects were placed at: (i, j, level).
    pub item: (u32, u32, u32),
    /// The objects placed for editing.
    pub objects: Vec<crate::Handle>,
    /// Objects with this handle or above were drawn while editing; they join the source too.
    pub first_new: u64,
}

/// Rectangular, polar or path placement.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ArrayKind {
    Rect(RectArray),
    Polar(PolarArray),
    Path(PathArray),
}

/// A rectangular array: columns along the axis angle, rows across it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RectArray {
    pub cols: u32,
    pub rows: u32,
    pub col_spacing: f64,
    pub row_spacing: f64,
    /// Direction of the rows (the column axis), radians.
    #[serde(default)]
    pub axis_angle: f64,
    /// Elevation added per row.
    #[serde(default)]
    pub row_elevation: f64,
}

/// A polar array: items around a centre, rows as rings further out.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolarArray {
    pub center: Vec2,
    pub items: u32,
    /// Angle to fill, degrees (positive = counterclockwise, ±360 = a full circle).
    pub fill: f64,
    /// Items are rotated as they go around (else they keep their orientation).
    pub rotate: bool,
    #[serde(default = "one")]
    pub rows: u32,
    /// Distance between rings, outwards from the centre.
    #[serde(default)]
    pub row_spacing: f64,
    #[serde(default)]
    pub row_elevation: f64,
}

/// A path array: items distributed along a curve (segments).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathArray {
    pub path: Vec<Segment>,
    #[serde(default)]
    pub closed: bool,
    pub items: u32,
    pub spacing: f64,
    #[serde(default = "true_fn")]
    pub align: bool,
    #[serde(default = "one")]
    pub rows: u32,
    #[serde(default)]
    pub row_spacing: f64,
    #[serde(default)]
    pub row_elevation: f64,
}

/// An item that refers to another block. `i` is the column (rectangular) or the item around the
/// circle (polar), `j` the row.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplacedItem {
    pub i: u32,
    pub j: u32,
    #[serde(default)]
    pub level: u32,
    pub block: String,
}

/// Where one item goes: its block reference's insertion point (the image of the array's base
/// point) and rotation (radians).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArrayItem {
    pub i: u32,
    pub j: u32,
    pub level: u32,
    pub insert: Vec3,
    pub rotation: f64,
}

impl ArrayParams {
    /// Items along the first direction (columns, items around or along path) and rows.
    pub fn grid(&self) -> (u32, u32) {
        match &self.kind {
            ArrayKind::Rect(r) => (r.cols, r.rows),
            ArrayKind::Polar(p) => (p.items, p.rows),
            ArrayKind::Path(pa) => (pa.items, pa.rows),
        }
    }

    /// The number of items, saturating.
    pub fn item_count(&self) -> u64 {
        let (a, b) = self.grid();
        u64::from(a).saturating_mul(u64::from(b)).saturating_mul(u64::from(self.levels))
    }

    /// Whether the parameters are usable: at least one item in every direction, at most
    /// [`MAX_ARRAY_ITEMS`] in all, and finite numbers. Parameters read from files are checked
    /// with this before they are kept.
    pub fn is_valid(&self) -> bool {
        let (a, b) = self.grid();
        let nums = match &self.kind {
            ArrayKind::Rect(r) => [r.col_spacing, r.row_spacing, r.axis_angle, r.row_elevation, 0.0, 0.0],
            ArrayKind::Polar(p) => [p.center.x, p.center.y, p.fill, p.row_spacing, p.row_elevation, 0.0],
            ArrayKind::Path(pa) => [pa.spacing, pa.row_spacing, pa.row_elevation, 0.0, 0.0, 0.0],
        };
        a >= 1
            && b >= 1
            && self.levels >= 1
            && self.item_count() <= MAX_ARRAY_ITEMS
            && !self.source.trim().is_empty()
            && nums.iter().chain(std::iter::once(&self.level_spacing)).all(|v| v.is_finite())
            && self.replaced.len() as u64 <= MAX_ARRAY_ITEMS
            && match &self.kind {
                ArrayKind::Path(pa) => !pa.path.is_empty() && pa.spacing > 0.0,
                _ => true,
            }
    }

    /// Where every item goes, row by row and level by level; `None` when the parameters are not
    /// [valid](Self::is_valid).
    pub fn items(&self) -> Option<Vec<ArrayItem>> {
        if !self.is_valid() {
            return None;
        }
        let n = usize::try_from(self.item_count()).ok()?;
        let mut out = Vec::with_capacity(n);
        for level in 0..self.levels {
            let lz = self.level_spacing * f64::from(level);
            match &self.kind {
                ArrayKind::Rect(r) => {
                    let (ux, uy) = (Vec2::from_angle(r.axis_angle), Vec2::from_angle(r.axis_angle).perp());
                    for j in 0..r.rows {
                        for i in 0..r.cols {
                            let p = ux * (r.col_spacing * f64::from(i)) + uy * (r.row_spacing * f64::from(j));
                            out.push(ArrayItem { i, j, level, insert: p.to3(lz + r.row_elevation * f64::from(j)), rotation: 0.0 });
                        }
                    }
                }
                ArrayKind::Polar(p) => {
                    let total = p.fill.to_radians();
                    let full = (total.abs() - std::f64::consts::TAU).abs() < 1e-9;
                    let step = if full { total / f64::from(p.items.max(1)) } else { total / f64::from(p.items.saturating_sub(1).max(1)) };
                    // Rows move the items outwards: along the line from the centre to the base point.
                    let out_dir = {
                        let d = (Vec2::ZERO - p.center).normalized();
                        if d == Vec2::ZERO { Vec2::X } else { d }
                    };
                    for j in 0..p.rows {
                        let o = out_dir * (p.row_spacing * f64::from(j));
                        for i in 0..p.items {
                            let a = step * f64::from(i);
                            let at = o.rotate_about(p.center, a);
                            let rotation = if p.rotate { a } else { 0.0 };
                            out.push(ArrayItem { i, j, level, insert: at.to3(lz + p.row_elevation * f64::from(j)), rotation });
                        }
                    }
                }
                ArrayKind::Path(pa) => {
                    let init_tangent = pa.path.first().map(|s| s.tangent(0.0)).unwrap_or(Vec2::X);
                    let init_angle = init_tangent.angle();
                    for j in 0..pa.rows {
                        for i in 0..pa.items {
                            let d = pa.spacing * f64::from(i);
                            let (p, tangent) = segments_at_length(&pa.path, d).unwrap_or((Vec2::ZERO, Vec2::X));
                            let row_offset = tangent.perp() * (pa.row_spacing * f64::from(j));
                            let final_pos = p + row_offset;
                            let rotation = if pa.align { tangent.angle() - init_angle } else { 0.0 };
                            out.push(ArrayItem { i, j, level, insert: final_pos.to3(lz + pa.row_elevation * f64::from(j)), rotation });
                        }
                    }
                }
            }
        }
        Some(out)
    }

    /// The block an item refers to: a replacement, else the source.
    pub fn block_for(&self, i: u32, j: u32, level: u32) -> &str {
        self.replaced.iter().rev().find(|r| (r.i, r.j, r.level) == (i, j, level)).map_or(self.source.as_str(), |r| r.block.as_str())
    }

    /// The block references that make up the array block: one per item, on layer 0 with ByBlock
    /// properties so they take the array's. Handles are allocated from `d`. `None` when the
    /// parameters are not [valid](Self::is_valid).
    pub fn item_entities(&self, d: &mut Drawing) -> Option<Vec<Entity>> {
        let items = self.items()?;
        let common = Common {
            layer: "0".into(),
            color: cadcraft_color::Color::ByBlock,
            linetype: "ByBlock".into(),
            lineweight: Lineweight::ByBlock,
            ..Common::default()
        };
        let mut out = Vec::with_capacity(items.len());
        for it in items {
            if self.erased.contains(&(it.i, it.j, it.level)) {
                continue;
            }
            let ins = Insert {
                block: self.block_for(it.i, it.j, it.level).to_string(),
                insert: it.insert,
                scale: Vec3::new(1.0, 1.0, 1.0),
                rotation: it.rotation,
                attribs: Vec::new(),
                cols: 1,
                rows: 1,
                col_spacing: 0.0,
                row_spacing: 0.0,
                clip: None,
            };
            out.push(Entity { handle: d.new_handle(), common: common.clone(), kind: EntityKind::Insert(ins) });
        }
        Some(out)
    }

    /// Take the block names from the array block's items `ents` (in the order [`items`]
    /// lists them): the source block is whatever the items that are not replaced refer to, so
    /// the parameters follow blocks renamed on paste, on INSERT of a file or by other programs
    /// (which may rename anonymous blocks). `false` when the items don't match the parameters
    /// (another program changed the block), so the array should be treated as a plain block.
    ///
    /// [`items`]: Self::items
    pub fn sync_names<'a>(&mut self, ents: impl Iterator<Item = &'a Entity>) -> bool {
        let n = self.item_count().saturating_sub(self.erased.len() as u64);
        let ents: Vec<&Entity> = ents.take(usize::try_from(n).unwrap_or(usize::MAX).saturating_add(1)).collect();
        if ents.len() as u64 != n {
            return false;
        }
        let Some(items) = self.items() else { return false };
        let mut source: Option<&str> = None;
        for (it, e) in items.iter().filter(|it| !self.erased.contains(&(it.i, it.j, it.level))).zip(&ents) {
            let EntityKind::Insert(ins) = &e.kind else { return false };
            match self.replaced.iter_mut().rev().find(|r| (r.i, r.j, r.level) == (it.i, it.j, it.level)) {
                Some(r) => r.block.clone_from(&ins.block),
                None => match source {
                    None => source = Some(&ins.block),
                    Some(s) if s.eq_ignore_ascii_case(&ins.block) => {}
                    Some(_) => return false,
                },
            }
        }
        if let Some(s) = source {
            self.source = s.to_string();
        }
        true
    }

    /// Defining grips for the associative array in world coordinates.
    pub fn grips(&self, e: &Entity) -> Option<Vec<Vec2>> {
        let EntityKind::Insert(ins) = &e.kind else { return None };
        let base = ins.insert.xy();
        let mut out = Vec::new();
        match &self.kind {
            ArrayKind::Rect(r) => {
                let ux = Vec2::from_angle(r.axis_angle);
                let uy = ux.perp();
                // 0: Base point
                out.push(base);
                // 1: Column spacing
                out.push(base + ux * r.col_spacing);
                // 2: Row spacing
                out.push(base + uy * r.row_spacing);
                // 3: Column count (last column)
                out.push(base + ux * (r.col_spacing * f64::from(r.cols.saturating_sub(1).max(1))));
                // 4: Row count (last row)
                out.push(base + uy * (r.row_spacing * f64::from(r.rows.saturating_sub(1).max(1))));
                // 5: Corner (both count)
                out.push(
                    base + ux * (r.col_spacing * f64::from(r.cols.saturating_sub(1).max(1)))
                        + uy * (r.row_spacing * f64::from(r.rows.saturating_sub(1).max(1))),
                );
            }
            ArrayKind::Polar(k) => {
                // 0: Center
                out.push(base + k.center);
                // 1: Base point / radius
                out.push(base);
                // 2: Items count / angle between
                let full = (k.fill.abs() - 360.0).abs() < 1e-9;
                let step = if full {
                    k.fill.to_radians() / f64::from(k.items.max(1))
                } else {
                    k.fill.to_radians() / f64::from(k.items.saturating_sub(1).max(1))
                };
                let p1 = Vec2::ZERO.rotate_about(k.center, step);
                out.push(base + p1);
                // 3: Fill angle
                let p_last = Vec2::ZERO.rotate_about(k.center, k.fill.to_radians());
                out.push(base + p_last);
                // 4: Row spacing (if rows > 1)
                if k.rows > 1 {
                    let out_dir = (Vec2::ZERO - k.center).normalized();
                    let d = if out_dir == Vec2::ZERO { Vec2::X } else { out_dir };
                    out.push(base + d * (k.row_spacing * f64::from(k.rows.saturating_sub(1))));
                }
            }
            ArrayKind::Path(pa) => {
                // 0: Base point
                out.push(base);
                // 1: Item spacing
                let p1 = segments_at_length(&pa.path, pa.spacing).map(|(p, _)| p).unwrap_or(Vec2::X * pa.spacing);
                out.push(base + p1);
                // 2: Items count
                let plast = segments_at_length(&pa.path, pa.spacing * f64::from(pa.items.saturating_sub(1))).map(|(p, _)| p).unwrap_or(p1);
                out.push(base + plast);
                // 3: Row spacing (if rows > 1)
                if pa.rows > 1 {
                    out.push(base + Vec2::Y * (pa.row_spacing * f64::from(pa.rows.saturating_sub(1))));
                }
            }
        }
        Some(out)
    }
}

impl Drawing {
    /// The parameters of `e` when it is an associative array (a block reference to an array
    /// block whose items match its parameters), with the block names its items use.
    pub fn array_params(&self, e: &Entity) -> Option<ArrayParams> {
        let EntityKind::Insert(i) = &e.kind else { return None };
        let b = self.block(&i.block)?;
        let mut p = b.array.clone()?;
        p.sync_names(b.entities.iter().map(|e| e.as_ref())).then_some(p)
    }

    /// Grip points for `e`: associative array grips if it is an associative array, else `e.kind.grips()`.
    pub fn entity_grips(&self, e: &Entity) -> Vec<Vec2> {
        if let Some(p) = self.array_params(e)
            && let Some(g) = p.grips(e)
        {
            return g;
        }
        e.kind.grips()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(cols: u32, rows: u32) -> ArrayParams {
        ArrayParams {
            source: "*U1".into(),
            kind: ArrayKind::Rect(RectArray { cols, rows, col_spacing: 2.0, row_spacing: 3.0, axis_angle: 0.0, row_elevation: 0.0 }),
            levels: 2,
            level_spacing: 5.0,
            replaced: Vec::new(),
            editing: None,
            erased: Vec::new(),
        }
    }

    #[test]
    fn rect_items_go_column_row_level() {
        let items = rect(3, 2).items().unwrap_or_default();
        assert_eq!(items.len(), 12);
        let last = items.last().copied().map(|i| (i.i, i.j, i.level, i.insert));
        assert_eq!(last, Some((2, 1, 1, Vec3::new(4.0, 3.0, 5.0))));
    }

    #[test]
    fn polar_rows_go_outwards() {
        let p = ArrayParams {
            source: "*U1".into(),
            kind: ArrayKind::Polar(PolarArray {
                center: Vec2::new(-10.0, 0.0),
                items: 4,
                fill: 360.0,
                rotate: true,
                rows: 2,
                row_spacing: 1.0,
                row_elevation: 0.0,
            }),
            levels: 1,
            level_spacing: 0.0,
            replaced: Vec::new(),
            editing: None,
            erased: Vec::new(),
        };
        let items = p.items().unwrap_or_default();
        assert_eq!(items.len(), 8);
        // Item 1 of row 0 is a quarter turn round: base (0,0) → (-10,10).
        assert!(items[1].insert.xy().near(Vec2::new(-10.0, 10.0), 1e-9));
        // Row 1 starts one unit further out.
        assert!(items[4].insert.xy().near(Vec2::new(1.0, 0.0), 1e-9));
    }

    #[test]
    fn path_array_items_and_grips() {
        use cadcraft_geom::{Line, Segment};
        let pa = PathArray {
            path: vec![Segment::Line(Line { a: Vec2::ZERO, b: Vec2::new(10.0, 0.0) })],
            closed: false,
            items: 3,
            spacing: 4.0,
            align: true,
            rows: 1,
            row_spacing: 1.0,
            row_elevation: 0.0,
        };
        let p = ArrayParams {
            source: "*U1".into(),
            kind: ArrayKind::Path(pa),
            levels: 1,
            level_spacing: 0.0,
            replaced: Vec::new(),
            editing: None,
            erased: vec![(1, 0, 0)],
        };
        let items = p.items().unwrap_or_default();
        assert_eq!(items.len(), 3);
        let mut doc = Drawing::default();
        let ents = p.item_entities(&mut doc).unwrap_or_default();
        // Item (1, 0, 0) was erased, so only 2 entities created
        assert_eq!(ents.len(), 2);

        let ins = Entity {
            handle: crate::Handle(1),
            common: Common::default(),
            kind: EntityKind::Insert(Insert {
                block: "*U_ARR".into(),
                insert: Vec3::new(100.0, 50.0, 0.0),
                scale: Vec3::new(1.0, 1.0, 1.0),
                rotation: 0.0,
                attribs: Vec::new(),
                cols: 1,
                rows: 1,
                col_spacing: 0.0,
                row_spacing: 0.0,
                clip: None,
            }),
        };
        let grips = p.grips(&ins).unwrap_or_default();
        assert_eq!(grips.len(), 3);
        assert!(grips[0].near(Vec2::new(100.0, 50.0), 1e-6));
        assert!(grips[1].near(Vec2::new(104.0, 50.0), 1e-6));
        assert!(grips[2].near(Vec2::new(108.0, 50.0), 1e-6));
    }

    #[test]
    fn hostile_params_are_invalid() {
        let mut p = rect(u32::MAX, u32::MAX);
        assert!(!p.is_valid() && p.items().is_none());
        p = rect(0, 1);
        assert!(!p.is_valid());
        p = rect(2, 2);
        p.level_spacing = f64::NAN;
        assert!(!p.is_valid());
    }
}
