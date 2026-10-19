//! Integration tests for dynamic block persistence across BEDIT (BSAVE/BSAVEAS), WBLOCK, and INSERT.

use std::sync::Arc;

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::foreign::{Foreign, ForeignExtra, ForeignObject};
use cadcraft_engine::doc::{EntityKind, Handle, RawTag};
use serde_json::json;

fn obj(handle: u64, kind: &str, tags: &[(i32, &str)]) -> ForeignObject {
    let mut all_tags = vec![RawTag { code: 5, value: format!("{handle:X}") }];
    all_tags.extend(tags.iter().map(|(c, v)| RawTag { code: *c, value: (*v).into() }));
    ForeignObject { handle: Handle(handle), kind: kind.into(), tags: all_tags }
}

fn session() -> Session {
    let mut s = Session::new();
    let l = s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap()["handles"][0].clone();
    s.execute("block", &json!({ "name": "Door", "base": [0, 0], "handles": [l] })).unwrap();
    let line = s.doc().unwrap().block("Door").unwrap().entities.handles()[0].hex();

    let mut f = Foreign::default();
    f.records.insert(("BLOCK_RECORD".into(), "DOOR".into()), ForeignExtra { xdict: Some(Handle(0xF00)), ..Default::default() });
    f.objects.push(obj(0xF00, "DICTIONARY", &[(100, "AcDbDictionary"), (3, "ACAD_ENHANCEDBLOCK"), (360, "F01")]));
    f.objects.push(obj(0xF01, "ACAD_EVALUATION_GRAPH", &[(330, "F00"), (100, "AcDbEvalGraph"), (91, "0"), (95, "1"), (360, "F02")]));
    f.objects.push(obj(
        0xF02,
        "BLOCKVISIBILITYPARAMETER",
        &[
            (330, "F01"),
            (100, "AcDbEvalExpr"),
            (90, "1"),
            (100, "AcDbBlockElement"),
            (300, "Visibility State"),
            (100, "AcDbBlockParameter"),
            (280, "1"),
            (100, "AcDbBlock1PtParameter"),
            (1010, "2"),
            (1020, "3"),
            (100, "AcDbBlockVisibilityParameter"),
            (301, "Visibility1"),
            (302, ""),
            (93, "1"),
            (331, &line),
            (92, "2"),
            (303, "Open"),
            (94, "1"),
            (332, &line),
            (95, "0"),
            (303, "Closed"),
            (94, "0"),
            (95, "0"),
        ],
    ));

    let mut d = s.doc().unwrap().clone();
    d.foreign = Arc::new(f);
    s.open_drawing(d, "dynamic", None);
    s
}

#[test]
fn bsaveas_clones_dynamic_block_definition() {
    let mut s = session();
    s.execute("bedit", &json!({ "name": "Door" })).unwrap();
    s.execute("bsaveas", &json!({ "name": "DoorCopy" })).unwrap();
    s.execute("bclose", &json!({})).unwrap();

    // DoorCopy is now a registered dynamic block
    let m = s.execute("blocks.dynamic", &json!({ "name": "DoorCopy" })).unwrap();
    assert_eq!(m["parameters"][0]["kind"]["type"], "visibility");
    assert_eq!(m["parameters"][0]["kind"]["label"], "Visibility1");

    // Insert DoorCopy and change its visibility state
    let ins = s.execute("insert", &json!({ "name": "DoorCopy", "at": [10, 10] })).unwrap()["handle"].as_str().unwrap().to_string();
    let r = s.execute("blocks.reference", &json!({ "handle": ins })).unwrap();
    assert_eq!(r["definition"], "DoorCopy");
    assert_eq!(r["properties"][0]["value"], "Open");

    s.execute("blocks.reference.set", &json!({ "handle": ins, "name": "Visibility1", "value": "Closed" })).unwrap();
    let r = s.execute("blocks.reference", &json!({ "handle": ins })).unwrap();
    assert_eq!(r["properties"][0]["value"], "Closed");
}

#[test]
fn bsave_refreshes_dynamic_block_references() {
    let mut s = session();
    let insert = s.doc().unwrap().model.handles()[0].hex();

    // Set Visibility1 to Closed
    s.execute("blocks.reference.set", &json!({ "handle": insert, "name": "Visibility1", "value": "Closed" })).unwrap();

    // Modify Door in BEDIT by adding a second line
    s.execute("bedit", &json!({ "name": "Door" })).unwrap();
    s.execute("line", &json!({ "points": [[0, 0], [0, 5]] })).unwrap();
    s.execute("bsave", &json!({})).unwrap();
    s.execute("bclose", &json!({})).unwrap();

    // Verify the reference in model space retains Visibility1 = Closed
    let r = s.execute("blocks.reference", &json!({ "handle": insert })).unwrap();
    assert_eq!(r["definition"], "Door");
    assert_eq!(r["properties"][0]["value"], "Closed");

    // And verify the updated geometry is present in the block reference
    let ins_entity = s.doc().unwrap().entity(Handle::parse_hex(&insert).unwrap()).unwrap().clone();
    if let EntityKind::Insert(i) = &ins_entity.kind {
        let b = s.doc().unwrap().block(&i.block).unwrap().clone();
        assert_eq!(b.entities.len(), 2, "anonymous block regenerated with newly added line from master definition");
    } else {
        panic!("not an insert");
    }
}

#[test]
fn wblock_and_insert_file_preserves_dynamic_block() {
    set_io(IoHooks {
        read: |bytes, path| cadcraft_io::read(bytes, path).map_err(|e| e.to_string()),
        write: |doc, path| cadcraft_io::write(doc, path).map_err(|e| e.to_string()),
        plot: None,
    });

    let mut s = session();
    let temp_path = std::env::temp_dir().join(format!("cadcraft-dyn-wblock-{}.dxf", std::process::id()));
    let temp_str = temp_path.to_string_lossy().to_string();

    s.execute("wblock", &json!({ "name": "Door", "path": temp_str })).unwrap();

    // Read file directly and verify authoring elements on *Model_Space
    let bytes = std::fs::read(&temp_path).unwrap();
    let exported_doc = cadcraft_io::read(&bytes, &temp_str).unwrap();
    assert!(cadcraft_engine::blockedit::has_authoring_elements(&exported_doc));

    // Now in a fresh session, insert the exported file as "ExportedDoor"
    let mut s2 = Session::new();
    s2.execute("insert", &json!({ "file": temp_str, "name": "ExportedDoor", "at": [0, 0] })).unwrap();

    let m = s2.execute("blocks.dynamic", &json!({ "name": "ExportedDoor" })).unwrap();
    assert_eq!(m["parameters"][0]["kind"]["type"], "visibility");
    assert_eq!(m["parameters"][0]["kind"]["label"], "Visibility1");

    let _ = std::fs::remove_file(&temp_path);
}
