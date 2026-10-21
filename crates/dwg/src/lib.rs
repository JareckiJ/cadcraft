//! DWG support through a DXF bridge.
//!
//! A DWG file is converted to DXF bytes, which CADCraft's own DXF reader maps to its document
//! model; saving goes the other way. Two engines do the conversion, behind this byte-level API so
//! that nothing else in CADCraft depends on their types:
//!
//! 1. **BlackCAD's open DWG and DXF crates** (`cad-dwg`, `cad-dxf`, MIT OR Apache-2.0), tried
//!    first. They read DWG from R13 to 2018 and later except 2007 (AC1021), write AutoCAD 2018
//!    (AC1032) DWG, and count what a conversion leaves out. They use the standard library alone,
//!    so the web build reads and writes DWG with them too.
//! 2. **The `acadrust` crate** (MPL-2.0, used unmodified as a dependency; native targets only, as
//!    it memory-maps files), for whatever BlackCAD doesn't convert whole: a version it doesn't
//!    read or write, a file it can't read, or one it would carry over only in part.
//!
//! So a conversion never loses what acadrust would have kept: BlackCAD's result is used when it
//! is complete, acadrust's otherwise, and BlackCAD's partial one (with notes on what it left
//! out) only when acadrust can't convert the file at all.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod blackcad;

#[cfg(not(target_arch = "wasm32"))]
mod acadrust_bridge;

/// DWG version codes we can write.
pub const VERSIONS: &[(&str, &str)] =
    &[("AC1015", "2000"), ("AC1018", "2004"), ("AC1021", "2007"), ("AC1024", "2010"), ("AC1027", "2013"), ("AC1032", "2018")];

/// The version BlackCAD's writer writes; the others are acadrust's.
const BLACKCAD_WRITES: &str = "AC1032";

/// True when the bytes look like a DWG file (`AC10xx` magic).
pub fn is_dwg(bytes: &[u8]) -> bool {
    bytes.len() > 6 && bytes.starts_with(b"AC10") && bytes.get(4..6).is_some_and(|v| v.iter().all(u8::is_ascii_digit))
}

/// The DWG version code of a file (e.g. "AC1032").
pub fn version(bytes: &[u8]) -> Option<String> {
    is_dwg(bytes).then(|| String::from_utf8_lossy(bytes.get(0..6).unwrap_or_default()).to_string())
}

/// Size limits for reading a DWG file. DWG is compressed and the DXF text it converts to is
/// several times larger (about 6x in our tests: a 10 MB DWG becomes about 60 MB of DXF), so
/// the limits are generous for real drawings. They exist because a damaged file, or one the reader
/// misreads, can make the DWG reader produce a drawing thousands of times larger than the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Largest DWG file read at all (checked before reading starts).
    pub max_dwg_bytes: usize,
    /// Largest DXF rendition of a DWG (checked while it is written).
    pub max_dxf_bytes: usize,
}

impl Limits {
    /// 512 MB of DWG, 1 GB of DXF (more than the DXF reader's group-code limit fills).
    pub const DEFAULT: Limits = Limits { max_dwg_bytes: 512 << 20, max_dxf_bytes: 1 << 30 };
}

/// A byte count in megabytes for messages ("9.9 MB", "1024 MB").
fn mb(n: u64) -> String {
    let m = n as f64 / f64::from(1u32 << 20);
    if m >= 100.0 { format!("{m:.0} MB") } else { format!("{m:.1} MB") }
}

/// The DWG engines, in the order they're tried.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Engine {
    /// BlackCAD's open DWG and DXF crates: tried first, on every target.
    BlackCad,
    /// acadrust: for what BlackCAD doesn't convert whole (native builds).
    Acadrust,
}

/// A DWG file converted to DXF.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Converted {
    /// The drawing as ASCII DXF.
    pub dxf: Vec<u8>,
    /// What the conversion left out, one note each; empty when it carried everything over.
    pub notes: Vec<String>,
    /// The engine that converted it.
    pub engine: Engine,
}

