//! One failing line costs that line, not the drawing (#1522): the rest of the script still runs,
//! the drawing is still written, and the exit status still reports the failure.

use std::process::Command;

#[test]
fn a_bad_line_is_reported_and_the_rest_still_runs() {
    let dir = std::env::temp_dir().join(format!("cc-cli-scriptfail-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("bad.scr");
    std::fs::write(&script, "CIRCLE 5,5 2\nNOT_A_COMMAND\nLINE 0,0 10,0\n").unwrap();
    let drawing = dir.join("out.dxf");
    let out = Command::new(env!("CARGO_BIN_EXE_cadcraft-cli"))
        .args(["run", "--metric", "--script-file", script.to_str().unwrap(), "--save", drawing.to_str().unwrap()])
        .output()
        .expect("run cadcraft-cli");
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "the failure is still reported: {err}");
    assert!(err.contains("script line 2: unknown command `NOT_A_COMMAND`"), "{err}");
    let saved = std::fs::read_to_string(&drawing).expect("the drawing is written even though a line failed");
    assert!(saved.contains("CIRCLE") && saved.contains("LINE"), "both good commands ran: {}", &saved[..saved.len().min(200)]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The entity types in a saved DXF: the values after group code 0 in the ENTITIES section.
fn entities(dxf: &str) -> Vec<String> {
    let lines: Vec<&str> = dxf.lines().map(str::trim).collect();
    let mut out = Vec::new();
    let mut inside = false;
    for pair in lines.chunks(2) {
        let [code, value] = pair else { break };
        if *code != "0" && !(*code == "2" && *value == "ENTITIES") {
            continue;
        }
        match *value {
            "ENTITIES" => inside = true,
            "ENDSEC" if inside => break,
            "SECTION" => {}
            v if inside => out.push(v.to_string()),
            _ => {}
        }
    }
    out
}

fn run_script(name: &str, text: &str) -> (Option<i32>, String, Vec<String>) {
    let dir = std::env::temp_dir().join(format!("cc-cli-scriptfail-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("s.scr");
    std::fs::write(&script, text).unwrap();
    let drawing = dir.join("out.dxf");
    let out = Command::new(env!("CARGO_BIN_EXE_cadcraft-cli"))
        .args(["run", "--metric", "--script-file", script.to_str().unwrap(), "--save", drawing.to_str().unwrap()])
        .output()
        .expect("run cadcraft-cli");
    let saved = std::fs::read_to_string(&drawing).expect("the drawing is written");
    let _ = std::fs::remove_dir_all(&dir);
    (out.status.code(), String::from_utf8_lossy(&out.stderr).into_owned(), entities(&saved))
}

#[test]
fn bad_input_inside_a_command_fails_that_line_only() {
    // `abc` is no radius: line 1 fails and CIRCLE ends, so the LINE on line 2 is drawn as a line.
    let (code, err, ents) = run_script("badinput", "CIRCLE 5,5 abc\nLINE 0,0 10,0\n\n");
    assert_eq!(code, Some(1), "{err}");
    assert!(err.contains("script line 1:"), "{err}");
    assert_eq!(ents, vec!["LINE"], "{err}");
}

#[test]
fn an_unfinished_last_command_is_reported() {
    let (code, err, ents) = run_script("unfinished", "LINE 0,0 10,0\n\nCIRCLE 5,5\n");
    assert_eq!(code, Some(1), "{err}");
    assert!(err.contains("script line 3:") && err.contains("CIRCLE"), "{err}");
    assert_eq!(ents, vec!["LINE"], "everything else is written: {err}");
}

#[test]
fn a_fully_good_script_still_exits_zero() {
    let out =
        Command::new(env!("CARGO_BIN_EXE_cadcraft-cli")).args(["run", "--metric", "--script", "CIRCLE 5,5 2\\n"]).output().expect("run cadcraft-cli");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}
