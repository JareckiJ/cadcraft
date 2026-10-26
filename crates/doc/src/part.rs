//! Persistent reference and feature identities for native part documents.
use crate::Handle;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlaneBasis {
    XY,
    XZ,
    YZ,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatumPlane {
    pub id: u32,
    pub name: String,
    pub basis: PlaneBasis,
    pub parent: Option<u32>,
    pub offset: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartSketch {
    pub id: u32,
    pub name: String,
    pub plane: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face: Option<FaceReference>,
    pub profile: Handle,
    #[serde(default)]
    pub entities: Vec<Handle>,
    #[serde(default)]
    pub width_parameter: String,
    #[serde(default)]
    pub height_parameter: String,
}

/// A planar mesh face in a particular stage of the ordered feature history.
/// The expected outward normal detects incompatible topology changes.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FaceReference {
    pub feature: u32,
    pub body: u32,
    pub face: u32,
    pub normal: [f64; 3],
}

impl PartSketch {
    pub fn members(&self) -> Vec<Handle> {
        if self.entities.is_empty() { vec![self.profile] } else { self.entities.clone() }
    }
    pub fn width_name(&self) -> &str {
        if self.width_parameter.is_empty() { "width" } else { &self.width_parameter }
    }
    pub fn height_name(&self) -> &str {
        if self.height_parameter.is_empty() { "height" } else { &self.height_parameter }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SolidOperation {
    #[default]
    NewBody,
    Add,
    Remove,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartExtrusion {
    pub id: u32,
    pub name: String,
    pub sketch: u32,
    pub depth: String,
    pub operation: SolidOperation,
    pub target: Option<u32>,
    #[serde(default)]
    pub reverse: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartModel {
    pub units: String,
    pub planes: Vec<DatumPlane>,
    pub sketches: Vec<PartSketch>,
    #[serde(default)]
    pub features: Vec<PartExtrusion>,
}

impl PartModel {
    pub fn new(metric: bool) -> Self {
        Self {
            units: if metric { "mm" } else { "in" }.into(),
            sketches: Vec::new(),
            features: Vec::new(),
            planes: [(1, "XY", PlaneBasis::XY), (2, "XZ", PlaneBasis::XZ), (3, "YZ", PlaneBasis::YZ)]
                .into_iter()
                .map(|(id, name, basis)| DatumPlane { id, name: name.into(), basis, parent: None, offset: "0".into() })
                .collect(),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if !matches!(self.units.as_str(), "mm" | "in") || self.planes.len() > 128 || self.sketches.len() > 128 || self.features.len() > 128 {
            return Err("Unsupported part units or feature count".into());
        }
        let mut ids = std::collections::BTreeSet::new();
        for plane in &self.planes {
            if plane.id == 0 || plane.id >= 1000 || !ids.insert(plane.id) || plane.name.len() > 128 || plane.offset.len() > 1024 {
                return Err("Invalid datum plane identity or definition".into());
            }
            let mut next = plane.parent;
            for depth in 0..=self.planes.len() {
                let Some(id) = next else { break };
                if id == plane.id || depth == self.planes.len() {
                    return Err("Cyclic datum plane dependency".into());
                }
                let parent = self.planes.iter().find(|p| p.id == id).ok_or("Missing datum parent")?;
                if parent.basis != plane.basis {
                    return Err("Offset planes must inherit their parent orientation".into());
                }
                next = parent.parent;
            }
        }
        for sketch in &self.sketches {
            if sketch.id < 1000
                || sketch.id >= 2000
                || !ids.insert(sketch.id)
                || !self.planes.iter().any(|p| p.id == sketch.plane)
                || sketch.entities.len() > 4096
                || sketch.name.len() > 128
            {
                return Err("Invalid sketch identity or plane".into());
            }
        }
        let mut members = std::collections::BTreeSet::new();
        for sketch in &self.sketches {
            if !sketch.members().contains(&sketch.profile) || sketch.members().iter().any(|h| !members.insert(*h)) {
                return Err("Invalid or shared sketch geometry ownership".into());
            }
        }
        let mut bodies = std::collections::BTreeSet::new();
        let mut consumed = std::collections::BTreeSet::new();
        for f in &self.features {
            if f.id < 2000
                || f.id >= 3000
                || !ids.insert(f.id)
                || !self.sketches.iter().any(|s| s.id == f.sketch)
                || !consumed.insert(f.sketch)
                || f.name.len() > 128
                || f.depth.len() > 1024
            {
                return Err("Invalid feature identity, sketch reference or duplicate sketch ownership".into());
            }
            match f.operation {
                SolidOperation::NewBody => {
                    if f.target.is_some() {
                        return Err("New body cannot have a target".into());
                    }
                    bodies.insert(f.id);
                }
                _ => {
                    if !f.target.is_some_and(|id| bodies.contains(&id)) {
                        return Err("Modification must target a body from an earlier feature".into());
                    }
                }
            }
        }
        for sketch in &self.sketches {
            if let Some(face) = sketch.face {
                let source = self.features.iter().position(|f| f.id == face.feature).ok_or("Missing face source feature")?;
                let feature = self.features.get(source).ok_or("Missing face source")?;
                if face.body != feature.target.unwrap_or(feature.id)
                    || face.face >= 16384
                    || face.normal.iter().any(|v| !v.is_finite())
                    || (face.normal.iter().map(|v| v * v).sum::<f64>() - 1.0).abs() > 1e-6
                    || self.features.iter().position(|f| f.sketch == sketch.id).is_some_and(|owner| owner <= source)
                {
                    return Err("Invalid or cyclic sketch face reference".into());
                }
            }
        }
        Ok(())
    }
}

/// Geometry supported by the shared part sketch editor and native storage.
pub fn supported_sketch_geometry(kind: &crate::EntityKind) -> bool {
    use crate::EntityKind as E;
    match kind {
        E::Line(_) | E::Circle(_) | E::Arc(_) | E::Ellipse(_) | E::Point(_) | E::Ray(_) | E::XLine(_) => true,
        E::LwPolyline(p) => p.vertices.len() <= 4096,
        E::Spline(s) => s.degree <= 64 && s.control.len() <= 4096 && s.fit.len() <= 4096 && s.knots.len() <= 8192 && s.weights.len() <= 4096,
        E::Region(r) => r.loops.iter().map(|l| l.vertices.len()).sum::<usize>() <= 4096,
        _ => false,
    }
}
