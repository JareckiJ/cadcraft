//! FF1 preservation matrix: original synthetic sources, two edited saves, loss reports and bounded output.
//! DWG source/output oracles share acadrust 0.6.5; no independent AutoCAD verification is claimed.
#![cfg(not(target_arch = "wasm32"))]

#[path = "support/ff1.rs"]
mod ff1;
#[path = "support/ff1_metadata.rs"]
mod metadata;

use std::io::Cursor;

use acadrust::io::dwg::dwg_reference_type::DwgReferenceType;
use acadrust::io::dwg::dwg_stream_readers::{handle_reader::read_handles, object_reader::DwgObjectReader};
use acadrust::objects::ObjectType;
use acadrust::{DwgReader, DxfVersion, EntityType};
use ff1::{CustomRecords, TestResult};

/// A save that must succeed, with its loss report.
fn saved(doc: &cadcraft_doc::Drawing, path: &str, opts: &cadcraft_io::SaveOptions) -> TestResult<(Vec<u8>, Option<cadcraft_io::LossReport>)> {
    Ok(cadcraft_io::write_with_report(doc, path, opts)?)
}

fn lost_names(r: &cadcraft_io::LossReport) -> Vec<&str> {
    r.records.iter().filter_map(|x| x.record.as_deref()).collect()
}

/// The bits of an R2010+ record before its handle stream: its data, which carrying it never moves
/// unless a reference in it (an extended-data application) must.
fn data_bits(raw: &[u8], handle_bits: i64) -> Vec<bool> {
    let n = (raw.len() * 8).saturating_sub(usize::try_from(handle_bits).unwrap_or(0));
    (0..n).map(|i| raw.get(i / 8).is_some_and(|b| (b >> (7 - (i % 8))) & 1 == 1)).collect()
}

#[test]
fn generated_dwg_sources_contain_exact_opaque_records_before_the_bridge() -> TestResult {
    for variant in [CustomRecords::EntityOnly, CustomRecords::ObjectOnly, CustomRecords::Combined] {
        let source = ff1::dwg_source(variant)?;
        let outcome = DwgReader::from_stream(Cursor::new(source.bytes.as_ref())).read_with_stats()?;
        assert_eq!(outcome.stats.skipped_source_records, 0, "{variant:?}");
        let doc = outcome.document;
        let Some(EntityType::Line(line)) = doc.get_entity(source.line) else { return Err("source LINE absent".into()) };
        assert_eq!((line.start.x, line.start.y, line.end.x, line.end.y), (1., 2., 11., 12.));
        assert_eq!(doc.entities().filter(|e| matches!(e, EntityType::Line(_))).count(), 1);
        assert_eq!(doc.entities().filter(|e| matches!(e, EntityType::Unknown(_))).count(), usize::from(source.entity.is_some()));
        assert_eq!(doc.objects.values().filter(|o| matches!(o, ObjectType::Unknown { .. })).count(), usize::from(source.object.is_some()));

        let mut reader = DwgReader::from_stream(Cursor::new(source.bytes.as_ref()));
        let info = reader.read_file_header()?;
        assert_eq!(info.version_string, "AC1032");
        let handles = read_handles(&reader.get_section_buffer("AcDb:Handles", &info)?)?;
        let objects = reader.get_section_buffer("AcDb:AcDbObjects", &info)?;
        let raw_reader = DwgObjectReader::new(objects, DxfVersion::AC1032, handles)?;
        for (is_entity, expected) in [(true, source.entity.as_ref()), (false, source.object.as_ref())] {
            let Some(expected) = expected else { continue };
            let class = doc.classes.get_by_name(expected.name).ok_or("source CLASS absent")?;
            assert_eq!(class.class_number, expected.class_number);
            assert_eq!(class.cpp_class_name, format!("Rust{}", expected.name));
            assert_eq!(class.application_name, "FF1_SYNTHETIC");
            assert_eq!(class.is_an_entity, is_entity);
            if is_entity {
                let Some(EntityType::Unknown(u)) = doc.get_entity(expected.handle) else { return Err("source unknown entity absent".into()) };
                assert_eq!(u.dxf_name, "FF1_WIDGET");
                assert_eq!(u.common.owner_handle, expected.owner);
                assert_eq!(u.raw_dwg_data.as_deref(), Some(expected.raw.as_ref()));
                assert_eq!(u.dwg_handle_bits, expected.handle_bits);
                assert_eq!(u.dwg_type_code, expected.class_number);
                assert_eq!(u.dwg_source_version, Some(DxfVersion::AC1032));
            } else {
                let Some(ObjectType::Unknown { type_name, handle, owner, raw_dwg_data, raw_dwg_handle_bits, raw_dwg_version, .. }) =
                    doc.objects.get(&expected.handle)
                else {
                    return Err("source unknown object absent".into());
                };
                assert_eq!(type_name, "FF1_OBJECT");
                assert_eq!(*handle, expected.handle);
                assert_eq!(*owner, expected.owner);
                assert_eq!(raw_dwg_data.as_deref(), Some(expected.raw.as_ref()));
                assert_eq!(*raw_dwg_handle_bits, expected.handle_bits);
                assert_eq!(*raw_dwg_version, Some(DxfVersion::AC1032));
                let Some(ObjectType::Dictionary(nod)) = doc.objects.get(owner) else { return Err("source owner dictionary absent".into()) };
                assert_eq!(nod.entries.iter().filter(|(name, h)| name == "FF1_OBJECT" && *h == expected.handle).count(), 1);
            }
            // Inspect actual source streams, not the bytes generated on the builder's side.
            let offset = usize::try_from(raw_reader.offset_for(expected.handle.value()).ok_or("source offset absent")?)?;
            let (type_code, mut stream) = raw_reader.read_record_at(offset)?;
            assert_eq!(type_code, class.class_number);
            let (handle, owner) = if is_entity {
                let common = raw_reader.read_common_entity_data(&mut stream, type_code);
                // Model-space owners are implicit in the DWG entity preamble.
                assert_eq!(common.entity_mode, 2);
                (common.common.handle, doc.header.model_space_block_handle.value())
            } else {
                let common = raw_reader.read_common_non_entity_data(&mut stream, type_code);
                (common.common.handle, common.owner_handle)
            };
            assert_eq!((handle, owner), (expected.handle.value(), expected.owner.value()));
            let bits: Vec<_> = (0..13).map(|_| stream.read_bit()).collect();
            assert_eq!(
                bits,
                if is_entity {
                    [true, false, true, true, false, true, false, false, true, false, true, false, true]
                } else {
                    [true, true, false, true, false, false, true, true, false, true, false, true, true]
                }
            );
            assert_eq!(stream.read_variable_text(), if is_entity { "FF1_ENTITY_UNIQUE" } else { "FF1_OBJECT_UNIQUE" });
            assert_eq!(stream.read_typed_handle(), (source.line.value(), DwgReferenceType::HardPointer));
            stream.check_error()?;
        }
        for name in ["FF1_WIDGET", "FF1_OBJECT"] {
            assert_eq!(
                doc.classes.iter().filter(|c| c.dxf_name == name).count(),
                usize::from(if name == "FF1_WIDGET" { source.entity.is_some() } else { source.object.is_some() })
            );
        }
    }
    Ok(())
}

