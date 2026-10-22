//! The outline of a filled primitive, for the selection and hover highlight: the boundary of the
//! region its triangles cover, chained into polylines so a dash pattern runs along it.
//!
//! An edge two triangles share is inside the region; an edge only one triangle has is on its
//! boundary. Triangulations from lyon share edges exactly. The slab filler
//! ([`crate::fill::triangulate_evenodd`]) cuts at horizontal lines where the trapezoids above and
//! below need not end at the same x, so horizontal edges are split at every x on their line and
//! a piece is boundary when an odd number of edges cover it.

use std::collections::HashMap;

use cadcraft_geom::{Bounds2, Vec2};

/// Triangle vertices beyond which the outline is the bounding box (a hatch or wide polyline
/// covering the screen does not need its exact boundary to show it is selected).
const MAX_VERTS: usize = 600_000;

/// Closed polylines (first point repeated last) around the region `tris` covers, three vertices
/// per triangle. Empty for no triangles; the bounding box for very large inputs.
pub fn fill_outlines(tris: &[Vec2]) -> Vec<Vec<Vec2>> {
    if tris.len() > MAX_VERTS {
        let b = Bounds2::from_points(tris.iter().copied().filter(|p| p.is_finite()));
        if b.is_empty() {
            return Vec::new();
        }
        return vec![vec![b.min, Vec2::new(b.max.x, b.min.y), b.max, Vec2::new(b.min.x, b.max.y), b.min]];
    }
    let mut edges: HashMap<(Key, Key), u32> = HashMap::new();
    let mut flat: HashMap<u64, Vec<(f64, f64)>> = HashMap::new();
    for [p, q, r] in tris.as_chunks::<3>().0 {
        for (a, b) in [(*p, *q), (*q, *r), (*r, *p)] {
            let (a, b) = (norm(a), norm(b));
            if !(a.is_finite() && b.is_finite()) || key(a) == key(b) {
                continue;
            }
            if a.y.to_bits() == b.y.to_bits() {
                flat.entry(a.y.to_bits()).or_default().push((a.x.min(b.x), a.x.max(b.x)));
            } else {
                let (ka, kb) = (key(a), key(b));
                *edges.entry(if ka < kb { (ka, kb) } else { (kb, ka) }).or_default() += 1;
            }
        }
    }
    let mut segs: Vec<(Vec2, Vec2)> = edges.into_iter().filter(|(_, n)| n % 2 == 1).map(|((a, b), _)| (unkey(a), unkey(b))).collect();
    for (y, spans) in flat {
        segs.extend(odd_cover(&spans).into_iter().map(|(x0, x1)| (Vec2::new(x0, f64::from_bits(y)), Vec2::new(x1, f64::from_bits(y)))));
    }
    chain(segs)
}

