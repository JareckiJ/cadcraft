//! USAGE: a read-only cross-reference of the named objects a drawing uses — for each layer,
//! block, text style and linetype, how many objects use it, whether they are only inside block
//! definitions, and which are empty or unused. The unused lists are PURGE's scan (see
//! `cmd::blocks::purge`); nothing here changes or deletes anything.

use std::collections::{HashMap, HashSet};

use cadcraft_doc::{Drawing, Entity, EntityKind};
use serde_json::{Value, json};

use super::super::blocks::purge;
use super::super::*;
use crate::{Result, Session};

const CMD: &str = "usage";
/// Names listed per kind in `unused`; the counts are always complete.
const MAX_NAMES: usize = 5_000;

pub const PARAMS: &str = "{type?: layers|blocks|textstyles|linetypes|all (default all), limit? (default 200), offset?} → \
    {schema: 1, count (all items of the type), returned, offset, truncated, \
    items: [{type: layer|block|textStyle|linetype, name, entities (objects in model space and layouts that use it), \
    blockEntities (objects in block definitions that use it), references (named objects that use it: a layer's linetype, \
    a text style's dimension and multileader styles), used (for a block: reachable from model space, a layout or a dimension), \
    insideBlocksOnly (used, but by block definitions only), empty (nothing refers to it), unused (PURGE would offer it), \
    current? (the current layer, text style or linetype)}], \
    summary: {<kind>: {total, used, insideBlocksOnly, empty, unused}}, \
    unused: {<kind>: {count, returned, truncated, names}} (PURGE's scan, in table order)}";

/// The four kinds of named object USAGE reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Category {
    Layer,
    Block,
    TextStyle,
    Linetype,
}

impl Category {
    const ALL: [Category; 4] = [Category::Layer, Category::Block, Category::TextStyle, Category::Linetype];

    fn key(self) -> &'static str {
        match self {
            Category::Layer => "layer",
            Category::Block => "block",
            Category::TextStyle => "textStyle",
            Category::Linetype => "linetype",
        }
    }
    fn plural(self) -> &'static str {
        match self {
            Category::Layer => "layers",
            Category::Block => "blocks",
            Category::TextStyle => "textStyles",
            Category::Linetype => "linetypes",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Category::Layer => "Layers",
            Category::Block => "Blocks",
            Category::TextStyle => "Text styles",
            Category::Linetype => "Linetypes",
        }
    }
    fn purge(self) -> purge::Kind {
        match self {
            Category::Layer => purge::Kind::Layers,
            Category::Block => purge::Kind::Blocks,
            Category::TextStyle => purge::Kind::TextStyles,
            Category::Linetype => purge::Kind::Linetypes,
        }
    }
    /// `None` for an unknown name, `Some(None)` for all kinds.
    fn parse(s: &str) -> Option<Option<Category>> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "all" => Some(None),
            "layer" | "layers" => Some(Some(Category::Layer)),
            "block" | "blocks" => Some(Some(Category::Block)),
            "style" | "styles" | "textstyle" | "textstyles" | "text_style" | "text_styles" => Some(Some(Category::TextStyle)),
            "linetype" | "linetypes" | "ltype" | "ltypes" => Some(Some(Category::Linetype)),
            _ => None,
        }
    }
}

/// Objects in model space and the layouts (`false`), and in block definitions (`true`).
fn each_entity(d: &Drawing) -> impl Iterator<Item = (&Entity, bool)> {
    let top = d.model.iter().chain(d.layouts.iter().flat_map(|l| l.entities.iter())).map(|e| (&**e, false));
    let blocks = d.blocks.values().flat_map(|b| b.entities.iter()).map(|e| (&**e, true));
    top.chain(blocks).take(5_000_000)
}

/// Uses per lower-case name: (in model space and layouts, in block definitions).
type Uses = HashMap<String, (usize, usize)>;

fn bump(map: &mut Uses, name: &str, in_block: bool) {
    let slot = map.entry(name.to_ascii_lowercase()).or_default();
    if in_block {
        slot.1 = slot.1.saturating_add(1);
    } else {
        slot.0 = slot.0.saturating_add(1);
    }
}

fn bump_reference(map: &mut HashMap<String, usize>, name: &str) {
    let slot = map.entry(name.to_ascii_lowercase()).or_default();
    *slot = slot.saturating_add(1);
}

/// `ByLayer` and `ByBlock` name a value, not the linetype table's entries.
fn linetype_value(name: &str) -> bool {
    name.eq_ignore_ascii_case("ByLayer") || name.eq_ignore_ascii_case("ByBlock")
}