#[test]
fn native_dxf_builders_have_classes_payloads_owners_and_line_references() -> TestResult {
    use cadcraft_doc::{EntityKind, Handle};
    for variant in [CustomRecords::EntityOnly, CustomRecords::ObjectOnly, CustomRecords::Combined] {
        let text = ff1::dxf_source(variant);
        let doc = cadcraft_io::read_dxf(text.as_bytes())?;
        let entity = !matches!(variant, CustomRecords::ObjectOnly);
        let object = !matches!(variant, CustomRecords::EntityOnly);
        assert_eq!(doc.foreign.classes.len(), usize::from(entity) + usize::from(object));
        for (name, cpp, graphical, present) in [("FF1_WIDGET", "RustFF1_WIDGET", "1", entity), ("FF1_OBJECT", "RustFF1_OBJECT", "0", object)] {
            let classes: Vec<_> = doc.foreign.classes.iter().filter(|c| c.iter().any(|t| t.code == 1 && t.value == name)).collect();
            assert_eq!(classes.len(), usize::from(present));
            if let Some(class) = classes.first() {
                for (code, value) in [(2, cpp), (3, "FF1_SYNTHETIC"), (281, graphical)] {
                    assert!(class.iter().any(|t| t.code == code && t.value == value));
                }
            }
        }
        let Some(EntityKind::Line(line)) = doc.model.get(Handle(0x2A)).map(|e| &e.kind) else { return Err("DXF source LINE absent".into()) };
        assert_eq!((line.a.x, line.a.y, line.b.x, line.b.y), (1., 2., 11., 12.));
        // Owner tags are interpreted by the DXF reader; inspect the original record as well.
        let tags = cadcraft_dxf::parse_ascii(&text)?;
        let entities = cadcraft_dxf::sections(&tags).into_iter().find(|section| section.name == "ENTITIES").ok_or("DXF ENTITIES absent")?;
        if entity {
            let Some(EntityKind::Unknown(unknown)) = doc.model.get(Handle(0x30)).map(|e| &e.kind) else {
                return Err("DXF source entity absent".into());
            };
            assert_eq!(unknown.dxf_type, "FF1_WIDGET");
            let (_, raw) = cadcraft_dxf::records(&entities.tags).into_iter().find(|(kind, _)| kind == "FF1_WIDGET").ok_or("DXF raw entity absent")?;
            assert!(raw.iter().any(|t| t.code == 330 && t.str() == "1F"));
            for (code, value) in [(1, "FF1_ENTITY_UNIQUE"), (310, "B4A8"), (90, "13"), (340, "2A")] {
                assert!(unknown.tags.iter().any(|t| t.code == code && t.value == value));
            }
        }
        assert_eq!(doc.foreign.objects.len(), usize::from(object));
        if object {
            let obj = &doc.foreign.objects[0];
            assert_eq!((obj.kind.as_str(), obj.handle), ("FF1_OBJECT", Handle(0x42)));
            for (code, value) in [(330, "C"), (1, "FF1_OBJECT_UNIQUE"), (310, "D358"), (90, "13"), (340, "2A")] {
                assert!(obj.tags.iter().any(|t| t.code == code && t.value == value));
            }
            assert_eq!(doc.foreign.dictionary, [("FF1_OBJECT".into(), Handle(0x42))]);
        }
        assert!(!doc.preservation.blocks_save());
    }
    Ok(())
}

/// DXF→DWG leaves out another program's records the DWG can't hold, but never silently: the loss
/// report names each, with its DXF groups (the bridge used to delete them and report success).
#[test]
fn custom_records_are_left_out_of_a_dwg_and_reported_with_their_groups() -> TestResult {
    for variant in [CustomRecords::EntityOnly, CustomRecords::ObjectOnly, CustomRecords::Combined] {
        let doc = cadcraft_io::read_dxf(ff1::dxf_source(variant).as_bytes())?;
        for version in [cadcraft_dxf::Version::R2004, cadcraft_dxf::Version::R2007, cadcraft_dxf::Version::R2018] {
            let opts = cadcraft_io::SaveOptions { version: Some(version), ..Default::default() };
            let (bytes, report) = saved(&doc, "out.dwg", &opts)?;
            let report = report.ok_or("no loss report")?;
            assert_eq!(report.target, format!("DWG {}", version.code()));
            let mut expected = Vec::new();
            if !matches!(variant, CustomRecords::ObjectOnly) {
                expected.push("FF1_WIDGET");
            }
            if !matches!(variant, CustomRecords::EntityOnly) {
                expected.push("FF1_OBJECT");
            }
            assert_eq!(lost_names(&report), expected, "{variant:?} {version:?}");
            for r in &report.records {
                assert!(r.dxf.iter().any(|(c, v)| *c == 1 && v.ends_with("_UNIQUE")), "{r:?}");
            }
            // The rest is a DWG that reads back, with the LINE and without the custom records.
            let native = DwgReader::from_stream(Cursor::new(&bytes)).read()?;
            assert_eq!(native.entities().filter(|e| matches!(e, EntityType::Line(_))).count(), 1);
            assert!(!native.objects.values().any(|o| matches!(o, ObjectType::Unknown { .. })));
        }
        // DXF keeps them: nothing to report.
        assert!(saved(&doc, "out.dxf", &Default::default())?.1.is_none());
    }
    Ok(())
}

#[test]
fn the_bridge_refuses_custom_records_it_cannot_write_as_dwg() {
    for variant in [CustomRecords::EntityOnly, CustomRecords::ObjectOnly, CustomRecords::Combined] {
        let dxf = ff1::dxf_source(variant);
        let err = cadcraft_dwg::dxf_to_dwg(dxf.as_bytes()).expect_err(&format!("{variant:?}"));
        if !matches!(variant, CustomRecords::ObjectOnly) {
            assert!(err.contains("FF1_WIDGET"), "{variant:?}: {err}");
        }
        if !matches!(variant, CustomRecords::EntityOnly) {
            assert!(err.contains("FF1_OBJECT"), "{variant:?}: {err}");
        }
        assert!(cadcraft_dwg::dxf_to_dwg_as(dxf.as_bytes(), "AC1027").is_err());
    }
}

/// Object-only customs are reported from the drawing inventory even though there is no entity
/// warning to stand in for them (the old path only listed Unknown entities after a successful write).
#[test]
fn object_only_customs_are_reported_from_foreign_inventory() -> TestResult {
    let doc = cadcraft_io::read_dxf(ff1::dxf_source(CustomRecords::ObjectOnly).as_bytes())?;
    assert!(doc.model.iter().all(|e| !matches!(e.kind, cadcraft_doc::EntityKind::Unknown(_))));
    assert_eq!(doc.foreign.objects.iter().map(|o| o.kind.as_str()).collect::<Vec<_>>(), ["FF1_OBJECT"]);
    let report = saved(&doc, "out.dwg", &Default::default())?.1.ok_or("no loss report")?;
    assert_eq!(lost_names(&report), ["FF1_OBJECT"]);
    assert_eq!(report.records.first().map(|r| r.kind), Some("object"));
    assert!(saved(&doc, "out.dxf", &Default::default())?.1.is_none());
    Ok(())
}

fn edit_line(doc: &mut cadcraft_doc::Drawing, handle: cadcraft_doc::Handle) -> TestResult {
    doc.modify_entity(handle, |e| {
        if let cadcraft_doc::EntityKind::Line(l) = &mut e.kind {
            l.b.x += 5.0;
        }
    })?;
    Ok(())
}

fn native_import(variant: CustomRecords) -> TestResult<(cadcraft_doc::Drawing, cadcraft_doc::Handle)> {
    let source = ff1::dwg_source(variant)?;
    let doc = cadcraft_io::read(source.bytes.as_ref(), "source.dwg")?;
    let line = doc.model.iter().find(|e| matches!(e.kind, cadcraft_doc::EntityKind::Line(_))).map(|e| e.handle).ok_or("imported LINE absent")?;
    Ok((doc, line))
}

/// The convenience string writer writes what the byte writer writes; the byte writer's report
/// keeps each lost record's own DWG bytes, so nothing is gone for good.
#[test]
fn public_dxf_writers_save_incomplete_native_imports_and_the_report_keeps_their_bytes() -> TestResult {
    for variant in [CustomRecords::EntityOnly, CustomRecords::ObjectOnly, CustomRecords::Combined] {
        let source = ff1::dwg_source(variant)?;
        let (mut doc, line) = native_import(variant)?;
        for _ in 0..2 {
            edit_line(&mut doc, line)?;
            let before = doc.clone();
            let text = cadcraft_io::write_dxf(&doc);
            let legacy = cadcraft_io::SaveOptions { version: Some(cadcraft_io::Version::R2000), binary: false };
            let (bytes, report) = saved(&doc, "out.dxf", &legacy)?;
            assert_eq!(text.as_bytes(), bytes);
            let report = report.ok_or("no loss report")?;
            for expected in [&source.entity, &source.object].into_iter().flatten() {
                let r = report.records.iter().find(|r| r.record.as_deref() == Some(expected.name)).ok_or("record not reported")?;
                let raw = r.dwg.as_ref().ok_or("the record's bytes aren't kept")?;
                assert_eq!((raw.version.as_str(), raw.handle_bits, raw.bytes.as_slice()), ("AC1032", expected.handle_bits, expected.raw.as_ref()));
                assert_eq!(r.handle.as_deref(), Some(format!("{:X}", expected.handle.value()).as_str()));
                assert_eq!(r.class.as_ref().map(|c| c.2.as_str()), Some("FF1_SYNTHETIC"));
            }
            assert!(report.to_json().contains("data_base64"));
            assert_eq!(doc, before, "serialization changed the drawing");
        }
    }
    Ok(())
}