/// Read DWG bytes and return an ASCII DXF rendition, within [`Limits::DEFAULT`].
pub fn dwg_to_dxf(bytes: &[u8]) -> Result<Vec<u8>, String> {
    dwg_to_dxf_with(bytes, Limits::DEFAULT)
}

/// [`dwg_to_dxf`] with explicit limits. Both are checked before the expensive step they
/// guard: the input size before a DWG reader runs, the DXF size before it is used, so a
/// runaway conversion stops at the limit instead of filling memory later.
pub fn dwg_to_dxf_with(bytes: &[u8], limits: Limits) -> Result<Vec<u8>, String> {
    dwg_to_dxf_noted(bytes, limits).map(|(dxf, _)| dxf)
}

/// [`dwg_to_dxf_with`], also returning notes on what the read left out, so a partial read never
/// passes for a complete one.
pub fn dwg_to_dxf_noted(bytes: &[u8], limits: Limits) -> Result<(Vec<u8>, Vec<String>), String> {
    convert_dwg(bytes, limits).map(|c| (c.dxf, c.notes))
}

/// [`dwg_to_dxf_noted`], also saying which engine converted the file: BlackCAD when it converts
/// it whole, otherwise acadrust (native builds), otherwise BlackCAD's partial conversion with
/// its notes. The error, when neither converts the file, is acadrust's (BlackCAD's in the web
/// build).
pub fn convert_dwg(bytes: &[u8], limits: Limits) -> Result<Converted, String> {
    if bytes.len() > limits.max_dwg_bytes {
        return Err(too_large(bytes, limits));
    }
    let (partial, blackcad_error) = match blackcad::dwg_to_dxf(bytes, limits) {
        Ok(blackcad::Outcome::Whole(dxf)) => return Ok(Converted { dxf, notes: Vec::new(), engine: Engine::BlackCad }),
        Err(blackcad::Failure::Limit(e)) => return Err(e),
        Ok(blackcad::Outcome::Partial(dxf, notes)) => (Some((dxf, notes)), None),
        Err(blackcad::Failure::Failed(e)) => (None, Some(e)),
    };
    match (acadrust_dwg_to_dxf(bytes, limits), partial) {
        (Ok((dxf, notes)), _) => Ok(Converted { dxf, notes, engine: Engine::Acadrust }),
        (Err(_), Some((dxf, notes))) => Ok(Converted { dxf, notes, engine: Engine::BlackCad }),
        (Err(e), None) => Err(if cfg!(target_arch = "wasm32") { blackcad_error.unwrap_or(e) } else { e }),
    }
}

/// [`convert_dwg`] with one engine only, whatever it leaves out.
pub fn convert_dwg_with(engine: Engine, bytes: &[u8], limits: Limits) -> Result<Converted, String> {
    if bytes.len() > limits.max_dwg_bytes {
        return Err(too_large(bytes, limits));
    }
    match engine {
        Engine::BlackCad => match blackcad::dwg_to_dxf(bytes, limits) {
            Ok(blackcad::Outcome::Whole(dxf)) => Ok(Converted { dxf, notes: Vec::new(), engine }),
            Ok(blackcad::Outcome::Partial(dxf, notes)) => Ok(Converted { dxf, notes, engine }),
            Err(blackcad::Failure::Limit(e) | blackcad::Failure::Failed(e)) => Err(e),
        },
        Engine::Acadrust => acadrust_dwg_to_dxf(bytes, limits).map(|(dxf, notes)| Converted { dxf, notes, engine }),
    }
}

/// The error for a DWG file over `Limits::max_dwg_bytes`.
fn too_large(bytes: &[u8], limits: Limits) -> String {
    format!("DWG: the file is {}, larger than the {} CADCraft opens", mb(bytes.len() as u64), mb(limits.max_dwg_bytes as u64))
}

/// Convert DXF bytes into a DWG file of the DXF's version, and at least AutoCAD 2004
/// (AC1018). A 2018 DXF is written by BlackCAD when it carries the drawing over whole.
pub fn dxf_to_dwg(dxf: &[u8]) -> Result<Vec<u8>, String> {
    let blackcad = declared_version(dxf).is_some_and(|v| v == BLACKCAD_WRITES);
    write_dwg(dxf, blackcad, None)
}

