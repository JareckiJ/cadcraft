//! Dimension associativity CADCraft doesn't model (FF1): a DIMASSOC of a dimension in a block, or
//! linked by an object snap CADCraft doesn't infer, is kept and written back with its references
//! moved, through DXF and DWG; erasing the geometry it links drops it (the dimension is then no
//! longer associative, as in AutoCAD). Original synthetic DXF after the DXF Reference (DIMENSION,
//! DIMASSOC, DICTIONARY). The DWG oracle shares acadrust 0.6.5; no AutoCAD validation is claimed.

#[path = "support/dimassoc.rs"]
mod fixture;

use cadcraft_doc::{EntityKind, Handle};
use cadcraft_dxf::Tag;
use fixture::source;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

/// For each DIMASSOC written: (its dimension, the geometry it links), checked to be reachable the
/// way AutoCAD reaches it (the dimension's extension dictionary, entry ACAD_DIMASSOC) and named in
/// the dimension's reactors.
fn written(text: &str) -> TestResult<Vec<(String, String)>> {
    let tags = cadcraft_dxf::parse(text.as_bytes())?;
    let recs = cadcraft_dxf::records(&tags);
    let get = |r: &[Tag], c: i32| r.iter().find(|t| t.code == c).map(|t| t.str());
    let group = |r: &[Tag], name: &str| -> Vec<String> {
        r.iter().skip_while(|t| t.str() != format!("{{{name}")).skip(1).take_while(|t| t.code != 102).map(Tag::str).collect()
    };
    let by_handle = |h: &str| recs.iter().find(|(_, r)| get(r, 5).as_deref() == Some(h));
    let mut out = Vec::new();
    for (_, r) in recs.iter().filter(|(k, _)| k == "DIMASSOC") {
        let me = get(r, 5).ok_or("handle")?;
        let body: Vec<&Tag> = r.iter().skip_while(|t| t.str() != "AcDbDimAssoc").collect();
        let dim = body.iter().find(|t| t.code == 330).map(|t| t.str()).ok_or("dimension")?;
        let geometry = body.iter().find(|t| t.code == 331).map(|t| t.str()).ok_or("geometry")?;
        let (kind, d) = by_handle(&dim).ok_or("dimension written")?;
        assert_eq!(kind, "DIMENSION");
        assert!(group(d, "ACAD_REACTORS").contains(&me), "{dim} names its DIMASSOC {me} as a reactor");
        let xd = group(d, "ACAD_XDICTIONARY").into_iter().next().ok_or("extension dictionary")?;
        let (_, x) = by_handle(&xd).ok_or("extension dictionary written")?;
        assert!(x.windows(2).any(|w| w[0].code == 3 && w[0].str() == "ACAD_DIMASSOC" && w[1].str() == me));
        assert!(by_handle(&geometry).is_some_and(|(k, _)| k == "LINE"), "{geometry} is written");
        out.push((dim, geometry));
    }
    out.sort();
    Ok(out)
}

fn both() -> Vec<(String, String)> {
    vec![("31".into(), "30".into()), ("51".into(), "50".into())]
}

#[test]
fn associations_cadcraft_doesnt_model_are_written_back_with_their_references() -> TestResult {
    let d = cadcraft_io::read_dxf(source().as_bytes())?;
    // Neither is in CADCraft's model: one is in a block, the other's snap isn't inferred.
    let dims = d.model.iter().chain(d.blocks.values().flat_map(|b| b.entities.iter()));
    assert!(dims.filter_map(|e| if let EntityKind::Dimension(dm) = &e.kind { Some(dm) } else { None }).all(|dm| dm.assoc.is_empty()));
    let text = cadcraft_io::write_dxf(&d);
    assert_eq!(written(&text)?, both());
    // Read back and saved again, they stay.
    let back = cadcraft_io::read_dxf(text.as_bytes())?;
    assert_eq!(written(&cadcraft_io::write_dxf(&back))?, both());
    Ok(())
}

#[test]
fn erasing_the_linked_geometry_drops_the_association() -> TestResult {
    let mut d = cadcraft_io::read_dxf(source().as_bytes())?;
    assert!(d.model.remove(Handle(0x50)).is_some());
    let text = cadcraft_io::write_dxf(&d);
    assert_eq!(written(&text)?, vec![("31".to_string(), "30".to_string())]);
    // No reference to the dropped object is left behind (the dimension's reactors among them).
    let tags = cadcraft_dxf::parse(text.as_bytes())?;
    let handles: std::collections::HashSet<String> = tags.iter().filter(|t| t.code == 5).map(Tag::str).collect();
    for t in tags.iter().filter(|t| matches!(t.code, 330 | 331 | 350 | 360)) {
        assert!(t.str() == "0" || handles.contains(&t.str()), "dangling {} {}", t.code, t.str());
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn associations_go_into_the_dwg_and_back() -> TestResult {
    use acadrust::objects::ObjectType;
    let d = cadcraft_io::read_dxf(source().as_bytes())?;
    let mut bytes = cadcraft_io::write_with(&d, "out.dwg", &cadcraft_io::SaveOptions::default())?;
    for _ in 0..2 {
        let out = acadrust::DwgReader::from_stream(std::io::Cursor::new(&bytes)).read_with_stats()?;
        assert_eq!((out.stats.skipped_source_records, out.stats.recovered_errors), (0, 0));
        let mut found: Vec<(u64, bool)> = out
            .document
            .objects
            .values()
            .filter_map(|o| match o {
                ObjectType::Associative(a) if a.dxf_name.eq_ignore_ascii_case("DIMASSOC") => {
                    let dim = [0x31u64, 0x51].into_iter().find(|h| a.references_handle(acadrust::Handle::new(*h)))?;
                    let geometry = acadrust::Handle::new(if dim == 0x31 { 0x30 } else { 0x50 });
                    Some((dim, a.references_handle(geometry)))
                }
                _ => None,
            })
            .collect();
        found.sort_unstable();
        assert_eq!(found, [(0x31, true), (0x51, true)]);
        // The reopened DWG saves them again.
        let back = cadcraft_io::read(&bytes, "out.dwg")?;
        bytes = cadcraft_io::write_with(&back, "out.dwg", &cadcraft_io::SaveOptions::default())?;
    }
    Ok(())
}