#[test]
fn public_dxf_writers_keep_native_dxf_payloads_classes_and_references_identically() -> TestResult {
    use cadcraft_doc::{EntityKind, Handle, RawTag};
    for version in [cadcraft_io::Version::R2004, cadcraft_io::Version::R2007, cadcraft_io::Version::R2018] {
        for binary in [false, true] {
            for variant in [CustomRecords::EntityOnly, CustomRecords::ObjectOnly, CustomRecords::Combined] {
                let mut doc = cadcraft_io::read_dxf(ff1::dxf_source(variant).as_bytes())?;
                let classes = doc.foreign.classes.clone();
                for _ in 0..2 {
                    let line = doc.model.iter().find(|e| matches!(e.kind, EntityKind::Line(_))).map(|e| e.handle).ok_or("LINE absent")?;
                    edit_line(&mut doc, line)?;
                    let expected_line = doc.model.get(line).ok_or("edited LINE absent")?.kind.clone();
                    let text = cadcraft_io::write_dxf(&doc);
                    let legacy = cadcraft_io::SaveOptions { version: Some(cadcraft_io::Version::R2000), binary: false };
                    assert_eq!(text.as_bytes(), cadcraft_io::write_with(&doc, "out.dxf", &legacy)?);
                    let opts = cadcraft_io::SaveOptions { version: Some(version), binary };
                    let bytes = cadcraft_io::write_with(&doc, "out.dxf", &opts)?;
                    assert_eq!(bytes.starts_with(b"AutoCAD Binary DXF"), binary);
                    doc = cadcraft_io::read_dxf(&bytes)?;
                    assert_eq!(cadcraft_io::save_version(&doc), version);
                    let line = doc.model.iter().find(|e| matches!(e.kind, EntityKind::Line(_))).ok_or("saved LINE absent")?;
                    assert_eq!(line.kind, expected_line, "known edit was not saved");
                    assert_eq!(doc.foreign.classes.len(), classes.len());
                    for (saved, original) in doc.foreign.classes.iter().zip(&classes) {
                        // The writer adds the current instance count (91); all source CLASS tags stay.
                        assert_eq!(saved.iter().filter(|t| t.code != 91).cloned().collect::<Vec<_>>(), *original);
                        assert_eq!(saved.iter().filter(|t| t.code == 91 && t.value == "1").count(), 1);
                    }
                    let has_entity = !matches!(variant, CustomRecords::ObjectOnly);
                    let has_object = !matches!(variant, CustomRecords::EntityOnly);
                    let unknowns: Vec<_> =
                        doc.model.iter().filter_map(|e| if let EntityKind::Unknown(u) = &e.kind { Some(u) } else { None }).collect();
                    assert_eq!(unknowns.len(), usize::from(has_entity));
                    assert_eq!(doc.foreign.objects.len(), usize::from(has_object));
                    let line_handle = line.handle.hex();
                    let assert_payload = |tags: &[RawTag], text: &str, bytes: &str| {
                        for (code, value) in [(1, text), (310, bytes), (90, "13"), (340, line_handle.as_str())] {
                            assert_eq!(tags.iter().filter(|t| t.code == code && t.value == value).count(), 1, "{code} {value}");
                        }
                    };
                    if let Some(entity) = unknowns.first() {
                        assert_eq!(entity.dxf_type, "FF1_WIDGET");
                        assert_payload(&entity.tags, "FF1_ENTITY_UNIQUE", "B4A8");
                        let tags = cadcraft_dxf::parse(&bytes)?;
                        let records = cadcraft_dxf::records(&tags);
                        let model = records
                            .iter()
                            .find(|(k, b)| k == "BLOCK_RECORD" && b.iter().any(|t| t.code == 2 && t.str() == "*Model_Space"))
                            .ok_or("model owner absent")?;
                        let owner = model.1.iter().find(|t| t.code == 5).ok_or("model handle absent")?.str();
                        let unknown = records.iter().find(|(k, _)| k == "FF1_WIDGET").ok_or("unknown record absent")?;
                        assert_eq!(unknown.1.iter().filter(|t| t.code == 330).map(|t| t.str()).collect::<Vec<_>>(), [owner]);
                    }
                    if let Some(object) = doc.foreign.objects.first() {
                        assert_eq!(object.kind, "FF1_OBJECT");
                        assert_payload(&object.tags, "FF1_OBJECT_UNIQUE", "D358");
                        let owner =
                            object.tags.iter().find(|t| t.code == 330).and_then(|t| Handle::parse_hex(&t.value)).ok_or("object owner absent")?;
                        let tags = cadcraft_dxf::parse(&bytes)?;
                        let objects = cadcraft_dxf::sections(&tags).into_iter().find(|s| s.name == "OBJECTS").ok_or("OBJECTS absent")?;
                        assert!(cadcraft_dxf::records(&objects.tags).iter().any(|(kind, tags)| {
                            kind == "DICTIONARY"
                                && tags.iter().any(|t| t.code == 5 && t.str() == owner.hex())
                                && tags
                                    .windows(2)
                                    .any(|w| w[0].code == 3 && w[0].str() == "FF1_OBJECT" && w[1].code == 350 && w[1].str() == object.handle.hex())
                        }));
                    }
                }
            }
        }
    }
    Ok(())
}

fn all_database_targets() -> Vec<(&'static str, cadcraft_io::SaveOptions)> {
    use cadcraft_dxf::Version;
    let v = |version| cadcraft_io::SaveOptions { version: Some(version), binary: false };
    vec![
        ("out.dxf", Default::default()),
        ("out.dxf", cadcraft_io::SaveOptions { version: None, binary: true }),
        ("out.dwg", v(Version::R2004)),
        ("out.dwg", v(Version::R2007)),
        ("out.dwg", v(Version::R2018)),
        ("out.dwt", Default::default()),
    ]
}

/// A DWG whose custom records the editable model can't hold: a DWG in the source's version keeps
/// the carried object (with its own bytes, its owner moved to the new named object dictionary),
/// every other database target saves and reports what it lacks, before and after editing the LINE.
#[test]
fn native_import_keeps_or_reports_every_record_for_every_database_target() -> TestResult {
    for variant in [CustomRecords::EntityOnly, CustomRecords::ObjectOnly, CustomRecords::Combined] {
        let source = ff1::dwg_source(variant)?;
        let (mut doc, line) = native_import(variant)?;
        assert!(doc.preservation.blocks_save(), "{variant:?}");
        assert_eq!(doc.preservation.source().map(|s| s.version.as_str()), Some("AC1032"));
        assert!(doc.read_notes.iter().any(|n| n.contains(".lost.json")), "{:?}", doc.read_notes);
        let before = doc.clone();
        for edited in [false, true] {
            if edited {
                edit_line(&mut doc, line)?;
            }
            for (name, opts) in all_database_targets() {
                let (bytes, report) = saved(&doc, name, &opts)?;
                let same_version = name.ends_with("dwt") || opts.version == Some(cadcraft_dxf::Version::R2018);
                let report = report.ok_or("no loss report")?;
                let lost = lost_names(&report);
                if let Some(entity) = &source.entity {
                    assert!(lost.contains(&entity.name), "{variant:?} {name} {opts:?}: {lost:?}");
                }
                if let Some(object) = &source.object {
                    assert_eq!(lost.contains(&object.name), !same_version, "{variant:?} {name} {opts:?}: {lost:?}");
                    assert_eq!(report.kept_natively, usize::from(same_version));
                    if same_version {
                        let native = DwgReader::from_stream(Cursor::new(&bytes)).read()?;
                        let Some(ObjectType::Unknown { type_name, raw_dwg_data: Some(raw), raw_dwg_handle_bits, owner, .. }) =
                            native.objects.get(&object.handle)
                        else {
                            return Err(format!("{variant:?} {name}: carried record absent").into());
                        };
                        assert_eq!(type_name, object.name);
                        assert_eq!(*owner, native.header.named_objects_dict_handle);
                        assert_eq!(data_bits(raw, *raw_dwg_handle_bits), data_bits(&object.raw, object.handle_bits), "its data, bit for bit");
                        let nod = match native.objects.get(&native.header.named_objects_dict_handle) {
                            Some(ObjectType::Dictionary(d)) => d,
                            _ => return Err("no named object dictionary".into()),
                        };
                        assert!(nod.entries.iter().any(|(k, h)| k == object.name && *h == object.handle));
                        let class = native.classes.iter().nth(usize::try_from(object.class_number - 500)?).ok_or("class position")?;
                        assert!(class.dxf_name.eq_ignore_ascii_case(object.name), "the type code still names its class");
                    }
                }
                assert!(report.records.iter().all(|r| r.dwg.is_some()), "every lost record keeps its bytes");
                assert_eq!(report.more, 0);
            }
            // The default entry points save too.
            for name in ["a.dxf", "a.dwg", "a.dwt"] {
                assert!(cadcraft_io::write(&doc, name).is_ok(), "{name}");
                assert!(cadcraft_io::write_framed(&doc, name, None).is_ok(), "{name}");
            }
        }
        // Saving left the drawing as it was (apart from the LINE edit).
        assert_eq!(before.foreign, doc.foreign);
        assert_eq!(before.preservation, doc.preservation);
        assert!(cadcraft_io::write(&doc, "a.svg").is_ok());
        assert!(cadcraft_io::write(&doc, "a.pdf").is_ok());
        assert!(cadcraft_io::png(&doc, &cadcraft_doc::Space::Model, 64, 48).is_ok());
    }
    Ok(())
}

