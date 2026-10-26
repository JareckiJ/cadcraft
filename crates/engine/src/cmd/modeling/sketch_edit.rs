//! Shared drawing engine hosted in a transactional part sketch editor.
use super::{bad, id_param, plane_frame, validate};
use crate::{DocState, Result, Session, Snapshot};
use cadcraft_doc::{Drawing, EntityStore, FaceReference, PartSketch};
use serde_json::{Value, json};
use std::{collections::BTreeSet, sync::Arc};

#[derive(Clone, Debug)]
pub struct Edit {
    pub edit_id: u64,
    pub frame: super::FaceFrame,
    pub host: Box<DocState>,
    pub plane: u32,
    pub sketch: Option<u32>,
    pub face: Option<FaceReference>,
    shadows: BTreeSet<String>,
}

pub fn begin(s: &mut Session, p: &Value) -> Result<Value> {
    if s.state()?.part_edit.is_some() || s.state()?.block_edit.is_some() {
        return Err(bad("part.sketch.begin", "Finish or cancel the current editor first"));
    }
    let host = s.state()?.clone();
    let part = host.doc.part.as_ref().ok_or_else(|| bad("part.sketch.begin", "Create a 3D Part first"))?;
    let id = id_param(p, "sketch")?;
    let sketch = id.map(|id| part.sketches.iter().find(|sk| sk.id == id).ok_or_else(|| bad("part.sketch.begin", "Missing sketch"))).transpose()?;
    let plane = sketch.map_or(id_param(p, "plane")?.unwrap_or(1), |sk| sk.plane);
    let face = if let Some(sketch) = sketch {
        sketch.face
    } else {
        p.get("face")
            .filter(|v| !v.is_null())
            .map(|v| serde_json::from_value::<FaceReference>(v.clone()).map_err(|e| bad("part.sketch.begin", e.to_string())))
            .transpose()?
    };
    let frame = if let Some(reference) = face { super::support_frame(&host.doc, &reference)? } else { plane_frame(&host.doc, plane)? };
    let members: BTreeSet<_> = sketch.map(PartSketch::members).unwrap_or_default().into_iter().collect();
    let mut draft = (*host.doc).clone();
    draft.part = None;
    draft.model = EntityStore::new();
    for e in host.doc.model.iter().filter(|e| members.contains(&e.handle)) {
        draft.model.push((**e).clone());
    }
    draft.constraints.retain(|c| !c.refs.is_empty() && c.refs.iter().all(|r| members.contains(&r.handle)));
    let mut shadows = BTreeSet::new();
    // Reserve dimensional names belonging to other sketches so new dimensions stay unique.
    for row in super::parameters(&host.doc) {
        if host.doc.constraints.iter().any(|c| c.name == row.name)
            && !draft.constraints.iter().any(|c| c.name == row.name)
            && let Some(value) = row.value
        {
            shadows.insert(row.name.clone());
            draft.parametric.parameters.push(cadcraft_doc::Parameter { name: row.name, expr: value.to_string(), description: String::new() });
        }
    }
    s.cancel();
    let mut editor = DocState::new(draft, &host.title, None);
    let edit_id = editor.uid;
    editor.uid = host.uid;
    editor.part_edit = Some(Edit { edit_id, frame, host: Box::new(host), plane, sketch: id, face, shadows });
    *s.state_mut()? = editor;
    let _ = s.zoom_extents();
    if id.is_none()
        && let Some(reference) = face
    {
        let edit = s.state()?.part_edit.as_ref().ok_or_else(|| bad("sketch", "Missing editor"))?;
        if let Some(body) = super::bodies(&edit.host.doc)?.iter().find(|b| b.id == reference.body)
            && let Some(points) = body.face_points(reference.face as usize)
        {
            let bounds = cadcraft_geom::Bounds2::from_points(
                points.iter().map(|p| cadcraft_geom::Vec2::new((*p - frame.0).dot(frame.1), (*p - frame.0).dot(frame.2))),
            );
            let mut view = s.state()?.view();
            view.center = (bounds.min + bounds.max) * 0.5;
            view.height = ((bounds.max - bounds.min).len() * 1.7).max(20.0);
            s.state_mut()?.set_view(view);
        }
    }
    let edit = s.state()?.part_edit.as_ref().ok_or_else(|| bad("sketch", "Missing editor"))?;
    let context = super::bodies_before_sketch(&edit.host.doc, id)?;
    if !context.is_empty() {
        let mut bounds = cadcraft_geom::Bounds2::EMPTY;
        for body in &context {
            for p in &body.vertices {
                let p = cadcraft_geom::Vec3::new(p[0], p[1], p[2]) - frame.0;
                bounds.add(cadcraft_geom::Vec2::new(p.dot(frame.1), p.dot(frame.2)));
            }
        }
        let view = s.state()?.view();
        let aspect = (s.viewport_px.0 / s.viewport_px.1.max(1.0)).clamp(0.2, 10.0);
        if id.is_some() {
            let half = cadcraft_geom::Vec2::new(view.height * aspect * 0.5, view.height * 0.5);
            bounds = bounds.union(&cadcraft_geom::Bounds2::new(view.center - half, view.center + half));
        }
        let mut view = view;
        view.center = (bounds.min + bounds.max) * 0.5;
        view.height = ((bounds.max.y - bounds.min.y).max((bounds.max.x - bounds.min.x) / aspect) * 1.2).max(10.0);
        s.state_mut()?.set_view(view);
    }
    Ok(json!({"sketch":id,"plane":plane}))
}

