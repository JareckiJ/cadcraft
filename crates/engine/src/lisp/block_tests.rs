//! `entmake` defines blocks (#1662): a BLOCK list begins a definition, the entities made after it
//! go into the block instead of the drawing, and an ENDBLK list adds the block and returns its name.

#![allow(clippy::unwrap_used)]

use super::tests::session;
use super::value::{Value, to_text};
use crate::Session;

fn eval(s: &mut Session, code: &str) -> Result<String, String> {
    match s.lisp_eval(code, false, true, Vec::<Value>::new()) {
        Ok(Some(Ok(v))) => Ok(to_text(&v, true)),
        Ok(Some(Err(e))) => Err(e),
        Ok(None) => Err("did not finish".into()),
        Err(e) => Err(e.to_string()),
    }
}

fn ok(s: &mut Session, code: &str) -> String {
    match eval(s, code) {
        Ok(v) => v,
        Err(e) => panic!("{code}: {e}"),
    }
}

fn model_len(s: &Session) -> usize {
    s.doc().unwrap().model.len()
}

const BLOCK_EB: &str = r#"(entmake '((0 . "BLOCK") (2 . "EB") (70 . 0) (10 5.0 5.0 0.0) (4 . "made by entmake")))"#;
const LINE: &str = r#"(entmake '((0 . "LINE") (8 . "0") (10 0.0 0.0 0.0) (11 10.0 0.0 0.0)))"#;
const CIRCLE: &str = r#"(entmake '((0 . "CIRCLE") (8 . "PARTS") (10 5.0 5.0 0.0) (40 . 2.5)))"#;
const ENDBLK: &str = r#"(entmake '((0 . "ENDBLK")))"#;

/// The issue's routine: BLOCK, a LINE, ENDBLK, then tblsearch.
#[test]
fn the_issues_routine_defines_the_block() {
    let mut s = session();
    let code = r#"(list (entmake (quote ((0 . "BLOCK") (2 . "EB") (70 . 0) (10 0.0 0.0 0.0)))) (entmake (quote ((0 . "LINE") (8 . "0") (10 0.0 0.0 0.0) (11 10.0 0.0 0.0)))) (entmake (quote ((0 . "ENDBLK")))) (cdr (assoc 2 (tblsearch "BLOCK" "EB"))))"#;
    let v = ok(&mut s, code);
    assert!(v.starts_with("(((0 . \"BLOCK\")"), "{v}");
    assert!(v.ends_with(" \"EB\" \"EB\")"), "ENDBLK returns the name and tblsearch finds it: {v}");
    assert_eq!(model_len(&s), 0, "the LINE went into the block, not model space");
    let d = s.doc().unwrap();
    assert_eq!(d.block("EB").unwrap().entities.len(), 1);
}

#[test]
fn the_entities_base_point_and_description_are_kept() {
    let mut s = session();
    assert!(ok(&mut s, BLOCK_EB).starts_with("((0 . \"BLOCK\")"));
    assert!(ok(&mut s, LINE).starts_with("((0 . \"LINE\")"), "an entity in a definition returns its list");
    ok(&mut s, CIRCLE);
    assert_eq!(model_len(&s), 0);
    assert!(s.doc().unwrap().block("EB").is_none(), "nothing is added before ENDBLK");
    assert_eq!(ok(&mut s, ENDBLK), "\"EB\"");
    let d = s.doc().unwrap();
    let b = d.block("EB").unwrap();
    assert_eq!((b.base.x, b.base.y, b.description.as_str()), (5.0, 5.0, "made by entmake"));
    let hs: Vec<_> = b.entities.iter().map(|e| e.handle).collect();
    assert_eq!(hs.len(), 2);
    assert!(hs[0].0 != 0 && hs[0] != hs[1], "each entity gets its own handle: {hs:?}");
    assert!(d.layers.iter().any(|l| l.name == "PARTS"), "a layer the entities name is created");
    // It inserts like any block, and entmake makes entities in the drawing again afterwards.
    s.execute("insert", &serde_json::json!({"name": "EB", "at": [100, 0]})).unwrap();
    ok(&mut s, LINE);
    assert_eq!(model_len(&s), 2);
}

