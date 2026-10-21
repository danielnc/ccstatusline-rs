//! Session Clock, Session Name, Compaction Counter, Thinking Effort and
//! Skills (upstream `SessionClock.ts`, `SessionName.ts`,
//! `CompactionCounter.ts`, `ThinkingEffort.ts`, `Skills.tsx`).

use crate::context::RenderContext;
use crate::jsnum::{parse_int, to_js_string};
use crate::settings::WidgetItem;
use crate::transcript::state::{format_duration_ms, normalize_effort, Compaction, Effort};
use crate::widgets::shared::{
    format_tokens, get_format, is_flag, is_hidden, nerd_font_enabled, raw_or_labeled, resolve_number_format,
};
use crate::widgets::Widget;

// ------------------------------------------------------------ session clock

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
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        const LABEL: &str = "Session: ";
        if ctx.is_preview {
            return Some(raw_or_labeled(item, LABEL, "2hr 15m"));
        }
        let hide_zero = is_hidden(item, "zero", false);
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
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        const LABEL: &str = "Session: ";
        if ctx.is_preview {
            return Some(raw_or_labeled(item, LABEL, "my-session"));
        }
        let name = crate::transcript::session_name(ctx)?;
        Some(raw_or_labeled(item, LABEL, &name))
    }
}

// ------------------------------------------------------- compaction counter

const COMPACTION_FORMATS: [&str; 3] = ["icon-space-number", "text-and-number", "number"];
const METRICS: [&str; 5] = ["count", "auto", "manual", "unknown", "reclaimed"];

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
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        let format = resolve_number_format("token", item, ctx.settings);
        let data = if ctx.is_preview {
            Compaction { count: 2.0, auto: 1.0, manual: 1.0, unknown: 0.0, reclaimed: 120000.0 }
        } else {
            crate::transcript::compaction(ctx).unwrap_or_default()
        };
        let metric = item.meta("metric").filter(|m| METRICS.contains(m)).unwrap_or("count");
        let hide_zero = is_hidden(item, "zero", false) && !ctx.is_preview;
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
        let fmt = get_format(item, &COMPACTION_FORMATS, "icon-space-number");
        let icon = if nerd_font_enabled(item, fmt == "icon-space-number") { "\u{F021}" } else { "↻" };
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
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        const LABEL: &str = "Thinking: ";
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

fn list_limit(item: &WidgetItem) -> usize {
    match parse_int(item.meta("listLimit").unwrap_or("0")) {
        Some(p) if p >= 0.0 => p.min(usize::MAX as f64) as usize,
        _ => 0,
    }
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
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        let mode = item.meta("mode").filter(|m| matches!(*m, "current" | "count" | "list")).unwrap_or("current");
        let raw = item.raw();
        let hide = is_hidden(item, "empty", false);
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