impl Edit {
    pub fn compose(&self, draft: &Drawing) -> Result<(Drawing, u32)> {
        if draft.model.is_empty() || draft.model.len() > 4096 || draft.constraints.len() > 5000 {
            return Err(bad("part.sketch", "Sketch must contain between 1 and 4096 objects"));
        }
        if draft.model.iter().any(|e| !cadcraft_doc::supported_sketch_geometry(&e.kind) || !crate::finite::is_finite(&**e)) {
            return Err(bad("part.sketch", "Sketch contains unsupported or invalid geometry"));
        }
        let mut d = (*self.host.doc).clone();
        let old = d.part.as_ref().and_then(|p| p.sketches.iter().find(|sk| Some(sk.id) == self.sketch)).cloned();
        let members: BTreeSet<_> = old.as_ref().map(PartSketch::members).unwrap_or_default().into_iter().collect();
        for h in &members {
            d.model.remove(*h);
        }
        d.constraints.retain(|c| !c.refs.iter().any(|r| members.contains(&r.handle)));
        let mut new_members = Vec::new();
        for e in draft.model.iter() {
            if d.entity(e.handle).is_some() {
                return Err(bad("part.sketch", "Sketch entity identity conflict"));
            }
            d.bump_handseed(e.handle);
            new_members.push(e.handle);
            d.model.push((**e).clone());
        }
        let member_set: BTreeSet<_> = new_members.iter().copied().collect();
        for c in &draft.constraints {
            if !c.refs.iter().all(|r| member_set.contains(&r.handle)) {
                return Err(bad("part.sketch", "Missing constraint reference"));
            }
            let mut c = c.clone();
            c.id = cadcraft_constraints::next_id(&d);
            d.constraints.push(c);
        }
        for p in &draft.parametric.parameters {
            if self.shadows.contains(&p.name) {
                continue;
            }
            if let Some(old) = d.parametric.parameters.iter_mut().find(|old| old.name == p.name) {
                *old = p.clone();
            } else {
                d.parametric.parameters.push(p.clone());
            }
        }
        let profile = *new_members.first().ok_or_else(|| bad("part.sketch", "Empty sketch"))?;
        let part = d.part.as_mut().ok_or_else(|| bad("part.sketch", "Missing part"))?;
        let id = match old {
            Some(old) => {
                let sk = part.sketches.iter_mut().find(|sk| sk.id == old.id).ok_or_else(|| bad("part.sketch", "Missing sketch"))?;
                sk.profile = profile;
                sk.entities = new_members;
                old.id
            }
            None => {
                let id = part.sketches.iter().map(|sk| sk.id).max().unwrap_or(999) + 1;
                if id >= 2000 || part.sketches.len() >= 128 {
                    return Err(bad("part.sketch", "Sketch limit reached"));
                }
                part.sketches.push(PartSketch {
                    id,
                    name: format!("Sketch {}", id - 999),
                    plane: self.plane,
                    face: self.face,
                    profile,
                    entities: new_members,
                    width_parameter: String::new(),
                    height_parameter: String::new(),
                });
                id
            }
        };
        cadcraft_constraints::solve(&mut d, &Default::default()).map_err(|e| bad("part.sketch", e.to_string()))?;
        Ok((d, id))
    }
}

