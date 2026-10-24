//! Independent original metadata fixtures. Each source exercises one metadata family only.
//! The native reader shares the writer's acadrust codec; this is not AutoCAD validation.

use acadrust::entities::Line;
use acadrust::io::dwg::{DwgVersion, dwg_reference_type::DwgReferenceType, dwg_stream_writers::DwgMergedWriter};
use acadrust::objects::{ObjectType, XRecord, XRecordEntry, XRecordValue};
use acadrust::{CadDocument, Color, EntityType, Handle};

use super::ff1::TestResult;

pub const BOOK: &str = "FF1Book$FF1Color";
pub const REACTOR: &str = "FF1_REACTOR";
pub const REACTOR_PAYLOAD: &str = "reactor-target-unique";

#[derive(Clone, Copy, Debug)]
pub enum Family {
    EntityColour,
    LayerColour,
    KnownReactor,
    CustomReactor,
}

pub struct Source {
    pub doc: CadDocument,
    pub line: Handle,
    pub target: Handle,
    pub raw: Option<Vec<u8>>,
}

/// A source at `version` with the supported metadata of `family`.
pub fn source(version: acadrust::DxfVersion, family: Family) -> TestResult<Source> {
    let mut doc = CadDocument::with_version(version);
    let line = doc.add_entity(EntityType::Line(Line::from_coords(1., 2., 0., 11., 12., 0.)))?;
    let mut target = line;
    let mut raw = None;
    match family {
        Family::EntityColour => {
            let common = doc.get_entity_mut(line).ok_or("LINE absent")?.common_mut();
            common.color = Color::Rgb { r: 12, g: 34, b: 56 };
            common.color_name = Some(BOOK.into());
        }
        Family::LayerColour => {
            let layer = doc.layers.get_mut("0").ok_or("LAYER absent")?;
            layer.color = Color::Rgb { r: 12, g: 34, b: 56 };
            layer.book_name = Some("FF1Book".into());
            layer.color_name = Some("FF1Color".into());
            target = layer.handle;
        }
        Family::KnownReactor | Family::CustomReactor => {
            target = doc.allocate_handle();
            let owner = doc.header.named_objects_dict_handle;
            let object = if matches!(family, Family::KnownReactor) {
                let mut record = XRecord::new();
                record.handle = target;
                record.owner = owner;
                record.entries = vec![
                    XRecordEntry::new(1, XRecordValue::String(REACTOR_PAYLOAD.into())),
                    XRecordEntry::new(90, XRecordValue::Int32(137)),
                    XRecordEntry::new(1, XRecordValue::String("reactor-ordered-tail".into())),
                ];
                ObjectType::XRecord(record)
            } else {
                // Keep this raw record's class profile, rather than regenerating a profile
                // for a new document (notably in AC1018/AC1021).
                doc.dwg_source_version = Some(version);
                let mut class = acadrust::classes::DxfClass::new(REACTOR, "RustFF1Reactor");
                class.application_name = "FF1_SYNTHETIC".into();
                doc.classes.add_or_update(class);
                let class = doc.classes.get_by_name(REACTOR).ok_or("CLASS absent")?.class_number;
                let dwg_version = DwgVersion::from_dxf_version(version)?;
                let mut writer = DwgMergedWriter::new(dwg_version, version);
                writer.write_object_type(class);
                if dwg_version.r2000_plus() && !dwg_version.r2010_plus() {
                    writer.save_position_for_size();
                }
                writer.main_mut().write_handle_undefined(target.value());
                writer.write_bit_short(0); // No EED.
                writer.write_handle(DwgReferenceType::SoftPointer, owner.value());
                writer.write_bit_long(0); // No reactors on the target itself.
                writer.write_bit(true); // No extension dictionary.
                if dwg_version.r2013_plus(version) {
                    writer.write_bit(false); // No binary data.
                }
                writer.write_variable_text(REACTOR_PAYLOAD);
                writer.write_handle(DwgReferenceType::HardPointer, line.value());
                let bytes = writer.merge();
                let bits = i64::try_from(bytes.len())?.checked_mul(8).ok_or("payload overflow")? - writer.handle_start_bits();
                raw = Some(bytes.clone());
                ObjectType::Unknown {
                    type_name: REACTOR.into(),
                    handle: target,
                    owner,
                    raw_dxf_codes: None,
                    raw_dwg_data: Some(bytes),
                    raw_dwg_handle_bits: bits,
                    raw_dwg_version: Some(version),
                }
            };
            doc.objects.insert(target, object);
            let Some(ObjectType::Dictionary(nod)) = doc.objects.get_mut(&owner) else { return Err("NOD absent".into()) };
            nod.entries.push((REACTOR.into(), target));
            doc.get_entity_mut(line).ok_or("LINE absent")?.common_mut().reactors.push(target);
        }
    }
    Ok(Source { doc, line, target, raw })
}

