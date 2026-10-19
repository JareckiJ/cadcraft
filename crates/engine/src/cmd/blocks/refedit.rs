//! REFEDIT, REFSET and REFCLOSE: edit a block reference in place.
//!
//! REFEDIT hides the picked reference and puts the block's objects (all, or the ones named) into
//! the drawing where the reference shows them: the *working set*, ordinary objects that every
//! command edits. The rest of the block stands there as faded copies, and everything else in the
//! drawing is locked ([`crate::cmd::curves::is_locked`], enforced by [`after_command`]). Objects
//! drawn while editing join the working set. REFSET adds objects to or removes them from it.
//! REFCLOSE writes the working set back into the block definition, so every reference to the
//! block shows the change, or discards the edit.
//!
//! The state is [`Drawing::refedit`]: it travels with undo, is never written to a file (files
//! are written from [`Drawing::committed`]) and ends with the drawing it belongs to.
//!
//! External references (their source drawing is written back on REFCLOSE Save), and nested
//! references picked at a depth. Not yet: annotative scale contexts.

use std::collections::HashSet;
use std::sync::Arc;

use cadcraft_doc::{Drawing, Entity, EntityStore, Handle, RefEdit, Transparency, WorkItem, XrefStatus};
use cadcraft_geom::Mat3;
use serde_json::{Value, json};

use super::super::curves::handle_value;
use super::super::machines::{SelOutcome, SelectPhase};
use super::references_block;
use super::xref::{host, load};
use crate::cmd::{CommandSpec, bad, bool_or, has_doc, str_param, targets};
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

/// Transparency of the faded copies of the block's other objects.
const GHOST_FADE: u8 = 70;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("refedit", "Edit Reference In-Place", run_refedit)
            .menu(&["Tools", "Xref and Block In-place Editing", "Edit Reference In-Place"])
            .params("{handle? | handles? | name? (a block reference, default the selection), objects?: [block object handles] (default all), nested?: name of nested block} → {block, insert, working: [handles], faded, xref, xref_file, nested, nested_path}")
            .enabled(has_doc)
            .interactive(|_| Ok(Box::new(RefeditM::default()))),
        CommandSpec::new("refset", "Working Set", run_refset)
            .menu(&["Tools", "Xref and Block In-place Editing", "Add to Working Set"])
            .params("{add?: [handles], remove?: [handles]} (or {handles, mode?: \"add\"|\"remove\"}); with neither: lists the working set → {working: [handles], added, removed}")
            .enabled(editing)
            .interactive(|_| Ok(Box::new(RefsetM::default()))),
        CommandSpec::new("refclose", "Save Reference Edits", run_refclose)
            .menu(&["Tools", "Xref and Block In-place Editing", "Save Reference Edits"])
            .params("{option?: \"save\"|\"discard\" | save?: bool (default true)} → {block, saved, objects, synced}")
            .enabled(editing)
            .interactive(|_| Ok(Box::new(RefcloseM))),
    ]
}

fn editing(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    if s.doc().is_ok_and(|d| d.refedit.is_some()) { Ok(()) } else { Err("no block reference is being edited (REFEDIT)".into()) }
}

fn active(s: &Session, cmd: &str) -> Result<RefEdit> {
    s.doc()?.refedit.clone().ok_or_else(|| bad(cmd, "no block reference is being edited (REFEDIT)"))
}

/// A block object as the drawing shows it through the edited reference.
fn to_host(to_world: &Mat3, e: &Entity, h: Handle, ghost: bool) -> Entity {
    let mut c = e.clone();
    c.handle = h;
    c.kind.transform(to_world);
    if ghost {
        c.common.transparency = Transparency::Percent(GHOST_FADE);
    }
    c
}

/// A drawing object as it belongs in the block, with the block object's handle `h`.
fn to_block(inv: &Mat3, e: &Entity, h: Handle) -> Entity {
    let mut c = e.clone();
    c.handle = h;
    c.kind.transform(inv);
    c
}

fn put(d: &mut Drawing, r: &RefEdit, e: Entity) -> Result<()> {
    d.ensure_layer(&e.common.layer);
    d.space_mut(&r.space).ok_or_else(|| bad("refedit", "the space of the reference is gone"))?.push(e);
    Ok(())
}

fn hexes(hs: impl IntoIterator<Item = Handle>) -> Vec<String> {
    hs.into_iter().map(|h| h.hex()).collect()
}

// ---------------- REFEDIT helpers ----------------

#[derive(Clone, Debug)]
struct NestedRef {
    block: String,
    to_world: Mat3,
    path: Vec<String>,
}

