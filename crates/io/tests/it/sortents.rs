//! Draw order from DXF files (#439): a SORTENTSTABLE reorders its space by sort handle, and the
//! order CADCraft writes (the order of the entity records, and a SORTENTSTABLE when it isn't
//! handle order) reads back unchanged. DWG: the table goes through the DWG bridge both ways
//! (the DWG oracle shares acadrust 0.6.5; no AutoCAD validation is claimed).

use cadcraft_doc::*;

/// A LINE record in model space with handle `h` at height `y`.
fn line(h: &str, y: f64) -> String {
    format!("0\nLINE\n5\n{h}\n330\n1F\n8\n0\n10\n0\n20\n{y}\n30\n0\n11\n10\n21\n{y}\n31\n0\n")
}

/// A DXF with model space, three lines (A1, A2, A3) and `objects` in OBJECTS.
fn dxf(objects: &str) -> String {
    let mut s = String::from("0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nBLOCK_RECORD\n0\nBLOCK_RECORD\n5\n1F\n2\n*Model_Space\n0\nENDTAB\n0\nENDSEC\n");
    s.push_str("0\nSECTION\n2\nENTITIES\n");
    s.push_str(&line("A1", 0.0));
    s.push_str(&line("A2", 1.0));
    s.push_str(&line("A3", 2.0));
    s.push_str("0\nENDSEC\n0\nSECTION\n2\nOBJECTS\n");
    s.push_str(objects);
    s.push_str("0\nENDSEC\n0\nEOF\n");
    s
}

/// Model space's entity handles in draw order.
fn order(d: &Drawing) -> Vec<String> {
    d.model.iter().map(|e| e.handle.hex()).collect()
}

/// A table reorders model space; written back with a table, the order survives.
#[test]
fn sortentstable_sets_the_draw_order() {
    // A1 is drawn by sort handle A4 (over A2 and A3); A3 by 10 (under everything).
    let table = "0\nSORTENTSTABLE\n5\nB0\n330\nB1\n100\nAcDbSortentsTable\n330\n1F\n331\nA1\n5\nA4\n331\nA3\n5\n10\n";
    let d = cadcraft_io::read_dxf(dxf(table).as_bytes()).unwrap();
    assert_eq!(order(&d), ["A3", "A2", "A1"]);
    // Written back in draw order, with a table that swaps A1 and A3 (A2 is in its handle's
    // place), the order survives.
    let text = cadcraft_io::write_dxf(&d);
    let tags = cadcraft_dxf::parse(text.as_bytes()).expect("parse DXF");
    let (table, xdict) = written_table(&tags).expect("a SORTENTSTABLE");
    assert_eq!(table, [("A3".to_string(), "A1".to_string()), ("A1".into(), "A3".into())]);
    assert!(xdict, "model space's extension dictionary has ACAD_SORTENTS");
    let back = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
    assert_eq!(order(&back), ["A3", "A2", "A1"]);
}

/// The written SORTENTSTABLE's (entity, sort handle) pairs, and whether the model space record's
/// extension dictionary names it ACAD_SORTENTS.
fn written_table(tags: &[cadcraft_dxf::Tag]) -> Option<(Vec<(String, String)>, bool)> {
    let recs: Vec<&[cadcraft_dxf::Tag]> = {
        let starts: Vec<usize> = tags.iter().enumerate().filter(|(_, t)| t.code == 0).map(|(i, _)| i).collect();
        starts.iter().enumerate().map(|(k, &s)| &tags[s..starts.get(k + 1).copied().unwrap_or(tags.len())]).collect()
    };
    let kind = |r: &[cadcraft_dxf::Tag]| r.first().map(|t| t.str()).unwrap_or_default();
    let get = |r: &[cadcraft_dxf::Tag], c: i32| r.iter().find(|t| t.code == c).map(|t| t.str());
    let table = recs.iter().find(|r| kind(r) == "SORTENTSTABLE")?;
    let handle = get(table, 5)?;
    let mut pairs = Vec::new();
    let mut ent = None;
    for t in table.iter().skip_while(|t| t.code != 100).skip(1) {
        match t.code {
            331 => ent = Some(t.str()),
            5 => pairs.extend(ent.take().map(|e| (e, t.str()))),
            _ => {}
        }
    }
    let ms = recs.iter().find(|r| kind(r) == "BLOCK_RECORD" && get(r, 2).as_deref() == Some("*Model_Space"))?;
    let xd = ms.iter().skip_while(|t| t.str() != "{ACAD_XDICTIONARY").nth(1).map(|t| t.str())?;
    let dict = recs.iter().find(|r| kind(r) == "DICTIONARY" && get(r, 5).as_deref() == Some(xd.as_str()))?;
    let named = dict.windows(2).any(|w| w[0].code == 3 && w[0].str() == "ACAD_SORTENTS" && w[1].str() == handle);
    Some((pairs, named))
}

