//! Original CADCraft FF1 fixtures, generated in memory; no third-party binary/source assets.
//!
//! Before the FF1 carry, an edited save of these sources lost both custom records.
//! The DXF records follow the public DXF Reference (CLASS, common entities, DICTIONARY,
//! group-code value types); the DWG builder uses unmodified acadrust's public writer APIs.
//! Native-reader assertions share that codec and are not independent AutoCAD validation.
//! These payloads are test data only, not a production raw-byte preservation mechanism.

use std::sync::Arc;

use acadrust::classes::DxfClass;
use acadrust::entities::{Line, UnknownEntity};
use acadrust::io::dwg::{DwgVersion, dwg_reference_type::DwgReferenceType, dwg_stream_writers::DwgMergedWriter};
use acadrust::objects::ObjectType;
use acadrust::{CadDocument, DwgWriter, DxfVersion, EntityType, Handle};

pub type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Copy, Debug)]
pub enum CustomRecords {
    EntityOnly,
    ObjectOnly,
    Combined,
}

pub struct OpaqueRecord {
    pub name: &'static str,
    pub handle: Handle,
    pub owner: Handle,
    pub class_number: i16,
    pub raw: Arc<[u8]>,
    pub handle_bits: i64,
}

pub struct DwgSource {
    pub bytes: Arc<[u8]>,
    pub line: Handle,
    pub entity: Option<OpaqueRecord>,
    pub object: Option<OpaqueRecord>,
}

/// AC1032 with one ordinary LINE and selected custom-class records. No I/O or bridge involved.
pub fn dwg_source(records: CustomRecords) -> TestResult<DwgSource> {
    let version = DxfVersion::AC1032;
    let mut doc = CadDocument::with_version(version);
    let line = doc.add_entity(EntityType::Line(Line::from_coords(1., 2., 0., 11., 12., 0.)))?;
    let mut entity = None;
    let mut object = None;
    for is_entity in [true, false] {
        if matches!((records, is_entity), (CustomRecords::EntityOnly, false) | (CustomRecords::ObjectOnly, true)) {
            continue;
        }
        let name = if is_entity { "FF1_WIDGET" } else { "FF1_OBJECT" };
        let mut class = if is_entity { DxfClass::new_entity(name, format!("Rust{name}")) } else { DxfClass::new(name, format!("Rust{name}")) };
        class.application_name = "FF1_SYNTHETIC".into();
        doc.classes.add_or_update(class);
        let class_number = doc.classes.get_by_name(name).ok_or("missing custom class")?.class_number;
        let handle = doc.allocate_handle();
        let owner = if is_entity { doc.header.model_space_block_handle } else { doc.header.named_objects_dict_handle };
        let mut writer = DwgMergedWriter::new(DwgVersion::from_dxf_version(version)?, version);
        writer.write_object_type(class_number);
        writer.main_mut().write_handle_undefined(handle.value());
        writer.write_bit_short(0); // No EED.
        if is_entity {
            writer.write_bit(false); // No proxy graphics.
            writer.main_mut().write_2bits(2); // Model space: implicit owner.
            writer.write_bit_long(0); // No reactors.
            writer.write_bit(true); // No extension dictionary.
            writer.write_bit(false); // No binary data.
            writer.write_en_color(&acadrust::Color::ByLayer, &acadrust::types::Transparency::ByLayer);
            writer.write_bit_double(1.);
            writer.write_handle(DwgReferenceType::HardPointer, doc.layers.get("0").ok_or("missing layer 0")?.handle.value());
            writer.main_mut().write_2bits(0); // ByLayer linetype.
            writer.main_mut().write_2bits(0); // ByLayer material.
            writer.write_byte(0); // Shadow flags.
            writer.main_mut().write_2bits(0); // ByLayer plot style.
            for _ in 0..3 {
                writer.write_bit(false);
            } // No visual styles.
            writer.write_bit_short(0); // Invisible flag.
            writer.write_byte(29); // Default lineweight.
        } else {
            writer.write_handle(DwgReferenceType::SoftPointer, owner.value());
            writer.write_bit_long(0);
            writer.write_bit(true);
            writer.write_bit(false);
        }
        // Intentionally non-byte-aligned custom data, separate text and typed reference stream.
        let custom_bits = if is_entity {
            [true, false, true, true, false, true, false, false, true, false, true, false, true]
        } else {
            [true, true, false, true, false, false, true, true, false, true, false, true, true]
        };
        for bit in custom_bits {
            writer.write_bit(bit);
        }
        writer.write_variable_text(if is_entity { "FF1_ENTITY_UNIQUE" } else { "FF1_OBJECT_UNIQUE" });
        writer.write_handle(DwgReferenceType::HardPointer, line.value());
        let raw = writer.merge();
        let handle_bits = i64::try_from(raw.len())?.checked_mul(8).ok_or("payload length overflow")? - writer.handle_start_bits();
        let expected = OpaqueRecord { name, handle, owner, class_number, raw: Arc::from(raw.clone()), handle_bits };
        if is_entity {
            let mut unknown = UnknownEntity::new(name);
            unknown.common.handle = handle;
            unknown.common.owner_handle = owner;
            unknown.raw_dwg_data = Some(raw);
            unknown.dwg_handle_bits = handle_bits;
            unknown.dwg_type_code = class_number;
            unknown.dwg_source_version = Some(version);
            doc.add_entity(EntityType::Unknown(unknown))?;
            entity = Some(expected);
        } else {
            doc.objects.insert(
                handle,
                ObjectType::Unknown {
                    type_name: name.into(),
                    handle,
                    owner,
                    raw_dxf_codes: None,
                    raw_dwg_data: Some(raw),
                    raw_dwg_handle_bits: handle_bits,
                    raw_dwg_version: Some(version),
                },
            );
            let Some(ObjectType::Dictionary(nod)) = doc.objects.get_mut(&owner) else { return Err("missing NOD".into()) };
            nod.entries.push((name.into(), handle));
            object = Some(expected);
        }
    }
    Ok(DwgSource { bytes: Arc::from(DwgWriter::write_to_vec(&doc)?), line, entity, object })
}