pub fn candidate(s: &Session, p: &Value) -> Result<(Drawing, Value)> {
    let edit = s.state()?.part_edit.as_ref().ok_or_else(|| bad("part.sketch.commit", "No sketch editor is open"))?;
    let (d, id) = edit.compose(s.doc()?)?;
    if p.get("extrude").and_then(Value::as_bool) == Some(true) {
        let mut trial = Session::new();
        *trial.doc_mut()? = d;
        let mut args = p.clone();
        args["sketch"] = json!(id);
        // The geometry was edited directly; rectangle dimensions must not overwrite it.
        if let Some(o) = args.as_object_mut() {
            o.remove("width");
            o.remove("height");
        }
        let result = trial.execute("extrude", &args)?;
        Ok((trial.doc()?.clone(), result))
    } else {
        validate(&d)?;
        Ok((d, json!({"sketch":id})))
    }
}
pub fn commit(s: &mut Session, p: &Value) -> Result<Value> {
    s.cancel();
    let (d, result) = candidate(s, p)?;
    let edit = s.state_mut()?.part_edit.take().ok_or_else(|| bad("part.sketch.commit", "No sketch editor is open"))?;
    let mut host = *edit.host;
    let before = host.doc.clone();
    let selection = host.selection.clone();
    host.doc = Arc::new(d);
    host.push_undo(Snapshot { label: "Part Sketch".into(), doc: before, selection });
    host.selection.clear();
    host.revision += 1;
    *s.state_mut()? = host;
    Ok(result)
}
pub fn cancel(s: &mut Session, _: &Value) -> Result<Value> {
    s.cancel();
    let edit = s.state_mut()?.part_edit.take().ok_or_else(|| bad("part.sketch.cancel", "No sketch editor is open"))?;
    *s.state_mut()? = *edit.host;
    Ok(Value::Null)
}

