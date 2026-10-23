//! ARC_DIMENSION (issue #1166): AutoCAD's arc-length dimension is CADCraft's arc-length
//! dimension (what DIMARC draws), read from and written to DXF (and through the DWG bridge) as
//! the DXF Reference's `ARC_DIMENSION` / `AcDbArcDimension`, with its `*D` block like the other
//! dimensions. Before, it was read as an unknown entity (not drawn, counted as a Proxy) and DIMARC
//! was saved as an aligned DIMENSION that AutoCAD shows as the chord.
//!
//! The fixture is written by hand with the groups the DXF Reference lists for ARC_DIMENSION.

use cadcraft_doc::{DimKind, Drawing, Entity, EntityKind, Space};
use cadcraft_geom::Vec3;
use cadcraft_io::{SaveOptions, Version, read, read_dxf, write_with};

/// The drawing as DXF text in `version`.
fn save_as(d: &Drawing, version: Version) -> String {
    let bytes = write_with(d, "a.dxf", &SaveOptions { version: Some(version), binary: false }).unwrap();
    String::from_utf8(bytes).unwrap()
}

/// The drawing as DXF text in AutoCAD's current format (the default of a new drawing).
fn write_dxf(d: &Drawing) -> String {
    save_as(d, Version::R2018)
}

/// DXF text from lines of `code value`.
fn dxf(src: &str) -> String {
    let mut out = String::new();
    for line in src.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let (code, value) = line.split_once(' ').unwrap_or((line, ""));
        out.push_str(&format!("{code}\n{value}\n"));
    }
    out
}

/// An arc and an arc-length dimension on it, with the dimension's `*D1` block holding one line.
const ARC_DIM: &str = "
0 SECTION
2 BLOCKS
0 BLOCK
8 0
2 *D1
70 1
10 0
20 0
30 0
3 *D1
0 LINE
8 0
10 320
20 150
11 330
21 175
0 ENDBLK
0 ENDSEC
0 SECTION
2 ENTITIES
0 ARC
8 0
10 300
20 150
40 20
50 0
51 90
0 ARC_DIMENSION
5 41
100 AcDbEntity
8 0
100 AcDbDimension
2 *D1
10 330
20 175
11 327.7
21 177.7
70 5
42 31.4159
100 AcDbArcDimension
13 320
23 150
14 300
24 170
15 300
25 150
40 0
41 1.5708
0 ENDSEC
0 EOF
";

fn pairs(dxf: &str) -> Vec<(i32, String)> {
    let lines: Vec<&str> = dxf.lines().collect();
    lines.chunks(2).filter_map(|c| Some((c.first()?.trim().parse().ok()?, c.get(1)?.trim().to_string()))).collect()
}

/// The records of a DXF: (type, groups after it).
fn records(dxf: &str) -> Vec<(String, Vec<(i32, String)>)> {
    let mut out: Vec<(String, Vec<(i32, String)>)> = Vec::new();
    for (code, v) in pairs(dxf) {
        if code == 0 {
            out.push((v, Vec::new()));
        } else if let Some(last) = out.last_mut() {
            last.1.push((code, v));
        }
    }
    out
}

fn num(groups: &[(i32, String)], code: i32) -> Option<f64> {
    groups.iter().find(|(c, _)| *c == code).and_then(|(_, v)| v.parse().ok())
}

fn text(groups: &[(i32, String)], code: i32) -> Option<&str> {
    groups.iter().find(|(c, _)| *c == code).map(|(_, v)| v.as_str())
}

fn dims(d: &Drawing) -> Vec<(&Entity, &cadcraft_doc::Dimension)> {
    d.model.iter().filter_map(|e| if let EntityKind::Dimension(x) = &e.kind { Some((e.as_ref(), x)) } else { None }).collect()
}

fn near(a: Vec3, x: f64, y: f64) -> bool {
    near_by(a, x, y, 1e-9)
}

fn near_by(a: Vec3, x: f64, y: f64, tol: f64) -> bool {
    (a.x - x).abs() < tol && (a.y - y).abs() < tol
}

