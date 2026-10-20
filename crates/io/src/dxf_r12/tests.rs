use std::sync::Arc as SArc;

use cadcraft_color::Color;
use cadcraft_geom::{PolyVertex, Vec2, Vec3};

use super::*;
use crate::{SaveOptions, read, save_version, write_with};

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

fn mtext(contents: &str, at: (f64, f64), attach: u8) -> EntityKind {
    EntityKind::MText(MText {
        insert: Vec3::new(at.0, at.1, 0.0),
        height: 0.25,
        width: 0.0,
        attach,
        rotation: 0.0,
        style: "Standard".into(),
        contents: contents.into(),
        line_spacing: 1.0,
        line_spacing_exact: false,
        columns: None,
    })
}

fn hatch(pts: &[(f64, f64)], solid: bool) -> EntityKind {
    EntityKind::Hatch(Hatch {
        pattern: if solid { "SOLID".into() } else { "ANSI31".into() },
        solid,
        loops: vec![HatchLoop { vertices: pts.iter().map(|(x, y)| PolyVertex::new(Vec2::new(*x, *y))).collect(), outer: true, sources: Vec::new() }],
        scale: 1.0,
        angle: 0.0,
        associative: false,
        style: 0,
        elevation: 0.0,
        gradient: None,
        origin: Vec2::ZERO,
        background: None,
        pattern_lines: Vec::new(),
    })
}

/// A drawing with a block `DOOR` (one line and an arc), and the entity kinds R12 converts.
fn sample() -> Drawing {
    let mut d = Drawing::new_metric();
    d.layers.push(Layer { name: "Muri interni/piano 1".into(), color: Color::Index(1), ..Layer::default() });
    d.add(&Space::Model, common("Muri interni/piano 1"), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(10.0, 0.0, 0.0) })).unwrap();
    d.add(&Space::Model, common("0"), EntityKind::Circle(Circle { center: Vec3::new(3.0, 4.0, 0.0), radius: 2.5 })).unwrap();
    d.add(&Space::Model, common("0"), EntityKind::Arc(Arc { center: Vec3::ZERO, radius: 1.0, start: 0.5, end: 2.0 })).unwrap();
    d.add(
        &Space::Model,
        common("0"),
        EntityKind::LwPolyline(LwPolyline {
            vertices: vec![PolyVertex::new(Vec2::ZERO), PolyVertex::with_bulge(Vec2::new(4.0, 0.0), 0.5), PolyVertex::new(Vec2::new(4.0, 3.0))],
            closed: true,
            const_width: 0.0,
            elevation: 0.0,
            plinegen: false,
        }),
    )
    .unwrap();
    d.add(&Space::Model, common("0"), text("Caffè 20°", (1.0, 1.0), 0.25)).unwrap();
    d.add(&Space::Model, common("0"), mtext("line one\\Pline two", (5.0, 5.0), 1)).unwrap();
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
        EntityKind::Spline(cadcraft_geom::Spline::from_fit_points(&[Vec2::ZERO, Vec2::new(1.0, 2.0), Vec2::new(3.0, 1.0), Vec2::new(4.0, 4.0)])),
    )
    .unwrap();
    d.add(&Space::Model, common("0"), hatch(&[(0.0, 10.0), (2.0, 10.0), (2.0, 12.0), (0.0, 12.0)], false)).unwrap();
    // A block with a line and an arc, referenced twice (one rotated).
    let mut door = Drawing::new_metric();
    door.add(&Space::Model, common("0"), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(1.0, 0.0, 0.0) })).unwrap();
    door.add(&Space::Model, common("0"), EntityKind::Arc(Arc { center: Vec3::ZERO, radius: 1.0, start: 0.0, end: 1.0 })).unwrap();
    let mut block = door_block("DOOR", door.model);
    block.base = Vec3::ZERO;
    d.blocks.insert("DOOR".into(), SArc::new(block));
    let insert = |at: Vec2, rot: f64| {
        EntityKind::Insert(Insert {
            block: "DOOR".into(),
            insert: at.to3(0.0),
            scale: Vec3::new(1.0, 1.0, 1.0),
            rotation: rot,
            attribs: Vec::new(),
            cols: 1,
            rows: 1,
            col_spacing: 0.0,
            row_spacing: 0.0,
        })
    };
    d.add(&Space::Model, common("0"), insert(Vec2::new(20.0, 20.0), 0.0)).unwrap();
    d.add(&Space::Model, common("0"), insert(Vec2::new(30.0, 20.0), std::f64::consts::FRAC_PI_2)).unwrap();
    d
}

