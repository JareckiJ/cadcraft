//! A drawing opens at its stored model view instead of fitting geometry extents (#1602).

use cadcraft_engine::Session;
use cadcraft_geom::Vec2;

const REPRO: &str = "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1027\n9\n$VIEWCTR\n10\n10\n20\n10\n30\n0\n9\n$VIEWSIZE\n40\n5\n0\nENDSEC\n0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nVPORT\n0\nVPORT\n2\n*ACTIVE\n70\n0\n10\n0\n20\n0\n11\n1\n21\n1\n12\n50\n22\n50\n40\n20\n41\n1.5\n0\nENDTAB\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0\n20\n0\n30\n0\n11\n100\n21\n60\n31\n0\n0\nLINE\n8\n0\n10\n5000000\n20\n5000000\n30\n0\n11\n5000100\n21\n5000060\n31\n0\n0\nENDSEC\n0\nEOF\n";

#[test]
fn active_vport_wins_over_header_and_far_away_geometry() {
    let drawing = cadcraft_io::read_dxf(REPRO.as_bytes()).expect("synthetic DXF");
    assert!(drawing.model_view.is_some());

    let mut session = Session::empty();
    session.open_drawing(drawing, "saved-view-repro.dxf", None);
    let view = session.state().unwrap().view();
    assert_eq!(view.center, Vec2::new(50.0, 50.0));
    assert_eq!(view.height, 20.0);
}

#[test]
fn header_view_is_the_fallback_when_there_is_no_active_vport() {
    let dxf = REPRO.replace("0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nVPORT\n0\nVPORT\n2\n*ACTIVE\n70\n0\n10\n0\n20\n0\n11\n1\n21\n1\n12\n50\n22\n50\n40\n20\n41\n1.5\n0\nENDTAB\n0\nENDSEC\n", "");
    let drawing = cadcraft_io::read_dxf(dxf.as_bytes()).expect("synthetic DXF");
    assert!(drawing.model_view.is_none());

    let mut session = Session::empty();
    session.open_drawing(drawing, "header-view-repro.dxf", None);
    let view = session.state().unwrap().view();
    assert_eq!(view.center, Vec2::new(10.0, 10.0));
    assert_eq!(view.height, 5.0);
}
