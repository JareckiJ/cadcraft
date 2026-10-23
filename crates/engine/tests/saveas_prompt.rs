//! SAVEAS at the command line (gaps FF3): typed, in a script or with FILEDIA 0 it asks for the
//! file format and version, then the file name, as AutoCAD does without its Save dialog. Enter
//! takes each default, Esc saves nothing, an existing file is replaced only after a Yes.

use std::sync::Mutex;

use cadcraft_doc::Drawing;
use cadcraft_engine::cmd::file::{FileFormat, IoHooks, SaveAs, Version, set_io, set_untitled_save_default, set_write_as};
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{Input, Session};
use serde_json::json;

/// The untitled-drawing default is process-wide: the tests here run one at a time.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// The writer with options: the bytes name the version and encoding it got.
fn with_options(_: &Drawing, _: &str, o: &SaveAs) -> Result<Vec<u8>, String> {
    Ok(format!("{}|{}", o.version.map(|v| v.year()).unwrap_or("-"), o.binary).into_bytes())
}

fn install() {
    set_io(IoHooks { read: |_, _| Ok(Drawing::new_metric()), write: |_, _| Ok(b"plain".to_vec()), plot: None });
    set_write_as(with_options);
}

fn dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("cadcraft-saveas-prompt-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn read(p: &std::path::Path) -> String {
    String::from_utf8(std::fs::read(p).unwrap()).unwrap()
}

fn prompt(s: &Session) -> String {
    s.prompt_text()
}

#[test]
fn untitled_drawing_saves_as_chosen_dxf_version_and_encoding() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    install();
    let d = dir("dxf");
    let mut s = Session::new();
    s.cmdline("circle 0,0 5").unwrap();
    s.cmdline("saveas").unwrap();
    assert!(s.log.iter().any(|l| l == "Current file format: DXF 2018"), "{:?}", s.log);
    assert_eq!(prompt(&s), "SAVEAS Enter file format or [2000/2004/2007/2010/2013/2018/DXF/Template] <DXF>:");
    s.cmdline("").unwrap();
    assert_eq!(prompt(&s), "SAVEAS Enter DXF version or [2000/2004/2007/2010/2013/2018] <2018>:");
    s.cmdline("2010").unwrap();
    assert_eq!(prompt(&s), "SAVEAS Enter DXF encoding or [Ascii/Binary] <Ascii>:");
    s.cmdline("b").unwrap();
    assert_eq!(prompt(&s), "SAVEAS Save drawing as <Drawing1.dxf>:");
    // No extension: the format's is added.
    let target = d.join("plan");
    s.cmdline(&target.to_string_lossy()).unwrap();
    assert!(s.running.is_none());
    let saved = d.join("plan.dxf");
    assert_eq!(read(&saved), "2010|true");
    let st = s.state().unwrap();
    assert!(!st.is_dirty() && st.binary_dxf);
    assert_eq!(st.path.as_deref(), Some(saved.to_string_lossy().as_ref()));
    assert!(s.log.iter().any(|l| l.starts_with("Saved ") && l.ends_with("(binary DXF 2010).")), "{:?}", s.log);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_year_saves_dwg_and_an_existing_file_is_replaced_only_after_yes() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    install();
    let d = dir("dwg");
    let mut s = Session::new();
    s.cmdline("saveas").unwrap();
    s.cmdline("2013").unwrap();
    assert_eq!(prompt(&s), "SAVEAS Save drawing as <Drawing1.dwg>:");
    // Another drawing extension is replaced by the chosen format's.
    s.cmdline(&d.join("site.dxf").to_string_lossy()).unwrap();
    let site = d.join("site.dwg");
    assert_eq!(read(&site), "2013|false");
    assert!(!d.join("site.dxf").exists());

    // The drawing now has a DWG 2013 file: that is what SAVEAS offers, and the file exists.
    s.cmdline("line 0,0 5,5 ").unwrap();
    s.cmdline("saveas").unwrap();
    assert!(s.log.iter().any(|l| l == "Current file format: DWG 2013"), "{:?}", s.log);
    assert!(prompt(&s).ends_with("<2013>:"), "{}", prompt(&s));
    s.cmdline("").unwrap();
    assert_eq!(prompt(&s), format!("SAVEAS Save drawing as <{}>:", site.to_string_lossy()));
    s.cmdline("").unwrap();
    assert_eq!(prompt(&s), "SAVEAS A file with this name already exists. Replace it? [Yes/No] <No>:");
    // No (Enter): asked for another name; nothing written.
    s.cmdline("").unwrap();
    assert!(prompt(&s).starts_with("SAVEAS Save drawing as"), "{}", prompt(&s));
    assert!(s.state().unwrap().is_dirty());
    s.cmdline("").unwrap();
    s.cmdline("y").unwrap();
    assert!(s.running.is_none());
    assert!(!s.state().unwrap().is_dirty(), "Yes replaced the file");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn esc_at_any_prompt_saves_nothing() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    install();
    let d = dir("esc");
    let target = d.join("never.dxf");
    // Esc after 0, 1, 2 and 3 answers (format, version, encoding, then at the file name).
    for answers in 0..4 {
        let mut s = Session::new();
        s.cmdline("circle 0,0 1").unwrap();
        s.cmdline("saveas").unwrap();
        for _ in 0..answers {
            s.cmdline("").unwrap();
        }
        assert!(s.running.is_some(), "still asking after {answers} answers");
        s.input(Input::Cancel).unwrap();
        assert!(s.running.is_none());
        assert!(s.log.iter().any(|l| l == "*Cancel*"));
        let st = s.state().unwrap();
        assert!(st.is_dirty() && st.path.is_none(), "after {answers} answers");
    }
    // Esc at "Replace it?".
    std::fs::write(&target, b"keep").unwrap();
    let mut s = Session::new();
    s.cmdline("saveas").unwrap();
    s.cmdline("dxf").unwrap();
    s.cmdline("").unwrap();
    s.cmdline("").unwrap();
    s.cmdline(&target.to_string_lossy()).unwrap();
    assert!(prompt(&s).contains("Replace it?"));
    s.input(Input::Cancel).unwrap();
    assert!(s.running.is_none());
    assert_eq!(read(&target), "keep");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn unknown_and_older_formats_ask_again() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    install();
    let mut s = Session::new();
    s.cmdline("saveas").unwrap();
    for typed in ["R14", "foo", "12"] {
        s.cmdline(typed).unwrap();
        assert!(prompt(&s).contains("Enter file format"), "{typed}: {}", prompt(&s));
    }
    assert!(s.log.iter().any(|l| l.contains("R14 files are not available yet")), "{:?}", s.log);
    assert!(s.log.iter().any(|l| l.contains("Unknown file format \"foo\"")), "{:?}", s.log);
    // A click, or a very long answer, is no answer either.
    s.input(Input::Point(Vec2::new(1.0, 2.0))).unwrap();
    s.cmdline(&"x".repeat(100_000)).unwrap();
    assert!(prompt(&s).contains("Enter file format"));
    // R2004 and AC1018 name DWG 2004.
    s.cmdline("AC1018").unwrap();
    assert_eq!(prompt(&s), "SAVEAS Save drawing as <Drawing1.dwg>:");
    s.cmdline(&"y".repeat(5000)).unwrap();
    assert!(s.log.iter().any(|l| l.contains("too long")));
    assert!(prompt(&s).starts_with("SAVEAS Save drawing as"));
    s.input(Input::Cancel).unwrap();

    // DXF: an unknown version and an unknown encoding ask again.
    s.cmdline("saveas").unwrap();
    s.cmdline("dxf").unwrap();
    s.cmdline("R12").unwrap();
    assert!(prompt(&s).contains("Enter DXF version"));
    assert!(s.log.iter().any(|l| l.contains("R12 DXF is not available yet")), "{:?}", s.log);
    s.cmdline("r2007").unwrap();
    s.cmdline("utf16").unwrap();
    assert!(prompt(&s).contains("Enter DXF encoding"));
    s.input(Input::Cancel).unwrap();
}