/// A block definition from a store of entities (through DXF: `Block` has no constructor here).
fn door_block(name: &str, entities: EntityStore) -> Block {
    let src = format!("0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n2\n{name}\n10\n0\n20\n0\n0\nENDBLK\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n");
    let d = read(src.as_bytes(), "x.dxf").unwrap();
    let mut b = d.block(name).unwrap().as_ref().clone();
    b.entities = entities;
    b
}

/// The DXF type names in the ENTITIES section, in order.
fn entity_types(dxf: &str) -> Vec<String> {
    let tags = cadcraft_dxf::parse_ascii(dxf).unwrap();
    let mut out = Vec::new();
    let mut in_entities = false;
    let mut i = 0;
    while i + 1 < tags.len() {
        let (t, next) = (&tags[i], &tags[i + 1]);
        if t.code == 0 && t.str() == "SECTION" && next.code == 2 {
            in_entities = next.str() == "ENTITIES";
        } else if t.code == 0 && in_entities && !matches!(t.str().as_str(), "ENDSEC" | "SECTION") {
            out.push(t.str());
        }
        i += 1;
    }
    out
}

fn count(types: &[String], name: &str) -> usize {
    types.iter().filter(|t| *t == name).count()
}

fn header(dxf: &str, var: &str) -> Option<String> {
    let tags = cadcraft_dxf::parse_ascii(dxf).unwrap();
    let i = tags.iter().position(|t| t.code == 9 && t.str() == var)?;
    tags.get(i + 1).map(|t| t.str())
}

#[test]
fn r12_file_has_only_what_release_12_knows() {
    let dxf = write(&sample(), &R12Options::default());
    assert_eq!(header(&dxf, "$ACADVER").as_deref(), Some("AC1009"));
    assert!(dxf.is_ascii(), "R12 text is in the code page: escaped here");
    assert!(dxf.contains("\\U+00E8"), "non-ASCII text is escaped, not lost");
    for absent in ["LWPOLYLINE", "MTEXT", "ELLIPSE", "SPLINE", "HATCH", "AcDb", "CLASSES", "OBJECTS", "HANDSEED", "$HANDLING"] {
        assert!(!dxf.contains(absent), "{absent} must not appear in an R12 file");
    }
    let types = entity_types(&dxf);
    assert!(count(&types, "LINE") >= 2, "the drawing's line and the hatch's pattern lines; block contents stay in the block");
    assert_eq!(count(&types, "CIRCLE"), 1);
    assert_eq!(count(&types, "ARC"), 1);
    assert_eq!(count(&types, "POLYLINE"), 3, "the polyline, the ellipse and the spline");
    assert_eq!(count(&types, "SEQEND"), 3);
    assert!(count(&types, "VERTEX") > 3 + 8, "curves are tessellated");
    assert_eq!(count(&types, "TEXT"), 3, "one TEXT and the two lines of the MTEXT");
    assert_eq!(count(&types, "INSERT"), 2);
    assert!(
        types.iter().all(|t| matches!(t.as_str(), "LINE" | "CIRCLE" | "ARC" | "POLYLINE" | "VERTEX" | "SEQEND" | "TEXT" | "INSERT" | "POINT")),
        "{types:?}"
    );
    // No handles anywhere.
    let tags = cadcraft_dxf::parse_ascii(&dxf).unwrap();
    assert!(tags.iter().all(|t| t.code != 5 && t.code != 330 && t.code != 100));
    // The block is defined with its contents and an R12 name.
    assert!(dxf.contains("\r\nBLOCK\r\n  8\r\n0\r\n  2\r\nDOOR\r\n"));
    assert!(dxf.contains("MURI_INTERNI_PIANO_1"), "layer names are R12 names (upper case, no `/` or spaces)");
    assert!(!dxf.contains("Muri interni"));
}

