//! The current UCS survives saving and reopening (#1106): model space's in the header
//! ($UCSORG, $UCSXDIR, $UCSYDIR, $UCSNAME) and the shown VPORT, each layout's in its LAYOUT
//! object (groups 13, 16, 17 and 345 for a named UCS), through DXF and DWG.

use cadcraft_doc::*;
use cadcraft_geom::Vec3;

fn near3(a: Vec3, b: Vec3) -> bool {
    (a - b).len() < 1e-9
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

/// The LAYOUT object of layout `name` (its group 1 after `100 AcDbLayout`; the first group 1 is
/// the page setup's).
fn layout_record(text: &str, name: &str) -> Vec<(String, String)> {
    records(text, "LAYOUT")
        .into_iter()
        .find(|r| r.iter().skip_while(|(c, v)| !(c == "100" && v == "AcDbLayout")).any(|(c, v)| c == "1" && v == name))
        .unwrap_or_else(|| panic!("no LAYOUT {name}"))
}

fn num(rec: &[(String, String)], code: &str) -> f64 {
    group(rec, code).and_then(|v| v.trim().parse().ok()).unwrap_or(f64::NAN)
}

/// The header variable `$name`'s groups in DXF text.
fn header_var(text: &str, name: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let mut out = Vec::new();
    let mut on = false;
    for pair in lines.chunks(2) {
        let [code, value] = pair else { break };
        if *code == "9" {
            on = *value == format!("${name}");
            continue;
        }
        if *code == "0" {
            on = false;
        }
        if on {
            out.push((code.to_string(), value.to_string()));
        }
    }
    out
}

const C30: f64 = 0.866_025_403_784_438_6;

/// A drawing whose model space is in a named UCS turned 30° at (100, 50, 0), and whose first
/// layout is in an unnamed one at (5, 5) turned 90°.
fn drawing() -> Drawing {
    let mut d = Drawing::default();
    let site = Ucs { name: "SITE".into(), origin: Vec3::new(100.0, 50.0, 0.0), x_axis: Vec3::new(C30, 0.5, 0.0), y_axis: Vec3::new(-0.5, C30, 0.0) };
    d.ucss.push(site.clone());
    d.header.set("UCSORG", HVal::Point(site.origin));
    d.header.set("UCSXDIR", HVal::Point(site.x_axis));
    d.header.set("UCSYDIR", HVal::Point(site.y_axis));
    d.header.set_str("UCSNAME", "SITE");
    d.layouts[0].ucs =
        Some(Ucs { name: String::new(), origin: Vec3::new(5.0, 5.0, 0.0), x_axis: Vec3::new(0.0, 1.0, 0.0), y_axis: Vec3::new(-1.0, 0.0, 0.0) });
    d
}

fn check(back: &Drawing) {
    let h = &back.header;
    assert!(near3(h.point("UCSORG").unwrap(), Vec3::new(100.0, 50.0, 0.0)));
    assert!(near3(h.point("UCSXDIR").unwrap(), Vec3::new(C30, 0.5, 0.0)));
    assert!(near3(h.point("UCSYDIR").unwrap(), Vec3::new(-0.5, C30, 0.0)));
    assert_eq!(h.str("UCSNAME", ""), "SITE");
    let l = back.layouts[0].ucs.clone().expect("the layout keeps its UCS");
    assert!(near3(l.origin, Vec3::new(5.0, 5.0, 0.0)) && near3(l.x_axis, Vec3::new(0.0, 1.0, 0.0)) && l.name.is_empty(), "{l:?}");
    assert!(back.layouts.iter().skip(1).all(|l| l.ucs.is_none()), "the other layouts stay in the World UCS");
}

#[test]
fn model_and_layout_ucs_survive_a_dxf_save() {
    let d = drawing();
    let text = cadcraft_io::write_dxf(&d);
    // The header and the shown viewport both carry model space's UCS.
    let org = header_var(&text, "UCSORG");
    assert_eq!((num(&org, "10"), num(&org, "20"), num(&org, "30")), (100.0, 50.0, 0.0));
    assert_eq!(group(&header_var(&text, "UCSNAME"), "2"), Some("SITE"));
    let active = records(&text, "VPORT").into_iter().find(|r| group(r, "2").is_some_and(|n| n.eq_ignore_ascii_case("*ACTIVE"))).unwrap();
    assert_eq!((num(&active, "110"), num(&active, "120")), (100.0, 50.0));
    assert!((num(&active, "111") - C30).abs() < 1e-12 && group(&active, "65") == Some("1"));
    // The layout object carries the layout's.
    let lay = layout_record(&text, &d.layouts[0].name);
    assert_eq!((num(&lay, "13"), num(&lay, "23"), num(&lay, "16"), num(&lay, "26")), (5.0, 5.0, 0.0, 1.0));
    assert!(group(&lay, "345").is_none(), "unnamed: no UCS record");
    check(&cadcraft_io::read_dxf(text.as_bytes()).unwrap());
}

#[test]
fn a_layouts_named_ucs_points_at_its_record() {
    let mut d = drawing();
    d.layouts[0].ucs.as_mut().unwrap().name = "SITE".into();
    let text = cadcraft_io::write_dxf(&d);
    let site = records(&text, "UCS").into_iter().find(|r| group(r, "2") == Some("SITE")).and_then(|r| group(&r, "5").map(str::to_string)).unwrap();
    let lay = layout_record(&text, &d.layouts[0].name);
    assert_eq!(group(&lay, "345"), Some(site.as_str()));
    let back = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
    assert_eq!(back.layouts[0].ucs.as_ref().unwrap().name, "SITE");
    // A deleted named UCS isn't pointed at, and the layout keeps the axes unnamed.
    let mut gone = back.clone();
    gone.ucss.clear();
    let text = cadcraft_io::write_dxf(&gone);
    let lay = layout_record(&text, &d.layouts[0].name);
    assert!(group(&lay, "345").is_none());
    let back = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
    let u = back.layouts[0].ucs.clone().unwrap();
    assert!(u.name.is_empty() && near3(u.x_axis, Vec3::new(0.0, 1.0, 0.0)));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn model_and_layout_ucs_survive_a_dwg_save() {
    let d = drawing();
    let dwg = cadcraft_io::write(&d, "x.dwg").unwrap();
    let back = cadcraft_io::read(&dwg, "x.dwg").unwrap();
    check(&back);
    // And a second trip.
    let again = cadcraft_io::read(&cadcraft_io::write(&back, "x.dwg").unwrap(), "x.dwg").unwrap();
    check(&again);
}

#[test]
fn a_drawing_in_the_world_ucs_is_written_as_before() {
    let d = Drawing::default();
    let text = cadcraft_io::write_dxf(&d);
    assert!(header_var(&text, "UCSORG").is_empty(), "nothing set, nothing written");
    let back = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
    assert!(back.layouts.iter().all(|l| l.ucs.is_none()));
    assert!(back.header.point("UCSXDIR").is_none_or(|x| near3(x, Vec3::new(1.0, 0.0, 0.0))));
}

#[test]
fn a_tilted_ucs_is_kept_as_it_is() {
    let mut d = Drawing::default();
    d.header.set("UCSORG", HVal::Point(Vec3::new(0.0, 0.0, 10.0)));
    d.header.set("UCSXDIR", HVal::Point(Vec3::new(1.0, 0.0, 0.0)));
    d.header.set("UCSYDIR", HVal::Point(Vec3::new(0.0, 0.0, 1.0)));
    d.ucss.push(Ucs { name: "FRONT".into(), origin: Vec3::ZERO, x_axis: Vec3::new(1.0, 0.0, 0.0), y_axis: Vec3::new(0.0, 0.0, 1.0) });
    let back = cadcraft_io::read_dxf(cadcraft_io::write_dxf(&d).as_bytes()).unwrap();
    assert!(near3(back.header.point("UCSYDIR").unwrap(), Vec3::new(0.0, 0.0, 1.0)));
    assert!(near3(back.header.point("UCSORG").unwrap(), Vec3::new(0.0, 0.0, 10.0)));
    assert!(near3(back.ucss[0].y_axis, Vec3::new(0.0, 0.0, 1.0)));
}

#[test]
fn broken_ucs_values_are_not_written_back() {
    let mut d = Drawing::default();
    d.header.set("UCSORG", HVal::Point(Vec3::new(f64::NAN, 1.0, 0.0)));
    d.header.set("UCSXDIR", HVal::Point(Vec3::ZERO));
    d.header.set("UCSYDIR", HVal::Point(Vec3::new(f64::INFINITY, 0.0, 0.0)));
    d.layouts[0].ucs = Some(Ucs { name: "X".into(), origin: Vec3::new(f64::NAN, 0.0, 0.0), x_axis: Vec3::ZERO, y_axis: Vec3::ZERO });
    let text = cadcraft_io::write_dxf(&d);
    assert!(!text.to_ascii_lowercase().contains("nan") && !text.to_ascii_lowercase().contains("inf"), "no non-finite numbers written");
    let x = header_var(&text, "UCSXDIR");
    assert_eq!((num(&x, "10"), num(&x, "20")), (1.0, 0.0), "the World UCS's axes instead");
    let back = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
    assert!(back.layouts[0].ucs.is_none());
}

#[test]
fn hostile_layout_ucs_groups_are_ignored() {
    let d = Drawing::default();
    let text = cadcraft_io::write_dxf(&d);
    let name = d.layouts[0].name.clone();
    // Rewrite the layout's UCS axes (X: groups 16/26, Y: 17/27) as nonsense, each with a dangling
    // 345 handle: no length, Y along X, not a number.
    let usable = |l: &cadcraft_doc::Layout| l.ucs.as_ref().is_none_or(|u| u.name.is_empty() && u.x_axis.cross(u.y_axis).len() > 1e-9);
    for (case, [x16, x26, y17, y27], h) in [
        ("no length", ["0.0", "0.0", "0.0", "0.0"], "FFFF"),
        ("Y along X", ["1.0", "0.0", "2.0", "0.0"], "0"),
        ("not a number", ["nan", "0.0", "0.0", "1.0"], "ZZ"),
    ] {
        let mut out = String::new();
        let mut in_layout = false;
        let mut lines = text.lines();
        while let (Some(c), Some(v)) = (lines.next(), lines.next()) {
            let (ct, vt) = (c.trim(), v.trim());
            if ct == "0" {
                in_layout = vt == "LAYOUT";
            }
            let v = match (in_layout, ct) {
                (true, "16") => x16.to_string(),
                (true, "26") => x26.to_string(),
                (true, "17") => y17.to_string(),
                (true, "27") => y27.to_string(),
                _ => v.to_string(),
            };
            out.push_str(&format!("{c}\n{v}\n"));
            if in_layout && ct == "76" {
                out.push_str(&format!("345\n{h}\n"));
            }
        }
        // Refusing the file is fine; reading it must leave no unusable UCS, saved and reopened too.
        // Parallel axes are plain numbers: that file must read, so the rejection below runs.
        let back = match cadcraft_io::read_dxf(out.as_bytes()) {
            Ok(back) => back,
            Err(e) if case == "Y along X" => panic!("{case}: {e}"),
            Err(_) => continue,
        };
        let l = back.layout(&name).unwrap();
        assert!(usable(l), "{case}: {:?}", l.ucs);
        if case == "Y along X" {
            assert!(l.ucs.is_none(), "parallel axes are rejected: {:?}", l.ucs);
        }
        let again = cadcraft_io::read_dxf(cadcraft_io::write_dxf(&back).as_bytes()).unwrap();
        assert!(usable(again.layout(&name).unwrap()), "{case} after save and reopen");
    }
}

#[test]
fn a_stale_ucs_name_copy_is_not_brought_back() {
    let text = cadcraft_io::write_dxf(&drawing());
    // As another program would save it after changing the UCS: $UCSNAME cleared, the axes moved,
    // CADCraft's copy of the old name still there.
    let mut out = String::new();
    let mut lines = text.lines().peekable();
    let mut var = String::new();
    while let (Some(c), Some(v)) = (lines.next(), lines.next()) {
        if c.trim() == "9" {
            var = v.trim().to_string();
        }
        let v = match (var.as_str(), c.trim()) {
            ("$UCSNAME", "2") => String::new(),
            ("$UCSXDIR", "10") => "0.0".into(),
            ("$UCSXDIR", "20") => "1.0".into(),
            ("$UCSYDIR", "10") => "-1.0".into(),
            ("$UCSYDIR", "20") => "0.0".into(),
            _ => v.to_string(),
        };
        out.push_str(&format!("{c}\n{v}\n"));
    }
    let back = cadcraft_io::read_dxf(out.as_bytes()).unwrap();
    assert_eq!(back.header.str("UCSNAME", ""), "", "the UCS is no longer SITE");
    assert!(near3(back.header.point("UCSXDIR").unwrap(), Vec3::new(0.0, 1.0, 0.0)));
    // While it still matches, the copy names it (what a DWG round trip relies on).
    let mut same = String::new();
    let mut lines = text.lines();
    let mut var = String::new();
    while let (Some(c), Some(v)) = (lines.next(), lines.next()) {
        if c.trim() == "9" {
            var = v.trim().to_string();
        }
        let v = if var == "$UCSNAME" && c.trim() == "2" { String::new() } else { v.to_string() };
        same.push_str(&format!("{c}\n{v}\n"));
    }
    assert_eq!(cadcraft_io::read_dxf(same.as_bytes()).unwrap().header.str("UCSNAME", ""), "SITE");
}