/// Closed contours whose nonzero winding covers exactly what `tris` covers (three vertices per
/// triangle), for writing a fill as one path (PDF, SVG) instead of one subpath per triangle.
///
/// Every triangle is turned counter-clockwise, so the winding number at a point is the number of
/// triangles covering it and overlapping triangles (wide polyline joins) stay covered. An edge
/// two triangles share in opposite directions cancels; the slab filler's split horizontal edges
/// cancel piece by piece, cut at every vertex on their line so each contour can turn there.
/// `None` for very large inputs or if the edges do not close into contours: the caller keeps
/// the triangles. Contours are in a canonical order, so the output does not vary between runs.
pub fn fill_contours(tris: &[Vec2]) -> Option<Vec<Vec<Vec2>>> {
    if tris.len() > MAX_VERTS {
        return None;
    }
    let mut edges: HashMap<(Key, Key), i64> = HashMap::new();
    let mut flat: HashMap<u64, Vec<(f64, f64, i64)>> = HashMap::new();
    let mut verts: Vec<Vec2> = Vec::with_capacity(tris.len());
    for [p, q, r] in tris.as_chunks::<3>().0 {
        let (p, q, r) = (norm(*p), norm(*q), norm(*r));
        let area = (q - p).cross(r - p);
        if !(p.is_finite() && q.is_finite() && r.is_finite() && area.is_finite()) || area == 0.0 {
            continue;
        }
        let (q, r) = if area > 0.0 { (q, r) } else { (r, q) };
        verts.extend([p, q, r]);
        for (a, b) in [(p, q), (q, r), (r, p)] {
            if a.y.to_bits() == b.y.to_bits() {
                let dir = if a.x < b.x { 1 } else { -1 };
                flat.entry(a.y.to_bits()).or_default().push((a.x.min(b.x), a.x.max(b.x), dir));
            } else {
                let (ka, kb) = (key(a), key(b));
                if ka < kb {
                    *edges.entry((ka, kb)).or_default() += 1;
                } else {
                    *edges.entry((kb, ka)).or_default() -= 1;
                }
            }
        }
    }
    let mut cuts: HashMap<u64, Vec<f64>> = HashMap::new();
    for v in &verts {
        if flat.contains_key(&v.y.to_bits()) {
            cuts.entry(v.y.to_bits()).or_default().push(v.x);
        }
    }
    let mut segs: Vec<(Vec2, Vec2)> = Vec::new();
    let mut push = |a: Vec2, b: Vec2, n: i64| {
        let (a, b) = if n > 0 { (a, b) } else { (b, a) };
        for _ in 0..n.unsigned_abs() {
            segs.push((a, b));
        }
    };
    for ((ka, kb), n) in edges {
        push(unkey(ka), unkey(kb), n);
    }
    for (y, spans) in flat {
        let at = cuts.get(&y).map_or(&[][..], Vec::as_slice);
        for (x0, x1, n) in net_cover(&spans, at) {
            let y = f64::from_bits(y);
            push(Vec2::new(x0, y), Vec2::new(x1, y), n);
        }
    }
    close(segs)
}

/// Net signed coverage of a line by directed `spans` (`dir` +1 left to right, -1 right to left),
/// one piece per stretch between consecutive span ends or `cuts`, zero-coverage pieces left out.
fn net_cover(spans: &[(f64, f64, i64)], cuts: &[f64]) -> Vec<(f64, f64, i64)> {
    let mut xs: Vec<f64> = spans.iter().flat_map(|&(a, b, _)| [a, b]).chain(cuts.iter().copied()).collect();
    xs.sort_by(f64::total_cmp);
    xs.dedup_by(|a, b| a.to_bits() == b.to_bits());
    let mut delta = vec![0i64; xs.len()];
    for &(a, b, dir) in spans {
        let (Ok(i), Ok(j)) = (xs.binary_search_by(|x| x.total_cmp(&a)), xs.binary_search_by(|x| x.total_cmp(&b))) else { continue };
        if let Some(d) = delta.get_mut(i) {
            *d += dir;
        }
        if let Some(d) = delta.get_mut(j) {
            *d -= dir;
        }
    }
    let mut out = Vec::new();
    let mut cover = 0i64;
    for (w, d) in xs.windows(2).zip(&delta) {
        cover += d;
        if let [x0, x1] = *w
            && cover != 0
        {
            out.push((x0, x1, cover));
        }
    }
    out
}

