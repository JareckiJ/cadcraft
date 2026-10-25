//! Automatic dimension breaks follow their source objects after geometry and dimension
//! associativity have settled. The update belongs to the editing command's undo step.

use std::sync::Arc;

use cadcraft_doc::{AutoDimBreak, DimBreak, Dimension, Drawing, EntityKind, Handle, MAX_DIM_BREAK_OBJECTS, Prim, Space};
use cadcraft_geom::{Line, Segment, Vec2, Vec3};

/// Bound both the output and the intersection work of one dimension, including hostile files.
const MAX_AUTO_BREAKS: usize = 1000;
const MAX_INTERSECTION_TESTS: usize = 2_000_000;

/// Gaps made by the selected source objects in this dimension's own space. Manual gaps do
/// not hide intersections from Auto; neither do automatic gaps from the previous edit.
pub(crate) fn automatic(d: &Drawing, space: &Space, dm: &Dimension, auto: &AutoDimBreak) -> Vec<DimBreak> {
    if !auto.size.is_finite() || auto.size <= 0.0 {
        return Vec::new();
    }
    let Some(store) = d.space(space) else { return Vec::new() };
    let mut plain = dm.clone();
    plain.marks.breaks.clear();
    plain.marks.auto_break = None;
    let geometry = cadcraft_render::dimension_in(d, &plain);
    let mut points: Vec<Vec2> = Vec::new();
    let mut tests = 0usize;
    'objects: for h in auto.objects.iter().take(MAX_DIM_BREAK_OBJECTS) {
        let Some(e) = store.get(*h) else { continue };
        for p in e.kind.prims() {
            for line in &geometry.lines {
                for w in line.windows(2) {
                    if tests >= MAX_INTERSECTION_TESTS || points.len() >= MAX_AUTO_BREAKS {
                        break 'objects;
                    }
                    tests += 1;
                    let (Some(a), Some(b)) = (w.first().copied(), w.get(1).copied()) else { continue };
                    let seg = Line::new(a, b);
                    let hits = match &p {
                        Prim::Seg(other) => cadcraft_geom::intersect_segments(&Segment::Line(seg), other),
                        Prim::Circle(c) => cadcraft_geom::line_circle(&seg, c)
                            .into_iter()
                            .filter(|(_, t)| (-1e-9..=1.0 + 1e-9).contains(t))
                            .map(|(q, _)| q)
                            .collect(),
                        Prim::Infinite { base, dir, ray } => cadcraft_geom::line_line_infinite(a, b, *base, *base + *dir)
                            .filter(|(_, t, u)| (-1e-9..=1.0 + 1e-9).contains(t) && (!ray || *u >= -1e-9))
                            .map(|(q, _, _)| vec![q])
                            .unwrap_or_default(),
                        _ => Vec::new(),
                    };
                    for q in hits {
                        if q.is_finite() && !points.iter().any(|o| o.dist(q) < 1e-9) {
                            points.push(q);
                            if points.len() >= MAX_AUTO_BREAKS {
                                break 'objects;
                            }
                        }
                    }
                }
            }
        }
    }
    points.into_iter().map(|p| DimBreak { at: Vec3::new(p.x, p.y, 0.0), size: auto.size, auto: true }).collect()
}

/// Refresh only relationships whose dimension, style or source geometry changed. In
/// particular, unrelated edits do not rebuild every dimension's rendered geometry.
pub(crate) fn update(after: &mut Arc<Drawing>, before: &Drawing) {
    let d: &Drawing = after;
    let styles_changed = before.dim_styles != d.dim_styles
        || before.text_styles != d.text_styles
        || before.header.f64("DIMSCALE", 1.0) != d.header.f64("DIMSCALE", 1.0);
    let stores = std::iter::once((Space::Model, &d.model)).chain(d.layouts.iter().map(|l| (Space::Paper(l.name.clone()), &l.entities)));
    let mut changes: Vec<(Handle, Dimension)> = Vec::new();
    for (space, store) in stores {
        let old_store = before.space(&space);
        if !styles_changed && old_store.is_some_and(|old| old.same_as(store)) {
            continue;
        }
        for e in store.iter() {
            let EntityKind::Dimension(dm) = &e.kind else { continue };
            let Some(auto) = &dm.marks.auto_break else { continue };
            let touched = styles_changed
                || old_store.and_then(|s| s.get(e.handle)).is_none_or(|old| !Arc::ptr_eq(old, e) && old.kind != e.kind)
                || auto.objects.iter().take(MAX_DIM_BREAK_OBJECTS).any(|h| match (old_store.and_then(|s| s.get(*h)), store.get(*h)) {
                    (Some(a), Some(b)) => !Arc::ptr_eq(a, b) && a.kind != b.kind,
                    (None, None) => false,
                    _ => true,
                });
            if !touched {
                continue;
            }
            let mut next = dm.clone();
            next.marks.breaks.retain(|b| !b.auto);
            next.marks.breaks.extend(automatic(d, &space, dm, auto));
            if next.marks.breaks != dm.marks.breaks {
                next.block = None;
                changes.push((e.handle, next));
            }
        }
    }
    if !changes.is_empty() {
        let d = Arc::make_mut(after);
        for (h, next) in changes {
            let _ = d.modify_entity(h, |e| e.kind = EntityKind::Dimension(next));
        }
    }
}
