//! UCS construction, preview and selection use one frame; JSON coordinates remain WCS.
use cadcraft_doc::{Block, Common, Entity, EntityKind, Handle, Line, Solid, Space};
use cadcraft_engine::{Input, Session, select};
use cadcraft_geom::{Vec2, Vec3};
use serde_json::json;
use std::sync::Arc;

fn session(angle: f64) -> Session {
    let mut s = Session::new();
    s.settings.osmode = 0;
    s.settings.orthomode = false;
    s.settings.snapmode = false;
    s.execute("ucs", &json!({"option":"origin","origin":[100,50]})).unwrap();
    s.execute("ucs", &json!({"option":"z","angle":angle})).unwrap();
    s
}
fn point(s: &mut Session, x: f64, y: f64) {
    s.input(Input::Point(s.input_ucs().to_wcs(Vec2::new(x, y)))).unwrap();
}
fn answer(s: &mut Session, text: &str) {
    s.input(if text.is_empty() { Input::Enter } else { Input::Text(text.into()) }).unwrap();
}
fn last(s: &Session) -> EntityKind {
    s.doc().unwrap().model.iter().last().unwrap().kind.clone()
}
fn angle(actual: f64, expected: f64) {
    assert!((actual - expected).sin().abs() < 1e-8 && (actual - expected).cos() > 0.999999, "{actual} != {expected}");
}
fn block(s: &mut Session) {
    let mut block = Block::new("Probe");
    block.entities.push(Entity::new(Handle(10_000), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(3.0, 1.0, 0.0) })));
    s.doc_mut().unwrap().blocks.insert("Probe".into(), Arc::new(block));
}

#[test]
fn rectangles_use_ucs_axes_in_every_quadrant_and_preview_matches_commit() {
    for rotation in [37.0, 90.0, 137.0, -25.0] {
        for (x, y) in [(8.0, 3.0), (-8.0, 3.0), (8.0, -3.0), (-8.0, -3.0)] {
            let mut s = session(rotation);
            s.start("rectangle").unwrap();
            point(&mut s, 0.0, 0.0);
            let cursor = s.input_ucs().to_wcs(Vec2::new(x, y));
            let preview = s.preview(cursor).first().unwrap().kind.clone();
            point(&mut s, x, y);
            assert_eq!(preview, last(&s));
            let EntityKind::LwPolyline(poly) = last(&s) else { panic!("rectangle") };
            let expected = [Vec2::ZERO, Vec2::new(x, 0.0), Vec2::new(x, y), Vec2::new(0.0, y)];
            for e in expected {
                assert!(poly.vertices.iter().any(|v| s.input_ucs().to_ucs(v.p).dist(e) < 1e-8));
            }
            s.undo().unwrap();
            assert!(s.doc().unwrap().model.is_empty());
        }
    }
}

#[test]
fn rectangle_dimensions_and_explicit_zero_rotation_use_the_same_ucs() {
    let mut s = session(37.0);
    s.cmdline("rectang 0,0 dimensions 8 3 rotation 0").unwrap();
    point(&mut s, 10.0, 4.0);
    let EntityKind::LwPolyline(poly) = last(&s) else { panic!("rectangle") };
    assert!(s.input_ucs().vec_to_ucs(poly.vertices[1].p - poly.vertices[0].p).dist(Vec2::new(8.0, 0.0)) < 1e-8);
}