fn collect_nested_refs(d: &Drawing, blk_name: &str, current_transform: &Mat3, path: &[String], depth: usize, out: &mut Vec<NestedRef>) {
    if depth > cadcraft_doc::MAX_BLOCK_DEPTH {
        return;
    }
    let Some(blk) = d.block(blk_name) else { return };
    for e in blk.entities.iter() {
        if let cadcraft_doc::EntityKind::Insert(child_ins) = &e.kind
            && let Some(child_blk) = d.block(&child_ins.block)
        {
            let child_to_parent = child_ins.transform(child_blk.base.xy());
            let child_to_world = child_to_parent.then(*current_transform);
            let mut child_path = path.to_vec();
            child_path.push(child_ins.block.clone());
            out.push(NestedRef { block: child_ins.block.clone(), to_world: child_to_world, path: child_path.clone() });
            collect_nested_refs(d, &child_ins.block, &child_to_world, &child_path, depth + 1, out);
        }
    }
}

fn resolve_xref_path(s: &Session, path: &str, block_name: &str) -> Result<String> {
    let host_path = host(s);
    let host_path_buf = host_path.as_deref().map(std::path::Path::new);
    let file = load::find(path, host_path_buf, false).map_err(|m| match m {
        load::Missing::NotFound => bad("refedit", format!("{path}: file not found for xref \"{block_name}\"")),
        load::Missing::Refused(r) => bad("refedit", format!("{path}: not opened, {r}")),
    })?;
    let file_str = file.to_string_lossy().into_owned();
    let canon = file.canonicalize().unwrap_or_else(|_| file.clone());

    for (idx, doc_state) in s.docs.iter().enumerate() {
        if idx != s.active
            && let Some(p) = &doc_state.path
        {
            let doc_file = std::path::Path::new(p);
            let doc_canon = doc_file.canonicalize().unwrap_or_else(|_| doc_file.to_path_buf());
            if doc_canon == canon || p.eq_ignore_ascii_case(&file_str) {
                return Err(bad(
                    "refedit",
                    format!(
                        "xref \"{block_name}\" cannot be edited in place because it is open in another tab; edit and save it there, or close its tab first"
                    ),
                ));
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let md = std::fs::metadata(&file).map_err(|e| bad("refedit", format!("{}: {e}", file.display())))?;
        if md.permissions().readonly() {
            return Err(bad("refedit", format!("xref \"{block_name}\" file is read-only ({})", file.display())));
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        return Err(bad("refedit", "editing xrefs in place is not available on the web"));
    }

    Ok(file_str)
}

fn write_back_xref(nd: &Drawing, r: &RefEdit, file_path: &str) -> Result<()> {
    let hooks = crate::cmd::file::io().ok_or_else(|| bad("refclose", "file formats are not available in this build"))?;
    #[cfg(not(target_arch = "wasm32"))]
    let bytes = std::fs::read(file_path).map_err(|e| bad("refclose", format!("{file_path}: {e}")))?;
    #[cfg(target_arch = "wasm32")]
    return Err(bad("refclose", "files are not available on the web"));

    let mut xref_doc = (hooks.read)(&bytes, file_path).map_err(|e| bad("refclose", format!("reading {file_path}: {e}")))?;

    let xref_root_name = r.nested_path.first().cloned().unwrap_or_else(|| r.block.clone());
    let prefix = format!("{xref_root_name}|");

    let un_dep = |name: &str| -> String {
        if let Some(rest) = name.strip_prefix(&prefix) {
            rest.to_string()
        } else if let Some(idx) = name.to_ascii_uppercase().find(&prefix.to_ascii_uppercase()) {
            if idx == 0 { name.get(prefix.len()..).unwrap_or_default().to_string() } else { name.to_string() }
        } else {
            name.to_string()
        }
    };

    let un_dep_entity = |e: &mut Entity| {
        e.common.layer = un_dep(&e.common.layer);
        e.common.linetype = un_dep(&e.common.linetype);
        match &mut e.kind {
            cadcraft_doc::EntityKind::Text(t) => t.style = un_dep(&t.style),
            cadcraft_doc::EntityKind::MText(m) => m.style = un_dep(&m.style),
            cadcraft_doc::EntityKind::AttDef(a) => a.text.style = un_dep(&a.text.style),
            cadcraft_doc::EntityKind::Insert(i) => {
                i.block = un_dep(&i.block);
                for a in &mut i.attribs {
                    a.text.style = un_dep(&a.text.style);
                }
            }
            cadcraft_doc::EntityKind::Dimension(dm) => {
                dm.style = un_dep(&dm.style);
                if let Some(b) = &mut dm.block {
                    *b = un_dep(b);
                }
            }
            cadcraft_doc::EntityKind::Leader(l) => l.style = un_dep(&l.style),
            cadcraft_doc::EntityKind::MLeader(m) => {
                m.style = un_dep(&m.style);
                if let Some(t) = &mut m.text {
                    t.style = un_dep(&t.style);
                }
                if let Some(b) = &mut m.block {
                    b.block = un_dep(&b.block);
                }
            }
            cadcraft_doc::EntityKind::Table(t) => t.style = un_dep(&t.style),
            _ => {}
        }
    };

    let Some(edited_block) = nd.block(&r.block) else {
        return Err(bad("refclose", format!("block \"{}\" not found in host drawing", r.block)));
    };

    // Bring over any new layers or styles that were added during the edit
    for e in edited_block.entities.iter() {
        let un_layer = un_dep(&e.common.layer);
        if !un_layer.is_empty()
            && un_layer != "0"
            && xref_doc.layer(&un_layer).is_none()
            && let Some(src_l) = nd.layer(&e.common.layer)
        {
            let mut new_l = src_l.clone();
            new_l.name = un_layer;
            new_l.linetype = un_dep(&new_l.linetype);
            xref_doc.layers.push(new_l);
        }
        let un_lt = un_dep(&e.common.linetype);
        if !un_lt.is_empty()
            && !["ByLayer", "ByBlock", "Continuous"].iter().any(|k| k.eq_ignore_ascii_case(&un_lt))
            && xref_doc.linetype(&un_lt).is_none()
            && let Some(src_lt) = nd.linetype(&e.common.linetype)
        {
            let mut new_lt = src_lt.clone();
            new_lt.name = un_lt;
            xref_doc.linetypes.push(new_lt);
        }
        if let cadcraft_doc::EntityKind::Text(t) = &e.kind {
            let un_s = un_dep(&t.style);
            if !un_s.is_empty()
                && un_s != "STANDARD"
                && xref_doc.text_style(&un_s).is_none()
                && let Some(src_s) = nd.text_style(&t.style)
            {
                let mut new_s = src_s.clone();
                new_s.name = un_s;
                xref_doc.text_styles.push(new_s);
            }
        }
    }

    if r.block.eq_ignore_ascii_case(&xref_root_name) {
        let orig_handles: Vec<Handle> = xref_doc.model.iter().map(|e| e.handle).collect();
        let mut new_model = cadcraft_doc::EntityStore::new();
        for (idx, e) in edited_block.entities.iter().enumerate() {
            let mut e_clone = (**e).clone();
            un_dep_entity(&mut e_clone);
            if let Some(h) = orig_handles.get(idx).copied() {
                e_clone.handle = h;
            } else {
                e_clone.handle = xref_doc.new_handle();
            }
            new_model.push(e_clone);
        }
        xref_doc.model = new_model;
    } else {
        let un_block_name = un_dep(&r.block);
        let orig_handles: Vec<Handle> =
            xref_doc.blocks.get(&un_block_name).map(|b| b.entities.iter().map(|e| e.handle).collect()).unwrap_or_default();
        let mut new_block_ents = cadcraft_doc::EntityStore::new();
        for (idx, e) in edited_block.entities.iter().enumerate() {
            let mut e_clone = (**e).clone();
            un_dep_entity(&mut e_clone);
            if let Some(h) = orig_handles.get(idx).copied() {
                e_clone.handle = h;
            } else {
                e_clone.handle = xref_doc.new_handle();
            }
            new_block_ents.push(e_clone);
        }
        if let Some(b) = xref_doc.blocks.get_mut(&un_block_name) {
            let b = std::sync::Arc::make_mut(b);
            b.entities = new_block_ents;
        } else {
            let mut nb = cadcraft_doc::Block::new(&un_block_name);
            nb.base = edited_block.base;
            nb.entities = new_block_ents;
            xref_doc.blocks.insert(un_block_name, std::sync::Arc::new(nb));
        }
    }

    let out_bytes = (hooks.write)(&xref_doc, file_path).map_err(|e| bad("refclose", format!("writing {file_path}: {e}")))?;
    #[cfg(not(target_arch = "wasm32"))]
    std::fs::write(file_path, &out_bytes).map_err(|e| bad("refclose", format!("saving {file_path}: {e}")))?;

    Ok(())
}

// ---------------- REFEDIT ----------------

fn run_refedit(s: &mut Session, p: &Value) -> Result<Value> {
    if s.doc()?.refedit.is_some() {
        return Err(bad("refedit", "a reference is already being edited; finish with REFCLOSE"));
    }
    let d = s.doc()?;
    let picked = match str_param(p, "name") {
        Some(n) => d
            .model
            .iter()
            .chain(d.layouts.iter().flat_map(|l| l.entities.iter()))
            .find(|e| matches!(&e.kind, cadcraft_doc::EntityKind::Insert(i) if i.block.eq_ignore_ascii_case(n)))
            .map(|e| e.handle),
        None => targets(s, p)?.into_iter().find(|h| d.entity(*h).is_some_and(|e| matches!(e.kind, cadcraft_doc::EntityKind::Insert(_)))),
    };
    let ih = picked.ok_or_else(|| bad("refedit", "select a block reference"))?;
    let (ins, space) = match d.entity(ih).map(|e| &e.kind) {
        Some(cadcraft_doc::EntityKind::Insert(i)) => (i.clone(), d.space_of(ih).ok_or_else(|| bad("refedit", "the reference is in no space"))?),
        _ => return Err(bad("refedit", "select a block reference")),
    };
    let blk = d.block(&ins.block).cloned().ok_or_else(|| bad("refedit", format!("block \"{}\" not found", ins.block)))?;
    if blk.name.starts_with('*') || blk.array.is_some() {
        return Err(bad("refedit", format!("\"{}\" is generated geometry and can't be edited in place", blk.name)));
    }
    if blk.xref_path.is_some() && blk.xref_status != XrefStatus::Loaded {
        return Err(bad("refedit", format!("xref \"{}\" is not loaded ({})", blk.name, blk.xref_status.label())));
    }
    let top_to_world = ins.transform(blk.base.xy());
    let (sx, sy) = (ins.scale.x.abs(), ins.scale.y.abs());
    let finite = [top_to_world.a, top_to_world.b, top_to_world.c, top_to_world.d, top_to_world.e, top_to_world.f].iter().all(|v| v.is_finite());
    if !finite || top_to_world.det().abs() < 1e-12 || (sx - sy).abs() > 1e-9 * sx.max(sy) {
        return Err(bad("refedit", "the reference must be scaled equally in X and Y (and not flat) to be edited in place"));
    }

    let mut nested_refs = Vec::new();
    collect_nested_refs(d, &blk.name, &top_to_world, std::slice::from_ref(&blk.name), 0, &mut nested_refs);

    let target_nested = str_param(p, "nested").or_else(|| str_param(p, "block"));
    let (edit_blk_name, to_world, nested_path) = if let Some(target) = target_nested {
        if target.eq_ignore_ascii_case(&blk.name) {
            (blk.name.clone(), top_to_world, vec![blk.name.clone()])
        } else if let Some(nr) = nested_refs.iter().find(|nr| nr.block.eq_ignore_ascii_case(target)) {
            (nr.block.clone(), nr.to_world, nr.path.clone())
        } else {
            return Err(bad("refedit", format!("nested block \"{target}\" not found in reference \"{}\"", blk.name)));
        }
    } else {
        (blk.name.clone(), top_to_world, vec![blk.name.clone()])
    };

    let edit_blk = d.block(&edit_blk_name).cloned().ok_or_else(|| bad("refedit", format!("block \"{edit_blk_name}\" not found")))?;

    // Resolve xref file if the block or an ancestor is an xref
    let xref_file = if let Some(path) = &edit_blk.xref_path {
        Some(resolve_xref_path(s, path, &edit_blk.name)?)
    } else {
        let mut found = None;
        for parent_name in &nested_path {
            if let Some(p_blk) = d.block(parent_name)
                && let Some(path) = &p_blk.xref_path
            {
                found = Some(resolve_xref_path(s, path, &p_blk.name)?);
                break;
            }
        }
        found
    };

    let chosen: Option<HashSet<Handle>> = match p.get("objects") {
        None | Some(Value::Null) => None,
        Some(Value::String(a)) if a.eq_ignore_ascii_case("all") => None,
        Some(Value::Array(a)) => {
            let mut set = HashSet::new();
            for v in a {
                let h = handle_value(v).ok_or_else(|| bad("refedit", format!("`objects`: {v} is not an object handle")))?;
                if edit_blk.entities.get(h).is_none() {
                    return Err(bad("refedit", format!("object {} is not in block \"{}\"", h.hex(), edit_blk.name)));
                }
                set.insert(h);
            }
            if set.is_empty() {
                return Err(bad("refedit", "`objects` is empty"));
            }
            Some(set)
        }
        Some(v) => return Err(bad("refedit", format!("`objects` must be \"all\" or an array of handles, got {v}"))),
    };

    let was_visible = d.entity(ih).is_some_and(|e| e.common.visible);
    let mut r = RefEdit {
        block: edit_blk.name.clone(),
        insert: ih,
        insert_was_visible: was_visible,
        space,
        to_world,
        items: Vec::new(),
        ghosts: Vec::new(),
        released: Vec::new(),
        xref_file: xref_file.clone(),
        nested_path: nested_path.clone(),
    };
    let doc = s.doc_mut()?;
    for e in edit_blk.entities.iter() {
        let working = chosen.as_ref().is_none_or(|c| c.contains(&e.handle));
        let h = doc.new_handle();
        put(doc, &r, to_host(&to_world, e, h, !working))?;
        if working {
            r.items.push(WorkItem { copy: h, source: Some(e.handle) });
        } else {
            r.ghosts.push((h, e.handle));
        }
    }

    if edit_blk.name != blk.name {
        // Show context from parent block as ghosts
        for e in blk.entities.iter() {
            if let cadcraft_doc::EntityKind::Insert(i) = &e.kind
                && r.nested_path.iter().any(|np| np.eq_ignore_ascii_case(&i.block))
            {
                continue;
            }
            let h = doc.new_handle();
            put(doc, &r, to_host(&top_to_world, e, h, true))?;
            r.ghosts.push((h, e.handle));
        }
    }

    doc.modify_entity(ih, |e| e.common.visible = false).map_err(|e| bad("refedit", e.to_string()))?;
    let out = json!({
        "block": r.block,
        "insert": ih.hex(),
        "working": hexes(r.items.iter().map(|i| i.copy)),
        "faded": r.ghosts.len(),
        "xref": r.xref_file.is_some(),
        "xref_file": r.xref_file,
        "nested": r.nested_path.len() > 1,
        "nested_path": r.nested_path,
    });
    let sel: Vec<Handle> = r.items.iter().map(|i| i.copy).collect();
    doc.refedit = Some(r);
    s.set_selection(sel);
    Ok(out)
}

// ---------------- REFSET ----------------

fn handle_list(p: &Value, key: &str) -> Result<Vec<Handle>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(a)) => {
            a.iter().map(|v| handle_value(v).ok_or_else(|| bad("refset", format!("`{key}`: {v} is not an object handle")))).collect()
        }
        Some(v) => Err(bad("refset", format!("`{key}` must be an array of object handles, got {v}"))),
    }
}