#[test]
fn known_and_native_dxf_only_documents_stay_writable() -> TestResult {
    for variant in [CustomRecords::EntityOnly, CustomRecords::ObjectOnly, CustomRecords::Combined] {
        let doc = cadcraft_io::read(ff1::dxf_source(variant).as_bytes(), "s.dxf")?;
        assert!(!doc.preservation.blocks_save());
        for (name, opts) in [("o.dxf", cadcraft_io::SaveOptions::default()), ("o.dxf", cadcraft_io::SaveOptions { version: None, binary: true })] {
            assert!(cadcraft_io::write_with(&doc, name, &opts).is_ok());
        }
    }
    let known = cadcraft_doc::Drawing::new_metric();
    for name in ["a.dxf", "a.dwg", "a.dwt", "a.svg"] {
        assert!(cadcraft_io::write(&known, name).is_ok(), "{name}");
    }
    Ok(())
}

/// A damaged DXF has skipped records and notes but no preservation issue: editing and saving
/// ASCII and binary DXF works and reopens.
#[test]
fn damaged_dxf_with_skips_remains_savable_as_dxf() -> TestResult {
    const DAMAGED: &str = "0\nSECTION\n2\nENTITIES\n0\nLINE\n5\n2A\n8\n0\n10\n0\n20\n0\n11\n5\n21\n5\n0\nVERTEX\n8\n0\n10\n1\n20\n1\n0\nENDSEC\n0\nLINE\n8\n0\n10\n9\n20\n9\n11\n8\n21\n8\n0\nEOF\n";
    let mut doc = cadcraft_io::read(DAMAGED.as_bytes(), "damaged.dxf")?;
    assert!(!doc.skipped_records.is_empty());
    assert!(!doc.preservation.blocks_save());
    let line = doc.model.iter().find(|e| matches!(e.kind, cadcraft_doc::EntityKind::Line(_))).map(|e| e.handle).ok_or("LINE absent")?;
    edit_line(&mut doc, line)?;
    for binary in [false, true] {
        let bytes = cadcraft_io::write_with(&doc, "o.dxf", &cadcraft_io::SaveOptions { version: None, binary })?;
        let back = cadcraft_io::read(&bytes, "o.dxf")?;
        let lines: Vec<_> =
            back.model.iter().filter_map(|e| if let cadcraft_doc::EntityKind::Line(l) = &e.kind { Some(l.b.x) } else { None }).collect();
        assert!(lines.contains(&10.0), "binary={binary}: {lines:?}");
    }
    Ok(())
}

const BLOCK_APP: &str = "FF1_BLOCK_APP";
const BLOCK_NAME: &str = "FF1_BLOCK";

/// Independent DXF source: only a BLOCK marker has metadata, plus an editable LINE.
/// In particular, no custom CLASS/object can accidentally trigger a different save guard.
fn block_dxf_source(version: cadcraft_io::Version) -> String {
    format!(
        "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\n{}\n0\nENDSEC\n\
         0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nAPPID\n70\n1\n\
         0\nAPPID\n5\n10\n100\nAcDbSymbolTableRecord\n100\nAcDbRegAppTableRecord\n2\n{BLOCK_APP}\n70\n0\n0\nENDTAB\n\
         0\nTABLE\n2\nBLOCK_RECORD\n70\n1\n0\nBLOCK_RECORD\n5\n20\n100\nAcDbSymbolTableRecord\n100\nAcDbBlockTableRecord\n2\n{BLOCK_NAME}\n70\n0\n0\nENDTAB\n0\nENDSEC\n\
         0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n5\n21\n330\n20\n100\nAcDbEntity\n8\n0\n100\nAcDbBlockBegin\n2\n{BLOCK_NAME}\n70\n0\n10\n0\n20\n0\n30\n0\n3\n{BLOCK_NAME}\n1\n\n\
         1001\n{BLOCK_APP}\n1000\nblock-marker-unique\n1070\n137\n1000\nordered-tail\n\
         0\nENDBLK\n5\n22\n330\n20\n100\nAcDbEntity\n8\n0\n100\nAcDbBlockEnd\n0\nENDSEC\n\
         0\nSECTION\n2\nENTITIES\n0\nLINE\n5\n2A\n100\nAcDbEntity\n8\n0\n100\nAcDbLine\n10\n1\n20\n2\n30\n0\n11\n11\n21\n12\n31\n0\n0\nENDSEC\n0\nEOF\n",
        version.code()
    )
}

fn assert_block_payload(doc: &cadcraft_doc::Drawing) -> TestResult {
    let extra = doc.foreign.records.get(&("BLOCK".into(), BLOCK_NAME.into())).ok_or("BLOCK foreign role absent")?;
    let actual: Vec<_> = extra.xdata.iter().map(|t| (t.code, t.value.as_str())).collect();
    assert_eq!(actual, [(1001, BLOCK_APP), (1000, "block-marker-unique"), (1070, "137"), (1000, "ordered-tail")]);
    assert!(doc.foreign.classes.is_empty() && doc.foreign.objects.is_empty());
    assert!(!doc.preservation.blocks_save(), "a DWG export must not mark a native DXF import");
    Ok(())
}

/// BLOCK-marker XDATA a DWG can't hold is reported (removing the prepared-tag check would deliver a
/// DWG without it and no word); DXF saves keep it, repeatedly, in both encodings.
#[test]
fn dxf_block_marker_xdata_is_reported_for_dwg_and_kept_by_repeated_dxf_saves() -> TestResult {
    use cadcraft_io::{SaveOptions, Version};
    for version in [Version::R2004, Version::R2007, Version::R2018] {
        let source = block_dxf_source(version);
        for binary in [false, true] {
            let mut doc = cadcraft_io::read(source.as_bytes(), "block-source.dxf")?;
            for cycle in 1..=2 {
                assert_block_payload(&doc)?;
                let line = doc.model.iter().find(|e| matches!(e.kind, cadcraft_doc::EntityKind::Line(_))).ok_or("LINE absent")?.handle;
                edit_line(&mut doc, line)?;
                let before = doc.clone();
                let opts = SaveOptions { version: Some(version), binary: false };
                let report = saved(&doc, "out.dwg", &opts)?.1.ok_or("BLOCK XDATA silently lost")?;
                let r = report.records.iter().find(|r| r.kind == "metadata").ok_or("no BLOCK-marker record")?;
                assert_eq!(r.record.as_deref(), Some(BLOCK_NAME));
                assert!(r.reason.contains(BLOCK_APP) && r.reason.contains(version.code()), "{}", r.reason);
                assert_eq!(doc, before, "DWG export changed the drawing");
                // A DXF save still keeps the exact ordered payload, no duplicates, and the LINE edit.
                let bytes = cadcraft_io::write_with(&doc, "out.dxf", &SaveOptions { version: Some(version), binary })?;
                doc = cadcraft_io::read(&bytes, "out.dxf")?;
                assert_block_payload(&doc)?;
                let end = doc.model.iter().find_map(|e| if let cadcraft_doc::EntityKind::Line(l) = &e.kind { Some(l.b.x) } else { None });
                assert_eq!(end, Some(11.0 + 5.0 * f64::from(cycle)));
            }
        }
    }
    Ok(())
}

#[test]
fn block_marker_report_names_the_target_and_only_counts_blocks_actually_written() -> TestResult {
    use cadcraft_io::{SaveOptions, Version};
    let mut doc = cadcraft_io::read(block_dxf_source(Version::R2007).as_bytes(), "block.dxf")?;
    let opts = SaveOptions { version: Some(Version::R2018), binary: false };
    let report = saved(&doc, "out.dwg", &opts)?.1.ok_or("no loss report")?;
    assert_eq!(report.target, "DWG AC1032");
    assert!(report.summary().contains("DWG AC1032"), "{}", report.summary());
    assert_block_payload(&cadcraft_io::read_dxf(cadcraft_io::write_dxf(&doc).as_bytes())?)?;
    // Deleting the block leaves its foreign role in the source inventory, but it is no longer
    // emitted. Checking the inventory instead of the prepared tags would falsely report it.
    assert!(doc.blocks.remove(BLOCK_NAME).is_some());
    assert!(doc.foreign.records.contains_key(&("BLOCK".into(), BLOCK_NAME.into())));
    let (bytes, report) = saved(&doc, "out.dwg", &opts)?;
    assert!(report.is_none(), "{report:?}");
    let saved = read_native_metadata(&bytes)?;
    assert!(!saved.block_records.iter().any(|b| b.name == BLOCK_NAME));
    Ok(())
}