pub fn refused(s: &Session, id: &str) -> Option<String> {
    s.state().ok()?.part_edit.as_ref()?;
    // File and editor swaps would detach this draft from its native part.
    (matches!(
        id,
        "new"
            | "open"
            | "close"
            | "save"
            | "saveas"
            | "qsave"
            | "bedit"
            | "refedit"
            | "part.new"
            | "part.sketch.begin"
            | "part.sketch"
            | "part.plane"
            | "extrude"
    ) || id.starts_with("export")
        || id.starts_with("file."))
    .then(|| "Finish or cancel the part sketch before this command".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::modeling::{bodies, profile};
    fn status(s: &Session) -> profile::Profile {
        let d = s.doc().unwrap();
        profile::analyze(d, &d.model.iter().map(|e| e.handle).collect::<Vec<_>>())
    }
    #[test]
    fn sketch_recovery_preserves_draft_or_reports_failure_without_saving_old_host() {
        let mut s = Session::new();
        s.execute("part.new", &json!({})).unwrap();
        s.execute("part.sketch.begin", &json!({"plane":1})).unwrap();
        assert!(s.state().unwrap().recovery_drawing().is_err());
        let mut autosaver = crate::autosave::Autosaver::new("55-b");
        autosaver.plan(&s, 0.0, 1, "/auto");
        assert!(autosaver.plan(&s, 60.0, 1, "/auto").is_empty());
        assert!(autosaver.plan(&s, 120.0, 1, "/auto").is_empty());
        s.execute("line", &json!({"points":[[0,0],[10,0]]})).unwrap();
        let recovered = s.state().unwrap().recovery_drawing().unwrap();
        assert_eq!(recovered.part.as_ref().unwrap().sketches.len(), 1);
        assert_eq!(recovered.model.len(), 1);
        assert!(s.state().unwrap().drawing().part.as_ref().unwrap().sketches.is_empty());
        let jobs = autosaver.plan(&s, 180.0, 1, "/auto");
        assert!(jobs.iter().any(|job| matches!(job, crate::autosave::Job::Write { doc, .. } if doc.as_ref() == recovered.as_ref())));
        let bytes = cadcraft_io::write(&recovered, "recovery.cadcraft").unwrap();
        assert_eq!(cadcraft_io::read(&bytes, "recovery.cadcraft").unwrap(), *recovered);
    }
    #[test]
    fn shared_line_tools_closed_shading_validation_extrusion_and_history() {
        let mut s = Session::new();
        s.execute("part.new", &json!({})).unwrap();
        let before = s.doc().unwrap().clone();
        s.execute("part.sketch.begin", &json!({"plane":1})).unwrap();
        s.execute("line", &json!({"points":[[0,0],[40,0],[40,20],[0,20]]})).unwrap();
        assert!(!status(&s).ready());
        assert!(s.execute("part.sketch.commit", &json!({"extrude":true,"depth":10})).is_err());
        assert_eq!(s.state().unwrap().drawing().as_ref(), &before);
        assert!(s.execute("saveas", &json!({"path":"should-not-save.dxf"})).is_err());
        s.execute("line", &json!({"points":[[0,20],[0,0]]})).unwrap();
        assert!(status(&s).ready(), "{}", status(&s).message());
        assert!(!status(&s).triangles.is_empty());
        s.execute("part.sketch.commit", &json!({"extrude":true,"depth":10})).unwrap();
        assert!((bodies(s.doc().unwrap()).unwrap()[0].volume - 8000.0).abs() < 1e-5);
        let part = s.doc().unwrap().part.as_ref().unwrap();
        assert_eq!(part.sketches[0].members().len(), 4);
        assert_eq!(part.features[0].sketch, part.sketches[0].id);
        let bytes = cadcraft_io::write(s.doc().unwrap(), "test.cadcraft").unwrap();
        assert_eq!(cadcraft_io::read(&bytes, "test.cadcraft").unwrap(), *s.doc().unwrap());
        s.undo().unwrap();
        assert_eq!(*s.doc().unwrap(), before);
        s.redo().unwrap();
        let saved = s.doc().unwrap().clone();
        s.execute("part.sketch.begin", &json!({"sketch":1000})).unwrap();
        s.execute("circle", &json!({"center":[100,100],"radius":5})).unwrap();
        s.execute("part.sketch.cancel", &Value::Null).unwrap();
        assert_eq!(*s.doc().unwrap(), saved);
    }
    #[test]
    fn circles_holes_and_invalid_crossing_profiles() {
        let mut s = Session::new();
        s.execute("part.new", &json!({})).unwrap();
        s.execute("part.sketch.begin", &json!({})).unwrap();
        for radius in [10, 5] {
            s.execute("circle", &json!({"center":[0,0],"radius":radius})).unwrap();
        }
        assert!(status(&s).ready(), "{}", status(&s).message());
        s.execute("part.sketch.commit", &json!({"extrude":true,"depth":10})).unwrap();
        let body = bodies(s.doc().unwrap()).unwrap().remove(0);
        assert!((body.volume - 750.0 * std::f64::consts::PI).abs() < 10.0);
        let mut volume = 0.0;
        for f in &body.faces {
            for tri in [[f[0], f[1], f[2]], [f[0], f[2], f[3]]] {
                let [a, b, c] = tri.map(|i| {
                    let p = body.vertices[i];
                    cadcraft_geom::Vec3::new(p[0], p[1], p[2])
                });
                volume += a.dot(b.cross(c)) / 6.0;
            }
        }
        assert!((volume - body.volume).abs() < 1e-4, "mesh {volume}, expected {}", body.volume);
        s.execute("part.sketch.begin", &json!({})).unwrap();
        s.execute("line", &json!({"points":[[0,0],[20,20],[0,20],[20,0],[0,0]]})).unwrap();
        assert!(!status(&s).ready());
        assert!(status(&s).triangles.is_empty());
        s.execute("part.sketch.cancel", &Value::Null).unwrap();
    }
    #[test]
    fn shared_spline_arc_ellipse_and_modify_tools_persist_and_extrude() {
        for (cmd, args) in [
            ("spline", json!({"fit":[[0,0],[20,0],[20,20],[0,20]],"closed":true})),
            ("ellipse", json!({"center":[0,0],"major":[20,0],"ratio":0.5})),
            ("arc", json!({"center":[0,0],"radius":10,"start":0,"end":180})),
        ] {
            let mut s = Session::new();
            s.execute("part.new", &json!({})).unwrap();
            s.execute("part.sketch.begin", &json!({"plane":2})).unwrap();
            s.execute(cmd, &args).unwrap();
            if cmd == "arc" {
                s.execute("line", &json!({"points":[[-10,0],[10,0]]})).unwrap();
            }
            assert!(status(&s).ready(), "{cmd}: {}", status(&s).message());
            s.execute("part.sketch.commit", &json!({"extrude":true,"depth":5})).unwrap();
            let original = bodies(s.doc().unwrap()).unwrap()[0].volume;
            let bytes = cadcraft_io::write(s.doc().unwrap(), "curve.cadcraft").unwrap();
            let restored = cadcraft_io::read(&bytes, "curve.cadcraft").unwrap();
            assert!((bodies(&restored).unwrap()[0].volume - original).abs() < 1e-5);
            s.execute("part.sketch.begin", &json!({"sketch":1000})).unwrap();
            let handles: Vec<_> = s.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect();
            s.execute("move", &json!({"handles":handles,"delta":[10,3]})).unwrap();
            s.execute("part.sketch.commit", &Value::Null).unwrap();
            assert!((bodies(s.doc().unwrap()).unwrap()[0].volume - original).abs() < 1e-5);
        }
    }
}
