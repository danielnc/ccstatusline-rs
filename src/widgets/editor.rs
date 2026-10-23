//! Editor-side helpers shared by widgets: modifier text, the shared key bindings and hideable states
//! (upstream `src/widgets/shared/{editor-display,max-width,symbol-override,hideable}.ts`), and the editor's own
//! overrides on item JSON objects: hide states, label overrides (`raw-or-labeled.ts`) and the precision cycle
//! (`utils/number-format.ts`).

use super::{HideableState, Keybind};
use crate::jsobj;
use crate::settings::WidgetItem;
use serde_json::{Map, Value};

/// `makeModifierText`.
pub fn make_modifier_text(modifiers: &[String]) -> Option<String> {
    (!modifiers.is_empty()).then(|| format!("({})", modifiers.join(", ")))
}

pub const MAX_WIDTH_ACTION: &str = "edit-max-width";
pub const SYMBOL_OVERRIDE_ACTION: &str = "edit-symbol-override";

pub fn max_width_keybind() -> Keybind {
    Keybind::new("w", "(w)idth", MAX_WIDTH_ACTION)
}

pub fn symbol_keybind() -> Keybind {
    Keybind::new("g", "(g)lyph", SYMBOL_OVERRIDE_ACTION)
}

/// `getMaxWidthModifier`: `max:N` when a (truthy) max width is set.
pub fn max_width_modifier(item: &WidgetItem) -> Option<String> {
    item.max_width.filter(|w| *w != 0.0 && !w.is_nan()).map(|w| format!("max:{}", crate::jsnum::to_js_string(w)))
}

pub const NO_GIT: HideableState =
    HideableState { key: "no-git", label: "when not in a git repo", default_enabled: false };
pub const NO_JJ: HideableState = HideableState { key: "no-jj", label: "when not in a jj repo", default_enabled: false };
pub const NO_REMOTE: HideableState =
    HideableState { key: "no-remote", label: "when there is no remote", default_enabled: false };
pub const NO_UPSTREAM: HideableState =
    HideableState { key: "no-upstream", label: "when there is no upstream", default_enabled: false };
pub const MERGE_TARGET_HIDDEN: HideableState =
    HideableState { key: "merge-target-hidden", label: "when merge target is hidden", default_enabled: false };

pub const EDIT_HIDE_STATES_ACTION: &str = "edit-hide-states";
pub const EDIT_LABEL_ACTION: &str = "edit-label";
pub const CYCLE_NUMBER_STYLE_ACTION: &str = "cycle-number-style";

pub fn hide_keybind() -> Keybind {
    Keybind::new("h", "(h)ide…", EDIT_HIDE_STATES_ACTION)
}

pub fn label_keybind() -> Keybind {
    Keybind::new("e", "(e)dit label…", EDIT_LABEL_ACTION)
}

pub fn number_format_keybind() -> Keybind {
    Keybind::new(".", "(.) precision", CYCLE_NUMBER_STYLE_ACTION)
}

// --- Hide states -----------------------------------------------------------------------------------------------

fn parse_hide_states(value: Option<&str>) -> Vec<String> {
    value
        .map(|v| v.split(',').map(|k| crate::jsnum::js_trim(k).to_string()).filter(|k| !k.is_empty()).collect())
        .unwrap_or_default()
}

pub fn is_hidden(item: &Value, key: &str, default_enabled: bool) -> bool {
    match jsobj::metadata_str(item, "hide") {
        None => default_enabled,
        Some(v) => parse_hide_states(Some(v)).iter().any(|k| k == key),
    }
}

pub fn enabled_hide_states(item: &Value, states: &[HideableState]) -> Vec<&'static str> {
    states.iter().filter(|s| is_hidden(item, s.key, s.default_enabled)).map(|s| s.key).collect()
}

/// `setEnabledHideStates`: the canonical list, or no `hide` key when it matches the defaults.
pub fn set_enabled_hide_states(item: &Value, states: &[HideableState], enabled: &[&str]) -> Value {
    let ordered: Vec<&str> = states.iter().filter(|s| enabled.contains(&s.key)).map(|s| s.key).collect();
    let defaults: Vec<&str> = states.iter().filter(|s| s.default_enabled).map(|s| s.key).collect();
    let matches_defaults = ordered.len() == defaults.len() && ordered.iter().all(|k| defaults.contains(k));
    let cleaned = jsobj::remove_metadata_keys(item, &["hide"]);
    if matches_defaults {
        return cleaned;
    }
    jsobj::set_metadata(&cleaned, "hide", &ordered.join(","))
}

pub fn hide_modifier_text(item: &Value, states: &[HideableState]) -> Option<String> {
    let enabled = enabled_hide_states(item, states);
    (!enabled.is_empty()).then(|| format!("(hide: {})", enabled.join(", ")))
}

// --- Labels ----------------------------------------------------------------------------------------------------

pub fn get_label(item: &Value, default_label: &str) -> String {
    jsobj::metadata_str(item, "label").unwrap_or(default_label).to_string()
}

pub fn clear_label(item: &Value) -> Value {
    jsobj::remove_metadata_keys(item, &["label"])
}

pub fn set_label(item: &Value, value: &str) -> Value {
    jsobj::set_metadata(item, "label", value)
}

pub fn label_modifier_text(item: &Value) -> Option<String> {
    let label = jsobj::metadata_str(item, "label")?;
    let mut quoted = String::new();
    crate::jsjson::quote(label, &mut quoted);
    Some(format!("(label: {quoted})"))
}

// --- Number format ---------------------------------------------------------------------------------------------

fn number_style(item: &Value) -> Option<&str> {
    jsobj::get(item, "numberFormat").and_then(|f| jsobj::get_str(f, "style"))
}

pub fn number_format_modifier_text(item: &Value) -> Option<String> {
    match number_style(item) {
        Some(s @ ("compact" | "whole")) => Some(format!("({s})")),
        _ => None,
    }
}

/// `cycleNumberStyle`: precise → compact → whole → precise, keeping any explicit `decimals`.
pub fn cycle_number_style(item: &Value) -> Value {
    let current = number_style(item).filter(|s| *s != "precise");
    let next = match current {
        None => Some("compact"),
        Some("compact") => Some("whole"),
        _ => None,
    };
    let decimals = jsobj::get(item, "numberFormat").and_then(|f| jsobj::get(f, "decimals")).cloned();
    if next.is_none() && decimals.is_none() {
        return jsobj::without(item, "numberFormat");
    }
    let mut format = Map::new();
    if let Some(n) = next {
        format.insert("style".into(), Value::String(n.into()));
    }
    if let Some(d) = decimals {
        format.insert("decimals".into(), d);
    }
    jsobj::with(item, "numberFormat", Value::Object(format))
}
