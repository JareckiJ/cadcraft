//! Tests for UCS (User Coordinate System), PLAN view, and 3D drafting variables (ELEVATION/THICKNESS).

use cadcraft_doc::EntityKind;
use cadcraft_engine::Session;
use serde_json::json;

#[test]
fn ucs_world_origin_and_sysvars() {
    let mut s = Session::new();
    assert_eq!(s.doc().unwrap().header.str("UCSNAME", ""), "");
    assert_eq!(s.execute("getvar", &json!({ "name": "UCSORG" })).unwrap()["value"], json!([0.0, 0.0, 0.0]));
    assert_eq!(s.execute("getvar", &json!({ "name": "UCSXDIR" })).unwrap()["value"], json!([1.0, 0.0, 0.0]));
    assert_eq!(s.execute("getvar", &json!({ "name": "UCSYDIR" })).unwrap()["value"], json!([0.0, 1.0, 0.0]));

    // Set origin to (10, 20, 0)
    s.execute("ucs", &json!({ "origin": [10, 20, 0] })).unwrap();
    assert_eq!(s.execute("getvar", &json!({ "name": "UCSORG" })).unwrap()["value"], json!([10.0, 20.0, 0.0]));

    // Typing coordinates at command line uses active UCS
    s.cmdline("LINE").unwrap();
    s.cmdline("0,0").unwrap();
    s.cmdline("5,5").unwrap();
    s.cmdline("").unwrap();

    let d = s.doc().unwrap();
    let l1 = d.model.iter().find_map(|e| if let EntityKind::Line(l) = &e.kind { Some(l.clone()) } else { None }).unwrap();
    assert!((l1.a.x - 10.0).abs() < 1e-6);
    assert!((l1.a.y - 20.0).abs() < 1e-6);
    assert!((l1.b.x - 15.0).abs() < 1e-6);
    assert!((l1.b.y - 25.0).abs() < 1e-6);

    // Using * prefix overrides UCS to World Coordinate System (WCS)
    s.cmdline("LINE").unwrap();
    s.cmdline("*0,0").unwrap();
    s.cmdline("*5,5").unwrap();
    s.cmdline("").unwrap();

    let d = s.doc().unwrap();
    let lines: Vec<_> = d.model.iter().filter_map(|e| if let EntityKind::Line(l) = &e.kind { Some(l.clone()) } else { None }).collect();
    assert_eq!(lines.len(), 2);
    let l2 = &lines[1];
    assert!((l2.a.x - 0.0).abs() < 1e-6);
    assert!((l2.a.y - 0.0).abs() < 1e-6);
    assert!((l2.b.x - 5.0).abs() < 1e-6);
    assert!((l2.b.y - 5.0).abs() < 1e-6);
}

#[test]
fn ucs_rotation_and_coordinate_transforms() {
    let mut s = Session::new();

    // Rotate UCS 90 degrees around Z axis
    s.execute("ucs", &json!({ "rotate": "z", "angle": 90.0 })).unwrap();
    let xdir = s.execute("getvar", &json!({ "name": "UCSXDIR" })).unwrap()["value"].as_array().unwrap().clone();
    assert!((xdir[0].as_f64().unwrap() - 0.0).abs() < 1e-6);
    assert!((xdir[1].as_f64().unwrap() - 1.0).abs() < 1e-6);

    // In a 90-degree rotated UCS, (10, 0) in UCS maps to (0, 10) in WCS
    s.cmdline("LINE").unwrap();
    s.cmdline("0,0").unwrap();
    s.cmdline("10,0").unwrap();
    s.cmdline("").unwrap();

    let d = s.doc().unwrap();
    let l = d.model.iter().find_map(|e| if let EntityKind::Line(l) = &e.kind { Some(l.clone()) } else { None }).unwrap();
    assert!((l.a.x - 0.0).abs() < 1e-6);
    assert!((l.a.y - 0.0).abs() < 1e-6);
    assert!((l.b.x - 0.0).abs() < 1e-6);
    assert!((l.b.y - 10.0).abs() < 1e-6);
}

