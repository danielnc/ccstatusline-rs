//! The editor half of the misc widgets (upstream `CustomText.tsx`, `CustomCommand.tsx`, `CustomSymbol.tsx`,
//! `Link.tsx`, `CurrentWorkingDir.tsx`, `SessionCost.ts`, `TerminalWidth.ts`, `Version.ts`, ...): editor display,
//! hideable states, key bindings and editor actions. Label prefixes and number-format support sit on the impls.

use super::custom_command::utf16_prefix;
use crate::jsnum::js_trim;
use crate::jsobj;
use crate::settings::WidgetItem;
use crate::widgets::editor::{self, symbol_keybind};
use crate::widgets::shared::is_flag;
use crate::widgets::{EditorDisplay, HideableState, Keybind};
use serde_json::{Map, Value};

/// The types whose editor methods are ported (checked by `editor_meta_tests`).
pub(crate) const EDITOR_PORTED: &[&str] = &[
    "output-style",
    "current-working-dir",
    "session-cost",
    "session-cost-rate",
    "terminal-width",
    "version",
    "custom-text",
    "custom-symbol",
    "custom-command",
    "link",
    "claude-session-id",
    "sandbox-status",
    "free-memory",
    "vim-mode",
    "voice-status",
    "remote-control-status",
];

pub const DEFAULT_VALUE_HIDEABLE: HideableState =
    HideableState { key: "default-value", label: "when style is 'default'", default_enabled: false };
pub const ZERO_COST_HIDEABLE: HideableState =
    HideableState { key: "zero", label: "when cost is $0.00", default_enabled: false };

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

// --- current-working-dir ---------------------------------------------------------------------------------------

const TOGGLE_ABBREVIATE_HOME_ACTION: &str = "toggle-abbreviate-home";
const TOGGLE_FISH_STYLE_ACTION: &str = "toggle-fish-style";

pub fn cwd_display(item: &WidgetItem, name: &str) -> EditorDisplay {
    // `metadata.segments ? parseInt(segments, 10) : undefined`, shown when > 0
    let segments = item.meta("segments").filter(|s| !s.is_empty()).and_then(crate::jsnum::parse_int);
    let mut modifiers = Vec::new();
    if is_flag(item, "abbreviateHome") {
        modifiers.push("~".to_string());
    }
    if is_flag(item, "fishStyle") {
        modifiers.push("fish-style".to_string());
    } else if let Some(n) = segments.filter(|n| *n > 0.0) {
        modifiers.push(format!("segments: {}", crate::jsnum::to_js_string(n)));
    }
    EditorDisplay { text: name.into(), modifier: editor::make_modifier_text(&modifiers) }
}

pub fn cwd_keybinds() -> Vec<Keybind> {
    vec![
        Keybind::new("h", "(h)ome ~", TOGGLE_ABBREVIATE_HOME_ACTION),
        Keybind::new("s", "(s)egments", "edit-segments"),
        Keybind::new("f", "(f)ish style", TOGGLE_FISH_STYLE_ACTION),
        symbol_keybind(),
    ]
}

/// The two mutually exclusive toggles: turning `flag` on drops `clears` and sets it to `'true'`; turning it off
/// drops it, and the metadata with it when nothing else is left.
fn toggle_exclusive_flag(item: &Value, flag: &str, clears: &[&str]) -> Value {
    let meta = jsobj::get(item, "metadata").cloned().unwrap_or_else(|| Value::Object(Map::new()));
    if jsobj::get_str(&meta, flag) != Some("true") {
        let rest = clears.iter().fold(meta, |m, k| jsobj::without(&m, k));
        return jsobj::with(item, "metadata", jsobj::with(&rest, flag, Value::String("true".into())));
    }
    let rest = jsobj::without(&meta, flag);
    let empty = rest.as_object().is_none_or(Map::is_empty);
    jsobj::with(item, "metadata", if empty { jsobj::undefined() } else { rest })
}

pub fn cwd_action(action: &str, item: &Value) -> Option<Value> {
    match action {
        TOGGLE_ABBREVIATE_HOME_ACTION => Some(toggle_exclusive_flag(item, "abbreviateHome", &["fishStyle"])),
        TOGGLE_FISH_STYLE_ACTION => Some(toggle_exclusive_flag(item, "fishStyle", &["segments", "abbreviateHome"])),
        _ => None,
    }
}

// --- session-cost-rate -----------------------------------------------------------------------------------------

