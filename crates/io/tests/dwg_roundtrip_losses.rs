//! Data that used to be lost only when a drawing is saved as DWG and reopened (the DWG bridge
//! converts our DXF with acadrust), and the paper-space viewport the DXF writer adds for it.
#![cfg(not(target_arch = "wasm32"))]

use cadcraft_color::{Color, Rgb};
use cadcraft_doc::*;
use cadcraft_geom::{Vec2, Vec3};

fn through_dwg(d: &Drawing) -> Drawing {
    let dwg = cadcraft_io::write(d, "x.dwg").unwrap();
    cadcraft_io::read(&dwg, "x.dwg").unwrap()
}

fn line(d: &mut Drawing, space: &Space, common: Common) -> Handle {
    d.add(space, common, EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(10.0, 5.0, 0.0) })).unwrap()
}

fn viewport(id: u32) -> EntityKind {
    EntityKind::Viewport(Viewport {
        center: Vec3::new(50.0, 40.0, 0.0),
        width: 80.0,
        height: 60.0,
        view_center: Vec2::new(10.0, 10.0),
        view_height: 50.0,
        id,
        locked: false,
        frozen_layers: vec![],
        layer_colors: vec![],
        clip: None,
        on: true,
    })
}

/// The (code, value) pairs of every `kind` record in DXF text.
fn records(text: &str, kind: &str) -> Vec<Vec<(String, String)>> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let mut out: Vec<Vec<(String, String)>> = Vec::new();
    let mut inside = false;
    for pair in lines.chunks(2) {
        let [code, value] = pair else { break };
        if *code == "0" {
            inside = *value == kind;
            if inside {
                out.push(Vec::new());
            }
        } else if inside && let Some(rec) = out.last_mut() {
            rec.push((code.to_string(), value.to_string()));
        }
    }
    out
}

fn group<'a>(rec: &'a [(String, String)], code: &str) -> Option<&'a str> {
    rec.iter().find(|(c, _)| c == code).map(|(_, v)| v.as_str())
}

fn viewport_ids(l: &Layout) -> Vec<u32> {
    l.entities.iter().filter_map(|e| if let EntityKind::Viewport(v) = &e.kind { Some(v.id) } else { None }).collect()
}

/// True colours (entity and layer) and entity transparency: a new drawing's DWG is written as
/// AutoCAD 2018 (R2000 DWG has neither), while the DXF the bridge reads stays R2000.
#[test]
fn dwg_keeps_true_colours_and_transparency() {
    let mut d = Drawing::new_metric();
    d.layers.push(Layer { name: "Fancy".into(), color: Color::True(Rgb(12, 200, 99)), ..Layer::default() });
    let h = line(
        &mut d,
        &Space::Model,
        Common { layer: "Fancy".into(), color: Color::True(Rgb(1, 2, 3)), transparency: Transparency::Percent(40), ..Common::default() },
    );

    let dwg = cadcraft_io::write(&d, "x.dwg").unwrap();
    assert_eq!(cadcraft_dwg::version(&dwg).as_deref(), Some("AC1032"));
    let header = records(&cadcraft_io::write_dxf(&d), "SECTION");
    assert_eq!(header.first().and_then(|h| group(h, "1")), Some("AC1015"), "the DXF stays R2000");

    let back = cadcraft_io::read(&dwg, "x.dwg").unwrap();
    let e = back.entity(h).unwrap();
    assert_eq!(e.common.color, Color::True(Rgb(1, 2, 3)));
    assert_eq!(e.common.transparency, Transparency::Percent(40));
    assert_eq!(back.layers.iter().find(|l| l.name == "Fancy").map(|l| l.color), Some(Color::True(Rgb(12, 200, 99))));
}

/// Entities of a layout other than the first stay in that layout.
#[test]
fn dwg_keeps_paper_space_entities_in_their_layout() {
    let mut d = Drawing::new_metric();
    let second = Space::Paper(d.layouts[1].name.clone());
    let h = d.add(&second, Common::default(), EntityKind::Circle(Circle { center: Vec3::new(1.0, 1.0, 0.0), radius: 0.5 })).unwrap();
    let back = through_dwg(&d);
    assert!(back.layouts[1].entities.contains(h), "{:?}", back.layouts.iter().map(|l| l.entities.len()).collect::<Vec<_>>());
    assert!(!back.layouts[0].entities.contains(h));
}

