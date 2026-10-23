//! Session Clock, Session Name, Compaction Counter, Thinking Effort and
//! Skills (upstream `SessionClock.ts`, `SessionName.ts`,
//! `CompactionCounter.ts`, `ThinkingEffort.ts`, `Skills.tsx`).

use crate::context::RenderContext;
use crate::jsnum::{parse_int, to_js_string};
use crate::jsobj;
use crate::settings::WidgetItem;
use crate::transcript::state::{format_duration_ms, normalize_effort, Compaction, Effort};
use crate::widgets::editor::{self, FormatOptions};
use crate::widgets::shared::{
    format_tokens, get_format, is_flag, is_hidden, nerd_font_enabled, raw_or_labeled, resolve_number_format,
};
use crate::widgets::{EditorDisplay, HideableState, Keybind, Widget};
use serde_json::Value;

const SESSION_LABEL: &str = "Session: ";

// ------------------------------------------------------------ session clock

const CLOCK_ZERO: HideableState = HideableState { key: "zero", label: "when under 1 minute", default_enabled: false };

pub struct Clock;

impl Widget for Clock {
    fn default_color(&self) -> &'static str {
        "yellow"
    }
    fn display_name(&self) -> &'static str {
        "Session Clock"
    }
    fn description(&self) -> &'static str {
        "Shows elapsed time since current session started"
    }
    fn category(&self) -> &'static str {
        "Session"
    }
    fn label_prefix(&self, _item: &WidgetItem) -> Option<String> {
        Some(SESSION_LABEL.into())
    }
    fn hideable_states(&self) -> &'static [HideableState] {
        &[CLOCK_ZERO]
    }
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        const LABEL: &str = SESSION_LABEL;
        if ctx.is_preview {
            return Some(raw_or_labeled(item, LABEL, "2hr 15m"));
        }
        let hide_zero = is_hidden(item, CLOCK_ZERO.key, false);
        let stdin = ctx.data.cost.as_ref().and_then(|c| c.total_duration_ms).filter(|d| d.is_finite() && *d >= 0.0);
        if let Some(ms) = stdin {
            if ms < 60000.0 && hide_zero {
                return None;
            }
            return Some(raw_or_labeled(item, LABEL, &format_duration_ms(ms)));
        }
        let duration = crate::transcript::session_duration(ctx).unwrap_or_else(|| "0m".into());
        if (duration == "0m" || duration == "<1m") && hide_zero {
            return None;
        }
        Some(raw_or_labeled(item, LABEL, &duration))
    }
}

// ------------------------------------------------------------- session name

pub struct Name;

impl Widget for Name {
    fn default_color(&self) -> &'static str {
        "cyan"
    }
    fn display_name(&self) -> &'static str {
        "Session Name"
    }
    fn description(&self) -> &'static str {
        "Shows the session name set via /rename command in Claude Code"
    }
    fn category(&self) -> &'static str {
        "Session"
    }
    fn label_prefix(&self, _item: &WidgetItem) -> Option<String> {
        Some(SESSION_LABEL.into())
    }
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        const LABEL: &str = SESSION_LABEL;
        if ctx.is_preview {
            return Some(raw_or_labeled(item, LABEL, "my-session"));
        }
        let name = crate::transcript::session_name(ctx)?;
        Some(raw_or_labeled(item, LABEL, &name))
    }
}

// ------------------------------------------------------- compaction counter

const COMPACTION_FORMATS: [&str; 3] = ["icon-space-number", "text-and-number", "number"];
const COMPACTION_DEFAULT_FORMAT: &str = "icon-space-number";
const METRICS: [&str; 5] = ["count", "auto", "manual", "unknown", "reclaimed"];
const CYCLE_METRIC_ACTION: &str = "cycle-metric";
const TOGGLE_TRIGGERS_ACTION: &str = "toggle-triggers";
const TOGGLE_RECLAIMED_ACTION: &str = "toggle-reclaimed";
const COMPACTION_ZERO: HideableState =
    HideableState { key: "zero", label: "when count is zero", default_enabled: false };

/// Only the icon format draws a glyph; the other two are text.
fn compaction_can_use_nerd_font(item: &Value) -> bool {
    editor::get_format(item, &COMPACTION_FORMAT_OPTIONS) == COMPACTION_DEFAULT_FORMAT
}

