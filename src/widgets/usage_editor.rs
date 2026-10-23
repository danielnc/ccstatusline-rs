//! The editor half of the usage widgets (upstream `SessionUsage.ts`, `WeeklyUsage.ts`, `*ResetTimer.ts`,
//! `BlockTimer.ts`, `ExtraUsage*.ts`, `ContextBar.ts`, `ClaudeStatus.ts`, ... and
//! `shared/{usage-display,usage-percent-widget}.ts`): editor display, hideable states, label prefixes, key bindings
//! and editor actions.

use super::editor;
use super::usage::{display_mode, is_progress, is_slider, DisplayMode, PercentKind, TimerKind};
use super::{EditorDisplay, HideableState, Keybind};
use crate::jsobj;
use crate::settings::WidgetItem;
use serde_json::Value;

/// The types whose editor methods are ported (checked by `editor_meta_tests`).
pub(crate) const EDITOR_PORTED: &[&str] = &[
    "session-usage",
    "weekly-usage",
    "weekly-sonnet-usage",
    "weekly-opus-usage",
    "fable-weekly-usage",
    "reset-timer",
    "weekly-reset-timer",
    "block-timer",
    "extra-usage-used",
    "extra-usage-remaining",
    "extra-usage-utilization",
    "extra-usage-daily-budget",
    "context-bar",
    "claude-status",
    "claude-account-email",
];

/// `USAGE_NO_DATA_HIDEABLE_STATE`, shared by the usage percentages and the reset timers.
pub const NO_DATA: HideableState =
    HideableState { key: "no-data", label: "when usage data is unavailable", default_enabled: false };
const BLOCK_NO_DATA: HideableState =
    HideableState { key: "no-data", label: "when there is no active block", default_enabled: false };
/// `EXTRA_USAGE_DISABLED_HIDEABLE_STATE`.
const EXTRA_USAGE_DISABLED: HideableState =
    HideableState { key: "disabled", label: "when extra usage is disabled", default_enabled: false };

pub const USAGE_STATES: &[HideableState] = &[NO_DATA];
pub const BLOCK_STATES: &[HideableState] = &[BLOCK_NO_DATA];
pub const EXTRA_USAGE_STATES: &[HideableState] = &[EXTRA_USAGE_DISABLED, NO_DATA];

const TOGGLE_PROGRESS: &str = "toggle-progress";
const TOGGLE_INVERT: &str = "toggle-invert";
const TOGGLE_COMPACT: &str = "toggle-compact";
const TOGGLE_CURSOR: &str = "toggle-cursor";
const TOGGLE_DATE: &str = "toggle-date";
const TOGGLE_HOUR_FORMAT: &str = "toggle-hour-format";
const TOGGLE_WEEKDAY: &str = "toggle-weekday";
const TOGGLE_HOURS: &str = "toggle-hours";
const TOGGLE_WEEKDAYS: &str = "toggle-weekdays";
const TOGGLE_HISTORY: &str = "toggle-history";

fn progress_keybind() -> Keybind {
    Keybind::new("p", "(p)rogress toggle", TOGGLE_PROGRESS)
}

fn is_bar(mode: DisplayMode) -> bool {
    is_progress(mode) || is_slider(mode)
}

fn flag(item: &WidgetItem, key: &str) -> bool {
    item.meta(key) == Some("true")
}

/// `getUsageTimezone` / `getUsageLocale`: a non-empty string.
fn non_empty_meta<'a>(item: &'a WidgetItem, key: &str) -> Option<&'a str> {
    item.meta(key).filter(|v| !v.is_empty())
}

fn bar_modifier(mode: DisplayMode) -> Option<&'static str> {
    match mode {
        DisplayMode::Progress => Some("long bar"),
        DisplayMode::ProgressShort => Some("medium bar"),
        DisplayMode::Slider => Some("short bar"),
        DisplayMode::SliderOnly => Some("short bar only"),
        DisplayMode::Time => None,
    }
}

#[derive(Default)]
struct ModifierOptions {
    include_compact: bool,
    include_cursor: bool,
    include_date: bool,
    show_usage_direction: bool,
}

