//! Faces for simple 3D primitives: a prism from a closed profile (EXTRUDE), boxes and cylinders.
//!
//! Every face is a 3DFACE-style quad: four corners, a triangle repeats its last corner, and the
//! `hidden` bits (1 = first edge, 2 = second, 4 = third, 8 = fourth) mark the edges a triangulated
//! cap draws only as a diagonal. Faces wind counter-clockwise seen from outside the solid.
//!
//! Input is hostile: every size is checked, the profile must be a simple (non self-touching)
//! polygon, and the number of points and faces is capped, so nothing here panics or allocates
//! without bound.

use std::fmt;

use crate::{EPS, TAU, Vec2, Vec3, shoelace};

/// Most profile points a prism takes.
pub const MAX_PROFILE: usize = 4096;
/// Most sides a cylinder (or a tessellated circle) gets.
pub const MAX_SEGMENTS: usize = 1024;

/// One planar face with up to four corners.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Face {
    pub corners: [Vec3; 4],
    /// Invisible edges, bit `i` for the edge from corner `i` to corner `i + 1`.
    pub hidden: u8,
}

/// Why a solid could not be built; the text says what to change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MeshError {
    /// The profile is unusable (too few points, open, self-intersecting, without area…).
    Profile(&'static str),
    /// A size or position is not a usable number.
    Size(&'static str),
}

impl fmt::Display for MeshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MeshError::Profile(m) | MeshError::Size(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for MeshError {}

fn orient(a: Vec2, b: Vec2, c: Vec2) -> f64 {
    (b - a).cross(c - a)
}

fn on_segment(a: Vec2, b: Vec2, p: Vec2, tol: f64) -> bool {
    p.x >= a.x.min(b.x) - tol && p.x <= a.x.max(b.x) + tol && p.y >= a.y.min(b.y) - tol && p.y <= a.y.max(b.y) + tol
}

/// True when the segments `ab` and `cd` share any point (touching counts).
fn segments_meet(a: Vec2, b: Vec2, c: Vec2, d: Vec2, eps: f64, tol: f64) -> bool {
    let d1 = orient(c, d, a);
    let d2 = orient(c, d, b);
    let d3 = orient(a, b, c);
    let d4 = orient(a, b, d);
    if ((d1 > eps && d2 < -eps) || (d1 < -eps && d2 > eps)) && ((d3 > eps && d4 < -eps) || (d3 < -eps && d4 > eps)) {
        return true;
    }
    (d1.abs() <= eps && on_segment(c, d, a, tol))
        || (d2.abs() <= eps && on_segment(c, d, b, tol))
        || (d3.abs() <= eps && on_segment(a, b, c, tol))
        || (d4.abs() <= eps && on_segment(a, b, d, tol))
}

/// The profile as a simple counter-clockwise polygon: repeated points and a repeated closing point
/// dropped, orientation normalised. Refuses fewer than three points, points that are not finite,
/// a polygon without area, and one that crosses or touches itself.
pub fn simple_profile(pts: &[Vec2]) -> Result<Vec<Vec2>, MeshError> {
    if pts.len() > MAX_PROFILE {
        return Err(MeshError::Profile("the profile has too many points"));
    }
    if pts.iter().any(|p| !p.is_finite()) {
        return Err(MeshError::Profile("the profile has a point that is not a number"));
    }
    let (mut lo, mut hi) = (Vec2::new(f64::MAX, f64::MAX), Vec2::new(f64::MIN, f64::MIN));
    for p in pts {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    let diag = if pts.is_empty() { 0.0 } else { (hi - lo).len() };
    let tol = 1e-9 * (1.0 + diag);
    let mut poly: Vec<Vec2> = Vec::with_capacity(pts.len());
    for p in pts {
        if !poly.last().is_some_and(|l| l.near(*p, tol)) {
            poly.push(*p);
        }
    }
    if poly.len() > 1 && poly.first().zip(poly.last()).is_some_and(|(f, l)| f.near(*l, tol)) {
        poly.pop();
    }
    let n = poly.len();
    if n < 3 {
        return Err(MeshError::Profile("the profile needs at least 3 distinct points"));
    }
    let area = shoelace(&poly);
    let eps = 1e-12 * diag * diag;
    if area.abs() <= eps.max(f64::MIN_POSITIVE) {
        return Err(MeshError::Profile("the profile has no area"));
    }
    for i in 0..n {
        let (Some(a), Some(b)) = (poly.get(i).copied(), poly.get((i + 1) % n).copied()) else { continue };
        // A spike: the next edge folds back over this one.
        if let Some(c) = poly.get((i + 2) % n).copied()
            && orient(a, b, c).abs() <= eps
            && (b - a).dot(c - b) < 0.0
        {
            return Err(MeshError::Profile("the profile folds back on itself"));
        }
        for j in (i + 2)..n {
            if i == 0 && j == n - 1 {
                continue;
            }
            let (Some(c), Some(d)) = (poly.get(j).copied(), poly.get((j + 1) % n).copied()) else { continue };
            if segments_meet(a, b, c, d, eps, tol) {
                return Err(MeshError::Profile("the profile crosses or touches itself"));
            }
        }
    }
    if area < 0.0 {
        poly.reverse();
    }
    Ok(poly)
}

fn inside_or_on(a: Vec2, b: Vec2, c: Vec2, p: Vec2, eps: f64) -> bool {
    orient(a, b, p) >= -eps && orient(b, c, p) >= -eps && orient(c, a, p) >= -eps
}

/// Ear clipping of a simple counter-clockwise polygon: triangles as index triples (also
/// counter-clockwise). Collinear vertices are dropped (the side faces still carry them).
fn triangulate(poly: &[Vec2]) -> Option<Vec<[usize; 3]>> {
    let n = poly.len();
    let (mut lo, mut hi) = (Vec2::new(f64::MAX, f64::MAX), Vec2::new(f64::MIN, f64::MIN));
    for p in poly {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    let diag = (hi - lo).len();
    let eps = 1e-12 * diag * diag;
    let mut idx: Vec<usize> = (0..n).collect();
    let mut tris: Vec<[usize; 3]> = Vec::with_capacity(n.saturating_sub(2));
    while idx.len() > 3 {
        let m = idx.len();
        let mut clipped = false;
        for k in 0..m {
            let (Some(&ip), Some(&ic), Some(&inx)) = (idx.get((k + m - 1) % m), idx.get(k), idx.get((k + 1) % m)) else { continue };
            let (Some(&p), Some(&c), Some(&q)) = (poly.get(ip), poly.get(ic), poly.get(inx)) else { continue };
            let turn = orient(p, c, q);
            if turn <= eps {
                continue;
            }
            let blocked = idx.iter().any(|&o| {
                o != ip
                    && o != ic
                    && o != inx
                    && poly.get(o).is_some_and(|&po| !po.near(p, EPS) && !po.near(q, EPS) && !po.near(c, EPS) && inside_or_on(p, c, q, po, eps))
            });
            if !blocked {
                tris.push([ip, ic, inx]);
                idx.remove(k);
                clipped = true;
                break;
            }
        }
        if clipped {
            continue;
        }
        // No ear: drop a collinear vertex, or give up on a polygon too degenerate to cut.
        let flat = (0..m).find(|&k| {
            let (Some(&ip), Some(&ic), Some(&inx)) = (idx.get((k + m - 1) % m), idx.get(k), idx.get((k + 1) % m)) else { return false };
            match (poly.get(ip), poly.get(ic), poly.get(inx)) {
                (Some(&p), Some(&c), Some(&q)) => orient(p, c, q).abs() <= eps,
                _ => false,
            }
        })?;
        idx.remove(flat);
    }
    if let [a, b, c] = idx[..]
        && let (Some(&p), Some(&q), Some(&r)) = (poly.get(a), poly.get(b), poly.get(c))
        && orient(p, q, r) > eps
    {
        tris.push([a, b, c]);
    }
    (!tris.is_empty()).then_some(tris)
}

fn is_convex(poly: &[Vec2]) -> bool {
    let n = poly.len();
    (0..n).all(|i| match (poly.get(i), poly.get((i + 1) % n), poly.get((i + 2) % n)) {
        (Some(&a), Some(&b), Some(&c)) => orient(a, b, c) > 0.0,
        _ => false,
    })
}

fn at(p: Vec2, z: f64) -> Vec3 {
    p.to3(z)
}

/// A prism of `poly` (simple, counter-clockwise) between `lo` and `hi`. With `fan` the caps are
/// triangle fans around that point (a cylinder), otherwise they are cut by ear clipping.
fn prism(poly: &[Vec2], lo: f64, hi: f64, fan: Option<Vec2>) -> Result<Vec<Face>, MeshError> {
    let n = poly.len();
    let mut faces: Vec<Face> = Vec::with_capacity(n * 3);
    for i in 0..n {
        let (Some(&a), Some(&b)) = (poly.get(i), poly.get((i + 1) % n)) else { continue };
        faces.push(Face { corners: [at(a, lo), at(b, lo), at(b, hi), at(a, hi)], hidden: 0 });
    }
    if let Some(c) = fan {
        for i in 0..n {
            let (Some(&a), Some(&b)) = (poly.get(i), poly.get((i + 1) % n)) else { continue };
            faces.push(Face { corners: [at(c, hi), at(a, hi), at(b, hi), at(b, hi)], hidden: 1 | 4 | 8 });
            faces.push(Face { corners: [at(c, lo), at(b, lo), at(a, lo), at(a, lo)], hidden: 1 | 4 | 8 });
        }
        return Ok(faces);
    }
    if n == 3 || (n == 4 && is_convex(poly)) {
        let q = |i: usize| poly.get(i).copied().unwrap_or(Vec2::ZERO);
        if n == 3 {
            faces.push(Face { corners: [at(q(0), hi), at(q(1), hi), at(q(2), hi), at(q(2), hi)], hidden: 4 });
            faces.push(Face { corners: [at(q(0), lo), at(q(2), lo), at(q(1), lo), at(q(1), lo)], hidden: 4 });
        } else {
            faces.push(Face { corners: [at(q(0), hi), at(q(1), hi), at(q(2), hi), at(q(3), hi)], hidden: 0 });
            faces.push(Face { corners: [at(q(0), lo), at(q(3), lo), at(q(2), lo), at(q(1), lo)], hidden: 0 });
        }
        return Ok(faces);
    }
    let tris = triangulate(poly).ok_or(MeshError::Profile("the profile cannot be split into faces"))?;
    let boundary = |u: usize, v: usize| v == (u + 1) % n || u == (v + 1) % n;
    let bit = |u: usize, v: usize, b: u8| if boundary(u, v) { 0 } else { b };
    for [i, j, k] in tris {
        let (Some(&pi), Some(&pj), Some(&pk)) = (poly.get(i), poly.get(j), poly.get(k)) else { continue };
        faces.push(Face { corners: [at(pi, hi), at(pj, hi), at(pk, hi), at(pk, hi)], hidden: bit(i, j, 1) | bit(j, k, 2) | 4 | bit(k, i, 8) });
        faces.push(Face { corners: [at(pi, lo), at(pk, lo), at(pj, lo), at(pj, lo)], hidden: bit(i, k, 1) | bit(k, j, 2) | 4 | bit(j, i, 8) });
    }
    Ok(faces)
}

fn z_range(z: f64, height: f64) -> Result<(f64, f64), MeshError> {
    if !z.is_finite() || !height.is_finite() {
        return Err(MeshError::Size("the height and position must be numbers"));
    }
    if height.abs() <= EPS * (1.0 + z.abs()) {
        return Err(MeshError::Size("the height must not be zero"));
    }
    Ok(if height > 0.0 { (z, z + height) } else { (z + height, z) })
}

/// The faces of `profile` extruded from elevation `z` by `height` along +Z (downwards when negative).
pub fn extrude(profile: &[Vec2], z: f64, height: f64) -> Result<Vec<Face>, MeshError> {
    let (lo, hi) = z_range(z, height)?;
    let poly = simple_profile(profile)?;
    prism(&poly, lo, hi, None)
}

/// The faces of an axis-aligned box from `corner` by `size` (each side may be negative, none zero).
pub fn box_faces(corner: Vec3, size: Vec3) -> Result<Vec<Face>, MeshError> {
    if !corner.is_finite() || !size.is_finite() {
        return Err(MeshError::Size("the corner and the size must be numbers"));
    }
    if size.x.abs() <= EPS || size.y.abs() <= EPS {
        return Err(MeshError::Size("the length and the width must not be zero"));
    }
    let (x0, x1) = (corner.x.min(corner.x + size.x), corner.x.max(corner.x + size.x));
    let (y0, y1) = (corner.y.min(corner.y + size.y), corner.y.max(corner.y + size.y));
    let rect = [Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)];
    extrude(&rect, corner.z, size.z)
}

/// Sides that keep a circle of `radius` within `tol` of the true curve, between 8 and
/// [`MAX_SEGMENTS`].
pub fn circle_segments(radius: f64, tol: f64) -> usize {
    crate::arc_segments(radius, TAU, tol).clamp(8, MAX_SEGMENTS)
}

/// The faces of a cylinder on `center` (the centre of the base), `segments` sides (3 to
/// [`MAX_SEGMENTS`]), `height` along +Z (downwards when negative).
pub fn cylinder(center: Vec3, radius: f64, height: f64, segments: usize) -> Result<Vec<Face>, MeshError> {
    if !center.is_finite() || !radius.is_finite() {
        return Err(MeshError::Size("the centre and the radius must be numbers"));
    }
    if radius <= EPS {
        return Err(MeshError::Size("the radius must be greater than zero"));
    }
    if !(3..=MAX_SEGMENTS).contains(&segments) {
        return Err(MeshError::Size("a cylinder needs between 3 and 1024 sides"));
    }
    let (lo, hi) = z_range(center.z, height)?;
    let c = center.xy();
    let poly: Vec<Vec2> = (0..segments).map(|i| Vec2::polar(c, radius, TAU * i as f64 / segments as f64)).collect();
    prism(&poly, lo, hi, Some(c))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PI;

    fn v(x: f64, y: f64) -> Vec2 {
        Vec2::new(x, y)
    }

    /// Volume by the divergence theorem over the faces (triangles and quads as fans).
    fn volume(faces: &[Face]) -> f64 {
        let mut vol = 0.0;
        for f in faces {
            let [a, b, c, d] = f.corners;
            vol += a.dot(b.cross(c)) / 6.0;
            if c != d {
                vol += a.dot(c.cross(d)) / 6.0;
            }
        }
        vol
    }

    #[test]
    fn box_has_six_outward_faces_and_the_right_volume() {
        let f = box_faces(Vec3::new(1.0, 2.0, 3.0), Vec3::new(4.0, 5.0, 6.0)).unwrap();
        assert_eq!(f.len(), 6);
        assert!((volume(&f) - 120.0).abs() < 1e-9, "{}", volume(&f));
    }

    #[test]
    fn negative_sizes_extend_the_other_way() {
        let f = box_faces(Vec3::new(0.0, 0.0, 0.0), Vec3::new(-2.0, -3.0, -4.0)).unwrap();
        assert!((volume(&f) - 24.0).abs() < 1e-9);
        let zs: Vec<f64> = f.iter().flat_map(|f| f.corners.iter().map(|c| c.z)).collect();
        assert!(zs.iter().all(|z| (-4.0..=0.0).contains(z)));
    }

    #[test]
    fn extrusion_of_an_l_shape_is_closed_and_hides_its_diagonals() {
        let l = [v(0.0, 0.0), v(4.0, 0.0), v(4.0, 1.0), v(1.0, 1.0), v(1.0, 3.0), v(0.0, 3.0)];
        let f = extrude(&l, 0.0, 2.0).unwrap();
        assert!((volume(&f) - 12.0).abs() < 1e-9, "{}", volume(&f));
        // 6 sides plus 2 caps of 4 triangles each.
        assert_eq!(f.len(), 6 + 8);
        assert!(f.iter().skip(6).all(|t| t.hidden & 4 != 0));
        assert!(f.iter().skip(6).any(|t| t.hidden & 11 != 0), "triangulation diagonals are hidden");
        // The same shape clockwise gives the same solid.
        let mut cw = l;
        cw.reverse();
        assert!((volume(&extrude(&cw, 0.0, 2.0).unwrap()) - 12.0).abs() < 1e-9);
    }

    #[test]
    fn extrusion_downwards_keeps_faces_outward() {
        let sq = [v(0.0, 0.0), v(2.0, 0.0), v(2.0, 2.0), v(0.0, 2.0)];
        let f = extrude(&sq, 5.0, -3.0).unwrap();
        assert!((volume(&f) - 12.0).abs() < 1e-9);
        assert!(f.iter().flat_map(|f| f.corners.iter()).all(|c| (2.0..=5.0).contains(&c.z)));
    }

    #[test]
    fn closing_point_and_repeats_are_dropped() {
        let sq = [v(0.0, 0.0), v(2.0, 0.0), v(2.0, 0.0), v(2.0, 2.0), v(0.0, 2.0), v(0.0, 0.0)];
        assert_eq!(simple_profile(&sq).unwrap().len(), 4);
    }

    #[test]
    fn bad_profiles_are_refused_not_panicked() {
        let bow = [v(0.0, 0.0), v(2.0, 2.0), v(2.0, 0.0), v(0.0, 2.0)];
        assert!(simple_profile(&bow).is_err());
        assert!(simple_profile(&[v(0.0, 0.0), v(1.0, 0.0)]).is_err());
        assert!(simple_profile(&[v(0.0, 0.0), v(1.0, 0.0), v(2.0, 0.0)]).is_err());
        assert!(simple_profile(&[v(0.0, 0.0), v(1.0, f64::NAN), v(2.0, 0.0)]).is_err());
        assert!(simple_profile(&[v(0.0, 0.0), v(f64::INFINITY, 0.0), v(2.0, 2.0)]).is_err());
        let spike = [v(0.0, 0.0), v(4.0, 0.0), v(2.0, 0.0), v(2.0, 2.0)];
        assert!(simple_profile(&spike).is_err());
        let touch = [v(0.0, 0.0), v(4.0, 0.0), v(4.0, 4.0), v(2.0, 0.0), v(0.0, 4.0)];
        assert!(simple_profile(&touch).is_err());
        let many = vec![v(0.0, 0.0); MAX_PROFILE + 1];
        assert!(simple_profile(&many).is_err());
    }

    #[test]
    fn zero_and_hostile_sizes_are_refused() {
        let sq = [v(0.0, 0.0), v(2.0, 0.0), v(2.0, 2.0), v(0.0, 2.0)];
        assert!(extrude(&sq, 0.0, 0.0).is_err());
        assert!(extrude(&sq, 0.0, f64::NAN).is_err());
        assert!(extrude(&sq, f64::INFINITY, 1.0).is_err());
        assert!(box_faces(Vec3::ZERO, Vec3::new(0.0, 1.0, 1.0)).is_err());
        assert!(box_faces(Vec3::ZERO, Vec3::new(1.0, 1.0, 0.0)).is_err());
        assert!(cylinder(Vec3::ZERO, 0.0, 1.0, 16).is_err());
        assert!(cylinder(Vec3::ZERO, -1.0, 1.0, 16).is_err());
        assert!(cylinder(Vec3::ZERO, 1.0, 1.0, 2).is_err());
        assert!(cylinder(Vec3::ZERO, 1.0, 1.0, MAX_SEGMENTS + 1).is_err());
        assert!(cylinder(Vec3::ZERO, f64::NAN, 1.0, 16).is_err());
        let _ = extrude(&sq, 1e300, 1e300);
    }

    #[test]
    fn cylinder_volume_approaches_pi_r2_h() {
        let f = cylinder(Vec3::new(1.0, 1.0, 1.0), 2.0, 3.0, 256).unwrap();
        assert_eq!(f.len(), 256 * 3);
        let want = PI * 4.0 * 3.0;
        assert!((volume(&f) - want).abs() / want < 1e-3, "{} vs {want}", volume(&f));
    }

    #[test]
    fn circle_segments_follows_the_tolerance() {
        let coarse = circle_segments(10.0, 0.1);
        let fine = circle_segments(10.0, 0.001);
        assert!(fine > coarse && coarse >= 8);
        assert_eq!(circle_segments(f64::NAN, 1.0), 8);
        assert!(circle_segments(1.0, 0.0) <= MAX_SEGMENTS);
        assert!(circle_segments(1.0, 1e-30) <= MAX_SEGMENTS);
        assert_eq!(circle_segments(1.0, 5.0), 8);
    }

    #[test]
    fn a_concave_polygon_with_collinear_points_still_triangulates() {
        let p = [v(0.0, 0.0), v(2.0, 0.0), v(4.0, 0.0), v(4.0, 4.0), v(2.0, 2.0), v(0.0, 4.0)];
        let f = extrude(&p, 0.0, 1.0).unwrap();
        assert!((volume(&f) - 12.0).abs() < 1e-9, "{}", volume(&f));
    }
}
