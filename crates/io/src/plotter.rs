//! The plot hook the apps install ([`plot`]): PDF, PNG or SVG output of a plotter configuration (`output`), on the
//! device's paper (`width`/`height`, `margins` in mm) at its `resolution`.
//!
//! PDF goes to [`crate::pdf`]. PNG draws the sheet on white paper at the device resolution
//! (capped at [`MAX_PNG_PIXELS`]), with the plot style table and lineweights as PDF uses them.
//! SVG shows the sheet of a layout or the model-space extents.

use std::sync::Arc;

use cadcraft_color::Rgb;
use cadcraft_doc::{Drawing, Space};
use cadcraft_geom::{Bounds2, Vec2};
use cadcraft_render::{Sheet, paper, raster};
use serde_json::Value;

use crate::{IoError, PdfOptions, Result, pdf};

/// Largest PNG plot (pixels); larger sheets are drawn at a lower resolution.
pub const MAX_PNG_PIXELS: f64 = 40.0e6;

/// Plot `space` with JSON options (the engine's `plot` hook): PDF unless `output` is `png` or
/// `svg`. `margins: [l, b, r, t]` (mm) bound the printable area of this plot (model space or a
/// layout).
pub fn plot(d: &Drawing, space: &Space, opts: &Value) -> Result<Vec<u8>> {
    pdf::validate_plot_offset_json(opts)?;
    if matches!(space, Space::Model)
        && opts.get("output").and_then(Value::as_str).is_some_and(|output| output.eq_ignore_ascii_case("svg"))
        && opts.get("center").and_then(Value::as_bool) == Some(false)
    {
        return Err(IoError::Format("model-space plot offsets are not supported for SVG; choose PDF/PNG or center the plot".into()));
    }
    let margins = pdf::margins(opts.get("margins"));
    let with_margins;
    let d = match (space, margins) {
        (Space::Paper(n), Some(m)) => {
            let mut c = d.clone();
            if let Some(l) = c.layouts.iter_mut().find(|l| l.name.eq_ignore_ascii_case(n)) {
                l.page.margins_mm = m;
            }
            with_margins = c;
            &with_margins
        }
        _ => d,
    };
    match opts.get("output").and_then(Value::as_str).map(str::to_ascii_lowercase).as_deref() {
        None | Some("pdf") => pdf::plot(d, space, opts),
        Some("png") => png(d, space, opts),
        Some("svg") => svg(d, space, opts),
        Some(other) => Err(IoError::Unsupported(other.to_string())),
    }
}

/// The sheet a plot draws on (a model-space plot's printable area inside the device margins).
fn sheet(d: &Drawing, space: &Space, o: &PdfOptions) -> Result<(Sheet, cadcraft_doc::PageSetup)> {
    let page = pdf::page_for(d, space, o)?;
    Ok((Sheet::from_page(&page, paper::page_unit_mm(d, &page)), page))
}

/// Paper units per drawing unit fitting `b` into the printable area; with `fit: false`, the
/// `scale` option or 1:1, as PDF plots it.
fn fit_scale(b: &Bounds2, sheet: &Sheet, o: &PdfOptions) -> f64 {
    pdf::plot_scale(b, sheet, o.fit != Some(false), o.scale)
}