const COMPACTION_FORMAT_OPTIONS: FormatOptions = FormatOptions {
    formats: &COMPACTION_FORMATS,
    default_format: COMPACTION_DEFAULT_FORMAT,
    can_use_nerd_font: compaction_can_use_nerd_font,
};

fn compaction_metric(metric: Option<&str>) -> &'static str {
    metric.and_then(|m| METRICS.iter().find(|x| **x == m)).copied().unwrap_or("count")
}

/// `setMetric`: the default metric is stored as no key.
fn set_compaction_metric(item: &Value, metric: &str) -> Value {
    if metric == "count" {
        return jsobj::remove_metadata_keys(item, &["metric"]);
    }
    jsobj::set_metadata(item, "metric", metric)
}

pub struct CompactionCounter;

impl Widget for CompactionCounter {
    fn default_color(&self) -> &'static str {
        "yellow"
    }
    fn display_name(&self) -> &'static str {
        "Compaction Counter"
    }
    fn description(&self) -> &'static str {
        "Count of context compaction events in the current session."
    }
    fn category(&self) -> &'static str {
        "Context"
    }
    fn supports_raw_value(&self) -> bool {
        false
    }
    fn supports_number_format(&self) -> bool {
        true
    }
    fn hideable_states(&self) -> &'static [HideableState] {
        &[COMPACTION_ZERO]
    }
    fn editor_display(&self, item: &WidgetItem) -> EditorDisplay {
        let metric = compaction_metric(item.meta("metric"));
        let mut modifiers = Vec::new();
        if metric != "count" {
            modifiers.push(format!("{metric} value"));
        } else {
            let format = get_format(item, &COMPACTION_FORMATS, COMPACTION_DEFAULT_FORMAT);
            modifiers.push(format.to_string());
            if nerd_font_enabled(item, format == COMPACTION_DEFAULT_FORMAT) {
                modifiers.push("nerd font".into());
            }
            if is_flag(item, "showTriggers") {
                modifiers.push("trigger split".into());
            }
            if is_flag(item, "showReclaimed") {
                modifiers.push("reclaimed".into());
            }
        }
        EditorDisplay { text: "Compaction Counter".into(), modifier: Some(format!("({})", modifiers.join(", "))) }
    }
    fn handle_editor_action(&self, action: &str, item: &Value) -> Option<Value> {
        if action == CYCLE_METRIC_ACTION {
            let current = compaction_metric(jsobj::metadata_str(item, "metric"));
            let index = METRICS.iter().position(|m| *m == current).unwrap_or(0);
            return Some(set_compaction_metric(item, METRICS[(index + 1) % METRICS.len()]));
        }
        if let Some(result) = editor::handle_format_action(action, item, &COMPACTION_FORMAT_OPTIONS) {
            return Some(result);
        }
        match action {
            TOGGLE_TRIGGERS_ACTION => Some(jsobj::toggle_metadata_flag(item, "showTriggers")),
            TOGGLE_RECLAIMED_ACTION => Some(jsobj::toggle_metadata_flag(item, "showReclaimed")),
            _ => None,
        }
    }
    fn custom_keybinds(&self, item: &WidgetItem) -> Vec<Keybind> {
        let mut keybinds = vec![Keybind::new("v", "(v)alue", CYCLE_METRIC_ACTION)];
        // The format, glyph and trigger toggles only shape the composite count display
        if compaction_metric(item.meta("metric")) != "count" {
            return keybinds;
        }
        let format = get_format(item, &COMPACTION_FORMATS, COMPACTION_DEFAULT_FORMAT);
        keybinds.extend(editor::format_keybinds(format == COMPACTION_DEFAULT_FORMAT));
        keybinds.extend([
            Keybind::new("s", "(s)plit by trigger", TOGGLE_TRIGGERS_ACTION),
            Keybind::new("t", "(t)okens reclaimed", TOGGLE_RECLAIMED_ACTION),
            editor::symbol_keybind(),
        ]);
        keybinds
    }
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        let format = resolve_number_format("token", item, ctx.settings);
        let data = if ctx.is_preview {
            Compaction { count: 2.0, auto: 1.0, manual: 1.0, unknown: 0.0, reclaimed: 120000.0 }
        } else {
            crate::transcript::compaction(ctx).unwrap_or_default()
        };
        let metric = compaction_metric(item.meta("metric"));
        let hide_zero = is_hidden(item, COMPACTION_ZERO.key, false) && !ctx.is_preview;
        if metric != "count" {
            let value = match metric {
                "auto" => data.auto,
                "manual" => data.manual,
                "unknown" => data.unknown,
                _ => data.reclaimed,
            };
            if value == 0.0 && hide_zero {
                return None;
            }
            return Some(if metric == "reclaimed" { format_tokens(value, &format, 1) } else { to_js_string(value) });
        }
        if data.count == 0.0 && hide_zero {
            return None;
        }
        let fmt = get_format(item, &COMPACTION_FORMATS, COMPACTION_DEFAULT_FORMAT);
        let icon = if nerd_font_enabled(item, fmt == COMPACTION_DEFAULT_FORMAT) { "\u{F021}" } else { "↻" };
        let count = to_js_string(data.count);
        let mut out = match fmt {
            "icon-space-number" => format!("{icon} {count}"),
            "text-and-number" => format!("Compactions: {count}"),
            _ => count,
        };
        if is_flag(item, "showTriggers") {
            let mut parts = Vec::new();
            for (n, what) in [(data.auto, "auto"), (data.manual, "manual"), (data.unknown, "unknown")] {
                if n > 0.0 {
                    parts.push(format!("{} {what}", to_js_string(n)));
                }
            }
            if !parts.is_empty() {
                out += &format!(" ({})", parts.join(", "));
            }
        }
        if is_flag(item, "showReclaimed") && data.reclaimed > 0.0 {
            let symbol = item.meta("symbolReclaimed").unwrap_or("↓");
            out += &format!(" {symbol}{}", format_tokens(data.reclaimed, &format, 1));
        }
        Some(out)
    }
}

