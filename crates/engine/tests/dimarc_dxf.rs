//! DIMARC saves as an ARC_DIMENSION (issue #1166): the arc-length dimension of the issue's
//! reproduction, `ARC C 300,150 320,150 300,170` and `dimarc` at 330,175, written with the
//! DXF Reference's groups, read back whole, and counted as a dimension by `info`.

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{EntityCodec, set_entity_codec};
use cadcraft_engine::doc::{DimKind, EntityKind};
use serde_json::json;

fn session() -> Session {
    set_entity_codec(EntityCodec { groups: cadcraft_io::entity_groups, entity: cadcraft_io::entity_from_groups });
    let mut s = Session::new();
    s.script("ARC C 300,150 320,150 300,170\n").unwrap();
    let h = s.doc().unwrap().model.iter().find(|e| matches!(e.kind, EntityKind::Arc(_))).map(|e| e.handle).unwrap();
    s.execute("dimarc", &json!({ "handle": h.hex(), "at": [330, 175] })).unwrap();
    s
}

fn saved(s: &Session) -> String {
    let opts = cadcraft_io::SaveOptions { version: Some(cadcraft_io::Version::R2018), binary: false };
    String::from_utf8(cadcraft_io::write_with(s.doc().unwrap(), "a.dxf", &opts).unwrap()).unwrap()
}

#[test]
fn dimarc_is_saved_as_arc_dimension() {
    let out = saved(&session());
    let lines: Vec<&str> = out.lines().map(str::trim).collect();
    assert!(lines.windows(2).any(|w| w == ["0", "ARC_DIMENSION"]), "no ARC_DIMENSION:\n{out}");
    assert!(lines.windows(2).any(|w| w == ["100", "AcDbArcDimension"]), "no AcDbArcDimension");
    assert!(!lines.windows(2).any(|w| w == ["0", "DIMENSION"]), "still a DIMENSION");
    assert!(!lines.windows(2).any(|w| w == ["1000", "ARCLEN"]), "centre still in xdata");
}

#[test]
fn dimarc_reads_back_as_the_same_dimension() {
    let s = session();
    let back = cadcraft_io::read_dxf(saved(&s).as_bytes()).unwrap();
    let dims: Vec<_> = back.model.iter().filter_map(|e| if let EntityKind::Dimension(d) = &e.kind { Some(d) } else { None }).collect();
    assert_eq!(dims.len(), 1);
    let d = dims[0];
    assert_eq!(d.kind, DimKind::ArcLength);
    assert!((d.p15.x - 300.0).abs() < 1e-9 && (d.p15.y - 150.0).abs() < 1e-9, "{:?}", d.p15);
    assert!((d.measurement - 31.4159).abs() < 1e-3, "{}", d.measurement);
    assert!(!back.model.iter().any(|e| matches!(e.kind, EntityKind::Unknown(_))));
}

fn lisp(s: &mut Session, code: &str) -> String {
    match s.lisp_eval(code, false, true, Vec::new()) {
        Ok(Some(Ok(v))) => format!("{v:?}"),
        other => format!("ERR {other:?}"),
    }
}

fn the_dimension(s: &Session) -> (cadcraft_engine::doc::Handle, cadcraft_engine::doc::Dimension) {
    let found = s.doc().unwrap().model.iter().find_map(|e| if let EntityKind::Dimension(d) = &e.kind { Some((e.handle, d.clone())) } else { None });
    found.unwrap()
}

#[test]
fn an_arc_length_dimension_names_itself_arc_dimension_everywhere() {
    let mut s = session();
    // entget and the entity's own type name agree, so ssget filters, LIST and QSELECT find it.
    assert!(lisp(&mut s, "(cdr (assoc 0 (entget (entlast))))").contains("ARC_DIMENSION"));
    assert_eq!(lisp(&mut s, "(sslength (ssget \"X\" '((0 . \"ARC_DIMENSION\"))))"), "Int(1)");
    let (h, _) = the_dimension(&s);
    assert_eq!(s.doc().unwrap().entity(h).unwrap().kind.dxf_name(), "ARC_DIMENSION");
    // A linear dimension stays a DIMENSION.
    s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [10, 0], "at": [5, 5] })).unwrap();
    assert_eq!(lisp(&mut s, "(sslength (ssget \"X\" '((0 . \"DIMENSION\"))))"), "Int(1)");
}

