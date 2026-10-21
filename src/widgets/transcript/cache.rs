//! Cache Hit Rate / Cache Read / Cache Write (upstream `CacheHitRate.ts`,
//! `CacheRead.ts`, `CacheWrite.ts`, `widgets/shared/cache-{metrics,scope}.ts`)
//! and Cache Timer (`CacheTimer.ts`).

use crate::context::RenderContext;
use crate::jsnum::{parse_int, to_js_string};
use crate::settings::{NumberFormat, WidgetItem};
use crate::status::CurrentUsage;
use crate::transcript::tail::{timer_state, TimerState};
use crate::widgets::shared::{
    format_percent, format_tokens, is_flag, is_hidden, parse_usage_tokens, raw_or_labeled, resolve_number_format,
};
use crate::widgets::Widget;

struct CacheTokens {
    read: f64,
    creation: f64,
    input: f64,
}

/// `getCacheTokens(context, sessionScope)`
fn cache_tokens(item: &WidgetItem, ctx: &RenderContext) -> Option<CacheTokens> {
    if is_flag(item, "cacheScopeSession") {
        let m = crate::transcript::token_metrics(ctx)?;
        return Some(CacheTokens {
            read: m.cache_read_tokens,
            creation: m.cache_creation_tokens,
            input: m.input_tokens,
        });
    }
    match ctx.data.context_window.as_ref()?.current_usage.as_ref()? {
        CurrentUsage::Obj(o) => {
            let t = parse_usage_tokens(o);
            Some(CacheTokens { read: t.read, creation: t.creation, input: t.input })
        }
        CurrentUsage::Num(_) => None,
    }
}

fn hit_rate(t: &CacheTokens) -> Option<f64> {
    let d = t.read + t.creation;
    (d > 0.0).then(|| (t.read / d) * 100.0)
}

fn share(part: f64, t: &CacheTokens) -> Option<f64> {
    let d = t.input + t.read + t.creation;
    (d > 0.0).then(|| (part / d) * 100.0)
}

/// `formatTokensWithPercentage`
fn tokens_with_percentage(count: f64, pct: Option<f64>, tf: &NumberFormat, pf: &NumberFormat) -> String {
    let tokens = format_tokens(count, tf, 1);
    match pct {
        None => tokens,
        Some(p) => format!("{tokens} ({})", format_percent(p, pf, 1)),
    }
}

pub struct HitRate;

impl Widget for HitRate {
    fn default_color(&self) -> &'static str {
        "green"
    }
    fn display_name(&self) -> &'static str {
        "Cache Hit Rate"
    }
    fn description(&self) -> &'static str {
        "Shows prompt cache hit rate (cache reads vs cache writes)"
    }
    fn category(&self) -> &'static str {
        "Cache"
    }
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        const LABEL: &str = "Cache Hit: ";
        let format = resolve_number_format("percent", item, ctx.settings);
        if ctx.is_preview {
            return Some(raw_or_labeled(item, LABEL, &format_percent(87.0, &format, 1)));
        }
        let hide = is_hidden(item, "empty", false);
        let Some(tokens) = cache_tokens(item, ctx) else {
            return (!hide).then(|| raw_or_labeled(item, LABEL, "n/a"));
        };
        let Some(rate) = hit_rate(&tokens) else {
            return (!hide).then(|| raw_or_labeled(item, LABEL, &format_percent(0.0, &format, 1)));
        };
        if rate == 0.0 && hide {
            return None;
        }
        Some(raw_or_labeled(item, LABEL, &format_percent(rate, &format, 1)))
    }
}

pub struct ReadWrite {
    write: bool,
}

