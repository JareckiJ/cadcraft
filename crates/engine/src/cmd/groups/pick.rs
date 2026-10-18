//! PICKSTYLE: with group selection on (bit 1, the default), picking or windowing an object of a
//! selectable group selects the whole group. Ctrl+Shift+A (the `pickstyle` command without a
//! value) turns group selection on and off.

use std::collections::{HashMap, HashSet};

use cadcraft_doc::{EntityKind, Handle};
use serde_json::{Value, json};

use super::super::*;
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("pickstyle", "Group Selection", run)
            .key("Cmd+Shift+A")
            .params("{value?: 0 none | 1 group selection | 2 associative hatch | 3 both} (no value: group selection on/off) → {value, message}")
            .enabled(always)
            .noundo()
            .transparent()
            .interactive(|s| Ok(Box::new(PickstyleM { current: s.settings.pickstyle }))),
    ]
}

/// The commands whose object prompts pick single members, not whole groups.
const MEMBER_PICKS: &[&str] = &["groupedit", "-group", "hatchedit", "-hatchedit"];

/// `hs` plus the other members of the selectable groups they belong to (in the space being
/// edited, visible), when PICKSTYLE has group selection on (bit 1), and associative hatches
/// and their boundary sources when PICKSTYLE has associative hatch selection on (bit 2).
/// Order: `hs` first.
pub(crate) fn expand(s: &Session, hs: Vec<Handle>) -> Vec<Handle> {
    if hs.is_empty() || s.running.as_ref().is_some_and(|r| MEMBER_PICKS.contains(&r.id.as_str())) {
        return hs;
    }
    let pick_groups = s.settings.pickstyle & 1 != 0;
    let pick_hatch = s.settings.pickstyle & 2 != 0;
    if !pick_groups && !pick_hatch {
        return hs;
    }
    let Ok(d) = s.doc() else { return hs };
    let space = s.space();
    let Some(store) = d.space(&space) else { return hs };

    // Member → its selectable groups, built once.
    let mut groups_of: HashMap<Handle, Vec<usize>> = HashMap::new();
    if pick_groups && d.groups.iter().any(|g| g.selectable) {
        for (i, g) in d.groups.iter().enumerate().filter(|(_, g)| g.selectable) {
            for m in &g.members {
                groups_of.entry(*m).or_default().push(i);
            }
        }
    }

    // Associative hatch <-> boundary sources mapping, built once.
    let mut hatch_to_sources: HashMap<Handle, Vec<Handle>> = HashMap::new();
    let mut source_to_hatches: HashMap<Handle, Vec<Handle>> = HashMap::new();
    if pick_hatch {
        for e in store.iter() {
            if let EntityKind::Hatch(ref h) = e.kind
                && h.associative
                && d.is_visible(e)
            {
                let mut sources = Vec::new();
                for l in &h.loops {
                    for src in &l.sources {
                        if store.get(*src).is_some_and(|be| d.is_visible(be)) {
                            sources.push(*src);
                            source_to_hatches.entry(*src).or_default().push(e.handle);
                        }
                    }
                }
                if !sources.is_empty() {
                    hatch_to_sources.insert(e.handle, sources);
                }
            }
        }
    }

    // Clipped viewport <-> boundary entity pairing.
    let mut vp_to_boundary: HashMap<Handle, Handle> = HashMap::new();
    let mut boundary_to_vp: HashMap<Handle, Handle> = HashMap::new();
    for e in store.iter() {
        if let EntityKind::Viewport(ref vp) = e.kind
            && let Some(bh) = vp.clip
            && store.get(bh).is_some_and(|be| d.is_visible(be))
        {
            vp_to_boundary.insert(e.handle, bh);
            boundary_to_vp.insert(bh, e.handle);
        }
    }

    let mut seen: HashSet<Handle> = hs.iter().copied().collect();
    let mut done_groups: HashSet<usize> = HashSet::new();
    let mut out = hs.clone();
    let mut i = 0;
    while i < out.len() {
        let h = out[i];
        i += 1;

        if let Some(&bh) = vp_to_boundary.get(&h)
            && seen.insert(bh)
        {
            out.push(bh);
        }
        if let Some(&vh) = boundary_to_vp.get(&h)
            && seen.insert(vh)
        {
            out.push(vh);
        }

        if pick_groups {
            for gi in groups_of.get(&h).into_iter().flatten() {
                if !done_groups.insert(*gi) {
                    continue;
                }
                let Some(g) = d.groups.get(*gi) else { continue };
                for m in &g.members {
                    if store.get(*m).is_some_and(|e| d.is_visible(e)) && seen.insert(*m) {
                        out.push(*m);
                    }
                }
            }
        }

        if pick_hatch {
            if let Some(sources) = hatch_to_sources.get(&h) {
                for src in sources {
                    if seen.insert(*src) {
                        out.push(*src);
                    }
                }
            }
            if let Some(hatches) = source_to_hatches.get(&h) {
                for hatch in hatches {
                    if seen.insert(*hatch) {
                        out.push(*hatch);
                    }
                }
            }
        }
    }
    out
}

fn run(s: &mut Session, p: &Value) -> Result<Value> {
    let v = match p.get("value") {
        None | Some(Value::Null) => s.settings.pickstyle ^ 1,
        Some(v) => v
            .as_u64()
            .or_else(|| v.as_str().and_then(|t| t.trim().parse().ok()))
            .filter(|v| *v <= 3)
            .and_then(|v| u8::try_from(v).ok())
            .ok_or_else(|| bad("pickstyle", "`value` must be 0, 1, 2 or 3"))?,
    };
    s.settings.pickstyle = v;
    let msg = format!("<Group selection {}>", if v & 1 != 0 { "on" } else { "off" });
    Ok(json!({ "value": v, "message": msg }))
}

/// Typed PICKSTYLE: "Enter new value for PICKSTYLE <1>:".
struct PickstyleM {
    current: u8,
}

impl Interactive for PickstyleM {
    fn name(&self) -> &'static str {
        "PICKSTYLE"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new("Enter new value for PICKSTYLE", Accept::NUMBER).default(self.current.to_string())
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Enter => Ok(Step::Done),
            Input::Text(t) => match run(s, &json!({ "value": t })) {
                Ok(_) => Ok(Step::Done),
                Err(_) => {
                    s.echo("Requires an integer between 0 and 3.");
                    Ok(Step::Continue)
                }
            },
            _ => Ok(Step::Continue),
        }
    }
}
