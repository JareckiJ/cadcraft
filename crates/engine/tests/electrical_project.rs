//! Electrical projects on real DXF files in a temporary folder: two sheets, a coil on one and
//! its contact on the other, cross-referenced and wire-numbered across the project. Its own test
//! binary, as it installs the file format hooks. Every call names its project: the tests run in
//! parallel and the open project is shared.

use std::path::PathBuf;

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::EntityKind;
use serde_json::json;

fn hooks() {
    set_io(IoHooks {
        read: |b, name| cadcraft_io::read(b, name).map_err(|e| e.to_string()),
        write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
        plot: None,
    });
}

fn folder(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cadcraft-eproject-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn switch_to(s: &mut Session, suffix: &str) {
    let i = s.docs.iter().position(|d| d.path.as_deref().is_some_and(|p| p.ends_with(suffix))).unwrap();
    s.execute("document.switch", &json!({ "index": i })).unwrap();
}

/// The value of attribute `tag` of every block reference of `block` in the active drawing.
fn attr(s: &Session, block: &str, tag: &str) -> Vec<String> {
    s.doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Insert(i) if i.block == block => i.attribs.iter().find(|a| a.tag == tag).map(|a| a.text.value.clone()),
            _ => None,
        })
        .collect()
}

fn wire_numbers(s: &Session) -> Vec<String> {
    let mut v: Vec<String> = s
        .doc()
        .unwrap()
        .model
        .iter()
        .filter(|e| e.common.layer == "WIRENO")
        .filter_map(|e| if let EntityKind::Text(t) = &e.kind { Some(t.value.clone()) } else { None })
        .collect();
    v.sort();
    v
}

#[test]
fn coil_and_contact_on_different_sheets() {
    hooks();
    let dir = folder("two-sheets");
    let project = dir.join("plant.ecproj").to_string_lossy().to_string();
    let mut s = Session::new();
    let r = s.execute("eproject.new", &json!({ "path": project, "sheets": 2 })).unwrap();
    assert_eq!(r["drawings"].as_array().unwrap().len(), 2);
    assert!(dir.join("plant-01.dxf").exists() && dir.join("plant-02.dxf").exists());
    s.execute("eproject.open", &json!({ "path": project })).unwrap();

    // Sheet 1: a ladder and a coil on the second rung.
    switch_to(&mut s, "plant-01.dxf");
    s.execute("eladder", &json!({ "top": 250, "bottom": 50 })).unwrap();
    let k = s.execute("esymbol", &json!({ "name": "IEC_COIL", "at": [63, 150] })).unwrap();
    assert_eq!(k["tag"], "-K102");
    s.execute("qsave", &json!({})).unwrap();

    // Sheet 2: a ladder and the coil's contact on the first rung.
    switch_to(&mut s, "plant-02.dxf");
    s.execute("eladder", &json!({ "top": 250, "bottom": 50 })).unwrap();
    s.execute("esymbol", &json!({ "name": "IEC_NO", "at": [21, 150], "tag": "-K102" })).unwrap();
    s.execute("qsave", &json!({})).unwrap();

    // Cross-references across the sheets.
    let x = s.execute("eproject.xref", &json!({ "project": project })).unwrap();
    assert_eq!(x["orphans"], json!([]), "{x}");
    switch_to(&mut s, "plant-01.dxf");
    assert_eq!(attr(&s, "IEC_COIL", "XREF"), vec!["NO 13-14 /2.1"]);
    switch_to(&mut s, "plant-02.dxf");
    assert_eq!(attr(&s, "IEC_NO", "XREF"), vec!["/1.2"]);

    // Wire numbers: each sheet numbers from its own sheet number, none repeated.
    s.execute("eproject.wires", &json!({ "project": project })).unwrap();
    switch_to(&mut s, "plant-01.dxf");
    let one = wire_numbers(&s);
    switch_to(&mut s, "plant-02.dxf");
    let two = wire_numbers(&s);
    assert!(!one.is_empty() && !two.is_empty());
    assert!(one.iter().all(|n| n.starts_with('1')) && two.iter().all(|n| n.starts_with('2')), "{one:?} {two:?}");

    // Everything was saved: the contact's reference is in the file.
    let bytes = std::fs::read(dir.join("plant-02.dxf")).unwrap();
    let d = cadcraft_io::read(&bytes, "plant-02.dxf").unwrap();
    let saved: Vec<String> = d
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Insert(i) => i.attribs.iter().find(|a| a.tag == "XREF").map(|a| a.text.value.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(saved, vec!["/1.2"]);

    // The project database: every symbol on both sheets, the contact linked to its coil.
    let r = s.execute("eproject.sync", &json!({ "project": project })).unwrap();
    assert_eq!(r["stats"]["pages"], 2, "{r}");
    assert!(r["stats"]["devices"].as_u64().unwrap() >= 2);
    assert!(dir.join("plant.ecdb").exists());

    // A new sheet joins the project as sheet 3, framed and open.
    let a = s.execute("eproject.addsheet", &json!({ "project": project })).unwrap();
    assert_eq!(a["sheet"], 3);
    assert!(dir.join("plant-03.dxf").exists());
    let l = s.execute("eproject.list", &json!({ "project": project })).unwrap();
    assert_eq!(l["drawings"].as_array().unwrap().len(), 3);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn renumbering_a_coil_renames_its_contact_on_another_sheet() {
    hooks();
    let dir = folder("renumber");
    let project = dir.join("p.ecproj").to_string_lossy().to_string();
    let mut s = Session::new();
    s.execute("eproject.new", &json!({ "path": project, "sheets": 2 })).unwrap();
    s.execute("eproject.open", &json!({ "path": project })).unwrap();
    switch_to(&mut s, "p-01.dxf");
    s.execute("esymbol", &json!({ "name": "IEC_COIL", "at": [100, 150], "tag": "-K999" })).unwrap();
    s.execute("qsave", &json!({})).unwrap();
    switch_to(&mut s, "p-02.dxf");
    s.execute("esymbol", &json!({ "name": "IEC_NC", "at": [30, 150], "tag": "-K999" })).unwrap();
    s.execute("qsave", &json!({})).unwrap();
    let r = s.execute("eproject.tag", &json!({ "mode": "all", "project": project })).unwrap();
    assert_eq!(r["contactsFollowed"], 1, "{r}");
    switch_to(&mut s, "p-01.dxf");
    assert_eq!(attr(&s, "IEC_COIL", "TAG1"), vec!["-K103"]);
    switch_to(&mut s, "p-02.dxf");
    assert_eq!(attr(&s, "IEC_NC", "TAG2"), vec!["-K103"]);
    let _ = std::fs::remove_dir_all(&dir);
}
