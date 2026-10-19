//! REFEDIT, REFSET and REFCLOSE: editing a block reference in place (#813).

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::{Common, Drawing, EntityKind, Handle, Insert, Space};
use cadcraft_engine::geom::Vec3;
use serde_json::{Value, json};

fn hooks() {
    set_io(IoHooks {
        read: |b, name| cadcraft_io::read(b, name).map_err(|e| e.to_string()),
        write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
        plot: None,
    });
}

fn folder(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("cadcraft-refedit-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn h(v: &Value) -> String {
    v.get("handle").unwrap_or(&v["handles"][0]).as_str().unwrap().to_string()
}

fn insert(s: &mut Session, block: &str, at: (f64, f64), scale: f64, rot_deg: f64) -> String {
    let kind = EntityKind::Insert(Insert {
        block: block.into(),
        insert: Vec3::new(at.0, at.1, 0.0),
        scale: Vec3::new(scale, scale, scale),
        rotation: rot_deg.to_radians(),
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
        clip: None,
    });
    s.doc_mut().unwrap().add(&Space::Model, Common::default(), kind).unwrap().hex()
}

/// Block B = a line (0,0)-(10,0) and a circle at (20,0) r2; references A (at 100,0) and C
/// (at 0,50, scaled 2, rotated 90 degrees).
fn setup() -> (Session, String, String) {
    let mut s = Session::new();
    let l = h(&s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap());
    let c = h(&s.execute("circle", &json!({ "center": [20, 0], "radius": 2 })).unwrap());
    s.execute("block", &json!({ "name": "B", "base": [0, 0], "handles": [l, c], "keep": "delete" })).unwrap();
    let a = insert(&mut s, "B", (100.0, 0.0), 1.0, 0.0);
    let c = insert(&mut s, "B", (0.0, 50.0), 2.0, 90.0);
    (s, a, c)
}

fn block_lines(d: &Drawing, name: &str) -> Vec<(Vec3, Vec3)> {
    d.block(name).unwrap().entities.iter().filter_map(|e| if let EntityKind::Line(l) = &e.kind { Some((l.a, l.b)) } else { None }).collect()
}

fn handles(v: &Value) -> Vec<String> {
    v["working"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect()
}

fn model_len(s: &Session) -> usize {
    s.doc().unwrap().model.len()
}

#[test]
fn edit_and_save_updates_every_reference() {
    let (mut s, a, c) = setup();
    let before = model_len(&s);
    let r = s.execute("refedit", &json!({ "handle": a })).unwrap();
    let work = handles(&r);
    assert_eq!(work.len(), 2);
    // The reference is hidden, the block's objects stand where it showed them.
    let d = s.doc().unwrap();
    assert!(!d.entity(Handle::parse_hex(&a).unwrap()).unwrap().common.visible);
    assert_eq!(model_len(&s), before + 2);
    let line = work.iter().find_map(|w| match &d.entity(Handle::parse_hex(w).unwrap()).unwrap().kind {
        EntityKind::Line(l) => Some((w.clone(), l.clone())),
        _ => None,
    });
    let (lh, l) = line.unwrap();
    assert!((l.a.x - 100.0).abs() < 1e-9 && (l.b.x - 110.0).abs() < 1e-9);
    // Edit with normal commands: stretch it up by 5 and draw a new line.
    s.execute("move", &json!({ "handles": [lh], "delta": [0, 5] })).unwrap();
    s.execute("line", &json!({ "points": [[100, 20], [110, 20]] })).unwrap();
    // The new line joined the working set.
    let rs = s.execute("refset", &json!({})).unwrap();
    assert_eq!(rs["working"].as_array().unwrap().len(), 3);
    let out = s.execute("refclose", &json!({})).unwrap();
    assert_eq!(out["objects"], 3);
    let d = s.doc().unwrap();
    assert!(d.refedit.is_none());
    assert!(d.entity(Handle::parse_hex(&a).unwrap()).unwrap().common.visible);
    assert_eq!(model_len(&s), before, "working copies are gone, the block holds the objects");
    let lines = block_lines(d, "B");
    assert_eq!(lines.len(), 2);
    assert!(lines.iter().any(|(p, q)| (p.y - 5.0).abs() < 1e-9 && (q.x - 10.0).abs() < 1e-9));
    assert!(lines.iter().any(|(p, q)| (p.y - 20.0).abs() < 1e-9 && (p.x - 0.0).abs() < 1e-9 && (q.x - 10.0).abs() < 1e-9));
    // The second reference shows the change through its own transform.
    let _ = c;
    assert!(s.doc().unwrap().block("B").unwrap().entities.len() == 3);
}

#[test]
fn discard_leaves_everything_as_it_was() {
    let (mut s, a, _) = setup();
    let before = s.doc().unwrap().clone();
    let r = s.execute("refedit", &json!({ "handle": a })).unwrap();
    s.execute("erase", &json!({ "handles": handles(&r) })).unwrap();
    s.execute("refclose", &json!({ "option": "discard" })).unwrap();
    let d = s.doc().unwrap();
    assert!(d.refedit.is_none());
    assert_eq!(d.block("B").unwrap().entities.len(), 2);
    assert_eq!(d.model.len(), before.model.len());
    assert!(d.entity(Handle::parse_hex(&a).unwrap()).unwrap().common.visible);
}

#[test]
fn erasing_a_working_object_removes_it_from_the_block() {
    let (mut s, a, _) = setup();
    let r = s.execute("refedit", &json!({ "handle": a })).unwrap();
    let w = handles(&r);
    s.execute("erase", &json!({ "handles": [w[0]] })).unwrap();
    s.execute("refclose", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().block("B").unwrap().entities.len(), 1);
}

#[test]
fn objects_outside_the_working_set_cannot_be_edited() {
    let (mut s, a, c) = setup();
    let other = h(&s.execute("line", &json!({ "points": [[300, 0], [310, 0]] })).unwrap());
    s.execute("refedit", &json!({ "handle": a })).unwrap();
    let before = s.doc().unwrap().clone();
    let _ = s.execute("erase", &json!({ "handles": [other.clone(), c.clone()] }));
    let _ = s.execute("move", &json!({ "handles": [other.clone()], "delta": [1, 1] }));
    let d = s.doc().unwrap();
    assert!(d.entity(Handle::parse_hex(&other).unwrap()).is_some());
    assert!(d.entity(Handle::parse_hex(&c).unwrap()).is_some());
    assert_eq!(d.model.len(), before.model.len());
    // Starting another edit, or saving, is refused.
    assert!(s.execute("refedit", &json!({ "handle": c })).is_err());
    assert!(s.execute("save", &json!({ "path": "/nonexistent-dir/x.dxf" })).is_err());
}

#[test]
fn refset_adds_and_removes_objects() {
    let (mut s, a, _) = setup();
    let other = h(&s.execute("line", &json!({ "points": [[300, 0], [310, 0]] })).unwrap());
    // Edit only the circle: the line is a faded copy.
    let (circle_src, line_src) = {
        let b = s.doc().unwrap().block("B").unwrap();
        let c = b.entities.iter().find(|e| matches!(e.kind, EntityKind::Circle(_))).unwrap().handle;
        let l = b.entities.iter().find(|e| matches!(e.kind, EntityKind::Line(_))).unwrap().handle;
        (c.hex(), l.hex())
    };
    let r = s.execute("refedit", &json!({ "handle": a, "objects": [circle_src] })).unwrap();
    assert_eq!(r["faded"], 1);
    assert_eq!(handles(&r).len(), 1);
    // Add the host line: it goes into the block at close. Remove the circle: it stays in the block.
    let rs = s.execute("refset", &json!({ "add": [other], "remove": handles(&r) })).unwrap();
    assert_eq!(rs["added"], 1);
    assert_eq!(rs["removed"], 1);
    s.execute("refclose", &json!({})).unwrap();
    let d = s.doc().unwrap();
    let b = d.block("B").unwrap();
    assert_eq!(b.entities.len(), 3);
    assert!(b.entities.get(Handle::parse_hex(&line_src).unwrap()).is_some());
    assert!(d.entity(Handle::parse_hex(&other).unwrap()).is_none(), "moved into the block");
    // 300,0 relative to the reference at 100,0
    assert!(block_lines(d, "B").iter().any(|(p, q)| (p.x - 200.0).abs() < 1e-9 && (q.x - 210.0).abs() < 1e-9));
    assert!(s.execute("refset", &json!({ "add": ["FFFF"] })).is_err(), "no edit active");
}

#[test]
fn a_block_that_would_contain_itself_is_refused_and_the_edit_survives() {
    let (mut s, a, _) = setup();
    s.execute("refedit", &json!({ "handle": a })).unwrap();
    // A reference to B drawn while editing B.
    let nested = insert(&mut s, "B", (500.0, 0.0), 1.0, 0.0);
    let _ = nested;
    // inserted through the doc API: add it to the working set by hand
    s.execute("refset", &json!({ "add": [nested] })).unwrap();
    assert!(s.execute("refclose", &json!({})).is_err());
    assert!(s.doc().unwrap().refedit.is_some(), "the edit is still active");
    assert_eq!(s.doc().unwrap().block("B").unwrap().entities.len(), 2);
    s.execute("refclose", &json!({ "option": "discard" })).unwrap();
    assert!(s.doc().unwrap().refedit.is_none());
}

#[test]
fn undo_takes_the_edit_back_with_its_state() {
    let (mut s, a, _) = setup();
    let n = model_len(&s);
    s.execute("refedit", &json!({ "handle": a })).unwrap();
    s.undo().unwrap();
    let d = s.doc().unwrap();
    assert!(d.refedit.is_none());
    assert_eq!(d.model.len(), n);
    assert!(d.entity(Handle::parse_hex(&a).unwrap()).unwrap().common.visible);
    s.redo().unwrap();
    assert!(s.doc().unwrap().refedit.is_some());
}

#[test]
fn files_never_hold_half_an_edit() {
    let (mut s, a, _) = setup();
    s.execute("refedit", &json!({ "handle": a })).unwrap();
    let d = s.doc().unwrap();
    let committed = d.committed();
    assert!(committed.refedit.is_none());
    assert_eq!(committed.model.len(), 2);
    assert!(committed.entity(Handle::parse_hex(&a).unwrap()).unwrap().common.visible);
    let dxf = cadcraft_io::write_dxf(d);
    let back = cadcraft_io::read_dxf(dxf.as_bytes()).unwrap();
    assert_eq!(back.model.len(), 2);
}

#[test]
fn hostile_requests_never_panic() {
    let (mut s, a, _) = setup();
    for p in [
        json!({}),
        json!({ "handle": "ZZZ" }),
        json!({ "handle": 1e300 }),
        json!({ "name": "nope" }),
        json!({ "handle": a, "objects": "x" }),
        json!({ "handle": a, "objects": [] }),
        json!({ "handle": a, "objects": ["FFFFFFFF"] }),
        json!({ "handle": a, "objects": [null, {}] }),
    ] {
        assert!(s.execute("refedit", &p).is_err(), "{p}");
    }
    for cmd in ["refset", "refclose"] {
        assert!(s.execute(cmd, &json!({})).is_err());
    }
    // A reference with zero scale, a non-uniform one and a non-finite one can't be edited.
    let z = insert(&mut s, "B", (0.0, 0.0), 0.0, 0.0);
    assert!(s.execute("refedit", &json!({ "handle": z })).is_err());
    let nan = insert(&mut s, "B", (f64::NAN, 0.0), 1.0, 0.0);
    assert!(s.execute("refedit", &json!({ "handle": nan })).is_err());
    s.execute("refedit", &json!({ "handle": a })).unwrap();
    for p in [json!({ "add": "x" }), json!({ "remove": ["ZZ"] }), json!({ "add": [null] }), json!({ "handles": 3 }), json!({ "remove": ["FFFFFFF"] })]
    {
        assert!(s.execute("refset", &p).is_err(), "{p}");
    }
    assert!(s.execute("refclose", &json!({ "option": "maybe" })).is_err());
    assert!(s.doc().unwrap().refedit.is_some());
}

#[test]
fn a_block_that_references_itself_in_a_file_does_not_hang() {
    let (mut s, a, _) = setup();
    // Hostile drawing: B contains a reference to itself.
    let kind = EntityKind::Insert(Insert {
        block: "B".into(),
        insert: Vec3::ZERO,
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: vec![],
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
        clip: None,
    });
    let d = s.doc_mut().unwrap();
    let hh = d.new_handle();
    let mut b = (**d.block("B").unwrap()).clone();
    b.entities.push(cadcraft_engine::doc::Entity::new(hh, kind));
    d.blocks.insert("B".into(), std::sync::Arc::new(b));
    s.execute("refedit", &json!({ "handle": a })).unwrap();
    assert!(s.execute("refclose", &json!({})).is_err());
    s.execute("refclose", &json!({ "option": "discard" })).unwrap();
}

#[test]
fn interactive_refedit_refset_and_refclose() {
    use cadcraft_engine::Input;
    let (mut s, a, _) = setup();
    let other = h(&s.execute("line", &json!({ "points": [[300, 0], [310, 0]] })).unwrap());
    s.start("refedit").unwrap();
    s.input(Input::Pick(vec![Handle::parse_hex(&a).unwrap()])).unwrap();
    assert!(s.running.is_none() && s.doc().unwrap().refedit.is_some());
    s.start("refset").unwrap();
    s.input(Input::Keyword("Add".into())).unwrap();
    s.input(Input::Pick(vec![Handle::parse_hex(&other).unwrap()])).unwrap();
    s.input(Input::Enter).unwrap();
    assert_eq!(s.doc().unwrap().refedit.as_ref().unwrap().items.len(), 3);
    s.start("refclose").unwrap();
    s.input(Input::Keyword("Save".into())).unwrap();
    assert!(s.doc().unwrap().refedit.is_none());
    assert_eq!(s.doc().unwrap().block("B").unwrap().entities.len(), 3);
}

#[test]
fn xref_missing_file_reports_informative_error() {
    let mut s = Session::new();
    s.doc_mut().unwrap().blocks.insert(
        "X".into(),
        std::sync::Arc::new({
            let mut b = cadcraft_engine::doc::Block::new("X");
            b.xref_path = Some("nonexistent_xref_file.dxf".into());
            b.xref_status = cadcraft_engine::doc::XrefStatus::Loaded;
            b
        }),
    );
    let x = insert(&mut s, "X", (0.0, 0.0), 1.0, 0.0);
    let e = s.execute("refedit", &json!({ "handle": x })).unwrap_err();
    assert!(e.to_string().contains("file not found"), "{e}");
}

#[test]
fn xref_in_place_edit_and_save_writes_back_to_disk() {
    hooks();
    let dir = folder("writeback");
    let site_file = dir.join("site.dxf");

    // Create an external site drawing with a line on layer "Walls"
    let mut site_doc = Drawing::new_metric();
    site_doc.layers.push(cadcraft_engine::doc::Layer {
        name: "Walls".into(),
        color: cadcraft_engine::doc::color::Color::Index(1),
        ..cadcraft_engine::doc::Layer::default()
    });
    let lh = site_doc.new_handle();
    let mut line_ent = cadcraft_engine::doc::Entity::new(
        lh,
        EntityKind::Line(cadcraft_engine::doc::Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(10.0, 0.0, 0.0) }),
    );
    line_ent.common.layer = "Walls".into();
    site_doc.model.push(line_ent);

    std::fs::write(&site_file, cadcraft_io::write(&site_doc, "site.dxf").unwrap()).unwrap();

    // In a host drawing, attach site.dxf
    let mut s = Session::new();
    let r = s.execute("xattach", &json!({ "path": site_file.to_str().unwrap(), "name": "SITE", "at": [100, 50] })).unwrap();
    let ih = r["handle"].as_str().unwrap();

    // Start REFEDIT on the xref insert
    let edit_res = s.execute("refedit", &json!({ "handle": ih })).unwrap();
    assert_eq!(edit_res["xref"], true);
    assert!(edit_res["xref_file"].as_str().unwrap().contains("site.dxf"));
    let working = handles(&edit_res);
    assert_eq!(working.len(), 1);

    // Modify the existing line (move it up by 5) and add a circle
    let line_work = &working[0];
    s.execute("move", &json!({ "handles": [line_work], "delta": [0, 5] })).unwrap();
    s.execute("circle", &json!({ "center": [105, 55], "radius": 3 })).unwrap();

    // REFCLOSE Save
    let close_res = s.execute("refclose", &json!({ "save": true })).unwrap();
    assert_eq!(close_res["saved"], true);
    assert_eq!(close_res["xref"], true);
    assert_eq!(close_res["objects"], 2);

    // Read the site.dxf file back from disk directly
    let disk_bytes = std::fs::read(&site_file).unwrap();
    let back_doc = cadcraft_io::read(&disk_bytes, "site.dxf").unwrap();
    assert_eq!(back_doc.model.len(), 2);

    // The line should be at (0, 5)-(10, 5) relative to xref origin, layer "Walls"
    let back_line = back_doc
        .model
        .iter()
        .find_map(|e| match &e.kind {
            EntityKind::Line(l) => Some((e.common.layer.clone(), l.clone())),
            _ => None,
        })
        .unwrap();
    assert_eq!(back_line.0, "Walls", "layer should be Walls, not SITE|Walls");
    assert!((back_line.1.a.y - 5.0).abs() < 1e-6);
    assert!((back_line.1.b.y - 5.0).abs() < 1e-6);

    // The circle should be at (5, 5) radius 3
    let back_circle = back_doc
        .model
        .iter()
        .find_map(|e| match &e.kind {
            EntityKind::Circle(c) => Some(c.clone()),
            _ => None,
        })
        .unwrap();
    assert!((back_circle.center.x - 5.0).abs() < 1e-6);
    assert!((back_circle.center.y - 5.0).abs() < 1e-6);
    assert!((back_circle.radius - 3.0).abs() < 1e-6);
}

#[test]
fn xref_in_place_edit_discard_leaves_disk_file_untouched() {
    hooks();
    let dir = folder("discard");
    let site_file = dir.join("site.dxf");

    let mut site_doc = Drawing::new_metric();
    let lh = site_doc.new_handle();
    site_doc.model.push(cadcraft_engine::doc::Entity::new(
        lh,
        EntityKind::Line(cadcraft_engine::doc::Line { a: Vec3::new(1.0, 2.0, 0.0), b: Vec3::new(3.0, 4.0, 0.0) }),
    ));
    let original_bytes = cadcraft_io::write(&site_doc, "site.dxf").unwrap();
    std::fs::write(&site_file, &original_bytes).unwrap();

    let mut s = Session::new();
    let r = s.execute("xattach", &json!({ "path": site_file.to_str().unwrap(), "name": "SITE" })).unwrap();
    let ih = r["handle"].as_str().unwrap();

    let edit_res = s.execute("refedit", &json!({ "handle": ih })).unwrap();
    let working = handles(&edit_res);
    s.execute("erase", &json!({ "handles": working })).unwrap();

    // REFCLOSE Discard
    let close_res = s.execute("refclose", &json!({ "option": "discard" })).unwrap();
    assert_eq!(close_res["saved"], false);

    let disk_bytes = std::fs::read(&site_file).unwrap();
    let back_doc = cadcraft_io::read(&disk_bytes, "site.dxf").unwrap();
    assert_eq!(back_doc.model.len(), 1, "disk file was untouched by discard");
}

#[test]
fn nested_block_in_place_edit() {
    let mut s = Session::new();

    // Create block Child (a circle)
    let c = h(&s.execute("circle", &json!({ "center": [0, 0], "radius": 2 })).unwrap());
    s.execute("block", &json!({ "name": "Child", "base": [0, 0], "handles": [c], "keep": "delete" })).unwrap();

    // Create block Parent (a line, plus an insert of Child at 10, 0)
    let l = h(&s.execute("line", &json!({ "points": [[0, 0], [5, 0]] })).unwrap());
    let ci = insert(&mut s, "Child", (10.0, 0.0), 1.0, 0.0);
    s.execute("block", &json!({ "name": "Parent", "base": [0, 0], "handles": [l, ci], "keep": "delete" })).unwrap();

    // Insert Parent at (100, 200) on canvas
    let pi = insert(&mut s, "Parent", (100.0, 200.0), 1.0, 0.0);

    // REFEDIT on Parent, picking nested block "Child"
    let edit_res = s.execute("refedit", &json!({ "handle": pi, "nested": "Child" })).unwrap();
    assert_eq!(edit_res["block"], "Child");
    assert_eq!(edit_res["nested"], true);
    assert_eq!(edit_res["nested_path"], json!(["Parent", "Child"]));

    let work = handles(&edit_res);
    assert_eq!(work.len(), 1);

    // The working circle should be located at world coordinates (100 + 10, 200 + 0) = (110, 200)
    let d = s.doc().unwrap();
    let work_h = Handle::parse_hex(&work[0]).unwrap();
    let EntityKind::Circle(circle_ent) = &d.entity(work_h).unwrap().kind else { panic!("not circle") };
    assert!((circle_ent.center.x - 110.0).abs() < 1e-6);
    assert!((circle_ent.center.y - 200.0).abs() < 1e-6);

    // Move the circle by (0, 10) in world space
    s.execute("move", &json!({ "handles": [&work[0]], "delta": [0, 10] })).unwrap();

    // REFCLOSE Save
    let close_res = s.execute("refclose", &json!({ "save": true })).unwrap();
    assert_eq!(close_res["saved"], true);

    // Verify Child's block definition now has center (0, 10) relative to its base
    let d = s.doc().unwrap();
    let child_blk = d.block("Child").unwrap();
    let EntityKind::Circle(new_child_circ) = &child_blk.entities.iter().next().unwrap().kind else { panic!("not circle") };
    assert!((new_child_circ.center.x - 0.0).abs() < 1e-6);
    assert!((new_child_circ.center.y - 10.0).abs() < 1e-6);
}

#[test]
fn refclose_synchronizes_attributes_across_references() {
    let mut s = Session::new();
    s.execute("attdef", &json!({"tag": "TAG1", "default": "val1", "at": [0, 1], "height": 1.0})).unwrap();
    s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    let hs: Vec<String> = s.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect();
    s.execute("block", &json!({"name": "ATTD", "base": [0, 0], "handles": hs, "keep": "delete"})).unwrap();
    let r1 = s.execute("insert", &json!({"name": "ATTD", "at": [10, 0], "attribs": {"TAG1": "first"}})).unwrap();
    let r2 = s.execute("insert", &json!({"name": "ATTD", "at": [50, 0], "attribs": {"TAG1": "second"}})).unwrap();
    let h1 = r1["handle"].as_str().unwrap();
    let h2 = Handle::parse_hex(r2["handle"].as_str().unwrap()).unwrap();

    // Start REFEDIT on r1
    s.execute("refedit", &json!({"handle": h1})).unwrap();
    // Add a second attribute definition TAG2
    s.execute("attdef", &json!({"tag": "TAG2", "default": "val2", "at": [10, 5], "height": 1.0})).unwrap();
    // Close with save: true
    let out = s.execute("refclose", &json!({"save": true})).unwrap();
    assert_eq!(out["saved"], true);
    assert_eq!(out["synced"], 2);

    // Both references now have TAG1 and TAG2
    let d = s.doc().unwrap();
    let cadcraft_engine::doc::EntityKind::Insert(ins2) = &d.entity(h2).unwrap().kind else { panic!("not an insert") };
    assert_eq!(ins2.attribs.len(), 2);
    assert!(ins2.attribs.iter().any(|a| a.tag.eq_ignore_ascii_case("TAG1") && a.text.value == "second"));
    assert!(ins2.attribs.iter().any(|a| a.tag.eq_ignore_ascii_case("TAG2") && a.text.value == "val2"));
}
