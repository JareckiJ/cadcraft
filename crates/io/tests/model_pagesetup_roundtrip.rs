//! Model space page setup, margins, plot transparency, and binary style detection.

use cadcraft_doc::{Drawing, PlotStyleTable, PlotterConfig};
use cadcraft_io::{read_dxf, write_dxf};

#[test]
fn metric_drawing_has_a4_model_page_with_5mm_margins() {
    let d = Drawing::new_metric();
    assert_eq!(d.model_page.paper, "A4");
    assert_eq!(d.model_page.margins_mm, [5.0, 5.0, 5.0, 5.0]);
    assert!(d.model_page.landscape);
    assert_eq!(d.model_page.plot_area, "extents");
    assert!(d.model_page.scale_to_fit);
}

#[test]
fn model_pagesetup_and_transparency_roundtrip_in_dxf() {
    let mut d = Drawing::new_metric();
    d.model_page.paper = "A3 (297 x 420 MM)".into();
    d.model_page.width_mm = 297.0;
    d.model_page.height_mm = 420.0;
    d.model_page.margins_mm = [12.0, 10.0, 12.0, 10.0];
    d.model_page.plot_area = "display".into();
    d.model_page.scale_to_fit = false;
    d.model_page.scale = 2.0;
    d.model_page.plot_transparency = true;

    // Layout also with plot transparency
    if let Some(l) = d.layouts.first_mut() {
        l.page.plot_transparency = true;
    }

    let text = write_dxf(&d);
    let loaded = read_dxf(text.as_bytes()).unwrap();

    assert_eq!(loaded.model_page.paper, "A3 (297 x 420 MM)");
    assert_eq!(loaded.model_page.margins_mm, [12.0, 10.0, 12.0, 10.0]);
    assert_eq!(loaded.model_page.plot_area, "display");
    assert!(!loaded.model_page.scale_to_fit);
    assert!((loaded.model_page.scale - 2.0).abs() < 1e-6);
    assert!(loaded.model_page.plot_transparency);
    assert!(loaded.layouts.first().unwrap().page.plot_transparency);
}

#[test]
fn binary_plot_style_table_reports_clear_error() {
    let binary_ctb = b"\x1f\x8b\x08\x00\x00\x00\x00\x00some binary compressed ctb";
    let text = String::from_utf8_lossy(binary_ctb);
    let err = PlotStyleTable::from_file_text(&text).unwrap_err();
    assert!(err.contains("binary AutoCAD .ctb/.stb plot style tables are not supported"), "{err}");

    let null_bytes = b"{\x00\x01\x02binary";
    let text = String::from_utf8_lossy(null_bytes);
    let err = PlotStyleTable::from_file_text(&text).unwrap_err();
    assert!(err.contains("binary AutoCAD .ctb/.stb plot style tables are not supported"), "{err}");
}

#[test]
fn binary_pc3_plotter_reports_clear_error() {
    let binary_pc3 = b"\x1f\x8b\x08\x00\x00\x00\x00\x00some binary compressed pc3";
    let text = String::from_utf8_lossy(binary_pc3);
    let err = PlotterConfig::from_file_text(&text).unwrap_err();
    assert!(err.contains("binary AutoCAD .pc3 plotter files are not supported"), "{err}");
}