/// Small native DXF counterpart with literal payloads, CLASS records, owner and hard pointers.
/// Kept separate from DWG generation so the native DXF path has no dependency conversion.
pub fn dxf_source(records: CustomRecords) -> String {
    let mut src = String::from("0 SECTION\n2 HEADER\n9 $ACADVER\n1 AC1032\n9 $HANDSEED\n5 100\n0 ENDSEC\n0 SECTION\n2 CLASSES\n");
    for (name, cpp, graphical) in [("FF1_WIDGET", "RustFF1_WIDGET", true), ("FF1_OBJECT", "RustFF1_OBJECT", false)] {
        if matches!((records, graphical), (CustomRecords::EntityOnly, false) | (CustomRecords::ObjectOnly, true)) {
            continue;
        }
        src.push_str(&format!("0 CLASS\n1 {name}\n2 {cpp}\n3 FF1_SYNTHETIC\n90 0\n280 0\n281 {}\n", u8::from(graphical)));
    }
    src.push_str("0 ENDSEC\n0 SECTION\n2 TABLES\n0 TABLE\n2 BLOCK_RECORD\n5 1\n330 0\n70 1\n0 BLOCK_RECORD\n5 1F\n330 1\n100 AcDbSymbolTableRecord\n100 AcDbBlockTableRecord\n2 *Model_Space\n70 0\n0 ENDTAB\n0 ENDSEC\n0 SECTION\n2 BLOCKS\n0 BLOCK\n5 20\n330 1F\n100 AcDbEntity\n8 0\n100 AcDbBlockBegin\n2 *Model_Space\n70 0\n10 0\n20 0\n30 0\n3 *Model_Space\n1\n0 ENDBLK\n5 21\n330 1F\n100 AcDbEntity\n8 0\n100 AcDbBlockEnd\n0 ENDSEC\n0 SECTION\n2 ENTITIES\n0 LINE\n5 2A\n330 1F\n100 AcDbEntity\n8 0\n100 AcDbLine\n10 1\n20 2\n30 0\n11 11\n21 12\n31 0\n");
    if !matches!(records, CustomRecords::ObjectOnly) {
        src.push_str("0 FF1_WIDGET\n5 30\n330 1F\n100 AcDbEntity\n8 0\n100 RustFF1_WIDGET\n1 FF1_ENTITY_UNIQUE\n310 B4A8\n90 13\n340 2A\n");
    }
    src.push_str("0 ENDSEC\n0 SECTION\n2 OBJECTS\n0 DICTIONARY\n5 C\n330 0\n100 AcDbDictionary\n281 1\n");
    if !matches!(records, CustomRecords::EntityOnly) {
        src.push_str("3 FF1_OBJECT\n350 42\n0 FF1_OBJECT\n5 42\n330 C\n100 RustFF1_OBJECT\n1 FF1_OBJECT_UNIQUE\n310 D358\n90 13\n340 2A\n");
    }
    src.push_str("0 ENDSEC\n0 EOF\n");
    src.lines()
        .map(|line| {
            let (code, value) = line.split_once(' ').unwrap_or((line, ""));
            format!("{code}\n{value}\n")
        })
        .collect()
}

