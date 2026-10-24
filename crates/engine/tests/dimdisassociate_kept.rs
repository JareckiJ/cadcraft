//! DIMDISASSOCIATE on dimensions whose association CADCraft keeps without modelling it (FF1): the
//! kept DIMASSOC goes, so the saved file's dimension is no longer associative; UNDO brings it back.

#[path = "../../io/tests/support/dimassoc.rs"]
mod fixture;

use cadcraft_doc::Handle;
use cadcraft_engine::Session;
use serde_json::json;

/// DIMDISASSOCIATE drops only the chosen dimension's kept DIMASSOC; UNDO brings it back.
#[test]
fn disassociating_drops_the_kept_association() -> Result<(), Box<dyn std::error::Error>> {
    let mut s = Session::empty();
    s.open_drawing(cadcraft_io::read_dxf(fixture::source().as_bytes())?, "kept", None);
    let kept = |s: &Session, h: u64| s.doc().map(|d| d.foreign.has_dimassoc(Handle(h)));
    assert!(kept(&s, 0x51)? && kept(&s, 0x31)?);
    let r = s.execute("dimdisassociate", &json!({ "handles": ["51"] }))?;
    assert_eq!(r["disassociated"], 1);
    assert!(!kept(&s, 0x51)? && kept(&s, 0x31)?, "only the chosen dimension's");
    let text = cadcraft_io::write_dxf(s.doc()?);
    let written = cadcraft_dxf::parse(text.as_bytes())?.iter().filter(|t| t.code == 0 && t.str() == "DIMASSOC").count();
    assert_eq!(written, 1);
    s.execute("undo", &json!({}))?;
    assert!(kept(&s, 0x51)?);
    Ok(())
}
