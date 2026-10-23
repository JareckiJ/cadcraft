//! FF7: what another program's DXF keeps through open → save → reopen in CADCraft: the handles
//! of table records, blocks, the objects CADCraft rewrites (layouts, dictionaries, scales…), the
//! objects it keeps without modelling them, attributes and sequence ends.
//!
//! The fixture is synthetic, written from the public DXF Reference: a drawing as another program
//! might save it.

use std::collections::HashMap;

use cadcraft_doc::Drawing;
use cadcraft_dxf::{Tag, records, sections};
use cadcraft_io::read_dxf;

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
9 $ACADMAINTVER
70 105
9 $DWGCODEPAGE
3 ANSI_1252
9 $MEASUREMENT
70 1
9 $DGNFRAME
280 2
9 $OSMODE
70 4133
9 $UCSORG
10 1
20 2
30 3
9 $PELEVATION
40 2.5
9 $VERSIONGUID
2 {00000000-0000-0000-0000-000000000001}
9 $DRAGVS
349 1C0
9 $INTERFEREOBJVS
345 999
9 $DIMSTYLE
2 Standard
9 $DIMZIN
70 3
9 $DIMTXT
40 0.18
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
70 3
0 LTYPE
5 14
330 5
100 AcDbSymbolTableRecord
100 AcDbLinetypeTableRecord
2 ByBlock
70 0
3
72 65
73 0
40 0
0 LTYPE
5 15
330 5
100 AcDbSymbolTableRecord
100 AcDbLinetypeTableRecord
2 ByLayer
70 0
3
72 65
73 0
40 0
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
70 2
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
0 STYLE
5 1A1
330 3
100 AcDbSymbolTableRecord
100 AcDbTextStyleTableRecord
2
70 1
40 0
41 1
50 0
71 0
42 0.2
3 ltypeshp.shx
4
0 ENDTAB
0 TABLE
2 APPID
5 9
330 0
100 AcDbSymbolTable
70 2
0 APPID
5 12
330 9
100 AcDbSymbolTableRecord
100 AcDbRegAppTableRecord
2 ACAD
70 0
0 APPID
5 1A2
330 9
100 AcDbSymbolTableRecord
100 AcDbRegAppTableRecord
2 ACME
70 0
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
78 8
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
0 LINE
5 30C
330 1F
100 AcDbEntity
8 0
100 AcDbLine
10 0
20 30
30 0
11 5
21 30
31 0
1001 ACME
1000 layer-ref
1005 1A0
0 ENDSEC
0 SECTION
2 OBJECTS
0 DICTIONARY
5 C
330 0
100 AcDbDictionary
281 1
3 ACAD_GROUP
350 D
3 ACAD_LAYOUT
350 1A
3 ACAD_SCALELIST
350 1C1
3 ACAD_VISUALSTYLE
350 1BF
3 ACME_DATA
350 1C5
0 DICTIONARY
5 D
330 C
100 AcDbDictionary
281 1
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
0 DICTIONARY
5 1BF
330 C
100 AcDbDictionary
281 1
3 2dWireframe
350 1C0
0 VISUALSTYLE
5 1C0
330 1BF
100 AcDbVisualStyle
2 2dWireframe
70 4
0 DICTIONARY
5 1C1
330 C
100 AcDbDictionary
281 1
3 A0
350 1C2
3 A1
350 1C3
0 SCALE
5 1C2
330 1C1
100 AcDbScale
70 0
300 1:1
140 1
141 1
290 1
0 SCALE
5 1C3
330 1C1
100 AcDbScale
70 0
300 1:2
140 1
141 2
290 0
0 XRECORD
5 1C5
330 C
100 AcDbXrecord
280 1
1 acme-data
340 1C3
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

/// The handle of the record of type `kind` whose group `code` is `value`.
fn find(text: &str, kind: &str, code: i32, value: &str) -> Option<String> {
    recs(text)
        .into_iter()
        .find(|(_, k, t)| k == kind && val(t, code).is_some_and(|v| v.eq_ignore_ascii_case(value)))
        .and_then(|(_, _, t)| handle_of(&t))
}

/// The groups of the record with handle `h`.
fn record(text: &str, h: &str) -> Vec<Tag> {
    recs(text).into_iter().find(|(s, _, t)| s != "HEADER" && handle_of(t).as_deref() == Some(h)).map(|(_, _, t)| t).unwrap_or_default()
}

/// Every record handle of a DXF text, with how many records have it.
fn handle_counts(text: &str) -> HashMap<String, usize> {
    let mut out = HashMap::new();
    for (s, _, t) in recs(text) {
        if s != "HEADER"
            && let Some(h) = handle_of(&t)
        {
            *out.entry(h).or_insert(0) += 1;
        }
    }
    out
}

