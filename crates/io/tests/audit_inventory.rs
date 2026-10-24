//! `cadcraft-cli audit`'s independent count (FF1): draw order is counted as spaces drawn out of
//! handle order, not as SORTENTSTABLE records, and empty dictionaries aren't counted (AutoCAD
//! leaves many on layers; a save that drops one loses nothing). Original synthetic sources built
//! with acadrust's public writer; no AutoCAD validation is claimed.
#![cfg(not(target_arch = "wasm32"))]

use acadrust::entities::Line;
use acadrust::objects::{Dictionary, ObjectType, SortEntitiesTable};
use acadrust::{CadDocument, DwgWriter, DxfVersion, EntityType, Handle};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

/// AC1032 with three lines in model space, a table that swaps the first and the last, and an
/// empty extension dictionary on layer 0.
fn source() -> TestResult<Vec<u8>> {
    let mut doc = CadDocument::with_version(DxfVersion::AC1032);
    let mut h = [Handle::NULL; 3];
    for (i, slot) in h.iter_mut().enumerate() {
        let y = i as f64;
        *slot = doc.add_entity(EntityType::Line(Line::from_coords(0., y, 0., 10., y, 0.)))?;
    }
    let ms = doc.block_records.get("*Model_Space").ok_or("model space")?.handle;
    let xd = doc.ensure_extension_dictionary(ms);
    let th = doc.allocate_handle();
    let mut t = SortEntitiesTable::for_block(ms);
    t.handle = th;
    t.owner_handle = xd;
    t.add_entry(h[0], h[2]);
    t.add_entry(h[2], h[0]);
    doc.objects.insert(th, ObjectType::SortEntitiesTable(t));
    let Some(ObjectType::Dictionary(d)) = doc.objects.get_mut(&xd) else { return Err("extension dictionary".into()) };
    d.entries.push(("ACAD_SORTENTS".into(), th));
    let layer0 = doc.layers.get("0").ok_or("layer 0")?.handle;
    let empty = doc.allocate_handle();
    let mut e = Dictionary::new();
    e.handle = empty;
    e.owner = layer0;
    doc.objects.insert(empty, ObjectType::Dictionary(e));
    Ok(DwgWriter::write_to_vec(&doc)?)
}

#[test]
fn draw_order_is_counted_by_space_and_empty_dictionaries_not_at_all() -> TestResult {
    let bytes = source()?;
    let doc = acadrust::DwgReader::from_stream(std::io::Cursor::new(&bytes)).read()?;
    let dicts: Vec<usize> =
        doc.objects.values().filter_map(|o| if let ObjectType::Dictionary(d) = o { Some(d.entries.len()) } else { None }).collect();
    assert!(dicts.contains(&0), "the source has an empty dictionary (layer 0's)");
    let counts = cadcraft_dwg::inventory(&bytes)?;
    assert_eq!(counts.get("draw order: spaces drawn out of handle order"), Some(&1));
    assert!(!counts.keys().any(|k| k.contains("SortEntitiesTable")), "{counts:?}");
    // Model space's dictionary holds only the draw order table: counted by its space, not here.
    let counted = counts.get("object Dictionary").copied().unwrap_or(0);
    assert_eq!(counted, dicts.iter().filter(|n| **n > 0).count() as u64 - 1);
    Ok(())
}