#[test]
fn entmod_of_an_arc_length_dimension_changes_it_and_keeps_it_an_arc_length_dimension() {
    let mut s = session();
    let (h, before) = the_dimension(&s);
    // The unchanged list.
    let r = lisp(&mut s, "(entmod (entget (entlast)))");
    assert!(r != "Nil" && !r.starts_with("ERR"), "entmod of the unchanged list: {r}");
    // A changed text.
    let r = lisp(&mut s, "(entmod (append (entget (entlast)) (list (cons 1 \"X <>\"))))");
    assert!(r != "Nil" && !r.starts_with("ERR"), "entmod of the changed list: {r}");
    let (_, after) = the_dimension(&s);
    assert_eq!(after.kind, DimKind::ArcLength);
    assert_eq!(after.text, "X <>");
    assert_eq!((after.p13, after.p14, after.p15), (before.p13, before.p14, before.p15));
    assert_eq!(s.doc().unwrap().entity(h).unwrap().kind.dxf_name(), "ARC_DIMENSION");
}

#[test]
fn mirror_keeps_the_arc_length_of_an_arc_length_dimension() {
    let mut s = session();
    let (h, before) = the_dimension(&s);
    let style = s.doc().unwrap().dim_style("Standard").cloned().unwrap_or_default();
    let measure = |d: &cadcraft_engine::doc::Dimension| cadcraft_render::dimension_geometry(d, &style, 1.0).measurement;
    assert!((measure(&before) - 31.4159).abs() < 1e-3);
    s.execute("mirror", &json!({ "handles": [h.hex()], "p1": [0, 0], "p2": [0, 10], "erase": true })).unwrap();
    let (_, after) = the_dimension(&s);
    assert_eq!(after.kind, DimKind::ArcLength);
    assert!((measure(&after) - 31.4159).abs() < 1e-3, "after MIRROR the dimension measures {}", measure(&after));
}

/// The ends of an arc-length dimension can be attached to a line that touches the arc (an end
/// picked on the line). MIRROR swaps the dimension's ends (counter-clockwise from 13 to 14), and a
/// line's ends don't swap as an arc's do: once the attachments are followed after the command, the
/// dimension must still measure the same arc, not the rest of the circle.
#[test]
fn mirror_keeps_the_arc_length_when_an_end_is_attached_to_a_line() {
    use cadcraft_engine::doc::{AssocSnap, DimAssoc};
    let mut s = session();
    s.execute("line", &json!({ "points": [[320, 150], [340, 150]] })).unwrap();
    let d = s.doc().unwrap();
    let line = d.model.iter().find(|e| matches!(e.kind, EntityKind::Line(_))).map(|e| e.handle).unwrap();
    let arc = d.model.iter().find(|e| matches!(e.kind, EntityKind::Arc(_))).map(|e| e.handle).unwrap();
    let (h, before) = the_dimension(&s);
    assert!((before.p13.x - 320.0).abs() < 1e-9 && (before.p13.y - 150.0).abs() < 1e-9, "p13 {:?}", before.p13);
    // p13 on the line's start (the arc's start too), p14 on the arc's end.
    s.doc_mut()
        .unwrap()
        .modify_entity(h, |e| {
            if let EntityKind::Dimension(dm) = &mut e.kind {
                dm.assoc = vec![
                    DimAssoc { point: "p13".into(), handle: line, snap: AssocSnap::Start },
                    DimAssoc { point: "p14".into(), handle: arc, snap: AssocSnap::End },
                ];
            }
        })
        .unwrap();
    let style = s.doc().unwrap().dim_style("Standard").cloned().unwrap_or_default();
    let measure = |d: &cadcraft_engine::doc::Dimension| cadcraft_render::dimension_geometry(d, &style, 1.0).measurement;
    assert!((measure(&before) - 31.4159).abs() < 1e-3);
    let all: Vec<String> = s.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect();
    s.execute("mirror", &json!({ "handles": all, "p1": [0, 0], "p2": [0, 10], "erase": true })).unwrap();
    let (_, after) = the_dimension(&s);
    assert!(
        (measure(&after) - 31.4159).abs() < 1e-3,
        "after MIRROR the dimension measures {} (p13 {:?}, p14 {:?})",
        measure(&after),
        after.p13,
        after.p14
    );
    // Both ends stay attached, each to where it now is: p14 on the line's start, p13 on the
    // mirrored arc's start (the arc runs the other way now).
    let mut assoc: Vec<(String, bool, AssocSnap)> = after.assoc.iter().map(|a| (a.point.clone(), a.handle == line, a.snap.clone())).collect();
    assoc.sort_by(|x, y| x.0.cmp(&y.0));
    assert_eq!(assoc, [("p13".to_string(), false, AssocSnap::Start), ("p14".to_string(), true, AssocSnap::Start)], "{:?}", after.assoc);
    // And the line still carries p14: moving the line moves that end.
    s.execute("move", &json!({ "handles": [line.hex()], "from": [0, 0], "to": [0, 5] })).unwrap();
    let (_, moved) = the_dimension(&s);
    assert!((moved.p14.x + 320.0).abs() < 1e-9 && (moved.p14.y - 155.0).abs() < 1e-9, "p14 {:?} did not follow the line", moved.p14);
}
