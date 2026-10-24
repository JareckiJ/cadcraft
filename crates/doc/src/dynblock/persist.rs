//! Persistence and lifecycle of dynamic blocks: cloning on BSAVEAS, exporting to model space
//! on WBLOCK, importing from external files on INSERT, and refreshing dynamic references on BSAVE.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::{reference, set_property};
use crate::foreign::ForeignObject;
use crate::{Drawing, EntityKind, Handle};

/// Import dynamic block data from `src` (block `from_name`, or `"*Model_Space"` for a drawing's own)
/// into `d` under block `to_name` (or `"*Model_Space"`).
pub fn import_dynamic_block(d: &mut Drawing, src: &Drawing, from_name: &str, to_name: &str) -> bool {
    let from_upper = from_name.trim().to_ascii_uppercase();
    let to_upper = to_name.trim().to_ascii_uppercase();
    let Some(from_extra) = src.foreign.records.get(&("BLOCK_RECORD".to_string(), from_upper)).cloned() else {
        return false;
    };
    let Some(xdict_h) = from_extra.xdict else {
        return false;
    };

    let src_objects: HashMap<Handle, &ForeignObject> = src.foreign.objects.iter().map(|o| (o.handle, o)).collect();
    let mut to_clone = Vec::new();
    let mut queue = vec![xdict_h];
    let mut seen = HashSet::new();
    seen.insert(xdict_h);

    while let Some(h) = queue.pop() {
        to_clone.push(h);
        if let Some(obj) = src_objects.get(&h) {
            for t in &obj.tags {
                if matches!(t.code, 340 | 350 | 360)
                    && let Some(target) = Handle::parse_hex(&t.value)
                    && src_objects.contains_key(&target)
                    && seen.insert(target)
                {
                    queue.push(target);
                }
            }
        }
    }

    let handle_map: HashMap<Handle, Handle> = to_clone.iter().map(|&h| (h, d.new_handle())).collect();

    let mut entity_map: HashMap<Handle, Handle> = HashMap::new();
    let se_handles: Vec<Handle> = if from_name.eq_ignore_ascii_case("*Model_Space") {
        src.model.handles()
    } else {
        src.block(from_name).map(|b| b.entities.handles()).unwrap_or_default()
    };
    let de_handles: Vec<Handle> = if to_name.eq_ignore_ascii_case("*Model_Space") {
        d.model.handles()
    } else {
        d.block(to_name).map(|b| b.entities.handles()).unwrap_or_default()
    };

    for (s_h, d_h) in se_handles.iter().zip(de_handles.iter()) {
        entity_map.insert(*s_h, *d_h);
    }

    let mut cloned_objects = Vec::with_capacity(to_clone.len());
    for &old_h in &to_clone {
        let Some(obj) = src_objects.get(&old_h) else { continue };
        let new_h = handle_map.get(&old_h).copied().unwrap_or(old_h);
        let mut tags = Vec::with_capacity(obj.tags.len() + 1);
        let mut has_h = false;
        for t in &obj.tags {
            let mut t = t.clone();
            if t.code == 5 {
                has_h = true;
                t.value = new_h.hex();
            } else if t.code == 330 {
                if let Some(new_target) = Handle::parse_hex(&t.value).and_then(|h| handle_map.get(&h)) {
                    t.value = new_target.hex();
                }
            } else if matches!(t.code, 340 | 350 | 360) {
                if let Some(new_target) = Handle::parse_hex(&t.value).and_then(|h| handle_map.get(&h)) {
                    t.value = new_target.hex();
                }
            } else if matches!(t.code, 331..=333)
                && let Some(new_target) = Handle::parse_hex(&t.value).and_then(|h| entity_map.get(&h))
            {
                t.value = new_target.hex();
            }
            tags.push(t);
        }
        if !has_h {
            tags.insert(0, crate::RawTag { code: 5, value: new_h.hex() });
        }
        cloned_objects.push(ForeignObject { handle: new_h, kind: obj.kind.clone(), tags });
    }

    let f = Arc::make_mut(&mut d.foreign);
    f.objects.extend(cloned_objects);

    for cls in &src.foreign.classes {
        let cls_name = cls.iter().find(|t| t.code == 1).map(|t| t.value.as_str()).unwrap_or("");
        if !cls_name.is_empty() && !f.classes.iter().any(|c| c.iter().any(|t| t.code == 1 && t.value.eq_ignore_ascii_case(cls_name))) {
            f.classes.push(cls.clone());
        }
    }

    let new_xdict = handle_map.get(&xdict_h).copied().unwrap_or(xdict_h);
    let mut new_extra = from_extra;
    new_extra.xdict = Some(new_xdict);
    f.records.insert(("BLOCK_RECORD".to_string(), to_upper), new_extra);

    true
}

/// Clone dynamic block data from `from_name` to `to_name` within `d` (used by BSAVEAS).
pub fn clone_dynamic_block(d: &mut Drawing, from_name: &str, to_name: &str) -> bool {
    let src = d.clone();
    import_dynamic_block(d, &src, from_name, to_name)
}

/// Export dynamic block data of block `block_name` into `d`'s model space and block record (used by WBLOCK).
pub fn export_dynamic_block_to_model_space(d: &mut Drawing, src: &Drawing, block_name: &str) -> bool {
    let ok1 = import_dynamic_block(d, src, block_name, "*Model_Space");
    let _ = import_dynamic_block(d, src, block_name, block_name);
    ok1
}

/// Refresh any dynamic block references in `d` when `block_name` is redefined (BSAVE), so their
/// anonymous blocks are regenerated from the updated definition while retaining custom values.
pub fn refresh_dynamic_references(d: &mut Drawing, block_name: &str) {
    let mut targets = Vec::new();
    for e in d.model.iter() {
        if let EntityKind::Insert(i) = &e.kind
            && let Some(r) = reference(d, e.handle)
            && r.definition.eq_ignore_ascii_case(block_name)
        {
            targets.push((e.handle, i.block.clone()));
        }
    }
    for l in &d.layouts {
        for e in l.entities.iter() {
            if let EntityKind::Insert(i) = &e.kind
                && let Some(r) = reference(d, e.handle)
                && r.definition.eq_ignore_ascii_case(block_name)
            {
                targets.push((e.handle, i.block.clone()));
            }
        }
    }
    for (handle, old_block) in targets {
        if old_block.starts_with('*')
            && let Some((_, props)) = super::custom_properties(d, handle)
        {
            let _ = d.modify_entity(handle, |e| {
                if let EntityKind::Insert(i) = &mut e.kind {
                    i.block = block_name.to_string();
                }
            });
            for prop in props {
                if prop.editable {
                    let _ = set_property(d, handle, &prop.name, &prop.value);
                }
            }
        }
    }
}