/// Every use of the drawing's layers, blocks, text styles and linetypes, in one pass over the
/// objects (plus the block reachability PURGE already computes).
#[derive(Default)]
struct Scan {
    layers: Uses,
    blocks: Uses,
    styles: Uses,
    linetypes: Uses,
    /// Dimension and multileader styles that name a text style.
    style_refs: HashMap<String, usize>,
    /// Layers that name a linetype.
    linetype_refs: HashMap<String, usize>,
    /// Upper-case names of blocks reachable from model space, the layouts and dimensions.
    used_blocks: HashSet<String>,
}

impl Scan {
    fn new(d: &Drawing) -> Scan {
        let mut sc = Scan { used_blocks: purge::used_blocks(d), ..Scan::default() };
        for (e, in_block) in each_entity(d) {
            bump(&mut sc.layers, &e.common.layer, in_block);
            if !linetype_value(&e.common.linetype) {
                bump(&mut sc.linetypes, &e.common.linetype, in_block);
            }
            match &e.kind {
                EntityKind::Insert(i) => {
                    bump(&mut sc.blocks, &i.block, in_block);
                    for a in &i.attribs {
                        bump(&mut sc.styles, &a.text.style, in_block);
                    }
                }
                EntityKind::Dimension(dm) => {
                    if let Some(b) = &dm.block {
                        bump(&mut sc.blocks, b, in_block);
                    }
                }
                EntityKind::MLeader(m) => {
                    if let Some(b) = &m.block {
                        bump(&mut sc.blocks, &b.block, in_block);
                    }
                    if let Some(t) = &m.text {
                        bump(&mut sc.styles, &t.style, in_block);
                    }
                }
                EntityKind::Text(t) => bump(&mut sc.styles, &t.style, in_block),
                EntityKind::MText(t) => bump(&mut sc.styles, &t.style, in_block),
                EntityKind::AttDef(a) => bump(&mut sc.styles, &a.text.style, in_block),
                _ => {}
            }
        }
        for st in &d.dim_styles {
            bump_reference(&mut sc.style_refs, &st.text_style);
        }
        for st in &d.mleader_styles {
            bump_reference(&mut sc.style_refs, &st.text_style);
        }
        for l in &d.layers {
            if !linetype_value(&l.linetype) {
                bump_reference(&mut sc.linetype_refs, &l.linetype);
            }
        }
        sc
    }
}

/// PURGE's unused lists, kept once so each item can say whether PURGE would offer it.
struct Unused {
    lists: Vec<(Category, Vec<String>, HashSet<String>)>,
}

impl Unused {
    fn new(d: &Drawing) -> Unused {
        let lists = Category::ALL
            .into_iter()
            .map(|c| {
                let names = purge::candidates(d, c.purge());
                let lower = names.iter().map(|n| n.to_ascii_lowercase()).collect();
                (c, names, lower)
            })
            .collect();
        Unused { lists }
    }
    fn has(&self, c: Category, name: &str) -> bool {
        self.lists.iter().find(|(k, ..)| *k == c).is_some_and(|(_, _, lower)| lower.contains(&name.to_ascii_lowercase()))
    }
    fn count(&self, c: Category) -> usize {
        self.lists.iter().find(|(k, ..)| *k == c).map_or(0, |(_, names, _)| names.len())
    }
    fn json(&self) -> Value {
        let mut out = serde_json::Map::new();
        for (c, names, _) in &self.lists {
            let page: Vec<&String> = names.iter().take(MAX_NAMES).collect();
            out.insert(
                c.plural().into(),
                json!({
                    "count": names.len(),
                    "returned": page.len(),
                    "truncated": page.len() < names.len(),
                    "names": page,
                }),
            );
        }
        Value::Object(out)
    }
}

fn item_json(
    c: Category,
    name: &str,
    entities: usize,
    block_entities: usize,
    references: usize,
    used: bool,
    empty: bool,
    current: Option<bool>,
    unused: &Unused,
) -> Value {
    let mut v = json!({
        "type": c.key(),
        "name": name,
        "entities": entities,
        "blockEntities": block_entities,
        "references": references,
        "used": used,
        "insideBlocksOnly": used && entities == 0 && block_entities > 0 && references == 0,
        "empty": empty,
        "unused": unused.has(c, name),
    });
    if let (Some(cur), Some(o)) = (current, v.as_object_mut()) {
        o.insert("current".into(), json!(cur));
    }
    v
}