/// Native metadata fixtures use the dependency's writer directly, with no CADCraft bridge.
fn metadata_dwg(version: DxfVersion, family: &str) -> TestResult<(Vec<u8>, acadrust::Handle)> {
    use acadrust::entities::{Block, Line};
    use acadrust::tables::{AppId, BlockRecord};
    use acadrust::xdata::{ExtendedData, ExtendedDataRecord, XDataValue};
    let mut doc = acadrust::CadDocument::with_version(version);
    let app = doc.allocate_handle();
    let mut app_id = AppId::new(BLOCK_APP);
    app_id.handle = app;
    doc.app_ids.add(app_id)?;
    doc.add_entity(EntityType::Line(Line::from_coords(1., 2., 0., 11., 12., 0.)))?;
    let mut record = BlockRecord::new(BLOCK_NAME);
    record.handle = doc.allocate_handle();
    record.block_entity_handle = doc.allocate_handle();
    record.block_end_handle = doc.allocate_handle();
    let (owner, marker) = (record.handle, record.block_entity_handle);
    doc.block_records.add(record)?;
    let mut block = Block::new(BLOCK_NAME, acadrust::types::Vector3::ZERO);
    block.common.handle = marker;
    block.common.owner_handle = owner;
    let mut record = ExtendedDataRecord::new(BLOCK_APP);
    record.values = vec![XDataValue::String("block-marker-unique".into()), XDataValue::Integer16(137), XDataValue::String("ordered-tail".into())];
    let mut data = ExtendedData::new();
    data.add_record(record);
    let target = match family {
        "BLOCK" => {
            block.common.extended_data = data;
            marker
        }
        "BLOCK_RECORD" => {
            doc.object_xdata.insert(owner, data);
            owner
        }
        "LAYER" => {
            let handle = doc.layers.get("0").ok_or("layer absent")?.handle;
            doc.object_xdata.insert(handle, data);
            handle
        }
        "STYLE" => {
            let handle = doc.text_styles.get("Standard").ok_or("style absent")?.handle;
            doc.object_xdata.insert(handle, data);
            handle
        }
        "plain" => marker,
        _ => return Err("unknown test metadata family".into()),
    };
    doc.add_entity(EntityType::Block(block))?;
    Ok((acadrust::DwgWriter::write_to_vec(&doc)?, target))
}

fn read_native_metadata(bytes: &[u8]) -> TestResult<acadrust::CadDocument> {
    let outcome = DwgReader::from_stream(Cursor::new(bytes)).read_with_stats()?;
    assert!(outcome.stats.source_records > 0 && outcome.stats.record_stream_read && outcome.stats.stream_completed);
    assert_eq!(outcome.stats.skipped_source_records, 0);
    assert_eq!(outcome.stats.recovered_errors, 0);
    Ok(outcome.document)
}

const APPID_CUSTOM: &str = "FF1_APPID_OWN";
const APPID_PAYLOAD_APP: &str = "FF1_APPID_PAYLOAD";
const APPID_UNUSED: &str = "FF1_APPID_UNUSED";

/// Independent native source: APPID metadata is present even without any entity using it.
fn appid_source(version: DxfVersion, name: &str, entity_use: bool) -> TestResult<acadrust::CadDocument> {
    use acadrust::tables::AppId;
    use acadrust::xdata::{ExtendedData, ExtendedDataRecord, XDataValue};
    let mut doc = acadrust::CadDocument::with_version(version);
    for app in [name, APPID_PAYLOAD_APP, APPID_UNUSED] {
        if doc.app_ids.get(app).is_none() {
            let mut record = AppId::new(app);
            record.handle = doc.allocate_handle();
            doc.app_ids.add(record)?;
        }
    }
    let app = doc.app_ids.get(name).ok_or("source APPID absent")?.handle;
    let layer = doc.layers.get("0").ok_or("source LAYER absent")?.handle;
    let mut record = ExtendedDataRecord::new(APPID_PAYLOAD_APP);
    record.values = vec![
        XDataValue::String("appid-own-unique".into()),
        XDataValue::Integer16(137),
        XDataValue::Handle(app),
        XDataValue::Handle(layer),
        XDataValue::String("appid-ordered-tail".into()),
    ];
    let mut data = ExtendedData::new();
    data.add_record(record);
    doc.object_xdata.insert(app, data);
    let mut line = acadrust::entities::Line::from_coords(1., 2., 0., 11., 12., 0.);
    if entity_use {
        // Different case must discover the same registration rather than adding a duplicate.
        let mut record = ExtendedDataRecord::new(name.to_ascii_lowercase());
        record.values = vec![XDataValue::String("line-appid-unique".into()), XDataValue::Integer16(271)];
        line.common.extended_data.add_record(record);
    }
    doc.add_entity(EntityType::Line(line))?;
    Ok(doc)
}

fn assert_native_appid(doc: &acadrust::CadDocument, name: &str, entity_use: bool, end: f64) -> TestResult {
    use acadrust::xdata::XDataValue;
    let app = doc.app_ids.get(name).ok_or("native APPID absent")?.handle;
    let layer = doc.layers.get("0").ok_or("native LAYER absent")?.handle;
    assert_eq!(doc.app_ids.iter().filter(|a| a.name.eq_ignore_ascii_case(name)).count(), 1);
    let records = doc.object_xdata.get(&app).ok_or("native APPID own XDATA absent")?.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].application_name, APPID_PAYLOAD_APP);
    assert_eq!(
        records[0].values,
        [
            XDataValue::String("appid-own-unique".into()),
            XDataValue::Integer16(137),
            XDataValue::Handle(app),
            XDataValue::Handle(layer),
            XDataValue::String("appid-ordered-tail".into()),
        ]
    );
    let lines: Vec<_> = doc.entities().filter_map(|e| if let EntityType::Line(l) = e { Some(l) } else { None }).collect();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].end.x, end);
    let records = lines[0].common.extended_data.records();
    assert_eq!(records.len(), usize::from(entity_use));
    if entity_use {
        assert!(records[0].application_name.eq_ignore_ascii_case(name));
        assert_eq!(records[0].values, [XDataValue::String("line-appid-unique".into()), XDataValue::Integer16(271)]);
    }
    Ok(())
}

fn native_dxf(doc: &acadrust::CadDocument) -> TestResult<Vec<u8>> {
    let mut bytes = Vec::new();
    acadrust::DxfWriter::new(doc).write_to_writer(&mut bytes)?;
    Ok(bytes)
}

/// Inspect tags independently of CADCraft's foreign-extra reader: identity, owner, ordered
/// payload and semantic references must all survive handle allocation, with no duplicates.
fn assert_appid_tags(bytes: &[u8], name: &str, entity_use: bool, end: f64) -> TestResult {
    use cadcraft_dxf::{Tag, records};
    let tags = cadcraft_dxf::parse(bytes)?;
    let records = records(&tags);
    let value = |body: &[Tag], code| body.iter().find(|t| t.code == code).map(Tag::str);
    let appids: Vec<_> = records.iter().filter(|(kind, _)| kind == "APPID").collect();
    let mut names = std::collections::HashSet::new();
    let table = records.iter().find(|(k, b)| k == "TABLE" && value(b, 2).as_deref() == Some("APPID")).ok_or("APPID table absent")?;
    let table_handle = value(&table.1, 5).ok_or("APPID table handle absent")?;
    assert_eq!(table.1.iter().find(|t| t.code == 70).ok_or("APPID count absent")?.i64(), i64::try_from(appids.len())?);
    let mut handles = std::collections::HashSet::new();
    for (_, body) in &records {
        if let Some(handle) = value(body, 5).or_else(|| value(body, 105)) {
            assert!(handles.insert(handle), "duplicate record handle");
        }
    }
    for (_, body) in &appids {
        assert!(names.insert(value(body, 2).ok_or("APPID name absent")?.to_ascii_uppercase()), "duplicate APPID name");
        assert_eq!(value(body, 330).as_deref(), Some(table_handle.as_str()), "APPID owner does not resolve to its table");
    }
    let (_, body) = appids.iter().find(|(_, b)| value(b, 2).is_some_and(|n| n.eq_ignore_ascii_case(name))).ok_or("saved APPID absent")?;
    let app_handle = value(body, 5).ok_or("APPID handle absent")?;
    let (_, layer) = records.iter().find(|(k, b)| k == "LAYER" && value(b, 2).as_deref() == Some("0")).ok_or("LAYER absent")?;
    let layer_handle = value(layer, 5).ok_or("LAYER handle absent")?;
    let actual: Vec<_> = body.iter().filter(|t| t.code >= 1000).map(|t| (t.code, t.str())).collect();
    assert_eq!(
        actual,
        [
            (1001, APPID_PAYLOAD_APP.into()),
            (1000, "appid-own-unique".into()),
            (1070, "137".into()),
            (1005, app_handle),
            (1005, layer_handle),
            (1000, "appid-ordered-tail".into()),
        ]
    );
    // The payload application is discovered from foreign table XDATA, even with no entity use.
    assert!(names.contains(APPID_PAYLOAD_APP));
    let lines: Vec<_> = records.iter().filter(|(k, _)| k == "LINE").collect();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].1.iter().find(|t| t.code == 11).ok_or("LINE endpoint absent")?.f64(), end);
    let actual: Vec<_> = lines[0].1.iter().filter(|t| t.code >= 1000).map(|t| (t.code, t.str())).collect();
    if entity_use {
        assert_eq!(actual.len(), 3);
        assert_eq!(actual[0].0, 1001);
        assert!(actual[0].1.eq_ignore_ascii_case(name));
        assert_eq!(&actual[1..], [(1000, "line-appid-unique".into()), (1070, "271".into())]);
    } else {
        assert!(actual.is_empty());
    }
    Ok(())
}