fn run_refset(s: &mut Session, p: &Value) -> Result<Value> {
    let (mut add, mut remove) = (handle_list(p, "add")?, handle_list(p, "remove")?);
    let hs = handle_list(p, "handles")?;
    if str_param(p, "mode").is_some_and(|m| m.to_ascii_lowercase().starts_with('r')) {
        remove.extend(hs)
    } else {
        add.extend(hs)
    }
    apply_refset(s, &add, &remove)
}

fn apply_refset(s: &mut Session, add: &[Handle], remove: &[Handle]) -> Result<Value> {
    let mut r = active(s, "refset")?;
    let inv = r.to_world.inverse().ok_or_else(|| bad("refset", "the reference can't be inverted"))?;
    let d = s.doc()?;
    for h in add.iter().chain(remove) {
        if d.entity(*h).is_none() {
            return Err(bad("refset", format!("no object {}", h.hex())));
        }
    }
    if let Some(h) = remove.iter().find(|h| !r.is_working(**h)) {
        return Err(bad("refset", format!("object {} is not in the working set", h.hex())));
    }
    let blk = d.block(&r.block).cloned().ok_or_else(|| bad("refset", format!("block \"{}\" is gone; REFCLOSE Discard", r.block)))?;
    let (mut added, mut removed) = (0usize, 0usize);
    let doc = s.doc_mut()?;
    for h in add {
        if *h == r.insert || r.is_working(*h) {
            continue;
        }
        if let Some(pos) = r.ghosts.iter().position(|(g, _)| g == h) {
            // A faded block object joins the working set (with the edits it had before).
            let (g, src) = r.ghosts.remove(pos);
            doc.remove_entity(g);
            let cur = match r.released.iter().position(|e| e.handle == src) {
                Some(i) => Some(r.released.remove(i)),
                None => blk.entities.get(src).map(|e| (**e).clone()),
            };
            if let Some(cur) = cur {
                let c = doc.new_handle();
                put(doc, &r, to_host(&r.to_world, &cur, c, false))?;
                r.items.push(WorkItem { copy: c, source: Some(src) });
                added += 1;
            }
        } else {
            r.items.push(WorkItem { copy: *h, source: None });
            added += 1;
        }
    }
    for h in remove {
        let Some(pos) = r.items.iter().position(|i| i.copy == *h) else { continue };
        let item = r.items.remove(pos);
        if let Some(src) = item.source
            && let Some(e) = doc.entity(*h).map(|e| (**e).clone())
        {
            // Back to the block, edits kept, shown faded.
            let kept = to_block(&inv, &e, src);
            doc.remove_entity(*h);
            let g = doc.new_handle();
            put(doc, &r, to_host(&r.to_world, &kept, g, true))?;
            r.ghosts.push((g, src));
            r.released.push(kept);
        }
        removed += 1;
    }
    let working = hexes(r.items.iter().map(|i| i.copy));
    doc.refedit = Some(r);
    Ok(json!({ "working": working, "added": added, "removed": removed }))
}

