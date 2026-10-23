//! The Model layout's plot settings say what CADCraft plots for model space: the drawing extents
//! fitted to the paper. Plot type 5 ("Layout information", DXF Reference, PLOTSETTINGS group 74)
//! means nothing for model space; another program's Plot dialog then showed no plot area and no
//! scale for Model.
#![cfg(not(target_arch = "wasm32"))]

use cadcraft_doc::*;
use cadcraft_geom::Vec3;

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

/// A LAYOUT record's name (the code 1 after the `AcDbLayout` marker), plot type (74) and
/// standard scale type (75).
fn layout_plot(rec: &[(String, String)]) -> (String, Option<i64>, Option<i64>) {
    let at = rec.iter().position(|(c, v)| c == "100" && v == "AcDbLayout").unwrap_or(rec.len());
    let name = rec.iter().skip(at).find(|(c, _)| c == "1").map(|(_, v)| v.clone()).unwrap_or_default();
    let int = |code: &str| rec.iter().take(at).find(|(c, _)| c == code).and_then(|(_, v)| v.trim().parse().ok());
    (name, int("74"), int("75"))
}

fn plots(text: &str) -> Vec<(String, Option<i64>, Option<i64>)> {
    records(text, "LAYOUT").iter().map(|r| layout_plot(r)).collect()
}

fn drawing() -> Drawing {
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(100.0, 50.0, 0.0) })).unwrap();
    d
}

#[test]
fn model_layout_plots_extents_fitted_and_layouts_their_sheet() {
    let p = plots(&cadcraft_io::write_dxf(&drawing()));
    let model = p.iter().find(|(n, _, _)| n == "Model").unwrap();
    assert_eq!((model.1, model.2), (Some(1), Some(0)), "Model: drawing extents, scaled to fit");
    // Paper-space layouts keep "Layout information" at 1:1.
    let layouts: Vec<_> = p.iter().filter(|(n, _, _)| n != "Model").collect();
    assert!(!layouts.is_empty());
    assert!(layouts.iter().all(|(_, t, s)| (*t, *s) == (Some(5), Some(16))), "{layouts:?}");
}

#[test]
fn model_layout_plot_type_survives_the_dwg_bridge() {
    let dwg = cadcraft_io::write(&drawing(), "x.dwg").unwrap();
    // What the DWG holds, as the bridge's own DXF.
    let dxf = String::from_utf8_lossy(&cadcraft_dwg::dwg_to_dxf(&dwg).unwrap()).to_string();
    let p = plots(&dxf);
    let model = p.iter().find(|(n, _, _)| n.eq_ignore_ascii_case("Model")).unwrap();
    assert_eq!(model.1, Some(1), "{p:?}");
    assert_ne!(p.iter().find(|(n, _, _)| !n.eq_ignore_ascii_case("Model")).and_then(|l| l.1), None);
}

/// Model space's page setup as PAGESETUP or the Plot dialog's Apply to Layout keeps it.
fn model_page() -> PageSetup {
    PageSetup {
        device: "DWG To PDF.pc3".into(),
        paper: "ISO A3 (297.00 x 420.00 MM)".into(),
        paper_mm: true,
        width_mm: 297.0,
        height_mm: 420.0,
        margins_mm: [5.0, 6.0, 7.0, 8.0],
        landscape: false,
        plot_area: "display".into(),
        plot_window: [0.0; 4],
        scale_to_fit: false,
        scale: 0.02,
        plot_style_table: "monochrome.ctb".into(),
        center: false,
        lineweights: false,
        show_plot_styles: true,
        plot_transparency: false,
    }
}