/// `getUsageDisplayModifierText`.
fn display_modifier_text(item: &WidgetItem, options: ModifierOptions) -> Option<String> {
    let mode = display_mode(item);
    let mut modifiers: Vec<String> = Vec::new();
    modifiers.extend(bar_modifier(mode).map(String::from));
    let inverted = flag(item, "invert");
    if options.show_usage_direction {
        modifiers.push(if inverted { "remaining" } else { "used" }.into());
    } else if inverted {
        modifiers.push("inverted".into());
    }
    if options.include_cursor && flag(item, "cursor") && is_bar(mode) {
        modifiers.push("time cursor".into());
    }
    if options.include_compact && !is_progress(mode) && flag(item, "compact") {
        modifiers.push("compact".into());
    }
    let dated = options.include_date && !is_progress(mode) && flag(item, "absolute");
    if dated {
        modifiers.push("date".into());
        if flag(item, "hour12") {
            modifiers.push("12hr".into());
        }
        if let Some(tz) = non_empty_meta(item, "timezone") {
            modifiers.push(format!("tz: {tz}"));
        }
        if let Some(locale) = non_empty_meta(item, "locale") {
            modifiers.push(format!("locale: {locale}"));
        }
    }
    editor::make_modifier_text(&modifiers)
}

/// `cycleUsageDisplayMode`.
fn cycle_display_mode(
    item: &Value,
    disabled_in_progress_keys: &[&str],
    include_slider: bool,
    preserve_invert_in_time: bool,
) -> Value {
    let current = jsobj::metadata_str(item, "display")
        .filter(|m| matches!(*m, "progress" | "progress-short" | "slider" | "slider-only"))
        .unwrap_or("time");
    let next = match (include_slider, current) {
        (_, "time") => "progress",
        (_, "progress") => "progress-short",
        (true, "progress-short") => "slider",
        (true, "slider") => "slider-only",
        _ => "time",
    };
    let keys_to_remove: &[&str] = match (next, preserve_invert_in_time) {
        ("time", true) => &["cursor"],
        ("time", false) => &["invert", "cursor"],
        _ => disabled_in_progress_keys,
    };
    let next_item = jsobj::remove_metadata_keys(item, keys_to_remove);
    jsobj::set_metadata(&next_item, "display", next)
}

/// `getUsagePercentCustomKeybinds`.
fn percent_keybinds(item: &WidgetItem, include_cursor: bool) -> Vec<Keybind> {
    let next_direction = if flag(item, "invert") { "used" } else { "remaining" };
    let mut keybinds =
        vec![progress_keybind(), Keybind::new("u", &format!("(u) show {next_direction}"), TOGGLE_INVERT)];
    if include_cursor && is_bar(display_mode(item)) {
        keybinds.push(Keybind::new("t", "(t)ime cursor", TOGGLE_CURSOR));
    }
    keybinds
}

#[derive(Default)]
struct TimerKeybindOptions {
    include_date: bool,
    include_hour_format: bool,
    include_locale: bool,
    include_timezone: bool,
    include_weekday: bool,
}

/// `getUsageTimerCustomKeybinds`.
fn timer_keybinds(item: &WidgetItem, options: TimerKeybindOptions) -> Vec<Keybind> {
    let mut keybinds = vec![progress_keybind()];
    let bar_mode = is_bar(display_mode(item));
    if bar_mode {
        keybinds.push(Keybind::new("v", "in(v)ert fill", TOGGLE_INVERT));
    } else {
        keybinds.push(Keybind::new("s", "(s)hort time", TOGGLE_COMPACT));
        if options.include_date {
            keybinds.push(Keybind::new("t", "(t)imestamp", TOGGLE_DATE));
        }
    }
    if flag(item, "absolute") && !bar_mode {
        if options.include_hour_format {
            keybinds.push(Keybind::new("f", "12/24 (f)ormat", TOGGLE_HOUR_FORMAT));
        }
        if options.include_weekday {
            keybinds.push(Keybind::new("w", "(w)eekday", TOGGLE_WEEKDAY));
        }
        if options.include_timezone {
            keybinds.push(Keybind::new("z", "time(z)one", "edit-timezone"));
        }
        if options.include_locale {
            keybinds.push(Keybind::new("l", "(l)ocale", "edit-locale"));
        }
    }
    keybinds
}

// --- Usage percentages (usage-percent-widget.ts) -----------------------------------------------------------------

pub fn percent_display(item: &WidgetItem, name: &str) -> EditorDisplay {
    let options = ModifierOptions { show_usage_direction: true, include_cursor: true, ..Default::default() };
    EditorDisplay { text: name.into(), modifier: display_modifier_text(item, options) }
}

pub fn percent_label(kind: PercentKind) -> Option<String> {
    Some(kind.label().into())
}