// ---------------- REFCLOSE ----------------

fn run_refclose(s: &mut Session, p: &Value) -> Result<Value> {
    let save = match str_param(p, "option") {
        Some(o) if o.to_ascii_lowercase().starts_with('d') => false,
        Some(o) if o.to_ascii_lowercase().starts_with('s') => true,
        Some(o) => return Err(bad("refclose", format!("unknown option \"{o}\" (save or discard)"))),
        None => bool_or(p, "save", true),
    };
    close(s, save)
}

fn close(s: &mut Session, save: bool) -> Result<Value> {
    let r = active(s, "refclose")?;
    // Work on a copy: the drawing changes only when everything went through.
    let mut nd: Drawing = s.doc()?.clone();
    let mut objects = 0;
    if save {
        objects = commit_block(&mut nd, &r)?;
        if let Some(file_path) = &r.xref_file {
            write_back_xref(&nd, &r, file_path)?;
        }
    }
    nd.end_refedit_discard(&r);
    *s.doc_mut()? = nd;
    s.set_selection(Vec::new());
    let had_attdefs = r.released.iter().any(|e| matches!(&e.kind, cadcraft_doc::EntityKind::AttDef(_)));
    let synced = if save && (super::attribs::has_attdefs(s, &r.block) || had_attdefs) {
        super::attribs::sync(s, &r.block).map(|(_, n)| n).unwrap_or(0)
    } else {
        0
    };
    Ok(json!({
        "block": r.block,
        "saved": save,
        "objects": objects,
        "synced": synced,
        "xref": r.xref_file.is_some(),
        "xref_file": r.xref_file,
    }))
}

