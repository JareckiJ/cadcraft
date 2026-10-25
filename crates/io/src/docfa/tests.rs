use std::sync::Arc as SArc;

use cadcraft_color::Color;
use cadcraft_geom::{PolyVertex, Vec2, Vec3};

use super::*;
use crate::{read, save_version, write_docfa};

fn common(layer: &str) -> Common {
    Common { layer: layer.into(), ..Common::default() }
}

fn lwpoly(pts: &[(f64, f64)], closed: bool) -> EntityKind {
    EntityKind::LwPolyline(LwPolyline {
        vertices: pts.iter().map(|(x, y)| PolyVertex::new(Vec2::new(*x, *y))).collect(),
        closed,
        const_width: 0.0,
        elevation: 0.0,
        plinegen: false,
    })
}

fn text(value: &str, at: (f64, f64), height: f64) -> EntityKind {
    EntityKind::Text(Text {
        insert: Vec3::new(at.0, at.1, 0.0),
        align_pt: None,
        height,
        value: value.into(),
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: HAlign::Left,
        valign: VAlign::Baseline,
    })
}

fn insert(block: &str, at: Vec2, rotation: f64) -> EntityKind {
    EntityKind::Insert(Insert {
        block: block.into(),
        insert: at.to3(0.0),
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation,
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    })
}

/// A block definition from a store of entities (through DXF: `Block` has no constructor here).
fn block(name: &str, entities: EntityStore) -> Block {
    let src = format!("0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n2\n{name}\n10\n0\n20\n0\n0\nENDBLK\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n");
    let d = read(src.as_bytes(), "x.dxf").unwrap();
    let mut b = d.block(name).unwrap().as_ref().clone();
    b.entities = entities;
    b
}

/// A plan: walls, a block `DOOR` (a line and an arc) referenced twice, text with accents, multiline
/// text, an ellipse, a spline and a hatch; two surface polygons (one not closed, one red by itself,
/// one green from its layer) and the sheet frame on a layer that is off.
fn plan() -> Drawing {
    let mut d = Drawing::new_metric();
    d.layers.push(Layer { name: "Muri".into(), color: Color::Index(1), ..Layer::default() });
    d.layers.push(Layer { name: "DocFa_Poligoni".into(), color: Color::Index(3), ..Layer::default() });
    d.layers.push(Layer { name: "RIQUADRO".into(), on: false, ..Layer::default() });
    d.add(&Space::Model, common("Muri"), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(10.0, 0.0, 0.0) })).unwrap();
    d.add(&Space::Model, Common { color: Color::Index(1), ..common("0") }, EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(1.0, 1.0, 0.0) }))
        .unwrap();
    d.add(&Space::Model, common("0"), text("Caffè 20°", (1.0, 1.0), 0.25)).unwrap();
    d.add(
        &Space::Model,
        common("0"),
        EntityKind::MText(MText {
            insert: Vec3::new(5.0, 5.0, 0.0),
            height: 0.25,
            width: 0.0,
            attach: 1,
            rotation: 0.0,
            style: "Standard".into(),
            contents: "line one\\Pline two".into(),
            line_spacing: 1.0,
            line_spacing_exact: false,
            columns: None,
        }),
    )
    .unwrap();
    d.add(
        &Space::Model,
        common("0"),
        EntityKind::Ellipse(Ellipse {
            center: Vec3::new(8.0, 8.0, 0.0),
            major: Vec3::new(2.0, 0.0, 0.0),
            ratio: 0.5,
            start: 0.0,
            end: std::f64::consts::TAU,
        }),
    )
    .unwrap();
    d.add(
        &Space::Model,
        common("0"),
        EntityKind::Spline(cadcraft_geom::Spline::from_fit_points(&[
            Vec2::new(1.0, 1.0),
            Vec2::new(2.0, 3.0),
            Vec2::new(4.0, 2.0),
            Vec2::new(5.0, 5.0),
        ])),
    )
    .unwrap();
    d.add(
        &Space::Model,
        common("0"),
        EntityKind::Hatch(Hatch {
            pattern: "ANSI31".into(),
            solid: false,
            loops: vec![HatchLoop {
                vertices: [(0.0, 10.0), (2.0, 10.0), (2.0, 12.0), (0.0, 12.0)].iter().map(|(x, y)| PolyVertex::new(Vec2::new(*x, *y))).collect(),
                outer: true,
                sources: Vec::new(),
            }],
            scale: 1.0,
            angle: 0.0,
            associative: false,
            style: 0,
            elevation: 0.0,
            gradient: None,
            origin: Vec2::ZERO,
            background: None,
            pattern_lines: Vec::new(),
        }),
    )
    .unwrap();
    let mut door = Drawing::new_metric();
    door.add(&Space::Model, common("0"), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(1.0, 0.0, 0.0) })).unwrap();
    door.add(&Space::Model, common("0"), EntityKind::Arc(Arc { center: Vec3::ZERO, radius: 1.0, start: 0.0, end: 1.0 })).unwrap();
    d.blocks.insert("DOOR".into(), SArc::new(block("DOOR", door.model)));
    d.add(&Space::Model, common("0"), insert("DOOR", Vec2::new(20.0, 20.0), 0.0)).unwrap();
    d.add(&Space::Model, common("0"), insert("DOOR", Vec2::new(30.0, 20.0), std::f64::consts::FRAC_PI_2)).unwrap();
    d.add(&Space::Model, common("DocFa_Poligoni"), lwpoly(&[(0.0, 0.0), (4.0, 0.0), (4.0, 3.0)], true)).unwrap();
    d.add(&Space::Model, Common { color: Color::Index(1), ..common("DocFa_Poligoni") }, lwpoly(&[(5.0, 0.0), (9.0, 0.0), (9.0, 3.0)], false))
        .unwrap();
    d.add(&Space::Model, common("RIQUADRO"), lwpoly(&[(0.0, 0.0), (39.0, 0.0), (39.0, 47.0), (0.0, 47.0)], true)).unwrap();
    d
}

