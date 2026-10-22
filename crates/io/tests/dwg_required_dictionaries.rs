//! #1813: AutoCAD always writes an `AcDbVariableDictionary` in the named object dictionary;
//! ezdxf, and CADCraft's own DXF writer, do not. Vectric Aspire 10.5 reads an AC1018 DWG without
//! it as empty ("No data was imported..."), though it reads the same drawing as DXF (and as an
//! AC1032 DWG) fine. The DWG bridge adds the dictionary AutoCAD writes, with its usual variables,
//! when the DXF has none.
#![cfg(not(target_arch = "wasm32"))]

use std::io::Cursor;

use cadcraft_doc::{Common, Drawing, EntityKind, LwPolyline, Space};
use cadcraft_geom::{PolyVertex, Vec2};
use cadcraft_io::{SaveOptions, Version, write_with};

/// ezdxf's rectangle: one closed LWPOLYLINE in model space.
fn rectangle() -> Drawing {
    let mut d = Drawing::new_metric();
    let corners = [(0.0, 0.0), (4.0, 0.0), (4.0, 2.0), (0.0, 2.0)];
    let pl = LwPolyline {
        vertices: corners.iter().map(|(x, y)| PolyVertex::new(Vec2::new(*x, *y))).collect(),
        closed: true,
        const_width: 0.0,
        elevation: 0.0,
        plinegen: false,
    };
    d.add(&Space::Model, Common::default(), EntityKind::LwPolyline(pl)).expect("add the rectangle");
    d
}

/// The drawing variables in the named object dictionary, with their values, or nothing when the
/// dictionary is missing. Dictionary keys are case-insensitive in AutoCAD; its own spelling is
/// `AcDbVariableDictionary` (the DXF Reference calls it the ACDBVARIABLEDICTIONARY).
fn variable_dictionary(doc: &acadrust::CadDocument) -> Option<Vec<(String, String)>> {
    let acadrust::objects::ObjectType::Dictionary(root) = doc.objects.get(&doc.header.named_objects_dict_handle)? else {
        return None;
    };
    let acadrust::objects::ObjectType::Dictionary(vars) = doc.objects.get(&root.get("AcDbVariableDictionary")?)? else {
        return None;
    };
    Some(
        vars.entries
            .iter()
            .filter_map(|(name, handle)| match doc.objects.get(handle) {
                Some(acadrust::objects::ObjectType::DictionaryVariable(v)) => Some((name.clone(), v.value.clone())),
                _ => None,
            })
            .collect(),
    )
}

/// Read a DWG back with acadrust's reader, so the assertions inspect the file as another
/// application would rather than through CADCraft's own importer.
fn read_dwg(dwg: &[u8]) -> acadrust::CadDocument {
    acadrust::DwgReader::from_stream(Cursor::new(dwg)).read().expect("read the DWG back")
}

/// The value of the variable called `name`, matched case-insensitively as AutoCAD matches
/// dictionary keys, or nothing when the dictionary has no such entry.
fn value<'a>(vars: &'a [(String, String)], name: &str) -> Option<&'a str> {
    vars.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
}

/// #1813's regression: a DXF written without an `AcDbVariableDictionary` (as ezdxf and CADCraft
/// write them) must gain AutoCAD's dictionary when exported as an AC1018 DWG, so readers that
/// require it see the drawing's contents; the added objects must not break CADCraft's own read.
#[test]
fn an_ac1018_dwg_gets_the_variable_dictionary_autocad_always_writes() {
    let d = rectangle();
    let dxf = write_with(&d, "rect.dxf", &SaveOptions { version: Some(Version::R2000), binary: false }).expect("write the DXF");
    let text = String::from_utf8_lossy(&dxf).to_ascii_lowercase();
    assert!(!text.contains("acdbvariabledictionary"), "this DXF is one without the dictionary, like ezdxf's");

    let dwg = write_with(&d, "rect.dwg", &SaveOptions { version: Some(Version::R2004), binary: false }).expect("write the DWG");
    assert!(dwg.starts_with(b"AC1018"), "the DWG is AutoCAD 2004");
    let doc = read_dwg(&dwg);
    let vars = variable_dictionary(&doc).expect("AC1018 DWG: the named object dictionary has an AcDbVariableDictionary");

    // The variables AutoCAD keeps there, by name (Aspire needs at least the dictionary; these
    // are the ones the reference application writes).
    for (name, want) in [("CMLEADERSTYLE", "Standard"), ("CTABLESTYLE", "Standard"), ("CANNOSCALE", "1:1"), ("DIMASSOC", "2"), ("HIDETEXT", "1")] {
        assert_eq!(value(&vars, name), Some(want), "{name} in {vars:?}");
    }
    assert!(value(&vars, "LAYEREVAL").is_some() && value(&vars, "LAYERNOTIFY").is_some(), "{vars:?}");

    // The added objects don't disturb CADCraft's own read of the file: the rectangle is there.
    let back = cadcraft_io::read(&dwg, "rect.dwg").expect("CADCraft reads its own 2004 DWG");
    assert_eq!(back.model.len(), 1);
}

/// The AC1032 (AutoCAD 2018) path is deliberately unchanged: those DWGs read in Vectric Aspire
/// without the dictionary, so no dictionary is injected and the existing output stays as it was.
#[test]
fn an_ac1032_dwg_stays_as_it_was() {
    // 2018 DWGs read fine in Aspire without the dictionary, so the latest format is unchanged.
    let d = rectangle();
    let dwg = write_with(&d, "rect.dwg", &SaveOptions { version: Some(Version::R2018), binary: false }).expect("write the DWG");
    assert!(dwg.starts_with(b"AC1032"));
    let doc = read_dwg(&dwg);
    assert_eq!(variable_dictionary(&doc), None, "no dictionary is added to a 2018 DWG");
}

/// A drawing that already has the dictionary must keep its own entries and values: conversion
/// only supplies AutoCAD's dictionary when it is absent, rather than merging defaults into one
/// that a producer such as ezdxf's `add_dict_var` already wrote.
#[test]
fn a_variable_dictionary_the_drawing_has_is_left_alone() {
    // A drawing that already has the dictionary keeps its own values and entries (here only
    // CMLEADERSTYLE, as ezdxf's add_dict_var("CMLEADERSTYLE", "Standard") writes it): a DWG
    // export must not add defaults next to them.
    let mut source = acadrust::CadDocument::new();
    source.version = acadrust::DxfVersion::AC1015;
    assert!(source.set_drawing_variable("CMLEADERSTYLE", "Notes"));
    let dxf = acadrust::DxfWriter::new(&source).write_to_vec().expect("write a DXF with the dictionary");

    let dwg = cadcraft_dwg::dxf_to_dwg_as(&dxf, "AC1018").expect("write the DWG");
    let doc = read_dwg(&dwg);
    let vars = variable_dictionary(&doc).expect("the drawing's dictionary is kept");
    assert_eq!(vars.len(), 1, "{vars:?}");
    assert_eq!(value(&vars, "CMLEADERSTYLE"), Some("Notes"), "{vars:?}");
}