#[test]
fn r12_reads_back_as_the_reduced_drawing() {
    let d = sample();
    let bytes = write_with(&d, "x.dxf", &SaveOptions { version: Some(cadcraft_dxf::Version::R12), ..SaveOptions::default() }).unwrap();
    let back = read(&bytes, "x.dxf").unwrap();
    assert_eq!(save_version(&back), cadcraft_dxf::Version::R12, "the file's version is remembered");
    let kinds: Vec<&str> = back.model.iter().map(|e| e.kind.type_name()).collect();
    assert_eq!(kinds.iter().filter(|k| **k == "Polyline").count(), 3);
    assert_eq!(kinds.iter().filter(|k| **k == "Text").count(), 3);
    assert_eq!(kinds.iter().filter(|k| **k == "Block Reference").count(), 2);
    assert!(!kinds.contains(&"MText") && !kinds.contains(&"Ellipse") && !kinds.contains(&"Hatch"));
    // The bulge survived the POLYLINE/VERTEX form, and the original text too (via the escape).
    let bulged = back.model.iter().find_map(|e| match &e.kind {
        EntityKind::LwPolyline(p) if p.closed && p.vertices.len() == 3 => Some(p.vertices[1].bulge),
        _ => None,
    });
    assert_eq!(bulged, Some(0.5));
    let texts: Vec<String> = back.model.iter().filter_map(|e| if let EntityKind::Text(t) = &e.kind { Some(t.value.clone()) } else { None }).collect();
    assert!(texts.contains(&"Caffè 20°".to_string()), "{texts:?}");
    assert!(texts.contains(&"line one".to_string()) && texts.contains(&"line two".to_string()), "{texts:?}");
    // The block came back with both its entities, and the references their rotation.
    let door = back.block("DOOR").unwrap();
    assert_eq!(door.entities.len(), 2);
    let rotations: Vec<f64> = back.model.iter().filter_map(|e| if let EntityKind::Insert(i) = &e.kind { Some(i.rotation) } else { None }).collect();
    assert!(rotations.iter().any(|r| (r - std::f64::consts::FRAC_PI_2).abs() < 1e-9), "{rotations:?}");
    // Reading the R12 file and writing it again as R12 is stable.
    let again = write_with(&back, "x.dxf", &SaveOptions::default()).unwrap();
    assert_eq!(header(std::str::from_utf8(&again).unwrap(), "$ACADVER").as_deref(), Some("AC1009"));
}

#[test]
fn mtext_lines_follow_the_attachment_point() {
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, common("0"), mtext("a\\Pb\\Pc", (10.0, 10.0), 7)).unwrap(); // bottom left
    d.add(&Space::Model, common("0"), mtext("x\\Py", (0.0, 0.0), 3)).unwrap(); // top right
    let back = read(write(&d, &R12Options::default()).as_bytes(), "x.dxf").unwrap();
    let lines: Vec<(String, Vec3, HAlign)> = back
        .model
        .iter()
        .filter_map(|e| if let EntityKind::Text(t) = &e.kind { Some((t.value.clone(), t.align_pt.unwrap_or(t.insert), t.halign)) } else { None })
        .collect();
    let find = |v: &str| lines.iter().find(|l| l.0 == v).cloned().unwrap();
    // Bottom-left: the last line's baseline is at the attachment point, the others above.
    let (a, b, c) = (find("a"), find("b"), find("c"));
    assert!((c.1.y - 10.0).abs() < 1e-9 && b.1.y > c.1.y && a.1.y > b.1.y, "{lines:?}");
    assert!((b.1.y - c.1.y - 0.25 * LINE_SPACING).abs() < 1e-9);
    assert_eq!(a.2, HAlign::Left);
    // Top-right: the first line hangs below the attachment point, right-justified at it.
    let (x, y) = (find("x"), find("y"));
    assert!((x.1.y - (0.0 - 0.25)).abs() < 1e-9 && y.1.y < x.1.y, "{lines:?}");
    assert_eq!(x.2, HAlign::Right);
    assert!((x.1.x).abs() < 1e-9);
}