#[test]
fn text_and_mtext_defaults_and_typed_zero_follow_the_ucs() {
    for explicit in [false, true] {
        let mut s = session(37.0);
        s.start("text").unwrap();
        point(&mut s, 1.0, 2.0);
        answer(&mut s, "1");
        answer(&mut s, if explicit { "0" } else { "" });
        answer(&mut s, "Label");
        answer(&mut s, "");
        let EntityKind::Text(text) = last(&s) else { panic!("text") };
        angle(text.rotation, 37f64.to_radians());
        s.start("mtext").unwrap();
        point(&mut s, 0.0, 0.0);
        point(&mut s, 8.0, 3.0);
        answer(&mut s, "Wrapped label");
        answer(&mut s, "");
        let EntityKind::MText(text) = last(&s) else { panic!("mtext") };
        angle(text.rotation, 37f64.to_radians());
        assert!((text.width - 8.0).abs() < 1e-8);
        assert!(s.input_ucs().to_ucs(text.insert.xy()).dist(Vec2::new(0.0, 3.0)) < 1e-8);
    }
}

#[test]
fn block_defaults_and_corner_scale_follow_ucs_including_xattach_shared_machine() {
    for corner in [false, true] {
        let mut s = session(37.0);
        block(&mut s);
        s.start("insert").unwrap();
        answer(&mut s, "Probe");
        let cursor = s.input_ucs().to_wcs(Vec2::new(2.0, 3.0));
        let EntityKind::Insert(preview) = s.preview(cursor).first().unwrap().kind.clone() else { panic!("preview") };
        angle(preview.rotation, 37f64.to_radians());
        point(&mut s, 2.0, 3.0);
        if corner {
            point(&mut s, 4.0, 6.0);
        } else {
            answer(&mut s, "");
            answer(&mut s, "");
        }
        answer(&mut s, "");
        let EntityKind::Insert(insert) = last(&s) else { panic!("insert") };
        angle(insert.rotation, 37f64.to_radians());
        let expected = if corner { Vec2::new(2.0, 3.0) } else { Vec2::new(1.0, 1.0) };
        assert!(insert.scale.xy().dist(expected) < 1e-8);
    }
}

#[test]
fn numeric_polygon_and_fixed_xlines_follow_working_axes() {
    let mut s = session(37.0);
    s.cmdline("polygon 4 0,0 i 5").unwrap();
    let EntityKind::LwPolyline(poly) = last(&s) else { panic!("polygon") };
    let local: Vec<_> = poly.vertices.iter().map(|v| s.input_ucs().to_ucs(v.p)).collect();
    assert!((local[0].x - 5.0 / 2f64.sqrt()).abs() < 1e-8 && (local[0].y + 5.0 / 2f64.sqrt()).abs() < 1e-8);
    for (option, direction) in [("hor", s.input_ucs().x.xy()), ("ver", s.input_ucs().y.xy())] {
        s.cmdline(&format!("xline {option} 0,0")).unwrap();
        answer(&mut s, "");
        let EntityKind::XLine(line) = last(&s) else { panic!("xline") };
        assert!(line.dir.xy().dist(direction) < 1e-8);
    }
}

#[test]
fn oriented_windows_exclude_empty_world_box_corners_and_choose_crossing_in_ucs() {
    let mut s = session(137.0);
    for (a, b) in [(Vec2::new(1.0, 1.0), Vec2::new(2.0, 1.0)), (Vec2::new(1.0, 4.0), Vec2::new(2.0, 4.0))] {
        let (a, b) = (s.input_ucs().to_wcs(a), s.input_ucs().to_wcs(b));
        s.execute("line", &json!({"points":[[a.x,a.y],[b.x,b.y]]})).unwrap();
    }
    let all: Vec<_> = s.doc().unwrap().model.iter().map(|e| e.handle).collect();
    let (a, b) = (s.input_ucs().to_wcs(Vec2::ZERO), s.input_ucs().to_wcs(Vec2::new(3.0, 3.0)));
    assert!(b.x < a.x, "world X would choose the wrong selection mode");
    s.begin_window(a);
    s.idle_click(b, false).unwrap();
    assert_eq!(s.selection(), vec![all[0]]);
    s.set_selection(Vec::new());
    s.start("select").unwrap();
    s.cmdline("w 0,0 3,3").unwrap();
    answer(&mut s, "");
    assert_eq!(s.selection(), vec![all[0]]);
}