fn appid_roundtrips(name: &str, entity_use: bool) -> TestResult {
    use cadcraft_io::{SaveOptions, Version};
    for (native_version, version) in
        [(DxfVersion::AC1018, Version::R2004), (DxfVersion::AC1021, Version::R2007), (DxfVersion::AC1032, Version::R2018)]
    {
        let source = appid_source(native_version, name, entity_use)?;
        let dwg = acadrust::DwgWriter::write_to_vec(&source)?;
        let native = read_native_metadata(&dwg)?;
        // Prove source capability before the bridge, including the ordinary unused control.
        assert_native_appid(&native, name, entity_use, 11.)?;
        let unused = native.app_ids.get(APPID_UNUSED).ok_or("source unused APPID absent")?;
        assert!(!native.object_xdata.contains_key(&unused.handle));
        let bridge = cadcraft_dwg::dwg_to_dxf(&dwg)?;
        assert_appid_tags(&bridge, name, entity_use, 11.)?;
        let dxf = native_dxf(&source)?;
        assert_appid_tags(&dxf, name, entity_use, 11.)?;
        for (input, input_path) in [(&dwg, "source.dwg"), (&dxf, "source.dxf")] {
            for (output_path, binary) in [("saved.dxf", false), ("saved.dxf", true), ("saved.dwg", false)] {
                let mut doc = cadcraft_io::read(input, input_path)?;
                assert!(!doc.preservation.blocks_save());
                let extra = doc.foreign.records.get(&("APPID".into(), name.into())).ok_or("retained APPID extra absent")?;
                assert_eq!(extra.xdata.len(), 6);
                // A plain unused registration has no extras to merge; this fix does not
                // invent a new document model for otherwise unmodelled registrations.
                assert!(!doc.foreign.records.contains_key(&("APPID".into(), APPID_UNUSED.into())));
                for cycle in 1..=2 {
                    let line = doc.model.iter().find(|e| matches!(e.kind, cadcraft_doc::EntityKind::Line(_))).ok_or("LINE absent")?.handle;
                    edit_line(&mut doc, line)?;
                    let bytes = cadcraft_io::write_with(&doc, output_path, &SaveOptions { version: Some(version), binary })?;
                    let end = 11. + 5. * f64::from(cycle);
                    if output_path.ends_with("dwg") {
                        let native = read_native_metadata(&bytes)?;
                        assert_native_appid(&native, name, entity_use, end)?;
                        assert_appid_tags(&native_dxf(&native)?, name, entity_use, end)?;
                    } else {
                        assert_appid_tags(&bytes, name, entity_use, end)?;
                    }
                    doc = cadcraft_io::read(&bytes, output_path)?;
                    assert!(!doc.preservation.blocks_save());
                    assert_eq!(doc.foreign.records.get(&("APPID".into(), name.into())).ok_or("reopened APPID extra absent")?.xdata.len(), 6);
                }
            }
        }
    }
    Ok(())
}

#[test]
fn custom_appid_own_xdata_survives_without_entity_use() -> TestResult {
    appid_roundtrips(APPID_CUSTOM, false)
}

#[test]
fn custom_appid_own_xdata_survives_with_case_insensitive_entity_use() -> TestResult {
    appid_roundtrips(APPID_CUSTOM, true)
}

#[test]
fn builtin_appid_foreign_xdata_remains_writable() -> TestResult {
    appid_roundtrips("ACAD", false)
}

#[test]
fn native_block_source_has_public_xdata_and_raw_eed_and_every_database_save_reports_it() -> TestResult {
    use acadrust::xdata::XDataValue;
    for version in [DxfVersion::AC1018, DxfVersion::AC1021, DxfVersion::AC1032] {
        let (bytes, marker) = metadata_dwg(version, "BLOCK")?;
        let native = read_native_metadata(&bytes)?;
        assert!(!native.object_xdata.contains_key(&marker), "this lookup cannot establish BLOCK safety");
        let Some(EntityType::Block(block)) = native.get_entity(marker) else { return Err("native BLOCK absent".into()) };
        let records = block.common.extended_data.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].application_name, BLOCK_APP);
        assert_eq!(
            records[0].values,
            [XDataValue::String("block-marker-unique".into()), XDataValue::Integer16(137), XDataValue::String("ordered-tail".into())]
        );
        // Read raw EED independently through the public record reader too. The payload is present
        // in the source, even though the dependency's DXF writer cannot carry it across the bridge.
        let mut reader = DwgReader::from_stream(Cursor::new(&bytes));
        let info = reader.read_file_header()?;
        let handles = read_handles(&reader.get_section_buffer("AcDb:Handles", &info)?)?;
        let objects = reader.get_section_buffer("AcDb:AcDbObjects", &info)?;
        let raw = DwgObjectReader::new(objects, version, handles)?;
        let offset = usize::try_from(raw.offset_for(marker.value()).ok_or("BLOCK offset absent")?)?;
        let (kind, mut stream) = raw.read_record_at(offset)?;
        let common = raw.read_common_data(&mut stream, kind);
        assert_eq!(common.handle, marker.value());
        assert_eq!(common.eed_raw.len(), 1);
        assert!(!common.eed_raw[0].1.is_empty());
        assert_eq!(block.common.extended_data.raw_dwg_eed, common.eed_raw);
        let mut doc = cadcraft_io::read(&bytes, "block.dwg")?;
        assert!(doc.preservation.blocks_save());
        assert_eq!(doc.preservation.issue_count(), 1, "metadata-only source must exercise only the BLOCK report");
        let line = doc.model.iter().find(|e| matches!(e.kind, cadcraft_doc::EntityKind::Line(_))).ok_or("LINE absent")?.handle;
        for _ in 0..2 {
            edit_line(&mut doc, line)?;
            for (target, opts) in all_database_targets() {
                let report = saved(&doc, target, &opts)?.1.ok_or("native BLOCK loss not reported")?;
                assert_eq!(report.count(), 1);
                let r = report.records.first().ok_or("no record")?;
                assert_eq!((r.kind, r.record.as_deref()), ("metadata", Some(BLOCK_NAME)));
                assert!(r.reason.contains("BLOCK marker"), "{}", r.reason);
            }
            assert!(!cadcraft_io::write_dxf(&doc).is_empty());
        }
    }
    Ok(())
}