fn items(scan: &Scan, unused: &Unused, d: &Drawing) -> Vec<Value> {
    let mut out = Vec::new();
    let current_layer = d.header.str("CLAYER", "0");
    for l in &d.layers {
        let (e, b) = scan.layers.get(&l.name.to_ascii_lowercase()).copied().unwrap_or_default();
        let uses = e.saturating_add(b);
        let cur = l.name.eq_ignore_ascii_case(&current_layer);
        out.push(item_json(Category::Layer, &l.name, e, b, 0, uses > 0, uses == 0, Some(cur), unused));
    }
    for b in d.blocks.values() {
        let (e, n) = scan.blocks.get(&b.name.to_ascii_lowercase()).copied().unwrap_or_default();
        // A definition is used when a reference to it is reachable, as PURGE counts it.
        let used = scan.used_blocks.contains(&b.name.to_ascii_uppercase());
        out.push(item_json(Category::Block, &b.name, e, n, 0, used, e.saturating_add(n) == 0, None, unused));
    }
    let current_style = d.header.str("TEXTSTYLE", "Standard");
    for t in &d.text_styles {
        let (e, b) = scan.styles.get(&t.name.to_ascii_lowercase()).copied().unwrap_or_default();
        let r = scan.style_refs.get(&t.name.to_ascii_lowercase()).copied().unwrap_or(0);
        let uses = e.saturating_add(b).saturating_add(r);
        let cur = t.name.eq_ignore_ascii_case(&current_style);
        out.push(item_json(Category::TextStyle, &t.name, e, b, r, uses > 0, uses == 0, Some(cur), unused));
    }
    let current_linetype = d.header.str("CELTYPE", "ByLayer");
    for t in &d.linetypes {
        let (e, b) = scan.linetypes.get(&t.name.to_ascii_lowercase()).copied().unwrap_or_default();
        let r = scan.linetype_refs.get(&t.name.to_ascii_lowercase()).copied().unwrap_or(0);
        let uses = e.saturating_add(b).saturating_add(r);
        let cur = t.name.eq_ignore_ascii_case(&current_linetype);
        out.push(item_json(Category::Linetype, &t.name, e, b, r, uses > 0, uses == 0, Some(cur), unused));
    }
    out
}

fn summary(items: &[Value], unused: &Unused) -> Value {
    let mut out = serde_json::Map::new();
    for c in Category::ALL {
        let mut total = 0usize;
        let mut used = 0usize;
        let mut inside = 0usize;
        let mut empty = 0usize;
        for it in items.iter().filter(|i| i["type"] == c.key()) {
            total = total.saturating_add(1);
            used = used.saturating_add(usize::from(it["used"].as_bool() == Some(true)));
            inside = inside.saturating_add(usize::from(it["insideBlocksOnly"].as_bool() == Some(true)));
            empty = empty.saturating_add(usize::from(it["empty"].as_bool() == Some(true)));
        }
        out.insert(c.plural().into(), json!({ "total": total, "used": used, "insideBlocksOnly": inside, "empty": empty, "unused": unused.count(c) }));
    }
    Value::Object(out)
}

fn message(items: &[&Value], summary: &Value) -> String {
    let mut lines = Vec::new();
    for c in Category::ALL {
        let s = &summary[c.plural()];
        lines.push(format!(
            "{}: {} total, {} used, {} inside blocks only, {} empty, {} unused.",
            c.label(),
            s["total"].as_u64().unwrap_or(0),
            s["used"].as_u64().unwrap_or(0),
            s["insideBlocksOnly"].as_u64().unwrap_or(0),
            s["empty"].as_u64().unwrap_or(0),
            s["unused"].as_u64().unwrap_or(0),
        ));
    }
    if items.is_empty() {
        lines.push("There are no named objects of this type.".into());
    }
    lines.join("\n")
}

pub fn run(s: &mut Session, p: &Value) -> Result<Value> {
    let given = |k: &str| p.get(k).filter(|v| !v.is_null());
    let count = |k: &str, default: usize| -> Result<usize> {
        given(k).map_or(Ok(default), |v| {
            v.as_u64().map(|n| usize::try_from(n).unwrap_or(usize::MAX)).ok_or_else(|| bad(CMD, format!("`{k}` must be a whole number, 0 or more")))
        })
    };
    let ty = match given("type") {
        None => None,
        Some(Value::String(t)) => {
            Category::parse(t).ok_or_else(|| bad(CMD, format!("`type` must be layers, blocks, textstyles, linetypes or all, got \"{t}\"")))?
        }
        Some(v) => return Err(bad(CMD, format!("`type` must be a string, got {v}"))),
    };
    let limit = count("limit", 200)?;
    let offset = count("offset", 0)?;
    let st = s.state()?;
    let d = &st.doc;
    let scan = Scan::new(d);
    let unused = Unused::new(d);
    let all = items(&scan, &unused, d);
    let matched: Vec<&Value> = all.iter().filter(|i| ty.is_none_or(|c| i["type"] == c.key())).collect();
    let total = matched.len();
    let page: Vec<&Value> = matched.iter().skip(offset).take(limit).copied().collect();
    let summary = summary(&all, &unused);
    Ok(json!({
        "schema": 1,
        "count": total,
        "returned": page.len(),
        "offset": offset,
        "truncated": page.len() < total.saturating_sub(offset),
        "items": page,
        "summary": summary,
        "unused": unused.json(),
        "message": message(&matched, &summary),
    }))
}