fn header(dxf: &str, var: &str) -> Option<String> {
    let tags = cadcraft_dxf::parse_ascii(dxf).unwrap();
    let i = tags.iter().position(|t| t.code == 9 && t.str() == var)?;
    tags.get(i + 1).map(|t| t.str())
}

/// The DXF type of each model-space object.
fn kinds(d: &Drawing) -> Vec<&'static str> {
    d.model
        .iter()
        .map(|e| match e.kind {
            EntityKind::Line(_) => "LINE",
            EntityKind::Arc(_) => "ARC",
            EntityKind::LwPolyline(_) => "LWPOLYLINE",
            EntityKind::Text(_) => "TEXT",
            EntityKind::MText(_) => "MTEXT",
            EntityKind::Insert(_) => "INSERT",
            EntityKind::Hatch(_) => "HATCH",
            EntityKind::Ellipse(_) => "ELLIPSE",
            EntityKind::Spline(_) => "SPLINE",
            _ => "other",
        })
        .collect()
}

#[test]
fn the_copy_explodes_blocks_drops_hatches_and_colours_only_the_polygons() {
    let (copy, report) = drawing(&plan(), 1.0);
    let k = kinds(&copy);
    let count = |t: &str| k.iter().filter(|x| **x == t).count();
    assert_eq!(count("INSERT"), 0, "block references are exploded: {k:?}");
    assert!(copy.blocks.is_empty(), "no block definitions");
    assert_eq!(count("ARC"), 2, "the two doors' arcs: {k:?}");
    assert_eq!(count("LINE"), 2 + 2, "the two lines and the two doors' lines: {k:?}");
    assert_eq!(count("LWPOLYLINE"), 2 + 2, "the ellipse, the spline and the two polygons; no frame: {k:?}");
    assert_eq!(count("TEXT"), 1 + 2, "the text and the two lines of the MTEXT: {k:?}");
    assert_eq!(count("HATCH") + count("MTEXT") + count("ELLIPSE") + count("SPLINE"), 0, "{k:?}");
    assert!(copy.model.iter().any(|e| matches!(&e.kind, EntityKind::Text(t) if t.value == "Caffe 20%%d")), "text in ASCII");
    // Colours: the polygons carry theirs (green from the layer, red of its own); nothing else does.
    let mut colours: Vec<Color> = copy.model.iter().filter(|e| e.common.layer == LAYER).map(|e| e.common.color).collect();
    colours.sort_by_key(|c| c.to_aci());
    assert_eq!(colours, vec![Color::Index(1), Color::Index(3)]);
    assert!(
        copy.model
            .iter()
            .filter(|e| e.common.layer != LAYER)
            .all(|e| e.common.color == Color::ByLayer && e.common.linetype.eq_ignore_ascii_case("ByLayer"))
    );
    // Layers white, continuous, on; the polygons' layer under the exact name DOCFA reads.
    assert!(copy.layers.iter().all(|l| l.color == Color::Index(7) && l.linetype == "Continuous" && l.on && !l.frozen));
    assert!(copy.layers.iter().any(|l| l.name == LAYER));
    let notes = report.lines();
    for expected in [
        "2 surface polygons on DOCFA_POLIGONI",
        "2 INSERT exploded",
        "1 HATCH (DOCFA takes no hatching) left out",
        "1 object on a layer that is off or frozen left out",
        "1 MTEXT → TEXT",
    ] {
        assert!(notes.iter().any(|n| n == expected), "{expected}: {notes:?}");
    }
    assert!(notes.iter().any(|n| n.starts_with("1 polylines on DOCFA_POLIGONI are not closed")), "{notes:?}");
    assert!(!notes.iter().any(|n| n.contains("origin")), "the plan starts at 0,0: {notes:?}");
}