/// Table styles (R2000 DWG has no TABLESTYLE objects).
#[test]
fn dwg_keeps_table_styles() {
    let mut d = Drawing::new_metric();
    let sched = TableStyle { name: "Sched".into(), text_height: 2.5, margin: 1.0, title: false, header: true, ..TableStyle::default() };
    d.table_styles.push(sched.clone());
    let back = through_dwg(&d);
    assert_eq!(back.table_styles.iter().find(|s| s.name == "Sched"), Some(&sched));
}

/// DWG doesn't store viewport ids; readers number a layout's viewports in order, and id 1 is the
/// paper-space viewport (the sheet, not drawn). The writer puts that viewport ahead of a layout's
/// own viewports, so a viewport made in CADCraft (id 2) keeps its id instead of becoming the
/// sheet; our reader drops the added viewport again, so a DXF round trip is unchanged.
#[test]
fn viewport_ids_survive_dwg_and_the_paper_viewport_stays_implicit() {
    let mut d = Drawing::new_metric();
    let first = Space::Paper(d.layouts[0].name.clone());
    let h = d.add(&first, Common::default(), viewport(2)).unwrap();
    d.add(&first, Common::default(), viewport(3)).unwrap();

    let text = cadcraft_io::write_dxf(&d);
    let vps = records(&text, "VIEWPORT");
    assert_eq!(vps.len(), 3, "{text}");
    assert_eq!(group(&vps[0], "69"), Some("1"));
    assert_eq!(group(&vps[0], "1000"), Some("PAPERVIEW"));
    // Viewports are written "on": status 68 positive, and 90 with the always-set bit 32768.
    assert_eq!(group(&vps[1], "68"), Some("2"));
    assert_eq!(group(&vps[1], "90"), Some("32768"));

    let back = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
    assert_eq!(viewport_ids(&back.layouts[0]), [2, 3]);
    assert_eq!(back.layouts[0].entities.len(), 2);

    let back = through_dwg(&d);
    assert_eq!(viewport_ids(&back.layouts[0]), [2, 3]);
    assert!(back.layouts[0].entities.contains(h));

    // A layout that has its paper-space viewport (from another program) gets no second one.
    let mut d = Drawing::new_metric();
    d.add(&first, Common::default(), viewport(1)).unwrap();
    d.add(&first, Common::default(), viewport(2)).unwrap();
    let text = cadcraft_io::write_dxf(&d);
    assert!(!text.contains("PAPERVIEW"));
    assert_eq!(viewport_ids(&cadcraft_io::read_dxf(text.as_bytes()).unwrap().layouts[0]), [1, 2]);
}

#[test]
fn ucs_elevation_and_ortho_survives_dwg_and_dxf() {
    let mut d = Drawing::new_metric();
    d.ucss.push(Ucs {
        name: "Elevated".into(),
        origin: Vec3::new(10.0, 20.0, 30.0),
        x_axis: Vec3::X,
        y_axis: Vec3::Y,
        elevation: 15.5,
        ortho_type: 2,
    });
    let dxf = cadcraft_io::write_dxf(&d);
    assert!(dxf.contains("146\r\n15.5") || dxf.contains("146\n15.5"));
    assert!(dxf.contains(" 79\r\n2") || dxf.contains("79\n2"));
    let back_dxf = cadcraft_io::read_dxf(dxf.as_bytes()).unwrap();
    let u_dxf = back_dxf.ucss.iter().find(|u| u.name == "Elevated").unwrap();
    assert_eq!(u_dxf.elevation, 15.5);
    assert_eq!(u_dxf.ortho_type, 2);

    let back_dwg = through_dwg(&d);
    let u_dwg = back_dwg.ucss.iter().find(|u| u.name == "Elevated").unwrap();
    assert_eq!(u_dwg.elevation, 15.5);
    assert_eq!(u_dwg.ortho_type, 2);
}