/// Each supported family is isolated: a failure cannot be hidden by another record's XDATA.
fn table_roundtrips(native_version: DxfVersion, version: cadcraft_io::Version, family: &str) -> TestResult {
    use acadrust::xdata::XDataValue;
    let (bytes, target) = metadata_dwg(native_version, family)?;
    let native = read_native_metadata(&bytes)?;
    let expected = if family == "plain" {
        Vec::new()
    } else {
        let records = native.object_xdata.get(&target).ok_or("source table XDATA absent")?.records().to_vec();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].application_name, BLOCK_APP);
        assert_eq!(
            records[0].values,
            [XDataValue::String("block-marker-unique".into()), XDataValue::Integer16(137), XDataValue::String("ordered-tail".into())]
        );
        records
    };
    let check_tags = |bytes: &[u8], end: f64| -> TestResult {
        let tags = cadcraft_dxf::parse(bytes)?;
        let records = cadcraft_dxf::records(&tags);
        let (kind, name) = match family {
            "LAYER" => ("LAYER", "0"),
            "STYLE" => ("STYLE", "Standard"),
            "BLOCK_RECORD" => ("BLOCK_RECORD", BLOCK_NAME),
            _ => ("BLOCK", BLOCK_NAME),
        };
        let targets: Vec<_> = records.iter().filter(|(k, b)| k == kind && b.iter().any(|t| t.code == 2 && t.str() == name)).collect();
        assert_eq!(targets.len(), 1);
        let body = &targets[0].1;
        let actual: Vec<_> = body.iter().filter(|t| t.code >= 1000).map(|t| (t.code, t.str())).collect();
        let payload = if family == "plain" {
            Vec::new()
        } else {
            vec![(1001, BLOCK_APP.into()), (1000, "block-marker-unique".into()), (1070, "137".into()), (1000, "ordered-tail".into())]
        };
        assert_eq!(actual, payload);
        let owner = body.iter().find(|t| t.code == 330).ok_or("record owner absent")?.str();
        assert!(
            records
                .iter()
                .any(|(k, b)| k == if family == "plain" { "BLOCK_RECORD" } else { "TABLE" } && b.iter().any(|t| t.code == 5 && t.str() == owner)),
            "owner must resolve"
        );
        let lines: Vec<_> = records.iter().filter(|(k, _)| k == "LINE").collect();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].1.iter().find(|t| t.code == 11).ok_or("LINE endpoint absent")?.f64(), end);
        assert!(!records.iter().any(|(k, _)| k.starts_with("FF1_")), "fixture contains unexpected custom records");
        Ok(())
    };
    check_tags(&native_dxf(&native)?, 11.)?;
    for (path, binary) in [("saved.dwg", false), ("saved.dxf", false), ("saved.dxf", true)] {
        let mut doc = cadcraft_io::read(&bytes, "metadata.dwg")?;
        assert!(!doc.preservation.blocks_save(), "{native_version:?} {family}");
        for cycle in 1..=2 {
            let line = doc.model.iter().find(|e| matches!(e.kind, cadcraft_doc::EntityKind::Line(_))).ok_or("LINE absent")?.handle;
            edit_line(&mut doc, line)?;
            let bytes = cadcraft_io::write_with(&doc, path, &cadcraft_io::SaveOptions { version: Some(version), binary })?;
            let end = 11. + 5. * f64::from(cycle);
            if path.ends_with("dwg") {
                assert_eq!(cadcraft_dwg::version(&bytes).as_deref(), Some(version.code()));
                let saved = read_native_metadata(&bytes)?;
                let target = match family {
                    "BLOCK_RECORD" => saved.block_records.get(BLOCK_NAME).ok_or("saved BLOCK_RECORD absent")?.handle,
                    "LAYER" => saved.layers.get("0").ok_or("saved LAYER absent")?.handle,
                    "STYLE" => saved.text_styles.get("Standard").ok_or("saved STYLE absent")?.handle,
                    _ => saved.block_records.get(BLOCK_NAME).ok_or("saved plain BLOCK absent")?.block_entity_handle,
                };
                assert_eq!(saved.object_xdata.get(&target).map(|x| x.records()).unwrap_or(&[]), expected);
                let line = saved.entities().find_map(|e| if let EntityType::Line(l) = e { Some(l) } else { None }).ok_or("native LINE absent")?;
                assert_eq!((line.start.x, line.start.y, line.end.x, line.end.y), (1., 2., end, 12.));
                check_tags(&native_dxf(&saved)?, end)?;
            } else {
                check_tags(&bytes, end)?;
            }
            doc = cadcraft_io::read(&bytes, path)?;
            assert!(!doc.preservation.blocks_save());
            assert_eq!(cadcraft_io::save_version(&doc), version);
        }
    }
    Ok(())
}

fn supported_metadata_roundtrips(native_version: DxfVersion, version: cadcraft_io::Version, family: metadata::Family) -> TestResult {
    let source = metadata::source(native_version, family)?;
    let dwg = acadrust::DwgWriter::write_to_vec(&source.doc)?;
    let native = read_native_metadata(&dwg)?;
    metadata::assert_native(&native, family, 11.)?;
    assert!(native.get_entity(source.line).is_some());
    assert!(
        native.get_entity(source.target).is_some()
            || native.objects.contains_key(&source.target)
            || native.layers.iter().any(|l| l.handle == source.target)
    );
    metadata::assert_tags(&native_dxf(&native)?, family, 11.)?;
    for (path, binary) in [("saved.dwg", false), ("saved.dxf", false), ("saved.dxf", true)] {
        let mut doc = cadcraft_io::read(&dwg, "source.dwg")?;
        for cycle in 1..=2 {
            assert!(!doc.preservation.blocks_save());
            let line = doc.model.iter().find(|e| matches!(e.kind, cadcraft_doc::EntityKind::Line(_))).ok_or("LINE absent")?.handle;
            edit_line(&mut doc, line)?;
            let bytes = cadcraft_io::write_with(&doc, path, &cadcraft_io::SaveOptions { version: Some(version), binary })?;
            let end = 11. + 5. * f64::from(cycle);
            if path.ends_with("dwg") {
                assert_eq!(cadcraft_dwg::version(&bytes).as_deref(), Some(version.code()));
                let native = read_native_metadata(&bytes)?;
                metadata::assert_native(&native, family, end)?;
                metadata::assert_tags(&native_dxf(&native)?, family, end)?;
            } else {
                metadata::assert_tags(&bytes, family, end)?;
            }
            doc = cadcraft_io::read(&bytes, path)?;
            assert_eq!(cadcraft_io::save_version(&doc), version);
        }
    }
    Ok(())
}

fn custom_reactor_target(native_version: DxfVersion, _version: cadcraft_io::Version, family: metadata::Family) -> TestResult {
    let source = metadata::source(native_version, family)?;
    let bytes = acadrust::DwgWriter::write_to_vec(&source.doc)?;
    let native = read_native_metadata(&bytes)?;
    metadata::assert_native(&native, family, 11.)?;
    let Some(ObjectType::Unknown { raw_dwg_data, .. }) = native.objects.get(&source.target) else { return Err("custom target absent".into()) };
    assert_eq!(raw_dwg_data, &source.raw);
    // Independently resolve the custom target's typed back-reference and text from source bits.
    let mut reader = DwgReader::from_stream(Cursor::new(&bytes));
    let info = reader.read_file_header()?;
    let handles = read_handles(&reader.get_section_buffer("AcDb:Handles", &info)?)?;
    let raw = DwgObjectReader::new(reader.get_section_buffer("AcDb:AcDbObjects", &info)?, native_version, handles)?;
    let offset = usize::try_from(raw.offset_for(source.target.value()).ok_or("target offset absent")?)?;
    let (kind, mut stream) = raw.read_record_at(offset)?;
    assert_eq!(kind, native.classes.get_by_name(metadata::REACTOR).ok_or("source CLASS absent")?.class_number);
    let common = raw.read_common_non_entity_data(&mut stream, kind);
    assert_eq!(common.common.handle, source.target.value());
    assert_eq!(common.owner_handle, native.header.named_objects_dict_handle.value());
    assert_eq!(stream.read_variable_text(), metadata::REACTOR_PAYLOAD);
    assert_eq!(stream.read_typed_handle(), (source.line.value(), DwgReferenceType::HardPointer));
    stream.check_error()?;
    let mut doc = cadcraft_io::read(&bytes, "reactor.dwg")?;
    assert_eq!(doc.preservation.issue_count(), 1);
    // Carried back only into a DWG of its own version, from 2010 on (R2010+ record layout).
    let carried_version = native_version >= DxfVersion::AC1024;
    for _ in 0..2 {
        let line = doc.model.iter().find(|e| matches!(e.kind, cadcraft_doc::EntityKind::Line(_))).ok_or("LINE absent")?.handle;
        edit_line(&mut doc, line)?;
        let before = doc.clone();
        for (path, opts) in all_database_targets() {
            // A template saves in the drawing's own version.
            let version = if path.ends_with(".dwt") { Some(cadcraft_io::save_version(&doc).max(cadcraft_io::Version::R2004)) } else { opts.version };
            let same = !path.ends_with(".dxf") && version.map(|v| v.code()) == Some(native_version.as_str());
            let (bytes, report) = saved(&doc, path, &opts)?;
            let report = report.ok_or("no loss report")?;
            if same && carried_version {
                assert_eq!((report.kept_natively, report.count()), (1, 0), "{path} {opts:?}");
                let saved = read_native_metadata(&bytes)?;
                let Some(ObjectType::Unknown { type_name, .. }) = saved.objects.get(&source.target) else {
                    return Err("carried target absent".into());
                };
                assert_eq!(type_name, metadata::REACTOR);
            } else {
                let r = report.records.iter().find(|r| r.record.as_deref() == Some(metadata::REACTOR)).ok_or("lost target not reported")?;
                assert_eq!(r.dwg.as_ref().map(|d| &d.bytes), source.raw.as_ref(), "{path} {opts:?}");
                assert_eq!(r.class.as_ref().map(|c| c.1.as_str()), Some("RustFF1Reactor"));
            }
        }
        assert_eq!(doc, before);
    }
    Ok(())
}