#[test]
fn the_dxf_is_autocad_2000_in_metres_with_no_declared_unit() {
    let mut d = plan();
    d.header.set_i64("INSUNITS", 4);
    let (bytes, notes) = write_docfa(&d, "plan.dxf", 1.0).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert_eq!(header(&text, "$ACADVER").as_deref(), Some("AC1015"));
    assert_eq!(header(&text, "$INSUNITS").as_deref(), Some("0"), "metres, but no declared unit, as the Agenzia's templates");
    assert!(text.is_ascii() && !text.contains("\\U+"), "text is transliterated");
    assert!(!notes.is_empty());
    let back = read(&bytes, "plan.dxf").unwrap();
    assert_eq!(save_version(&back), cadcraft_dxf::Version::R2000);
    assert!(!back.model.iter().any(|e| matches!(e.kind, EntityKind::Insert(_) | EntityKind::Hatch(_) | EntityKind::MText(_))));
    let poly = back.model.iter().find(|e| matches!(e.kind, EntityKind::LwPolyline(_)) && e.common.layer == LAYER).unwrap();
    assert!(matches!(poly.common.color, Color::Index(1 | 3)), "{:?}", poly.common.color);
    // Neither SVG nor PDF.
    assert!(write_docfa(&d, "plan.pdf", 1.0).is_err());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn the_dwg_is_autocad_2000_with_the_same_contents() {
    let d = plan();
    let (dwg, notes) = write_docfa(&d, "plan.dwg", 1.0).unwrap();
    assert_eq!(cadcraft_dwg::version(&dwg).as_deref(), Some("AC1015"));
    assert!(notes.iter().any(|n| n.contains("from DXF files only")), "{notes:?}");
    let back = read(&dwg, "plan.dwg").unwrap();
    assert_eq!(save_version(&back), cadcraft_dxf::Version::R2000);
    assert!(!back.model.iter().any(|e| matches!(e.kind, EntityKind::Insert(_) | EntityKind::Hatch(_))));
    let (dxf, _) = write_docfa(&d, "plan.dxf", 1.0).unwrap();
    let from_dxf = read(&dxf, "plan.dxf").unwrap();
    assert_eq!(back.model.len(), from_dxf.model.len(), "the DWG holds what the DXF holds");
    assert!(back.model.iter().any(|e| matches!(e.kind, EntityKind::LwPolyline(_)) && e.common.layer.eq_ignore_ascii_case(LAYER)));
}

#[test]
fn scale_turns_millimetres_into_metres() {
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, common("0"), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(1000.0, 0.0, 0.0) })).unwrap();
    d.add(&Space::Model, common("0"), text("k", (500.0, 500.0), 250.0)).unwrap();
    d.add(&Space::Model, common("0"), EntityKind::Circle(Circle { center: Vec3::new(2000.0, 0.0, 0.0), radius: 100.0 })).unwrap();
    let mut door = Drawing::new_metric();
    door.add(&Space::Model, common("0"), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(1.0, 0.0, 0.0) })).unwrap();
    d.blocks.insert("DOOR".into(), SArc::new(block("DOOR", door.model)));
    d.add(&Space::Model, common("0"), insert("DOOR", Vec2::new(3000.0, 0.0), 0.0)).unwrap();
    let (copy, report) = drawing(&d, 0.001);
    for e in copy.model.iter() {
        match &e.kind {
            EntityKind::Line(l) if l.a.x == 0.0 => assert!((l.b.x - 1.0).abs() < 1e-12),
            EntityKind::Line(l) => assert!((l.a.x - 3.0).abs() < 1e-12 && (l.b.x - 3.001).abs() < 1e-12, "the door scales once: {l:?}"),
            EntityKind::Text(t) => assert!((t.height - 0.25).abs() < 1e-12 && (t.insert.x - 0.5).abs() < 1e-12),
            EntityKind::Circle(c) => assert!((c.radius - 0.1).abs() < 1e-12 && (c.center.x - 2.0).abs() < 1e-12),
            other => panic!("{other:?}"),
        }
    }
    assert!(!report.lines().iter().any(|n| n.contains("unit right")), "{:?}", report.lines());
    // Unscaled, the check says the plan can't be in metres; scaled twice, that it is too small.
    assert!(drawing(&d, 1.0).1.lines().iter().any(|n| n.contains("more than 1000 m")));
    assert!(drawing(&d, 0.000_001).1.lines().iter().any(|n| n.contains("less than 1 m")));
    // A scale that isn't a positive number is 1.
    for k in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(drawing(&d, k).0.model.iter().any(|e| matches!(&e.kind, EntityKind::Line(l) if l.b.x == 1000.0)), "{k}");
    }
}