/// The one ARC_DIMENSION record of a saved file.
fn arc_dim_record(out: &str) -> Vec<(i32, String)> {
    let recs: Vec<_> = records(out).into_iter().filter(|(k, _)| k == "ARC_DIMENSION").collect();
    assert_eq!(recs.len(), 1, "ARC_DIMENSION records: {}\n{out}", recs.len());
    recs.into_iter().next().map(|r| r.1).unwrap_or_default()
}

/// What an arc-length dimension of the fixture must carry, read back. A dimension whose text
/// the user did not move gets its text position recomputed on save, as for every dimension, so
/// after a save it only has to be where the hand-written one is, give or take half a unit.
fn check_dim(d: &Drawing, text_tol: f64) {
    let ds = dims(d);
    assert_eq!(ds.len(), 1, "dimensions: {}", ds.len());
    let dm = ds[0].1;
    assert_eq!(dm.kind, DimKind::ArcLength);
    assert!(near(dm.p15, 300.0, 150.0), "centre {:?}", dm.p15);
    assert!(near(dm.p13, 320.0, 150.0), "start {:?}", dm.p13);
    assert!(near(dm.p14, 300.0, 170.0), "end {:?}", dm.p14);
    assert!((dm.measurement - 31.4159).abs() < 1e-3, "measurement {}", dm.measurement);
    assert!(near_by(dm.text_mid, 327.7, 177.7, text_tol), "text {:?}", dm.text_mid);
    assert!(near(dm.defpt, 330.0, 175.0), "defpt {:?}", dm.defpt);
}

#[test]
fn reads_an_arc_dimension_as_the_arc_length_dimension() {
    let d = read_dxf(dxf(ARC_DIM).as_bytes()).unwrap();
    check_dim(&d, 1e-9);
    let dm = dims(&d)[0].1;
    assert_eq!(dm.style, "Standard");
    assert_eq!(dm.block.as_deref(), Some("*D1"));
    assert!(d.block("*D1").is_some());
    assert!(!d.model.iter().any(|e| matches!(e.kind, EntityKind::Unknown(_))), "still an unknown entity");
}

#[test]
fn counts_as_a_dimension_not_a_proxy() {
    // What `info` and AUDIT count: by object type name.
    let d = read_dxf(dxf(ARC_DIM).as_bytes()).unwrap();
    let names: Vec<&str> = d.model.iter().map(|e| e.kind.type_name()).collect();
    assert_eq!(names.iter().filter(|n| **n == "Dimension").count(), 1, "{names:?}");
    assert!(!names.contains(&"Proxy"), "{names:?}");
    assert_eq!(names.iter().filter(|n| **n == "Arc").count(), 1, "{names:?}");
}

#[test]
fn writes_the_arc_length_dimension_as_arc_dimension() {
    let d = read_dxf(dxf(ARC_DIM).as_bytes()).unwrap();
    let out = write_dxf(&d);
    let recs = records(&out);
    assert!(!recs.iter().any(|(k, _)| k == "DIMENSION"), "written as a DIMENSION:\n{out}");
    let g = arc_dim_record(&out);
    // Subclass markers, in the order of the Reference.
    let marks: Vec<&str> = g.iter().filter(|(c, _)| *c == 100).map(|(_, v)| v.as_str()).collect();
    assert_eq!(marks, ["AcDbEntity", "AcDbDimension", "AcDbArcDimension"]);
    // 70: arc-length is the three-point angular type (as AutoCAD 2027 writes it), plus 32.
    assert_eq!(num(&g, 70), Some(37.0), "{g:?}");
    assert!((num(&g, 42).unwrap_or(0.0) - 31.4159).abs() < 1e-3);
    assert_eq!((num(&g, 13), num(&g, 23)), (Some(320.0), Some(150.0)));
    assert_eq!((num(&g, 14), num(&g, 24)), (Some(300.0), Some(170.0)));
    assert_eq!((num(&g, 15), num(&g, 25)), (Some(300.0), Some(150.0)));
    // 40 and 41: the arc's start and end angle, radians.
    assert!(num(&g, 40).unwrap_or(9.0).abs() < 1e-9, "{g:?}");
    assert!((num(&g, 41).unwrap_or(0.0) - std::f64::consts::FRAC_PI_2).abs() < 1e-6, "{g:?}");
    // Its anonymous block is generated like those of the other dimensions.
    let blk = text(&g, 2).unwrap_or("").to_string();
    assert!(blk.starts_with("*D"), "block {blk:?}");
    let defined = recs.iter().any(|(k, b)| k == "BLOCK" && text(b, 2) == Some(blk.as_str()));
    assert!(defined, "block {blk} not defined:\n{out}");
    // And the class is declared.
    assert!(recs.iter().any(|(k, c)| k == "CLASS" && text(c, 1) == Some("ARC_DIMENSION") && text(c, 2) == Some("AcDbArcDimension")), "{out}");
}

