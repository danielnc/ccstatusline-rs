//! The editor half of the transcript widgets (upstream `Tokens*.ts`, `Context*.ts`, `*Speed.ts`, `Session*.ts`,
//! `Cache*.ts`, ...): shared editor helpers for this group live here; each widget's own methods sit on its impl.

use crate::jsobj;
use crate::settings::WidgetItem;
use crate::widgets::shared::is_flag;
use crate::widgets::{editor, EditorDisplay, HideableState, Keybind};
use serde_json::Value;

/// The types whose editor methods are ported (checked by `editor_meta_tests`).
pub(crate) const EDITOR_PORTED: &[&str] = &[
    "tokens-input",
    "tokens-output",
    "tokens-cached",
    "tokens-total",
    "cache-hit-rate",
    "cache-read",
    "cache-write",
    "cache-timer",
    "input-speed",
    "output-speed",
    "total-speed",
    "context-length",
    "context-window",
    "context-percentage",
    "context-percentage-usable",
    "session-clock",
    "compaction-counter",
    "thinking-effort",
    "session-name",
    "skills",
];

// --- Token counts (`shared/token-count-widget.ts`) --------------------------------------------------------------

pub const TOKEN_ZERO: HideableState =
    HideableState { key: "zero", label: "when token count is zero", default_enabled: false };

// --- Cache scope (`shared/cache-scope.ts`) ----------------------------------------------------------------------

const SCOPE_SESSION_KEY: &str = "cacheScopeSession";
const TOGGLE_CACHE_SCOPE_ACTION: &str = "toggle-cache-scope";

pub const CACHE_EMPTY: HideableState =
    HideableState { key: "empty", label: "when there is no cache activity", default_enabled: false };

/// `getCacheModifierText`.
pub fn cache_display(name: &str, item: &WidgetItem) -> EditorDisplay {
    let mut modifiers = Vec::new();
    if is_flag(item, SCOPE_SESSION_KEY) {
        modifiers.push("session".to_string());
    }
    EditorDisplay { text: name.into(), modifier: editor::make_modifier_text(&modifiers) }
}

/// `getCacheKeybinds`.
pub fn cache_keybinds() -> Vec<Keybind> {
    vec![Keybind::new("t", "(t)urn/session", TOGGLE_CACHE_SCOPE_ACTION)]
}

/// `handleCacheOptionsAction`.
pub fn cache_action(action: &str, item: &Value) -> Option<Value> {
    (action == TOGGLE_CACHE_SCOPE_ACTION).then(|| jsobj::toggle_metadata_flag(item, SCOPE_SESSION_KEY))
}

// --- Context inverse and slider (`shared/context-{inverse,slider}.ts`) ------------------------------------------

const INVERSE_KEY: &str = "inverse";
const TOGGLE_INVERSE_ACTION: &str = "toggle-inverse";
const TOGGLE_SLIDER_ACTION: &str = "toggle-slider";

/// Context % and Context % (usable)'s `getEditorDisplay`: the inverse and slider modifiers, unwrapped and joined.
pub fn context_percentage_display(name: &str, item: &WidgetItem) -> EditorDisplay {
    let mut modifiers = Vec::new();
    if is_flag(item, INVERSE_KEY) {
        modifiers.push("remaining".to_string());
    }
    match item.meta("display") {
        Some("slider") => modifiers.push("short bar".into()),
        Some("slider-only") => modifiers.push("short bar only".into()),
        _ => {}
    }
    EditorDisplay { text: name.into(), modifier: editor::make_modifier_text(&modifiers) }
}

pub fn context_percentage_keybinds() -> Vec<Keybind> {
    vec![
        Keybind::new("u", "(u)sed/remaining", TOGGLE_INVERSE_ACTION),
        Keybind::new("p", "(p)rogress toggle", TOGGLE_SLIDER_ACTION),
    ]
}

/// `cycleContextSliderMode`: none → slider → slider-only → none.
fn cycle_slider_mode(item: &Value) -> Value {
    match jsobj::metadata_str(item, "display") {
        Some("slider") => jsobj::set_metadata(item, "display", "slider-only"),
        Some("slider-only") => jsobj::remove_metadata_keys(item, &["display"]),
        _ => jsobj::set_metadata(item, "display", "slider"),
    }
}

pub fn context_percentage_action(action: &str, item: &Value) -> Option<Value> {
    match action {
        TOGGLE_SLIDER_ACTION => Some(cycle_slider_mode(item)),
        TOGGLE_INVERSE_ACTION => Some(jsobj::toggle_metadata_flag(item, INVERSE_KEY)),
        _ => None,
    }
}

// --- Speed (`shared/speed-widget.tsx`) --------------------------------------------------------------------------

pub const SPEED_NO_DATA: HideableState =
    HideableState { key: "no-data", label: "when there is no speed data (—)", default_enabled: false };

/// `getSpeedWidgetEditorDisplay`.
pub fn speed_display(name: &str, window_seconds: f64) -> EditorDisplay {
    let modifier = if window_seconds > 0.0 {
        format!("{}s window", crate::jsnum::to_js_string(window_seconds))
    } else {
        "session avg".to_string()
    };
    EditorDisplay { text: name.into(), modifier: editor::make_modifier_text(&[modifier]) }
}

pub fn speed_keybinds() -> Vec<Keybind> {
    vec![Keybind::new("w", "(w)indow", "edit-window")]
}
