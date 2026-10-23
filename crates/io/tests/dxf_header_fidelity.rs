//! The header variables of another program's DXF survive open → save → reopen in CADCraft (gap
//! FF1): the variables CADCraft doesn't use itself are written back with the group codes they
//! were read with, pointers at records follow those records, and dimension variables that
//! override the current style stay as read.
//!
//! The fixture is synthetic, written from the public DXF Reference: a drawing as another program
//! might save it.

use cadcraft_doc::{Drawing, HVal};
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
70 1
0 LAYER
5 10
330 2
100 AcDbSymbolTableRecord
100 AcDbLayerTableRecord
2 0
70 0
62 7
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
78 8
140 0.18
0 ENDTAB
0 TABLE
2 BLOCK_RECORD
5 1
330 0
100 AcDbSymbolTable
70 2
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
0 ENDSEC
0 SECTION
2 ENTITIES
0 LINE
5 300
330 1F
100 AcDbEntity
8 0
100 AcDbLine
10 0
20 0
30 0
11 5
21 0
31 0
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
3 ACAD_VISUALSTYLE
350 1BF
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
72 1
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

fn save_as(d: &Drawing, version: Version) -> String {
    String::from_utf8(write_with(d, "x.dxf", &SaveOptions { version: Some(version), binary: false }).expect("save")).expect("text")
}

/// The values of header variable `var`.
fn header(text: &str, var: &str) -> Vec<(i32, String)> {
    let tags = cadcraft_dxf::parse(text.as_bytes()).expect("parse");
    let Some(h) = sections(&tags).into_iter().find(|s| s.name == "HEADER") else { return Vec::new() };
    let Some(at) = h.tags.iter().position(|t| t.code == 9 && t.str().trim() == var) else { return Vec::new() };
    h.tags.iter().skip(at + 1).take_while(|t| t.code != 9).map(|t| (t.code, t.str().trim().to_string())).collect()
}

/// The handle of the first record of type `kind` in the OBJECTS section.
fn object_handle(text: &str, kind: &str) -> Option<String> {
    let tags = cadcraft_dxf::parse(text.as_bytes()).expect("parse");
    let objects = sections(&tags).into_iter().find(|s| s.name == "OBJECTS")?;
    records(&objects.tags)
        .into_iter()
        .find(|(k, _)| k == kind)
        .and_then(|(_, t)| t.iter().find(|x| x.code == 5).map(|x| x.str().trim().to_ascii_uppercase()))
}

/// The header variables CADCraft doesn't use are written back with their group codes; the
/// version's GUID is not (a save is a new version); a pointer at a record follows the record, and
/// a pointer at a record the file doesn't have becomes null. Into an older version than the
/// file's, they are left out.
#[test]
fn header_variables_are_kept() {
    let d = read_dxf(fixture().as_bytes()).expect("read");
    let out = save(&d);
    let one = |v: &str| header(&out, v);
    assert_eq!(one("$ACADMAINTVER"), vec![(70, "105".to_string())]);
    assert_eq!(one("$DGNFRAME"), vec![(280, "2".to_string())]);
    assert_eq!(one("$OSMODE"), vec![(70, "4133".to_string())]);
    assert_eq!(one("$PELEVATION").first().map(|(c, v)| (*c, v.parse::<f64>().ok())), Some((40, Some(2.5))));
    let org: Vec<(i32, f64)> = one("$UCSORG").into_iter().map(|(c, v)| (c, v.parse().unwrap_or(f64::NAN))).collect();
    assert_eq!(org, vec![(10, 1.0), (20, 2.0), (30, 3.0)]);
    assert!(one("$VERSIONGUID").is_empty());
    assert_eq!(one("$INTERFEREOBJVS"), vec![(345, "0".to_string())]);
    // The visual style is kept (with a new handle): the pointer follows it.
    let vs = object_handle(&out, "VISUALSTYLE").expect("visual style kept");
    assert_eq!(one("$DRAGVS"), vec![(349, vs)]);
    // Each variable once.
    let tags = cadcraft_dxf::parse(out.as_bytes()).expect("parse");
    let mut vars: Vec<String> = tags.iter().filter(|t| t.code == 9).map(Tag::str).collect();
    let n = vars.len();
    vars.sort();
    vars.dedup();
    assert_eq!(vars.len(), n, "a header variable written twice");
    // Reopened, the drawing has the same values with the same codes.
    let back = read_dxf(out.as_bytes()).expect("reopen");
    for var in ["ACADMAINTVER", "DGNFRAME", "OSMODE", "UCSORG", "PELEVATION"] {
        assert_eq!(back.header.get(var), d.header.get(var), "{var}");
        assert_eq!(back.header.code(var), d.header.code(var), "{var}");
    }
    assert_eq!(back.header.get("INTERFEREOBJVS"), Some(&HVal::Str("0".into())));
    // Saved again, the same header.
    assert_eq!(header(&save(&back), "$DGNFRAME"), vec![(280, "2".to_string())]);
    // Saved as 2000, older than the file's 2018: not written.
    let old = save_as(&d, Version::R2000);
    assert!(header(&old, "$DGNFRAME").is_empty());
    assert!(header(&old, "$OSMODE").is_empty());
}

/// A drawing made in CADCraft has no codes read, so its header is as before.
#[test]
fn new_drawings_write_no_kept_variables() {
    let d = Drawing::new_metric();
    assert!(d.header.codes.is_empty());
    let out = save(&d);
    assert!(header(&out, "$DGNFRAME").is_empty());
    assert!(header(&out, "$VERSIONGUID").is_empty());
}

/// A dimension variable of the header that overrides the current style is written back as read;
/// one that only repeats the style follows the style when the style changes.
#[test]
fn header_dimension_overrides_are_kept() {
    let mut d = read_dxf(fixture().as_bytes()).expect("read");
    let out = save(&d);
    assert_eq!(header(&out, "$DIMZIN"), vec![(70, "3".to_string())], "the override");
    if let Some(s) = d.dim_styles.iter_mut().find(|s| s.name == "Standard") {
        s.text_height = 0.5;
    }
    let out = save(&d);
    let txt: Vec<f64> = header(&out, "$DIMTXT").into_iter().filter_map(|(_, v)| v.parse().ok()).collect();
    assert_eq!(txt, vec![0.5], "follows the style");
    assert_eq!(header(&out, "$DIMZIN"), vec![(70, "3".to_string())]);
}

/// Hostile header values: codes that can't hold the value, non-finite reals, pointers that don't
/// parse. Nothing panics and the output reads back.
#[test]
fn hostile_header_values_never_panic() {
    let hostile = fixture().replacen(
        "9\n$DGNFRAME\n280\n2\n",
        "9\n$DGNFRAME\n280\nnot a number\n9\n$ODDREAL\n40\nnan\n9\n$ODDPTR\n340\nZZZZ\n9\n$ODDPOINT\n10\ninf\n20\n1\n",
        1,
    );
    let d = read_dxf(hostile.as_bytes()).expect("read");
    for v in [Version::R2018, Version::R2000] {
        let out = save_as(&d, v);
        assert!(read_dxf(out.as_bytes()).is_ok(), "{v:?}");
    }
    assert_eq!(header(&save(&d), "$ODDPTR"), vec![(340, "0".to_string())]);
}