#[test]
fn ucs_save_restore_delete_and_previous() {
    let mut s = Session::new();

    // Set custom UCS and save as "Plate"
    s.execute("ucs", &json!({ "origin": [50, 100, 0] })).unwrap();
    s.execute("ucs", &json!({ "save": "Plate" })).unwrap();
    assert_eq!(s.doc().unwrap().header.str("UCSNAME", ""), "Plate");
    assert_eq!(s.doc().unwrap().ucss.len(), 1);

    // Return to World UCS
    s.execute("ucs", &json!({ "world": true })).unwrap();
    assert_eq!(s.doc().unwrap().header.str("UCSNAME", ""), "");
    assert_eq!(s.execute("getvar", &json!({ "name": "UCSORG" })).unwrap()["value"], json!([0.0, 0.0, 0.0]));

    // Previous restores Plate
    s.execute("ucs", &json!({ "previous": true })).unwrap();
    assert_eq!(s.execute("getvar", &json!({ "name": "UCSORG" })).unwrap()["value"], json!([50.0, 100.0, 0.0]));

    // Reset to World, then restore by name
    s.execute("ucs", &json!({ "world": true })).unwrap();
    s.execute("ucs", &json!({ "restore": "Plate" })).unwrap();
    assert_eq!(s.doc().unwrap().header.str("UCSNAME", ""), "Plate");
    assert_eq!(s.execute("getvar", &json!({ "name": "UCSORG" })).unwrap()["value"], json!([50.0, 100.0, 0.0]));

    // Delete
    s.execute("ucs", &json!({ "delete": "Plate" })).unwrap();
    assert_eq!(s.doc().unwrap().ucss.len(), 0);
    assert_eq!(s.doc().unwrap().header.str("UCSNAME", ""), "");
}

#[test]
fn ucs_3point_and_zaxis() {
    let mut s = Session::new();

    // 3-point UCS
    s.execute(
        "ucs",
        &json!({
            "origin": [10, 10, 0],
            "xAxis": [20, 10, 0],
            "yAxis": [10, 30, 0]
        }),
    )
    .unwrap();
    assert_eq!(s.execute("getvar", &json!({ "name": "UCSORG" })).unwrap()["value"], json!([10.0, 10.0, 0.0]));

    // Z-axis vector UCS
    s.execute(
        "ucs",
        &json!({
            "origin": [0, 0, 0],
            "zAxis": [0, 0, 5]
        }),
    )
    .unwrap();
    assert_eq!(s.execute("getvar", &json!({ "name": "UCSXDIR" })).unwrap()["value"], json!([1.0, 0.0, 0.0]));
}

#[test]
fn plan_view_command() {
    let mut s = Session::new();
    let res = s.execute("plan", &json!({ "mode": "world" })).unwrap();
    assert_eq!(res["plan"], "world");

    let res2 = s.execute("plan", &json!({ "mode": "current" })).unwrap();
    assert_eq!(res2["plan"], "current");
}

#[test]
fn elevation_and_thickness_commands() {
    let mut s = Session::new();
    assert_eq!(s.execute("getvar", &json!({ "name": "ELEVATION" })).unwrap()["value"], json!(0.0));
    assert_eq!(s.execute("getvar", &json!({ "name": "THICKNESS" })).unwrap()["value"], json!(0.0));

    s.execute("elevation", &json!({ "elevation": 15.0, "thickness": 2.5 })).unwrap();
    assert_eq!(s.execute("getvar", &json!({ "name": "ELEVATION" })).unwrap()["value"], json!(15.0));
    assert_eq!(s.execute("getvar", &json!({ "name": "THICKNESS" })).unwrap()["value"], json!(2.5));

    s.execute("thickness", &json!({ "thickness": 5.0 })).unwrap();
    assert_eq!(s.execute("getvar", &json!({ "name": "THICKNESS" })).unwrap()["value"], json!(5.0));
}

#[test]
fn interactive_ucs_at_the_command_line() {
    let mut s = Session::new();
    s.cmdline("UCS").unwrap();
    s.cmdline("Origin").unwrap();
    s.cmdline("25,35").unwrap();
    assert_eq!(s.execute("getvar", &json!({ "name": "UCSORG" })).unwrap()["value"], json!([25.0, 35.0, 0.0]));

    // Interactive World
    s.cmdline("UCS").unwrap();
    s.cmdline("World").unwrap();
    assert_eq!(s.execute("getvar", &json!({ "name": "UCSORG" })).unwrap()["value"], json!([0.0, 0.0, 0.0]));
}

