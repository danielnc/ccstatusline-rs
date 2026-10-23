//! Claude Code's layered settings (`voice.enabled`, `sandbox.enabled`) and
//! per-PID session manifests (remote control), ported from upstream
//! `src/utils/claude-settings.ts` (`getVoiceConfig`, `getSandboxConfig`,
//! `resolveClaudeConfigCwd`, `getRemoteControlStatus`).

use crate::context::RenderContext;
use serde_json::Value;

/// `resolveClaudeConfigCwd(context)`: the first non-blank of
/// project_dir, cwd, current_dir.
pub fn resolve_config_cwd<'a>(ctx: &'a RenderContext) -> Option<&'a str> {
    let ws = ctx.data.workspace.as_ref();
    [ws.and_then(|w| w.project_dir.as_deref()), ctx.data.cwd.as_deref(), ws.and_then(|w| w.current_dir.as_deref())]
        .into_iter()
        .flatten()
        .find(|c| !crate::jsnum::js_trim(c).is_empty())
}

/// `getLayeredSettingsCandidatePathsByPriority(cwd)`, deduplicated in order.
fn candidate_paths(cwd: &str) -> Vec<String> {
    let user_dir = crate::claude_config::config_dir().to_string_lossy().into_owned();
    let project_dir = super::posix_join(&[cwd, ".claude"]);
    let all = [
        super::posix_join(&[&project_dir, "settings.local.json"]),
        super::posix_join(&[&project_dir, "settings.json"]),
        super::posix_join(&[&user_dir, "settings.local.json"]),
        super::posix_join(&[&user_dir, "settings.json"]),
    ];
    let mut out: Vec<String> = Vec::new();
    for p in all {
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// `fs.readFileSync(p, 'utf-8')`: Err(true) when the error is not ENOENT
/// (the file "existed" for the layer logic).
fn read_text(path: &str) -> Result<String, bool> {
    match std::fs::read(path) {
        Ok(b) => Ok(String::from_utf8_lossy(&b).into_owned()),
        Err(e) => Err(e.raw_os_error() != Some(libc::ENOENT)),
    }
}

/// One layer: (file existed, `<key>.enabled` when the layer defines it).
fn read_layer(path: &str, key: &str) -> (bool, Option<bool>) {
    let text = match read_text(path) {
        Ok(t) => t,
        Err(existed) => return (existed, None),
    };
    let Ok(parsed) = serde_json::from_str::<Value>(&text) else { return (true, None) };
    // `parsed.<key>`: null throws (caught -> no override); primitives have no such key
    let section = match &parsed {
        Value::Object(o) => o.get(key),
        _ => None,
    };
    let enabled = match section {
        // z.object({ enabled: z.boolean().optional() }).safeParse(section)
        Some(Value::Object(o)) => match o.get("enabled") {
            Some(Value::Bool(b)) => Some(*b),
            _ => None,
        },
        _ => None,
    };
    (true, enabled)
}

/// `getVoiceConfig` / `getSandboxConfig`: None when no layer file exists.
pub fn layered_enabled(cwd: Option<&str>, key: &str) -> Option<bool> {
    let cwd = match cwd {
        Some(c) => c.to_string(),
        None => std::env::current_dir().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default(),
    };
    let mut any_existed = false;
    for p in candidate_paths(&cwd) {
        let (existed, enabled) = read_layer(&p, key);
        any_existed |= existed;
        if let Some(e) = enabled {
            return Some(e);
        }
    }
    any_existed.then_some(false)
}

/// `getRemoteControlStatus(sessionId)`.
pub fn remote_control_enabled(session_id: Option<&str>) -> Option<bool> {
    let sid = session_id.filter(|s| !s.is_empty())?;
    let dir = crate::claude_config::config_dir().join("sessions");
    let mut names: Vec<std::ffi::OsString> = std::fs::read_dir(&dir).ok()?.flatten().map(|e| e.file_name()).collect();
    // libuv's scandir sorts entries with strcmp
    names.sort_by(|a, b| a.as_encoded_bytes().cmp(b.as_encoded_bytes()));
    for name in names {
        let lossy = name.to_string_lossy();
        if !lossy.ends_with(".json") {
            continue;
        }
        let Ok(text) = read_text(&dir.join(&name).to_string_lossy()) else { continue };
        let Ok(Value::Object(o)) = serde_json::from_str::<Value>(&text) else { continue };
        // z.object({ sessionId: string?, bridgeSessionId: string|null? })
        let session = match o.get("sessionId") {
            None => None,
            Some(Value::String(s)) => Some(s.as_str()),
            Some(_) => continue,
        };
        let bridge = match o.get("bridgeSessionId") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) => Some(s.as_str()),
            Some(_) => continue,
        };
        if session != Some(sid) {
            continue;
        }
        return Some(bridge.is_some_and(|b| !b.is_empty()));
    }
    None
}