#[test]
fn model_page_setup_round_trips() {
    let mut d = drawing();
    d.model_page = Some(model_page());
    let text = cadcraft_io::write_dxf(&d);
    let model = plots(&text).into_iter().find(|(n, _, _)| n == "Model").unwrap();
    assert_eq!((model.1, model.2), (Some(0), Some(16)), "display, custom scale");
    let back = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
    assert_eq!(back.model_page, Some(model_page()));
    let default_layout = PageSetup { paper_mm: true, ..Default::default() };
    assert!(back.layouts.iter().all(|l| l.page == default_layout), "layouts keep their own settings in metric paper units");
    // Limits too; unsupported model plot areas use the extents.
    for (area, read) in [("limits", "limits"), ("window", "extents"), ("layout", "extents")] {
        d.model_page = Some(PageSetup { plot_area: area.into(), ..model_page() });
        let back = cadcraft_io::read_dxf(cadcraft_io::write_dxf(&d).as_bytes()).unwrap();
        assert_eq!(back.model_page.map(|p| p.plot_area), Some(read.to_string()), "{area}");
    }
}

/// `text` with group `code` of its first LAYOUT record (Model's) set to `value`.
fn set_in_model_layout(text: &str, code: &str, value: &str) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let mut inside = false;
    let mut i = 0;
    while i + 1 < lines.len() {
        let (c, v) = (lines[i].trim().to_string(), lines[i + 1].trim().to_string());
        if c == "0" {
            if inside {
                break;
            }
            inside = v == "LAYOUT";
        } else if inside && c == code {
            lines[i + 1] = value.to_string();
        }
        i += 2;
    }
    lines.join("\n")
}

#[test]
fn a_drawing_without_a_model_page_setup_reads_back_without_one() {
    let text = cadcraft_io::write_dxf(&drawing());
    assert_eq!(plots(&text).first().map(|p| p.0.as_str()), Some("Model"), "Model's LAYOUT comes first");
    assert_eq!(cadcraft_io::read_dxf(text.as_bytes()).unwrap().model_page, None);
    // Both explicit millimetres and the units flag older versions wrote remain unset.
    for units in ["0", "1"] {
        let text = set_in_model_layout(&text, "72", units);
        let back = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
        assert_eq!(back.model_page, None, "paper units {units}");
        assert_eq!(cadcraft_render::paper::model_page(&back).width_mm, 210.0, "the metric default stays A4");
    }
    // What older versions wrote for Model (the default page, plot type 5 at 1:1) reads the same.
    let older = set_in_model_layout(&set_in_model_layout(&text, "74", "5"), "75", "16");
    assert_eq!(plots(&older).first().map(|p| (p.1, p.2)), Some((Some(5), Some(16))));
    assert_eq!(cadcraft_io::read_dxf(older.as_bytes()).unwrap().model_page, None);
    // So does a file without LAYOUT objects.
    let bare = "0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0\n20\n0\n11\n1\n21\n1\n0\nENDSEC\n0\nEOF\n";
    assert_eq!(cadcraft_io::read_dxf(bare.as_bytes()).unwrap().model_page, None);
    // Another program's Model plot settings are kept, hostile numbers replaced.
    let other = set_in_model_layout(&set_in_model_layout(&text, "44", "nan"), "142", "-3");
    let page = cadcraft_io::read_dxf(set_in_model_layout(&other, "74", "0").as_bytes()).unwrap().model_page.unwrap();
    assert_eq!((page.plot_area.as_str(), page.width_mm, page.scale), ("display", PageSetup::default().width_mm, 1.0));
}

#[test]
fn imperial_model_page_with_explicit_mm_units_is_preserved() {
    let mut d = Drawing::new_imperial();
    let page = PageSetup { paper_mm: true, plot_area: "extents".into(), scale_to_fit: true, ..Default::default() };
    d.model_page = Some(page.clone());
    let back = cadcraft_io::read_dxf(cadcraft_io::write_dxf(&d).as_bytes()).unwrap();
    assert_eq!(back.model_page, Some(page), "explicit metric paper units in an imperial drawing are a saved setup");
}

#[test]
fn model_page_setup_survives_the_dwg_bridge() {
    let mut d = drawing();
    d.model_page = Some(model_page());
    let back = cadcraft_io::read(&cadcraft_io::write(&d, "x.dwg").unwrap(), "x.dwg").unwrap();
    assert_eq!(back.model_page, Some(model_page()));
    // Without one, none comes back.
    let back = cadcraft_io::read(&cadcraft_io::write(&drawing(), "x.dwg").unwrap(), "x.dwg").unwrap();
    assert_eq!(back.model_page, None);
}
