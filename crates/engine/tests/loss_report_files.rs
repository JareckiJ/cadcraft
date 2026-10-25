//! The files a lossy save keeps beside a drawing (FF1): the loss report is bound to the file it
//! describes, so a stale one can't stop the backup; the backup is created exclusively, never
//! through a planted symlink; and the report is no more readable than the drawing.
#![cfg(not(target_arch = "wasm32"))]

use cadcraft_engine::cmd::file::{Loss, back_up_source, keep_loss_report, loss_report_path};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// A fresh directory for one test.
fn dir(name: &str) -> std::io::Result<std::path::PathBuf> {
    let d = std::env::temp_dir().join(format!("cadcraft-loss-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d)?;
    Ok(d)
}

/// A report of `count` lost records.
fn loss(count: u64) -> Loss {
    Loss { count, summary: format!("{count} lost"), json: format!("{{\"lost\":{count},\"records\":[]}}") }
}

/// A report of the records `ids` (kind, type, handle) lost.
fn loss_of(ids: &[(&str, &str, &str)]) -> Loss {
    let records: Vec<String> = ids.iter().map(|(k, t, h)| format!("{{\"kind\":\"{k}\",\"type\":\"{t}\",\"handle\":\"{h}\"}}")).collect();
    Loss {
        count: ids.len() as u64,
        summary: format!("{} lost", ids.len()),
        json: format!("{{\"lost\":{},\"records\":[{}]}}", ids.len(), records.join(",")),
    }
}

/// The file's report lists other records than this save loses (as many): those are still in the
/// file, so it is backed up; a save losing only what the report lists isn't.
#[test]
fn a_report_listing_other_records_does_not_stop_the_backup() -> TestResult {
    let d = dir("identity")?;
    let path = d.join("i.dwg");
    let p = path.to_str().ok_or("path")?;
    std::fs::write(&path, b"keeps B, lacks A")?;
    keep_loss_report(p, Some(&loss_of(&[("object", "X_A", "40")])))?;
    assert_eq!(back_up_source(p, Some(p), Some(&loss_of(&[("object", "X_A", "40")])))?, None);
    let copy = back_up_source(p, Some(p), Some(&loss_of(&[("object", "X_B", "41")])))?.ok_or("no backup")?;
    assert_eq!(std::fs::read(&copy)?, b"keeps B, lacks A");
    std::fs::remove_dir_all(&d)?;
    Ok(())
}

/// A save with records beyond its report's detail cap can't be compared: it is always backed up.
#[test]
fn a_save_with_unlisted_records_is_backed_up() -> TestResult {
    let d = dir("unlisted")?;
    let path = d.join("u.dwg");
    let p = path.to_str().ok_or("path")?;
    std::fs::write(&path, b"drawing")?;
    keep_loss_report(p, Some(&loss(3)))?;
    let mut capped = loss(3);
    capped.json = "{\"lost\":3,\"unlisted\":3,\"records\":[]}".into();
    assert!(back_up_source(p, Some(p), Some(&capped))?.is_some());
    std::fs::remove_dir_all(&d)?;
    Ok(())
}

/// A report kept for one file doesn't stop the backup of a different file at the same path.
#[test]
fn a_report_of_another_file_does_not_stop_the_backup() -> TestResult {
    let d = dir("stale")?;
    let path = d.join("a.dwg");
    let p = path.to_str().ok_or("path")?;
    std::fs::write(&path, b"first contents")?;
    keep_loss_report(p, Some(&loss(5)))?;
    // The report describes this file: a save losing no more than it says needs no copy.
    assert_eq!(back_up_source(p, Some(p), Some(&loss(5)))?, None);
    // The file changes (another program saved it, richer): the report no longer describes it.
    std::fs::write(&path, b"second, richer contents")?;
    let copy = back_up_source(p, Some(p), Some(&loss(5)))?.ok_or("no backup")?;
    assert_eq!(std::fs::read(&copy)?, b"second, richer contents");
    std::fs::remove_dir_all(&d)?;
    Ok(())
}

/// A name taken by a symlink is skipped, and what it points to is left alone.
#[cfg(unix)]
#[test]
fn the_backup_never_goes_through_a_planted_symlink() -> TestResult {
    let d = dir("symlink")?;
    let path = d.join("b.dwg");
    let p = path.to_str().ok_or("path")?;
    std::fs::write(&path, b"drawing")?;
    let victim = d.join("victim");
    std::fs::write(&victim, b"precious")?;
    std::os::unix::fs::symlink(&victim, d.join("b.orig.dwg"))?;
    let copy = back_up_source(p, Some(p), Some(&loss(1)))?.ok_or("no backup")?;
    assert!(copy.ends_with("b.orig-2.dwg"), "{copy}");
    assert_eq!(std::fs::read(&victim)?, b"precious");
    assert_eq!(std::fs::read(&copy)?, b"drawing");
    std::fs::remove_dir_all(&d)?;
    Ok(())
}

/// The report (it holds the lost records' own data) gets the drawing's permissions, and so does
/// the backup.
#[cfg(unix)]
#[test]
fn the_report_and_the_backup_are_no_more_readable_than_the_drawing() -> TestResult {
    use std::os::unix::fs::PermissionsExt;
    let d = dir("perms")?;
    let path = d.join("c.dwg");
    let p = path.to_str().ok_or("path")?;
    std::fs::write(&path, b"private drawing")?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    let copy = back_up_source(p, Some(p), Some(&loss(2)))?.ok_or("no backup")?;
    keep_loss_report(p, Some(&loss(2)))?;
    for f in [loss_report_path(p), copy] {
        assert_eq!(std::fs::metadata(&f)?.permissions().mode() & 0o777, 0o600, "{f}");
    }
    std::fs::remove_dir_all(&d)?;
    Ok(())
}