#[test]
fn rotated_crossing_windows_keep_fills_and_do_not_pick_empty_blocks() {
    let mut s = session(37.0);
    let frame = s.input_ucs();
    let corners = [Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(0.0, 10.0), Vec2::new(10.0, 10.0)].map(|p| frame.to_wcs(p).to3(0.0));
    let h = s.doc_mut().unwrap().add(&Space::Model, Common::default(), EntityKind::Solid(Solid { corners })).unwrap();
    let a = frame.to_wcs(Vec2::new(4.0, 4.0));
    let b = frame.to_wcs(Vec2::new(6.0, 6.0));
    assert_eq!(select::select_ucs_window(s.doc().unwrap(), &Space::Model, a, b, true), vec![h]);
    assert!(select::select_ucs_window(s.doc().unwrap(), &Space::Model, a, b, false).is_empty());
    block(&mut s);
    s.execute("insert", &json!({"name":"Probe","at":[100,50],"rotation":37})).unwrap();
    let handles =
        select::select_ucs_window(s.doc().unwrap(), &Space::Model, frame.to_wcs(Vec2::new(2.0, 0.1)), frame.to_wcs(Vec2::new(2.2, 0.2)), true);
    assert_eq!(handles, vec![h], "empty space in a block's bounds remains empty");
}

#[test]
fn table_layout_bounds_and_cell_picking_use_ucs_orientation() {
    let mut s = session(37.0);
    s.cmdline("table 2 2 0,0").unwrap();
    let EntityKind::Table(table) = last(&s) else { panic!("table") };
    angle(table.rotation, 37f64.to_radians());
    let p = table.rotate_point(table.insert.xy() + Vec2::new(table.col_widths[0] / 2.0, -table.row_heights[0] / 2.0));
    assert_eq!(table.cell_at_point(p), Some((0, 0)));
    let geometry = cadcraft_render::table_geometry(s.doc().unwrap(), &table);
    assert!(geometry.lines.iter().all(|l| table.bounds().expand(1e-8).contains(l.a) && table.bounds().expand(1e-8).contains(l.b)));
}

#[test]
fn ordinate_measurement_and_leader_text_use_the_ucs_origin_and_axes() {
    let mut s = session(37.0);
    s.start("dimordinate").unwrap();
    point(&mut s, 4.0, 2.0);
    point(&mut s, 4.0, 5.0);
    let EntityKind::Dimension(dimension) = last(&s) else { panic!("dimension") };
    let geometry = cadcraft_render::dimension_geometry(&dimension, &Default::default(), 1.0);
    assert!((geometry.measurement - 4.0).abs() < 1e-8, "{}", geometry.measurement);
    angle(dimension.horizontal_direction, 37f64.to_radians());
    s.start("mleader").unwrap();
    point(&mut s, 0.0, 0.0);
    point(&mut s, 5.0, 3.0);
    answer(&mut s, "Label");
    let EntityKind::MLeader(leader) = last(&s) else { panic!("leader: {:?}", s.current_prompt()) };
    angle(leader.text.as_ref().unwrap().rotation, 37f64.to_radians());
    let [a, b] = leader.landing_segment().unwrap();
    assert!(s.input_ucs().vec_to_ucs(b - a).y.abs() < 1e-8);
}

#[test]
fn json_points_and_explicit_rotations_remain_world_coordinates() {
    let mut s = session(37.0);
    s.execute("rectang", &json!({"p1":[0,0],"p2":[8,3]})).unwrap();
    let EntityKind::LwPolyline(poly) = last(&s) else { panic!("rectangle") };
    assert_eq!(poly.vertices[1].p, Vec2::new(8.0, 0.0));
    s.execute("text", &json!({"at":[1,2],"text":"World","rotation":0})).unwrap();
    let EntityKind::Text(text) = last(&s) else { panic!("text") };
    assert_eq!(text.insert.xy(), Vec2::new(1.0, 2.0));
    angle(text.rotation, 0.0);
}

