//! FF1: other programs' objects go back into a DWG of their own version (the carry), with their
//! references moved to what has the same role in the file CADCraft writes; what can't go back is
//! reported with its bytes. Original synthetic source shaped like Architectural Desktop's display
//! configuration (`support/ff1.rs`). DWG oracles share acadrust 0.6.5; no AutoCAD validation is
//! claimed.
#![cfg(not(target_arch = "wasm32"))]

#[allow(dead_code)] // this crate uses only the display-configuration source
#[path = "support/ff1.rs"]
mod ff1;

use std::io::Cursor;

use acadrust::objects::{ObjectType, XRecordValue};
use acadrust::{CadDocument, DwgReader, Handle};
use ff1::TestResult;

fn native(bytes: &[u8]) -> TestResult<CadDocument> {
    let out = DwgReader::from_stream(Cursor::new(bytes)).read_with_stats()?;
    assert_eq!((out.stats.skipped_source_records, out.stats.recovered_errors), (0, 0));
    Ok(out.document)
}

/// An R2010+ record's references: its extended data's applications, then its handle stream.
fn references(raw: &[u8], handle_bits: i64) -> (Vec<u64>, Vec<(u8, u64)>) {
    let bit = |i: usize| raw.get(i / 8).map_or(0u64, |b| u64::from((b >> (7 - (i % 8))) & 1));
    let read = |pos: &mut usize, n: usize| (0..n).fold(0u64, |v, _| (v << 1) | bit((*pos, *pos += 1).0));
    let total = raw.len() * 8;
    let start = total - usize::try_from(handle_bits).unwrap_or(0);
    let read_ref = |pos: &mut usize| {
        let code = read(pos, 4);
        let n = read(pos, 4) as usize;
        (code as u8, read(pos, n * 8))
    };
    let mut pos = 0;
    let bits = if read(&mut pos, 2) < 2 { 8 } else { 16 };
    read(&mut pos, bits);
    read_ref(&mut pos);
    let mut apps = Vec::new();
    loop {
        let size = match read(&mut pos, 2) {
            0 => {
                let lo = read(&mut pos, 8);
                lo | (read(&mut pos, 8) << 8)
            }
            1 => read(&mut pos, 8),
            2 => 0,
            _ => 256,
        };
        if size == 0 {
            break;
        }
        apps.push(read_ref(&mut pos).1);
        pos += size as usize * 8;
    }
    let mut refs = Vec::new();
    let mut pos = start;
    while total - pos >= 8 {
        refs.push(read_ref(&mut pos));
    }
    (apps, refs)
}

fn record(doc: &CadDocument, h: Handle) -> TestResult<(&String, &Vec<u8>, i64, Handle)> {
    match doc.objects.get(&h) {
        Some(ObjectType::Unknown { type_name, raw_dwg_data: Some(raw), raw_dwg_handle_bits, owner, .. }) => {
            Ok((type_name, raw, *raw_dwg_handle_bits, *owner))
        }
        _ => Err(format!("record {:X} absent", h.value()).into()),
    }
}

fn edit_line(doc: &mut cadcraft_doc::Drawing, dx: f64) -> TestResult {
    let h = doc.model.iter().find(|e| matches!(e.kind, cadcraft_doc::EntityKind::Line(_))).ok_or("LINE")?.handle;
    doc.modify_entity(h, |e| {
        if let cadcraft_doc::EntityKind::Line(l) = &mut e.kind {
            l.b.x += dx;
        }
    })?;
    Ok(())
}