#[test]
fn a_plan_off_the_origin_is_reported() {
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, common("0"), EntityKind::Line(Line { a: Vec3::new(-5.0, 0.0, 0.0), b: Vec3::new(1.0, 0.0, 0.0) })).unwrap();
    assert!(drawing(&d, 1.0).1.lines().iter().any(|n| n.contains("lower-left corner at 0,0")));
}

/// A table's block is relative to its own origin (the ACAD_TABLE reference adds the insertion
/// point): its lines and text must land at the insertion point.
#[test]
fn a_table_lands_at_its_insertion_point() {
    let cell = |t: &str| TableCell { text: t.into(), merged: None, ..TableCell::default() };
    let mut d = Drawing::new_metric();
    d.add(
        &Space::Model,
        common("0"),
        EntityKind::Table(Table {
            insert: Vec3::new(100.0, 200.0, 0.0),
            col_widths: vec![2.0, 3.0],
            row_heights: vec![1.0, 1.0],
            cells: vec![vec![cell("A"), cell("B")], vec![cell("1"), cell("2")]],
            style: "Standard".into(),
            text_height: 0.2,
            title: false,
            header: false,
            row_styles: Default::default(),
            breaks: Default::default(),
            data_link: Default::default(),
        }),
    )
    .unwrap();
    let (copy, _) = drawing(&d, 1.0);
    let ext = copy.extents(&Space::Model);
    assert!(ext.min.x >= 99.9 && ext.min.y >= 197.9 && ext.max.x <= 105.1 && ext.max.y <= 200.1, "{ext:?}");
    assert!(copy.model.iter().any(|e| matches!(&e.kind, EntityKind::Text(t) if t.value == "B")));
}