pub fn percent_keybinds_for(item: &WidgetItem) -> Vec<Keybind> {
    percent_keybinds(item, true)
}

/// `handleUsagePercentWidgetEditorAction`.
pub fn percent_action(action: &str, item: &Value) -> Option<Value> {
    match action {
        TOGGLE_PROGRESS => Some(cycle_display_mode(item, &[], true, true)),
        TOGGLE_INVERT => Some(jsobj::toggle_metadata_flag(item, "invert")),
        TOGGLE_CURSOR => Some(jsobj::toggle_metadata_flag(item, "cursor")),
        _ => None,
    }
}

// --- Timers (BlockResetTimer.ts, WeeklyResetTimer.ts, BlockTimer.ts) ---------------------------------------------

/// `getWeeklyResetModifierText`.
fn weekly_reset_modifier_text(item: &WidgetItem) -> Option<String> {
    let mode = display_mode(item);
    let date_mode = flag(item, "absolute");
    let bar_mode = is_bar(mode);
    let mut modifiers: Vec<String> = Vec::new();
    modifiers.extend(bar_modifier(mode).map(String::from));
    if flag(item, "invert") {
        modifiers.push("inverted".into());
    }
    if !bar_mode {
        if flag(item, "compact") {
            modifiers.push("compact".into());
        }
        if date_mode {
            modifiers.push("date".into());
            if flag(item, "hour12") {
                modifiers.push("12hr".into());
            }
            if flag(item, "weekday") {
                modifiers.push("weekday".into());
            }
        } else if flag(item, "hours") {
            modifiers.push("hours only".into());
        }
        if date_mode {
            if let Some(tz) = non_empty_meta(item, "timezone") {
                modifiers.push(format!("tz: {tz}"));
            }
            if let Some(locale) = non_empty_meta(item, "locale") {
                modifiers.push(format!("locale: {locale}"));
            }
        }
    }
    editor::make_modifier_text(&modifiers)
}

pub fn timer_display(kind: TimerKind, item: &WidgetItem, name: &str) -> EditorDisplay {
    let modifier = match kind {
        TimerKind::BlockReset => display_modifier_text(
            item,
            ModifierOptions { include_compact: true, include_date: true, ..Default::default() },
        ),
        TimerKind::WeeklyReset => weekly_reset_modifier_text(item),
        TimerKind::Block => {
            display_modifier_text(item, ModifierOptions { include_compact: true, ..Default::default() })
        }
    };
    EditorDisplay { text: name.into(), modifier }
}

pub fn timer_states(kind: TimerKind) -> &'static [HideableState] {
    match kind {
        TimerKind::Block => BLOCK_STATES,
        _ => USAGE_STATES,
    }
}

pub fn timer_keybinds_for(kind: TimerKind, item: &WidgetItem) -> Vec<Keybind> {
    match kind {
        TimerKind::BlockReset => timer_keybinds(
            item,
            TimerKeybindOptions {
                include_date: true,
                include_hour_format: true,
                include_locale: true,
                include_timezone: true,
                ..Default::default()
            },
        ),
        TimerKind::WeeklyReset => {
            let mut keybinds = timer_keybinds(
                item,
                TimerKeybindOptions {
                    include_date: true,
                    include_hour_format: true,
                    include_weekday: true,
                    include_locale: true,
                    include_timezone: true,
                },
            );
            if !is_bar(display_mode(item)) && !flag(item, "absolute") {
                keybinds.push(Keybind::new("o", "(o)nly hours", TOGGLE_HOURS));
            }
            keybinds
        }
        TimerKind::Block => timer_keybinds(item, TimerKeybindOptions::default()),
    }
}

pub fn timer_action(kind: TimerKind, action: &str, item: &Value) -> Option<Value> {
    let toggle = |key| Some(jsobj::toggle_metadata_flag(item, key));
    match (kind, action) {
        (TimerKind::BlockReset, TOGGLE_PROGRESS) => {
            Some(cycle_display_mode(item, &["compact", "absolute"], true, false))
        }
        (TimerKind::WeeklyReset, TOGGLE_PROGRESS) => {
            Some(cycle_display_mode(item, &["compact", "hours", "absolute"], true, false))
        }
        (TimerKind::Block, TOGGLE_PROGRESS) => Some(cycle_display_mode(item, &["compact"], true, false)),
        (_, TOGGLE_INVERT) => toggle("invert"),
        (_, TOGGLE_COMPACT) => toggle("compact"),
        (TimerKind::BlockReset | TimerKind::WeeklyReset, TOGGLE_DATE) => toggle("absolute"),
        (TimerKind::BlockReset | TimerKind::WeeklyReset, TOGGLE_HOUR_FORMAT) => toggle("hour12"),
        (TimerKind::WeeklyReset, TOGGLE_WEEKDAY) => toggle("weekday"),
        (TimerKind::WeeklyReset, TOGGLE_HOURS) => toggle("hours"),
        _ => None,
    }
}