#[test]
fn round_trips_through_dxf_twice() {
    let once = write_dxf(&read_dxf(dxf(ARC_DIM).as_bytes()).unwrap());
    let d = read_dxf(once.as_bytes()).unwrap();
    check_dim(&d, 0.5);
    let twice = write_dxf(&d);
    check_dim(&read_dxf(twice.as_bytes()).unwrap(), 0.5);
    assert_eq!(arc_dim_record(&twice).len(), arc_dim_record(&once).len());
}

#[test]
fn a_new_arc_length_dimension_round_trips_with_its_text_position() {
    let mut d = Drawing::new_metric();
    let mut dm = match read_dxf(dxf(ARC_DIM).as_bytes())
        .unwrap()
        .model
        .iter()
        .find_map(|e| if let EntityKind::Dimension(x) = &e.kind { Some(x.clone()) } else { None })
    {
        Some(x) => x,
        None => panic!("no dimension"),
    };
    dm.block = None;
    dm.user_text_pos = true;
    dm.text_mid = Vec3::new(340.0, 180.0, 0.0);
    let h = d.add(&Space::Model, Default::default(), EntityKind::Dimension(dm)).unwrap();
    let out = write_dxf(&d);
    assert_eq!(num(&arc_dim_record(&out), 70), Some(165.0), "37 + user text position (128)");
    let back = read_dxf(out.as_bytes()).unwrap();
    match &back.entity(h).unwrap().kind {
        EntityKind::Dimension(x) => {
            assert_eq!(x.kind, DimKind::ArcLength);
            assert!(x.user_text_pos && near(x.text_mid, 340.0, 180.0), "{x:?}");
            assert!(near(x.p15, 300.0, 150.0));
        }
        _ => panic!("not a dimension"),
    }
}

#[test]
fn round_trips_through_dwg() {
    let out = write_dxf(&read_dxf(dxf(ARC_DIM).as_bytes()).unwrap());
    let dwg = cadcraft_dwg::dxf_to_dwg(out.as_bytes()).unwrap();
    let back = read_dxf(&cadcraft_dwg::dwg_to_dxf(&dwg).unwrap()).unwrap();
    let ds = dims(&back);
    assert_eq!(ds.len(), 1, "{:?}", back.model.iter().map(|e| e.kind.type_name()).collect::<Vec<_>>());
    assert_eq!(ds[0].1.kind, DimKind::ArcLength);
    assert!(near(ds[0].1.p15, 300.0, 150.0) && near(ds[0].1.p13, 320.0, 150.0) && near(ds[0].1.p14, 300.0, 170.0), "{:?}", ds[0].1);
}