fn local_line(s: &mut Session) -> Handle {
    let frame = s.input_ucs();
    let (a, b) = (frame.to_wcs(Vec2::ZERO), frame.to_wcs(Vec2::new(5.0, 0.0)));
    let value = s.execute("line", &json!({"points":[[a.x,a.y],[b.x,b.y]]})).unwrap();
    Handle::parse_hex(value["handles"][0].as_str().unwrap()).unwrap()
}

#[test]
fn mouse_rotation_zero_is_the_ucs_x_axis_and_matches_typed_rotation() {
    let mut s = session(37.0);
    let handle = local_line(&mut s);
    s.set_selection(vec![handle]);
    s.start("rotate").unwrap();
    point(&mut s, 0.0, 0.0);
    point(&mut s, 5.0, 0.0);
    let EntityKind::Line(line) = last(&s) else { panic!("line") };
    assert!(s.input_ucs().to_ucs(line.b.xy()).dist(Vec2::new(5.0, 0.0)) < 1e-8);
    s.set_selection(vec![handle]);
    s.cmdline("rotate 0,0 reference  90").unwrap();
    let EntityKind::Line(line) = last(&s) else { panic!("line") };
    assert!(s.input_ucs().to_ucs(line.b.xy()).dist(Vec2::new(0.0, 5.0)) < 1e-8);
}

#[test]
fn displacement_vectors_do_not_include_the_translated_ucs_origin() {
    let mut s = session(37.0);
    let handle = local_line(&mut s);
    s.set_selection(vec![handle]);
    s.cmdline("move displacement 3,4").unwrap();
    let EntityKind::Line(line) = last(&s) else { panic!("line") };
    assert!(s.input_ucs().to_ucs(line.a.xy()).dist(Vec2::new(3.0, 4.0)) < 1e-8);
}

#[test]
fn stretch_uses_the_same_ucs_region_for_selection_preview_and_vertices() {
    let mut s = session(37.0);
    local_line(&mut s);
    s.cmdline("stretch c 6,-1 4,1  0,0 @2,0").unwrap();
    let EntityKind::Line(line) = last(&s) else { panic!("line") };
    assert!(s.input_ucs().to_ucs(line.a.xy()).dist(Vec2::ZERO) < 1e-8);
    assert!(s.input_ucs().to_ucs(line.b.xy()).dist(Vec2::new(7.0, 0.0)) < 1e-8);
}

#[test]
fn paste_and_pasteblock_map_the_copied_frame_to_the_current_ucs() {
    for command in ["pasteclip", "pasteblock"] {
        let mut s = session(0.0);
        let handle = local_line(&mut s);
        s.set_selection(vec![handle]);
        s.cmdline("copybase 0,0").unwrap();
        s.execute("ucs", &json!({"option":"z","angle":37})).unwrap();
        s.start(command).unwrap();
        let at = s.input_ucs().to_wcs(Vec2::new(2.0, 3.0));
        let preview = s.preview(at);
        s.input(Input::Point(at)).unwrap();
        let points = match last(&s) {
            EntityKind::Line(line) => vec![line.a.xy(), line.b.xy()],
            EntityKind::Insert(insert) => {
                let block = s.doc().unwrap().block(&insert.block).unwrap();
                let matrix = insert.transform(block.base.xy());
                let EntityKind::Line(line) = &block.entities.iter().next().unwrap().kind else { panic!("block line") };
                vec![matrix.apply(line.a.xy()), matrix.apply(line.b.xy())]
            }
            _ => panic!("pasted geometry"),
        };
        assert!(s.input_ucs().to_ucs(points[0]).dist(Vec2::new(2.0, 3.0)) < 1e-8);
        assert!(s.input_ucs().to_ucs(points[1]).dist(Vec2::new(7.0, 3.0)) < 1e-8);
        assert!(!preview.is_empty());
    }
}