// ---------------------------------------------------------- thinking effort

const THINKING_LABEL: &str = "Thinking: ";

pub struct Thinking;

/// `resolveThinkingEffort`; None renders as "default".
fn resolve_effort(ctx: &RenderContext) -> Option<Effort> {
    let d = ctx.data;
    if let (Some(effort), true) = (&d.effort, d.effort_has_level) {
        // an unrecognized level falls through to the other sources
        if let Some(e) = normalize_effort(Some(effort.as_deref()?)) {
            return Some(e);
        }
    }
    if let Some(e) = crate::transcript::thinking_effort(ctx) {
        return Some(e);
    }
    let settings = crate::claude_config::load_settings().ok()?;
    normalize_effort(settings.get("effortLevel").and_then(|v| v.as_str()))
}

impl Widget for Thinking {
    fn default_color(&self) -> &'static str {
        "magenta"
    }
    fn display_name(&self) -> &'static str {
        "Thinking Effort"
    }
    fn description(&self) -> &'static str {
        "Displays the current thinking effort level (low, medium, high, xhigh, max).\nClaude Code reports Ultracode as xhigh in status line data; Ultracode is not exposed as a separate effort level.\nUnknown levels are shown with a trailing \"?\" (e.g. \"super-max?\").\nMay be incorrect when multiple Claude Code sessions are running due to current Claude Code limitations."
    }
    fn category(&self) -> &'static str {
        "Core"
    }
    fn label_prefix(&self, _item: &WidgetItem) -> Option<String> {
        Some(THINKING_LABEL.into())
    }
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        const LABEL: &str = THINKING_LABEL;
        if ctx.is_preview {
            return Some(raw_or_labeled(item, LABEL, "high"));
        }
        let text = match resolve_effort(ctx) {
            None => "default".to_string(),
            Some(e) if e.known => e.value,
            Some(e) => format!("{}?", e.value),
        };
        Some(raw_or_labeled(item, LABEL, &text))
    }
}

// ------------------------------------------------------------------- skills

pub struct Skills;

const SKILLS_MODES: [&str; 3] = ["current", "count", "list"];
const SKILLS_EMPTY: HideableState =
    HideableState { key: "empty", label: "when no skills have been used", default_enabled: false };
const CYCLE_MODE_ACTION: &str = "cycle-mode";

/// `getMode`
fn skills_mode(mode: Option<&str>) -> &'static str {
    mode.and_then(|m| SKILLS_MODES.iter().find(|x| **x == m)).copied().unwrap_or("current")
}

/// `parseListLimit`
fn list_limit_value(item: &WidgetItem) -> f64 {
    match parse_int(item.meta("listLimit").unwrap_or("0")) {
        Some(p) if p >= 0.0 => p,
        _ => 0.0,
    }
}

fn list_limit(item: &WidgetItem) -> usize {
    list_limit_value(item).min(usize::MAX as f64) as usize
}

