//! Widget hooks: syncing them into Claude's settings.json (upstream
//! `src/utils/hooks.ts`) and the `--hook` handler (`hook-handler.ts`).

use crate::settings::Settings;
use serde_json::{Map, Value};

const HOOK_TAG: &str = "ccstatusline-managed";

fn is_legacy_command(cmd: &str) -> bool {
    // /ccstatusline.* --hook(?:\s|$)/
    let Some(start) = cmd.find("ccstatusline") else { return false };
    let rest = &cmd[start..];
    let mut search = 0;
    while let Some(i) = rest[search..].find(" --hook") {
        let after = &rest[search + i + 7..];
        if after.is_empty() || after.starts_with(char::is_whitespace) {
            return true;
        }
        search += i + 1;
    }
    false
}

fn strip_entry(entry: &Value) -> Option<Value> {
    let o = entry.as_object()?;
    if o.get("_tag").and_then(Value::as_str) == Some(HOOK_TAG) {
        return None;
    }
    let Some(Value::Array(hooks)) = o.get("hooks") else { return Some(entry.clone()) };
    let remaining: Vec<Value> = hooks
        .iter()
        .filter(|h| !h.get("command").and_then(Value::as_str).is_some_and(is_legacy_command))
        .cloned()
        .collect();
    if remaining.len() == hooks.len() {
        return Some(entry.clone());
    }
    if remaining.is_empty() {
        return None;
    }
    let mut m = o.clone();
    m.insert("hooks".into(), Value::Array(remaining));
    Some(Value::Object(m))
}

fn strip_managed(hooks: &mut Map<String, Value>) {
    let keys: Vec<String> = hooks.keys().cloned().collect();
    for k in keys {
        let list: Vec<Value> = match hooks.get(&k) {
            Some(Value::Array(a)) => a.iter().filter_map(strip_entry).collect(),
            _ => vec![],
        };
        if list.is_empty() {
            hooks.shift_remove(&k);
        } else {
            hooks.insert(k, Value::Array(list));
        }
    }
}

/// `(event, matcher)` hook definitions declared by the configured widgets.
fn active_hook_defs(st: &Settings) -> Vec<(&'static str, Option<&'static str>)> {
    let mut seen = Vec::new();
    for line in &st.lines {
        for item in line {
            if let Some(w) = crate::widgets::get_widget(&item.kind) {
                for &(event, matcher) in w.hooks() {
                    if !seen.contains(&(event, matcher)) {
                        seen.push((event, matcher));
                    }
                }
            }
        }
    }
    seen
}

pub fn sync_widget_hooks(st: &Settings) {
    let needed = active_hook_defs(st);
    let Ok(mut claude) = crate::claude_config::load_settings() else { return };
    let Some(obj) = claude.as_object_mut() else { return };
    let mut hooks = obj.get("hooks").and_then(Value::as_object).cloned().unwrap_or_default();
    strip_managed(&mut hooks);
    let status_cmd = obj.get("statusLine").and_then(|s| s.get("command")).and_then(Value::as_str).map(String::from);
    if status_cmd.as_deref().is_some_and(|c| c.contains("ccstatusline")) {
        let hook_cmd = format!("{} --hook", status_cmd.unwrap());
        for (event, matcher) in needed {
            let mut entry = Map::new();
            entry.insert("_tag".into(), Value::String(HOOK_TAG.into()));
            let mut h = Map::new();
            h.insert("type".into(), Value::String("command".into()));
            h.insert("command".into(), Value::String(hook_cmd.clone()));
            entry.insert("hooks".into(), Value::Array(vec![Value::Object(h)]));
            if let Some(m) = matcher {
                entry.insert("matcher".into(), Value::String(m.into()));
            }
            let list = hooks.entry(event.to_string()).or_insert_with(|| Value::Array(vec![]));
            if let Value::Array(a) = list {
                a.push(Value::Object(entry));
            }
        }
    }
    if hooks.is_empty() {
        obj.shift_remove("hooks");
    } else {
        obj.insert("hooks".into(), Value::Object(hooks));
    }
    crate::claude_config::save_settings(&claude);
}

pub fn skills_file_path(session_id: &str) -> Option<std::path::PathBuf> {
    if session_id.is_empty() || !session_id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
        return None;
    }
    Some(
        crate::settings::home_dir()
            .join(".cache")
            .join("ccstatusline")
            .join("skills")
            .join(format!("skills-{session_id}.jsonl")),
    )
}

/// `--hook` mode: record skill invocations for the skills widget.
pub fn handle_hook_input(input: Option<&str>) {
    let Some(input) = input.filter(|s| !s.is_empty()) else { return };
    let Ok(data) = serde_json::from_str::<Value>(input) else { return };
    let Some(session_id) = data.get("session_id").and_then(Value::as_str).filter(|s| !s.is_empty()) else { return };
    let event = data.get("hook_event_name").and_then(Value::as_str);
    let mut skill = String::new();
    if event == Some("PreToolUse") && data.get("tool_name").and_then(Value::as_str) == Some("Skill") {
        skill = data.get("tool_input").and_then(|t| t.get("skill")).and_then(Value::as_str).unwrap_or("").to_string();
    } else if event == Some("UserPromptSubmit") {
        let prompt = data.get("prompt").and_then(Value::as_str).unwrap_or("");
        if let Some(rest) = prompt.strip_prefix('/') {
            let name: String =
                rest.chars().take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | ':' | '-')).collect();
            let after = &rest[name.len()..];
            if !name.is_empty() && (after.is_empty() || after.starts_with(crate::gradient::is_js_space)) {
                skill = name;
            }
        }
    }
    if skill.is_empty() {
        return;
    }
    let Some(path) = skills_file_path(session_id) else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut entry = Map::new();
    entry.insert("timestamp".into(), Value::String(iso_now()));
    entry.insert("session_id".into(), Value::String(session_id.into()));
    entry.insert("skill".into(), Value::String(skill));
    entry.insert("source".into(), event.map(|e| Value::String(e.into())).unwrap_or(Value::Null));
    // JSON.stringify drops an undefined source
    if event.is_none() {
        entry.shift_remove("source");
    }
    let line = format!("{}\n", crate::jsjson::stringify(&Value::Object(entry)));
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = f.write_all(line.as_bytes());
    }
}

/// `new Date().toISOString()`
pub fn iso_now() -> String {
    crate::timefmt::iso_string(crate::clock::now_ms())
}

/// `removeManagedHooks`: the hooks ccstatusline added, gone from Claude's settings.
pub fn remove_managed_hooks() {
    let Ok(mut claude) = crate::claude_config::load_settings() else { return };
    let Some(obj) = claude.as_object_mut() else { return };
    let mut hooks = obj.get("hooks").and_then(Value::as_object).cloned().unwrap_or_default();
    strip_managed(&mut hooks);
    if hooks.is_empty() {
        obj.shift_remove("hooks");
    } else {
        obj.insert("hooks".into(), Value::Object(hooks));
    }
    crate::claude_config::save_settings(&claude);
}