#[test]
fn pline_initial_length_and_hatch_pattern_follow_ucs() {
    let mut s = session(37.0);
    s.cmdline("pline 0,0 length 5").unwrap();
    answer(&mut s, "");
    let EntityKind::LwPolyline(poly) = last(&s) else { panic!("polyline") };
    assert!(s.input_ucs().to_ucs(poly.vertices[1].p).dist(Vec2::new(5.0, 0.0)) < 1e-8);
    s.cmdline("rectang 0,0 8,3").unwrap();
    let handle = s.doc().unwrap().model.iter().last().unwrap().handle;
    s.start("hatch").unwrap();
    s.input(Input::Keyword("Select objects".into())).unwrap();
    s.input(Input::Pick(vec![handle])).unwrap();
    answer(&mut s, "");
    let hatch = s
        .doc()
        .unwrap()
        .model
        .iter()
        .find_map(|e| match &e.kind {
            EntityKind::Hatch(hatch) => Some(hatch),
            _ => None,
        })
        .unwrap();
    angle(hatch.angle, 37f64.to_radians());
}

#[test]
fn constraints_keep_their_creation_axes_after_the_ucs_changes() {
    let mut s = session(37.0);
    local_line(&mut s);
    s.start("gchorizontal").unwrap();
    point(&mut s, 2.0, 0.0);
    assert_eq!(s.doc().unwrap().constraints.len(), 1);
    s.start("dchorizontal").unwrap();
    point(&mut s, 2.0, 0.0);
    answer(&mut s, "8");
    assert_eq!(s.doc().unwrap().constraints.len(), 2);
    let frame = s.input_ucs();
    s.execute("ucs", &json!({"option":"world"})).unwrap();
    for c in &s.doc().unwrap().constraints {
        angle(c.axis_angle, 37f64.to_radians());
    }
    let EntityKind::Line(line) = last(&s) else { panic!("line") };
    let delta = frame.vec_to_ucs(line.b.xy() - line.a.xy());
    assert!((delta.x.abs() - 8.0).abs() < 1e-6 && delta.y.abs() < 1e-6);
}

#[test]
fn auto_constraints_use_the_current_ucs_instead_of_realigning_the_line_to_world() {
    let mut s = session(37.0);
    let h = local_line(&mut s);
    s.set_selection(vec![h]);
    s.start("autoconstrain").unwrap();
    assert!(s.doc().unwrap().constraints.iter().any(|c| c.kind == cadcraft_doc::ConstraintKind::Horizontal));
    let EntityKind::Line(line) = last(&s) else { panic!("line") };
    angle((line.b.xy() - line.a.xy()).angle(), 37f64.to_radians());
}

#[test]
fn qdim_and_arrayrect_use_ucs_dimensions_and_copy_directions() {
    let mut s = session(37.0);
    let h = local_line(&mut s);
    s.set_selection(vec![h]);
    s.start("qdim").unwrap();
    point(&mut s, 2.0, 3.0);
    let EntityKind::Dimension(dimension) = last(&s) else { panic!("qdim") };
    let cadcraft_doc::DimKind::Linear { rotation } = dimension.kind else { panic!("linear") };
    angle(rotation, 37f64.to_radians());
    s.doc_mut().unwrap().header.set_i64("ARRAYASSOCIATIVITY", 0);
    s.set_selection(vec![h]);
    s.start("arrayrect").unwrap();
    answer(&mut s, "");
    let lines: Vec<_> = s
        .doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Line(line) => Some(line),
            _ => None,
        })
        .collect();
    assert_eq!(lines.len(), 12);
    let origin = lines[0].a.xy();
    let deltas: Vec<_> = lines.iter().skip(1).map(|line| s.input_ucs().vec_to_ucs(line.a.xy() - origin)).collect();
    assert!(deltas.iter().any(|d| d.x > 0.0 && d.y.abs() < 1e-8));
    assert!(deltas.iter().any(|d| d.y > 0.0 && d.x.abs() < 1e-8));
}