impl Widget for ReadWrite {
    fn default_color(&self) -> &'static str {
        if self.write {
            "yellow"
        } else {
            "green"
        }
    }
    fn display_name(&self) -> &'static str {
        if self.write {
            "Cache Write"
        } else {
            "Cache Read"
        }
    }
    fn description(&self) -> &'static str {
        if self.write {
            "Shows cache write tokens written to cache, with context share"
        } else {
            "Shows cache read tokens served from cache, with context share"
        }
    }
    fn category(&self) -> &'static str {
        "Cache"
    }
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        let label = if self.write { "Cache Write: " } else { "Cache Read: " };
        let tf = resolve_number_format("token", item, ctx.settings);
        let pf = resolve_number_format("percent", item, ctx.settings);
        if ctx.is_preview {
            let v = if self.write {
                tokens_with_percentage(3000.0, Some(16.0), &tf, &pf)
            } else {
                tokens_with_percentage(12000.0, Some(64.0), &tf, &pf)
            };
            return Some(raw_or_labeled(item, label, &v));
        }
        let hide = is_hidden(item, "empty", false);
        let Some(tokens) = cache_tokens(item, ctx) else {
            return (!hide).then(|| raw_or_labeled(item, label, "n/a"));
        };
        let part = if self.write { tokens.creation } else { tokens.read };
        if part == 0.0 && hide {
            return None;
        }
        let v = tokens_with_percentage(part, share(part, &tokens), &tf, &pf);
        Some(raw_or_labeled(item, label, &v))
    }
}

pub static HIT_RATE: HitRate = HitRate;
pub static READ: ReadWrite = ReadWrite { write: false };
pub static WRITE: ReadWrite = ReadWrite { write: true };

// ---------------------------------------------------------------- timer

const SAFETY_MARGIN: f64 = 5.0;
const DEFAULT_TTL_SECONDS: f64 = 300.0;

fn slot<'a>(item: &'a WidgetItem, id: &str, default: &'a str) -> &'a str {
    item.meta(id).unwrap_or(default)
}

/// `getTtlSeconds`
fn ttl_seconds(item: &WidgetItem) -> f64 {
    let Some(raw) = item.meta("ttlSeconds") else { return DEFAULT_TTL_SECONDS };
    match parse_int(raw) {
        Some(p) if p.is_finite() && p > SAFETY_MARGIN => p,
        _ => DEFAULT_TTL_SECONDS,
    }
}

fn with_glyph(symbol: &str, text: &str) -> String {
    if symbol.is_empty() {
        text.to_string()
    } else {
        format!("{symbol} {text}")
    }
}

fn countdown(remaining: f64) -> String {
    if remaining <= 0.0 {
        return "COLD".into();
    }
    let m = (remaining / 60.0).floor();
    let s = (remaining % 60.0).floor();
    format!("{}:{:0>2}", to_js_string(m), to_js_string(s))
}

pub struct Timer;

impl Widget for Timer {
    fn default_color(&self) -> &'static str {
        "brightCyan"
    }
    fn display_name(&self) -> &'static str {
        "Cache Timer"
    }
    fn description(&self) -> &'static str {
        "Shows time remaining on the prompt cache TTL (5m by default, 1h configurable)"
    }
    fn category(&self) -> &'static str {
        "Session"
    }
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        const LABEL: &str = "Cache: ";
        let hide = is_hidden(item, "empty", false);
        if ctx.is_preview {
            return Some(raw_or_labeled(item, LABEL, &with_glyph(slot(item, "symbolFresh", "🟢"), "4:52")));
        }
        let Some(path) = ctx.data.transcript_path.as_deref().filter(|p| !p.is_empty()) else {
            return (!hide).then(|| raw_or_labeled(item, LABEL, "n/a"));
        };
        let last = match timer_state(path) {
            TimerState::Working => {
                return Some(raw_or_labeled(item, LABEL, &with_glyph(slot(item, "symbolHot", "🔥"), "HOT")));
            }
            TimerState::Idle(None) => return (!hide).then(|| raw_or_labeled(item, LABEL, "n/a")),
            TimerState::Idle(Some(ms)) => ms,
        };
        let ttl = ttl_seconds(item);
        let remaining = ttl - SAFETY_MARGIN - (crate::clock::now_ms() - last) / 1000.0;
        let glyph = if remaining <= 0.0 {
            slot(item, "symbolCold", "❄️")
        } else {
            let pct = remaining / (ttl - SAFETY_MARGIN);
            if pct > 0.5 {
                slot(item, "symbolFresh", "🟢")
            } else if pct > 0.2 {
                slot(item, "symbolDraining", "🟡")
            } else {
                slot(item, "symbolUrgent", "🔴")
            }
        };
        Some(raw_or_labeled(item, LABEL, &with_glyph(glyph, &countdown(remaining))))
    }
}

pub static TIMER: Timer = Timer;