/// Walks directed segments into closed contours; `None` if a walk cannot get back to its start.
fn close(mut segs: Vec<(Vec2, Vec2)>) -> Option<Vec<Vec<Vec2>>> {
    let cmp = |a: Vec2, b: Vec2| a.x.total_cmp(&b.x).then_with(|| a.y.total_cmp(&b.y));
    segs.sort_unstable_by(|(a, b), (c, d)| cmp(*a, *c).then_with(|| cmp(*b, *d)));
    let mut from: HashMap<Key, Vec<usize>> = HashMap::new();
    for (i, (a, _)) in segs.iter().enumerate() {
        from.entry(key(*a)).or_default().push(i);
    }
    let mut used = vec![false; segs.len()];
    let mut out = Vec::new();
    for start in 0..segs.len() {
        if used.get(start).copied().unwrap_or(true) {
            continue;
        }
        let Some(&(a, b)) = segs.get(start) else { continue };
        if let Some(u) = used.get_mut(start) {
            *u = true;
        }
        let mut line = vec![a, b];
        let mut end = b;
        // Each step uses one segment, so the walk ends within `segs.len()` steps.
        while key(end) != key(a) {
            let next = from.get(&key(end)).and_then(|ids| ids.iter().copied().find(|&i| !used.get(i).copied().unwrap_or(true)))?;
            let &(_, q) = segs.get(next)?;
            if let Some(u) = used.get_mut(next) {
                *u = true;
            }
            end = q;
            line.push(q);
        }
        out.push(straighten(line));
    }
    Some(out)
}

/// Drops the points of a contour that lie on the straight line through their neighbours: the
/// slab filler cuts every edge of a fill at every vertex height of the whole fill, so a line of
/// text comes back as glyph edges with hundreds of points each. The covered region is unchanged.
fn straighten(line: Vec<Vec2>) -> Vec<Vec2> {
    let mut out: Vec<Vec2> = Vec::with_capacity(line.len());
    for p in line {
        while let (Some(&b), Some(&a)) = (out.last(), out.len().checked_sub(2).and_then(|i| out.get(i))) {
            let (u, v) = (b - a, p - b);
            if u.cross(v).abs() > STRAIGHT * u.len() * v.len() {
                break;
            }
            out.pop();
        }
        out.push(p);
    }
    out
}

/// The sine of the turn below which a contour point counts as on a straight line.
const STRAIGHT: f64 = 1e-9;

type Key = (u64, u64);

/// -0.0 and 0.0 must meet.
fn norm(p: Vec2) -> Vec2 {
    Vec2::new(p.x + 0.0, p.y + 0.0)
}

fn key(p: Vec2) -> Key {
    (p.x.to_bits(), p.y.to_bits())
}

fn unkey(k: Key) -> Vec2 {
    Vec2::new(f64::from_bits(k.0), f64::from_bits(k.1))
}

/// The parts of a line covered by an odd number of `spans`, merged into maximal runs.
fn odd_cover(spans: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut xs: Vec<f64> = spans.iter().flat_map(|&(a, b)| [a, b]).collect();
    xs.sort_by(f64::total_cmp);
    xs.dedup_by(|a, b| a.to_bits() == b.to_bits());
    // Coverage changes only at span ends: +1 where a span starts, -1 where one ends.
    let mut delta = vec![0i64; xs.len()];
    for &(a, b) in spans {
        let (Ok(i), Ok(j)) = (xs.binary_search_by(|x| x.total_cmp(&a)), xs.binary_search_by(|x| x.total_cmp(&b))) else { continue };
        if let Some(d) = delta.get_mut(i) {
            *d += 1;
        }
        if let Some(d) = delta.get_mut(j) {
            *d -= 1;
        }
    }
    let mut out: Vec<(f64, f64)> = Vec::new();
    let mut cover = 0i64;
    for (w, d) in xs.windows(2).zip(&delta) {
        cover += d;
        let [x0, x1] = *w else { continue };
        if cover % 2 != 0 {
            match out.last_mut() {
                Some(last) if last.1.to_bits() == x0.to_bits() => last.1 = x1,
                _ => out.push((x0, x1)),
            }
        }
    }
    out
}