/// FF7: every record of another program's file keeps its handle on save: table records and
/// tables, blocks, the objects CADCraft rewrites from its model (dictionaries, layouts, scales),
/// those it keeps unmodelled, attributes and sequence ends; and references follow.
#[test]
fn records_and_objects_keep_their_handles() {
    let src = fixture();
    let d = read_dxf(src.as_bytes()).expect("read");
    let out = save(&d);
    for (kind, code, name, h) in [
        ("LAYER", 2, "0", "10"),
        ("LAYER", 2, "WALLS", "1A0"),
        ("LTYPE", 2, "ByBlock", "14"),
        ("LTYPE", 2, "Continuous", "16"),
        ("STYLE", 2, "Standard", "11"),
        ("APPID", 2, "ACME", "1A2"),
        ("DIMSTYLE", 2, "Standard", "27"),
        ("BLOCK_RECORD", 2, "*Model_Space", "1F"),
        ("BLOCK_RECORD", 2, "DOOR", "1A3"),
        ("BLOCK_RECORD", 2, "*D1", "1A4"),
        ("TABLE", 2, "LAYER", "2"),
        ("TABLE", 2, "BLOCK_RECORD", "1"),
        ("BLOCK", 2, "DOOR", "1A5"),
        ("BLOCK", 2, "*D1", "1A7"),
        ("VISUALSTYLE", 2, "2dWireframe", "1C0"),
        ("SCALE", 300, "1:2", "1C3"),
        ("XRECORD", 1, "acme-data", "1C5"),
        ("ATTRIB", 2, "TAG1", "308"),
        ("ATTRIB", 2, "TAG2", "309"),
    ] {
        assert_eq!(find(&out, kind, code, name).as_deref(), Some(h), "{kind} {name}");
    }
    assert_eq!(find(&out, "SEQEND", 330, "307").as_deref(), Some("30A"));
    assert!(record(&out, "1E").iter().any(|t| t.code == 1 && t.str() == "Layout1"), "LAYOUT Layout1");
    let nod = recs(&out).into_iter().find(|(s, k, _)| s == "OBJECTS" && k == "DICTIONARY").and_then(|(_, _, t)| handle_of(&t));
    assert_eq!(nod.as_deref(), Some("C"), "the named object dictionary");
    let layouts = recs(&out).into_iter().find(|(_, k, t)| k == "DICTIONARY" && t.iter().any(|x| x.code == 3 && x.str() == "Layout1"));
    assert_eq!(layouts.and_then(|(_, _, t)| handle_of(&t)).as_deref(), Some("1A"), "ACAD_LAYOUT");
    // References follow: the XRECORD's scale, the LINE's xdata naming the layer; the layout's
    // block record.
    assert_eq!(val(&record(&out, "1C5"), 340).as_deref(), Some("1C3"));
    let line = record(&out, "30C");
    assert!(line.iter().any(|t| t.code == 1005 && t.str().trim() == "1A0"), "xdata 1005 names layer WALLS");
    assert_eq!(val(&record(&out, "1E"), 330), Some("1A".to_string()));
    // No handle twice.
    assert!(handle_counts(&out).values().all(|n| *n == 1), "duplicate handles");
    // Saving again changes nothing.
    let back = read_dxf(out.as_bytes()).expect("reopen");
    let again = save(&back);
    assert_eq!(find(&again, "LAYER", 2, "WALLS").as_deref(), Some("1A0"));
    assert_eq!(find(&again, "ATTRIB", 2, "TAG1").as_deref(), Some("308"));
}

/// FF7: a role's handle that the drawing now gives to something else (here an entity) is not
/// taken back; that record gets a new handle and every handle stays unique.
#[test]
fn a_handle_now_in_use_is_not_taken_back() {
    let src = fixture().replace("5\n1A0\n330\n2\n", "5\n300\n330\n2\n");
    assert_ne!(src, fixture());
    let d = read_dxf(src.as_bytes()).expect("read");
    let out = save(&d);
    let walls = find(&out, "LAYER", 2, "WALLS").expect("WALLS");
    assert_ne!(walls, "300");
    assert!(recs(&out).iter().any(|(_, k, t)| k == "CIRCLE" && handle_of(t).as_deref() == Some("300")), "the circle keeps 300");
    assert!(handle_counts(&out).values().all(|n| *n == 1), "duplicate handles");
}

/// The handle-keeping pass with hostile handles: every role's handle at the top of the range,
/// handles that don't parse, a seed below everything. Nothing panics and the output reads.
#[test]
fn hostile_handles_never_panic() {
    let base = fixture();
    let variants = [
        base.replace("5\n1A0\n", "5\nFFFFFFFFFFFFFFFF\n")
            .replace("5\n1E\n", "5\nFFFFFFFFFFFFFFFE\n")
            .replace("$HANDSEED\n5\n400\n", "$HANDSEED\n5\nFFFFFFFFFFFFFFFF\n"),
        base.replace("5\n1A0\n", "5\nZZ\n").replace("5\n14\n", "5\n-1\n").replace("$HANDSEED\n5\n400\n", "$HANDSEED\n5\n1\n"),
        base.replace("5\n1C3\n", "5\n1C2\n").replace("5\n308\n", "5\n307\n"),
    ];
    for (i, v) in variants.iter().enumerate() {
        assert_ne!(v, &base, "variant {i}");
        let Ok(d) = read_dxf(v.as_bytes()) else { continue };
        let out = save(&d);
        assert!(read_dxf(out.as_bytes()).is_ok(), "variant {i}");
    }
}
