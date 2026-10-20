use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io, set_recover};
use serde_json::json;

#[test]
fn recover_with_recovering_reader_and_auditctl() {
    set_io(IoHooks {
        read: |b, name| cadcraft_io::read(b, name).map_err(|e| e.to_string()),
        write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
        plot: None,
    });
    set_recover(|b, name| cadcraft_io::read_recovering(b, name).map_err(|e| e.to_string()));

    let dir = std::env::temp_dir().join(format!("cadcraft-rec-adt-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("damaged.dxf");
    // Truncated DXF without 0\nEOF or complete entities
    let truncated_dxf = b"0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0.0\n20\n0.0\n11\n5.0\n21\n5.0\n";
    std::fs::write(&path, truncated_dxf).unwrap();

    let mut s = Session::new();
    s.execute("setvar", &json!({"name": "AUDITCTL", "value": 1})).unwrap();

    let r = s.execute("recover", &json!({"path": path.to_string_lossy()})).unwrap();
    assert!(r["uid"].is_number());
    let st = s.state().unwrap();
    assert_eq!(st.doc.model.len(), 1, "The truncated line was recovered");

    let adt_path = dir.join("damaged.adt");
    assert!(adt_path.exists(), ".adt log generated on RECOVER when AUDITCTL is 1");
    let content = std::fs::read_to_string(&adt_path).unwrap();
    assert!(content.contains("CADCraft AUDIT Report"), "{content}");

    s.execute("setvar", &json!({"name": "AUDITCTL", "value": 0})).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}