#[test]
fn vpoint_direction_rotate_and_sysvars() {
    let mut s = Session::new();
    assert_eq!(s.execute("getvar", &json!({ "name": "VIEWDIR" })).unwrap()["value"], json!([0.0, 0.0, 1.0]));
    assert_eq!(s.execute("getvar", &json!({ "name": "TARGET" })).unwrap()["value"], json!([0.0, 0.0, 0.0]));

    // Explicit direction vector: [1, 1, 1] normalized is [1/√3, 1/√3, 1/√3]
    let res = s.execute("vpoint", &json!({ "direction": [1, 1, 1] })).unwrap();
    let expected = 1.0 / (3.0_f64).sqrt();
    let d = res["direction"].as_array().unwrap();
    assert!((d[0].as_f64().unwrap() - expected).abs() < 1e-4);
    assert!((d[1].as_f64().unwrap() - expected).abs() < 1e-4);
    assert!((d[2].as_f64().unwrap() - expected).abs() < 1e-4);

    let viewdir = s.execute("getvar", &json!({ "name": "VIEWDIR" })).unwrap()["value"].as_array().unwrap().clone();
    assert!((viewdir[0].as_f64().unwrap() - expected).abs() < 1e-4);

    // Set TARGET
    s.execute("setvar", &json!({ "name": "TARGET", "value": [10.0, 20.0, 30.0] })).unwrap();
    assert_eq!(s.execute("getvar", &json!({ "name": "TARGET" })).unwrap()["value"], json!([10.0, 20.0, 30.0]));

    // Interactive VPOINT at command line
    s.cmdline("VPOINT").unwrap();
    s.cmdline("0,0,1").unwrap();
    assert_eq!(s.execute("getvar", &json!({ "name": "VIEWDIR" })).unwrap()["value"], json!([0.0, 0.0, 1.0]));

    // Interactive Rotate
    s.cmdline("VPOINT").unwrap();
    s.cmdline("Rotate").unwrap();
    s.cmdline("0").unwrap(); // angle in XY plane
    s.cmdline("0").unwrap(); // angle from XY plane -> direction [1, 0, 0]
    let vd = s.execute("getvar", &json!({ "name": "VIEWDIR" })).unwrap()["value"].as_array().unwrap().clone();
    assert!((vd[0].as_f64().unwrap() - 1.0).abs() < 1e-4);
    assert!((vd[1].as_f64().unwrap() - 0.0).abs() < 1e-4);
    assert!((vd[2].as_f64().unwrap() - 0.0).abs() < 1e-4);
}

#[test]
fn preset_3d_views() {
    let mut s = Session::new();

    s.execute("view.top", &json!({})).unwrap();
    assert_eq!(s.execute("getvar", &json!({ "name": "VIEWDIR" })).unwrap()["value"], json!([0.0, 0.0, 1.0]));

    s.execute("view.bottom", &json!({})).unwrap();
    assert_eq!(s.execute("getvar", &json!({ "name": "VIEWDIR" })).unwrap()["value"], json!([0.0, 0.0, -1.0]));

    s.execute("view.left", &json!({})).unwrap();
    assert_eq!(s.execute("getvar", &json!({ "name": "VIEWDIR" })).unwrap()["value"], json!([-1.0, 0.0, 0.0]));

    s.execute("view.right", &json!({})).unwrap();
    assert_eq!(s.execute("getvar", &json!({ "name": "VIEWDIR" })).unwrap()["value"], json!([1.0, 0.0, 0.0]));

    s.execute("view.front", &json!({})).unwrap();
    assert_eq!(s.execute("getvar", &json!({ "name": "VIEWDIR" })).unwrap()["value"], json!([0.0, -1.0, 0.0]));

    s.execute("view.back", &json!({})).unwrap();
    assert_eq!(s.execute("getvar", &json!({ "name": "VIEWDIR" })).unwrap()["value"], json!([0.0, 1.0, 0.0]));

    // Isometric views
    s.execute("view.neiso", &json!({})).unwrap();
    let expected = 1.0 / (3.0_f64).sqrt();
    let d = s.execute("getvar", &json!({ "name": "VIEWDIR" })).unwrap()["value"].as_array().unwrap().clone();
    assert!((d[0].as_f64().unwrap() - expected).abs() < 1e-4);
    assert!((d[1].as_f64().unwrap() - expected).abs() < 1e-4);
    assert!((d[2].as_f64().unwrap() - expected).abs() < 1e-4);
}

#[test]
fn visual_styles_and_vscurrent() {
    let mut s = Session::new();
    assert_eq!(s.execute("getvar", &json!({ "name": "VSCURRENT" })).unwrap()["value"], json!("2dWireframe"));

    // Subcommands for visual styles
    s.execute("vscurrent.realistic", &json!({})).unwrap();
    assert_eq!(s.execute("getvar", &json!({ "name": "VSCURRENT" })).unwrap()["value"], json!("Realistic"));

    s.execute("vscurrent.conceptual", &json!({})).unwrap();
    assert_eq!(s.execute("getvar", &json!({ "name": "VSCURRENT" })).unwrap()["value"], json!("Conceptual"));

    s.execute("vscurrent.shadededges", &json!({})).unwrap();
    assert_eq!(s.execute("getvar", &json!({ "name": "VSCURRENT" })).unwrap()["value"], json!("Shaded with Edges"));

    s.execute("vscurrent.xray", &json!({})).unwrap();
    assert_eq!(s.execute("getvar", &json!({ "name": "VSCURRENT" })).unwrap()["value"], json!("X-Ray"));

    // SETVAR with canonicalization
    s.execute("setvar", &json!({ "name": "VSCURRENT", "value": "hidden" })).unwrap();
    assert_eq!(s.execute("getvar", &json!({ "name": "VSCURRENT" })).unwrap()["value"], json!("Hidden"));

    // SHADEMODE alias at command line
    s.cmdline("shademode wireframe").unwrap();
    assert_eq!(s.execute("getvar", &json!({ "name": "VSCURRENT" })).unwrap()["value"], json!("Wireframe"));
}
