//! Bounded convex-polygon solid clipping for tessellated closed profiles.
//! Rectangular cell operations remain the exact fast path.
use super::{Body, bad};
use crate::Result;
use cadcraft_doc::SolidOperation;
use cadcraft_geom::{Vec2, Vec3};
const EPS: f64 = 1e-7;
const MAX_POLYGONS: usize = 20000;
const MAX_DEPTH: usize = 768;

#[derive(Clone)]
struct Polygon {
    points: Vec<Vec3>,
    normal: Vec3,
    w: f64,
    planar: bool,
}
impl Polygon {
    fn new(mut points: Vec<Vec3>, planar: bool) -> Option<Self> {
        points.dedup_by(|a, b| (*a - *b).len() < EPS);
        if points.first().zip(points.last()).is_some_and(|(a, b)| (*a - *b).len() < EPS) {
            points.pop();
        }
        let a = *points.first()?;
        let n = points.windows(2).map(|p| (p[0] - a).cross(p[1] - a)).max_by(|a, b| a.len().total_cmp(&b.len()))?;
        if points.len() < 3 || n.len() < 1e-10 {
            return None;
        }
        let normal = n.normalized();
        Some(Self { w: normal.dot(a), points, normal, planar })
    }
    fn invert(&mut self) {
        self.points.reverse();
        self.normal = self.normal * (-1.0);
        self.w = -self.w;
    }
}
#[derive(Clone, Copy)]
struct Plane {
    normal: Vec3,
    w: f64,
}
enum Split {
    Coplanar(Polygon, bool),
    Front(Polygon),
    Back(Polygon),
    Both(Option<Polygon>, Option<Polygon>),
}
struct Budget {
    work: usize,
}
impl Budget {
    fn spend(&mut self, n: usize) -> Result<()> {
        self.work = self.work.saturating_add(n);
        if self.work > 2_000_000 { Err(bad("extrude", "Solid clipping complexity limit reached")) } else { Ok(()) }
    }
}
fn split(p: Polygon, plane: Plane, budget: &mut Budget) -> Result<Split> {
    budget.spend(p.points.len())?;
    let distances: Vec<_> = p.points.iter().map(|v| plane.normal.dot(*v) - plane.w).collect();
    let front = distances.iter().any(|d| *d > EPS);
    let back = distances.iter().any(|d| *d < -EPS);
    if !front && !back {
        let same = plane.normal.dot(p.normal) > 0.0;
        return Ok(Split::Coplanar(p, same));
    }
    if !back {
        return Ok(Split::Front(p));
    }
    if !front {
        return Ok(Split::Back(p));
    }
    let mut a = Vec::new();
    let mut b = Vec::new();
    for i in 0..p.points.len() {
        let j = (i + 1) % p.points.len();
        let (Some(&v), Some(&q), Some(&d), Some(&e)) = (p.points.get(i), p.points.get(j), distances.get(i), distances.get(j)) else { continue };
        if d >= -EPS {
            a.push(v);
        }
        if d <= EPS {
            b.push(v);
        }
        if (d > EPS && e < -EPS) || (d < -EPS && e > EPS) {
            let cut = v + (q - v) * (d / (d - e));
            a.push(cut);
            b.push(cut);
        }
    }
    if a.len().max(b.len()) > 4096 {
        return Err(bad("extrude", "Clipped polygon limit reached"));
    }
    Ok(Split::Both(Polygon::new(a, p.planar), Polygon::new(b, p.planar)))
}
#[derive(Default)]
struct Node {
    plane: Option<Plane>,
    polygons: Vec<Polygon>,
    front: Option<Box<Node>>,
    back: Option<Box<Node>>,
}
impl Node {
    fn build(&mut self, polygons: Vec<Polygon>, depth: usize, budget: &mut Budget) -> Result<()> {
        if polygons.is_empty() {
            return Ok(());
        }
        if depth > MAX_DEPTH || polygons.len() > MAX_POLYGONS {
            return Err(bad("extrude", "Solid clipping tree limit reached"));
        }
        let first = polygons.first().ok_or_else(|| bad("extrude", "Missing polygon"))?;
        let plane = *self.plane.get_or_insert(Plane { normal: first.normal, w: first.w });
        let mut front = Vec::new();
        let mut back = Vec::new();
        for polygon in polygons {
            match split(polygon, plane, budget)? {
                Split::Coplanar(p, _) => self.polygons.push(p),
                Split::Front(p) => front.push(p),
                Split::Back(p) => back.push(p),
                Split::Both(a, b) => {
                    front.extend(a);
                    back.extend(b);
                }
            }
        }
        if !front.is_empty() {
            self.front.get_or_insert_with(Default::default).build(front, depth + 1, budget)?;
        }
        if !back.is_empty() {
            self.back.get_or_insert_with(Default::default).build(back, depth + 1, budget)?;
        }
        Ok(())
    }
    fn clip(&self, polygons: Vec<Polygon>, depth: usize, budget: &mut Budget) -> Result<Vec<Polygon>> {
        if depth > MAX_DEPTH {
            return Err(bad("extrude", "Solid clipping depth limit reached"));
        }
        let Some(plane) = self.plane else { return Ok(polygons) };
        let mut front = Vec::new();
        let mut back = Vec::new();
        for polygon in polygons {
            match split(polygon, plane, budget)? {
                Split::Coplanar(p, true) | Split::Front(p) => front.push(p),
                Split::Coplanar(p, false) | Split::Back(p) => back.push(p),
                Split::Both(a, b) => {
                    front.extend(a);
                    back.extend(b);
                }
            }
        }
        if let Some(node) = &self.front {
            front = node.clip(front, depth + 1, budget)?;
        }
        if let Some(node) = &self.back {
            back = node.clip(back, depth + 1, budget)?;
        } else {
            back.clear();
        }
        front.extend(back);
        if front.len() > MAX_POLYGONS {
            return Err(bad("extrude", "Solid polygon limit reached"));
        }
        Ok(front)
    }
    fn clip_to(&mut self, other: &Self, depth: usize, budget: &mut Budget) -> Result<()> {
        if depth > MAX_DEPTH {
            return Err(bad("extrude", "Solid clipping depth limit reached"));
        }
        self.polygons = other.clip(std::mem::take(&mut self.polygons), 0, budget)?;
        if let Some(node) = &mut self.front {
            node.clip_to(other, depth + 1, budget)?;
        }
        if let Some(node) = &mut self.back {
            node.clip_to(other, depth + 1, budget)?;
        }
        Ok(())
    }
    fn invert(&mut self) {
        for p in &mut self.polygons {
            p.invert();
        }
        if let Some(p) = &mut self.plane {
            p.normal = p.normal * (-1.0);
            p.w = -p.w;
        }
        if let Some(n) = &mut self.front {
            n.invert();
        }
        if let Some(n) = &mut self.back {
            n.invert();
        }
        std::mem::swap(&mut self.front, &mut self.back);
    }
    fn collect(self, out: &mut Vec<Polygon>) -> Result<()> {
        out.extend(self.polygons);
        if out.len() > MAX_POLYGONS {
            return Err(bad("extrude", "Solid polygon limit reached"));
        }
        if let Some(n) = self.front {
            n.collect(out)?;
        }
        if let Some(n) = self.back {
            n.collect(out)?;
        }
        Ok(())
    }
}
fn polygons(body: &Body) -> Vec<Polygon> {
    body.faces
        .iter()
        .enumerate()
        .filter_map(|(i, _)| Polygon::new(body.face_points(i)?.to_vec(), body.planar_faces.get(i).copied().unwrap_or(true)))
        .collect()
}
fn compute(a: &Body, b: &Body, operation: SolidOperation) -> Result<Vec<Polygon>> {
    let mut budget = Budget { work: 0 };
    let mut left = Node::default();
    let mut right = Node::default();
    left.build(polygons(a), 0, &mut budget)?;
    right.build(polygons(b), 0, &mut budget)?;
    if operation == SolidOperation::Remove {
        left.invert();
    }
    left.clip_to(&right, 0, &mut budget)?;
    right.clip_to(&left, 0, &mut budget)?;
    right.invert();
    right.clip_to(&left, 0, &mut budget)?;
    right.invert();
    let mut remaining = Vec::new();
    right.collect(&mut remaining)?;
    left.build(remaining, 0, &mut budget)?;
    if operation == SolidOperation::Remove {
        left.invert();
    }
    let mut result = Vec::new();
    left.collect(&mut result)?;
    Ok(result)
}
fn volume(polygons: &[Polygon]) -> f64 {
    let origin = polygons.first().and_then(|p| p.points.first()).copied().unwrap_or(Vec3::ZERO);
    polygons
        .iter()
        .map(|p| {
            let Some(&a) = p.points.first() else { return 0.0 };
            p.points.get(1..).unwrap_or_default().windows(2).map(|w| (a - origin).dot((w[0] - origin).cross(w[1] - origin)) / 6.0).sum::<f64>()
        })
        .sum::<f64>()
        .abs()
}
fn contact(a: &Body, b: &Body) -> Result<bool> {
    let left = polygons(a);
    let right = polygons(b);
    let mut budget = Budget { work: 0 };
    for p in &left {
        for q in &right {
            budget.spend(1)?;
            if p.normal.dot(q.normal) > -1.0 + 1e-7 || (p.w + q.w).abs() > EPS * 10.0 {
                continue;
            }
            let Some(&origin) = p.points.first() else { continue };
            let Some(&next) = p.points.get(1) else { continue };
            let u = (next - origin).normalized();
            let v = p.normal.cross(u);
            let project = |x: Vec3| Vec2::new((x - origin).dot(u), (x - origin).dot(v));
            let mut subject: Vec<_> = p.points.iter().map(|x| project(*x)).collect();
            let clip: Vec<_> = q.points.iter().map(|x| project(*x)).collect();
            let cross = |a: Vec2, b: Vec2| a.x * b.y - a.y * b.x;
            let area: f64 = clip.iter().zip(clip.iter().cycle().skip(1)).take(clip.len()).map(|(a, b)| cross(*a, *b)).sum();
            let sign = area.signum();
            for (&a, &b) in clip.iter().zip(clip.iter().cycle().skip(1)).take(clip.len()) {
                let mut out = Vec::new();
                for (&x, &y) in subject.iter().zip(subject.iter().cycle().skip(1)).take(subject.len()) {
                    let d = cross(b - a, x - a) * sign;
                    let e = cross(b - a, y - a) * sign;
                    if d >= -EPS {
                        out.push(x);
                    }
                    if (d >= -EPS) != (e >= -EPS) && (d - e).abs() > 1e-15 {
                        out.push(x + (y - x) * (d / (d - e)));
                    }
                }
                subject = out;
                if subject.is_empty() {
                    break;
                }
            }
            let area: f64 = subject.iter().zip(subject.iter().cycle().skip(1)).take(subject.len()).map(|(a, b)| cross(*a, *b)).sum();
            if area.abs() > EPS {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
pub fn suggest(target: &Body, tool: &Body) -> Result<SolidOperation> {
    let remainder = compute(target, tool, SolidOperation::Remove)?;
    if target.volume - volume(&remainder) > target.volume.max(1.0) * 1e-8 {
        return Ok(SolidOperation::Remove);
    }
    if contact(target, tool)? {
        Ok(SolidOperation::Add)
    } else {
        Err(bad("extrude", "Extrusion is disconnected from the target body; choose New Body"))
    }
}
pub fn apply(target: &mut Body, tool: &Body, operation: SolidOperation) -> Result<()> {
    let result = compute(target, tool, operation)?;
    let value = volume(&result);
    if !value.is_finite() || value <= EPS {
        return Err(bad("extrude", "This cut would remove the entire target body"));
    }
    if operation == SolidOperation::Remove && target.volume - value <= target.volume.max(1.0) * 1e-8 {
        return Err(bad("extrude", "The cut does not intersect the target body"));
    }
    if operation == SolidOperation::Add && !contact(target, tool)? && (target.volume + tool.volume - value).abs() <= target.volume.max(1.0) * 1e-8 {
        return Err(bad("extrude", "Add Material is disconnected; choose New Body"));
    }
    let mut vertices = Vec::new();
    let mut faces = Vec::new();
    let mut planar = Vec::new();
    for p in result {
        let Some(&a) = p.points.first() else { continue };
        for pair in p.points.get(1..).unwrap_or_default().windows(2) {
            if (pair[0] - a).cross(pair[1] - a).len() < 1e-10 {
                continue;
            }
            let start = vertices.len();
            for point in [a, pair[0], pair[1], pair[1]] {
                vertices.push([point.x, point.y, point.z]);
            }
            faces.push([start, start + 1, start + 2, start + 3]);
            planar.push(p.planar);
            if faces.len() > 16384 {
                return Err(bad("extrude", "Resulting solid mesh limit reached"));
            }
        }
    }
    target.vertices = vertices;
    target.faces = faces;
    target.planar_faces = planar;
    target.cells.clear();
    target.volume = value;
    let bounds = super::solid::Cell::from_vertices(&target.vertices);
    target.width = bounds.max[0] - bounds.min[0];
    target.height = bounds.max[1] - bounds.min[1];
    target.depth = bounds.max[2] - bounds.min[2];
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        Session,
        cmd::modeling::{bodies, face_reference},
    };
    use serde_json::json;
    fn base() -> Session {
        let mut s = Session::new();
        s.execute("part.new", &json!({})).unwrap();
        s.execute("extrude", &json!({"width":40,"height":30,"depth":10})).unwrap();
        s
    }
    fn sketch(s: &mut Session, command: &str, args: serde_json::Value) {
        let reference = face_reference(s.doc().unwrap(), 2000, 1).unwrap();
        s.execute("part.sketch.begin", &json!({"face":reference})).unwrap();
        s.execute(command, &args).unwrap();
    }
    #[test]
    fn closed_curved_profiles_add_cut_reverse_and_roundtrip() {
        for (command, args) in [
            ("circle", json!({"center":[10,10],"radius":3})),
            ("ellipse", json!({"center":[10,10],"major":[4,0],"ratio":0.5})),
            ("spline", json!({"fit":[[4,4],[14,4],[14,14],[4,14]],"closed":true})),
        ] {
            let mut s = base();
            sketch(&mut s, command, args);
            s.execute("part.sketch.commit", &json!({"extrude":true,"depth":5,"operation":"add","target":2000})).unwrap();
            let added = bodies(s.doc().unwrap()).unwrap()[0].volume - 12000.0;
            assert!(added > 10.0, "{command}: {added}");
            let cut = s.execute("extrude", &json!({"feature":2001,"reverse":true,"autoOperation":true})).unwrap();
            assert_eq!(cut["operation"], "remove");
            assert!((bodies(s.doc().unwrap()).unwrap()[0].volume - (12000.0 - added)).abs() < 0.01, "{command}");
            let add = s.execute("extrude", &json!({"feature":2001,"reverse":false,"autoOperation":true})).unwrap();
            assert_eq!(add["operation"], "add");
            assert!((bodies(s.doc().unwrap()).unwrap()[0].volume - (12000.0 + added)).abs() < 0.01);
            let bytes = cadcraft_io::write(s.doc().unwrap(), "curved.cadcraft").unwrap();
            let reopened = cadcraft_io::read(&bytes, "curved.cadcraft").unwrap();
            assert!((bodies(&reopened).unwrap()[0].volume - (12000.0 + added)).abs() < 0.01);
        }
    }
    #[test]
    fn through_hole_and_partial_overhang_leave_positive_oriented_solids() {
        let mut s = base();
        sketch(&mut s, "circle", json!({"center":[10,10],"radius":3}));
        s.execute("part.sketch.commit", &json!({"extrude":true,"depth":20,"operation":"remove","target":2000,"reverse":true})).unwrap();
        let body = bodies(s.doc().unwrap()).unwrap().remove(0);
        assert!((body.volume - (12000.0 - 90.0 * std::f64::consts::PI)).abs() < 2.0);
        let mut volume = 0.0;
        for face in &body.faces {
            for tri in [[face[0], face[1], face[2]], [face[0], face[2], face[3]]] {
                let [a, b, c] = tri.map(|i| {
                    let p = body.vertices[i];
                    cadcraft_geom::Vec3::new(p[0], p[1], p[2])
                });
                volume += a.dot(b.cross(c)) / 6.0;
            }
        }
        assert!((volume - body.volume).abs() < 0.01);
        let mut s = base();
        sketch(&mut s, "circle", json!({"center":[39,15],"radius":5}));
        s.execute("part.sketch.commit", &json!({"extrude":true,"depth":5,"operation":"add","target":2000})).unwrap();
        assert!((bodies(s.doc().unwrap()).unwrap()[0].volume - (12000.0 + 125.0 * std::f64::consts::PI)).abs() < 3.0);
    }
}
