//! What another program's DXF keeps of its layouts and styles through open → save → reopen in
//! CADCraft (gap FF1): the plot paper units of a layout, and unnamed shape styles.
//!
//! The fixture is synthetic, written from the public DXF Reference: a drawing as another program
//! might save it.

use cadcraft_doc::Drawing;
use cadcraft_dxf::{Tag, records, sections};
use cadcraft_io::read_dxf;

/// DXF text from lines of `code value` (the value may be empty).
fn dxf(src: &str) -> String {
    let mut out = String::new();
    for line in src.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let (code, value) = line.split_once(' ').unwrap_or((line, ""));
        out.push_str(&format!(
            "{code}
{value}
"
        ));
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

/// Every record of a DXF text: (type, groups).
fn recs(text: &str) -> Vec<(String, Vec<Tag>)> {
    let tags = cadcraft_dxf::parse(text.as_bytes()).expect("parse");
    sections(&tags).into_iter().flat_map(|s| records(&s.tags)).collect()
}

fn val(t: &[Tag], code: i32) -> Option<String> {
    t.iter().find(|x| x.code == code).map(|x| x.str().trim().to_string())
}

/// The groups of the first record of type `kind` with a group `code` that is `value`.
fn find(text: &str, kind: &str, code: i32, value: &str) -> Vec<Tag> {
    recs(text)
        .into_iter()
        .find(|(k, t)| k == kind && t.iter().any(|x| x.code == code && x.str().trim().eq_ignore_ascii_case(value)))
        .map(|(_, t)| t)
        .unwrap_or_default()
}

/// A layout in inches (plot paper units 0) of a metric drawing stays in inches in the file
/// (every layout's paper-in-mm flag went false → true); CADCraft still sizes it from
/// MEASUREMENT.
#[test]
fn layouts_keep_their_plot_paper_units() {
    let d = read_dxf(fixture().as_bytes()).expect("read");
    let out = save(&d);
    let layout = find(&out, "LAYOUT", 1, "Layout1");
    assert_eq!(val(&layout, 72).as_deref(), Some("0"));
    assert_eq!(cadcraft_render::sheet(&d, "Layout1").expect("sheet").unit_mm, 1.0);
    let back = read_dxf(out.as_bytes()).expect("reopen");
    let page = |d: &Drawing| d.layouts.iter().find(|l| l.name == "Layout1").map(|l| l.page.clone());
    assert_eq!(page(&back), page(&d));
    // Where the file's units are what CADCraft would write anyway, nothing is kept.
    let metric = fixture().replacen(
        "72
0
73
0
74
5
",
        "72
1
73
0
74
5
",
        1,
    );
    let d = read_dxf(metric.as_bytes()).expect("read");
    assert_eq!(page(&d).and_then(|p| p.plot_paper_units), None);
    // A hostile value is not kept.
    let hostile = fixture().replacen(
        "72
0
73
0
74
5
",
        "72
99999
73
0
74
5
",
        1,
    );
    let d = read_dxf(hostile.as_bytes()).expect("read");
    assert_eq!(page(&d).and_then(|p| p.plot_paper_units), None);
}

/// An unnamed shape style (a loaded shape file) is kept (one STYLE record was dropped on open).
#[test]
fn unnamed_shape_styles_are_kept() {
    let d = read_dxf(fixture().as_bytes()).expect("read");
    assert_eq!(d.foreign.shape_files, vec!["ltypeshp.shx".to_string()]);
    let out = save(&d);
    let style = find(&out, "STYLE", 3, "ltypeshp.shx");
    assert_eq!(val(&style, 70).as_deref(), Some("1"));
    assert_eq!(val(&style, 2).unwrap_or_default(), "");
    let back = read_dxf(out.as_bytes()).expect("reopen");
    assert_eq!(back.foreign.shape_files, vec!["ltypeshp.shx".to_string()]);
    // Saved again: still one record for the file.
    let again = save(&back);
    assert_eq!(recs(&again).iter().filter(|(k, t)| k == "STYLE" && val(t, 3).as_deref() == Some("ltypeshp.shx")).count(), 1);
}
