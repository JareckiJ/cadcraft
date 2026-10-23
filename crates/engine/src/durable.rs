//! Durable file replacement for saving drawings (native builds only; the web build has no files).
//!
//! [`write_durable`] replaces the file atomically ([`crate::atomic_write::write_atomic`]: a fresh,
//! exclusively created temporary file, synced and renamed over the target) and then (on Unix)
//! flushes the folder, so a crash or power loss leaves either the old file or the new one, never a
//! torn mix. [`backup_previous`] keeps the
//! version being replaced as `<name>.bak`, but only when that version is itself a readable drawing:
//! a damaged file never replaces a good backup.

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

/// Write `bytes` to `path` so that `path` is always either the old or the new content, and the
/// rename itself survives a power loss.
pub fn write_durable(path: &Path, bytes: &[u8]) -> io::Result<()> {
    crate::atomic_write::write_atomic(path, bytes)?;
    sync_parent(path);
    Ok(())
}

/// Flush the folder entry of `path` (Unix); best effort, since some file systems refuse it.
fn sync_parent(path: &Path) {
    #[cfg(unix)]
    if let Some(dir) = path.parent().map(|d| if d.as_os_str().is_empty() { Path::new(".") } else { d })
        && let Ok(d) = File::open(dir)
    {
        let _ = d.sync_all();
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// The backup file of the drawing at `path`: the same name with the extension `bak`. None for a
/// path that is itself a `.bak`.
pub fn backup_path(path: &str) -> Option<PathBuf> {
    let p = Path::new(path);
    if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("bak")) {
        return None;
    }
    Some(p.with_extension("bak"))
}

/// Before `path` is overwritten, keep its current content as `<name>.bak`, provided `is_valid`
/// accepts that content. A missing, unreadable or invalid current file leaves an existing backup
/// untouched. Returns whether a backup was written.
pub fn backup_previous(path: &str, is_valid: impl FnOnce(&[u8]) -> bool) -> io::Result<bool> {
    let Some(bak) = backup_path(path) else { return Ok(false) };
    let old = match fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    if old.is_empty() || !is_valid(&old) {
        return Ok(false);
    }
    write_durable(&bak, &old)?;
    Ok(true)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cadcraft-durable-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn write_durable_replaces_and_leaves_no_staging_file() {
        let d = dir("durable");
        let p = d.join("a.dxf");
        write_durable(&p, b"one").unwrap();
        write_durable(&p, b"two").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"two");
        assert_eq!(fs::read_dir(&d).unwrap().count(), 1);
        // A failed write (the target is a folder, so the rename fails) leaves nothing behind.
        let blocked = d.join("blocked");
        fs::create_dir(&blocked).unwrap();
        assert!(write_durable(&blocked, b"new").is_err());
        assert_eq!(fs::read_dir(&d).unwrap().count(), 2);
        assert_eq!(fs::read(&p).unwrap(), b"two");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn backup_keeps_the_previous_version_only_while_it_is_valid() {
        let d = dir("bak");
        let p = d.join("a.dxf").to_string_lossy().to_string();
        let bak = d.join("a.bak");
        assert!(!backup_previous(&p, |_| true).unwrap(), "nothing to back up yet");
        fs::write(&p, b"good").unwrap();
        assert!(backup_previous(&p, |b| b == b"good").unwrap());
        assert_eq!(fs::read(&bak).unwrap(), b"good");
        // A damaged current file must not replace the good backup.
        fs::write(&p, b"torn").unwrap();
        assert!(!backup_previous(&p, |b| b == b"good").unwrap());
        fs::write(&p, b"").unwrap();
        assert!(!backup_previous(&p, |_| true).unwrap(), "an empty file is never a backup");
        assert_eq!(fs::read(&bak).unwrap(), b"good");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_bak_file_has_no_backup() {
        assert_eq!(backup_path("/x/a.dwg"), Some(PathBuf::from("/x/a.bak")));
        assert_eq!(backup_path("/x/a.BAK"), None);
    }
}
