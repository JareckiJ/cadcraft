//! CLI publication boundary for FF1: every save writes its file, keeps a loss report beside it
//! naming each record it lacks (with that record's own data), and backs up a source it overwrites
//! with less. All sources are tiny original generated fixtures.
//! Native oracles share locked acadrust 0.6.5, independently of CADCraft's DXF bridge;
//! these tests do not claim independent AutoCAD validation or arbitrary DWG preservation.
#![cfg(not(target_arch = "wasm32"))]

#[path = "../../../crates/io/tests/support/ff1.rs"]
mod ff1;

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use acadrust::entities::{Block, Line};
use acadrust::io::dwg::dwg_reference_type::DwgReferenceType;
use acadrust::io::dwg::dwg_stream_readers::{handle_reader::read_handles, object_reader::DwgObjectReader};
use acadrust::objects::{ObjectType, XRecord, XRecordEntry, XRecordValue};
use acadrust::tables::{AppId, BlockRecord};
use acadrust::xdata::{ExtendedData, ExtendedDataRecord, XDataValue};
use acadrust::{CadDocument, DwgReader, DwgWriter, DxfVersion, EntityType};
use cadcraft_engine::doc::{Drawing, EntityKind, Handle};
use ff1::{CustomRecords, TestResult};

static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);
const APP: &str = "FF1_CLI_APP";
const BLOCK: &str = "FF1_CLI_BLOCK";
const SENTINEL: &[u8] = b"existing target must remain byte-identical\0\xff";

struct TestDir(PathBuf);

