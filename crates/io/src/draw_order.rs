//! Draw order in the written file (DXF Reference, SORTENTSTABLE). CADCraft keeps a space's draw
//! order as the order of its entities (DRAWORDER moves them; reading applies the file's table).
//! The writer puts the entity records in that order; [`finish`] adds, for each space whose order
//! isn't ascending handle order, a SORTENTSTABLE in its block record's extension dictionary, so
//! programs that draw by the table (AutoCAD among them) draw the same order. The table lists each
//! entity whose place doesn't match its own handle, with the handle at that place in ascending
//! order as its sort handle: the same handle swap DRAWORDER makes in AutoCAD's own files.

use std::collections::{HashMap, HashSet};

use cadcraft_doc::Handle;
use cadcraft_dxf::Tag;

use crate::dxf_raw::{head, max_handle, seed, seed_index, split, val};

/// Records in the ENTITIES and BLOCKS sections that aren't entities of a space: structure, and
/// sub-entities (drawn with the entity that owns them).
const NOT_DRAWN: &[&str] = &["SECTION", "ENDSEC", "BLOCK", "ENDBLK", "SEQEND", "VERTEX", "ATTRIB", "EOF"];

/// The extension dictionary entry AutoCAD looks a block record's table up by.
const KEY: &str = "ACAD_SORTENTS";

/// A space's table: the entities whose sort handle isn't their own, as (entity, sort handle).
fn table(order: &[Handle]) -> Option<Vec<(Handle, Handle)>> {
    let mut sorted = order.to_vec();
    sorted.sort_unstable();
    // A handle twice (or none) can't be ordered by handle: no table rather than a wrong one.
    if sorted.windows(2).any(|w| matches!(w, [a, b] if a == b)) || sorted.first().is_some_and(|h| h.0 == 0) {
        return None;
    }
    let moved: Vec<_> = order.iter().zip(&sorted).filter(|(e, k)| e != k).map(|(e, k)| (*e, *k)).collect();
    (!moved.is_empty()).then_some(moved)
}

/// Where a record's owner (330) is, outside its `102` groups (the record's end without one).
fn owner_at(body: &[Tag]) -> usize {
    let mut in_group = false;
    for (i, t) in body.iter().enumerate() {
        match t.code {
            102 => in_group = t.str().trim().starts_with('{'),
            330 if !in_group => return i,
            _ => {}
        }
    }
    body.len()
}