#[test]
fn docfa_profile_explodes_blocks_drops_hatches_and_colours_the_polygons() {
    let mut d = sample();
    d.layers.push(Layer { name: "DocFa_Poligoni".into(), color: Color::Index(3), ..Layer::default() });
    d.layers.push(Layer { name: "RIQUADRO".into(), on: false, ..Layer::default() });
    // Two polygons (one not closed), one ByLayer (green = 3), one red by itself.
    d.add(&Space::Model, common("DocFa_Poligoni"), lwpoly(&[(0.0, 0.0), (4.0, 0.0), (4.0, 3.0)], true)).unwrap();
    d.add(&Space::Model, Common { color: Color::Index(1), ..common("DocFa_Poligoni") }, lwpoly(&[(5.0, 0.0), (9.0, 0.0), (9.0, 3.0)], false))
        .unwrap();
    // The sheet frame on a layer that is off: left out, as the DOCFA guides want.
    d.add(&Space::Model, common("RIQUADRO"), lwpoly(&[(0.0, 0.0), (39.0, 0.0), (39.0, 47.0), (0.0, 47.0)], true)).unwrap();
    // A red line elsewhere keeps no colour in the file.
    d.add(&Space::Model, Common { color: Color::Index(1), ..common("0") }, EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(1.0, 1.0, 0.0) }))
        .unwrap();
    let o = R12Options { docfa: true, scale: 1.0 };
    let dxf = write(&d, &o);
    let types = entity_types(&dxf);
    assert_eq!(count(&types, "INSERT"), 0, "block references are exploded");
    assert_eq!(count(&types, "ARC"), 1 + 2, "the arc and the two doors' arcs");
    assert_eq!(count(&types, "LINE"), 2 + 2, "the two lines and the two doors' lines");
    assert!(!dxf.contains("\r\nBLOCK\r\n"), "no block definitions");
    assert_eq!(count(&types, "POLYLINE"), 3 + 2, "polyline, ellipse, spline and the two polygons; no frame, no hatch lines");
    assert!(!dxf.contains("\\U+"), "text is transliterated: {dxf}");
    assert!(dxf.contains("\r\nCaffe 20%%d\r\n"));
    // Colours: only the polygons carry group 62 (3 from the layer, 1 of its own).
    let tags = cadcraft_dxf::parse_ascii(&dxf).unwrap();
    let colours: Vec<i64> = tags
        .iter()
        .enumerate()
        .filter(|(i, t)| t.code == 62 && *i > 0 && tags[..*i].iter().rev().find(|x| x.code == 0).is_some_and(|x| x.str() == "POLYLINE"))
        .map(|(_, t)| t.value.as_i64())
        .collect();
    assert_eq!(colours, vec![3, 1], "{colours:?}");
    let entity_colours = tags
        .iter()
        .enumerate()
        .filter(|(i, t)| t.code == 62 && tags[..*i].iter().rev().find(|x| x.code == 0).is_some_and(|x| x.str() == "LINE"))
        .count();
    assert_eq!(entity_colours, 0, "lines carry no colour in the DOCFA profile");
    // Layers are white and continuous; names are R12 names.
    assert!(dxf.contains("\r\nDOCFA_POLIGONI\r\n 70\r\n0\r\n 62\r\n7\r\n  6\r\nCONTINUOUS\r\n"), "{dxf}");
    let notes = notes(&d, &o);
    assert!(notes.iter().any(|n| n == "2 surface polygons on DOCFA_POLIGONI"), "{notes:?}");
    assert!(notes.iter().any(|n| n.starts_with("1 polylines on DOCFA_POLIGONI are not closed")), "{notes:?}");
    assert!(notes.iter().any(|n| n == "2 INSERT exploded"), "{notes:?}");
    assert!(notes.iter().any(|n| n == "1 HATCH (DOCFA takes no hatching) left out"), "{notes:?}");
    assert!(notes.iter().any(|n| n == "1 object on a layer that is off or frozen left out"), "{notes:?}");
    assert!(notes.iter().any(|n| n == "1 MTEXT → TEXT"), "{notes:?}");
    assert!(notes.iter().any(|n| n.contains("lower-left corner at 0,0")), "the arc reaches left of the origin: {notes:?}");
}

