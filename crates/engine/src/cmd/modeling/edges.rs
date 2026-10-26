//! Visible crease/boundary edges, excluding flat mesh subdivisions and T-junction seams.
use super::Body;
use cadcraft_geom::Vec3;
use std::collections::{BTreeMap, BTreeSet};

pub struct DisplayEdge {
    pub a: Vec3,
    pub b: Vec3,
    pub faces: Vec<usize>,
    pub smooth: bool,
}
struct Span {
    low: f64,
    high: f64,
    face: usize,
    normal: Vec3,
}
struct Line {
    origin: Vec3,
    direction: Vec3,
    spans: Vec<Span>,
}
impl Body {
    pub fn display_edges(&self) -> Vec<DisplayEdge> {
        let mut lines: BTreeMap<[i64; 6], Line> = BTreeMap::new();
        for (index, _) in self.faces.iter().enumerate() {
            let (Some(points), Some(normal)) = (self.face_points(index), self.face_normal(index)) else { continue };
            for (&a, &b) in points.iter().zip(points.iter().cycle().skip(1)).take(4) {
                let delta = b - a;
                if !delta.is_finite() || delta.len() < 1e-8 {
                    continue;
                }
                let mut dir = delta.normalized();
                let first = [dir.x, dir.y, dir.z].into_iter().find(|v| v.abs() > 1e-9).unwrap_or(1.0);
                if first < 0.0 {
                    dir = dir * (-1.0);
                }
                let origin = a - dir * a.dot(dir);
                let key = [dir.x, dir.y, dir.z, origin.x, origin.y, origin.z].map(|v| (v * 1e6).round() as i64);
                let line = lines.entry(key).or_insert_with(|| Line { origin, direction: dir, spans: Vec::new() });
                let (t0, t1) = (a.dot(line.direction), b.dot(line.direction));
                line.spans.push(Span { low: t0.min(t1), high: t0.max(t1), face: index, normal });
            }
        }
        let mut result = Vec::new();
        for line in lines.values() {
            let mut events: Vec<_> = line.spans.iter().enumerate().flat_map(|(i, s)| [(s.low, i, true), (s.high, i, false)]).collect();
            events.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut active = BTreeSet::new();
            let mut i = 0;
            while let Some(&(low, _, _)) = events.get(i) {
                while let Some(&(t, index, start)) = events.get(i) {
                    if (t - low).abs() > 1e-7 {
                        break;
                    }
                    if start {
                        active.insert(index);
                    } else {
                        active.remove(&index);
                    }
                    i += 1;
                }
                let Some(&(high, _, _)) = events.get(i) else { break };
                let spans: Vec<_> = active.iter().filter_map(|index| line.spans.get(*index)).collect();
                let Some(first) = spans.first() else { continue };
                if spans.len() > 1 && spans.iter().all(|s| s.normal.dot(first.normal) > 1.0 - 1e-7) {
                    continue;
                }
                result.push(DisplayEdge {
                    a: line.origin + line.direction * low,
                    b: line.origin + line.direction * high,
                    faces: spans.iter().map(|s| s.face).collect(),
                    smooth: spans.len() > 1 && spans.iter().all(|s| !self.planar_faces.get(s.face).copied().unwrap_or(true)),
                });
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use crate::{Session, cmd::modeling::bodies};
    use serde_json::json;
    #[test]
    fn triangulating_flat_faces_does_not_add_visible_diagonals() {
        let mut s = Session::new();
        s.execute("part.new", &json!({})).unwrap();
        s.execute("extrude", &json!({"width":40,"height":20,"depth":10})).unwrap();
        let mut body = bodies(s.doc().unwrap()).unwrap().remove(0);
        assert_eq!(body.display_edges().len(), 12);
        let top = body.faces.remove(1);
        body.faces.push([top[0], top[1], top[2], top[2]]);
        body.faces.push([top[0], top[2], top[3], top[3]]);
        assert_eq!(body.display_edges().len(), 12);
    }
}
