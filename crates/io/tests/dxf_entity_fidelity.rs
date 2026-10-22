//! What another program's DXF entities keep through open → save → reopen in
//! CADCraft (gap FF1):
//!
//! - the extrusion (0,0,-1) of mirrored entities;
//! - the text position and measurement of dimensions saved unchanged;
//! - attribute justification (a left-justified multiline attribute was read as centred).
//!
//! The fixture is synthetic, written from the public DXF Reference: a drawing as another program
//! might save it.

use cadcraft_doc::{Drawing, EntityKind, HAlign, Handle, VAlign};
use cadcraft_dxf::{Tag, records, sections};
use cadcraft_io::{SaveOptions, Version, read_dxf, write_with};

/// DXF text from lines of `code value` (the value may be empty).
fn dxf(src: &str) -> String {
    let mut out = String::new();
    for line in src.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let (code, value) = line.split_once(' ').unwrap_or((line, ""));
        out.push_str(&format!("{code}\n{value}\n"));
    }
    out
}

const FIXTURE: &str = "
0 SECTION
2 HEADER
9 $ACADVER
1 AC1032
9 $DWGCODEPAGE
3 ANSI_1252
9 $MEASUREMENT
70 1
9 $DIMSTYLE
2 Standard
9 $HANDSEED
5 400
0 ENDSEC
0 SECTION
2 TABLES
0 TABLE
2 LTYPE
5 5
330 0
100 AcDbSymbolTable
70 1
0 LTYPE
5 16
330 5
100 AcDbSymbolTableRecord
100 AcDbLinetypeTableRecord
2 Continuous
70 0
3 Solid line
72 65
73 0
40 0
0 ENDTAB
0 TABLE
2 LAYER
5 2
330 0
100 AcDbSymbolTable
70 2
0 LAYER
5 10
330 2
100 AcDbSymbolTableRecord
100 AcDbLayerTableRecord
2 0
70 0
62 7
6 Continuous
0 LAYER
5 1A0
330 2
100 AcDbSymbolTableRecord
100 AcDbLayerTableRecord
2 WALLS
70 0
62 1
6 Continuous
0 ENDTAB
0 TABLE
2 STYLE
5 3
330 0
100 AcDbSymbolTable
70 1
0 STYLE
5 11
330 3
100 AcDbSymbolTableRecord
100 AcDbTextStyleTableRecord
2 Standard
70 0
40 0
41 1
50 0
71 0
42 0.2
3 txt
4
0 ENDTAB
0 TABLE
2 DIMSTYLE
5 A
330 0
100 AcDbSymbolTable
70 1
100 AcDbDimStyleTable
71 0
0 DIMSTYLE
105 27
330 A
100 AcDbSymbolTableRecord
100 AcDbDimStyleTableRecord
2 Standard
70 0
140 0.18
0 ENDTAB
0 TABLE
2 BLOCK_RECORD
5 1
330 0
100 AcDbSymbolTable
70 4
0 BLOCK_RECORD
5 1F
330 1
100 AcDbSymbolTableRecord
100 AcDbBlockTableRecord
2 *Model_Space
340 22
0 BLOCK_RECORD
5 1B
330 1
100 AcDbSymbolTableRecord
100 AcDbBlockTableRecord
2 *Paper_Space
340 1E
0 BLOCK_RECORD
5 1A3
330 1
100 AcDbSymbolTableRecord
100 AcDbBlockTableRecord
2 DOOR
340 0
70 0
280 1
0 BLOCK_RECORD
5 1A4
330 1
100 AcDbSymbolTableRecord
100 AcDbBlockTableRecord
2 *D1
340 0
0 ENDTAB
0 ENDSEC
0 SECTION
2 BLOCKS
0 BLOCK
5 20
330 1F
100 AcDbEntity
8 0
100 AcDbBlockBegin
2 *Model_Space
70 0
10 0
20 0
30 0
3 *Model_Space
1
0 ENDBLK
5 21
330 1F
100 AcDbEntity
8 0
100 AcDbBlockEnd
0 BLOCK
5 1C
330 1B
100 AcDbEntity
67 1
8 0
100 AcDbBlockBegin
2 *Paper_Space
70 0
10 0
20 0
30 0
3 *Paper_Space
1
0 ENDBLK
5 1D
330 1B
100 AcDbEntity
67 1
8 0
100 AcDbBlockEnd
0 BLOCK
5 1A5
330 1A3
100 AcDbEntity
8 0
100 AcDbBlockBegin
2 DOOR
70 0
10 0
20 0
30 0
3 DOOR
1
0 LINE
5 1AA
330 1A3
100 AcDbEntity
8 0
100 AcDbLine
10 0
20 0
30 0
11 1
21 0
31 0
0 ENDBLK
5 1A6
330 1A3
100 AcDbEntity
8 0
100 AcDbBlockEnd
0 BLOCK
5 1A7
330 1A4
100 AcDbEntity
8 0
100 AcDbBlockBegin
2 *D1
70 1
10 0
20 0
30 0
3 *D1
1
0 LINE
5 1A9
330 1A4
100 AcDbEntity
8 0
100 AcDbLine
10 0
20 5
30 0
11 10
21 5
31 0
0 ENDBLK
5 1A8
330 1A4
100 AcDbEntity
8 0
100 AcDbBlockEnd
0 ENDSEC
0 SECTION
2 ENTITIES
0 CIRCLE
5 300
330 1F
100 AcDbEntity
8 0
100 AcDbCircle
10 5
20 0
30 0
40 1
210 0
220 0
230 -1
0 ARC
5 301
330 1F
100 AcDbEntity
8 0
100 AcDbCircle
10 5
20 1
30 0
40 2
210 0
220 0
230 -1
100 AcDbArc
50 10
51 80
0 ELLIPSE
5 302
330 1F
100 AcDbEntity
8 0
100 AcDbEllipse
10 0
20 10
30 0
11 3
21 0
31 0
210 0
220 0
230 -1
40 0.5
41 0.5
42 2
0 LWPOLYLINE
5 303
330 1F
100 AcDbEntity
8 0
100 AcDbPolyline
90 2
70 0
10 1
20 1
42 0.5
10 4
20 1
210 0
220 0
230 -1
0 LEADER
5 304
330 1F
100 AcDbEntity
8 0
100 AcDbLeader
3 Standard
71 1
72 0
76 2
10 0
20 0
30 0
10 2
20 2
30 0
210 0
220 0
230 -1
0 SOLID
5 305
330 1F
100 AcDbEntity
8 0
100 AcDbTrace
10 1
20 1
30 0
11 2
21 1
31 0
12 1
22 2
32 0
13 2
23 2
33 0
210 0
220 0
230 -1
0 INSERT
5 306
330 1F
100 AcDbEntity
8 0
100 AcDbBlockReference
2 DOOR
10 3
20 4
30 0
41 1
42 2
43 1
50 30
210 0
220 0
230 -1
0 INSERT
5 307
330 1F
100 AcDbEntity
8 WALLS
100 AcDbBlockReference
66 1
2 DOOR
10 20
20 20
30 0
0 ATTRIB
5 308
330 307
100 AcDbEntity
8 WALLS
100 AcDbText
10 20
20 21
30 0
40 0.25
1 first
7 Standard
100 AcDbAttribute
280 0
2 TAG1
70 0
280 0
71 2
72 1
11 20
21 21
31 0
101 Embedded Object
10 20
20 21
30 0
40 0.25
41 0
46 0
71 1
72 5
1 first
7 Standard
73 1
44 1
0 ATTRIB
5 309
330 307
100 AcDbEntity
8 WALLS
100 AcDbText
10 20
20 22.375
30 0
40 0.25
1 second
7 Standard
72 0
11 20
21 22.5
31 0
100 AcDbAttribute
2 TAG2
70 0
73 0
74 2
0 SEQEND
5 30A
330 307
100 AcDbEntity
8 WALLS
0 DIMENSION
5 30B
330 1F
100 AcDbEntity
8 0
100 AcDbDimension
2 *D1
10 10
20 5
30 0
11 5
21 7.3
31 0
70 32
1
3 Standard
42 10
100 AcDbAlignedDimension
13 0
23 0
33 0
14 10
24 0
34 0
50 0
100 AcDbRotatedDimension
0 ENDSEC
0 SECTION
2 OBJECTS
0 DICTIONARY
5 C
330 0
100 AcDbDictionary
281 1
3 ACAD_LAYOUT
350 1A
0 DICTIONARY
5 1A
330 C
100 AcDbDictionary
281 1
3 Layout1
350 1E
3 Model
350 22
0 LAYOUT
5 22
330 1A
100 AcDbPlotSettings
1
2 none_device
4 ISO_A4_(210.00_x_297.00_MM)
6
40 0
41 0
42 0
43 0
44 210
45 297
70 688
72 1
73 0
74 1
100 AcDbLayout
1 Model
70 1
71 0
330 1F
0 LAYOUT
5 1E
330 1A
100 AcDbPlotSettings
1
2 none_device
4 ISO_A4_(210.00_x_297.00_MM)
6
40 0
41 0
42 0
43 0
44 210
45 297
70 688
72 0
73 0
74 5
100 AcDbLayout
1 Layout1
70 1
71 1
330 1B
0 ENDSEC
0 EOF
";