// --- Extra usage (ExtraUsageUtilization.ts, ExtraUsageDailyBudget.ts) --------------------------------------------

pub fn utilization_display(item: &WidgetItem, name: &str) -> EditorDisplay {
    let options = ModifierOptions { show_usage_direction: true, ..Default::default() };
    EditorDisplay { text: name.into(), modifier: display_modifier_text(item, options) }
}

pub fn utilization_keybinds(item: &WidgetItem) -> Vec<Keybind> {
    percent_keybinds(item, false)
}

pub fn utilization_action(action: &str, item: &Value) -> Option<Value> {
    match action {
        TOGGLE_PROGRESS => Some(cycle_display_mode(item, &[], true, true)),
        TOGGLE_INVERT => Some(jsobj::toggle_metadata_flag(item, "invert")),
        _ => None,
    }
}

const WEEKDAYS_ONLY_KEY: &str = "weekdaysOnly";

pub fn daily_budget_display(item: &WidgetItem, name: &str) -> EditorDisplay {
    EditorDisplay { text: name.into(), modifier: flag(item, WEEKDAYS_ONLY_KEY).then(|| "(weekdays)".into()) }
}

pub fn daily_budget_keybinds(item: &WidgetItem) -> Vec<Keybind> {
    let label = if flag(item, WEEKDAYS_ONLY_KEY) { "(w) count weekends" } else { "(w)eekdays only" };
    vec![Keybind::new("w", label, TOGGLE_WEEKDAYS)]
}

pub fn daily_budget_action(action: &str, item: &Value) -> Option<Value> {
    (action == TOGGLE_WEEKDAYS).then(|| jsobj::toggle_metadata_flag(item, WEEKDAYS_ONLY_KEY))
}

// --- Context bar (ContextBar.ts) ---------------------------------------------------------------------------------

/// ContextBar's own `getDisplayMode`: `progress-short` unless one of the other bars is set.
fn context_bar_mode(display: Option<&str>) -> &str {
    match display {
        Some(m @ ("progress" | "slider" | "slider-only")) => m,
        _ => "progress-short",
    }
}

pub fn context_bar_display(item: &WidgetItem, name: &str) -> EditorDisplay {
    let modifier = match context_bar_mode(item.meta("display")) {
        "progress-short" => Some("(medium bar)"),
        "slider" => Some("(short bar)"),
        "slider-only" => Some("(short bar only)"),
        _ => None,
    };
    EditorDisplay { text: name.into(), modifier: modifier.map(String::from) }
}

pub fn context_bar_keybinds() -> Vec<Keybind> {
    vec![progress_keybind()]
}

pub fn context_bar_action(action: &str, item: &Value) -> Option<Value> {
    if action != TOGGLE_PROGRESS {
        return None;
    }
    let next = match context_bar_mode(jsobj::metadata_str(item, "display")) {
        "progress-short" => "progress",
        "progress" => "slider",
        "slider" => "slider-only",
        _ => "progress-short",
    };
    Some(jsobj::set_metadata(item, "display", next))
}

// --- Claude status (ClaudeStatus.ts) -----------------------------------------------------------------------------

/// `isClaudeStatusHistoryEnabled` on the item's JSON object.
fn history_enabled(item: &Value) -> bool {
    jsobj::get_str(item, "type") == Some("claude-status") && jsobj::metadata_str(item, "history") == Some("true")
}

pub fn claude_status_display(item: &WidgetItem, name: &str) -> EditorDisplay {
    let modifier = crate::usage::service_status::history_enabled(item).then(|| "(history)".into());
    EditorDisplay { text: name.into(), modifier }
}

pub fn claude_status_keybinds() -> Vec<Keybind> {
    vec![Keybind::new("h", "(h)istory toggle", TOGGLE_HISTORY)]
}

pub fn claude_status_action(action: &str, item: &Value) -> Option<Value> {
    (action == TOGGLE_HISTORY)
        .then(|| jsobj::set_metadata(item, "history", if history_enabled(item) { "false" } else { "true" }))
}
