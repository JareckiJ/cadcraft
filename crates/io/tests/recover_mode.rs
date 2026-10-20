use cadcraft_io::{read, read_recovering};

#[test]
fn normal_read_fails_on_truncated_dxf_but_recover_succeeds() {
    // Cut off mid-pair (group code 11 with no value line)
    let truncated_dxf = b"0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n1.0\n20\n2.0\n11\n";
    // Normal read fails with EOF error
    assert!(read(truncated_dxf, "test.dxf").is_err());

    // Recover mode succeeds and recovers the line entity
    let d = read_recovering(truncated_dxf, "test.dxf").expect("recover should salvage truncated DXF");
    let ents: Vec<_> = d.model.iter().map(|e| (e.handle, format!("{:?}", e.kind))).collect();
    assert_eq!(ents.len(), 1, "Entities: {:?}", ents);
    assert!(d.read_notes.iter().any(|n| n.contains("RECOVER")));
}

#[test]
fn recover_mode_skips_corrupt_lines() {
    let corrupt_dxf = b"0\nSECTION\n2\nENTITIES\nGARBAGE LINE HERE\n0\nLINE\n8\n0\n10\n0.0\n20\n0.0\n11\n5.0\n21\n5.0\n0\nENDSEC\n0\nEOF\n";
    // Normal read fails with BadCode
    assert!(read(corrupt_dxf, "test.dxf").is_err());

    // Recover mode skips the garbage line and recovers the entity
    let d = read_recovering(corrupt_dxf, "test.dxf").expect("recover should skip corrupt line");
    assert_eq!(d.model.len(), 1);
}
