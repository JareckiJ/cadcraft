//! One bounded profile analysis shared by sketch shading and solid generation.
use cadcraft_doc::{Drawing, Handle, Prim};
use cadcraft_geom::{Bounds2, Vec2};

#[derive(Clone, Debug, Default)]
pub struct Profile {
    pub loops: Vec<Vec<Vec2>>,
    pub triangles: Vec<Vec2>,
    pub error: Option<String>,
}
impl Profile {
    pub fn ready(&self) -> bool {
        self.error.is_none() && !self.triangles.is_empty()
    }
    pub fn message(&self) -> &str {
        self.error.as_deref().unwrap_or(if self.ready() { "Closed profile - ready to extrude" } else { "Draw a closed profile" })
    }
}

pub fn analyze(d: &Drawing, members: &[Handle]) -> Profile {
    let mut result = Profile::default();
    let mut paths: Vec<Vec<Vec2>> = Vec::new();
    let mut count = 0;
    for h in members {
        let Some(e) = d.entity(*h) else {
            result.error = Some("Missing sketch geometry".into());
            return result;
        };
        if !cadcraft_doc::supported_sketch_geometry(&e.kind) {
            result.error = Some("Unsupported sketch geometry".into());
            return result;
        }
        for prim in e.kind.prims() {
            let mut points = Vec::new();
            match prim {
                Prim::Seg(s) => {
                    let b = s.bounds();
                    s.tessellate(((b.max - b.min).len() / 1024.0).max(0.001), &mut points);
                }
                Prim::Circle(c) => c.tessellate((c.radius.abs() / 512.0).max(0.001), &mut points),
                Prim::Ellipse(e) => e.tessellate((e.major.len() / 512.0).max(0.001), &mut points),
                Prim::Spline(s) => points = s.tessellate(0.01),
                Prim::Infinite { .. } | Prim::Point(_) => continue,
                _ => {
                    result.error = Some("Unsupported sketch boundary".into());
                    return result;
                }
            }
            points.dedup_by(|a, b| a.near(*b, 1e-8));
            count += points.len();
            if count > 2048 || points.iter().any(|p| !p.is_finite() || p.x.abs().max(p.y.abs()) > 1e9) {
                result.error = Some("Sketch exceeds the supported profile size or coordinate range".into());
                return result;
            }
            if points.len() < 2 {
                result.error = Some("Sketch contains a zero length edge".into());
                continue;
            }
            paths.push(points);
        }
    }
    // Join finite edges at coincident endpoints; ambiguous junctions never form a solid.
    while let Some(mut chain) = paths.pop() {
        for _ in 0..=members.len().saturating_add(count) {
            let (Some(&start), Some(&end)) = (chain.first(), chain.last()) else { break };
            if start.near(end, 1e-7) {
                break;
            }
            let matches: Vec<_> = paths
                .iter()
                .enumerate()
                .filter_map(|(i, p)| {
                    if p.first().is_some_and(|p| p.near(end, 1e-7)) {
                        Some((i, false))
                    } else if p.last().is_some_and(|p| p.near(end, 1e-7)) {
                        Some((i, true))
                    } else {
                        None
                    }
                })
                .collect();
            if matches.len() != 1 {
                break;
            }
            let Some(&(i, reverse)) = matches.first() else { break };
            let mut next = paths.remove(i);
            if reverse {
                next.reverse();
            }
            chain.extend(next.into_iter().skip(1));
        }
        if chain.first().zip(chain.last()).is_some_and(|(a, b)| a.near(*b, 1e-7)) && chain.len() >= 4 {
            chain.pop();
            result.loops.push(chain);
        } else {
            result.error = Some("Open sketch: join the boundary endpoints before extruding".into());
        }
    }
    let edges: Vec<_> = result
        .loops
        .iter()
        .enumerate()
        .flat_map(|(li, l)| l.iter().copied().zip(l.iter().copied().cycle().skip(1)).take(l.len()).enumerate().map(move |(i, (a, b))| (li, i, a, b)))
        .collect();
    for (i, &(li, ei, a, b)) in edges.iter().enumerate() {
        for &(lj, ej, c, e) in edges.iter().skip(i + 1) {
            let adjacent = li == lj && (ei.abs_diff(ej) == 1 || ei.abs_diff(ej) + 1 == result.loops.get(li).map_or(0, Vec::len));
            if adjacent {
                continue;
            }
            let cross = |x: Vec2, y: Vec2| x.x * y.y - x.y * y.x;
            let u = b - a;
            let v = e - c;
            let den = cross(u, v);
            let touching = if den.abs() > 1e-12 {
                let t = cross(c - a, v) / den;
                let s = cross(c - a, u) / den;
                (-1e-9..=1.0 + 1e-9).contains(&t) && (-1e-9..=1.0 + 1e-9).contains(&s)
            } else {
                cross(c - a, u).abs() < 1e-8 && Bounds2::new(a, b).intersects(&Bounds2::new(c, e))
            };
            if touching {
                result.error = Some("Intersecting or overlapping boundaries cannot be extruded".into());
                result.loops.clear();
                return result;
            }
        }
    }
    for l in &result.loops {
        for (i, b) in l.iter().enumerate() {
            let a = l.get((i + l.len() - 1) % l.len()).copied().unwrap_or(*b);
            let c = l.get((i + 1) % l.len()).copied().unwrap_or(*b);
            let u = *b - a;
            let v = c - *b;
            if (u.x * v.y - u.y * v.x).abs() < 1e-10 && u.dot(v) < 0.0 {
                result.error = Some("Sketch contains overlapping edges".into());
                result.loops.clear();
                return result;
            }
        }
        let area: f64 = l.iter().zip(l.iter().cycle().skip(1)).take(l.len()).map(|(a, b)| a.x * b.y - a.y * b.x).sum();
        if area.abs() < 1e-9 {
            result.error = Some("Sketch has no enclosed area".into());
            return result;
        }
    }
    result.triangles = cadcraft_render::triangulate_evenodd(&result.loops);
    result
}