const CLOCK_TIME_KEY: &str = "clockTime";
const TOGGLE_CLOCK_TIME_ACTION: &str = "toggle-clock-time";

pub fn cost_rate_display(item: &WidgetItem, name: &str) -> EditorDisplay {
    let modifier = if is_flag(item, CLOCK_TIME_KEY) { "(clock time)" } else { "(active time)" };
    EditorDisplay { text: name.into(), modifier: Some(modifier.into()) }
}

pub fn cost_rate_keybinds(item: &WidgetItem) -> Vec<Keybind> {
    let label = if is_flag(item, CLOCK_TIME_KEY) { "(t)ime: use active time" } else { "(t)ime: use clock time" };
    vec![Keybind::new("t", label, TOGGLE_CLOCK_TIME_ACTION)]
}

pub fn cost_rate_action(action: &str, item: &Value) -> Option<Value> {
    (action == TOGGLE_CLOCK_TIME_ACTION).then(|| jsobj::toggle_metadata_flag(item, CLOCK_TIME_KEY))
}

// --- custom-text / custom-symbol -------------------------------------------------------------------------------

pub fn custom_text_display(item: &WidgetItem, name: &str) -> EditorDisplay {
    EditorDisplay::plain(format!("{name} ({})", item.custom_text.as_deref().unwrap_or("Empty")))
}

pub fn custom_symbol_display(item: &WidgetItem, name: &str) -> EditorDisplay {
    EditorDisplay::plain(format!("{name} ({})", item.custom_symbol.as_deref().unwrap_or("?")))
}

// --- custom-command --------------------------------------------------------------------------------------------

const TOGGLE_PRESERVE_ACTION: &str = "toggle-preserve";

pub fn custom_command_display(item: &WidgetItem, name: &str) -> EditorDisplay {
    let cmd = item.command_path.as_deref().unwrap_or("No command");
    let truncated = if utf16_len(cmd) > 20 { format!("{}...", utf16_prefix(cmd, 17)) } else { cmd.to_string() };
    let mut modifiers = Vec::new();
    if let Some(m) = editor::max_width_modifier(item) {
        modifiers.push(m);
    }
    // `item.timeout && item.timeout !== 1000`
    if let Some(t) = item.timeout.filter(|t| *t != 0.0 && !t.is_nan() && *t != 1000.0) {
        modifiers.push(format!("timeout:{}ms", crate::jsnum::to_js_string(t)));
    }
    if item.preserve_colors == Some(true) {
        modifiers.push("preserve".to_string());
    }
    EditorDisplay { text: format!("{name} ({truncated})"), modifier: editor::make_modifier_text(&modifiers) }
}

pub fn custom_command_keybinds() -> Vec<Keybind> {
    vec![
        Keybind::new("e", "(e)dit cmd", "edit-command"),
        Keybind::new("w", "(w)idth", "edit-width"),
        Keybind::new("t", "(t)imeout", "edit-timeout"),
        Keybind::new("p", "(p)reserve colors", TOGGLE_PRESERVE_ACTION),
    ]
}

pub fn custom_command_action(action: &str, item: &Value) -> Option<Value> {
    // `{ ...item, preserveColors: !item.preserveColors }`
    let preserve = jsobj::get_bool(item, "preserveColors") == Some(true);
    (action == TOGGLE_PRESERVE_ACTION).then(|| jsobj::with(item, "preserveColors", Value::Bool(!preserve)))
}

// --- link ------------------------------------------------------------------------------------------------------

pub fn link_display(item: &WidgetItem, name: &str) -> EditorDisplay {
    let url = item.meta("url").map(js_trim).unwrap_or("");
    let custom_text = item.meta("text").map(js_trim).filter(|t| !t.is_empty());
    let label = custom_text.unwrap_or(if url.is_empty() { "no url" } else { url });
    let text = if item.raw() { label.to_string() } else { format!("🔗 {label}") };
    let short_url = (custom_text.is_some() && !url.is_empty()).then(|| {
        if utf16_len(url) > 28 {
            format!("{}...", utf16_prefix(url, 25))
        } else {
            url.to_string()
        }
    });
    EditorDisplay { text: format!("{name} ({text})"), modifier: short_url.map(|u| format!("({u})")) }
}

pub fn link_keybinds() -> Vec<Keybind> {
    vec![Keybind::new("u", "(u)rl", "edit-url"), Keybind::new("e", "(e)dit text", "edit-text")]
}