#[test]
fn display_objects_go_back_into_a_same_version_dwg_with_their_references_moved() -> TestResult {
    let src = ff1::display_source()?;
    let source = native(&src.bytes)?;
    let mut doc = cadcraft_io::read(&src.bytes, "display.dwg")?;
    assert_eq!(doc.preservation.issue_count(), 4, "the four opaque records aren't in the editable drawing");
    for cycle in 1..=2 {
        edit_line(&mut doc, 5.)?;
        let (bytes, report) = cadcraft_io::write_with_report(&doc, "out.dwg", &cadcraft_io::SaveOptions::default())?;
        let report = report.ok_or("no report")?;
        assert_eq!(report.kept_natively, 3, "props, rep and vars go back");
        // The odd one refers to a handle with no role: reported, with its bytes and the reason.
        // (The reopened file of the second cycle no longer has it.)
        if cycle == 1 {
            assert_eq!(report.records.len(), 1);
            let odd = report.records.first().ok_or("odd")?;
            assert_eq!(odd.record.as_deref(), Some(src.odd.name));
            assert!(odd.reason.contains(&format!("{:X}", ff1::NO_ROLE)), "{}", odd.reason);
            let (_, odd_raw, odd_bits, _) = record(&source, src.odd.handle)?;
            assert_eq!(odd.dwg.as_ref().map(|d| (&d.bytes, d.handle_bits)), Some((odd_raw, odd_bits)));
        } else {
            assert!(report.is_empty(), "{:?}", report.records);
        }
        let out = native(&bytes)?;
        let layer0 = out.layers.get("0").ok_or("layer 0")?.handle.value();
        let byblock = out.line_types.get("ByBlock").ok_or("ByBlock")?.handle.value();
        let standard = out.text_styles.get("Standard").ok_or("Standard")?.handle.value();
        let acad = out.app_ids.get("ACAD").ok_or("ACAD")?.handle.value();
        let nod = out.header.named_objects_dict_handle;
        let Some(ObjectType::Dictionary(n)) = out.objects.get(&nod) else { return Err("NOD".into()) };
        for (key, top) in [("FF1_PROPS", src.props.owner), ("FF1_REPS", src.rep.owner), ("FF1_VARS", src.vars.owner)] {
            assert!(n.entries.iter().any(|(k, h)| k == key && *h == top), "{key}");
            let Some(ObjectType::Dictionary(d)) = out.objects.get(&top) else { return Err(format!("{key} dictionary").into()) };
            assert_eq!(d.owner, nod);
        }
        let classes: Vec<_> = out.classes.iter().collect();
        for r in [&src.props, &src.rep, &src.vars] {
            let (name, raw, bits, owner) = record(&out, r.handle)?;
            assert_eq!((name.as_str(), owner), (r.name, r.owner));
            let (apps, refs) = references(raw, bits);
            assert_eq!(apps, [acad], "{}: its EED names the written file's ACAD", r.name);
            assert_eq!(refs.first(), Some(&(4, r.owner.value())), "{}: owner", r.name);
            // Its class is at the position its type code names (the classes section is positional).
            let class = classes.iter().find(|c| c.dxf_name == r.name).ok_or("class")?;
            assert_eq!(classes.iter().position(|c| std::ptr::eq(*c, *class)).map(|i| i + 500), Some(usize::try_from(class.class_number)?));
            // After the owner (no reactors, no extension dictionary): the record's own references.
            match r.name {
                "FF1_DISP_PROPS" => assert_eq!(refs.get(1..4), Some(&[(5, layer0), (5, byblock), (4, standard)][..])),
                "FF1_DISP_REP" => assert_eq!(refs.get(1), Some(&(4, src.props.handle.value())), "into another carried subtree"),
                _ => assert_eq!(refs.get(1..3), Some(&[(3, src.owned.0.value()), (5, src.line.value())][..])),
            }
        }
        // What vars owns came back with it.
        let Some(ObjectType::Dictionary(owned)) = out.objects.get(&src.owned.0) else { return Err("owned dictionary".into()) };
        assert_eq!(owned.owner, src.vars.handle);
        let Some(ObjectType::XRecord(x)) = out.objects.get(&src.owned.1) else { return Err("owned XRECORD".into()) };
        assert!(x.entries.iter().any(|e| e.value == XRecordValue::String("FF1_HISTORY_UNIQUE".into())));
        assert!(!out.objects.contains_key(&src.odd.handle));
        let line = out.get_entity(src.line).ok_or("LINE")?;
        let acadrust::EntityType::Line(l) = line else { return Err("LINE kind".into()) };
        assert_eq!(l.end.x, 11. + 5. * f64::from(cycle));
        // Reopened and saved again (the next cycle), the records go back again.
        doc = cadcraft_io::read(&bytes, "out.dwg")?;
    }
    Ok(())
}

#[test]
fn another_version_or_dxf_reports_every_display_object_with_its_bytes() -> TestResult {
    let src = ff1::display_source()?;
    let source = native(&src.bytes)?;
    let doc = cadcraft_io::read(&src.bytes, "display.dwg")?;
    for (name, version) in [("out.dwg", Some(cadcraft_io::Version::R2013)), ("out.dwg", Some(cadcraft_io::Version::R2004)), ("out.dxf", None)] {
        let (_, report) = cadcraft_io::write_with_report(&doc, name, &cadcraft_io::SaveOptions { version, binary: false })?;
        let report = report.ok_or("no report")?;
        assert_eq!(report.kept_natively, 0);
        assert_eq!(report.count(), 4, "{name} {version:?}");
        for r in [&src.props, &src.rep, &src.vars, &src.odd] {
            let lost = report.records.iter().find(|x| x.record.as_deref() == Some(r.name)).ok_or("not reported")?;
            let (_, raw, bits, _) = record(&source, r.handle)?;
            assert_eq!(lost.dwg.as_ref().map(|d| (&d.bytes, d.handle_bits, d.version.as_str())), Some((raw, bits, "AC1032")));
            assert_eq!(lost.handle.as_deref(), Some(format!("{:X}", r.handle.value()).as_str()));
        }
    }
    Ok(())
}

#[test]
fn audit_counts_what_a_save_keeps_and_loses_without_writing() -> TestResult {
    let src = ff1::display_source()?;
    let a = cadcraft_io::audit(&src.bytes, "display.dwg", "dwg", &cadcraft_io::SaveOptions::default())?;
    assert_eq!((a.source.as_str(), a.target.as_str()), ("DWG AC1032", "DWG AC1032"));
    let report = a.report.as_ref().ok_or("report")?;
    assert_eq!((report.kept_natively, report.count()), (3, 1));
    assert!(a.inventory_error.is_none());
    // The independent count sees the odd record missing from the saved file.
    assert!(a.differences.iter().any(|(k, x, y)| k == "object FF1_ODD" && (*x, *y) == (1, 0)), "{:?}", a.differences);
    assert!(!a.differences.iter().any(|(k, _, _)| k == "object FF1_DISP_PROPS"));
    let json: serde_json::Value = serde_json::from_str(&a.to_json())?;
    assert_eq!(json["kept_natively"], 3);
    assert_eq!(json["lost"], 1);
    Ok(())
}
