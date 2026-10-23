//! Claude Code config locations (the reader half of upstream
//! `src/utils/claude-settings.ts`).

use serde_json::Value;
use std::path::PathBuf;

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

pub fn save_settings(v: &Value) {
    let path = settings_path();
    // backup first, with the original's permissions
    if let Ok(content) = std::fs::read(&path) {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mode = std::fs::metadata(&path).map(|m| m.permissions().mode() & 0o777).unwrap_or(0o644);
        let bak = path.with_file_name("settings.json.bak");
        let _ = std::fs::remove_file(&bak);
        if let Ok(mut f) = std::fs::OpenOptions::new().write(true).create_new(true).mode(mode).open(&bak) {
            use std::io::Write;
            let _ = f.write_all(&content);
        }
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&path, crate::jsjson::stringify_pretty(v));
}