/// A block record whose extension dictionary is the last record of OBJECTS gets the entry inside it.
#[test]
fn the_entry_goes_into_a_kept_extension_dictionary_written_last() {
    // Model space's extension dictionary also holds another program's record, and comes last in
    // the file's objects (so last of the kept objects CADCraft writes back).
    let mut s = String::from("0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nBLOCK_RECORD\n");
    s.push_str("0\nBLOCK_RECORD\n5\n1F\n102\n{ACAD_XDICTIONARY\n360\nD1\n102\n}\n2\n*Model_Space\n0\nENDTAB\n0\nENDSEC\n");
    s.push_str("0\nSECTION\n2\nENTITIES\n");
    for (h, y) in [("A1", 0.0), ("A2", 1.0), ("A3", 2.0)] {
        s.push_str(&line(h, y));
    }
    s.push_str("0\nENDSEC\n0\nSECTION\n2\nOBJECTS\n0\nDICTIONARY\n5\nC\n330\n0\n100\nAcDbDictionary\n");
    s.push_str("0\nXRECORD\n5\nE1\n330\nD1\n100\nAcDbXrecord\n1\nkeep me\n");
    s.push_str("0\nSORTENTSTABLE\n5\nB0\n330\nD1\n100\nAcDbSortentsTable\n330\n1F\n331\nA1\n5\nA4\n331\nA3\n5\n10\n");
    s.push_str("0\nDICTIONARY\n5\nD1\n330\n1F\n100\nAcDbDictionary\n3\nOTHER_APP\n360\nE1\n3\nACAD_SORTENTS\n360\nB0\n0\nENDSEC\n0\nEOF\n");
    let d = cadcraft_io::read_dxf(s.as_bytes()).unwrap();
    assert_eq!(order(&d), ["A3", "A2", "A1"]);
    let text = cadcraft_io::write_dxf(&d);
    let tags = cadcraft_dxf::parse(text.as_bytes()).expect("parse DXF");
    let (table, xdict) = written_table(&tags).expect("a SORTENTSTABLE");
    assert_eq!(table.len(), 2);
    assert!(xdict, "the kept dictionary names the table");
    assert!(text.contains("OTHER_APP"), "and still holds the other program's entry");
    assert_eq!(order(&cadcraft_io::read_dxf(text.as_bytes()).unwrap()), ["A3", "A2", "A1"]);
}

/// Entities in handle order are written without a table.
#[test]
fn a_space_in_handle_order_gets_no_table() {
    let d = cadcraft_io::read_dxf(dxf("").as_bytes()).unwrap();
    let text = cadcraft_io::write_dxf(&d);
    assert!(!text.contains("SORTENTSTABLE"), "no table, and no class for one");
}