#[test]
fn the_old_cadcraft_file_still_reads() {
    // What 0.7.0 wrote: an aligned DIMENSION (70 = 33) with the centre in CADCRAFT/ARCLEN xdata.
    let old = dxf("
0 SECTION
2 ENTITIES
0 DIMENSION
8 0
10 330
20 175
11 327.7
21 177.7
70 33
42 31.4159
3 Standard
100 AcDbAlignedDimension
13 320
23 150
14 300
24 170
1001 CADCRAFT
1000 ARCLEN
1002 {
1010 300
1020 150
1030 0
1002 }
0 ENDSEC
0 EOF
");
    let d = read_dxf(old.as_bytes()).unwrap();
    let ds = dims(&d);
    assert_eq!(ds.len(), 1);
    assert_eq!(ds[0].1.kind, DimKind::ArcLength);
    assert!(near(ds[0].1.p15, 300.0, 150.0));
}

#[test]
fn damaged_arc_dimensions_do_not_panic() {
    // No points at all, NaN and huge numbers, a centre on the start point.
    for body in [
        "",
        "13 nan\n23 inf\n14 1e308\n24 -1e308\n15 nan\n25 nan\n42 nan\n",
        "13 5\n23 5\n14 5\n24 5\n15 5\n25 5\n",
        "13 1e300\n23 1e300\n14 -1e300\n24 1e300\n15 0\n25 0\n40 1e308\n41 -1e308\n",
    ] {
        let src = dxf(&format!(
            "0 SECTION\n2 ENTITIES\n0 ARC_DIMENSION\n8 0\n100 AcDbDimension\n10 1\n20 1\n70 5\n100 AcDbArcDimension\n{body}0 ENDSEC\n0 EOF\n"
        ));
        if let Ok(d) = read_dxf(src.as_bytes()) {
            // What reads must save and read again, as the same arc-length dimensions with their
            // blocks.
            let out = write_dxf(&d);
            let back = read_dxf(out.as_bytes()).unwrap_or_else(|e| panic!("body {body:?}: the saved file does not read back: {e}\n{out}"));
            assert_eq!(dims(&back).len(), dims(&d).len(), "body {body:?}: dimensions before and after\n{out}");
            for (_, dm) in dims(&back) {
                assert_eq!(dm.kind, DimKind::ArcLength, "body {body:?}\n{out}");
                if let Some(b) = &dm.block {
                    assert!(back.block(b).is_some(), "body {body:?}: the dimension names block {b}, which is not in the file\n{out}");
                }
            }
        }
    }
}

/// The fixture's dimension with another ARC_DIMENSION subclass body (groups 13, 14, 15 and what
/// follows), and `extra` groups before the subclass marker.
fn fixture_with(body: &str, extra: &str) -> String {
    let start = ARC_DIM.find("100 AcDbArcDimension").unwrap_or(0);
    let end = ARC_DIM.find("0 ENDSEC\n0 EOF").unwrap_or(ARC_DIM.len());
    let head = ARC_DIM[..start].replace("42 31.4159\n", "");
    format!("{head}{extra}100 AcDbArcDimension\n{body}{}", &ARC_DIM[end..])
}

fn read_dim(src: &str) -> cadcraft_doc::Dimension {
    let d = read_dxf(dxf(src).as_bytes()).unwrap();
    match dims(&d).first() {
        Some((_, x)) => (*x).clone(),
        None => panic!("no dimension"),
    }
}

#[test]
fn r2000_keeps_the_aligned_dimension_with_the_centre_in_xdata() {
    // ARC_DIMENSION came with AutoCAD 2006 (format 2004): R2000 has no such entity.
    let d = read_dxf(dxf(ARC_DIM).as_bytes()).unwrap();
    {
        let v = Version::R2000;
        let out = save_as(&d, v);
        let recs = records(&out);
        assert!(!recs.iter().any(|(k, _)| k == "ARC_DIMENSION"), "{v:?} has ARC_DIMENSION:\n{out}");
        assert!(!recs.iter().any(|(k, c)| k == "CLASS" && text(c, 1) == Some("ARC_DIMENSION")), "{v:?} has its CLASS");
        let dim: Vec<_> = recs.iter().filter(|(k, _)| k == "DIMENSION").collect();
        assert_eq!(dim.len(), 1, "{v:?}");
        // An aligned dimension (1 | 32) with the centre in CADCRAFT/ARCLEN xdata.
        assert_eq!(num(&dim[0].1, 70), Some(33.0), "{v:?}");
        assert!(dim[0].1.iter().any(|(c, x)| *c == 1000 && x == "ARCLEN"), "{v:?} lost its centre");
        // CADCraft reads it back as the same arc-length dimension.
        check_dim(&read_dxf(out.as_bytes()).unwrap(), 0.5);
    }
}

#[test]
fn from_2004_on_it_is_an_arc_dimension() {
    let d = read_dxf(dxf(ARC_DIM).as_bytes()).unwrap();
    for v in [Version::R2004, Version::R2007, Version::R2010, Version::R2013, Version::R2018] {
        let out = save_as(&d, v);
        assert!(!arc_dim_record(&out).is_empty(), "{v:?}");
        assert!(!records(&out).iter().any(|(k, _)| k == "DIMENSION"), "{v:?} still a DIMENSION");
        assert!(!out.contains("ARCLEN"), "{v:?} keeps the centre in xdata");
        check_dim(&read_dxf(out.as_bytes()).unwrap(), 0.5);
    }
}

#[test]
fn dwg_follows_the_same_version_threshold() {
    let d = read_dxf(dxf(ARC_DIM).as_bytes()).unwrap();
    for v in Version::ALL {
        let bytes = write_with(&d, "a.dwg", &SaveOptions { version: Some(v), binary: false }).unwrap();
        let back = read(&bytes, "a.dwg").unwrap();
        check_dim(&back, 0.5);
        let dxf_text = save_as(&back, v);
        let arc = records(&dxf_text).iter().any(|(k, _)| k == "ARC_DIMENSION");
        assert_eq!(arc, v >= Version::R2004, "{v:?}");
    }
}

#[test]
fn clockwise_ends_follow_the_start_and_end_angles() {
    // Another writer lists 13 and 14 clockwise; 40 and 41 say the arc runs from 0 to 90 degrees.
    // Counter-clockwise from 13 to 14 would be the 270 degree arc and the saved length would change.
    let body = "13 300\n23 170\n14 320\n24 150\n15 300\n25 150\n40 0\n41 1.5708\n";
    let src = fixture_with(body, "70 5\n42 31.4159\n");
    let dm = read_dim(&src);
    assert!(near(dm.p13, 320.0, 150.0) && near(dm.p14, 300.0, 170.0), "{:?} {:?}", dm.p13, dm.p14);
    let d = read_dxf(dxf(&src).as_bytes()).unwrap();
    let g = arc_dim_record(&write_dxf(&d));
    assert!((num(&g, 42).unwrap_or(0.0) - 31.4159).abs() < 1e-3, "{g:?}");
}

#[test]
fn clockwise_ends_follow_the_measurement_without_angles() {
    // No 40/41: the arc whose length is nearer to 42 (the quarter, 31.4159, not the 94.2478 of the rest).
    let body = "13 300\n23 170\n14 320\n24 150\n15 300\n25 150\n";
    let dm = read_dim(&fixture_with(body, "70 5\n42 31.4159\n"));
    assert!(near(dm.p13, 320.0, 150.0) && near(dm.p14, 300.0, 170.0), "{:?} {:?}", dm.p13, dm.p14);
    // The major arc, when that is what 42 says, stays the major arc.
    let dm = read_dim(&fixture_with(body, "70 5\n42 94.2478\n"));
    assert!(near(dm.p13, 300.0, 170.0) && near(dm.p14, 320.0, 150.0), "{:?} {:?}", dm.p13, dm.p14);
}

#[test]
fn counter_clockwise_ends_stay_and_unhelpful_files_stay_counter_clockwise() {
    let ccw = "13 320\n23 150\n14 300\n24 170\n15 300\n25 150\n40 0\n41 1.5708\n";
    let dm = read_dim(&fixture_with(ccw, "70 5\n42 31.4159\n"));
    assert!(near(dm.p13, 320.0, 150.0) && near(dm.p14, 300.0, 170.0));
    // Neither angles, nor a measurement, nor non-finite ones: as before, counter-clockwise from 13 to 14.
    for (body, extra) in [
        ("13 300\n23 170\n14 320\n24 150\n15 300\n25 150\n", "70 5\n"),
        ("13 300\n23 170\n14 320\n24 150\n15 300\n25 150\n40 nan\n41 inf\n", "70 5\n42 nan\n"),
    ] {
        let dm = read_dim(&fixture_with(body, extra));
        assert!(near(dm.p13, 300.0, 170.0) && near(dm.p14, 320.0, 150.0), "{:?} {:?}", dm.p13, dm.p14);
    }
}
