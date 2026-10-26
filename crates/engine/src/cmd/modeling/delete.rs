//! Transactional history deletion. Source sketches remain editable and reusable.
use super::{Session, Value, bad, id_param, json};
use crate::Result;

pub(super) fn feature(s: &mut Session, params: &Value) -> Result<Value> {
    const CMD: &str = "part.feature.delete";
    if s.state()?.part_edit.is_some() {
        return Err(bad(CMD, "Finish or cancel the active sketch before deleting a feature"));
    }
    let id = id_param(params, "feature")?.ok_or_else(|| bad(CMD, "A feature ID is required"))?;
    let mut drawing = s.doc()?.clone();
    let part = drawing.part.as_mut().ok_or_else(|| bad(CMD, "Open a 3D Part first"))?;
    let index = part.features.iter().position(|f| f.id == id).ok_or_else(|| bad(CMD, "Feature not found"))?;
    let mut dependents = std::collections::BTreeSet::new();
    for f in &part.features {
        if f.target == Some(id) {
            dependents.insert(f.name.clone());
        }
    }
    for sketch in &part.sketches {
        if sketch.face.is_some_and(|face| face.feature == id) {
            let name = part.features.iter().find(|f| f.sketch == sketch.id).map_or(sketch.name.as_str(), |f| f.name.as_str());
            dependents.insert(name.to_owned());
        }
    }
    if !dependents.is_empty() {
        return Err(bad(
            CMD,
            format!(
                "Cannot delete: referenced by {}. Delete dependent features or change their references first.",
                dependents.into_iter().collect::<Vec<_>>().join(", ")
            ),
        ));
    }
    let removed = part.features.remove(index);
    // A later cut may cease to intersect after an earlier additive feature is removed.
    // Regenerate the complete candidate before touching the committed document.
    super::validate(&drawing).map_err(|e| bad(CMD, format!("Cannot delete: remaining model would be invalid: {e}")))?;
    *s.doc_mut()? = drawing;
    Ok(json!({"feature":id,"sketch":removed.sketch,"message":"Feature deleted; source sketch retained"}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deleting_independent_feature_keeps_sketch_and_round_trips_with_undo() {
        let mut s = Session::new();
        s.execute("part.new", &json!({})).unwrap();
        s.execute("extrude", &json!({"width":40,"height":20,"depth":10})).unwrap();
        s.execute("extrude", &json!({"width":5,"height":5,"x":100,"depth":5})).unwrap();
        let before = s.doc().unwrap().clone();
        s.execute("part.feature.delete", &json!({"feature":2000})).unwrap();
        let after = s.doc().unwrap().clone();
        assert_eq!(super::super::bodies(&after).unwrap().len(), 1);
        let part = after.part.as_ref().unwrap();
        assert_eq!(part.sketches.len(), 2);
        assert_eq!(part.features[0].id, 2001);
        s.undo().unwrap();
        assert_eq!(*s.doc().unwrap(), before);
        s.redo().unwrap();
        assert_eq!(*s.doc().unwrap(), after);
        let bytes = cadcraft_io::write(&after, "deleted.cadcraft").unwrap();
        let restored = cadcraft_io::read(&bytes, "deleted.cadcraft").unwrap();
        assert_eq!(restored, after);
    }
    #[test]
    fn dependent_body_and_face_features_prevent_deletion_without_mutation() {
        let mut s = Session::new();
        s.execute("part.new", &json!({})).unwrap();
        s.execute("extrude", &json!({"width":40,"height":20,"depth":10})).unwrap();
        s.execute("extrude", &json!({"width":5,"height":5,"depth":5,"operation":"remove","target":2000})).unwrap();
        let before = s.doc().unwrap().clone();
        let error = s.execute("part.feature.delete", &json!({"feature":2000})).unwrap_err().to_string();
        assert!(error.contains("Extrude 2"));
        assert_eq!(*s.doc().unwrap(), before);
        s.execute("part.feature.delete", &json!({"feature":2001})).unwrap();
        assert_eq!(super::super::bodies(s.doc().unwrap()).unwrap()[0].volume, 8000.0);
        let reference = super::super::face_reference(s.doc().unwrap(), 2000, 1).unwrap();
        s.execute("part.sketch.begin", &json!({"face":reference})).unwrap();
        s.execute("rectang", &json!({"p1":[0,0],"p2":[2,2]})).unwrap();
        s.execute("part.sketch.commit", &json!({})).unwrap();
        let before = s.doc().unwrap().clone();
        assert!(s.execute("part.feature.delete", &json!({"feature":2000})).unwrap_err().to_string().contains("Sketch"));
        assert_eq!(*s.doc().unwrap(), before);
    }
}