#[test]
fn transliteration_keeps_italian_readable() {
    assert_eq!(transliterate("Soggiorno è a 2° piano, h. 2,70 m – ±1 cm, Ø 20"), "Soggiorno e a 2%%d piano, h. 2,70 m - %%p1 cm, %%c 20");
    assert_eq!(transliterate("ÀÈÌÒÙ àèìòù"), "AEIOU aeiou");
    assert_eq!(transliterate("plain"), "plain");
    assert_eq!(transliterate("图"), "?");
}

#[test]
fn hostile_drawings_do_not_panic() {
    // NaN coordinates, a block referencing itself, an enormous array.
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, common("0"), EntityKind::Line(Line { a: Vec3::new(f64::NAN, 0.0, 0.0), b: Vec3::new(1.0, f64::INFINITY, 0.0) })).unwrap();
    d.add(&Space::Model, common("0"), text("t", (f64::NAN, 0.0), f64::NAN)).unwrap();
    let mut cyclic = Drawing::new_metric();
    cyclic.add(&Space::Model, common("0"), insert("LOOP", Vec2::ZERO, 0.0)).unwrap();
    cyclic.add(&Space::Model, common("0"), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(1.0, 0.0, 0.0) })).unwrap();
    cyclic.add(&Space::Model, common("0"), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(0.0, 1.0, 0.0) })).unwrap();
    d.blocks.insert("LOOP".into(), SArc::new(block("LOOP", cyclic.model)));
    d.add(&Space::Model, common("0"), insert("LOOP", Vec2::ZERO, 0.0)).unwrap();
    let mut array = insert("LOOP", Vec2::ZERO, 0.0);
    if let EntityKind::Insert(i) = &mut array {
        (i.cols, i.rows, i.col_spacing, i.row_spacing) = (100_000, 100_000, 1.0, 1.0);
    }
    d.add(&Space::Model, common("0"), array).unwrap();
    for scale in [1.0, f64::NAN] {
        let (bytes, notes) = write_docfa(&d, "x.dxf", scale).unwrap();
        for expected in ["non-finite", "nested too deep", "too large"] {
            assert!(notes.iter().any(|n| n.contains(expected)), "{expected}: {notes:?}");
        }
        let _ = read(&bytes, "x.dxf");
    }
}

/// Block references that fan out (each holding many references to the next, in a cycle) draw
/// nothing but would visit billions of objects down to the depth limit: the visit budget stops
/// the expansion.
#[test]
fn a_block_nesting_that_fans_out_stops_at_the_visit_budget() {
    let mut d = Drawing::new_metric();
    for (name, next) in [("A", "B"), ("B", "A")] {
        let mut inner = Drawing::new_metric();
        for k in 0..16 {
            inner.add(&Space::Model, common("0"), insert(next, Vec2::new(f64::from(k), 0.0), 0.0)).unwrap();
        }
        d.blocks.insert(name.into(), SArc::new(block(name, inner.model)));
    }
    d.add(&Space::Model, common("0"), insert("A", Vec2::ZERO, 0.0)).unwrap();
    let start = std::time::Instant::now();
    let (copy, report) = drawing(&d, 1.0);
    assert!(copy.model.is_empty());
    assert!(report.lines().iter().any(|n| n.contains("cut short")), "{:?}", report.lines());
    assert!(start.elapsed().as_secs() < 120, "bounded: {:?}", start.elapsed());
}