fn fixture() -> String {
    dxf(FIXTURE)
}

/// Saved as a DXF of the drawing's own version (2018 for the fixture).
fn save(d: &Drawing) -> String {
    String::from_utf8(cadcraft_io::write(d, "x.dxf").expect("save")).expect("text")
}

/// Every record of a DXF text with its section: (section, type, groups).
fn recs(text: &str) -> Vec<(String, String, Vec<Tag>)> {
    let tags = cadcraft_dxf::parse(text.as_bytes()).expect("parse");
    sections(&tags).into_iter().flat_map(|s| records(&s.tags).into_iter().map(move |(k, t)| (s.name.clone(), k, t))).collect()
}

fn val(t: &[Tag], code: i32) -> Option<String> {
    t.iter().find(|x| x.code == code).map(|x| x.str().trim().to_string())
}

fn handle_of(t: &[Tag]) -> Option<String> {
    t.iter().find(|x| x.code == 5 || x.code == 105).map(|x| x.str().trim().to_ascii_uppercase())
}

/// The groups of the record with handle `h` (entities keep their handles on save).
fn record(text: &str, h: &str) -> Vec<Tag> {
    recs(text).into_iter().find(|(s, _, t)| s != "HEADER" && handle_of(t).as_deref() == Some(h)).map(|(_, _, t)| t).unwrap_or_default()
}