#[test]
fn scale_turns_millimetres_into_metres() {
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, common("0"), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(1000.0, 0.0, 0.0) })).unwrap();
    d.add(&Space::Model, common("0"), text("k", (500.0, 500.0), 250.0)).unwrap();
    d.add(&Space::Model, common("0"), EntityKind::Circle(Circle { center: Vec3::new(2000.0, 0.0, 0.0), radius: 100.0 })).unwrap();
    let o = R12Options { docfa: true, scale: 0.001 };
    let back = read(write(&d, &o).as_bytes(), "x.dxf").unwrap();
    for e in back.model.iter() {
        match &e.kind {
            EntityKind::Line(l) => assert!((l.b.x - 1.0).abs() < 1e-12),
            EntityKind::Text(t) => assert!((t.height - 0.25).abs() < 1e-12 && (t.insert.x - 0.5).abs() < 1e-12),
            EntityKind::Circle(c) => assert!((c.radius - 0.1).abs() < 1e-12 && (c.center.x - 2.0).abs() < 1e-12),
            other => panic!("{other:?}"),
        }
    }
    let dxf = write(&d, &o);
    assert_eq!(header(&dxf, "$EXTMAX").as_deref(), Some("2.1"));
    // A block reference scales once: the definition stays in block units in plain R12, and is
    // exploded at the file's scale in the DOCFA profile.
    let mut doors = sample();
    doors.model = Drawing::new_metric().model;
    doors
        .add(
            &Space::Model,
            common("0"),
            EntityKind::Insert(Insert {
                block: "DOOR".into(),
                insert: Vec3::new(1000.0, 1000.0, 0.0),
                scale: Vec3::new(2.0, 2.0, 2.0),
                rotation: 0.0,
                attribs: Vec::new(),
                cols: 1,
                rows: 1,
                col_spacing: 0.0,
                row_spacing: 0.0,
            }),
        )
        .unwrap();
    for docfa in [false, true] {
        let back = read(write(&doors, &R12Options { docfa, scale: 0.001 }).as_bytes(), "x.dxf").unwrap();
        let ext = back.extents(&Space::Model);
        // The door at 1000,1000 scaled ×2 spans 1000 → 1002 in millimetres: 1 → 1.002 m.
        assert!((ext.min.x - 1.0).abs() < 1e-9 && (ext.max.x - 1.002).abs() < 1e-9, "docfa={docfa}: {ext:?}");
    }
    assert_eq!(header(&dxf, "$LIMMIN").as_deref(), Some("0.0"));
    // Unscaled, DOCFA's check says the drawing is too big for metres.
    let unscaled = notes(&d, &R12Options { docfa: true, scale: 1.0 });
    assert!(unscaled.iter().any(|n| n.contains("more than 1000 units")), "{unscaled:?}");
    assert!(!notes(&d, &o).iter().any(|n| n.contains("more than 1000 units")));
}

#[test]
fn docfa_warns_about_a_sheet_off_the_origin() {
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, common("0"), EntityKind::Line(Line { a: Vec3::new(-5.0, 0.0, 0.0), b: Vec3::new(1.0, 0.0, 0.0) })).unwrap();
    let n = notes(&d, &R12Options { docfa: true, scale: 1.0 });
    assert!(n.iter().any(|n| n.contains("lower-left corner at 0,0")), "{n:?}");
    assert!(notes(&d, &R12Options::default()).is_empty(), "plain R12 keeps a line as it is");
}