/// Replace the block's objects by the working set (and move the drawing objects added to it
/// into the block). Returns the number of objects the block has now.
fn commit_block(nd: &mut Drawing, r: &RefEdit) -> Result<usize> {
    let blk = nd.block(&r.block).cloned().ok_or_else(|| bad("refclose", format!("block \"{}\" is gone; use REFCLOSE Discard", r.block)))?;
    let inv = r.to_world.inverse().ok_or_else(|| bad("refclose", "the reference can't be inverted; use REFCLOSE Discard"))?;
    let mut ents: Vec<Entity> = Vec::new();
    for e in blk.entities.iter() {
        match r.items.iter().find(|i| i.source == Some(e.handle)) {
            // A working object the user erased leaves the block.
            Some(i) => {
                if let Some(c) = nd.entity(i.copy) {
                    ents.push(to_block(&inv, c, e.handle));
                }
            }
            None => match r.released.iter().find(|x| x.handle == e.handle) {
                Some(x) => ents.push(x.clone()),
                None => ents.push((**e).clone()),
            },
        }
    }
    let mut moved = Vec::new();
    for i in r.items.iter().filter(|i| i.source.is_none()) {
        if let Some(c) = nd.entity(i.copy).map(|c| (**c).clone()) {
            let h = nd.new_handle();
            ents.push(to_block(&inv, &c, h));
            moved.push(i.copy);
        }
    }
    if references_block(nd, &ents, &r.block) {
        return Err(bad("refclose", format!("Block \"{}\" would reference itself; remove that reference or use REFCLOSE Discard.", r.block)));
    }
    let n = ents.len();
    let mut store = EntityStore::new();
    for e in ents {
        store.push(e);
    }
    let key = nd.blocks.keys().find(|k| k.eq_ignore_ascii_case(&r.block)).cloned().unwrap_or_else(|| r.block.clone());
    let mut nb = (*blk).clone();
    nb.entities = store;
    nd.blocks.insert(key, Arc::new(nb));
    for h in moved {
        nd.remove_entity(h);
    }
    Ok(n)
}

