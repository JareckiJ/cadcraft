//! A `--cmd` can act on a handle an earlier `--cmd` in the same run returned (#1663).
//!
//! `mview` answers `{"viewports":["100"]}`, but a following `viewport.set` could not use that
//! handle: it had to be guessed, selected by point, or activated with `mspace`. A `--cmd` parameter
//! value of exactly `{last}` is now filled in with the last handle an earlier `--cmd` returned.

use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cadcraft-cli"))
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("run cadcraft-cli")
}

/// The issue's sequence: `viewport.set` straight after `mview`, addressing the new viewport by the
/// handle the runner remembers.
#[test]
fn viewport_set_after_mview_uses_the_returned_handle() {
    let out = run(&[
        "run",
        "--metric",
        "--cmd",
        r#"layout.set {"name":"Layout1"}"#,
        "--cmd",
        r#"mview {"p1":[20,20],"p2":[200,150]}"#,
        "--cmd",
        r#"viewport.set {"handle":"{last}","scale":1}"#,
    ]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "exit {:?}\nstdout: {stdout}\nstderr: {stderr}", out.status);
    let mview = stdout.lines().find(|l| l.contains("\"viewports\"")).unwrap_or_else(|| panic!("no mview result in {stdout:?}"));
    let created: serde_json::Value = serde_json::from_str(mview).expect("mview result is JSON");
    let handle = created["viewports"].as_array().and_then(|a| a.first()).and_then(|h| h.as_str()).expect("a viewport handle");
    assert!(handle.eq_ignore_ascii_case("100"), "{mview}");
    let set = stdout.lines().find(|l| l.contains("\"viewHeight\"")).unwrap_or_else(|| panic!("no viewport.set result in {stdout:?}"));
    let vp: serde_json::Value = serde_json::from_str(set).expect("viewport.set result is JSON");
    assert_eq!(vp[0]["handle"].as_str(), Some(handle), "viewport.set changed the viewport `{handle}`: {set}");
    assert_eq!(vp[0]["scale"].as_f64(), Some(1.0), "{set}");
}

/// `{last}` with no handle to fill in is a clear error, not the token reaching the command.
#[test]
fn last_without_an_earlier_handle_is_an_error() {
    let out = run(&["run", "--metric", "--cmd", r#"viewport.set {"handle":"{last}","scale":1}"#]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "stdout: {:?}\nstderr: {stderr}", String::from_utf8_lossy(&out.stdout));
    assert!(stderr.contains("{last}"), "the error names the placeholder: {stderr}");
    assert!(!stderr.contains("panicked"), "{stderr}");
}

/// Only the exact token is a placeholder: ordinary JSON strings are left alone.
#[test]
fn ordinary_values_are_not_placeholders() {
    let dir = std::env::temp_dir().join(format!("cc-cli-last-placeholder-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let out_dxf = dir.join("text.dxf");
    let out = run(&[
        "run",
        "--metric",
        "--cmd",
        r#"layout.set {"name":"Layout1"}"#,
        "--cmd",
        r#"mview {"p1":[20,20],"p2":[200,150]}"#,
        "--cmd",
        r#"text {"at":[0,0],"height":2,"text":"last and {last}"}"#,
        "--save",
        out_dxf.to_str().expect("temp path"),
    ]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "exit {:?}\nstderr: {stderr}", out.status);
    let saved = std::fs::read_to_string(&out_dxf).expect("saved DXF");
    assert!(saved.contains("last and {last}"), "the text is stored literally");
    let _ = std::fs::remove_dir_all(&dir);
}