// Named cases expose a failing family/version independently in cargo's test output.
macro_rules! metadata_matrix {
    ($run:ident, $family:expr, $a:ident, $b:ident, $c:ident) => {
        #[test]
        fn $a() -> TestResult {
            $run(DxfVersion::AC1018, cadcraft_io::Version::R2004, $family)
        }
        #[test]
        fn $b() -> TestResult {
            $run(DxfVersion::AC1021, cadcraft_io::Version::R2007, $family)
        }
        #[test]
        fn $c() -> TestResult {
            $run(DxfVersion::AC1032, cadcraft_io::Version::R2018, $family)
        }
    };
}
metadata_matrix!(table_roundtrips, "LAYER", layer_xdata_ac1018, layer_xdata_ac1021, layer_xdata_ac1032);
metadata_matrix!(table_roundtrips, "BLOCK_RECORD", block_record_xdata_ac1018, block_record_xdata_ac1021, block_record_xdata_ac1032);
metadata_matrix!(table_roundtrips, "STYLE", style_xdata_ac1018, style_xdata_ac1021, style_xdata_ac1032);
metadata_matrix!(table_roundtrips, "plain", plain_block_ac1018, plain_block_ac1021, plain_block_ac1032);
metadata_matrix!(supported_metadata_roundtrips, metadata::Family::EntityColour, entity_colour_ac1018, entity_colour_ac1021, entity_colour_ac1032);
metadata_matrix!(supported_metadata_roundtrips, metadata::Family::LayerColour, layer_colour_ac1018, layer_colour_ac1021, layer_colour_ac1032);
metadata_matrix!(supported_metadata_roundtrips, metadata::Family::KnownReactor, known_reactor_ac1018, known_reactor_ac1021, known_reactor_ac1032);
metadata_matrix!(custom_reactor_target, metadata::Family::CustomReactor, custom_reactor_ac1018, custom_reactor_ac1021, custom_reactor_ac1032);

#[test]
fn dxf_custom_records_are_reported_by_every_dwg_version_whatever_the_source_version() -> TestResult {
    for source_version in [None, Some("AC1018"), Some("AC1021"), Some("AC1032")] {
        for variant in [CustomRecords::EntityOnly, CustomRecords::ObjectOnly, CustomRecords::Combined] {
            let source = ff1::dxf_source(variant)
                .replace("9\n$ACADVER\n1\nAC1032\n", &source_version.map(|v| format!("9\n$ACADVER\n1\n{v}\n")).unwrap_or_default());
            let mut doc = cadcraft_io::read_dxf(source.as_bytes())?;
            for _ in 0..2 {
                let line = doc.model.iter().find(|e| matches!(e.kind, cadcraft_doc::EntityKind::Line(_))).ok_or("LINE absent")?.handle;
                edit_line(&mut doc, line)?;
                for target in [cadcraft_io::Version::R2004, cadcraft_io::Version::R2007, cadcraft_io::Version::R2018] {
                    let report = saved(&doc, "out.dwg", &cadcraft_io::SaveOptions { version: Some(target), binary: false })?
                        .1
                        .ok_or("version fallback hid the custom records")?;
                    assert_eq!(report.target, format!("DWG {}", target.code()));
                    let lost = lost_names(&report);
                    assert_eq!(lost.contains(&"FF1_WIDGET"), !matches!(variant, CustomRecords::ObjectOnly), "{lost:?}");
                    assert_eq!(lost.contains(&"FF1_OBJECT"), !matches!(variant, CustomRecords::EntityOnly), "{lost:?}");
                }
                assert!(!doc.preservation.blocks_save(), "a DWG export must not mark a native DXF");
                let bytes = cadcraft_io::write(&doc, "safe.dxf")?;
                let reopened = cadcraft_io::read_dxf(&bytes)?;
                assert_eq!(reopened.foreign.classes.len(), doc.foreign.classes.len());
            }
        }
    }
    Ok(())
}

#[test]
fn malformed_or_truncated_native_sources_error_or_report_their_losses() -> TestResult {
    let source = ff1::dwg_source(CustomRecords::Combined)?;
    // Short/invalid headers cannot yield a complete editable database.
    for bytes in [b"".as_slice(), b"AC1032", b"AC1032 not a DWG database", b"AC1099 invalid version"] {
        for _ in 0..2 {
            assert!(cadcraft_dwg::dwg_to_dxf_detailed(bytes, cadcraft_dwg::Limits::DEFAULT).is_err());
            assert!(cadcraft_io::read(bytes, "broken.dwg").is_err());
        }
    }
    for cut in [100, source.bytes.len() / 2, source.bytes.len() - 1] {
        // A cut after all meaningful records may still decode; any returned drawing keeps the
        // custom-record loss, and every save reports it. Never invent output for a rejected source.
        for _ in 0..2 {
            if let Ok(doc) = cadcraft_io::read(&source.bytes[..cut], "truncated.dwg") {
                assert!(doc.preservation.blocks_save(), "cut {cut} returned a falsely complete drawing");
                for (path, opts) in all_database_targets() {
                    let report = saved(&doc, path, &opts)?.1.ok_or("truncated source saved with no report")?;
                    assert!(report.count() + report.kept_natively as u64 >= 1, "{report:?}");
                }
            }
        }
    }
    // Resource errors are deterministic and happen before the dependency decodes the source.
    let limits = cadcraft_dwg::Limits { max_dwg_bytes: source.bytes.len() - 1, max_dxf_bytes: 1024 };
    let error = cadcraft_dwg::dwg_to_dxf_detailed(&source.bytes, limits).expect_err("input limit ignored");
    assert!(error.contains("larger than"), "{error}");
    Ok(())
}

#[test]
fn native_issue_overflow_is_counted_in_full_through_the_io_mapping_with_bounded_messages() -> TestResult {
    use acadrust::entities::{Block, Line};
    use acadrust::tables::{AppId, BlockRecord};
    use acadrust::xdata::{ExtendedDataRecord, XDataValue};
    use cadcraft_doc::foreign::MAX_PRESERVATION_DETAILS;
    let count = cadcraft_dwg::MAX_BRIDGE_ISSUES.max(MAX_PRESERVATION_DETAILS) + 9;
    let mut native = acadrust::CadDocument::with_version(DxfVersion::AC1032);
    let mut app = AppId::new(BLOCK_APP);
    app.handle = native.allocate_handle();
    native.app_ids.add(app)?;
    native.add_entity(EntityType::Line(Line::from_coords(1., 2., 0., 11., 12., 0.)))?;
    let mut markers = Vec::new();
    for i in 0..count {
        // Long Unicode names also exercise UTF-8-safe report truncation at the native boundary.
        let name = format!("FF1_{i}_{}", "界".repeat(100));
        let mut record = BlockRecord::new(&name);
        record.handle = native.allocate_handle();
        record.block_entity_handle = native.allocate_handle();
        record.block_end_handle = native.allocate_handle();
        let mut block = Block::new(&name, acadrust::types::Vector3::ZERO);
        block.common.handle = record.block_entity_handle;
        block.common.owner_handle = record.handle;
        let mut data = ExtendedDataRecord::new(BLOCK_APP);
        data.values.push(XDataValue::String(format!("unique-{i}")));
        block.common.extended_data.add_record(data);
        markers.push(block.common.handle);
        native.block_records.add(record)?;
        native.add_entity(EntityType::Block(block))?;
    }
    let bytes = acadrust::DwgWriter::write_to_vec(&native)?;
    let source = read_native_metadata(&bytes)?;
    for (i, marker) in markers.iter().enumerate() {
        let block = source.get_entity(*marker).ok_or("source BLOCK absent")?;
        assert_eq!(block.common().extended_data.records()[0].values, [XDataValue::String(format!("unique-{i}"))]);
    }
    let bridge = cadcraft_dwg::dwg_to_dxf_detailed(&bytes, cadcraft_dwg::Limits::DEFAULT)?;
    assert_eq!(bridge.issue_count, u64::try_from(count)?);
    assert_eq!(bridge.issues.len(), cadcraft_dwg::MAX_BRIDGE_ISSUES);
    assert!(bridge.issues.iter().all(|i| i.record_name.as_ref().is_some_and(|n| n.len() <= 256)));
    let mut doc = cadcraft_io::read(&bytes, "many.dwg")?;
    assert_eq!(doc.preservation.issue_count(), bridge.issue_count);
    assert_eq!(doc.preservation.issues().len(), MAX_PRESERVATION_DETAILS);
    for _ in 0..2 {
        let line = doc.model.iter().find(|e| matches!(e.kind, cadcraft_doc::EntityKind::Line(_))).ok_or("LINE absent")?.handle;
        edit_line(&mut doc, line)?;
        for (path, opts) in all_database_targets() {
            let report = saved(&doc, path, &opts)?.1.ok_or("the detail cap hid the losses")?;
            assert_eq!(report.count(), u64::try_from(count)?, "every loss counted");
            assert_eq!(report.records.len(), MAX_PRESERVATION_DETAILS);
            assert_eq!(report.more, u64::try_from(count - MAX_PRESERVATION_DETAILS)?);
            let summary = report.summary();
            assert!(summary.len() < 4096 && summary.contains("more unlisted"), "{summary}");
        }
        assert_eq!(doc.preservation.issue_count(), bridge.issue_count);
    }
    Ok(())
}
