//! Planar support references resolved against an earlier stage of feature history.
use super::{Body, bad, plane_frame, regenerate};
use crate::Result;
use cadcraft_doc::{Drawing, FaceReference};
use cadcraft_geom::Vec3;
pub type FaceFrame = (Vec3, Vec3, Vec3, Vec3);

impl Body {
    pub fn face_points(&self, index: usize) -> Option<[Vec3; 4]> {
        let face = self.faces.get(index)?;
        let point = |i: usize| self.vertices.get(i).map(|p| Vec3::new(p[0], p[1], p[2]));
        Some([point(*face.first()?)?, point(*face.get(1)?)?, point(*face.get(2)?)?, point(*face.get(3)?)?])
    }
    pub fn face_normal(&self, index: usize) -> Option<Vec3> {
        let [a, b, c, q] = self.face_points(index)?;
        let n = (b - a).cross(c - a) + (c - a).cross(q - a);
        (n.is_finite() && n.len() > 1e-10).then(|| n.normalized())
    }
}
pub(super) fn from_reference(body: &Body, reference: &FaceReference) -> Result<FaceFrame> {
    let index = reference.face as usize;
    let n = body.face_normal(index).ok_or_else(|| bad("sketch", "The referenced face no longer exists; reselect a face"))?;
    let expected = Vec3::new(reference.normal[0], reference.normal[1], reference.normal[2]);
    if n.dot(expected) < 1.0 - 1e-7 {
        return Err(bad("sketch", "The referenced face changed orientation; reselect a face"));
    }
    if body.cells.is_empty() && !body.planar_faces.get(index).copied().unwrap_or(false) {
        return Err(bad("sketch", "Select a planar end face. Curved side walls cannot support a planar sketch yet."));
    }
    let points = body.face_points(index).ok_or_else(|| bad("sketch", "Missing face"))?;
    let a = points[0];
    if points.iter().any(|p| (*p - a).dot(n).abs() > 1e-6) {
        return Err(bad("sketch", "Select a planar face"));
    }
    let axis = if n.x.abs() < 0.9 { Vec3::new(1.0, 0.0, 0.0) } else { Vec3::new(0.0, 1.0, 0.0) };
    let u = (axis - n * axis.dot(n)).normalized();
    let v = n.cross(u);
    Ok((n * n.dot(a), u, v, n))
}
pub fn face_reference(d: &Drawing, body_id: u32, index: usize) -> Result<FaceReference> {
    let part = d.part.as_ref().ok_or_else(|| bad("sketch", "Create a part first"))?;
    let feature = part.features.iter().rev().find(|f| f.target.unwrap_or(f.id) == body_id).ok_or_else(|| bad("sketch", "Missing source feature"))?;
    let bodies = super::bodies(d)?;
    let body = bodies.iter().find(|b| b.id == body_id).ok_or_else(|| bad("sketch", "Missing body"))?;
    let n = body.face_normal(index).ok_or_else(|| bad("sketch", "Missing face"))?;
    let face = u32::try_from(index).map_err(|_| bad("sketch", "Invalid face index"))?;
    let reference = FaceReference { feature: feature.id, body: body_id, face, normal: [n.x, n.y, n.z] };
    from_reference(body, &reference)?;
    Ok(reference)
}
pub fn support_frame(d: &Drawing, reference: &FaceReference) -> Result<FaceFrame> {
    let body = support_body(d, reference)?;
    from_reference(&body, reference)
}
fn support_body(d: &Drawing, reference: &FaceReference) -> Result<Body> {
    let part = d.part.as_ref().ok_or_else(|| bad("sketch", "Missing part"))?;
    let count = part.features.iter().position(|f| f.id == reference.feature).ok_or_else(|| bad("sketch", "Missing support feature"))? + 1;
    let (bodies, _) = regenerate(d, count)?;
    let body = bodies.iter().find(|b| b.id == reference.body).ok_or_else(|| bad("sketch", "Missing support body"))?;
    Ok(body.clone())
}
pub fn support_edges(d: &Drawing, reference: &FaceReference) -> Result<Vec<[cadcraft_geom::Vec2; 2]>> {
    let body = support_body(d, reference)?;
    let (origin, u, v, n) = from_reference(&body, reference)?;
    Ok(body
        .display_edges()
        .iter()
        .filter(|edge| (edge.a - origin).dot(n).abs() < 1e-6 && (edge.b - origin).dot(n).abs() < 1e-6)
        .map(|edge| [edge.a, edge.b].map(|p| cadcraft_geom::Vec2::new((p - origin).dot(u), (p - origin).dot(v))))
        .collect())
}
pub fn sketch_frame(d: &Drawing, id: u32) -> Result<FaceFrame> {
    let sketch = d.part.as_ref().and_then(|p| p.sketches.iter().find(|s| s.id == id)).ok_or_else(|| bad("sketch", "Missing sketch"))?;
    match sketch.face {
        Some(reference) => support_frame(d, &reference),
        None => plane_frame(d, sketch.plane),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Session,
        cmd::modeling::{bodies, extrusion_guide, validate},
    };
    use serde_json::json;
    #[test]
    fn face_sketch_add_reverse_cut_regeneration_and_native_roundtrip() {
        let mut s = Session::new();
        s.execute("part.new", &json!({})).unwrap();
        s.execute("extrude", &json!({"width":40,"height":20,"depth":10})).unwrap();
        let reference = face_reference(s.doc().unwrap(), 2000, 1).unwrap();
        assert_eq!(reference.normal, [0.0, 0.0, 1.0]);
        s.execute("part.sketch.begin", &json!({"face":reference})).unwrap();
        s.execute("rectang", &json!({"p1":[2,2],"p2":[12,12]})).unwrap();
        s.execute("part.sketch.commit", &json!({"extrude":true,"depth":5,"operation":"add","target":2000})).unwrap();
        assert!((bodies(s.doc().unwrap()).unwrap()[0].volume - 8500.0).abs() < 1e-5);
        s.execute("extrude", &json!({"feature":2000,"depth":15})).unwrap();
        assert!((sketch_frame(s.doc().unwrap(), 1001).unwrap().0.z - 15.0).abs() < 1e-6);
        assert!((bodies(s.doc().unwrap()).unwrap()[0].volume - 12500.0).abs() < 1e-5);
        let (a, b) = extrusion_guide(s.doc().unwrap(), 1001, "5", false).unwrap();
        let (_, reverse) = extrusion_guide(s.doc().unwrap(), 1001, "5", true).unwrap();
        assert!((b.z - a.z - 5.0).abs() < 1e-6);
        assert!((reverse.z - a.z + 5.0).abs() < 1e-6);
        s.execute("part.sketch.begin", &json!({"face":reference})).unwrap();
        s.execute("rectang", &json!({"p1":[2,2],"p2":[7,7]})).unwrap();
        s.execute("part.sketch.commit", &json!({"extrude":true,"depth":3,"operation":"remove","target":2000,"reverse":true})).unwrap();
        assert!((bodies(s.doc().unwrap()).unwrap()[0].volume - 12425.0).abs() < 1e-5);
        let bytes = cadcraft_io::write(s.doc().unwrap(), "face.cadcraft").unwrap();
        let restored = cadcraft_io::read(&bytes, "face.cadcraft").unwrap();
        validate(&restored).unwrap();
        assert_eq!(restored, *s.doc().unwrap());
        let mut cyclic = restored.clone();
        cyclic.part.as_mut().unwrap().sketches[1].face.as_mut().unwrap().feature = 2001;
        assert!(validate(&cyclic).is_err());
        let before = s.doc().unwrap().clone();
        let mut broken = reference;
        broken.face = 16383;
        assert!(s.execute("part.sketch.begin", &json!({"face":broken})).is_err());
        assert_eq!(*s.doc().unwrap(), before);
        s.undo().unwrap();
        assert!((bodies(s.doc().unwrap()).unwrap()[0].volume - 12500.0).abs() < 1e-5);
        s.redo().unwrap();
        assert_eq!(*s.doc().unwrap(), before);
    }
    #[test]
    fn reverse_curved_extrusion_has_outward_faces_and_planar_caps() {
        let mut s = Session::new();
        s.execute("part.new", &json!({})).unwrap();
        s.execute("part.sketch.begin", &json!({})).unwrap();
        s.execute("circle", &json!({"center":[0,0],"radius":10})).unwrap();
        s.execute("part.sketch.commit", &json!({"extrude":true,"depth":5,"reverse":true})).unwrap();
        let body = bodies(s.doc().unwrap()).unwrap().remove(0);
        let mut volume = 0.0;
        for (i, face) in body.faces.iter().enumerate() {
            assert!(body.face_normal(i).is_some(), "face {i}: {:?}", body.face_points(i));
            for tri in [[face[0], face[1], face[2]], [face[0], face[2], face[3]]] {
                let [a, b, c] = tri.map(|j| {
                    let p = body.vertices[j];
                    Vec3::new(p[0], p[1], p[2])
                });
                volume += a.dot(b.cross(c)) / 6.0;
            }
        }
        assert!((volume - body.volume).abs() < 1e-4);
        assert!(body.vertices.iter().all(|p| p[2] <= 0.0));
        assert!(face_reference(s.doc().unwrap(), 2000, 0).is_ok());
        assert!(face_reference(s.doc().unwrap(), 2000, body.faces.len() - 1).is_err());
    }
    #[test]
    fn angled_planar_polygon_wall_supports_another_sketch() {
        let mut s = Session::new();
        s.execute("part.new", &json!({})).unwrap();
        s.execute("part.sketch.begin", &json!({})).unwrap();
        s.execute("line", &json!({"points":[[0,0],[20,0],[0,20],[0,0]]})).unwrap();
        s.execute("part.sketch.commit", &json!({"extrude":true,"depth":10})).unwrap();
        let body = bodies(s.doc().unwrap()).unwrap().remove(0);
        let side = body
            .faces
            .iter()
            .enumerate()
            .find(|(i, _)| body.face_normal(*i).is_some_and(|n| n.x.abs() > 0.1 && n.y.abs() > 0.1 && n.z.abs() < 0.1))
            .unwrap()
            .0;
        let reference = face_reference(s.doc().unwrap(), 2000, side).unwrap();
        s.execute("part.sketch.begin", &json!({"face":reference})).unwrap();
        s.execute("rectang", &json!({"p1":[1,1],"p2":[3,4]})).unwrap();
        s.execute("part.sketch.commit", &json!({"extrude":true,"depth":3})).unwrap();
        assert_eq!(bodies(s.doc().unwrap()).unwrap().len(), 2);
        assert!((bodies(s.doc().unwrap()).unwrap()[1].volume - 18.0).abs() < 1e-6);
    }
    #[test]
    fn automatic_operation_uses_the_target_before_the_edited_feature() {
        let mut s = Session::new();
        s.execute("part.new", &json!({})).unwrap();
        s.execute("extrude", &json!({"width":40,"height":20,"depth":10})).unwrap();
        let reference = face_reference(s.doc().unwrap(), 2000, 1).unwrap();
        s.execute("part.sketch.begin", &json!({"face":reference})).unwrap();
        s.execute("rectang", &json!({"p1":[2,2],"p2":[12,12]})).unwrap();
        s.execute("part.sketch.commit", &json!({"extrude":true,"depth":5,"operation":"add","target":2000})).unwrap();
        s.execute("extrude", &json!({"width":5,"height":5,"x":100,"depth":5})).unwrap();
        let saved = s.doc().unwrap().clone();
        let cut = s.execute("extrude", &json!({"feature":2001,"reverse":true,"autoOperation":true})).unwrap();
        assert_eq!(cut["operation"], "remove");
        assert!((bodies(s.doc().unwrap()).unwrap()[0].volume - 7500.0).abs() < 1e-5);
        let add = s.execute("extrude", &json!({"feature":2001,"reverse":false,"autoOperation":true})).unwrap();
        assert_eq!(add["operation"], "add");
        assert!((bodies(s.doc().unwrap()).unwrap()[0].volume - 8500.0).abs() < 1e-5);
        assert_eq!(bodies(s.doc().unwrap()).unwrap()[1].volume, 125.0);
        s.undo().unwrap();
        assert_eq!(s.doc().unwrap().part.as_ref().unwrap().features[1].operation, cadcraft_doc::SolidOperation::Remove);
        s.redo().unwrap();
        let before = s.doc().unwrap().clone();
        assert!(s.execute("extrude", &json!({"feature":2001,"reverse":true,"depth":20,"autoOperation":true})).is_ok());
        s.undo().unwrap();
        assert_eq!(*s.doc().unwrap(), before);
        let bytes = cadcraft_io::write(s.doc().unwrap(), "automatic.cadcraft").unwrap();
        assert_eq!(cadcraft_io::read(&bytes, "automatic.cadcraft").unwrap().part, s.doc().unwrap().part);
        assert_eq!(s.doc().unwrap().part, saved.part);
        let new_body = s.execute("extrude", &json!({"feature":2002,"reverse":true,"autoOperation":true})).unwrap();
        assert_eq!(new_body["operation"], "newBody");
        assert_eq!(bodies(s.doc().unwrap()).unwrap().len(), 2);
    }
    #[test]
    fn automatic_operation_does_not_create_disconnected_bodies() {
        let mut s = Session::new();
        s.execute("part.new", &json!({})).unwrap();
        s.execute("extrude", &json!({"width":40,"height":20,"depth":10})).unwrap();
        let before = s.doc().unwrap().clone();
        assert!(
            s.execute("extrude", &json!({"width":5,"height":5,"x":100,"depth":5,"operation":"remove","target":2000,"autoOperation":true})).is_err()
        );
        assert_eq!(*s.doc().unwrap(), before);
    }
}