/// Convert DXF bytes into a DWG file of `version`, a code from [`VERSIONS`] ("AC1032"),
/// whatever version the DXF declares. 2018 (AC1032) is written by BlackCAD when it carries the
/// drawing over whole, every other version by acadrust.
pub fn dxf_to_dwg_as(dxf: &[u8], version: &str) -> Result<Vec<u8>, String> {
    let code = version.trim().to_ascii_uppercase();
    if !VERSIONS.iter().any(|(c, _)| *c == code) {
        return Err(unsupported(version));
    }
    write_dwg(dxf, code == BLACKCAD_WRITES, Some(&code))
}

/// BlackCAD first when it writes the version asked for, then acadrust. When acadrust can't
/// write the drawing either, the error says what BlackCAD would have left out.
fn write_dwg(dxf: &[u8], blackcad: bool, version: Option<&str>) -> Result<Vec<u8>, String> {
    let mut left_out = None;
    if blackcad {
        match blackcad::dxf_to_dwg(dxf) {
            Ok(blackcad::Outcome::Whole(dwg)) => return Ok(dwg),
            Ok(blackcad::Outcome::Partial(_, notes)) => left_out = Some(notes),
            Err(_) => {}
        }
    }
    acadrust_dxf_to_dwg(dxf, version).map_err(|e| match left_out {
        Some(notes) => format!("{e} (BlackCAD's DWG writer would have left out: {})", notes.join("; ")),
        None => e,
    })
}

/// The version an ASCII DXF declares in `$ACADVER` (the header comes first), if it does.
fn declared_version(dxf: &[u8]) -> Option<String> {
    let head = String::from_utf8_lossy(dxf.get(..dxf.len().min(64 << 10)).unwrap_or_default());
    let mut lines = head.lines().map(str::trim);
    while let Some(line) = lines.next() {
        if line.eq_ignore_ascii_case("$ACADVER") {
            let _code = lines.next()?;
            return lines.next().map(str::to_ascii_uppercase);
        }
    }
    None
}

#[cfg(not(target_arch = "wasm32"))]
fn acadrust_dwg_to_dxf(bytes: &[u8], limits: Limits) -> Result<(Vec<u8>, Vec<String>), String> {
    acadrust_bridge::dwg_to_dxf_noted(bytes, limits)
}

#[cfg(not(target_arch = "wasm32"))]
fn acadrust_dxf_to_dwg(dxf: &[u8], version: Option<&str>) -> Result<Vec<u8>, String> {
    match version {
        Some(v) => acadrust_bridge::dxf_to_dwg_as(dxf, v),
        None => acadrust_bridge::dxf_to_dwg(dxf),
    }
}

#[cfg(target_arch = "wasm32")]
fn acadrust_dwg_to_dxf(_bytes: &[u8], _limits: Limits) -> Result<(Vec<u8>, Vec<String>), String> {
    Err("this DWG file can't be opened in the web build; open it in the desktop app, or save it as DXF".into())
}

#[cfg(target_arch = "wasm32")]
fn acadrust_dxf_to_dwg(_dxf: &[u8], version: Option<&str>) -> Result<Vec<u8>, String> {
    Err(format!(
        "the web build writes AutoCAD 2018 DWG of what it carries over whole; {} needs the desktop app, or save as DXF",
        version.map_or_else(|| "this drawing".to_string(), |v| format!("DWG version `{v}`"))
    ))
}

