//! The parts of upstream `src/utils/claude-settings.ts` the editor reads: whether ccstatusline is installed
//! in Claude Code's settings, the configured command, and how it was installed.

use super::components::main_menu::InstallMethod;
use serde_json::Value;

const AUTO_NPX: &str = "npx -y ccstatusline@latest";
const AUTO_BUNX: &str = "bunx -y ccstatusline@latest";
const GLOBAL: &str = "ccstatusline";

fn local_dev_re() -> regex::Regex {
    // /(?:^|[\s"'\\/])ccstatusline\.ts(?=$|[\s"'])/ without the lookahead: checked by hand below.
    regex::Regex::new(r#"(?:^|[\s"'\\/])ccstatusline\.ts"#).unwrap()
}

fn is_local_development_command(command: &str) -> bool {
    local_dev_re().find_iter(command).any(|m| {
        let rest = &command[m.end()..];
        rest.is_empty()
            || rest.starts_with(|c: char| crate::jsnum::js_trim(&c.to_string()).is_empty() || c == '"' || c == '\'')
    })
}

fn matches_base(command: &str, base: &str) -> bool {
    command == base || command.starts_with(&format!("{base} --config "))
}

/// `isKnownCommand`.
pub fn is_known_command(command: &str) -> bool {
    [AUTO_NPX, AUTO_BUNX, GLOBAL].iter().any(|p| matches_base(command, p)) || is_local_development_command(command)
}

fn load() -> Result<Value, ()> {
    crate::claude_config::load_settings()
}

/// `isInstalled()`.
pub fn is_installed() -> bool {
    let Ok(settings) = load() else { return false };
    let sl = settings.get("statusLine");
    let command = sl.and_then(|s| s.get("command")).and_then(Value::as_str).unwrap_or("");
    let padding = sl.and_then(|s| s.get("padding"));
    is_known_command(command) && (padding.is_none() || padding.and_then(Value::as_f64) == Some(0.0))
}

/// `getExistingStatusLine()`: `statusLine.command ?? null`, as long as it is a string.
pub fn existing_status_line() -> Option<String> {
    let settings = load().ok()?;
    settings.get("statusLine")?.get("command")?.as_str().map(str::to_string)
}

/// `getRefreshInterval()`: `statusLine?.refreshInterval ?? null`, whatever its type.
pub fn refresh_interval() -> Option<Value> {
    let settings = load().ok()?;
    settings.get("statusLine")?.get("refreshInterval").filter(|v| !v.is_null()).cloned()
}

/// `setRefreshInterval(interval)`: Ok without writing when the settings cannot be read or have no `statusLine`.
pub fn set_refresh_interval(interval: Option<f64>) -> Result<(), ()> {
    let Ok(mut settings) = load() else { return Ok(()) };
    let Some(status_line) = settings.get_mut("statusLine").filter(|v| super::truthy(v)) else { return Ok(()) };
    match status_line {
        Value::Object(o) => match interval {
            None => {
                o.shift_remove("refreshInterval");
            }
            Some(n) => {
                o.insert("refreshInterval".into(), crate::jsjson::num(n));
            }
        },
        // A property set on an array is not serialized; one set on a primitive throws in strict mode.
        Value::Array(_) => {}
        _ => return Err(()),
    }
    crate::claude_config::try_save_settings(&settings).map_err(|_| ())
}

/// `getClaudeCodeVersion()`: the leading `x.y.z` of `claude --version`, run through the shell with a 5 s timeout.
fn claude_code_version() -> Option<String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let mut child = Command::new("/bin/sh")
        .args(["-c", "claude --version"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    drop(child.stdin.take());
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = stdout.read_to_end(&mut out);
        out
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(5)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    let out = reader.join().ok()?;
    if !status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out);
    let re = regex::Regex::new(r"^([0-9]+\.[0-9]+\.[0-9]+)").unwrap();
    re.captures(crate::jsnum::js_trim(&text)).map(|c| c[1].to_string())
}

/// `isClaudeCodeVersionAtLeast(minVersion)`.
pub fn is_claude_code_version_at_least(min_version: &str) -> bool {
    let Some(version) = claude_code_version() else { return false };
    let parts = |v: &str| -> Vec<f64> { v.split('.').map(|p| p.parse::<f64>().unwrap_or(f64::NAN)).collect() };
    let current = parts(&version);
    let minimum = parts(min_version);
    for i in 0..3 {
        let c = current.get(i).copied().unwrap_or(0.0);
        let m = minimum.get(i).copied().unwrap_or(0.0);
        if c > m {
            return true;
        }
        if c < m {
            return false;
        }
    }
    true
}

fn method_of(metadata: Option<&Value>) -> Option<InstallMethod> {
    Some(match metadata?.get("method")?.as_str()? {
        "auto-update" => InstallMethod::AutoUpdate,
        "pinned" => InstallMethod::Pinned,
        "self-managed" => InstallMethod::SelfManaged,
        _ => InstallMethod::Unknown,
    })
}

/// `classifyInstallation(command, metadata).method`.
pub fn classify(command: Option<&str>, metadata: Option<&Value>) -> InstallMethod {
    let command = command.unwrap_or("");
    if matches_base(command, AUTO_NPX) || matches_base(command, AUTO_BUNX) {
        return InstallMethod::AutoUpdate;
    }
    if matches_base(command, GLOBAL) {
        if method_of(metadata) == Some(InstallMethod::Pinned) {
            return InstallMethod::Pinned;
        }
        return InstallMethod::SelfManaged;
    }
    if is_local_development_command(command) {
        return InstallMethod::SelfManaged;
    }
    InstallMethod::Unknown
}

/// `getCurrentInstallation(...).method`.
pub fn current_installation(installed: bool, existing: Option<&str>, metadata: Option<&Value>) -> InstallMethod {
    if installed && existing.is_none() {
        if let Some(m) = method_of(metadata) {
            return m;
        }
    }
    classify(existing, metadata)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_commands() {
        assert!(is_known_command("npx -y ccstatusline@latest"));
        assert!(is_known_command("ccstatusline --config /tmp/x.json"));
        assert!(is_known_command("bun run /src/ccstatusline.ts"));
        assert!(!is_known_command("ccstatusline-rs"));
        assert!(!is_known_command("bun run /src/ccstatusline.tsx"));
    }
}