fn png(d: &Drawing, space: &Space, opts: &Value) -> Result<Vec<u8>> {
    let o = PdfOptions::from_json(opts);
    let (sheet, page) = sheet(d, space, &o)?;
    let dpi = opts.get("resolution").and_then(Value::as_f64).filter(|r| r.is_finite()).unwrap_or(300.0).clamp(50.0, 2400.0);
    // Pixels per paper unit, lowered so the image stays within the caps.
    let mut k = sheet.unit_mm / 25.4 * dpi;
    let (w, h) = (sheet.size.x * k, sheet.size.y * k);
    let shrink = (MAX_PNG_PIXELS / (w * h).max(1.0)).sqrt().min(16384.0 / w.max(h).max(1.0)).min(1.0);
    if !(shrink.is_finite() && shrink > 0.0) {
        return Err(IoError::Format("the sheet is too large to plot as an image".into()));
    }
    k *= shrink;
    let px = |v: f64| (v * k).round().clamp(1.0, 16384.0) as u32;
    let (width, height) = (px(sheet.size.x), px(sheet.size.y));
    let lineweights = o.lineweights.unwrap_or(if space.is_paper() { page.lineweights } else { true });
    let table = cadcraft_doc::plot_style_table(d, &page.plot_style_table).map(Arc::new);
    // Paper units per drawing unit: the fitted scale for model space (or a layout fitted to the
    // paper); else, for a layout, the command's scale or the page setup's, as PDF plots it (1 at 1:1).
    let fit = matches!(space, Space::Model) || o.fit == Some(true);
    let window = o.window.filter(|w| matches!(space, Space::Model) && !w.is_empty());
    let unfitted = pdf::sheet_scale(space, &page, &o).unwrap_or(1.0);
    let est = if fit { fit_scale(&window.unwrap_or_else(|| d.extents(space)), &sheet, &o) } else { unfitted };
    // Chord tolerance: a fifth of a pixel.
    let tolerance = (0.2 / (k * est)).max(1e-9);
    let transparency = o.plot_transparency.unwrap_or(page.plot_transparency);
    let ropts = cadcraft_render::Options { tolerance, lineweights, plot_style_table: table, transparency, ..cadcraft_render::Options::default() };
    let mut list = cadcraft_render::build_plot(d, space, &ropts);
    let half = Vec2::new(f64::from(width), f64::from(height)) / (2.0 * k);
    let b = window.unwrap_or(list.bounds);
    let s = if fit { fit_scale(&b, &sheet, &o) } else { unfitted };
    // Plot style ends, joins and fill patterns at the plot scale, as PDF draws them (a layout's
    // are already drawn).
    cadcraft_render::pen::apply(&mut list, s * sheet.unit_mm);
    let view = if fit {
        let (from, to) = pdf::placement(space, &b, true, &sheet, &o);
        // The world point at the image centre, mapped through the same paper origin as PDF.
        let center = from + (half - to) / s;
        raster::View { center, scale: k * s, width, height }
    } else {
        // Paper origin at the image's corner; the drawing is `s` paper units per drawing unit.
        raster::View { center: half / s, scale: k * s, width, height }
    };
    let px_per_mm = if lineweights { (k / sheet.unit_mm) as f32 } else { 0.0 };
    let ro = raster::RasterOptions { background: Rgb(255, 255, 255), hairline: 1.0, px_per_mm, antialias: true };
    raster::render_png(&list, &view, &ro).ok_or_else(|| IoError::Format("render failed".into()))
}

