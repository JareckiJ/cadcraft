use std::sync::Arc;

use cadcraft_doc::*;
use cadcraft_geom::{Vec2, Vec3};
use cadcraft_io::{read_dxf, write_dxf};

fn roundtrip(d: &Drawing) -> Drawing {
    read_dxf(write_dxf(d).as_bytes()).expect("reopen")
}

#[test]
fn xclip_boundary_survives_dxf_roundtrip() {
    let mut d = Drawing::new_metric();
    let mut blk = Block::new("DETAIL");
    blk.entities.push(Entity::new(Handle(0x10), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(10.0, 10.0, 0.0) })));
    d.blocks.insert("DETAIL".into(), Arc::new(blk));

    let clip = ClipBoundary {
        points: vec![Vec2::new(1.0, 1.0), Vec2::new(5.0, 1.0), Vec2::new(5.0, 5.0), Vec2::new(1.0, 5.0)],
        enabled: true,
        inverted: false,
    };

    let ins = Insert {
        block: "DETAIL".into(),
        insert: Vec3::new(10.0, 20.0, 0.0),
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
        clip: Some(clip),
    };

    let h = d.add(&Space::Model, Common::default(), EntityKind::Insert(ins)).expect("add insert");

    let back = roundtrip(&d);
    let entity = back.model.get(h).expect("insert present");
    let EntityKind::Insert(back_ins) = &entity.kind else { panic!("expected Insert") };

    let back_clip = back_ins.clip.as_ref().expect("clip boundary preserved");
    assert!(back_clip.enabled);
    assert!(!back_clip.inverted);
    assert_eq!(back_clip.points.len(), 4);
    assert!((back_clip.points[0].x - 1.0).abs() < 1e-4);
    assert!((back_clip.points[0].y - 1.0).abs() < 1e-4);
    assert!((back_clip.points[2].x - 5.0).abs() < 1e-4);
    assert!((back_clip.points[2].y - 5.0).abs() < 1e-4);
}

#[test]
fn xclip_inverted_and_disabled_roundtrip() {
    let mut d = Drawing::new_metric();
    let mut blk = Block::new("PART");
    blk.entities.push(Entity::new(Handle(0x20), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(2.0, 2.0, 0.0) })));
    d.blocks.insert("PART".into(), Arc::new(blk));

    let clip = ClipBoundary { points: vec![Vec2::new(0.0, 0.0), Vec2::new(4.0, 0.0), Vec2::new(2.0, 3.0)], enabled: false, inverted: true };

    let ins = Insert {
        block: "PART".into(),
        insert: Vec3::ZERO,
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
        clip: Some(clip),
    };

    let h = d.add(&Space::Model, Common::default(), EntityKind::Insert(ins)).expect("add insert");

    let back = roundtrip(&d);
    let entity = back.model.get(h).expect("insert present");
    let EntityKind::Insert(back_ins) = &entity.kind else { panic!("expected Insert") };

    let back_clip = back_ins.clip.as_ref().expect("clip boundary preserved");
    assert!(!back_clip.enabled);
    assert!(back_clip.inverted);
    assert_eq!(back_clip.points.len(), 3);
}
