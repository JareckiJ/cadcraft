//! Block names are not case-sensitive (`Drawing::block`), so an object that names its block in
//! another case (`*d1` for `*D1`) must follow the block when a copy between drawings renames it:
//! the clipboard, IMPORT and INSERT from a file. Before, the rename was looked up by exact name,
//! and such an object kept pointing at the destination's own `*D1`.

use std::path::PathBuf;
use std::sync::Arc;

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::{Block, Common, Drawing, Entity, EntityKind, Handle, Insert, Line, RawTag, Space, Unknown};
use cadcraft_engine::geom::Vec3;
use serde_json::json;

fn tag(code: i32, value: &str) -> RawTag {
    RawTag { code, value: value.into() }
}

/// An ARC_DIMENSION kept raw, naming `block` in group 2.
fn arc_dim(block: &str) -> EntityKind {
    EntityKind::Unknown(Unknown {
        dxf_type: "ARC_DIMENSION".into(),
        tags: vec![
            tag(100, "AcDbEntity"),
            tag(8, "0"),
            tag(100, "AcDbDimension"),
            tag(2, block),
            tag(10, "1"),
            tag(20, "2"),
            tag(100, "AcDbArcDimension"),
        ],
    })
}

fn insert(block: &str) -> EntityKind {
    EntityKind::Insert(Insert {
        block: block.into(),
        insert: Vec3::ZERO,
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    })
}

/// A block holding one line starting at x.
fn block(name: &str, x: f64) -> Arc<Block> {
    let mut b = Block::new(name);
    b.anonymous = name.starts_with('*');
    b.entities.push(Entity::new(Handle(0x8000), EntityKind::Line(Line { a: Vec3::new(x, 0.0, 0.0), b: Vec3::new(x + 1.0, 1.0, 0.0) })));
    Arc::new(b)
}

/// The first x of the line in the block `name` resolves to in `d`.
fn first_x(d: &Drawing, name: &str) -> Option<f64> {
    match d.block(name)?.entities.iter().next().map(|e| &e.kind) {
        Some(EntityKind::Line(l)) => Some(l.a.x),
        _ => None,
    }
}

/// The block each object of model space names (an unknown dimension's group 2, a reference's
/// block, a dimension's block), with its kind.
fn names(d: &Drawing) -> Vec<(&'static str, String)> {
    d.model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Unknown(u) => u.block_names().next().map(|n| ("unknown", n.to_string())),
            EntityKind::Insert(i) => Some(("insert", i.block.clone())),
            EntityKind::Dimension(dm) => dm.block.clone().map(|b| ("dimension", b)),
            _ => None,
        })
        .collect()
}

/// Every object of `d`'s model space names the copied block (first x 1), none the destination's
/// own `*D1` (first x 5), and that one is unchanged.
fn assert_all_follow(d: &Drawing, expected: usize) {
    let named = names(d);
    assert_eq!(named.len(), expected, "{named:?} / {:?}", d.model.iter().map(|e| e.kind.type_name()).collect::<Vec<_>>());
    for (kind, n) in &named {
        assert_eq!(first_x(d, n), Some(1.0), "the {kind} names {n}, which is not the copied *D1 ({named:?})");
    }
    assert_eq!(first_x(d, "*D1"), Some(5.0), "the destination's own *D1 changed");
}

/// The source drawing: `*D1` (line at x 1) and three objects naming it as `*d1`: an unknown
/// dimension, a block reference and a dimension.
fn source() -> Drawing {
    let mut s = Session::new();
    s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [1, 0], "at": [0, 2] })).unwrap();
    let mut d = s.doc().unwrap().clone();
    for h in d.model.handles() {
        d.model.modify(h, |e| {
            if let EntityKind::Dimension(dm) = &mut e.kind {
                dm.block = Some("*d1".into());
            }
        });
    }
    d.blocks.retain(|k, _| !k.starts_with("*D") && !k.starts_with("*d"));
    d.blocks.insert("*D1".into(), block("*D1", 1.0));
    d.add(&Space::Model, Common::default(), arc_dim("*d1")).unwrap();
    d.add(&Space::Model, Common::default(), insert("*d1")).unwrap();
    d
}

/// A destination with a different `*D1` of its own (line at x 5).
fn destination() -> Drawing {
    let mut d = Drawing::new_metric();
    d.blocks.insert("*D1".into(), block("*D1", 5.0));
    d
}

#[test]
fn the_source_names_its_block_in_another_case() {
    let d = source();
    let named = names(&d);
    assert_eq!(named.len(), 3, "{named:?}");
    assert!(named.iter().all(|(_, n)| n == "*d1" && first_x(&d, n) == Some(1.0)), "{named:?}");
}

#[test]
fn clipboard_follows_a_rename_named_in_another_case() {
    let mut s = Session::empty();
    let a = s.open_drawing(source(), "A", None);
    let b = s.open_drawing(destination(), "B", None);
    s.active = a;
    let hs: Vec<String> = s.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect();
    s.execute("copyclip", &json!({ "handles": hs })).unwrap();
    s.active = b;
    s.execute("pasteclip", &json!({ "at": [0, 0] })).unwrap();
    // The pasted dimension is moved to the paste point and drops its block (it is drawn anew), so
    // the unknown dimension and the block reference are the ones naming a block.
    assert_all_follow(s.doc().unwrap(), 2);
}

fn read(_bytes: &[u8], _name: &str) -> Result<Drawing, String> {
    Ok(source())
}

fn file(name: &str) -> String {
    let dir: PathBuf = std::env::temp_dir().join(format!("cadcraft-block-case-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, "x").unwrap();
    p.to_string_lossy().into_owned()
}

#[test]
fn import_follows_a_rename_named_in_another_case() {
    set_io(IoHooks { read, write: |_, _| Err("no writer in this test".into()), plot: None });
    let mut s = Session::empty();
    s.open_drawing(destination(), "B", None);
    s.execute("import", &json!({ "path": file("import.dxf") })).unwrap();
    assert_all_follow(s.doc().unwrap(), 3);
}

#[test]
fn insert_from_a_file_follows_a_rename_named_in_another_case() {
    set_io(IoHooks { read, write: |_, _| Err("no writer in this test".into()), plot: None });
    let mut s = Session::empty();
    s.open_drawing(destination(), "B", None);
    s.execute("insert", &json!({ "name": "SRC", "file": file("src.dxf"), "at": [0, 0] })).unwrap();
    let d = s.doc().unwrap();
    // The file becomes block SRC: its objects (now inside SRC) must name the copied block.
    let src = d.block("SRC").unwrap_or_else(|| panic!("no block SRC"));
    let mut inner = Drawing::new_metric();
    inner.blocks = d.blocks.clone();
    for e in src.entities.iter() {
        inner.model.push((**e).clone());
    }
    assert_all_follow(&inner, 3);
}