// ---------------- while editing ----------------

/// After a command ran: objects it created join the working set, and a command that changed
/// anything outside the working set is undone (returns the reason).
pub(crate) fn after_command(s: &mut Session, before: &Arc<Drawing>, id: &str) -> Option<String> {
    if matches!(id, "refedit" | "refset" | "refclose") || !crate::cmd::find_command(id).is_some_and(|c| c.undoable) {
        return None;
    }
    let rb = before.refedit.as_ref()?;
    let st = s.state_mut().ok()?;
    if Arc::ptr_eq(before, &st.doc) {
        return None;
    }
    let ra = st.doc.refedit.as_ref().filter(|ra| ra.insert == rb.insert)?;
    let owned: HashSet<Handle> = rb.items.iter().map(|i| i.copy).chain(rb.ghosts.iter().map(|g| g.0)).chain([rb.insert]).collect();
    let stores = |d: &Drawing| std::iter::once(&d.model).chain(d.layouts.iter().map(|l| &l.entities)).cloned().collect::<Vec<_>>();
    let touched = stores(before)
        .iter()
        .any(|store| store.iter().any(|e| !owned.contains(&e.handle) && !st.doc.entity(e.handle).is_some_and(|a| Arc::ptr_eq(a, e))));
    if touched {
        st.doc = before.clone();
        return Some("Only the working set can be edited during REFEDIT (REFSET adds objects, REFCLOSE ends the edit).".into());
    }
    let seed = before.handseed;
    let mut new: Vec<Handle> = Vec::new();
    for store in stores(&st.doc) {
        new.extend(store.iter().map(|e| e.handle).filter(|h| h.0 >= seed && !ra.owns(*h)));
    }
    if !new.is_empty()
        && let Some(r) = Arc::make_mut(&mut st.doc).refedit.as_mut()
    {
        r.items.extend(new.into_iter().map(|copy| WorkItem { copy, source: None }));
    }
    None
}