/// The entities of model space by handle, with their geometry as JSON (the reopened drawing is
/// compared with the one read).
fn geometry(d: &Drawing) -> Vec<(Handle, serde_json::Value, cadcraft_geom::Vec3)> {
    d.model.iter().map(|e| (e.handle, serde_json::to_value(&e.kind).unwrap_or_default(), e.common.extrusion)).collect()
}

/// Two geometry lists agree to 1e-9 in every number.
fn same(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    use serde_json::Value as V;
    match (a, b) {
        (V::Number(x), V::Number(y)) => (x.as_f64().unwrap_or(f64::NAN) - y.as_f64().unwrap_or(f64::NAN)).abs() < 1e-9,
        (V::Array(x), V::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(x, y)| same(x, y)),
        (V::Object(x), V::Object(y)) => x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w))),
        _ => a == b,
    }
}

/// Entities read with extrusion (0,0,-1) are written back in that OCS with their extrusion: the
/// reader turned them into the plan view, the writer turns them back (ellipses in blocks and a
/// LEADER came back flipped to +Z).
#[test]
fn mirrored_entities_keep_their_extrusion() {
    let d = read_dxf(fixture().as_bytes()).expect("read");
    // Read into the plan view: the circle's centre is the OCS point mirrored.
    let circle = d.model.get(Handle(0x300)).expect("circle");
    let EntityKind::Circle(c) = &circle.kind else { panic!("not a circle") };
    assert!((c.center.x + 5.0).abs() < 1e-9 && c.center.y.abs() < 1e-9);
    let out = save(&d);
    for h in ["300", "301", "302", "303", "304", "305", "306"] {
        let r = record(&out, h);
        assert_eq!(val(&r, 230).and_then(|z| z.parse::<f64>().ok()), Some(-1.0), "{h}");
    }
    // As read: the circle's OCS centre, the arc's angles, the ellipse's parameters, the
    // polyline's vertices and bulge, the solid's corner, the reference's point, scale and
    // rotation.
    let num = |h: &str, code: i32| val(&record(&out, h), code).and_then(|v| v.parse::<f64>().ok()).unwrap_or(f64::NAN);
    let near = |a: f64, b: f64| (a - b).abs() < 1e-9;
    assert!(near(num("300", 10), 5.0));
    assert!(near(num("301", 50), 10.0) && near(num("301", 51), 80.0));
    assert!(near(num("302", 41), 0.5) && near(num("302", 42), 2.0));
    assert!(near(num("303", 10), 1.0) && near(num("303", 42), 0.5));
    assert!(near(num("305", 10), 1.0));
    assert!(near(num("306", 10), 3.0) && near(num("306", 41), 1.0) && near(num("306", 50), 30.0));
    // Reopened, the geometry and the extrusions are as read.
    let back = read_dxf(out.as_bytes()).expect("reopen");
    let (before, after) = (geometry(&d), geometry(&back));
    for (h, g, n) in &before {
        let Some((_, g2, n2)) = after.iter().find(|(h2, _, _)| h2 == h) else { panic!("{h:?} lost") };
        assert!(same(g, g2), "{h:?}: {g} vs {g2}");
        assert_eq!(n, n2, "{h:?}");
    }
}

