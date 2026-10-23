//! BREAKLINE's Block option takes a drawing file (as Express Tools' `brkline.dwg`): the file is
//! defined as a block named after it, as INSERT does, and its symbol is used. An unusable file
//! changes nothing.

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::geom::{PolyVertex, Vec2, Vec3};
use cadcraft_engine::doc::{Common, Drawing, EntityKind, LwPolyline, Point, Space};
use serde_json::json;

fn hooks() {
    set_io(IoHooks {
        read: |b, _| cadcraft_io::read_dxf(b).map_err(|e| e.to_string()),
        write: |d, _| Ok(cadcraft_io::write_dxf(d).into_bytes()),
        plot: None,
    });
}

/// A DXF file whose model space is a break symbol: Defpoints at (0,0) and (4,0), and a polyline
/// between them through (2,1) (a tent). `defpoints` false leaves the anchors off Defpoints.
fn symbol_file(name: &str, defpoints: bool) -> String {
    let mut d = Drawing::new_metric();
    let layer = if defpoints { "Defpoints" } else { "0" };
    d.ensure_layer(layer);
    for x in [0.0, 4.0] {
        let c = Common { layer: layer.into(), ..Common::default() };
        d.add(&Space::Model, c, EntityKind::Point(Point { p: Vec3::new(x, 0.0, 0.0), angle: 0.0 })).unwrap();
    }
    let vs = [(0.0, 0.0), (2.0, 1.0), (4.0, 0.0)].iter().map(|&(x, y)| PolyVertex::new(Vec2::new(x, y))).collect();
    let pl = LwPolyline { vertices: vs, closed: false, const_width: 0.0, elevation: 0.0, plinegen: false };
    d.add(&Space::Model, Common::default(), EntityKind::LwPolyline(pl)).unwrap();
    let dir = std::env::temp_dir().join(format!("cadcraft-breakline-file-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, cadcraft_io::write_dxf(&d)).unwrap();
    path.to_string_lossy().into_owned()
}

fn last_points(s: &Session) -> Vec<(f64, f64)> {
    let d = s.doc().unwrap();
    match &d.model.iter().last().unwrap().kind {
        EntityKind::LwPolyline(p) => p.vertices.iter().map(|v| ((v.p.x * 1e9).round() / 1e9, (v.p.y * 1e9).round() / 1e9)).collect(),
        k => panic!("not a polyline: {k:?}"),
    }
}

#[test]
fn breakline_block_from_a_file() {
    hooks();
    let good = symbol_file("tent.dxf", true);
    let bad = symbol_file("loose.dxf", false);

    // JSON: the file becomes block "tent"; the symbol (4 wide, scaled by size 0.5) sits mid-line.
    let mut s = Session::new();
    s.execute("breakline", &json!({"p1": [0, 0], "p2": [10, 0], "size": 0.5, "block": good})).unwrap();
    assert!(s.doc().unwrap().block("tent").is_some(), "the file was defined as a block");
    assert_eq!(last_points(&s), vec![(0.0, 0.0), (4.0, 0.0), (5.0, 0.5), (6.0, 0.0), (10.0, 0.0)]);
    // The block can then be named directly.
    s.execute("breakline", &json!({"p1": [0, 0], "p2": [10, 0], "size": 0.5, "block": "tent"})).unwrap();
    assert_eq!(last_points(&s), vec![(0.0, 0.0), (4.0, 0.0), (5.0, 0.5), (6.0, 0.0), (10.0, 0.0)]);

    // A file without Defpoints anchors, or a missing file, is refused and leaves no block behind.
    let mut s = Session::new();
    let blocks = s.doc().unwrap().blocks.len();
    assert!(s.execute("breakline", &json!({"p1": [0, 0], "p2": [10, 0], "block": bad})).is_err());
    assert!(s.execute("breakline", &json!({"p1": [0, 0], "p2": [10, 0], "block": "/no/such/brkline.dwg"})).is_err());
    // A good file with a line too short for the symbol also leaves the drawing as it was.
    assert!(s.execute("breakline", &json!({"p1": [0, 0], "p2": [1, 0], "block": good})).is_err());
    assert_eq!(s.doc().unwrap().blocks.len(), blocks);
    assert_eq!(s.doc().unwrap().model.len(), 0);

    // Typed: Block, the file path, then the points; the block is remembered as "tent".
    for t in ["breakline", "b"] {
        s.cmdline(t).unwrap();
    }
    s.cmdline(&good).unwrap();
    for t in ["s", "0.5", "0,0", "10,0", ""] {
        s.cmdline(t).unwrap();
    }
    assert!(s.current_prompt().is_none(), "{}", s.prompt_text());
    assert_eq!(last_points(&s), vec![(0.0, 0.0), (4.0, 0.0), (5.0, 0.5), (6.0, 0.0), (10.0, 0.0)]);
    assert_eq!(s.last_used.breakline.block, "tent");
}