/// Whether `h` may not be edited because a reference is being edited and it is not in the working set.
pub(crate) fn locked(d: &Drawing, h: Handle) -> bool {
    d.refedit.as_ref().is_some_and(|r| !r.is_working(h))
}

// ---------------- interactive ----------------

#[derive(Default)]
enum RStage {
    #[default]
    Mode,
    Pick(bool),
}

#[derive(Default)]
struct RefsetM {
    stage: RStage,
    sel: SelectPhase,
}

impl Interactive for RefsetM {
    fn name(&self) -> &'static str {
        "REFSET"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        active(s, "refset").map(|_| Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.stage {
            RStage::Mode => {
                let mut p = Prompt::new("Enter an option [Add/Remove] <Add>", Accept::TEXT);
                p.keywords = vec!["Add".into(), "Remove".into()];
                p.default = Some("Add".into());
                p
            }
            RStage::Pick(remove) => {
                let mut p = self.sel.prompt();
                p.message = format!("Select objects to {}", if remove { "remove" } else { "add" });
                p
            }
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match self.stage {
            RStage::Mode => {
                let remove = match i {
                    Input::Keyword(k) | Input::Text(k) => k.to_ascii_lowercase().starts_with('r'),
                    Input::Enter => false,
                    _ => return Ok(Step::Cancel),
                };
                self.sel = SelectPhase::begin(s);
                self.stage = RStage::Pick(remove);
                if self.sel.done {
                    let hs = self.sel.picked.clone();
                    return self.finish(s, remove, &hs);
                }
                Ok(Step::Continue)
            }
            RStage::Pick(remove) => match self.sel.feed(s, &i)? {
                SelOutcome::More => Ok(Step::Continue),
                SelOutcome::Empty => Ok(Step::Done),
                SelOutcome::Done(hs) => self.finish(s, remove, &hs),
            },
        }
    }
}

impl RefsetM {
    fn finish(&mut self, s: &mut Session, remove: bool, hs: &[Handle]) -> Result<Step> {
        let r = if remove { apply_refset(s, &[], hs) } else { apply_refset(s, hs, &[]) }?;
        s.echo(format!("{} object(s) in the working set.", r.get("working").and_then(Value::as_array).map_or(0, Vec::len)));
        s.set_selection(Vec::new());
        Ok(Step::Done)
    }
}

#[derive(Default)]
enum RefeditStage {
    #[default]
    Select,
    PickNested {
        handle: Handle,
        candidates: Vec<String>,
        idx: usize,
    },
}

#[derive(Default)]
struct RefeditM {
    stage: RefeditStage,
}

impl Interactive for RefeditM {
    fn name(&self) -> &'static str {
        "REFEDIT"
    }
    fn prompt(&self, _: &Session) -> Prompt {
        match &self.stage {
            RefeditStage::Select => Prompt::new("Select reference", Accept::SELECT),
            RefeditStage::PickNested { candidates, idx, .. } => {
                let cur = candidates.get(*idx).map(|s| s.as_str()).unwrap_or_default();
                let mut p = Prompt::new(format!("Select reference to edit: [Next/OK] <{cur}>"), Accept::TEXT);
                p.keywords = vec!["Next".into(), "OK".into()];
                p.default = Some("OK".into());
                p
            }
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match &mut self.stage {
            RefeditStage::Select => {
                if matches!(i, Input::Cancel) {
                    return Ok(Step::Cancel);
                }
                let h = match i {
                    Input::Pick(hs) => hs.into_iter().next(),
                    Input::Text(t) => Handle::parse_hex(t.trim()),
                    _ => None,
                };
                let Some(h) = h else { return Ok(Step::Continue) };
                let d = s.doc()?;
                let Some(cadcraft_doc::EntityKind::Insert(ins)) = d.entity(h).map(|e| &e.kind) else {
                    s.echo("Select a block or xref reference.");
                    return Ok(Step::Continue);
                };
                let Some(blk) = d.block(&ins.block) else {
                    s.echo(format!("Block \"{}\" not found.", ins.block));
                    return Ok(Step::Continue);
                };
                let mut candidates = vec![blk.name.clone()];
                let mut nested = Vec::new();
                collect_nested_refs(d, &blk.name, &Mat3::IDENTITY, std::slice::from_ref(&blk.name), 0, &mut nested);
                for nr in nested {
                    if !candidates.iter().any(|c| c.eq_ignore_ascii_case(&nr.block)) {
                        candidates.push(nr.block);
                    }
                }
                if candidates.len() > 1 {
                    self.stage = RefeditStage::PickNested { handle: h, candidates, idx: 0 };
                    Ok(Step::Continue)
                } else {
                    match run_refedit(s, &json!({ "handle": h.hex() })) {
                        Ok(_) => {
                            s.set_selection(Vec::new());
                            Ok(Step::Done)
                        }
                        Err(e) => {
                            s.echo(e.to_string());
                            Ok(Step::Cancel)
                        }
                    }
                }
            }
            RefeditStage::PickNested { handle, candidates, idx } => match i {
                Input::Cancel => Ok(Step::Cancel),
                Input::Keyword(k) if k.eq_ignore_ascii_case("Next") || k.eq_ignore_ascii_case("N") => {
                    *idx = (*idx + 1) % candidates.len();
                    Ok(Step::Continue)
                }
                Input::Enter | Input::Keyword(_) => {
                    let chosen = candidates.get(*idx).cloned().unwrap_or_default();
                    let h_hex = handle.hex();
                    match run_refedit(s, &json!({ "handle": h_hex, "nested": chosen })) {
                        Ok(_) => {
                            s.set_selection(Vec::new());
                            Ok(Step::Done)
                        }
                        Err(e) => {
                            s.echo(e.to_string());
                            Ok(Step::Cancel)
                        }
                    }
                }
                _ => Ok(Step::Continue),
            },
        }
    }
}

struct RefcloseM;

impl Interactive for RefcloseM {
    fn name(&self) -> &'static str {
        "REFCLOSE"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        let mut p = Prompt::new("Enter option [Save/Discard reference changes] <Save>", Accept::TEXT);
        p.keywords = vec!["Save".into(), "Discard".into()];
        p.default = Some("Save".into());
        p
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let save = match i {
            Input::Keyword(k) | Input::Text(k) => !k.to_ascii_lowercase().starts_with('d'),
            Input::Enter => true,
            _ => return Ok(Step::Cancel),
        };
        let r = close(s, save)?;
        if save {
            let synced = r.get("synced").and_then(Value::as_u64).unwrap_or(0);
            if let Some(xf) = r.get("xref_file").and_then(Value::as_str) {
                s.echo(format!("Reference changes saved and written to external reference \"{xf}\"."));
            } else if synced > 0 {
                s.echo(format!("Reference changes saved. {synced} reference{} synchronized.", if synced == 1 { "" } else { "s" }));
            } else {
                s.echo("Reference changes saved.");
            }
        } else {
            s.echo("Reference changes discarded.");
        }
        Ok(Step::Done)
    }
}