/// Add the SORTENTSTABLE of every space drawn out of handle order to the finished tags (after
/// `dxf_raw::finish`, which places the entities of other programs' types and their extension
/// dictionaries). A file without an OBJECTS section has nowhere to keep one.
pub(crate) fn finish(tags: &mut Vec<Tag>) {
    let mut sec = String::new();
    // Block record handle → (index of its `0` group, its extension dictionary, groups after it).
    let mut records: HashMap<String, (usize, Option<String>, usize)> = HashMap::new();
    // Owner handle → its entities' handles in the order written; owners in first-seen order.
    let mut spaces: HashMap<String, Vec<Handle>> = HashMap::new();
    let mut owners: Vec<String> = Vec::new();
    // DICTIONARY handle → (index of its `0` group, groups after it).
    let mut dictionaries: HashMap<String, (usize, usize)> = HashMap::new();
    let (mut objects_end, mut classes_end, mut has_class) = (None, None, false);
    for (kind, start, body) in split(tags) {
        match (kind.as_str(), sec.as_str()) {
            ("SECTION", _) => sec = val(body, 2).unwrap_or_default(),
            ("ENDSEC", s) => {
                match s {
                    "OBJECTS" => objects_end = Some(start),
                    "CLASSES" => classes_end = Some(start),
                    _ => {}
                }
                sec.clear();
            }
            ("CLASS", "CLASSES") => has_class |= val(body, 1).is_some_and(|n| n.trim().eq_ignore_ascii_case("SORTENTSTABLE")),
            ("BLOCK_RECORD", "TABLES") => {
                let h = head(body);
                if let Some(handle) = h.handle {
                    records.insert(handle, (start, h.xdict, body.len()));
                }
            }
            ("DICTIONARY", "OBJECTS") => {
                if let Some(handle) = head(body).handle {
                    dictionaries.entry(handle).or_insert((start, body.len()));
                }
            }
            (k, "ENTITIES" | "BLOCKS") if !NOT_DRAWN.contains(&k) => {
                let h = head(body);
                if let (Some(e), Some(o)) = (h.handle.as_deref().and_then(Handle::parse_hex), h.owner) {
                    if !spaces.contains_key(&o) {
                        owners.push(o.clone());
                    }
                    spaces.entry(o).or_default().push(e);
                }
            }
            _ => {}
        }
    }
    let Some(objects_end) = objects_end else { return };
    let mut next = seed(tags).max(max_handle(tags).saturating_add(1));
    let mut alloc = || {
        let h = Handle(next);
        next = next.saturating_add(1);
        h.hex()
    };
    // Insertions (index, groups), applied from the last index back; new objects go at the end.
    let mut inserts: Vec<(usize, Vec<Tag>)> = Vec::new();
    let mut objects: Vec<Tag> = Vec::new();
    let mut extended: HashSet<String> = HashSet::new();
    for owner in &owners {
        let Some(&(start, ref xdict, len)) = records.get(owner) else { continue };
        let Some(moved) = spaces.get(owner).and_then(|o| table(o)) else { continue };
        let table_h = alloc();
        let xd = match xdict {
            // The record's own extension dictionary gets the entry (once: a record has one table).
            Some(x) => {
                let Some(&(dstart, dlen)) = dictionaries.get(x) else { continue };
                if !extended.insert(x.clone()) {
                    continue;
                }
                let body = tags.get(dstart + 1..dstart + 1 + dlen).unwrap_or(&[]);
                let at = dstart + 1 + body.iter().position(|t| t.code >= 1000).unwrap_or(body.len());
                inserts.push((at, vec![Tag::s(3, KEY), Tag::s(360, table_h.clone())]));
                x.clone()
            }
            None => {
                let x = alloc();
                // The group goes before the record's owner (DXF Reference, common group codes).
                let body = tags.get(start + 1..start + 1 + len).unwrap_or(&[]);
                let at = start + 1 + owner_at(body);
                inserts.push((at, vec![Tag::s(102, "{ACAD_XDICTIONARY"), Tag::s(360, x.clone()), Tag::s(102, "}")]));
                objects.extend([
                    Tag::s(0, "DICTIONARY"),
                    Tag::s(5, x.clone()),
                    Tag::s(102, "{ACAD_REACTORS"),
                    Tag::s(330, owner.clone()),
                    Tag::s(102, "}"),
                    Tag::s(330, owner.clone()),
                    Tag::s(100, "AcDbDictionary"),
                    Tag::i(280, 1),
                    Tag::i(281, 1),
                    Tag::s(3, KEY),
                    Tag::s(360, table_h.clone()),
                ]);
                x
            }
        };
        objects.extend([
            Tag::s(0, "SORTENTSTABLE"),
            Tag::s(5, table_h),
            Tag::s(102, "{ACAD_REACTORS"),
            Tag::s(330, xd.clone()),
            Tag::s(102, "}"),
            Tag::s(330, xd),
            Tag::s(100, "AcDbSortentsTable"),
            Tag::s(330, owner.clone()),
        ]);
        for (e, k) in moved {
            objects.extend([Tag::s(331, e.hex()), Tag::s(5, k.hex())]);
        }
    }
    if objects.is_empty() {
        return;
    }
    // First among insertions at the same place: one spliced later goes before it, so a dictionary
    // entry for the last record of the section stays in that record, ahead of the new objects.
    inserts.insert(0, (objects_end, objects));
    if let (false, Some(at)) = (has_class, classes_end) {
        inserts.push((
            at,
            vec![
                Tag::s(0, "CLASS"),
                Tag::s(1, "SORTENTSTABLE"),
                Tag::s(2, "AcDbSortentsTable"),
                Tag::s(3, "ObjectDBX Classes"),
                Tag::i(90, 0),
                Tag::i(280, 0),
                Tag::i(281, 0),
            ],
        ));
    }
    inserts.sort_by_key(|(at, _)| std::cmp::Reverse(*at)); // stable: keeps that order at a place
    for (at, groups) in inserts {
        let at = at.min(tags.len());
        tags.splice(at..at, groups);
    }
    if let Some(i) = seed_index(tags)
        && let Some(t) = tags.get_mut(i)
    {
        *t = Tag::s(5, Handle(next).hex());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Handles from their values.
    fn hs(v: &[u64]) -> Vec<Handle> {
        v.iter().map(|h| Handle(*h)).collect()
    }

    /// Entities already in handle order (or none) need no table.
    #[test]
    fn a_space_in_handle_order_needs_no_table() {
        assert_eq!(table(&hs(&[0x10, 0x20, 0x30])), None);
        assert_eq!(table(&[]), None);
    }

    /// Moved entities swap handles; repeated or null handles give no table.
    #[test]
    fn the_table_swaps_handles_so_ascending_sort_handles_give_the_order() {
        // 30 drawn first, 10 last: 30 takes 10's place, 10 takes 30's; 20 stays.
        let t = table(&hs(&[0x30, 0x20, 0x10])).unwrap_or_default();
        assert_eq!(t, vec![(Handle(0x30), Handle(0x10)), (Handle(0x10), Handle(0x30))]);
        // A repeated or null handle can't be ordered by handle.
        assert_eq!(table(&hs(&[0x20, 0x10, 0x20])), None);
        assert_eq!(table(&hs(&[0x20, 0])), None);
    }
}
