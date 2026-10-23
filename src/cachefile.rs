//! Shared cache-file primitives: atomic writes (temp file + rename) and the
//! O_EXCL lock files upstream uses for best-effort mutual exclusion.

use std::io::Write;
use std::path::Path;

/// Write `data` to `path` atomically: a unique temp file in the same
/// directory, then rename over the target.
pub fn atomic_write(path: &Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let nanos =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    let tmp = path.with_file_name(format!(".{name}.{}.{nanos}.tmp", std::process::id()));
    let res = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(data)?;
        drop(f);
        std::fs::rename(&tmp, path)
    })();
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    res
}

/// Run `f` holding `<path>.lock` (O_CREAT|O_EXCL). A lock older than
/// `stale_ms` is stolen. When the lock is genuinely contended, `f` is skipped
/// and false is returned; any other lock failure runs `f` unlocked.
pub fn with_lock(path: &Path, stale_ms: f64, f: impl FnOnce()) -> bool {
    let lock =
        path.with_file_name(format!("{}.lock", path.file_name().map(|s| s.to_string_lossy()).unwrap_or_default()));
    let try_create = || std::fs::OpenOptions::new().write(true).create_new(true).open(&lock).map(|_| ());
    let mut have = false;
    let mut contended = false;
    match try_create() {
        Ok(()) => have = true,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            contended = true;
            let age = std::fs::metadata(&lock)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| std::time::SystemTime::now().duration_since(t).ok())
                .map(|d| d.as_millis() as f64);
            if age.is_some_and(|a| a > stale_ms) && std::fs::remove_file(&lock).is_ok() && try_create().is_ok() {
                have = true;
                contended = false;
            }
        }
        Err(_) => {}
    }
    let ran = have || !contended;
    if ran {
        f();
    }
    if have {
        let _ = std::fs::remove_file(&lock);
    }
    ran
}

/// Try to take `<path>.lock` without running anything; returns a guard that
/// removes it on drop. Used by background refreshers (one fetcher at a time).
pub struct LockGuard(std::path::PathBuf);

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

pub fn try_lock(lock: &Path, stale_ms: f64) -> Option<LockGuard> {
    if let Some(dir) = lock.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let create = || std::fs::OpenOptions::new().write(true).create_new(true).open(lock).map(|_| ());
    match create() {
        Ok(()) => Some(LockGuard(lock.to_path_buf())),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            let age = std::fs::metadata(lock)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| std::time::SystemTime::now().duration_since(t).ok())
                .map(|d| d.as_millis() as f64)?;
            if age > stale_ms && std::fs::remove_file(lock).is_ok() && create().is_ok() {
                Some(LockGuard(lock.to_path_buf()))
            } else {
                None
            }
        }
        Err(_) => None,
    }
}