#[test]
fn anonymous_blocks_get_a_name() {
    let mut s = session();
    ok(&mut s, r#"(entmake '((0 . "BLOCK") (2 . "*U") (70 . 1) (10 0.0 0.0 0.0)))"#);
    ok(&mut s, LINE);
    let name = ok(&mut s, ENDBLK);
    assert!(name.starts_with("\"*U") && name.len() > 4, "{name}");
    let d = s.doc().unwrap();
    let b = d.block(name.trim_matches('"')).unwrap();
    assert!(b.anonymous);
    assert_eq!(b.entities.len(), 1);
}

#[test]
fn a_block_of_the_same_name_is_redefined() {
    let mut s = session();
    for (n, code) in [(1, LINE), (2, CIRCLE)] {
        ok(&mut s, r#"(entmake '((0 . "BLOCK") (2 . "R") (70 . 0) (10 0.0 0.0 0.0)))"#);
        for _ in 0..n {
            ok(&mut s, code);
        }
        assert_eq!(ok(&mut s, ENDBLK), "\"R\"");
    }
    assert_eq!(s.doc().unwrap().block("R").unwrap().entities.len(), 2, "the second definition");
    // Names are case-insensitive: the existing spelling is kept.
    ok(&mut s, r#"(entmake '((0 . "BLOCK") (2 . "r") (70 . 0) (10 0.0 0.0 0.0)))"#);
    assert_eq!(ok(&mut s, ENDBLK), "\"R\"");
}

#[test]
fn what_cant_be_defined_is_refused() {
    let mut s = session();
    assert_eq!(ok(&mut s, ENDBLK), "nil", "ENDBLK without a BLOCK");
    for bad in [
        r#"(entmake '((0 . "BLOCK") (2 . "") (70 . 0) (10 0.0 0.0 0.0)))"#,
        r#"(entmake '((0 . "BLOCK") (2 . "A<B") (70 . 0) (10 0.0 0.0 0.0)))"#,
        r#"(entmake '((0 . "BLOCK") (2 . "*NOTANON") (70 . 0) (10 0.0 0.0 0.0)))"#,
        r#"(entmake '((0 . "BLOCK") (2 . "XR") (70 . 4) (10 0.0 0.0 0.0)))"#,
        r#"(entmake '((0 . "BLOCK") (70 . 0) (10 0.0 0.0 0.0)))"#,
    ] {
        assert_eq!(ok(&mut s, bad), "nil", "{bad}");
    }
    // A BLOCK while one is open is refused; the open one goes on.
    ok(&mut s, BLOCK_EB);
    assert_eq!(ok(&mut s, r#"(entmake '((0 . "BLOCK") (2 . "OTHER") (70 . 0) (10 0.0 0.0 0.0)))"#), "nil");
    ok(&mut s, LINE);
    assert_eq!(ok(&mut s, ENDBLK), "\"EB\"");
    assert!(s.doc().unwrap().block("OTHER").is_none());
    // A block that inserts itself isn't added.
    ok(&mut s, r#"(entmake '((0 . "BLOCK") (2 . "SELF") (70 . 0) (10 0.0 0.0 0.0)))"#);
    ok(&mut s, r#"(entmake '((0 . "INSERT") (2 . "SELF") (10 0.0 0.0 0.0)))"#);
    assert_eq!(ok(&mut s, ENDBLK), "nil");
    assert!(s.doc().unwrap().block("SELF").is_none());
    assert_eq!(model_len(&s), 0);
}

#[test]
fn a_saved_drawing_keeps_the_block() {
    let mut s = session();
    for code in [BLOCK_EB, LINE, CIRCLE, ENDBLK] {
        ok(&mut s, code);
    }
    s.execute("insert", &serde_json::json!({"name": "EB", "at": [100, 0]})).unwrap();
    let read = cadcraft_io::read_dxf(cadcraft_io::write_dxf(s.doc().unwrap()).as_bytes()).unwrap();
    let b = read.block("EB").unwrap();
    assert_eq!((b.entities.len(), b.base.x, b.base.y), (2, 5.0, 5.0));
    assert_eq!(read.model.len(), 1, "the insert");
}