/// An entity drawn in CADCraft (extrusion +Z) writes no extrusion, as before.
#[test]
fn plan_view_entities_write_no_extrusion() {
    let d = read_dxf(fixture().as_bytes()).expect("read");
    let out = save(&d);
    assert_eq!(val(&record(&out, "307"), 230), None);
}

/// A dimension saved unchanged keeps its text position and measurement as read; once edited
/// (the link to the block it was read with dropped) they are CADCraft's again (dimension text
/// midpoints moved on every save).
#[test]
fn unchanged_dimensions_keep_their_text_position() {
    let mut d = read_dxf(fixture().as_bytes()).expect("read");
    let out = save(&d);
    let r = record(&out, "30B");
    let num = |r: &[Tag], code: i32| val(r, code).and_then(|v| v.parse::<f64>().ok()).unwrap_or(f64::NAN);
    assert_eq!((num(&r, 11), num(&r, 21)), (5.0, 7.3));
    assert_eq!(num(&r, 42), 10.0);
    let back = read_dxf(out.as_bytes()).expect("reopen");
    let EntityKind::Dimension(dm) = &back.model.get(Handle(0x30B)).expect("dimension").kind else { panic!("not a dimension") };
    assert!((dm.text_mid.x - 5.0).abs() < 1e-9 && (dm.text_mid.y - 7.3).abs() < 1e-9);
    // Edited: the dimension is drawn (and saved) by CADCraft.
    d.modify_entity(Handle(0x30B), |e| {
        if let EntityKind::Dimension(dm) = &mut e.kind {
            dm.block = None;
        }
    })
    .expect("edit");
    let r = record(&save(&d), "30B");
    assert_ne!((num(&r, 11), num(&r, 21)), (5.0, 7.3));
}

/// An ATTRIB's justification comes from its text groups: the multiline header of its attribute
/// subclass (71, 72, 11) isn't the justification and alignment point, and 73 there is the field
/// length, the vertical justification being 74 (left-justified attributes became centred with
/// an alignment point).
#[test]
fn attribute_justification_is_read_from_its_text_groups() {
    let d = read_dxf(fixture().as_bytes()).expect("read");
    let check = |d: &Drawing| {
        let ins = d.model.get(Handle(0x307)).expect("insert");
        let EntityKind::Insert(i) = &ins.kind else { panic!("not an insert") };
        let [first, second] = i.attribs.as_slice() else { panic!("two attributes") };
        assert_eq!((first.text.halign, first.text.valign, first.text.align_pt), (HAlign::Left, VAlign::Baseline, None));
        assert_eq!((second.text.halign, second.text.valign), (HAlign::Left, VAlign::Middle));
        assert!(second.text.align_pt.is_some_and(|a| (a.y - 22.5).abs() < 1e-9));
    };
    check(&d);
    for v in [Version::R2018, Version::R2000] {
        let out = String::from_utf8(write_with(&d, "x.dxf", &SaveOptions { version: Some(v), binary: false }).expect("save")).expect("text");
        check(&read_dxf(out.as_bytes()).expect("reopen"));
    }
}
