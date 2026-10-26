//! Bounded exact set operations on disjoint axis-aligned rectangular cells.
//! This supports rectangular extrusions on the three datum orientations.
use super::{Body, bad};
use crate::Result;
use cadcraft_doc::SolidOperation;

const EPS: f64 = 1e-8;
const MAX_CELLS: usize = 256;
#[derive(Clone, Debug)]
pub struct Cell {
    pub min: [f64; 3],
    pub max: [f64; 3],
}
impl Cell {
    pub fn from_vertices(points: &[[f64; 3]]) -> Self {
        let mut c = Self { min: [f64::INFINITY; 3], max: [f64::NEG_INFINITY; 3] };
        for p in points {
            for (axis, value) in p.iter().enumerate() {
                if let (Some(lo), Some(hi)) = (c.min.get_mut(axis), c.max.get_mut(axis)) {
                    *lo = lo.min(*value);
                    *hi = hi.max(*value);
                }
            }
        }
        c
    }
    fn volume(&self) -> f64 {
        (self.max[0] - self.min[0]) * (self.max[1] - self.min[1]) * (self.max[2] - self.min[2])
    }
}
fn subtract(a: &Cell, b: &Cell) -> Vec<Cell> {
    let lo = std::array::from_fn::<_, 3, _>(|i| a.min[i].max(b.min[i]));
    let hi = std::array::from_fn::<_, 3, _>(|i| a.max[i].min(b.max[i]));
    if (0..3).any(|i| hi[i] - lo[i] <= EPS) {
        return vec![a.clone()];
    }
    let mut result = Vec::new();
    let mut core = a.clone();
    for axis in 0..3 {
        if core.min[axis] < lo[axis] - EPS {
            let mut strip = core.clone();
            strip.max[axis] = lo[axis];
            result.push(strip);
            core.min[axis] = lo[axis];
        }
        if core.max[axis] > hi[axis] + EPS {
            let mut strip = core.clone();
            strip.min[axis] = hi[axis];
            result.push(strip);
            core.max[axis] = hi[axis];
        }
    }
    result
}
pub fn suggest(target: &Body, tool: &Body) -> Result<SolidOperation> {
    if target.cells.is_empty() || tool.cells.len() != 1 {
        return Err(bad("extrude", "Automatic Add/Remove currently requires rectangular profiles and cell bodies"));
    }
    let cutter = tool.cells.first().ok_or_else(|| bad("extrude", "Missing extrusion tool"))?;
    let overlap: f64 = target
        .cells
        .iter()
        .map(|cell| {
            let lengths = std::array::from_fn::<_, 3, _>(|i| cell.max[i].min(cutter.max[i]) - cell.min[i].max(cutter.min[i]));
            if lengths.iter().all(|v| *v > EPS) { lengths.iter().product() } else { 0.0 }
        })
        .sum();
    if overlap > EPS {
        return Ok(SolidOperation::Remove);
    }
    let touches = target.cells.iter().any(|cell| touches(cell, cutter));
    if touches {
        Ok(SolidOperation::Add)
    } else {
        Err(bad("extrude", "Extrusion is disconnected from the target body; choose New Body to keep a separate solid"))
    }
}
fn touches(a: &Cell, b: &Cell) -> bool {
    (0..3).all(|i| a.max[i] >= b.min[i] - EPS && b.max[i] >= a.min[i] - EPS)
        && (0..3).filter(|i| a.max[*i].min(b.max[*i]) - a.min[*i].max(b.min[*i]) > EPS).count() >= 2
}
pub fn apply(target: &mut Body, tool: &Body, operation: SolidOperation) -> Result<()> {
    let cutter = tool.cells.first().ok_or_else(|| bad("extrude", "Invalid tool solid"))?;
    let mut cells = Vec::new();
    match operation {
        SolidOperation::Remove => {
            for cell in &target.cells {
                cells.extend(subtract(cell, cutter));
            }
        }
        SolidOperation::Add => {
            let mut added = vec![cutter.clone()];
            for cell in &target.cells {
                added = added.iter().flat_map(|piece| subtract(piece, cell)).collect();
                if added.len() > MAX_CELLS {
                    return Err(bad("extrude", "Solid complexity limit reached"));
                }
            }
            cells = target.cells.clone();
            cells.extend(added);
        }
        _ => return Err(bad("extrude", "Invalid modification operation")),
    }
    if cells.is_empty() {
        return Err(bad("extrude", "This cut would remove the entire target body"));
    }
    if cells.len() > MAX_CELLS {
        return Err(bad("extrude", "Solid complexity limit reached"));
    }
    let volume: f64 = cells.iter().map(Cell::volume).sum();
    if operation == SolidOperation::Remove && (volume - target.volume).abs() <= EPS {
        return Err(bad("extrude", "The cut does not intersect the target body"));
    }
    if operation == SolidOperation::Add {
        let touches = target.cells.iter().any(|a| touches(a, cutter));
        if !touches {
            return Err(bad("extrude", "Add Material must touch the target over a face or volume; use New Body for a separate solid"));
        }
    }
    target.cells = cells;
    target.volume = volume;
    surface(target)?;
    Ok(())
}

// Subtract one rectangle from another in a face's local coordinates.
fn rect_subtract(a: [f64; 4], b: [f64; 4]) -> Vec<[f64; 4]> {
    let (x0, y0, x1, y1) = (a[0].max(b[0]), a[1].max(b[1]), a[2].min(b[2]), a[3].min(b[3]));
    if x1 - x0 <= EPS || y1 - y0 <= EPS {
        return vec![a];
    }
    [[a[0], a[1], x0, a[3]], [x1, a[1], a[2], a[3]], [x0, a[1], x1, y0], [x0, y1, x1, a[3]]]
        .into_iter()
        .filter(|r| r[2] - r[0] > EPS && r[3] - r[1] > EPS)
        .collect()
}
fn surface(body: &mut Body) -> Result<()> {
    body.vertices.clear();
    body.faces.clear();
    for cell in &body.cells {
        for axis in 0..3 {
            for positive in [false, true] {
                let u = (axis + 1) % 3;
                let v = (axis + 2) % 3;
                let plane = if positive { cell.max[axis] } else { cell.min[axis] };
                let mut patches = vec![[cell.min[u], cell.min[v], cell.max[u], cell.max[v]]];
                for other in &body.cells {
                    let opposite = if positive { other.min[axis] } else { other.max[axis] };
                    if (plane - opposite).abs() < EPS {
                        let block = [other.min[u], other.min[v], other.max[u], other.max[v]];
                        patches = patches.into_iter().flat_map(|p| rect_subtract(p, block)).collect();
                    }
                }
                for r in patches {
                    if body.faces.len() >= 8192 {
                        return Err(bad("extrude", "Solid surface limit reached"));
                    }
                    let mut points = [[0.0; 3]; 4];
                    for (p, (x, y)) in points.iter_mut().zip([(r[0], r[1]), (r[2], r[1]), (r[2], r[3]), (r[0], r[3])]) {
                        p[axis] = plane;
                        p[u] = x;
                        p[v] = y;
                    }
                    if !positive {
                        points.reverse();
                    }
                    let start = body.vertices.len();
                    body.vertices.extend(points);
                    body.faces.push([start, start + 1, start + 2, start + 3]);
                }
            }
        }
    }
    let bounds = Cell::from_vertices(&body.vertices);
    body.width = bounds.max[0] - bounds.min[0];
    body.height = bounds.max[1] - bounds.min[1];
    body.depth = bounds.max[2] - bounds.min[2];
    Ok(())
}