#[test]
fn names_are_release_12_names_and_stay_distinct() {
    let mut w = W { t: Vec::new(), names: Default::default(), used_names: Default::default() };
    assert_eq!(w.name("Muri interni/piano 1"), "MURI_INTERNI_PIANO_1");
    assert_eq!(w.name("muri INTERNI/piano 1"), "MURI_INTERNI_PIANO_1", "the same name, any case, maps once");
    assert_eq!(w.name("Muri interni-piano 1"), "MURI_INTERNI-PIANO_1");
    assert_eq!(w.name("Muri interni.piano 1"), "MURI_INTERNI_PIANO_1_2", "a clash gets a suffix");
    assert_eq!(w.name("Muri interni:piano 1"), "MURI_INTERNI_PIANO_1_3");
    assert_eq!(w.name(""), "0");
    let long = "A".repeat(40);
    assert_eq!(w.name(&long).len(), 31);
    assert_eq!(w.name(&format!("{long}B")).len(), 31);
    assert_ne!(w.name(&long), w.name(&format!("{long}B")));
    assert_eq!(w.name("Déjà vu"), "D_J__VU");
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
    cyclic
        .add(
            &Space::Model,
            common("0"),
            EntityKind::Insert(Insert {
                block: "LOOP".into(),
                insert: Vec3::ZERO,
                scale: Vec3::new(1.0, 1.0, 1.0),
                rotation: 0.0,
                attribs: Vec::new(),
                cols: 1,
                rows: 1,
                col_spacing: 0.0,
                row_spacing: 0.0,
            }),
        )
        .unwrap();
    cyclic.add(&Space::Model, common("0"), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(1.0, 0.0, 0.0) })).unwrap();
    d.blocks.insert("LOOP".into(), SArc::new(door_block("LOOP", cyclic.model)));
    d.add(
        &Space::Model,
        common("0"),
        EntityKind::Insert(Insert {
            block: "LOOP".into(),
            insert: Vec3::ZERO,
            scale: Vec3::new(1.0, 1.0, 1.0),
            rotation: 0.0,
            attribs: Vec::new(),
            cols: 100_000,
            rows: 100_000,
            col_spacing: 1.0,
            row_spacing: 1.0,
        }),
    )
    .unwrap();
    for o in [R12Options::default(), R12Options { docfa: true, scale: 1.0 }, R12Options { docfa: true, scale: f64::NAN }] {
        let dxf = write(&d, &o);
        assert!(dxf.contains("EOF"));
        let n = notes(&d, &o);
        assert!(n.iter().any(|n| n.contains("non-finite")), "{n:?}");
        let _ = read(dxf.as_bytes(), "x.dxf");
    }
    let n = notes(&d, &R12Options { docfa: true, scale: 1.0 });
    assert!(n.iter().any(|n| n.contains("nested too deep") || n.contains("too large")), "{n:?}");
}

#[test]
fn save_options_route_r12_and_refuse_what_it_cannot_do() {
    let d = sample();
    let r12 = SaveOptions { version: Some(cadcraft_dxf::Version::R12), ..SaveOptions::default() };
    assert_eq!(header(std::str::from_utf8(&write_with(&d, "x.dxf", &r12).unwrap()).unwrap(), "$ACADVER").as_deref(), Some("AC1009"));
    let docfa = SaveOptions { docfa: true, ..SaveOptions::default() };
    let bytes = write_with(&d, "x.dxf", &docfa).unwrap();
    assert!(!std::str::from_utf8(&bytes).unwrap().contains("INSERT"), "DOCFA implies R12 with its profile");
    assert!(!crate::save_notes(&d, "x.dxf", &docfa).is_empty());
    assert!(crate::save_notes(&d, "x.dxf", &SaveOptions::default()).is_empty());
    assert!(crate::save_notes(&d, "x.dwg", &r12).is_empty());
    assert!(write_with(&d, "x.dxf", &SaveOptions { binary: true, ..r12 }).is_err(), "no binary R12");
    assert!(write_with(&d, "x.dwg", &r12).is_err(), "no R12 DWG");
    assert!(write_with(&d, "x.dxf", &SaveOptions { version: Some(cadcraft_dxf::Version::R2018), docfa: true, ..SaveOptions::default() }).is_err());
    assert!(write_with(&d, "x.svg", &docfa).is_err());
}

/// A table's block is relative to its own origin (the ACAD_TABLE reference adds the insertion
/// point): R12 has no table, so its lines and text must land at the insertion point.
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
    for docfa in [false, true] {
        let back = read(write(&d, &R12Options { docfa, scale: 1.0 }).as_bytes(), "x.dxf").unwrap();
        let ext = back.extents(&Space::Model);
        assert!(ext.min.x >= 99.9 && ext.min.y >= 197.9 && ext.max.x <= 105.1 && ext.max.y <= 200.1, "docfa={docfa}: {ext:?}");
        assert!(back.model.iter().any(|e| matches!(&e.kind, EntityKind::Text(t) if t.value == "B")), "docfa={docfa}");
    }
}