#[test]
fn mtext_background_color_survives_dwg_and_dxf() {
    let mut d = Drawing::new_metric();
    let h = d
        .add(
            &Space::Model,
            Common::default(),
            EntityKind::MText(MText {
                insert: Vec3::new(5.0, 5.0, 0.0),
                height: 2.5,
                width: 50.0,
                attach: 1,
                rotation: 0.0,
                style: "Standard".into(),
                contents: "Hello Background".into(),
                line_spacing: 1.0,
                line_spacing_exact: false,
                columns: None,
                background: Some(Color::True(Rgb(250, 240, 230))),
                background_scale: Some(1.25),
            }),
        )
        .unwrap();

    let dxf = cadcraft_io::write_dxf(&d);
    let back_dxf = cadcraft_io::read_dxf(dxf.as_bytes()).unwrap();
    let e_dxf = back_dxf.entity(h).unwrap();
    if let EntityKind::MText(m) = &e_dxf.kind {
        assert_eq!(m.background, Some(Color::True(Rgb(250, 240, 230))));
        assert_eq!(m.background_scale, Some(1.25));
    } else {
        panic!("expected MText");
    }

    let back_dwg = through_dwg(&d);
    let e_dwg = back_dwg.entity(h).unwrap();
    if let EntityKind::MText(m) = &e_dwg.kind {
        assert_eq!(m.background, Some(Color::True(Rgb(250, 240, 230))));
        assert_eq!(m.background_scale, Some(1.25));
    } else {
        panic!("expected MText");
    }
}

#[test]
fn rename_foreign_record_updates_keys_and_targets() {
    let mut d = Drawing::new_metric();
    d.rename_foreign_record("LAYER", "WALLS", "PARTITIONS");
    assert!(d.foreign.records.is_empty());

    let mut extra = cadcraft_doc::foreign::ForeignExtra::default();
    extra.xdata = vec![cadcraft_doc::RawTag { code: 1000, value: "app_data".into() }];
    std::sync::Arc::make_mut(&mut d.foreign).records.insert(("LAYER".into(), "WALLS".into()), extra);
    std::sync::Arc::make_mut(&mut d.foreign).targets.insert(Handle(0x100), ("LAYER".into(), "WALLS".into()));

    d.rename_foreign_record("LAYER", "WALLS", "PARTITIONS");
    assert!(!d.foreign.records.contains_key(&("LAYER".into(), "WALLS".into())));
    assert!(d.foreign.records.contains_key(&("LAYER".into(), "PARTITIONS".into())));
    assert_eq!(d.foreign.targets.get(&Handle(0x100)), Some(&("LAYER".into(), "PARTITIONS".into())));
}

#[test]
fn unknown_entity_common_properties_follow_edits_on_dxf_write() {
    let mut d = Drawing::new_metric();
    let _ = d
        .add(
            &Space::Model,
            Common {
                layer: "0".into(),
                color: Color::True(Rgb(10, 20, 30)),
                lineweight: Lineweight::Mm100(50),
                visible: false,
                transparency: Transparency::Percent(50),
                ..Common::default()
            },
            EntityKind::Unknown(Unknown {
                dxf_type: "CUSTOM_SOLVER".into(),
                tags: vec![
                    cadcraft_doc::RawTag { code: 100, value: "AcDbEntity".into() },
                    cadcraft_doc::RawTag { code: 8, value: "OLD_LAYER".into() },
                    cadcraft_doc::RawTag { code: 62, value: "1".into() },
                    cadcraft_doc::RawTag { code: 100, value: "AcDbCustomSolver".into() },
                    cadcraft_doc::RawTag { code: 1, value: "custom_data".into() },
                ],
            }),
        )
        .unwrap();

    let dxf = cadcraft_io::write_dxf(&d);
    assert!(dxf.contains("420\r\n660510") || dxf.contains("420\n660510")); // 10*65536 + 20*256 + 30
    assert!(dxf.contains("370\r\n50") || dxf.contains("370\n50"));
    assert!(dxf.contains(" 60\r\n1") || dxf.contains("60\n1"));
    assert!(dxf.contains("440\r\n") || dxf.contains("440\n"));
}