#[test]
fn ellipse_rotation_points_match_typed_angles_in_the_ucs() {
    for pointer in [false, true] {
        let mut s = session(37.0);
        s.cmdline("ellipse center 0,0 10,0 rotation").unwrap();
        if pointer {
            point(&mut s, 0.5, 3f64.sqrt() / 2.0);
        } else {
            answer(&mut s, "60");
        }
        let EntityKind::Ellipse(ellipse) = last(&s) else { panic!("ellipse") };
        assert!((ellipse.ratio - 0.5).abs() < 1e-8);
        angle(ellipse.major.xy().angle(), 37f64.to_radians());
    }
}

#[test]
fn defining_a_block_in_ucs_does_not_rotate_its_geometry_twice_when_reinserted() {
    let mut s = session(37.0);
    let handle = local_line(&mut s);
    s.set_selection(vec![handle]);
    s.cmdline("block Saved 0,0").unwrap();
    let EntityKind::Insert(first) = last(&s) else { panic!("converted block") };
    angle(first.rotation, 37f64.to_radians());
    s.cmdline("insert Saved 10,0   ").unwrap();
    if s.prompting() {
        answer(&mut s, "");
    }
    let EntityKind::Insert(insert) = last(&s) else { panic!("inserted block") };
    let definition = s.doc().unwrap().block("Saved").unwrap();
    let EntityKind::Line(line) = &definition.entities.iter().next().unwrap().kind else { panic!("definition") };
    let matrix = insert.transform(definition.base.xy());
    let a = s.input_ucs().to_ucs(matrix.apply(line.a.xy()));
    let b = s.input_ucs().to_ucs(matrix.apply(line.b.xy()));
    assert!(a.dist(Vec2::new(10.0, 0.0)) < 1e-8 && b.dist(Vec2::new(15.0, 0.0)) < 1e-8, "{a:?} {b:?}");
}

#[test]
fn collected_multileader_blocks_are_laid_out_along_the_ucs() {
    let mut s = session(37.0);
    block(&mut s);
    let mut handles = Vec::new();
    for x in [0.0, 10.0] {
        let a = s.input_ucs().to_wcs(Vec2::new(x, 0.0));
        let b = s.input_ucs().to_wcs(Vec2::new(x + 2.0, 3.0));
        let result = s.execute("mleader", &json!({"points":[[a.x,a.y],[b.x,b.y]],"block":"Probe"})).unwrap();
        handles.push(Handle::parse_hex(result["handle"].as_str().unwrap()).unwrap());
    }
    s.set_selection(handles);
    s.start("mleadercollect").unwrap();
    point(&mut s, 0.0, 10.0);
    let mut centers = Vec::new();
    for e in s.doc().unwrap().model.iter() {
        let insert = match &e.kind {
            EntityKind::MLeader(m) => m.block.as_ref(),
            EntityKind::Insert(i) => Some(i),
            _ => None,
        };
        if let Some(insert) = insert {
            let e = Entity::new(Handle(0), EntityKind::Insert(insert.clone()));
            centers.push(s.input_ucs().to_ucs(cadcraft_doc::entity_bounds(s.doc().unwrap(), &e, 0).center()));
        }
    }
    assert_eq!(centers.len(), 2);
    assert!((centers[0].y - centers[1].y).abs() < 1e-8);
    assert!((centers[0].x - centers[1].x).abs() > 1.0);
}