impl TestDir {
    /// A fresh directory, removed when dropped.
    fn new() -> TestResult<Self> {
        let path = std::env::temp_dir().join(format!("cc-cli-ff1-{}-{}", std::process::id(), NEXT_DIR.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TestDir {
    /// Remove the directory and what is in it.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Run the CLI binary with `args`.
fn cli(args: &[&str]) -> TestResult<Output> {
    Ok(Command::new(env!("CARGO_BIN_EXE_cadcraft-cli")).args(args).output()?)
}

/// A test path as text.
fn path(path: &Path) -> &str {
    path.to_str().expect("test path is UTF-8")
}

/// Assert the CLI succeeded, showing its stderr if not.
fn success(out: &Output) {
    assert!(out.status.success(), "{:?}: {}", out.status, String::from_utf8_lossy(&out.stderr));
}

/// Assert the first JSON result moved the LINE.
fn moved(out: &Output) -> TestResult {
    let first = out.stdout.split(|b| *b == b'\n').find(|line| !line.is_empty()).ok_or("no move result")?;
    let result: serde_json::Value = serde_json::from_slice(first)?;
    assert_eq!(result["moved"], 1, "LINE must be edited before save: {result}");
    Ok(())
}

/// The LINE's handle in a drawing file.
fn line_handle(bytes: &[u8]) -> TestResult<Handle> {
    let doc = cadcraft_io::read(bytes, "source.dxf")?;
    Ok(doc.model.iter().find(|e| matches!(e.kind, EntityKind::Line(_))).ok_or("LINE absent")?.handle)
}

/// A MOVE of the LINE by 5 along X, as a script line.
fn move_command(bytes: &[u8]) -> TestResult<String> {
    Ok(format!("move {}", serde_json::json!({"handle": line_handle(bytes)?.hex(), "delta": [5, 0]})))
}

/// Read a DWG natively, asserting nothing was skipped or recovered.
fn read_native(bytes: &[u8]) -> TestResult<CadDocument> {
    let out = DwgReader::from_stream(Cursor::new(bytes)).read_with_stats()?;
    assert!(out.stats.source_records > 0 && out.stats.record_stream_read && out.stats.stream_completed);
    assert_eq!(out.stats.skipped_source_records, 0);
    assert_eq!(out.stats.recovered_errors, 0);
    Ok(out.document)
}

/// Every save of `bytes` (convert, and an edited run) to each target writes the file, new or over
/// an existing one, and keeps a report beside it naming `lost(target)` (none: no report), each
/// record with its own DWG bytes or DXF groups; the source stays as it was. `check` sees each
/// written file. Last, an edited save over the source itself backs it up when it lacks something.
fn loss_matrix(
    bytes: &[u8],
    source_ext: &str,
    targets: &[&str],
    lost: impl Fn(&str) -> Vec<&'static str>,
    check: impl Fn(&str, &[u8]) -> TestResult,
    verify: impl Fn(&[u8]) -> TestResult,
) -> TestResult {
    verify(bytes)?;
    let dir = TestDir::new()?;
    let source = dir.0.join(format!("source.{source_ext}"));
    fs::write(&source, bytes)?;
    let command = move_command(bytes)?;
    for edit in [false, true] {
        let invoke = |target: &Path| -> TestResult<Output> {
            if edit {
                cli(&["run", path(&source), "--cmd", &command, "--save", path(target)])
            } else {
                cli(&["convert", path(&source), path(target)])
            }
        };
        for target in targets {
            let output = dir.0.join(format!("output.{target}"));
            let report = dir.0.join(format!("output.{target}.lost.json"));
            for existing in [false, true] {
                if existing {
                    fs::write(&output, SENTINEL)?;
                }
                let out = invoke(&output)?;
                success(&out);
                if edit {
                    moved(&out)?;
                }
                let written = fs::read(&output)?;
                assert_ne!(written, SENTINEL, "edit={edit} {target}: not written");
                check(target, &written)?;
                assert_report(&report, &lost(target), &String::from_utf8_lossy(&out.stderr))?;
                assert_eq!(fs::read(&source)?, bytes, "source changed: edit={edit} {target}");
            }
            fs::remove_file(&output)?;
            let _ = fs::remove_file(&report);
        }
    }
    if targets.contains(&source_ext) {
        let out = cli(&["run", path(&source), "--cmd", &command, "--save", path(&source)])?;
        success(&out);
        let backup = dir.0.join(format!("source.orig.{source_ext}"));
        if lost(source_ext).is_empty() {
            assert!(!backup.exists(), "nothing lost, nothing to back up");
        } else {
            assert_eq!(fs::read(&backup)?, bytes, "the original kept before the lossy save over it");
            verify(&fs::read(&backup)?)?;
        }
    }
    Ok(())
}

/// The loss report beside a saved file: each expected record listed with its own data, the loss
/// summarised on stderr; no report when nothing is lost.
fn assert_report(report: &Path, names: &[&str], stderr: &str) -> TestResult {
    if names.is_empty() {
        assert!(!report.exists(), "a report for a save that lacks nothing: {stderr}");
        return Ok(());
    }
    let json: serde_json::Value = serde_json::from_slice(&fs::read(report)?)?;
    let records = json["records"].as_array().ok_or("records")?;
    for name in names {
        let r = records.iter().find(|r| r["type"].as_str() == Some(name)).ok_or_else(|| format!("{name} not reported: {json}"))?;
        let has_data = r["dwg"]["data_base64"].as_str().is_some_and(|d| !d.is_empty()) || r["dxf_groups"].as_array().is_some_and(|g| !g.is_empty());
        assert!(has_data || r["kind"] == "metadata", "{name} without its data: {r}");
    }
    assert_eq!(json["lost"].as_u64(), Some(names.len() as u64), "{json}");
    assert!(stderr.contains("aren't in this") && stderr.contains(".lost.json"), "{stderr}");
    Ok(())
}

/// Validate the actual disk source, including exact opaque bits/text, CLASS binding, owners
/// and typed hard references. A bridge round trip cannot stand in for this source oracle.
fn assert_opaque(bytes: &[u8], expected: &ff1::DwgSource) -> TestResult {
    let doc = read_native(bytes)?;
    let Some(EntityType::Line(line)) = doc.get_entity(expected.line) else { return Err("source LINE absent".into()) };
    assert_eq!((line.start.x, line.start.y, line.end.x, line.end.y), (1., 2., 11., 12.));
    assert_eq!(doc.entities().filter(|e| matches!(e, EntityType::Unknown(_))).count(), usize::from(expected.entity.is_some()));
    assert_eq!(doc.objects.values().filter(|o| matches!(o, ObjectType::Unknown { .. })).count(), usize::from(expected.object.is_some()));
    let mut reader = DwgReader::from_stream(Cursor::new(bytes));
    let info = reader.read_file_header()?;
    assert_eq!(info.version_string, "AC1032");
    let handles = read_handles(&reader.get_section_buffer("AcDb:Handles", &info)?)?;
    let objects = reader.get_section_buffer("AcDb:AcDbObjects", &info)?;
    let raw = DwgObjectReader::new(objects, DxfVersion::AC1032, handles)?;
    for (graphical, record) in [(true, &expected.entity), (false, &expected.object)] {
        let Some(record) = record else { continue };
        assert_eq!(doc.classes.iter().filter(|c| c.dxf_name == record.name).count(), 1);
        let class = doc.classes.get_by_name(record.name).ok_or("CLASS absent")?;
        assert_eq!(class.cpp_class_name, format!("Rust{}", record.name));
        assert_eq!(class.application_name, "FF1_SYNTHETIC");
        assert_eq!(class.is_an_entity, graphical);
        assert_eq!(class.class_number, record.class_number);
        if graphical {
            let Some(EntityType::Unknown(u)) = doc.get_entity(record.handle) else { return Err("unknown entity absent".into()) };
            assert_eq!(u.common.owner_handle, record.owner);
            assert_eq!(u.raw_dwg_data.as_deref(), Some(record.raw.as_ref()));
            assert_eq!(u.dwg_handle_bits, record.handle_bits);
            assert_eq!(u.dwg_type_code, record.class_number);
        } else {
            let Some(ObjectType::Unknown { owner, raw_dwg_data, raw_dwg_handle_bits, .. }) = doc.objects.get(&record.handle) else {
                return Err("unknown object absent".into());
            };
            assert_eq!(*owner, record.owner);
            assert_eq!(raw_dwg_data.as_deref(), Some(record.raw.as_ref()));
            assert_eq!(*raw_dwg_handle_bits, record.handle_bits);
            let Some(ObjectType::Dictionary(nod)) = doc.objects.get(owner) else { return Err("owner dictionary absent".into()) };
            assert_eq!(nod.entries.iter().filter(|(n, h)| n == record.name && *h == record.handle).count(), 1);
        }
        let offset = usize::try_from(raw.offset_for(record.handle.value()).ok_or("raw offset absent")?)?;
        let (kind, mut stream) = raw.read_record_at(offset)?;
        assert_eq!(kind, record.class_number);
        if graphical {
            let common = raw.read_common_entity_data(&mut stream, kind);
            assert_eq!(common.common.handle, record.handle.value());
            assert_eq!(common.entity_mode, 2);
            assert_eq!(doc.header.model_space_block_handle, record.owner);
        } else {
            let common = raw.read_common_non_entity_data(&mut stream, kind);
            assert_eq!((common.common.handle, common.owner_handle), (record.handle.value(), record.owner.value()));
        }
        let bits: Vec<_> = (0..13).map(|_| stream.read_bit()).collect();
        assert_eq!(
            bits,
            if graphical {
                vec![true, false, true, true, false, true, false, false, true, false, true, false, true]
            } else {
                vec![true, true, false, true, false, false, true, true, false, true, false, true, true]
            }
        );
        assert_eq!(stream.read_variable_text(), if graphical { "FF1_ENTITY_UNIQUE" } else { "FF1_OBJECT_UNIQUE" });
        assert_eq!(stream.read_typed_handle(), (expected.line.value(), DwgReferenceType::HardPointer));
        stream.check_error()?;
    }
    Ok(())
}

/// Every CLI save of a DWG with `records`: kept by a same-version DWG/DWT, reported with bytes otherwise.
fn opaque_saves(records: CustomRecords) -> TestResult {
    let source = ff1::dwg_source(records)?;
    let entity: Vec<&'static str> = source.entity.as_ref().map(|e| e.name).into_iter().collect();
    let all: Vec<&'static str> = entity.iter().copied().chain(source.object.as_ref().map(|o| o.name)).collect();
    // A DWG or template in the source's version keeps the carried object; DXF can't hold its data.
    let lost = |target: &str| if target == "dxf" { all.clone() } else { entity.clone() };
    let check = |target: &str, bytes: &[u8]| -> TestResult {
        if target == "dxf" {
            return Ok(());
        }
        let doc = read_native(bytes)?;
        if let Some(o) = &source.object {
            let Some(ObjectType::Unknown { type_name, .. }) = doc.objects.get(&o.handle) else { return Err("carried object absent".into()) };
            assert_eq!(type_name, o.name);
        }
        assert!(doc.entities().all(|e| !matches!(e, EntityType::Unknown(_))), "custom entities aren't carried yet");
        Ok(())
    };
    for ext in ["dwg", "dwt"] {
        loss_matrix(&source.bytes, ext, &["dwg", "dxf", "dwt"], lost, check, |b| assert_opaque(b, &source))?;
    }
    Ok(())
}

/// A custom entity is reported by every save, with its bytes.
#[test]
fn native_entity_only_is_reported_by_every_save_with_its_bytes() -> TestResult {
    opaque_saves(CustomRecords::EntityOnly)
}

/// A custom object goes back into a same-version DWG and DWT and is reported by DXF.
#[test]
fn native_object_only_goes_back_into_dwg_and_dwt_and_is_reported_by_dxf() -> TestResult {
    opaque_saves(CustomRecords::ObjectOnly)
}

/// A custom entity and object together are kept or reported by every save.
#[test]
fn native_combined_customs_are_kept_or_reported_by_every_save() -> TestResult {
    opaque_saves(CustomRecords::Combined)
}

/// The source's extended data on the LINE.
fn xdata(line: acadrust::Handle) -> ExtendedData {
    let mut record = ExtendedDataRecord::new(APP);
    record.values = vec![
        XDataValue::String("ff1-cli-unique".into()),
        XDataValue::Integer16(137),
        XDataValue::Handle(line),
        XDataValue::String("ordered-tail".into()),
    ];
    let mut data = ExtendedData::new();
    data.add_record(record);
    data
}

/// BLOCK-marker metadata is isolated from custom records and supported table metadata.
/// The positive control has table XDATA, named entity colour and a known reactor target.
fn metadata_source(version: DxfVersion, marker_only: bool) -> TestResult<Vec<u8>> {
    let mut doc = CadDocument::with_version(version);
    let line = doc.add_entity(EntityType::Line(Line::from_coords(1., 2., 0., 11., 12., 0.)))?;
    let mut app = AppId::new(APP);
    app.handle = doc.allocate_handle();
    let app_handle = app.handle;
    doc.app_ids.add(app)?;
    let mut record = BlockRecord::new(BLOCK);
    record.handle = doc.allocate_handle();
    record.block_entity_handle = doc.allocate_handle();
    record.block_end_handle = doc.allocate_handle();
    let (owner, marker) = (record.handle, record.block_entity_handle);
    doc.block_records.add(record)?;
    let mut block = Block::new(BLOCK, acadrust::types::Vector3::ZERO);
    block.common.handle = marker;
    block.common.owner_handle = owner;
    if marker_only {
        block.common.extended_data = xdata(line);
    } else {
        for handle in
            [owner, app_handle, doc.layers.get("0").ok_or("LAYER absent")?.handle, doc.text_styles.get("Standard").ok_or("STYLE absent")?.handle]
        {
            doc.object_xdata.insert(handle, xdata(line));
        }
        let target = doc.allocate_handle();
        let nod = doc.header.named_objects_dict_handle;
        let mut reactor = XRecord::new();
        reactor.handle = target;
        reactor.owner = nod;
        reactor.entries = vec![XRecordEntry::new(1, XRecordValue::String("ff1-cli-reactor".into()))];
        doc.objects.insert(target, ObjectType::XRecord(reactor));
        let Some(ObjectType::Dictionary(dictionary)) = doc.objects.get_mut(&nod) else { return Err("NOD absent".into()) };
        dictionary.entries.push(("FF1_CLI_REACTOR".into(), target));
        let common = doc.get_entity_mut(line).ok_or("LINE absent")?.common_mut();
        common.reactors.push(target);
        common.color = acadrust::Color::Rgb { r: 12, g: 34, b: 56 };
        common.color_name = Some("FF1Book$FF1Color".into());
    }
    doc.add_entity(EntityType::Block(block))?;
    Ok(DwgWriter::write_to_vec(&doc)?)
}

/// Assert a saved DWG keeps the supported metadata and the LINE moved by `dx`.
fn assert_native_metadata(bytes: &[u8], marker_only: bool, dx: f64) -> TestResult {
    let doc = read_native(bytes)?;
    assert!(doc.entities().all(|e| !matches!(e, EntityType::Unknown(_))));
    assert!(doc.objects.values().all(|o| !matches!(o, ObjectType::Unknown { .. })));
    let lines: Vec<_> = doc.entities().filter_map(|e| if let EntityType::Line(l) = e { Some(l) } else { None }).collect();
    assert_eq!(lines.len(), 1);
    let line = lines[0];
    assert_eq!((line.start.x, line.start.y, line.end.x, line.end.y), (1. + dx, 2., 11. + dx, 12.));
    let record = doc.block_records.get(BLOCK).ok_or("BLOCK_RECORD absent")?;
    let Some(EntityType::Block(marker)) = doc.get_entity(record.block_entity_handle) else { return Err("BLOCK marker absent".into()) };
    assert_eq!(marker.common.owner_handle, record.handle);
    let expected = xdata(line.common.handle);
    if marker_only {
        assert_eq!(marker.common.extended_data.records(), expected.records());
        assert!(!doc.object_xdata.contains_key(&record.block_entity_handle));
        assert!(line.common.reactors.is_empty());
    } else {
        assert!(marker.common.extended_data.records().is_empty());
        for handle in [
            record.handle,
            doc.app_ids.get(APP).ok_or("APPID absent")?.handle,
            doc.layers.get("0").ok_or("LAYER absent")?.handle,
            doc.text_styles.get("Standard").ok_or("STYLE absent")?.handle,
        ] {
            assert_eq!(doc.object_xdata.get(&handle).ok_or("table XDATA absent")?.records(), expected.records());
        }
        assert_eq!(line.common.color, acadrust::Color::Rgb { r: 12, g: 34, b: 56 });
        assert_eq!(line.common.color_name.as_deref(), Some("FF1Book$FF1Color"));
        assert_eq!(line.common.reactors.len(), 1);
        let Some(ObjectType::XRecord(target)) = doc.objects.get(&line.common.reactors[0]) else { return Err("reactor target absent".into()) };
        assert_eq!(
            doc.objects
                .values()
                .filter(
                    |o| matches!(o, ObjectType::XRecord(x) if x.entries.iter().any(|e| e.value == XRecordValue::String("ff1-cli-reactor".into())))
                )
                .count(),
            1
        );
        assert_eq!(target.entries.len(), 1);
        assert_eq!(target.entries[0].value, XRecordValue::String("ff1-cli-reactor".into()));
        let Some(ObjectType::Dictionary(owner)) = doc.objects.get(&target.owner) else { return Err("reactor owner absent".into()) };
        assert_eq!(owner.entries.iter().filter(|(n, h)| n == "FF1_CLI_REACTOR" && *h == target.handle).count(), 1);
    }
    Ok(())
}

/// BLOCK-marker metadata alone is reported by every save, at AC1018, AC1021 and AC1032.
#[test]
fn native_block_metadata_alone_is_reported_by_every_save_in_all_three_measured_versions() -> TestResult {
    for version in [DxfVersion::AC1018, DxfVersion::AC1021, DxfVersion::AC1032] {
        let bytes = metadata_source(version, true)?;
        loss_matrix(&bytes, "dwg", &["dwg", "dxf", "dwt"], |_| vec![BLOCK], |_, _| Ok(()), |b| assert_native_metadata(b, true, 0.))?;
    }
    Ok(())
}

/// Assert the LINE is moved by `dx`; returns its handle.
fn assert_line(doc: &Drawing, dx: f64) -> TestResult<Handle> {
    let lines: Vec<_> = doc.model.iter().filter_map(|e| if let EntityKind::Line(l) = &e.kind { Some((e.handle, l)) } else { None }).collect();
    assert_eq!(lines.len(), 1);
    let (handle, line) = lines[0];
    assert_eq!((line.a.x, line.a.y, line.b.x, line.b.y), (1. + dx, 2., 11. + dx, 12.));
    Ok(handle)
}

/// Assert a saved DXF keeps the custom records' payloads, classes and references, and the LINE moved by `dx`.
fn assert_dxf_customs(bytes: &[u8], records: CustomRecords, dx: f64) -> TestResult {
    let doc = cadcraft_io::read(bytes, "source.dxf")?;
    let line = assert_line(&doc, dx)?;
    let entity = !matches!(records, CustomRecords::ObjectOnly);
    let object = !matches!(records, CustomRecords::EntityOnly);
    assert!(!doc.preservation.blocks_save());
    assert_eq!(doc.foreign.classes.len(), usize::from(entity) + usize::from(object));
    assert_eq!(doc.foreign.objects.len(), usize::from(object));
    let unknowns: Vec<_> = doc.model.iter().filter_map(|e| if let EntityKind::Unknown(u) = &e.kind { Some(u) } else { None }).collect();
    assert_eq!(unknowns.len(), usize::from(entity));
    for (name, present, graphical, payload, raw) in
        [("FF1_WIDGET", entity, "1", "FF1_ENTITY_UNIQUE", "B4A8"), ("FF1_OBJECT", object, "0", "FF1_OBJECT_UNIQUE", "D358")]
    {
        let classes: Vec<_> = doc.foreign.classes.iter().filter(|c| c.iter().any(|t| t.code == 1 && t.value == name)).collect();
        assert_eq!(classes.len(), usize::from(present));
        if !present {
            continue;
        }
        let class = classes[0];
        for (code, value) in [(2, format!("Rust{name}")), (3, "FF1_SYNTHETIC".into()), (281, graphical.into())] {
            assert_eq!(class.iter().filter(|t| t.code == code && t.value == value).count(), 1);
        }
        let tags = if name == "FF1_WIDGET" { &unknowns[0].tags } else { &doc.foreign.objects[0].tags };
        for (code, value) in [(1, payload.to_string()), (310, raw.into()), (90, "13".into()), (340, line.hex())] {
            assert_eq!(tags.iter().filter(|t| t.code == code && t.value == value).count(), 1, "{name}: {code} {value}");
        }
    }
    let tags = cadcraft_dxf::parse(bytes)?;
    let raw_records = cadcraft_dxf::records(&tags);
    if entity {
        let model = raw_records
            .iter()
            .find(|(k, body)| k == "BLOCK_RECORD" && body.iter().any(|t| t.code == 2 && t.str() == "*Model_Space"))
            .ok_or("model owner absent")?;
        let owner = model.1.iter().find(|t| t.code == 5).ok_or("model owner handle absent")?.str();
        let widget = raw_records.iter().find(|(k, _)| k == "FF1_WIDGET").ok_or("widget absent")?;
        assert_eq!(widget.1.iter().filter(|t| t.code == 330).map(|t| t.str()).collect::<Vec<_>>(), [owner]);
    }
    if object {
        let object = &doc.foreign.objects[0];
        assert_eq!(doc.foreign.dictionary, [("FF1_OBJECT".into(), object.handle)]);
        let owner = object.tags.iter().find(|t| t.code == 330).ok_or("object owner absent")?.value.as_str();
        assert!(raw_records.iter().any(|(kind, body)| kind == "DICTIONARY"
            && body.iter().any(|t| t.code == 5 && t.str() == owner)
            && body.windows(2).any(|w| w[0].code == 3 && w[0].str() == "FF1_OBJECT" && w[1].code == 350 && w[1].str() == object.handle.hex())));
    }
    Ok(())
}

/// Original literal native DXF: marker XDATA without any custom entity/object/CLASS.
fn dxf_block_source() -> String {
    format!(
        "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1032\n0\nENDSEC\n\
         0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nAPPID\n70\n1\n\
         0\nAPPID\n5\n10\n100\nAcDbSymbolTableRecord\n100\nAcDbRegAppTableRecord\n2\n{APP}\n70\n0\n0\nENDTAB\n\
         0\nTABLE\n2\nBLOCK_RECORD\n70\n1\n0\nBLOCK_RECORD\n5\n20\n100\nAcDbSymbolTableRecord\n100\nAcDbBlockTableRecord\n2\n{BLOCK}\n70\n0\n0\nENDTAB\n0\nENDSEC\n\
         0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n5\n21\n330\n20\n100\nAcDbEntity\n8\n0\n100\nAcDbBlockBegin\n2\n{BLOCK}\n70\n0\n10\n0\n20\n0\n30\n0\n3\n{BLOCK}\n1\n\n\
         1001\n{APP}\n1000\nff1-cli-unique\n1070\n137\n1005\n2A\n1000\nordered-tail\n\
         0\nENDBLK\n5\n22\n330\n20\n100\nAcDbEntity\n8\n0\n100\nAcDbBlockEnd\n0\nENDSEC\n\
         0\nSECTION\n2\nENTITIES\n0\nLINE\n5\n2A\n100\nAcDbEntity\n8\n0\n100\nAcDbLine\n10\n1\n20\n2\n30\n0\n11\n11\n21\n12\n31\n0\n0\nENDSEC\n0\nEOF\n"
    )
}

/// Assert a saved DXF keeps the BLOCK marker's XDATA and the LINE moved by `dx`.
fn assert_dxf_block(bytes: &[u8], dx: f64) -> TestResult {
    let doc = cadcraft_io::read(bytes, "source.dxf")?;
    let line = assert_line(&doc, dx)?;
    assert!(!doc.preservation.blocks_save());
    assert!(doc.foreign.classes.is_empty() && doc.foreign.objects.is_empty());
    assert!(doc.model.iter().all(|e| !matches!(e.kind, EntityKind::Unknown(_))));
    let tags = cadcraft_dxf::parse(bytes)?;
    let records = cadcraft_dxf::records(&tags);
    let blocks: Vec<_> = records.iter().filter(|(kind, body)| kind == "BLOCK" && body.iter().any(|t| t.code == 2 && t.str() == BLOCK)).collect();
    assert_eq!(blocks.len(), 1);
    assert_eq!(
        blocks[0].1.iter().filter(|t| t.code >= 1000).map(|t| (t.code, t.str())).collect::<Vec<_>>(),
        [(1001, APP.into()), (1000, "ff1-cli-unique".into()), (1070, "137".into()), (1005, line.hex()), (1000, "ordered-tail".into())]
    );
    Ok(())
}

/// DXF BLOCK-marker metadata is reported by DWG/DWT saves and kept by DXF.
#[test]
fn dxf_block_metadata_alone_is_reported_by_dwg_dwt_and_kept_by_native_dxf() -> TestResult {
    let source = dxf_block_source();
    loss_matrix(source.as_bytes(), "dxf", &["dwg", "dwt"], |_| vec![BLOCK], |_, _| Ok(()), |b| assert_dxf_block(b, 0.))?;
    two_edited_saves(source.as_bytes(), "dxf", "dxf", assert_dxf_block)
}

/// DXF custom records are reported with their groups by DWG and DWT saves.
#[test]
fn native_dxf_customs_are_reported_with_their_groups_by_dwg_and_dwt_saves() -> TestResult {
    for records in [CustomRecords::EntityOnly, CustomRecords::ObjectOnly, CustomRecords::Combined] {
        let source = ff1::dxf_source(records);
        let mut names: Vec<&'static str> = Vec::new();
        if !matches!(records, CustomRecords::ObjectOnly) {
            names.push("FF1_WIDGET");
        }
        if !matches!(records, CustomRecords::EntityOnly) {
            names.push("FF1_OBJECT");
        }
        loss_matrix(source.as_bytes(), "dxf", &["dwg", "dwt"], |_| names.clone(), |_, _| Ok(()), |b| assert_dxf_customs(b, records, 0.))?;
    }
    Ok(())
}

/// Real subprocess edits and reopens: returning source bytes instead of serialization fails.
fn two_edited_saves(bytes: &[u8], source_ext: &str, target_ext: &str, verify: impl Fn(&[u8], f64) -> TestResult) -> TestResult {
    let dir = TestDir::new()?;
    let mut input = dir.0.join(format!("source.{source_ext}"));
    fs::write(&input, bytes)?;
    verify(bytes, 0.)?;
    for cycle in 1..=2 {
        let original = fs::read(&input)?;
        let command = move_command(&original)?;
        let output = dir.0.join(format!("cycle-{cycle}.{target_ext}"));
        let out = cli(&["run", path(&input), "--cmd", &command, "--save", path(&output)])?;
        success(&out);
        moved(&out)?;
        assert_eq!(fs::read(&input)?, original, "save changed input");
        let saved = fs::read(&output)?;
        verify(&saved, f64::from(cycle) * 5.)?;
        input = output;
    }
    Ok(())
}

/// DXF foreign payloads, classes and references survive two CLI edits.
#[test]
fn native_dxf_foreign_payloads_classes_and_references_survive_two_cli_edits() -> TestResult {
    let bytes = ff1::dxf_source(CustomRecords::Combined);
    two_edited_saves(bytes.as_bytes(), "dxf", "dxf", |b, dx| assert_dxf_customs(b, CustomRecords::Combined, dx))
}

/// Supported metadata and LINE edits survive two DWG, DWT and DXF saves.
#[test]
fn supported_native_metadata_and_line_edits_survive_two_dwg_dwt_and_dxf_saves() -> TestResult {
    for version in [DxfVersion::AC1018, DxfVersion::AC1021, DxfVersion::AC1032] {
        let source = metadata_source(version, false)?;
        for ext in ["dwg", "dwt", "dxf"] {
            two_edited_saves(&source, "dwg", ext, |bytes, dx| {
                if bytes.starts_with(b"AC10") {
                    assert!(bytes.starts_with(version.as_str().as_bytes()));
                    assert_native_metadata(bytes, false, dx)
                } else {
                    let doc = cadcraft_io::read(bytes, "out.dxf")?;
                    let line = assert_line(&doc, dx)?;
                    assert!(!doc.preservation.blocks_save());
                    let tags = cadcraft_dxf::parse(bytes)?;
                    let records = cadcraft_dxf::records(&tags);
                    let line_tags = &records.iter().find(|(k, _)| k == "LINE").ok_or("LINE tags absent")?.1;
                    assert_eq!(line_tags.iter().filter(|t| t.code == 430).map(|t| t.str()).collect::<Vec<_>>(), ["FF1Book$FF1Color"]);
                    assert_eq!(line_tags.iter().filter(|t| t.code == 420).map(|t| t.str()).collect::<Vec<_>>(), ["795192"]);
                    let start = line_tags.iter().position(|t| t.code == 102 && t.str() == "{ACAD_REACTORS").ok_or("reactors absent")?;
                    let refs: Vec<_> = line_tags.iter().skip(start + 1).take_while(|t| t.code != 102).collect();
                    assert_eq!(refs.len(), 1);
                    assert_eq!(refs[0].code, 330);
                    let targets: Vec<_> =
                        records.iter().filter(|(k, b)| k == "XRECORD" && b.iter().any(|t| t.code == 1 && t.str() == "ff1-cli-reactor")).collect();
                    assert_eq!(targets.len(), 1);
                    let target = &targets[0].1;
                    assert_eq!(target.iter().filter(|t| t.code == 5).map(|t| t.str()).collect::<Vec<_>>(), [refs[0].str()]);
                    let owner = target.iter().find(|t| t.code == 330).ok_or("reactor owner absent")?.str();
                    assert!(records.iter().any(|(k, b)| k == "DICTIONARY"
                        && b.iter().any(|t| t.code == 5 && t.str() == owner)
                        && b.windows(2).any(|w| w[0].code == 3
                            && w[0].str() == "FF1_CLI_REACTOR"
                            && matches!(w[1].code, 350 | 360)
                            && w[1].str() == refs[0].str())));
                    for (kind, name) in [("BLOCK_RECORD", BLOCK), ("APPID", APP), ("LAYER", "0"), ("STYLE", "Standard")] {
                        let matches: Vec<_> =
                            records.iter().filter(|(k, body)| k == kind && body.iter().any(|t| t.code == 2 && t.str() == name)).collect();
                        assert_eq!(matches.len(), 1, "{kind} {name}");
                        assert_eq!(
                            matches[0].1.iter().filter(|t| t.code >= 1000).map(|t| (t.code, t.str())).collect::<Vec<_>>(),
                            [
                                (1001, APP.into()),
                                (1000, "ff1-cli-unique".into()),
                                (1070, "137".into()),
                                (1005, line.hex()),
                                (1000, "ordered-tail".into())
                            ]
                        );
                    }
                    Ok(())
                }
            })?;
        }
    }
    Ok(())
}

/// A partial import still supports `info` and image rendering.
#[test]
fn partial_native_import_still_supports_cli_info_and_image_rendering() -> TestResult {
    let source = ff1::dwg_source(CustomRecords::Combined)?;
    let dir = TestDir::new()?;
    let input = dir.0.join("source.dwg");
    fs::write(&input, &source.bytes)?;
    let info = cli(&["info", path(&input)])?;
    success(&info);
    let info: serde_json::Value = serde_json::from_slice(&info.stdout)?;
    assert_eq!(info["counts"]["Line"], 1);
    for ext in ["svg", "png", "pdf"] {
        let output = dir.0.join(format!("render.{ext}"));
        let out = cli(&["convert", path(&input), path(&output)])?;
        success(&out);
        let bytes = fs::read(output)?;
        assert!(match ext {
            "png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
            "pdf" => bytes.starts_with(b"%PDF-"),
            _ => String::from_utf8_lossy(&bytes).contains("<svg"),
        });
    }
    assert_eq!(fs::read(&input)?.as_slice(), source.bytes.as_ref());
    assert_opaque(&fs::read(input)?, &source)
}

/// `audit` says what saving a file would keep and lose, without writing it: the loss report's
/// summary, the record counts that differ in the saved file, and the full JSON on request.
#[test]
fn audit_reports_what_a_save_keeps_and_loses_without_writing() -> TestResult {
    let dir = TestDir::new()?;
    let src = ff1::display_source()?;
    let source = dir.0.join("display.dwg");
    fs::write(&source, &src.bytes)?;
    let report = dir.0.join("audit.json");
    let out = cli(&["audit", path(&source), "--report", path(&report)])?;
    success(&out);
    let text = String::from_utf8_lossy(&out.stdout);
    for expected in [
        "DWG AC1032 → saved in memory as DWG AC1032 (nothing written)",
        "kept as they were: 3 records",
        "FF1_ODD × 1",
        "object FF1_ODD: 1 → 0  (fewer)",
    ] {
        assert!(text.contains(expected), "missing `{expected}`: {text}");
    }
    let json: serde_json::Value = serde_json::from_slice(&fs::read(&report)?)?;
    assert_eq!((json["kept_natively"].as_u64(), json["lost"].as_u64()), (Some(3), Some(1)));
    assert!(json["report"]["records"][0]["dwg"]["data_base64"].as_str().is_some_and(|d| !d.is_empty()));
    // Nothing but the source and the requested report: the audit writes no drawing.
    assert_eq!(fs::read_dir(&dir.0)?.count(), 2);
    assert_eq!(fs::read(&source)?, src.bytes.as_ref());
    // As DXF: everything of the other program is listed.
    let out = cli(&["audit", path(&source), "--as", "dxf"])?;
    success(&out);
    assert!(String::from_utf8_lossy(&out.stdout).contains("4 record(s) of the source aren't in this DXF"));
    Ok(())
}
