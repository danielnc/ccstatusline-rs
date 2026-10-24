//! Claude Code config locations (the reader half of upstream
//! `src/utils/claude-settings.ts`).

use serde_json::Value;
use std::path::{Path, PathBuf};

/// `getClaudeConfigDir()`: CLAUDE_CONFIG_DIR when usable, else ~/.claude.
pub fn config_dir() -> PathBuf {
    if let Ok(env) = std::env::var("CLAUDE_CONFIG_DIR") {
        if !env.is_empty() {
            let resolved = crate::settings::path_resolve(&env);
            match std::fs::metadata(&resolved) {
                Ok(md) if md.is_dir() => return resolved,
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound && std::fs::symlink_metadata(&resolved).is_err() => {
                    return resolved
                }
                Err(_) => {}
            }
        }
    }
    crate::settings::home_dir().join(".claude")
}

/// `getClaudeJsonPath()`.
pub fn claude_json_path() -> PathBuf {
    let dir = config_dir();
    if let Ok(env) = std::env::var("CLAUDE_CONFIG_DIR") {
        if !env.is_empty() && dir == crate::settings::path_resolve(&env) {
            return dir.join(".claude.json");
        }
    }
    dir.parent().map(|p| p.join(".claude.json")).unwrap_or_else(|| PathBuf::from("/.claude.json"))
}

pub fn settings_path() -> PathBuf {
    config_dir().join("settings.json")
}

/// `loadClaudeSettingsSync()`: Ok({}) when missing, Err on read/parse failure.
pub fn load_settings() -> Result<Value, ()> {
    let path = settings_path();
    if !path.exists() {
        return Ok(Value::Object(Default::default()));
    }
    let text = std::fs::read(&path).map_err(|_| ())?;
    serde_json::from_str(&String::from_utf8_lossy(&text)).map_err(|_| ())
}

/// `saveClaudeSettings()`. Errors are dropped: the hook sync ignores them as
/// upstream's `saveSettings` does.
pub fn save_settings(v: &Value) {
    let _ = save_settings_at(&settings_path(), v);
}

/// `saveClaudeSettings()`, for a caller that reports a failed write (the editor's refresh interval).
pub fn try_save_settings(v: &Value) -> std::io::Result<()> {
    save_settings_at(&settings_path(), v)
}

pub(crate) fn save_settings_at(path: &Path, v: &Value) -> std::io::Result<()> {
    // backup first, with the original's permissions
    if let Ok(content) = std::fs::read(path) {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mode = std::fs::metadata(path).map(|m| m.permissions().mode() & 0o777).unwrap_or(0o644);
        let bak = path.with_file_name("settings.json.bak");
        let _ = std::fs::remove_file(&bak);
        if let Ok(mut f) = std::fs::OpenOptions::new().write(true).create_new(true).mode(mode).open(&bak) {
            use std::io::Write;
            let _ = f.write_all(&content);
        }
    }
    // Upstream overwrites in place, which a render killed mid-write leaves
    // empty; a rename cannot. Where that `writeFile` fails (a file it may not
    // write), the rename must not succeed either.
    if path.exists() {
        std::fs::OpenOptions::new().write(true).open(path)?;
    }
    crate::settings::write_json_atomic(path, v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ccsl-claude-config-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn save_replaces_atomically_keeping_mode_and_backup() {
        let dir = scratch("atomic");
        let path = dir.join("settings.json");
        std::fs::write(&path, "{\"model\":\"opus\"}").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let ino = std::fs::metadata(&path).unwrap().ino();
        let v = serde_json::json!({"model": "opus", "hooks": {}});
        let _ = save_settings_at(&path, &v);
        let md = std::fs::metadata(&path).unwrap();
        assert_ne!(md.ino(), ino, "the file is replaced by a rename, not truncated in place");
        assert_eq!(md.permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), crate::jsjson::stringify_pretty(&v));
        assert_eq!(std::fs::read_to_string(dir.join("settings.json.bak")).unwrap(), "{\"model\":\"opus\"}");
        let names: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names.len(), 2, "no temp file left behind: {names:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_keeps_a_symlink_and_a_read_only_file() {
        let dir = scratch("link");
        std::fs::create_dir_all(dir.join("dotfiles")).unwrap();
        std::fs::write(dir.join("dotfiles/settings.json"), "{}").unwrap();
        let path = dir.join("settings.json");
        std::os::unix::fs::symlink("dotfiles/settings.json", &path).unwrap();
        let v = serde_json::json!({"a": 1});
        let _ = save_settings_at(&path, &v);
        assert!(std::fs::symlink_metadata(&path).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), crate::jsjson::stringify_pretty(&v));

        if unsafe { libc::geteuid() } != 0 {
            let ro = dir.join("dotfiles/settings.json");
            std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o444)).unwrap();
            let _ = save_settings_at(&ro, &serde_json::json!({"b": 2}));
            assert_eq!(std::fs::read_to_string(&ro).unwrap(), crate::jsjson::stringify_pretty(&v));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