#[test]
fn combining_rotated_text_keeps_reading_order_in_the_source_text_frame() {
    let mut s = session(37.0);
    let mut handles = Vec::new();
    for (x, y, text) in [(0.0, 2.0, "Top"), (10.0, 0.0, "Bottom")] {
        let at = s.input_ucs().to_wcs(Vec2::new(x, y));
        let result = s.execute("text", &json!({"at":[at.x,at.y],"height":1,"text":text,"rotation":37})).unwrap();
        handles.push(result["handle"].clone());
    }
    s.execute("ucs", &json!({"option":"world"})).unwrap();
    s.execute("txt2mtxt", &json!({"handles":handles})).unwrap();
    let EntityKind::MText(text) = last(&s) else { panic!("mtext") };
    assert!(text.contents.find("Top").unwrap() < text.contents.find("Bottom").unwrap());
    angle(text.rotation, 37f64.to_radians());
}

#[test]
fn annotation_and_table_orientation_survive_file_round_trips() {
    for extension in ["dxf", "dwg"] {
        let mut s = session(37.0);
        s.cmdline("table 2 2 0,0").unwrap();
        s.start("dimordinate").unwrap();
        point(&mut s, 4.0, 2.0);
        point(&mut s, 4.0, 5.0);
        s.start("mleader").unwrap();
        point(&mut s, 0.0, 0.0);
        point(&mut s, 5.0, 3.0);
        answer(&mut s, "Label");
        let filename = format!("UcsGeometry.{extension}");
        let data = cadcraft_io::write(s.doc().unwrap(), &filename).unwrap();
        let drawing = cadcraft_io::read(&data, &filename).unwrap();
        let table = drawing.model.iter().find_map(|e| if let EntityKind::Table(t) = &e.kind { Some(t) } else { None }).unwrap();
        angle(table.rotation, 37f64.to_radians());
        let dimension = drawing.model.iter().find_map(|e| if let EntityKind::Dimension(d) = &e.kind { Some(d) } else { None }).unwrap();
        angle(dimension.horizontal_direction, 37f64.to_radians());
        let leader = drawing.model.iter().find_map(|e| if let EntityKind::MLeader(m) = &e.kind { Some(m) } else { None }).unwrap();
        angle(leader.text.as_ref().unwrap().rotation, 37f64.to_radians());
    }
}

#[test]
fn a_named_reversed_ucs_survives_dxf_and_dwg() {
    for extension in ["dxf", "dwg"] {
        let mut s = session(37.0);
        let frame = s.ucs();
        let reversed = cadcraft_engine::ucs::UcsFrame::new(frame.origin, frame.x, -frame.y).unwrap();
        s.set_ucs(reversed, "").unwrap();
        s.execute("ucs", &json!({"option":"named","action":"save","name":"Reversed"})).unwrap();
        let filename = format!("Reversed.{extension}");
        let data = cadcraft_io::write(s.doc().unwrap(), &filename).unwrap();
        let drawing = cadcraft_io::read(&data, &filename).unwrap();
        let mut reopened = Session::empty();
        reopened.open_drawing(drawing, &filename, None);
        assert_eq!(reopened.ucs_named().1, "Reversed");
        assert!((reopened.ucs().origin - reversed.origin).len() < 1e-8);
        assert!((reopened.ucs().x - reversed.x).len() < 1e-8);
        assert!((reopened.ucs().y - reversed.y).len() < 1e-8);
        assert!(reopened.ucs().reversed());
    }
}

#[test]
fn world_block_definition_preserves_entity_fields() {
    let mut session = Session::new();
    session.execute("text", &json!({"at":[0,0],"text":"A","rotation":-30})).unwrap();
    let entity = session.doc().unwrap().model.iter().next().unwrap().clone();
    session.execute("block", &json!({"name":"WorldGeometry","base":[0,0],"handles":[entity.handle.hex()],"keep":"retain"})).unwrap();
    let drawing = session.doc().unwrap();
    let copied = drawing.block("WorldGeometry").unwrap().entities.iter().next().unwrap();
    assert_eq!(copied.kind, entity.kind, "a WCS definition must not reinterpret stored angles");
}