/// One opaque record of [`DisplaySource`]: its handle, owner and type.
#[allow(dead_code)] // each test crate that includes this support module reads what it needs
pub struct DisplayRecord {
    pub name: &'static str,
    pub handle: Handle,
    pub owner: Handle,
}

/// An AC1032 source shaped like another program's display configuration (the Architectural
/// Desktop objects of real drawings): subtrees of the named object dictionary made of
/// dictionaries and opaque records that refer to layer 0, linetype ByBlock and the Standard text
/// style, name application ACAD in their extended data, refer into another subtree, and own a
/// dictionary with an XRECORD. One subtree also refers to a handle with no role, so it can't be
/// carried. Only acadrust's public writers; no third-party data.
#[allow(dead_code)] // each test crate that includes this support module reads what it needs
pub struct DisplaySource {
    pub bytes: Arc<[u8]>,
    pub line: Handle,
    pub props: DisplayRecord,
    pub rep: DisplayRecord,
    pub vars: DisplayRecord,
    pub odd: DisplayRecord,
    /// The dictionary `vars` owns, and the XRECORD in it.
    pub owned: (Handle, Handle),
}

/// Handle with no object in the source (the odd record refers to it).
#[allow(dead_code)]
pub const NO_ROLE: u64 = 0xF_FFFF;