fn svg(d: &Drawing, space: &Space, opts: &Value) -> Result<Vec<u8>> {
    let o = PdfOptions::from_json(opts);
    let Space::Model = space else {
        let (sheet, page) = sheet(d, space, &o)?;
        let transparency = o.plot_transparency.unwrap_or(page.plot_transparency);
        // The drawing window the sheet shows at the layout's plot scale, as PDF plots it (the
        // sheet itself at 1:1; a scale that overflows leaves the sheet as is).
        let shown = sheet.size / pdf::sheet_scale(space, &page, &o).unwrap_or(1.0);
        let window = if shown.x.is_finite() && shown.y.is_finite() { Bounds2::new(Vec2::ZERO, shown) } else { sheet.bounds() };
        return Ok(crate::svg::export_window_with(d, space, Some(window), transparency).into_bytes());
    };
    // Model space as plotted: the plot style table, lineweights, and the table's ends, joins and
    // fills at the scale that fits the window (or the extents) on the device's paper.
    let (sheet, page) = sheet(d, space, &o)?;
    let window = o.window.filter(|w| !w.is_empty());
    let table = cadcraft_doc::plot_style_table(d, &page.plot_style_table).map(Arc::new);
    let lineweights = o.lineweights.unwrap_or(true);
    let est = fit_scale(&window.unwrap_or_else(|| d.extents(space)), &sheet, &o);
    // Chord tolerance: about 0.05 mm on paper.
    let tolerance = (0.05 / (sheet.unit_mm * est)).max(1e-9);
    let tolerance = if tolerance.is_finite() { tolerance } else { cadcraft_render::Options::default().tolerance };
    let transparency = o.plot_transparency.unwrap_or(page.plot_transparency);
    let ropts = cadcraft_render::Options { tolerance, lineweights, plot_style_table: table, transparency, ..cadcraft_render::Options::default() };
    let mut list = cadcraft_render::build_plot(d, space, &ropts);
    let s = fit_scale(&window.unwrap_or(list.bounds), &sheet, &o);
    cadcraft_render::pen::apply(&mut list, s * sheet.unit_mm);
    Ok(crate::svg::export_list(&list, window).into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_doc::{Common, EntityKind, Line};
    use cadcraft_geom::{Bounds2, Vec2, Vec3};
    use serde_json::json;

    /// Width and height from a PNG's header.
    fn png_size(b: &[u8]) -> (u32, u32) {
        let be = |i: usize| u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        (be(16), be(20))
    }

    fn drawing() -> Drawing {
        let mut d = Drawing::new_metric();
        d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(100.0, 50.0, 0.0) })).unwrap();
        d
    }

    #[test]
    fn png_and_svg_devices() {
        let d = drawing();
        let a4 = json!({"output": "png", "resolution": 100, "width": 210, "height": 297, "landscape": true, "margins": [5, 5, 5, 5]});
        let png = plot(&d, &Space::Model, &a4).unwrap();
        assert_eq!(png_size(&png), (1169, 827), "A4 landscape at 100 dpi");
        // Huge sheets at a high resolution are capped.
        let big = json!({"output": "png", "resolution": 2400, "width": 841, "height": 1189});
        let (w, h) = png_size(&plot(&d, &Space::Model, &big).unwrap());
        assert!(f64::from(w) * f64::from(h) <= MAX_PNG_PIXELS * 1.01 && w <= 16384 && h <= 16384);
        let layout = Space::Paper(d.layouts[0].name.clone());
        let svg = String::from_utf8(plot(&d, &layout, &json!({"output": "svg"})).unwrap()).unwrap();
        assert!(svg.starts_with("<svg"));
        assert!(plot(&d, &layout, &json!({"output": "png", "resolution": f64::NAN, "margins": [1, 2, 3]})).is_ok());
        assert!(plot(&d, &Space::Model, &json!({"output": "dwf"})).is_err());
        assert!(plot(&d, &Space::Model, &json!({})).unwrap().starts_with(b"%PDF"));
    }

    #[test]
    fn model_plot_placement_preserves_centering_and_applies_mm_offsets() {
        let model = Space::Model;
        let bounds = Bounds2::new(Vec2::new(10.0, 20.0), Vec2::new(110.0, 70.0));
        let metric = drawing();
        let metric_sheet = Sheet::from_page(&pdf::page_for(&metric, &model, &PdfOptions::default()).unwrap(), 1.0);
        let centered = PdfOptions::from_json(&json!({"center": true}));
        assert_eq!(pdf::placement(&model, &bounds, true, &metric_sheet, &PdfOptions::default()), (bounds.center(), metric_sheet.printable.center()));
        assert_eq!(pdf::placement(&model, &bounds, true, &metric_sheet, &centered), (bounds.center(), metric_sheet.printable.center()));
        let moved = PdfOptions::from_json(&json!({"center": false, "offset": [10.0, 20.0]}));
        assert_eq!(pdf::placement(&model, &bounds, true, &metric_sheet, &moved), (bounds.min, metric_sheet.printable.min + Vec2::new(10.0, 20.0)));
        let origin = PdfOptions::from_json(&json!({"center": false}));
        assert_eq!(pdf::placement(&model, &bounds, true, &metric_sheet, &origin), (bounds.min, metric_sheet.printable.min));
        let negative = PdfOptions::from_json(&json!({"center": false, "offset": [-10.0, -20.0]}));
        assert_eq!(pdf::placement(&model, &bounds, true, &metric_sheet, &negative), (bounds.min, metric_sheet.printable.min - Vec2::new(10.0, 20.0)));

        let imperial = Drawing::new_imperial();
        let imperial_sheet = Sheet::from_page(&pdf::page_for(&imperial, &model, &PdfOptions::default()).unwrap(), 25.4);
        assert_eq!(
            pdf::placement(&model, &bounds, true, &imperial_sheet, &moved),
            (bounds.min, imperial_sheet.printable.min + Vec2::new(10.0 / 25.4, 20.0 / 25.4))
        );
        assert_eq!(
            pdf::placement(&model, &bounds, true, &imperial_sheet, &negative),
            (bounds.min, imperial_sheet.printable.min - Vec2::new(10.0 / 25.4, 20.0 / 25.4))
        );
    }

    #[test]
    fn plot_offsets_keep_default_bytes_and_match_across_pdf_and_png() {
        let d = drawing();
        let model = Space::Model;
        let pdf_default = plot(&d, &model, &json!({})).unwrap();
        let pdf_centered = plot(&d, &model, &json!({"center": true})).unwrap();
        assert_eq!(pdf_default, pdf_centered, "explicit center preserves legacy PDF bytes");

        let png_options = json!({"output": "png", "resolution": 100, "width": 210, "height": 297, "landscape": true, "margins": [5, 5, 5, 5]});
        let png_default = plot(&d, &model, &png_options).unwrap();
        let mut centered_png = png_options.clone();
        centered_png["center"] = json!(true);
        assert_eq!(png_default, plot(&d, &model, &centered_png).unwrap(), "explicit center preserves legacy PNG bytes");

        let mut pdf_moved = json!({"center": false, "offset": [10.0, 20.0]});
        let moved_pdf = plot(&d, &model, &pdf_moved).unwrap();
        assert_ne!(pdf_default, moved_pdf);
        pdf_moved["output"] = json!("png");
        pdf_moved["resolution"] = json!(100);
        pdf_moved["width"] = json!(210);
        pdf_moved["height"] = json!(297);
        pdf_moved["landscape"] = json!(true);
        pdf_moved["margins"] = json!([5, 5, 5, 5]);
        let moved_png = plot(&d, &model, &pdf_moved).unwrap();
        assert_ne!(png_default, moved_png);

        let layout = Space::Paper(d.layouts[0].name.clone());
        assert_eq!(plot(&d, &layout, &json!({})).unwrap(), plot(&d, &layout, &json!({"center": false, "offset": [10.0, 20.0]})).unwrap());
    }

    #[test]
    fn invalid_and_unsupported_plot_offsets_are_reported() {
        let d = drawing();
        let model = Space::Model;
        for offset in [json!([1]), json!([1, 2, 3]), json!([1, "2"]), json!([f64::NAN, 2]), json!([100_001, 0])] {
            assert!(plot(&d, &model, &json!({"offset": offset})).is_err());
        }
        let err = plot(&d, &model, &json!({"output": "svg", "center": false, "offset": [0, 0]})).unwrap_err();
        assert!(err.to_string().contains("not supported for SVG"));
        assert!(plot(&d, &model, &json!({"output": "svg", "center": true})).unwrap().starts_with(b"<svg"));
    }

    /// Layout `Layout1` (A3 landscape, no margins) saved at `scale`, holding a horizontal line from
    /// (100, 100) to (4100, 100) drawing units.
    fn layout_drawing(scale: f64) -> (Drawing, Space) {
        let mut d = Drawing::new_metric();
        let name = d.layouts[0].name.clone();
        let space = Space::Paper(name);
        d.add(&space, Common::default(), EntityKind::Line(Line { a: Vec3::new(100.0, 100.0, 0.0), b: Vec3::new(4100.0, 100.0, 0.0) })).unwrap();
        d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(100.0, 50.0, 0.0) })).unwrap();
        let page = &mut d.layouts[0].page;
        (page.paper, page.width_mm, page.height_mm, page.landscape) = ("A3".into(), 297.0, 420.0, true);
        (page.margins_mm, page.scale) = ([0.0; 4], scale);
        (d, space)
    }

    /// The horizontal extent in millimetres on the sheet of what a PDF plot draws.
    fn pdf_x_mm(b: &[u8]) -> (f64, f64) {
        let text = String::from_utf8_lossy(b).into_owned();
        let xs: Vec<f64> =
            text.lines().filter(|l| l.ends_with(" m") || l.ends_with(" l")).filter_map(|l| l.split(' ').next()?.parse::<f64>().ok()).collect();
        let (lo, hi) = xs.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), x| (lo.min(*x), hi.max(*x)));
        assert!(xs.len() >= 2, "a line in the PDF");
        (lo * 25.4 / 72.0, hi * 25.4 / 72.0)
    }

    /// The horizontal extent in millimetres on the sheet of the non-white pixels of a PNG made at
    /// `dpi` (and the image width in pixels).
    fn png_x_mm(b: &[u8], dpi: f64) -> (f64, f64, u32) {
        let pm = tiny_skia::Pixmap::decode_png(b).unwrap();
        let w = pm.width();
        let (mut lo, mut hi) = (u32::MAX, 0);
        for (i, px) in pm.pixels().iter().enumerate() {
            if (px.red(), px.green(), px.blue()) != (255, 255, 255) {
                let x = i as u32 % w;
                (lo, hi) = (lo.min(x), hi.max(x + 1));
            }
        }
        assert!(lo < hi, "something is drawn");
        (f64::from(lo) * 25.4 / dpi, f64::from(hi) * 25.4 / dpi, w)
    }

    /// The horizontal extent in millimetres on a sheet `sheet_mm` wide of the polylines of an SVG.
    fn svg_x_mm(svg: &str, sheet_mm: f64) -> (f64, f64) {
        let vb = svg.split("viewBox=\"").nth(1).unwrap().split('"').next().unwrap();
        let vw: f64 = vb.split(' ').nth(2).unwrap().parse().unwrap();
        let pts = svg.split("points=\"").nth(1).unwrap().split('"').next().unwrap();
        let xs: Vec<f64> = pts.split(' ').filter_map(|p| p.split(',').next()?.parse().ok()).collect();
        let (lo, hi) = xs.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), x| (lo.min(*x), hi.max(*x)));
        (lo / vw * sheet_mm, hi / vw * sheet_mm)
    }

    fn near(got: (f64, f64), want: (f64, f64), tol: f64) -> bool {
        (got.0 - want.0).abs() <= tol && (got.1 - want.1).abs() <= tol
    }

    #[test]
    fn png_and_svg_of_a_layout_use_the_scale_pdf_plots_at() {
        let (d, layout) = layout_drawing(0.1);
        let sheet = json!({"device": "None", "paper": "A3", "landscape": true});
        let with = |extra: Value| {
            let mut o = sheet.clone();
            o.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            o
        };
        // 1:10 from the page setup: the line runs from 10 mm to 410 mm of the 420 mm sheet.
        let pdf_mm = pdf_x_mm(&plot(&d, &layout, &with(json!({"compress": false}))).unwrap());
        assert!(near(pdf_mm, (10.0, 410.0), 0.01), "{pdf_mm:?}");
        let png = plot(&d, &layout, &with(json!({"output": "png", "resolution": 127.0}))).unwrap();
        let (lo, hi, w) = png_x_mm(&png, 127.0);
        assert_eq!(w, 2100, "the whole sheet");
        assert!(near((lo, hi), pdf_mm, 0.6), "PNG {lo} {hi} against the PDF {pdf_mm:?}");
        let svg = String::from_utf8(plot(&d, &layout, &with(json!({"output": "svg"}))).unwrap()).unwrap();
        assert!(near(svg_x_mm(&svg, 420.0), pdf_mm, 0.01), "SVG {:?}", svg_x_mm(&svg, 420.0));
        // The command's own scale wins in all three; a page setup fitted to paper leaves its scale aside.
        let o = with(json!({"scale": 0.05, "compress": false}));
        let pdf_mm = pdf_x_mm(&plot(&d, &layout, &o).unwrap());
        assert!(near(pdf_mm, (5.0, 205.0), 0.01), "{pdf_mm:?}");
        let (lo, hi, _) = png_x_mm(&plot(&d, &layout, &with(json!({"output": "png", "resolution": 127.0, "scale": 0.05}))).unwrap(), 127.0);
        assert!(near((lo, hi), pdf_mm, 0.6), "PNG with a scale {lo} {hi}");
        let svg = String::from_utf8(plot(&d, &layout, &with(json!({"output": "svg", "scale": 0.05}))).unwrap()).unwrap();
        assert!(near(svg_x_mm(&svg, 420.0), pdf_mm, 0.01), "SVG with a scale");
        let mut fitted = d.clone();
        fitted.layouts[0].page.scale_to_fit = true;
        let png = plot(&fitted, &layout, &with(json!({"output": "png", "resolution": 127.0}))).unwrap();
        let (lo, hi, _) = png_x_mm(&png, 127.0);
        assert!((lo - 100.0).abs() < 1.0 && hi > 419.0, "1:1 as before, off the sheet: {lo} {hi}");
    }

    #[test]
    fn the_plot_dialogs_preview_draws_at_the_scale_ok_plots_at() {
        let (d, layout) = layout_drawing(0.1);
        // The parameters the Plot dialog sends for a layout (no scale; fit as the page setup says).
        let preview = json!({"landscape": true, "fit": false, "lineweights": false, "plotTransparency": false,
            "device": "None", "output": "png", "resolution": 127.0, "width": 297.0, "height": 420.0, "margins": [0, 0, 0, 0]});
        let ok = json!({"landscape": true, "fit": false, "lineweights": false, "plotTransparency": false,
            "device": "CADCraft PDF", "paper": "A3", "compress": false});
        let (lo, hi, _) = png_x_mm(&plot(&d, &layout, &preview).unwrap(), 127.0);
        let pdf_mm = pdf_x_mm(&plot(&d, &layout, &ok).unwrap());
        assert!(near((lo, hi), pdf_mm, 0.6) && near(pdf_mm, (10.0, 410.0), 0.01), "preview {lo} {hi}, OK {pdf_mm:?}");
        // The same layout saved at 1:1 previews differently (it did not on the PNG path before).
        let (one, _) = layout_drawing(1.0);
        assert_ne!(plot(&one, &layout, &preview).unwrap(), plot(&d, &layout, &preview).unwrap());
    }

    #[test]
    fn model_space_png_and_svg_are_unchanged_and_ignore_the_layouts_scale() {
        // Model space fits the extents to the printable area, whatever the layouts or a `scale`.
        let (d, _) = layout_drawing(0.1);
        let (one, _) = layout_drawing(1.0);
        let a4 = json!({"output": "png", "resolution": 127.0, "width": 210, "height": 297, "landscape": true, "margins": [5, 5, 5, 5]});
        let fitted = plot(&d, &Space::Model, &a4).unwrap();
        assert_eq!(fitted, plot(&one, &Space::Model, &a4).unwrap(), "the layout's page setup does not reach model space");
        let mut scaled = a4.clone();
        scaled.as_object_mut().unwrap().extend(json!({"scale": 0.5}).as_object().unwrap().clone());
        assert_eq!(fitted, plot(&d, &Space::Model, &scaled).unwrap(), "model-space PNG fits unless `fit: false`");
        // The line (0,0)-(100,50) fills the printable width of the 297 mm sheet with 5 mm margins:
        // 5 mm .. 292 mm (main gives the same).
        let (lo, hi, w) = png_x_mm(&fitted, 127.0);
        assert_eq!(w, 1485);
        assert!(near((lo, hi), (5.0, 292.0), 0.6), "{lo} {hi}");
        let svg =
            |d: &Drawing| String::from_utf8(plot(d, &Space::Model, &json!({"output": "svg", "device": "None", "paper": "A3"})).unwrap()).unwrap();
        assert_eq!(svg(&d), svg(&one));
        assert!(svg(&d).contains("viewBox=\"0 0 104.000000 "), "model SVG in drawing units: the 100-unit line plus 2 % margins");
    }

    #[test]
    fn hostile_layout_scales_never_panic_in_png_and_svg() {
        for sc in [1e300, 1e12, 1e-12, 1e-300, 5e-324, f64::NAN, f64::INFINITY, -1.0, 0.0] {
            let (d, layout) = layout_drawing(sc);
            for out in ["png", "svg"] {
                let r = plot(&d, &layout, &json!({"output": out, "resolution": 50.0, "device": "None", "paper": "A3"}));
                assert!(r.is_ok(), "{out} at {sc:e}");
            }
            let r = plot(&layout_drawing(0.1).0, &layout, &json!({"output": "png", "resolution": 50.0, "scale": sc}));
            assert!(r.is_ok(), "png with scale {sc:e}");
        }
    }
}