#[test]
fn untitled_default_follows_the_host_and_scripts_answer_on_one_line() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    install();
    let d = dir("script");
    set_untitled_save_default(FileFormat::Dwg, Version::R2007);
    let mut s = Session::new();
    s.cmdline("saveas").unwrap();
    assert!(s.log.iter().any(|l| l == "Current file format: DWG 2007"), "{:?}", s.log);
    assert!(prompt(&s).ends_with("<2007>:"), "{}", prompt(&s));
    s.input(Input::Cancel).unwrap();
    set_untitled_save_default(FileFormat::Dxf, Version::R2018);

    // A script: the keywords split at spaces, the file name takes the rest of the line.
    let target = d.join("with space.dxf");
    s.script(&format!("saveas dxf 2004 ascii {}\n", target.to_string_lossy())).unwrap();
    assert!(s.running.is_none(), "{}", prompt(&s));
    assert_eq!(read(&target), "2004|false");
    // Template: a .dwt at the drawing's version (2004 now).
    let t = d.join("base");
    s.script(&format!("saveas t {}\n", t.to_string_lossy())).unwrap();
    assert_eq!(read(&d.join("base.dwt")), "2004|false");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn json_saveas_never_prompts_and_filedia_is_a_system_variable() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    install();
    let mut s = Session::new();
    assert!(s.execute("saveas", &json!({})).is_err(), "`path` is required");
    assert!(s.running.is_none());
    assert_eq!(s.execute("getvar", &json!({ "name": "FILEDIA" })).unwrap()["value"], json!(1));
    s.execute("setvar", &json!({ "name": "FILEDIA", "value": 0 })).unwrap();
    assert_eq!(s.execute("getvar", &json!({ "name": "FILEDIA" })).unwrap()["value"], json!(0));
    assert!(s.execute("setvar", &json!({ "name": "FILEDIA", "value": 2 })).is_err());
    s.cmdline("filedia 1").unwrap();
    assert!(cadcraft_engine::cmd::file::filedia());
}
