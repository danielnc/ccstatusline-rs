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

/// `getRefreshInterval()`.
pub fn refresh_interval() -> Option<f64> {
    let settings = load().ok()?;
    settings.get("statusLine")?.get("refreshInterval")?.as_f64()
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
