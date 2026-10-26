//! Explicit native part envelope. Source geometry is saved, not runtime caches.
use super::{IoError, Result};
use cadcraft_doc::{Constraint, Drawing, Entity, Parametric, PartModel};
use serde::{Deserialize, Serialize};

const MAX_BYTES: usize = 8 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct File {
    format: String,
    schema_version: u32,
    kind: String,
    part: PartModel,
    entities: Vec<Entity>,
    constraints: Vec<Constraint>,
    parametric: Parametric,
    layers: Vec<cadcraft_doc::Layer>,
    linetypes: Vec<cadcraft_doc::Linetype>,
}
pub fn write(d: &Drawing) -> Result<Vec<u8>> {
    let part = d.part.clone().ok_or_else(|| IoError::Format("Only a 3D Part can be saved as .cadcraft".into()))?;
    part.validate().map_err(IoError::Format)?;
    let bytes = serde_json::to_vec_pretty(&File {
        format: "CADCraft".into(),
        schema_version: 3,
        kind: "part".into(),
        part,
        entities: d.model.iter().map(|e| (**e).clone()).collect(),
        constraints: d.constraints.clone(),
        parametric: d.parametric.clone(),
        layers: d.layers.clone(),
        linetypes: d.linetypes.clone(),
    })
    .map_err(|e| IoError::Format(e.to_string()))?;
    if bytes.len() > MAX_BYTES {
        return Err(IoError::Format("Part exceeds native file size limit".into()));
    }
    // Never replace a user's file with a payload our native reader cannot reopen.
    // This also refuses unsupported geometry added through generic drawing commands.
    read(&bytes)?;
    Ok(bytes)
}
pub fn read(bytes: &[u8]) -> Result<Drawing> {
    if bytes.len() > MAX_BYTES {
        return Err(IoError::Format("Part exceeds native file size limit".into()));
    }
    let mut f: File = serde_json::from_slice(bytes).map_err(|e| IoError::Format(e.to_string()))?;
    if f.format != "CADCraft" || f.kind != "part" || !matches!(f.schema_version, 1..=3) {
        return Err(IoError::Format("Unsupported CADCraft part format or schema version".into()));
    }
    if f.schema_version == 1 && f.part.features.is_empty() {
        if f.parametric.extrusions.len() > 128 {
            return Err(IoError::Format("Legacy feature limit exceeded".into()));
        }
        for (i, old) in f.parametric.extrusions.iter().enumerate() {
            let sketch = f.part.sketches.iter().find(|s| s.profile == old.sketch).ok_or_else(|| IoError::Format("Missing legacy sketch".into()))?;
            f.part.features.push(cadcraft_doc::PartExtrusion {
                id: 2000 + i as u32,
                name: format!("Extrude {}", i + 1),
                sketch: sketch.id,
                depth: old.depth.clone(),
                operation: cadcraft_doc::SolidOperation::NewBody,
                reverse: false,
                target: None,
            });
        }
        f.parametric.extrusions.clear();
    }
    f.part.validate().map_err(IoError::Format)?;
    if f.entities.len() > 4096
        || f.constraints.len() > 5000
        || !f.parametric.extrusions.is_empty()
        || f.layers.len() > 4096
        || f.linetypes.len() > 4096
    {
        return Err(IoError::Format("Part exceeds supported geometry limits".into()));
    }
    let mut d = if f.part.units == "mm" { Drawing::new_metric() } else { Drawing::new_imperial() };
    let mut handles = std::collections::BTreeSet::new();
    for e in f.entities {
        if e.handle.0 == 0 || e.handle.0 == u64::MAX || !handles.insert(e.handle) {
            return Err(IoError::Format("Duplicate or invalid sketch entity ID".into()));
        }
        if !cadcraft_doc::supported_sketch_geometry(&e.kind) {
            return Err(IoError::Format("Unsupported or oversized native sketch geometry".into()));
        }
        d.bump_handseed(e.handle);
        d.model.push(e);
    }
    if f.part.sketches.iter().any(|s| s.members().iter().any(|h| !handles.contains(h)))
        || f.parametric.extrusions.iter().any(|e| !f.part.sketches.iter().any(|s| s.profile == e.sketch))
        || f.constraints.iter().flat_map(|c| &c.refs).any(|r| !handles.contains(&r.handle))
    {
        return Err(IoError::Format("Part contains a missing sketch reference".into()));
    }
    d.part = Some(f.part);
    d.layers = f.layers;
    d.linetypes = f.linetypes;
    d.constraints = f.constraints;
    d.parametric = f.parametric;
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_envelope_rejects_newer_schema_cycles_and_missing_references() {
        let mut d = Drawing::new_metric();
        d.part = Some(PartModel::new(true));
        let bytes = write(&d).unwrap();
        assert!(read(&bytes).unwrap().part.is_some());
        let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        value["schemaVersion"] = serde_json::json!(4);
        assert!(read(&serde_json::to_vec(&value).unwrap()).is_err());
        value["schemaVersion"] = serde_json::json!(1);
        value["part"]["planes"][0]["parent"] = serde_json::json!(1);
        assert!(read(&serde_json::to_vec(&value).unwrap()).is_err());
        d.part.as_mut().unwrap().sketches.push(cadcraft_doc::PartSketch {
            id: 1000,
            name: "Broken sketch".into(),
            plane: 1,
            face: None,
            profile: cadcraft_doc::Handle(99),
            entities: Vec::new(),
            width_parameter: String::new(),
            height_parameter: String::new(),
        });
        assert!(write(&d).is_err(), "refuse to save an unreopenable part");
        assert!(write(&Drawing::new_metric()).is_err());
    }
}
