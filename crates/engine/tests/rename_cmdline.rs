//! RENAME: blocks in the JSON form, and -RENAME's type / old name / new name prompts.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Drawing, EntityKind};
use serde_json::json;

/// Names of the blocks every INSERT (model, layouts and block definitions) refers to.
fn insert_targets(d: &Drawing) -> Vec<String> {
    let ents = d.model.iter().chain(d.layouts.iter().flat_map(|l| l.entities.iter())).chain(d.blocks.values().flat_map(|b| b.entities.iter()));
    let mut v: Vec<String> = ents.filter_map(|e| if let EntityKind::Insert(i) = &e.kind { Some(i.block.clone()) } else { None }).collect();
    v.sort();
    v
}

/// Valve (with an attribute) inserted in model space, in Layout1 and nested inside Assy.
fn drawing() -> Session {
    let mut s = Session::new();
    s.execute("circle", &json!({ "center": [0, 0], "radius": 1 })).unwrap();
    s.execute("attdef", &json!({ "tag": "TAG", "at": [0, 2] })).unwrap();
    s.execute("selectall", &json!({})).unwrap();
    s.execute("block", &json!({ "name": "Valve", "base": [0, 0], "keep": "delete" })).unwrap();
    let h =
        s.execute("insert", &json!({ "name": "Valve", "at": [0, 0], "attribs": { "TAG": "V-1" } })).unwrap()["handle"].as_str().unwrap().to_string();
    s.execute("block", &json!({ "name": "Assy", "base": [0, 0], "handles": [h], "keep": "convert" })).unwrap();
    s.execute("insert", &json!({ "name": "Valve", "at": [5, 0], "attribs": { "TAG": "V-2" } })).unwrap();
    s.execute("layout.set", &json!({ "name": "Layout1" })).unwrap();
    s.execute("insert", &json!({ "name": "Valve", "at": [1, 1] })).unwrap();
    s.execute("layout.set", &json!({ "name": "Model" })).unwrap();
    s
}

#[test]
fn json_block_rename_repoints_every_insert() {
    let mut s = drawing();
    assert_eq!(insert_targets(s.doc().unwrap()), ["Assy", "Valve", "Valve", "Valve"]);
    let undo = s.state().unwrap().undo.len();
    s.execute("rename", &json!({ "table": "block", "from": "valve", "to": "Pump" })).unwrap();
    assert_eq!(s.state().unwrap().undo.len(), undo + 1);
    let d = s.doc().unwrap();
    assert!(d.block("Valve").is_none());
    assert_eq!(d.block("Pump").unwrap().name, "Pump");
    assert_eq!(insert_targets(d), ["Assy", "Pump", "Pump", "Pump"]);
    // The attribute values stay on the renamed inserts.
    assert!(d.model.iter().any(|e| matches!(&e.kind, EntityKind::Insert(i) if i.block == "Pump" && i.attribs.iter().any(|a| a.text.value == "V-2"))));
    // Duplicates (any case), invalid names and unknown blocks are refused.
    assert!(s.execute("rename", &json!({ "table": "block", "from": "Pump", "to": "ASSY" })).is_err());
    assert!(s.execute("rename", &json!({ "table": "block", "from": "Pump", "to": "a*b" })).is_err());
    assert!(s.execute("rename", &json!({ "table": "block", "from": "Nope", "to": "X" })).is_err());
    assert_eq!(insert_targets(s.doc().unwrap()), ["Assy", "Pump", "Pump", "Pump"]);
    // Multileader styles: the current-style variable follows; Standard stays.
    s.execute("mleaderstyle", &json!({ "name": "Callout", "current": true })).unwrap();
    s.execute("rename", &json!({ "table": "mleaderstyle", "from": "Callout", "to": "Notes" })).unwrap();
    assert_eq!(s.doc().unwrap().header.str("CMLEADERSTYLE", ""), "Notes");
    assert!(s.execute("rename", &json!({ "table": "mleaderstyle", "from": "Standard", "to": "X" })).is_err());
    // A layer rename is one undo step too.
    s.execute("layer.new", &json!({ "name": "Old" })).unwrap();
    let undo = s.state().unwrap().undo.len();
    s.execute("rename", &json!({ "table": "layer", "from": "Old", "to": "New" })).unwrap();
    assert_eq!(s.state().unwrap().undo.len(), undo + 1);
    // Material, plotstyle, and vport renaming in JSON form.
    s.doc_mut().unwrap().vports.push(cadcraft_doc::VportConfig { name: "Cfg1".into(), tiles: Vec::new() });
    s.execute("rename", &json!({ "table": "vport", "from": "Cfg1", "to": "Cfg2" })).unwrap();
    assert!(s.doc().unwrap().vports.iter().any(|c| c.name == "Cfg2"));
    assert!(s.execute("rename", &json!({ "table": "vport", "from": "*Active", "to": "X" })).is_err());

    s.doc_mut().unwrap().materials.push(cadcraft_doc::Material { name: "Steel".into(), description: String::new() });
    s.execute("rename", &json!({ "table": "material", "from": "Steel", "to": "Iron" })).unwrap();
    assert!(s.doc().unwrap().materials.iter().any(|m| m.name == "Iron"));
    assert!(s.execute("rename", &json!({ "table": "material", "from": "Global", "to": "X" })).is_err());

    let pst = cadcraft_doc::PlotStyleTable::named("custom.stb", "", vec![cadcraft_doc::PlotStyle::object("Pen1")]);
    s.doc_mut().unwrap().plot_style_tables.push(pst);
    s.execute("rename", &json!({ "table": "plotstyle", "from": "Pen1", "to": "Pen2" })).unwrap();
    assert!(s.doc().unwrap().plot_style_tables[0].styles.iter().any(|st| st.name == "Pen2"));
    assert!(s.execute("rename", &json!({ "table": "plotstyle", "from": "Normal", "to": "X" })).is_err());
}

#[test]
fn rename_prompts_type_old_and_new_name() {
    let mut s = drawing();
    s.cmdline("-RENAME").unwrap();
    assert!(
        s.prompt_text().contains("[Block/Dimstyle/LAyer/LType/Material/multileadeRstyle/Plotstyle/textStyle/Tablestyle/Ucs/VIew/VPort]"),
        "{}",
        s.prompt_text()
    );
    s.cancel();
    s.execute("layer.new", &json!({ "name": "Old" })).unwrap();
    s.doc_mut().unwrap().vports.push(cadcraft_doc::VportConfig { name: "ViewCfg".into(), tiles: Vec::new() });
    s.script("-RENAME B Valve Pump\n-RENAME LA Old New\n-RENAME VP ViewCfg NewCfg\nLINE 0,0 1,0\n\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let d = s.doc().unwrap();
    assert!(d.block("Pump").is_some() && d.layer("New").is_some() && d.layer("Old").is_none(), "{:?}", s.log);
    assert!(d.vports.iter().any(|c| c.name == "NewCfg"));
    assert_eq!(insert_targets(d), ["Assy", "Pump", "Pump", "Pump"]);
    assert_eq!(d.model.iter().filter(|e| matches!(e.kind, EntityKind::Line(_))).count(), 1);
    // An unknown old name re-prompts.
    s.script("-RENAME LA Missing\nNew\nNewer\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(s.doc().unwrap().layer("Newer").is_some());
}