/// The error for a DWG version CADCraft can't write.
fn unsupported(version: &str) -> String {
    let list: Vec<&str> = VERSIONS.iter().map(|(_, year)| *year).collect();
    format!("DWG version `{version}` can't be written (CADCraft writes {})", list.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_magic() {
        assert!(is_dwg(b"AC1032\0\0\0\0"));
        assert!(!is_dwg(b"  0\r\nSECTION"));
        assert_eq!(version(b"AC1018xxxx").as_deref(), Some("AC1018"));
    }

    #[test]
    fn garbage_is_an_error_not_a_crash() {
        assert!(dwg_to_dxf(b"AC1032 this is not a dwg file at all").is_err());
        assert!(dwg_to_dxf(b"").is_err());
    }

    /// A 2018 DXF of a line and a circle, as a simple exporter writes it: no tables, no handles.
    fn simple_dxf() -> String {
        let mut s = String::from("  0\nSECTION\n  2\nHEADER\n  9\n$ACADVER\n  1\nAC1032\n  0\nENDSEC\n  0\nSECTION\n  2\nENTITIES\n");
        s += "  0\nLINE\n  8\n0\n 10\n0\n 20\n0\n 30\n0\n 11\n100\n 21\n50\n 31\n0\n";
        s += "  0\nCIRCLE\n  8\nHOLES\n 10\n20\n 20\n20\n 30\n0\n 40\n5\n";
        s + "  0\nENDSEC\n  0\nEOF\n"
    }

    /// The (code, value) pairs of a DXF text.
    fn pairs(dxf: &str) -> Vec<(String, String)> {
        let lines: Vec<&str> = dxf.lines().map(str::trim).collect();
        lines.chunks(2).filter_map(|p| Some((p.first()?.to_string(), p.get(1)?.to_string()))).collect()
    }

    /// BlackCAD writes the 2018 DWG of a drawing it carries over whole and reads it back whole,
    /// on its engine alone (so in the web build as well), naming CADCraft as the program that
    /// saved it.
    #[test]
    fn blackcad_writes_and_reads_2018_whole() {
        let dwg = dxf_to_dwg(simple_dxf().as_bytes()).unwrap();
        assert_eq!(version(&dwg).as_deref(), Some("AC1032"));
        let name: Vec<u8> = "CADCraft".encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert!(dwg.windows(name.len()).any(|w| w == name), "the file doesn't name CADCraft");
        let c = convert_dwg(&dwg, Limits::DEFAULT).unwrap();
        assert_eq!((c.engine, c.notes.as_slice()), (Engine::BlackCad, &[] as &[String]));
        let pairs = pairs(&String::from_utf8(c.dxf).unwrap());
        let has = |code: &str, value: &str| pairs.iter().any(|(c, v)| c == code && v == value);
        assert!(has("0", "LINE") && has("0", "CIRCLE") && has("8", "HOLES"), "{pairs:?}");
        assert!(has("40", "5.0") || has("40", "5"), "the circle's radius");
        // The same through BlackCAD alone, and for the version asked for by name.
        assert_eq!(convert_dwg_with(Engine::BlackCad, &dwg, Limits::DEFAULT).unwrap().engine, Engine::BlackCad);
        assert_eq!(version(&dxf_to_dwg_as(simple_dxf().as_bytes(), "ac1032").unwrap()).as_deref(), Some("AC1032"));
    }

    #[test]
    fn limits_apply_to_both_engines() {
        let dwg = dxf_to_dwg(simple_dxf().as_bytes()).unwrap();
        let small = Limits { max_dwg_bytes: 1000, ..Limits::DEFAULT };
        let e = convert_dwg(&dwg, small).unwrap_err();
        assert!(e.starts_with("DWG: the file is 0.") && e.ends_with("MB, larger than the 0.0 MB CADCraft opens"), "{e}");
        let tiny = Limits { max_dxf_bytes: 1000, ..Limits::DEFAULT };
        let e = convert_dwg(&dwg, tiny).unwrap_err();
        assert!(e.contains("produced more than 0.0 MB of drawing data (the limit)"), "{e}");
    }

    #[test]
    fn the_declared_version_comes_from_the_header() {
        assert_eq!(declared_version(simple_dxf().as_bytes()).as_deref(), Some("AC1032"));
        assert_eq!(declared_version(b"  0\nSECTION\n  2\nENTITIES\n  0\nENDSEC\n"), None);
        assert!(dxf_to_dwg_as(simple_dxf().as_bytes(), "AC1099").unwrap_err().contains("can't be written"));
    }

    /// Versions BlackCAD doesn't write are acadrust's, and a 2007 DWG, which BlackCAD doesn't
    /// read, is read by acadrust.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn other_versions_go_to_acadrust() {
        let dwg = dxf_to_dwg_as(simple_dxf().as_bytes(), "AC1018").unwrap();
        assert_eq!(version(&dwg).as_deref(), Some("AC1018"));
        let dwg = dxf_to_dwg_as(simple_dxf().as_bytes(), "AC1021").unwrap();
        assert_eq!(version(&dwg).as_deref(), Some("AC1021"));
        assert_eq!(convert_dwg(&dwg, Limits::DEFAULT).unwrap().engine, Engine::Acadrust);
        assert!(convert_dwg_with(Engine::BlackCad, &dwg, Limits::DEFAULT).is_err());
    }

    /// A DXF with one DIMENSION per (text, group 70, subclass, points) row.
    #[cfg(not(target_arch = "wasm32"))]
    fn dims_dxf(dims: &[(&str, i64, &str, &[(i32, [f64; 2])])]) -> String {
        let mut s = String::from("  0\nSECTION\n  2\nHEADER\n  9\n$ACADVER\n  1\nAC1032\n  0\nENDSEC\n  0\nSECTION\n  2\nENTITIES\n");
        for (text, flags, sub, pts) in dims {
            s += "  0\nDIMENSION\n  8\n0\n100\nAcDbEntity\n100\nAcDbDimension\n 10\n0\n 20\n0\n 30\n0\n";
            s += &format!(" 11\n50\n 21\n-20\n 31\n0\n 70\n{flags}\n  1\n{text}\n  3\nStandard\n100\n{sub}\n");
            for (code, [x, y]) in pts.iter() {
                s += &format!("{code:>3}\n{x}\n{:>3}\n{y}\n{:>3}\n0\n", code + 10, code + 20);
            }
        }
        s + "  0\nENDSEC\n  0\nEOF\n"
    }

    /// DWG stores DXF group 70's bit 128 inverted, in bit 0 of the dimension flags byte (set:
    /// the text is at its default position), and bit 32 in bit 1 (ODA DWG specification, common
    /// dimension data). The bytes are read back raw, so a reader that inverts bit 0 the same way
    /// as the writer can't hide a wrong byte.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn dimension_text_position_flags_reach_the_dwg_as_autocad_reads_them() {
        let linear: &[(i32, [f64; 2])] = &[(13, [0.0, 0.0]), (14, [100.0, 0.0])];
        let radius: &[(i32, [f64; 2])] = &[(15, [10.0, 0.0])];
        let angular: &[(i32, [f64; 2])] = &[(13, [0.0, 0.0]), (14, [10.0, 0.0]), (15, [0.0, 0.0]), (16, [0.0, 10.0])];
        let kinds = [
            (0, "AcDbAlignedDimension", linear),
            (1, "AcDbAlignedDimension", linear),
            (4, "AcDbRadialDimension", radius),
            (2, "AcDb2LineAngularDimension", angular),
        ];
        let mut dims = Vec::new();
        for (ty, sub, pts) in kinds {
            dims.push(("HOME", ty | 32, sub, pts));
            dims.push(("MOVED", ty | 32 | 128, sub, pts));
        }
        let dwg = dxf_to_dwg(dims_dxf(&dims).as_bytes()).unwrap();
        let doc = acadrust::DwgReader::from_stream(std::io::Cursor::new(&dwg)).read().unwrap();
        let mut seen = Vec::new();
        for e in doc.entities() {
            let acadrust::entities::EntityType::Dimension(d) = e else { continue };
            let (text, byte) = (d.base().text_override().unwrap_or_default(), d.base().dwg_flags_byte);
            seen.push((text.to_string(), byte & 0x03));
            assert_eq!(byte & 0x02, 0x02, "{text} {:?}: bit 32 of group 70 is lost (flags byte {byte:#04x})", d.base().dimension_type);
            let default_pos = text == "HOME";
            assert_eq!(byte & 0x01 != 0, default_pos, "{text} {:?}: flags byte {byte:#04x}", d.base().dimension_type);
        }
        for text in ["HOME", "MOVED"] {
            assert_eq!(seen.iter().filter(|(t, _)| t == text).count(), 4, "{seen:?}");
        }

        // And CADCraft reads its own DWG back as it wrote it: 128 only on the moved texts.
        let dxf = String::from_utf8(dwg_to_dxf(&dwg).unwrap()).unwrap();
        let lines: Vec<&str> = dxf.lines().map(str::trim).collect();
        let mut flags = Vec::new();
        for rec in lines.chunks(2).collect::<Vec<_>>().split(|p| p.first() == Some(&"0")) {
            let value = |code: &str| rec.iter().find(|p| p.first() == Some(&code)).and_then(|p| p.get(1).copied());
            if let (Some(text @ ("HOME" | "MOVED")), Some(v)) = (value("1"), value("70")) {
                flags.push((text, v.parse::<i64>().unwrap_or(-1) & 128));
            }
        }
        assert_eq!(flags.len(), 8, "{flags:?}");
        assert!(flags.iter().all(|(l, f)| (*f == 128) == (*l == "MOVED")), "{flags:?}");
    }

    /// The handles a DXF text refers to or gives its records (groups 5, 105 and 330–369).
    #[cfg(not(target_arch = "wasm32"))]
    fn dxf_handles(dxf: &str) -> std::collections::HashSet<u64> {
        let lines: Vec<&str> = dxf.lines().collect();
        lines
            .chunks(2)
            .filter_map(|p| match p {
                [c, v] if matches!(c.trim().parse::<i32>(), Ok(5 | 105 | 330..=369)) => u64::from_str_radix(v.trim(), 16).ok(),
                _ => None,
            })
            .collect()
    }

    /// Issue #1482: a DWG that once came from MicroStation can carry hundreds of thousands of DGN
    /// line style components no definition uses (a real 83 MB site plan: 888,483 of 889,053, a
    /// 1.2 GB DXF rendition over the limit, so it didn't open). They're left out with a note, and
    /// their dictionary entries with them; the definition and every component it reaches —
    /// through compound and point components, around a cycle — are kept.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn orphaned_dgn_line_style_components_are_left_out() {
        use acadrust::objects::*;
        let mut doc = acadrust::CadDocument::new();
        let container = doc.allocate_handle();
        let nod = doc.header.named_objects_dict_handle;
        let Some(ObjectType::Dictionary(root)) = doc.objects.get_mut(&nod) else { panic!("no named object dictionary") };
        root.entries.push(("ACAD_DGNLINESTYLECOMP".into(), container));
        let pattern = || DgnLsStrokePattern {
            has_iteration_limit: false,
            is_single_segment: false,
            iteration_limit: 0,
            auto_phase: 0.0,
            phase: 0.0,
            phase_mode: DgnLsPhaseMode::Distance,
            strokes: vec![DgnLsStroke {
                is_dash: true,
                bypass_corner: false,
                can_be_scaled: true,
                invert_at_origin: false,
                invert_at_end: false,
                length: 2.5,
                start_width: 0.0,
                end_width: 0.0,
                width_mode: 0,
                cap_mode: 0,
            }],
        };
        let kind = |c: &DgnLsComponentData| match c {
            DgnLsComponentData::Symbol(_) => DgnLsComponentType::Symbol,
            DgnLsComponentData::Compound(_) => DgnLsComponentType::Compound,
            DgnLsComponentData::Stroke(_) => DgnLsComponentType::Stroke,
            DgnLsComponentData::Point(_) => DgnLsComponentType::Point,
            DgnLsComponentData::Internal(_) => DgnLsComponentType::Internal,
        };
        let compound = |of: &[acadrust::Handle]| {
            DgnLsComponentData::Compound(DgnLsCompoundComponent {
                entries: of.iter().map(|&component| DgnLsCompoundEntry { component, offset: 0.0 }).collect(),
            })
        };
        let mut entries = Vec::new();
        let mut put = |doc: &mut acadrust::CadDocument, h: acadrust::Handle, data: DgnLineStyleData| {
            entries.push((format!("{:X}", h.value()), h));
            let reactors = vec![container];
            doc.objects
                .insert(h, ObjectType::DgnLineStyle(DgnLineStyleObject { handle: h, owner: container, reactors, xdictionary_handle: None, data }));
        };
        let component = |c: DgnLsComponentData| DgnLineStyleData::Component {
            kind: kind(&c),
            description: "dash".into(),
            version: 1,
            component_uid: [7; 16],
            scale: 1.0,
            property_flags: 0,
            component: c,
            properties: vec![],
        };
        let h: Vec<acadrust::Handle> = (0..9).map(|_| doc.allocate_handle()).collect();
        let [def, root, stroke, point, back, orphan, orphan_cycle, internal, shared] = h[..] else { panic!("nine handles") };
        put(
            &mut doc,
            def,
            DgnLineStyleData::Definition {
                description: "{ dash }".into(),
                version: 1,
                style_number: 1,
                component_uid: [1; 16],
                is_continuous: false,
                unit_definition: 1.0,
                unit_scale: 1.0,
                units_type: 0,
                is_element: false,
                is_physical: false,
                is_scale_independent: false,
                is_snappable: false,
                root_component: root,
                properties: vec![],
            },
        );
        // Used: def → root → {stroke, point → shared, back → root (a cycle)}.
        put(&mut doc, root, component(compound(&[stroke, point, back])));
        put(&mut doc, stroke, component(DgnLsComponentData::Stroke(pattern())));
        put(&mut doc, point, component(DgnLsComponentData::Point(DgnLsPointComponent { stroke_component: shared, symbols: vec![] })));
        put(&mut doc, back, component(compound(&[root])));
        put(&mut doc, shared, component(DgnLsComponentData::Stroke(pattern())));
        // Orphans: an internal component, and two compounds that refer to each other and to a
        // used component (which stays).
        let pattern_internal =
            DgnLsInternalComponent { pattern: pattern(), internal_version: 0, hardware_style: 0, is_hardware_style: false, line_code: 0 };
        put(&mut doc, internal, component(DgnLsComponentData::Internal(pattern_internal)));
        put(&mut doc, orphan, component(compound(&[orphan_cycle, shared])));
        put(&mut doc, orphan_cycle, component(compound(&[orphan])));
        let mut dict = Dictionary::new();
        dict.handle = container;
        dict.owner = nod;
        dict.entries = entries;
        doc.objects.insert(container, ObjectType::Dictionary(dict));
        let dwg = acadrust::DwgWriter::write_to_vec(&doc).unwrap();

        let (dxf, notes) = dwg_to_dxf_noted(&dwg, Limits::DEFAULT).unwrap();
        let dxf = String::from_utf8(dxf).unwrap();
        let handles = dxf_handles(&dxf);
        for kept in [def, root, stroke, point, back, shared] {
            assert!(handles.contains(&kept.value()), "{:X} was left out", kept.value());
        }
        for gone in [internal, orphan, orphan_cycle] {
            assert!(!handles.contains(&gone.value()), "{:X} was kept or is still referred to", gone.value());
        }
        let lines: Vec<&str> = dxf.lines().collect();
        let ls: Vec<&str> = lines.chunks(2).filter_map(|p| p.get(1).filter(|_| p[0].trim() == "0" && p[1].starts_with("LS")).copied()).collect();
        assert_eq!(ls.iter().filter(|t| t.ends_with("COMPONENT")).count(), 5, "{ls:?}");
        assert!(ls.contains(&"LSDEFINITION") && !ls.contains(&"LSINTERNALCOMPONENT"), "{ls:?}");
        assert_eq!(notes, ["DWG: 3 DGN line style components that no line style definition uses were left out"]);
    }
}