fn skills_metrics(ctx: &RenderContext) -> &'static crate::transcript::skills::SkillsMetrics {
    use std::sync::OnceLock;
    static METRICS: OnceLock<crate::transcript::skills::SkillsMetrics> = OnceLock::new();
    METRICS.get_or_init(|| match ctx.data.session_id.as_deref().filter(|s| !s.is_empty()) {
        Some(id) => crate::transcript::skills::skills_metrics(id),
        None => Default::default(),
    })
}

impl Widget for Skills {
    fn default_color(&self) -> &'static str {
        "magenta"
    }
    fn display_name(&self) -> &'static str {
        "Skills"
    }
    fn description(&self) -> &'static str {
        "Shows Claude Code skill invocations from hook data"
    }
    fn category(&self) -> &'static str {
        "Session"
    }
    fn hooks(&self) -> &'static [(&'static str, Option<&'static str>)] {
        &[("PreToolUse", Some("Skill")), ("UserPromptSubmit", None)]
    }
    fn custom_keybinds(&self, item: &WidgetItem) -> Vec<Keybind> {
        let mut keybinds = vec![Keybind::new("v", "(v)iew: last/count/list", CYCLE_MODE_ACTION)];
        if skills_mode(item.meta("mode")) == "list" {
            keybinds.push(Keybind::new("l", "(l)imit", "edit-list-limit"));
        }
        keybinds
    }
    fn hideable_states(&self) -> &'static [HideableState] {
        &[SKILLS_EMPTY]
    }
    fn editor_display(&self, item: &WidgetItem) -> EditorDisplay {
        let mode = skills_mode(item.meta("mode"));
        let label = match mode {
            "current" => "last used",
            "count" => "total count",
            _ => "unique list",
        };
        let mut modifiers = vec![label.to_string()];
        if mode == "list" {
            let limit = list_limit_value(item);
            if limit > 0.0 {
                modifiers.push(format!("limit: {}", to_js_string(limit)));
            }
        }
        EditorDisplay { text: "Skills".into(), modifier: editor::make_modifier_text(&modifiers) }
    }
    fn handle_editor_action(&self, action: &str, item: &Value) -> Option<Value> {
        if action != CYCLE_MODE_ACTION {
            return None;
        }
        let current = skills_mode(jsobj::metadata_str(item, "mode"));
        let index = SKILLS_MODES.iter().position(|m| *m == current).unwrap_or(0);
        let next = SKILLS_MODES[(index + 1) % SKILLS_MODES.len()];
        let next_item = if next == "list" { item.clone() } else { jsobj::remove_metadata_keys(item, &["listLimit"]) };
        Some(jsobj::set_metadata(&next_item, "mode", next))
    }
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        let mode = skills_mode(item.meta("mode"));
        let raw = item.raw();
        let hide = is_hidden(item, SKILLS_EMPTY.key, false);
        let pick = |r: &str, l: String| if raw { r.to_string() } else { l };
        if ctx.is_preview {
            return Some(match mode {
                "current" => pick("commit", "Skill: commit".into()),
                "count" => pick("5", "Skills: 5".into()),
                _ => pick("commit, review-pr", "Skills: commit, review-pr".into()),
            });
        }
        let m = skills_metrics(ctx);
        match mode {
            "current" => match m.last_skill.as_deref().filter(|s| !s.is_empty()) {
                None if hide => None,
                None => Some(pick("none", "Skill: none".into())),
                Some(s) => Some(pick(s, format!("Skill: {s}"))),
            },
            "count" => {
                let total = m.total_invocations;
                if hide && total == 0 {
                    return None;
                }
                Some(pick(&total.to_string(), format!("Skills: {total}")))
            }
            _ => {
                if m.unique_skills.is_empty() {
                    return if hide { None } else { Some(pick("none", "Skills: none".into())) };
                }
                let limit = list_limit(item);
                let visible =
                    if limit > 0 { &m.unique_skills[..limit.min(m.unique_skills.len())] } else { &m.unique_skills[..] };
                let list = visible.join(", ");
                Some(pick(&list, format!("Skills: {list}")))
            }
        }
    }
}

pub static CLOCK: Clock = Clock;
pub static NAME: Name = Name;
pub static COMPACTION: CompactionCounter = CompactionCounter;
pub static THINKING: Thinking = Thinking;
pub static SKILLS: Skills = Skills;