/// Tables that don't parse or name no space change nothing.
#[test]
fn broken_sortentstables_are_ignored() {
    for table in [
        "0\nSORTENTSTABLE\n5\nB0\n331\nA1\n5\nA4\n",
        "0\nSORTENTSTABLE\n100\nAcDbSortentsTable\n330\nFFFF\n331\nA1\n5\nA4\n",
        "0\nSORTENTSTABLE\n100\nAcDbSortentsTable\n330\n1F\n331\nzz\n5\nA4\n331\nA2\n5\n\n5\nA9\n",
        "0\nSORTENTSTABLE\n100\nAcDbSortentsTable\n",
    ] {
        let d = cadcraft_io::read_dxf(dxf(table).as_bytes()).unwrap();
        assert_eq!(order(&d), ["A1", "A2", "A3"], "{table:?}");
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod dwg {
    use acadrust::entities::Line;
    use acadrust::objects::{ObjectType, SortEntitiesTable};
    use acadrust::{CadDocument, DwgWriter, DxfVersion, EntityType, Handle};

    type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

    /// An AC1032 DWG with three lines in model space and a table that swaps the first and the
    /// last; the middle one isn't listed (it is drawn by its own handle). Drawn: 3, 2, 1.
    fn source() -> TestResult<(Vec<u8>, [Handle; 3])> {
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
        Ok((DwgWriter::write_to_vec(&doc)?, h))
    }

    /// Model space's entity handles in draw order.
    fn order(d: &cadcraft_doc::Drawing) -> Vec<u64> {
        d.model.iter().map(|e| e.handle.0).collect()
    }

    /// The draw order a DWG's model space has for a reader that honours the table.
    fn native_order(bytes: &[u8]) -> TestResult<Vec<u64>> {
        let out = acadrust::DwgReader::from_stream(std::io::Cursor::new(bytes)).read_with_stats()?;
        assert_eq!((out.stats.skipped_source_records, out.stats.recovered_errors), (0, 0));
        let doc = out.document;
        let ms = doc.block_records.get("*Model_Space").ok_or("model space")?;
        let mut tables = doc.objects.values().filter_map(|o| match o {
            ObjectType::SortEntitiesTable(t) if t.block_owner_handle == ms.handle => Some(t),
            _ => None,
        });
        let table = tables.next().ok_or("no table")?;
        assert!(tables.next().is_none(), "one table");
        // Found the way AutoCAD finds it: the record's extension dictionary, entry ACAD_SORTENTS.
        let xd = doc.extension_dictionary_handle(ms.handle).ok_or("no extension dictionary")?;
        let Some(ObjectType::Dictionary(d)) = doc.objects.get(&xd) else { return Err("extension dictionary".into()) };
        assert!(d.entries.iter().any(|(k, h)| k == "ACAD_SORTENTS" && *h == table.handle));
        let mut v: Vec<u64> = ms.entity_handles.iter().map(|h| h.value()).collect();
        v.sort_by_key(|h| table.effective_sort_handle(Handle::new(*h)).value());
        Ok(v)
    }

    /// A table listing only some entities reads in its own order.
    #[test]
    fn a_dwg_table_that_leaves_entities_out_reads_in_its_order() -> TestResult {
        let (bytes, h) = source()?;
        let want = vec![h[2].value(), h[1].value(), h[0].value()];
        assert_eq!(native_order(&bytes)?, want);
        // The DWG bridge's DXF used to renumber the listed entities above the unlisted one: 3, 1, 2.
        let d = cadcraft_io::read(&bytes, "order.dwg")?;
        assert_eq!(order(&d), want);
        Ok(())
    }

    /// The order goes back into the DWG, survives a second save, and a bring-to-front reaches the file.
    #[test]
    fn draw_order_goes_back_into_the_dwg_and_survives_a_second_save() -> TestResult {
        let (bytes, h) = source()?;
        let want = vec![h[2].value(), h[1].value(), h[0].value()];
        let mut d = cadcraft_io::read(&bytes, "order.dwg")?;
        for _ in 0..2 {
            let out = cadcraft_io::write_with(&d, "out.dwg", &cadcraft_io::SaveOptions::default())?;
            assert_eq!(native_order(&out)?, want);
            d = cadcraft_io::read(&out, "out.dwg")?;
            assert_eq!(order(&d), want);
        }
        // A DRAWORDER-style move (the first drawn to the top) is what the next file's table says.
        assert!(d.model.bring_to_front(cadcraft_doc::Handle(h[2].value())));
        let out = cadcraft_io::write_with(&d, "out.dwg", &cadcraft_io::SaveOptions::default())?;
        assert_eq!(native_order(&out)?, vec![h[1].value(), h[0].value(), h[2].value()]);
        Ok(())
    }
}