/// Build the [`DisplaySource`] DWG.
#[allow(dead_code)]
pub fn display_source() -> TestResult<DisplaySource> {
    use acadrust::objects::{Dictionary, XRecord, XRecordEntry, XRecordValue};
    let version = DxfVersion::AC1032;
    let mut doc = CadDocument::with_version(version);
    let line = doc.add_entity(EntityType::Line(Line::from_coords(1., 2., 0., 11., 12., 0.)))?;
    let nod = doc.header.named_objects_dict_handle;
    let layer0 = doc.layers.get("0").ok_or("layer 0")?.handle;
    let byblock = doc.line_types.get("ByBlock").ok_or("ByBlock")?.handle;
    let standard = doc.text_styles.get("Standard").ok_or("Standard")?.handle;
    let acad = doc.app_ids.get("ACAD").ok_or("APPID ACAD")?.handle;
    let class_of = |doc: &mut CadDocument, name: &str| -> TestResult<i16> {
        let mut class = DxfClass::new(name, format!("Rust{name}"));
        class.application_name = "FF1_DISPLAY".into();
        doc.classes.add_or_update(class);
        Ok(doc.classes.get_by_name(name).ok_or("class")?.class_number)
    };
    let (c_props, c_rep, c_vars, c_odd) =
        (class_of(&mut doc, "FF1_DISP_PROPS")?, class_of(&mut doc, "FF1_DISP_REP")?, class_of(&mut doc, "FF1_VARS")?, class_of(&mut doc, "FF1_ODD")?);
    // A dictionary under the named object dictionary.
    let root = |doc: &mut CadDocument, key: &str| -> TestResult<Handle> {
        let mut d = Dictionary::new();
        d.handle = doc.allocate_handle();
        d.owner = nod;
        let h = d.handle;
        doc.objects.insert(h, ObjectType::Dictionary(d));
        let Some(ObjectType::Dictionary(n)) = doc.objects.get_mut(&nod) else { return Err("NOD".into()) };
        n.entries.push((key.into(), h));
        Ok(h)
    };
    let (d_props, d_reps, d_vars, d_odd) =
        (root(&mut doc, "FF1_PROPS")?, root(&mut doc, "FF1_REPS")?, root(&mut doc, "FF1_VARS")?, root(&mut doc, "FF1_ODD")?);
    let (props, rep, vars, odd) = (doc.allocate_handle(), doc.allocate_handle(), doc.allocate_handle(), doc.allocate_handle());
    let owned = doc.allocate_handle();
    let xrec = doc.allocate_handle();
    // One opaque record: type, own handle, EED (application ACAD, 3 bytes), owner, no reactors,
    // no extension dictionary, data bits, text, then its references.
    let record = |name: &str, class: i16, handle: Handle, owner: Handle, refs: &[(DwgReferenceType, u64)]| -> TestResult<(Vec<u8>, i64)> {
        let mut w = DwgMergedWriter::new(DwgVersion::from_dxf_version(version)?, version);
        w.write_object_type(class);
        w.main_mut().write_handle_undefined(handle.value());
        w.write_bit_short(3);
        w.main_mut().write_handle_undefined(acad.value());
        w.write_bytes(&[0xAB, 0xCD, 0xEF]);
        w.write_bit_short(0);
        w.write_handle(DwgReferenceType::SoftPointer, owner.value());
        w.write_bit_long(0);
        w.write_bit(true);
        w.write_bit(false);
        for bit in [true, false, true, true, false, true, false, false, true] {
            w.write_bit(bit);
        }
        w.write_variable_text(&format!("{name}_UNIQUE"));
        for (kind, h) in refs {
            w.write_handle(*kind, *h);
        }
        let raw = w.merge();
        let bits = i64::try_from(raw.len())?.checked_mul(8).ok_or("overflow")? - w.handle_start_bits();
        Ok((raw, bits))
    };
    use DwgReferenceType::{HardOwnership, HardPointer, SoftPointer};
    let records = [
        (
            "FF1_DISP_PROPS",
            c_props,
            props,
            d_props,
            "*A1",
            vec![(HardPointer, layer0.value()), (HardPointer, byblock.value()), (SoftPointer, standard.value())],
        ),
        ("FF1_DISP_REP", c_rep, rep, d_reps, "Rep", vec![(SoftPointer, props.value())]),
        ("FF1_VARS", c_vars, vars, d_vars, "Setup", vec![(HardOwnership, owned.value()), (HardPointer, line.value())]),
        ("FF1_ODD", c_odd, odd, d_odd, "Odd", vec![(HardPointer, NO_ROLE)]),
    ];
    for (name, class, handle, owner, key, refs) in records {
        let (raw, bits) = record(name, class, handle, owner, &refs)?;
        doc.objects.insert(
            handle,
            ObjectType::Unknown {
                type_name: name.into(),
                handle,
                owner,
                raw_dxf_codes: None,
                raw_dwg_data: Some(raw),
                raw_dwg_handle_bits: bits,
                raw_dwg_version: Some(version),
            },
        );
        let Some(ObjectType::Dictionary(d)) = doc.objects.get_mut(&owner) else { return Err("root".into()) };
        d.entries.push((key.into(), handle));
    }
    // What the vars record owns: a dictionary with an XRECORD.
    let mut d = Dictionary::new();
    d.handle = owned;
    d.owner = vars;
    d.entries.push(("History".into(), xrec));
    doc.objects.insert(owned, ObjectType::Dictionary(d));
    let mut x = XRecord::new();
    x.handle = xrec;
    x.owner = owned;
    x.entries = vec![XRecordEntry::new(1, XRecordValue::String("FF1_HISTORY_UNIQUE".into()))];
    doc.objects.insert(xrec, ObjectType::XRecord(x));
    let rec = |name, handle, owner| DisplayRecord { name, handle, owner };
    Ok(DisplaySource {
        bytes: Arc::from(DwgWriter::write_to_vec(&doc)?),
        line,
        props: rec("FF1_DISP_PROPS", props, d_props),
        rep: rec("FF1_DISP_REP", rep, d_reps),
        vars: rec("FF1_VARS", vars, d_vars),
        odd: rec("FF1_ODD", odd, d_odd),
        owned: (owned, xrec),
    })
}