/// Joins segments that share end points into polylines (closed loops come back to their start).
fn chain(mut segs: Vec<(Vec2, Vec2)>) -> Vec<Vec<Vec2>> {
    // Hash-map iteration must not choose a contour's starting point or direction: each
    // redraw would shift the dash phase. Canonicalize endpoints and traversal order using
    // coordinates, so translating a region keeps the same starts and directions too.
    let cmp = |a: Vec2, b: Vec2| a.x.total_cmp(&b.x).then_with(|| a.y.total_cmp(&b.y));
    for (a, b) in &mut segs {
        if cmp(*a, *b).is_gt() {
            std::mem::swap(a, b);
        }
    }
    segs.sort_unstable_by(|(a, b), (c, d)| cmp(*a, *c).then_with(|| cmp(*b, *d)));
    let mut at: HashMap<Key, Vec<usize>> = HashMap::new();
    for (i, (a, b)) in segs.iter().enumerate() {
        at.entry(key(*a)).or_default().push(i);
        at.entry(key(*b)).or_default().push(i);
    }
    let mut used = vec![false; segs.len()];
    let mut out = Vec::new();
    for start in 0..segs.len() {
        if used.get(start).copied().unwrap_or(true) {
            continue;
        }
        let Some(&(a, b)) = segs.get(start) else { continue };
        if let Some(u) = used.get_mut(start) {
            *u = true;
        }
        let mut line = vec![a, b];
        let mut end = b;
        // Each step uses one segment, so the walk ends within `segs.len()` steps.
        loop {
            let next = at.get(&key(end)).and_then(|ids| ids.iter().copied().find(|&i| !used.get(i).copied().unwrap_or(true)));
            let Some(i) = next else { break };
            let Some(&(p, q)) = segs.get(i) else { break };
            if let Some(u) = used.get_mut(i) {
                *u = true;
            }
            end = if key(p) == key(end) { q } else { p };
            line.push(end);
        }
        out.push(line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn total_len(lines: &[Vec<Vec2>]) -> f64 {
        lines.iter().flat_map(|l| l.windows(2).map(|w| w[0].dist(w[1]))).sum()
    }

    #[test]
    fn a_square_of_two_triangles_is_one_loop() {
        let (a, b, c, d) = (Vec2::new(0.0, 0.0), Vec2::new(2.0, 0.0), Vec2::new(2.0, 1.0), Vec2::new(0.0, 1.0));
        let lines = fill_outlines(&[a, b, c, a, c, d]);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines.first().map(Vec::len), Some(5), "closed: four corners and back");
        assert!((total_len(&lines) - 6.0).abs() < 1e-12, "the diagonal is inside");
    }

    #[test]
    fn a_ring_from_the_slab_filler_has_two_clean_loops() {
        let ring = |r: f64| (0..32).map(|i| Vec2::polar(Vec2::ZERO, r, f64::from(i) / 32.0 * std::f64::consts::TAU)).collect::<Vec<_>>();
        let tris = crate::fill::triangulate_evenodd(&[ring(10.0), ring(6.0)]);
        let lines = fill_outlines(&tris);
        let perimeter = |r: f64| 32.0 * 2.0 * r * (std::f64::consts::PI / 32.0).sin();
        let expected = perimeter(10.0) + perimeter(6.0);
        assert!((total_len(&lines) - expected).abs() < 1e-6, "{} vs {expected}: no slab cuts left inside", total_len(&lines));
        assert!(lines.iter().all(|l| l.first().zip(l.last()).is_some_and(|(a, b)| a.dist(*b) < 1e-9)), "loops close");
    }

    #[test]
    fn contours_keep_their_start_direction_and_order_between_calls() {
        let p = |x, y| Vec2::new(x, y);
        // An outline-font-like hole and a separate filled island, triangulated by slabs.
        let tris = crate::fill::triangulate_evenodd(&[
            vec![p(-4.0, -3.0), p(5.0, -3.0), p(5.0, 6.0), p(-4.0, 6.0)],
            vec![p(-2.0, -1.0), p(3.0, -1.0), p(3.0, 4.0), p(-2.0, 4.0)],
            vec![p(8.0, 0.0), p(11.0, 0.0), p(11.0, 2.0), p(8.0, 2.0)],
        ]);
        let expected = fill_outlines(&tris);
        assert_eq!(expected.len(), 3);
        for _ in 0..64 {
            assert_eq!(fill_outlines(&tris), expected, "a repaint must not move the dash origin or reverse its direction");
        }
        let reversed: Vec<Vec2> = tris.as_chunks::<3>().0.iter().rev().flat_map(|tri| tri.iter().rev().copied()).collect();
        assert_eq!(fill_outlines(&reversed), expected, "triangle order and winding do not move the contour's dash origin");
        let shift = p(100.0, 100.0);
        let moved: Vec<_> = tris.iter().map(|q| *q + shift).collect();
        let moved_expected: Vec<Vec<_>> = expected.iter().map(|line| line.iter().map(|q| *q + shift).collect()).collect();
        assert_eq!(fill_outlines(&moved), moved_expected, "translation preserves the contour's relative dash anchor");
    }

    /// Winding number of `p` against closed contours (first point repeated last).
    fn winding(contours: &[Vec<Vec2>], p: Vec2) -> i64 {
        let mut w = 0;
        for c in contours {
            for s in c.windows(2) {
                let (a, b) = (s[0], s[1]);
                let side = (b - a).cross(p - a);
                if a.y <= p.y && b.y > p.y && side > 0.0 {
                    w += 1;
                } else if a.y > p.y && b.y <= p.y && side < 0.0 {
                    w -= 1;
                }
            }
        }
        w
    }

    fn covered(tris: &[Vec2], p: Vec2) -> i64 {
        tris.as_chunks::<3>().0.iter().filter(|t| winding(&[vec![t[0], t[1], t[2], t[0]]], p) != 0).count() as i64
    }

    #[test]
    fn contours_of_a_square_are_one_counter_clockwise_loop() {
        let (a, b, c, d) = (Vec2::new(0.0, 0.0), Vec2::new(2.0, 0.0), Vec2::new(2.0, 1.0), Vec2::new(0.0, 1.0));
        // One triangle each way round: winding is normalized.
        let contours = fill_contours(&[a, b, c, a, d, c]).unwrap_or_default();
        assert_eq!(contours.len(), 1);
        assert_eq!(contours.first().map(Vec::len), Some(5), "four corners and back, no diagonal");
        assert_eq!(winding(&contours, Vec2::new(1.0, 0.5)), 1);
    }

    #[test]
    fn contours_of_a_slab_filled_ring_keep_the_hole_and_drop_the_slab_cuts() {
        let ring = |r: f64| (0..32).map(|i| Vec2::polar(Vec2::ZERO, r, f64::from(i) / 32.0 * std::f64::consts::TAU)).collect::<Vec<_>>();
        let tris = crate::fill::triangulate_evenodd(&[ring(10.0), ring(6.0)]);
        let contours = fill_contours(&tris).unwrap_or_default();
        let perimeter = |r: f64| 32.0 * 2.0 * r * (std::f64::consts::PI / 32.0).sin();
        let expected = perimeter(10.0) + perimeter(6.0);
        assert!((total_len(&contours) - expected).abs() < 1e-6, "{} vs {expected}", total_len(&contours));
        for (p, n) in [(Vec2::new(8.0, 0.3), 1), (Vec2::new(0.1, 0.2), 0), (Vec2::new(11.0, 0.0), 0), (Vec2::new(-7.9, -0.4), 1)] {
            assert_eq!(winding(&contours, p), n, "at {p:?}");
        }
        assert!(contours.iter().map(Vec::len).sum::<usize>() < tris.len() / 2, "far fewer points than the triangles");
    }

    #[test]
    fn contours_drop_the_cuts_other_loops_put_on_straight_edges() {
        let p = |x, y| Vec2::new(x, y);
        // Like glyphs in one text hatch: each square's edges are cut at the other's heights.
        let tris = crate::fill::triangulate_evenodd(&[
            vec![p(0.0, 0.0), p(2.0, 0.0), p(2.0, 3.0), p(0.0, 3.0)],
            vec![p(5.0, 1.0), p(7.0, 1.0), p(7.0, 2.5), p(5.0, 2.5)],
            vec![p(9.0, 0.5), p(10.0, 0.5), p(10.0, 1.7), p(9.0, 1.7)],
        ]);
        let contours = fill_contours(&tris).unwrap_or_default();
        assert_eq!(contours.len(), 3);
        assert!(contours.iter().all(|c| c.len() == 5), "four corners and back each: {contours:?}");
    }

    #[test]
    fn contours_keep_overlapping_triangles_covered() {
        let p = |x, y| Vec2::new(x, y);
        // Two overlapping quads, as a wide polyline's joint draws them.
        let tris = [
            p(0.0, 0.0),
            p(4.0, 0.0),
            p(4.0, 2.0),
            p(0.0, 0.0),
            p(4.0, 2.0),
            p(0.0, 2.0),
            p(3.0, 1.0),
            p(6.0, 1.0),
            p(6.0, 3.0),
            p(3.0, 1.0),
            p(6.0, 3.0),
            p(3.0, 3.0),
        ];
        let contours = fill_contours(&tris).unwrap_or_default();
        for q in [p(1.0, 1.0), p(3.5, 1.5), p(5.0, 2.5), p(5.0, 0.5), p(1.0, 2.5), p(-1.0, 1.0)] {
            assert_eq!(winding(&contours, q) != 0, covered(&tris, q) > 0, "at {q:?}");
        }
        assert_eq!(winding(&contours, p(3.5, 1.5)), 2, "doubly covered, still filled under nonzero");
    }

    #[test]
    fn contours_do_not_vary_between_calls() {
        let p = |x, y| Vec2::new(x, y);
        let tris = crate::fill::triangulate_evenodd(&[
            vec![p(-4.0, -3.0), p(5.0, -3.0), p(5.0, 6.0), p(-4.0, 6.0)],
            vec![p(-2.0, -1.0), p(3.0, -1.0), p(3.0, 4.0), p(-2.0, 4.0)],
        ]);
        let first = fill_contours(&tris);
        assert!(first.is_some());
        for _ in 0..32 {
            assert_eq!(fill_contours(&tris), first);
        }
    }

    #[test]
    fn contours_of_hostile_inputs_do_not_panic() {
        assert_eq!(fill_contours(&[]), Some(Vec::new()));
        let nan = Vec2::new(f64::NAN, 1.0);
        let _ = fill_contours(&[nan, Vec2::ZERO, Vec2::new(1.0, 1.0), Vec2::ZERO, Vec2::ZERO]);
        let _ = fill_contours(&[Vec2::new(f64::INFINITY, 0.0), Vec2::new(-0.0, 0.0), Vec2::new(0.0, -0.0)]);
        let _ = fill_contours(&[Vec2::new(1e308, 0.0), Vec2::new(-1e308, 1.0), Vec2::new(1e308, 1e308)]);
        assert_eq!(fill_contours(&vec![Vec2::new(1.0, 2.0); MAX_VERTS + 3]), None, "too large: keep the triangles");
    }

    #[test]
    fn huge_and_hostile_inputs_do_not_panic() {
        assert!(fill_outlines(&[]).is_empty());
        let nan = Vec2::new(f64::NAN, 1.0);
        let _ = fill_outlines(&[nan, Vec2::ZERO, Vec2::new(1.0, 1.0), Vec2::ZERO, Vec2::ZERO]);
        let _ = fill_outlines(&[Vec2::new(f64::INFINITY, 0.0), Vec2::new(-0.0, 0.0), Vec2::new(0.0, -0.0)]);
        let big = vec![Vec2::new(1.0, 2.0); MAX_VERTS + 3];
        assert_eq!(fill_outlines(&big).len(), 1, "falls back to the bounding box");
    }
}
