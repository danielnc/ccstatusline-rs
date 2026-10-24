//! Installing ccstatusline-rs into Claude Code's settings (upstream `installStatusLine` and `uninstallStatusLine`
//! in `utils/claude-settings.ts`).
//!
//! A deliberate difference from upstream: upstream installs its npm package (`npx -y ccstatusline@latest`, `bunx`, or
//! a global npm/bun install). This writes the command that runs this binary, and has no package-manager actions.

use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

/// `needsQuoting` on POSIX.
fn needs_quoting(path: &str) -> bool {
    path.chars().any(|c| c.is_whitespace() || "()[];&#|'\"\\$`".contains(c))
}

/// `quotePathIfNeeded` on POSIX.
pub fn quote_path_if_needed(path: &str) -> String {
    if !needs_quoting(path) {
        return path.to_string();
    }
    format!("'{}'", path.replace('\'', "'\\''"))
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(name)).find(|p| p.is_file())
}

/// The command that runs this binary: its name when PATH finds this same file under it, else its path.
pub fn base_command() -> String {
    let exe = std::env::current_exe().ok().and_then(|p| std::fs::canonicalize(p).ok());
    let on_path = find_on_path("ccstatusline-rs").and_then(|p| std::fs::canonicalize(p).ok());
    match (exe, on_path) {
        (Some(e), Some(p)) if e == p => "ccstatusline-rs".into(),
        (Some(e), _) => quote_path_if_needed(&e.to_string_lossy()),
        (None, _) => "ccstatusline-rs".into(),
    }
}

/// `buildStatusLineCommand`: the base command, with `--config <path>` for a custom config.
pub fn status_line_command(base: &str, custom_config: Option<&Path>) -> String {
    match custom_config {
        Some(p) => format!("{base} --config {}", quote_path_if_needed(&p.to_string_lossy())),
        None => base.to_string(),
    }
}

/// Whether a status line command runs ccstatusline-rs: the binary's name or a path to it, alone or with `--config`.
pub fn is_rust_command(command: &str) -> bool {
    let first = command.split(" --config ").next().unwrap_or("");
    let unquoted = first.trim_matches('\'');
    unquoted == "ccstatusline-rs" || unquoted.ends_with("/ccstatusline-rs")
}

fn read_settings(path: &Path) -> Result<Value, String> {
    if !path.exists() {
        return Ok(Value::Object(Map::new()));
    }
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

/// Copies the settings file next to itself with `suffix`, keeping its permissions (`backupClaudeSettings`).
fn backup(path: &Path, suffix: &str) {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let Ok(content) = std::fs::read(path) else { return };
    let mode = std::fs::metadata(path).map(|m| m.permissions().mode() & 0o777).unwrap_or(0o644);
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    let target = path.with_file_name(name);
    let _ = std::fs::remove_file(&target);
    if let Ok(mut f) = std::fs::OpenOptions::new().write(true).create_new(true).mode(mode).open(&target) {
        use std::io::Write;
        let _ = f.write_all(&content);
    }
}

/// `saveClaudeSettings`: back up to `.bak`, then write.
fn save_settings(path: &Path, v: &Value) -> Result<(), String> {
    backup(path, ".bak");
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, crate::jsjson::stringify_pretty(v)).map_err(|e| e.to_string())
}

/// `installStatusLine`: the status line set to `command` (keeping a refresh interval already there, or 10, when the
/// installed Claude Code supports one). An unreadable settings file is left alone and reported.
pub fn install(path: &Path, command: &str, supports_refresh_interval: bool) -> Result<(), String> {
    let mut settings = read_settings(path).map_err(|reason| {
        format!(
            "Could not read existing Claude settings at {} ({reason}). The file was left unchanged; fix or remove it, then install again.",
            path.display()
        )
    })?;
    backup(path, ".orig");
    let existing_refresh = settings.get("statusLine").and_then(|s| s.get("refreshInterval")).cloned();
    let mut status_line = Map::new();
    status_line.insert("type".into(), Value::String("command".into()));
    status_line.insert("command".into(), Value::String(command.into()));
    status_line.insert("padding".into(), Value::from(0));
    if supports_refresh_interval {
        status_line.insert("refreshInterval".into(), existing_refresh.unwrap_or_else(|| Value::from(10)));
    }
    if let Some(o) = settings.as_object_mut() {
        o.insert("statusLine".into(), Value::Object(status_line));
    }
    save_settings(path, &settings)
}

/// `uninstallStatusLine`'s Claude-settings half: the status line removed. False when the file cannot be read.
pub fn uninstall(path: &Path) -> bool {
    let Ok(mut settings) = read_settings(path) else { return false };
    if settings.get("statusLine").is_some() {
        if let Some(o) = settings.as_object_mut() {
            o.shift_remove("statusLine");
        }
        let _ = save_settings(path, &settings);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ccsl-install-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn commands() {
        assert_eq!(status_line_command("ccstatusline-rs", None), "ccstatusline-rs");
        assert_eq!(
            status_line_command("ccstatusline-rs", Some(Path::new("/a b/c.json"))),
            "ccstatusline-rs --config '/a b/c.json'"
        );
        assert_eq!(quote_path_if_needed("/it's"), "'/it'\\''s'");
        assert!(is_rust_command("ccstatusline-rs"));
        assert!(is_rust_command("/opt/bin/ccstatusline-rs --config /x.json"));
        assert!(is_rust_command("'/my tools/ccstatusline-rs'"));
        assert!(!is_rust_command("ccstatusline"));
        assert!(!is_rust_command("npx -y ccstatusline@latest"));
    }

    #[test]
    fn install_keeps_other_settings_and_backs_up() {
        let d = temp_dir("install");
        let path = d.join("settings.json");
        std::fs::write(
            &path,
            r#"{"theme":"dark","statusLine":{"type":"command","command":"old","refreshInterval":3}}"#,
        )
        .unwrap();
        install(&path, "ccstatusline-rs", true).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["theme"], "dark");
        assert_eq!(v["statusLine"]["command"], "ccstatusline-rs");
        assert_eq!(v["statusLine"]["padding"], 0);
        assert_eq!(v["statusLine"]["refreshInterval"], 3);
        assert!(d.join("settings.json.orig").exists() && d.join("settings.json.bak").exists());
        assert!(uninstall(&path));
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(v.get("statusLine").is_none());
        assert_eq!(v["theme"], "dark");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn install_refuses_an_unreadable_file() {
        let d = temp_dir("bad");
        let path = d.join("settings.json");
        std::fs::write(&path, "{ nope").unwrap();
        let err = install(&path, "ccstatusline-rs", false).unwrap_err();
        assert!(err.starts_with("Could not read existing Claude settings at "), "{err}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ nope");
        assert!(!uninstall(&path));
        let fresh = d.join("new.json");
        install(&fresh, "x", false).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&fresh).unwrap()).unwrap();
        assert!(v["statusLine"].get("refreshInterval").is_none());
        let _ = std::fs::remove_dir_all(&d);
    }
}