/// Assert a native document keeps `family`'s metadata, the LINE ending at `end`.
pub fn assert_native(doc: &CadDocument, family: Family, end: f64) -> TestResult {
    let lines: Vec<_> = doc.entities().filter_map(|e| if let EntityType::Line(l) = e { Some(l) } else { None }).collect();
    assert_eq!(lines.len(), 1);
    let line = lines[0];
    assert_eq!((line.start.x, line.start.y, line.end.x, line.end.y), (1., 2., end, 12.));
    match family {
        Family::EntityColour => {
            assert_eq!(line.common.color, Color::Rgb { r: 12, g: 34, b: 56 });
            assert_eq!(line.common.color_name.as_deref(), Some(BOOK));
        }
        Family::LayerColour => {
            let layer = doc.layers.get("0").ok_or("LAYER absent")?;
            assert_eq!(layer.color, Color::Rgb { r: 12, g: 34, b: 56 });
            assert_eq!(layer.book_name.as_deref(), Some("FF1Book"));
            assert_eq!(layer.color_name.as_deref(), Some("FF1Color"));
        }
        Family::KnownReactor | Family::CustomReactor => {
            assert_eq!(line.common.reactors.len(), 1);
            let target = line.common.reactors[0];
            let owner = match doc.objects.get(&target).ok_or("reactor target absent")? {
                ObjectType::XRecord(record) if matches!(family, Family::KnownReactor) => {
                    assert_eq!(doc.objects.values().filter(|o| matches!(o, ObjectType::XRecord(x) if x.entries.iter().any(|e| e.value == XRecordValue::String(REACTOR_PAYLOAD.into())))).count(), 1, "duplicated unique reactor payload");
                    assert_eq!(
                        record.entries.iter().map(|e| e.value.clone()).collect::<Vec<_>>(),
                        [XRecordValue::String(REACTOR_PAYLOAD.into()), XRecordValue::Int32(137), XRecordValue::String("reactor-ordered-tail".into()),]
                    );
                    record.owner
                }
                ObjectType::Unknown { type_name, owner, raw_dwg_data, .. } if matches!(family, Family::CustomReactor) => {
                    assert_eq!(type_name, REACTOR);
                    assert!(raw_dwg_data.as_ref().is_some_and(|r| !r.is_empty()));
                    assert_eq!(doc.classes.iter().filter(|c| c.dxf_name == REACTOR).count(), 1);
                    let class = doc.classes.get_by_name(REACTOR).ok_or("CLASS absent")?;
                    assert_eq!(class.cpp_class_name, "RustFF1Reactor");
                    assert_eq!(class.application_name, "FF1_SYNTHETIC");
                    assert!(!class.is_an_entity);
                    *owner
                }
                _ => return Err("wrong reactor target type".into()),
            };
            let Some(ObjectType::Dictionary(nod)) = doc.objects.get(&owner) else { return Err("reactor owner absent".into()) };
            assert_eq!(nod.entries.iter().filter(|(n, h)| n == REACTOR && *h == target).count(), 1);
        }
    }
    Ok(())
}

/// Assert DXF tags keep `family`'s metadata, the LINE ending at `end`.
pub fn assert_tags(bytes: &[u8], family: Family, end: f64) -> TestResult {
    use cadcraft_dxf::{Tag, records};
    let tags = cadcraft_dxf::parse(bytes)?;
    let records = records(&tags);
    let value = |body: &[Tag], code| body.iter().find(|t| t.code == code).map(Tag::str);
    let lines: Vec<_> = records.iter().filter(|(k, _)| k == "LINE").collect();
    assert_eq!(lines.len(), 1);
    let line = &lines[0].1;
    assert_eq!(line.iter().find(|t| t.code == 11).ok_or("LINE endpoint absent")?.f64(), end);
    assert_eq!(records.iter().filter(|(k, b)| k == "CLASS" && value(b, 1).as_deref() == Some(REACTOR)).count(), 0);
    match family {
        Family::EntityColour | Family::LayerColour => {
            let body = if matches!(family, Family::EntityColour) {
                line
            } else {
                &records.iter().find(|(k, b)| k == "LAYER" && value(b, 2).as_deref() == Some("0")).ok_or("LAYER absent")?.1
            };
            assert_eq!(body.iter().filter(|t| t.code == 430).map(Tag::str).collect::<Vec<_>>(), [BOOK]);
            assert_eq!(value(body, 420).as_deref(), Some("795192"));
        }
        Family::KnownReactor => {
            assert_eq!(
                records.iter().filter(|(k, b)| k == "XRECORD" && b.iter().any(|t| t.code == 1 && t.str() == REACTOR_PAYLOAD)).count(),
                1,
                "duplicated unique reactor payload"
            );
            let start = line.iter().position(|t| t.code == 102 && t.str() == "{ACAD_REACTORS").ok_or("reactor group absent")?;
            let references: Vec<_> = line.iter().skip(start + 1).take_while(|t| t.code != 102).collect();
            assert_eq!(references.len(), 1);
            assert_eq!(references[0].code, 330);
            let target = references[0].str();
            let targets: Vec<_> = records.iter().filter(|(k, b)| k == "XRECORD" && value(b, 5).as_deref() == Some(target.as_str())).collect();
            assert_eq!(targets.len(), 1);
            let body = &targets[0].1;
            assert_eq!(
                body.iter().filter(|t| matches!(t.code, 1 | 90)).map(|t| (t.code, t.str())).collect::<Vec<_>>(),
                [(1, REACTOR_PAYLOAD.into()), (90, "137".into()), (1, "reactor-ordered-tail".into()),]
            );
            let owner = value(body, 330).ok_or("XRECORD owner absent")?;
            let nod = &records.iter().find(|(k, b)| k == "DICTIONARY" && value(b, 5).as_deref() == Some(owner.as_str())).ok_or("owner absent")?.1;
            assert_eq!(
                nod.windows(2).filter(|w| w[0].code == 3 && w[0].str() == REACTOR && matches!(w[1].code, 350 | 360) && w[1].str() == target).count(),
                1
            );
        }
        Family::CustomReactor => return Err("custom reactor has no supported saved output".into()),
    }
    Ok(())
}
