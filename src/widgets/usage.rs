//! Widget group: usage. Subscription usage percentages, reset timers, block
//! timer, extra-usage money widgets, context bar, Claude service status and
//! account email (upstream `src/widgets/{SessionUsage,WeeklyUsage,
//! WeeklySonnetUsage,WeeklyOpusUsage,FableWeeklyUsage,BlockResetTimer,
//! WeeklyResetTimer,BlockTimer,ExtraUsage*,ContextBar,ClaudeStatus,
//! ClaudeAccountEmail}.ts` and `src/widgets/shared/{usage-display,
//! usage-percent-widget,extra-usage-amount-widget,daily-budget}.ts`).
//!
//! The display helpers are public: the clauth widgets (`extra.rs`) render the
//! same displays from clauth's feed.

use super::shared::{
    format_percent, format_tokens, is_flag, is_hidden, raw_or_labeled, resolve_number_format, timer_progress_bar,
};
use super::Widget;
use crate::context::RenderContext;
use crate::settings::{NumberFormat, WidgetItem};
use crate::usage::currency::format_usage_currency;
use crate::usage::service_status::{self, Impact};
use crate::usage::windows::{self, Window};
use crate::usage::UsageData;

// ------------------------------------------------------------ usage-display

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DisplayMode {
    Time,
    Progress,
    ProgressShort,
    Slider,
    SliderOnly,
}

/// `getUsageDisplayMode`
pub fn display_mode(item: &WidgetItem) -> DisplayMode {
    match item.meta("display") {
        Some("progress") => DisplayMode::Progress,
        Some("progress-short") => DisplayMode::ProgressShort,
        Some("slider") => DisplayMode::Slider,
        Some("slider-only") => DisplayMode::SliderOnly,
        _ => DisplayMode::Time,
    }
}

pub fn is_progress(m: DisplayMode) -> bool {
    matches!(m, DisplayMode::Progress | DisplayMode::ProgressShort)
}

pub fn is_slider(m: DisplayMode) -> bool {
    matches!(m, DisplayMode::Slider | DisplayMode::SliderOnly)
}

pub fn progress_width(m: DisplayMode) -> usize {
    if m == DisplayMode::Progress {
        32
    } else {
        16
    }
}

/// `makeSliderBar(percent, 10, { cursorPercent })`
pub fn slider_bar(percent: f64, cursor: Option<f64>) -> String {
    const WIDTH: usize = 10;
    let clamped = percent.clamp(0.0, 100.0);
    let filled = crate::jsnum::js_round((clamped / 100.0) * WIDTH as f64) as isize;
    let cursor_pos = cursor
        .map(|c| (((c.clamp(0.0, 100.0)) / 100.0 * WIDTH as f64).floor() as isize).min(WIDTH as isize - 1))
        .unwrap_or(-1);
    (0..WIDTH as isize)
        .map(|i| {
            if i == cursor_pos {
                '│'
            } else if i < filled {
                '▓'
            } else {
                '░'
            }
        })
        .collect()
}

fn usage_timezone(item: &WidgetItem) -> Option<&str> {
    item.meta("timezone").filter(|s| !s.is_empty())
}

fn usage_locale(item: &WidgetItem) -> Option<&str> {
    item.meta("locale").filter(|s| !s.is_empty())
}

/// Percent displays: `[bar] 42.5%`, `▓▓▓░ 42.5%`, `▓▓▓░` or `42.5%`.
fn bar_display(mode: DisplayMode, percent: f64, f: &NumberFormat, cursor: Option<f64>) -> String {
    if is_progress(mode) {
        return format!(
            "[{}] {}",
            timer_progress_bar(percent, progress_width(mode), cursor),
            format_percent(percent, f, 1)
        );
    }
    let slider = slider_bar(percent, cursor);
    if mode == DisplayMode::Slider {
        format!("{slider} {}", format_percent(percent, f, 1))
    } else {
        slider
    }
}

/// `renderUsageDisplay`
fn render_usage_display(
    item: &WidgetItem,
    label: &str,
    percent: f64,
    f: &NumberFormat,
    cursor: impl FnOnce() -> Option<f64>,
) -> String {
    let mode = display_mode(item);
    if is_progress(mode) || is_slider(mode) {
        return raw_or_labeled(item, label, &bar_display(mode, percent, f, cursor()));
    }
    raw_or_labeled(item, label, &format_percent(percent, f, 1))
}

// ------------------------------------------------------------ percent widgets

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PercentKind {
    Session,
    Weekly,
    WeeklySonnet,
    WeeklyOpus,
    FableWeekly,
}

impl PercentKind {
    pub fn label(self) -> &'static str {
        match self {
            PercentKind::Session => "Session: ",
            PercentKind::Weekly => "Weekly: ",
            PercentKind::WeeklySonnet => "Weekly Sonnet: ",
            PercentKind::WeeklyOpus => "Weekly Opus: ",
            PercentKind::FableWeekly => "Weekly Fable: ",
        }
    }
    fn display_name(self) -> &'static str {
        match self {
            PercentKind::Session => "Session Usage",
            PercentKind::Weekly => "Weekly Usage",
            PercentKind::WeeklySonnet => "Weekly Sonnet Usage",
            PercentKind::WeeklyOpus => "Weekly Opus Usage",
            PercentKind::FableWeekly => "Weekly Fable Usage",
        }
    }
    fn description(self) -> &'static str {
        match self {
            PercentKind::Session => "Shows daily/session API usage percentage",
            PercentKind::Weekly => "Shows weekly API usage percentage",
            PercentKind::WeeklySonnet => "Shows weekly Sonnet API usage percentage",
            PercentKind::WeeklyOpus => "Shows weekly Opus API usage percentage",
            PercentKind::FableWeekly => "Shows Fable-only weekly usage percentage",
        }
    }
    fn preview_percent(self) -> f64 {
        match self {
            PercentKind::Session => 20.0,
            PercentKind::Weekly => 12.0,
            PercentKind::WeeklySonnet => 8.0,
            PercentKind::WeeklyOpus | PercentKind::FableWeekly => 4.0,
        }
    }
    pub fn value(self, d: &UsageData) -> Option<f64> {
        match self {
            PercentKind::Session => d.session_usage,
            PercentKind::Weekly => d.weekly_usage,
            PercentKind::WeeklySonnet => d.weekly_sonnet_usage,
            PercentKind::WeeklyOpus => d.weekly_opus_usage,
            PercentKind::FableWeekly => d.fable_usage,
        }
    }
    /// `resolveUsageWindow(kind, data, context)`; `block_fallback` adds the
    /// transcript block scan for the session window.
    pub fn window(self, d: &UsageData, now: f64, block_fallback: bool) -> Option<Window> {
        match self {
            PercentKind::Session if block_fallback => windows::session_window(d, now),
            PercentKind::Session => windows::from_reset_at(d.session_reset_at.as_deref(), now),
            PercentKind::Weekly => windows::weekly_window(d, now),
            PercentKind::WeeklySonnet => windows::model_window(d.weekly_sonnet_reset_at.as_deref(), d, now),
            PercentKind::WeeklyOpus => windows::model_window(d.weekly_opus_reset_at.as_deref(), d, now),
            PercentKind::FableWeekly => windows::model_window(d.fable_reset_at.as_deref(), d, now),
        }
    }
}

/// `renderUsagePercentWidgetValue` over an explicit usage record.
pub fn render_percent(
    kind: PercentKind,
    item: &WidgetItem,
    ctx: &RenderContext,
    data: Option<&UsageData>,
    block_fallback: bool,
) -> Option<String> {
    let inverted = is_flag(item, "invert");
    let show_cursor = is_flag(item, "cursor");
    let f = resolve_number_format("percent", item, ctx.settings);
    if ctx.is_preview {
        let p = kind.preview_percent();
        let p = if inverted { 100.0 - p } else { p };
        return Some(render_usage_display(item, kind.label(), p, &f, || show_cursor.then_some(50.0)));
    }
    let empty = UsageData::default();
    let data = data.unwrap_or(&empty);
    let Some(value) = kind.value(data) else {
        return match data.error {
            Some(e) if !is_hidden(item, "no-data", false) => Some(e.message().to_string()),
            _ => None,
        };
    };
    let percent = value.clamp(0.0, 100.0);
    let rendered = if inverted { 100.0 - percent } else { percent };
    Some(render_usage_display(item, kind.label(), rendered, &f, || {
        if !show_cursor {
            return None;
        }
        kind.window(data, crate::clock::now_ms(), block_fallback).map(|w| w.elapsed_percent)
    }))
}

pub struct UsagePercentWidget(PercentKind);

impl Widget for UsagePercentWidget {
    fn default_color(&self) -> &'static str {
        "brightBlue"
    }
    fn display_name(&self) -> &'static str {
        self.0.display_name()
    }
    fn description(&self) -> &'static str {
        self.0.description()
    }
    fn category(&self) -> &'static str {
        "Usage"
    }
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        let data = if ctx.is_preview { None } else { crate::usage::usage_data(ctx) };
        render_percent(self.0, item, ctx, data, true)
    }
}

// ------------------------------------------------------------ timers

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TimerKind {
    /// reset-timer (BlockResetTimer)
    BlockReset,
    WeeklyReset,
    /// block-timer: elapsed time in the 5h block
    Block,
}

impl TimerKind {
    pub fn label(self, item: &WidgetItem) -> &'static str {
        let bar = {
            let m = display_mode(item);
            is_progress(m) || is_slider(m)
        };
        match (self, bar) {
            (TimerKind::BlockReset, false) => "Reset: ",
            (TimerKind::BlockReset, true) => "Reset ",
            (TimerKind::WeeklyReset, false) => "Weekly Reset: ",
            (TimerKind::WeeklyReset, true) => "Weekly Reset ",
            (TimerKind::Block, false) => "Block: ",
            (TimerKind::Block, true) => "Block ",
        }
    }
}

fn timer_preview(kind: TimerKind, item: &WidgetItem, f: &NumberFormat) -> String {
    let mode = display_mode(item);
    let inverted = is_flag(item, "invert");
    let compact = is_flag(item, "compact");
    let label = kind.label(item);
    let preview_percent = match (kind, inverted) {
        (TimerKind::Block, true) => 26.1,
        (TimerKind::Block, false) => 73.9,
        (_, true) => 90.0,
        (_, false) => 10.0,
    };
    if is_progress(mode) || is_slider(mode) {
        return raw_or_labeled(item, label, &bar_display(mode, preview_percent, f, None));
    }
    let hour12 = is_flag(item, "hour12");
    match kind {
        TimerKind::Block => raw_or_labeled(item, label, if compact { "3h45m" } else { "3hr 45m" }),
        TimerKind::BlockReset => {
            if is_flag(item, "absolute") {
                let r = windows::format_reset_at(
                    Some("2026-03-12T08:30:00.000Z"),
                    compact,
                    usage_timezone(item),
                    usage_locale(item),
                    hour12,
                    false,
                );
                let fb = if compact { "03-12 08:30Z" } else { "2026-03-12 08:30 UTC" };
                return raw_or_labeled(item, label, r.as_deref().unwrap_or(fb));
            }
            raw_or_labeled(item, label, if compact { "4h30m" } else { "4hr 30m" })
        }
        TimerKind::WeeklyReset => {
            if is_flag(item, "absolute") {
                let weekday = is_flag(item, "weekday");
                let r = windows::format_reset_at(
                    Some("2026-03-15T08:30:00.000Z"),
                    compact,
                    usage_timezone(item),
                    usage_locale(item),
                    hour12,
                    weekday,
                );
                let fb = match (weekday, compact) {
                    (true, true) => "Sun 08:30Z",
                    (true, false) => "Sun 08:30 UTC",
                    (false, true) => "03-15 08:30Z",
                    (false, false) => "2026-03-15 08:30 UTC",
                };
                return raw_or_labeled(item, label, r.as_deref().unwrap_or(fb));
            }
            let use_days = !is_flag(item, "hours");
            raw_or_labeled(item, label, &windows::format_duration(36.5 * 3_600_000.0, compact, use_days))
        }
    }
}

/// What a timer shows when there is no window.
pub enum NoWindow {
    /// Upstream: `[Loading]` / the fetch error / the block timer's zero display.
    Upstream,
    /// Render nothing (the clauth widgets).
    Nothing,
}

/// The reset timers and the block timer over an explicit usage record.
pub fn render_timer(
    kind: TimerKind,
    item: &WidgetItem,
    ctx: &RenderContext,
    data: Option<&UsageData>,
    block_fallback: bool,
    no_window: NoWindow,
) -> Option<String> {
    let f = resolve_number_format("percent", item, ctx.settings);
    if ctx.is_preview {
        return Some(timer_preview(kind, item, &f));
    }
    let mode = display_mode(item);
    let inverted = is_flag(item, "invert");
    let compact = is_flag(item, "compact");
    let label = kind.label(item);
    let empty = UsageData::default();
    let data = data.unwrap_or(&empty);
    let now = crate::clock::now_ms();
    let window = match kind {
        TimerKind::WeeklyReset => windows::weekly_window(data, now),
        _ if block_fallback => windows::session_window(data, now),
        _ => windows::from_reset_at(data.session_reset_at.as_deref(), now),
    };
    let Some(w) = window else {
        if matches!(no_window, NoWindow::Nothing) || is_hidden(item, "no-data", false) {
            return None;
        }
        if kind == TimerKind::Block {
            let empty_percent = format_percent(0.0, &f, 1);
            if is_progress(mode) {
                let bar = "░".repeat(progress_width(mode));
                return Some(raw_or_labeled(item, label, &format!("[{bar}] {empty_percent}")));
            }
            if is_slider(mode) {
                let slider = slider_bar(0.0, None);
                let text = if mode == DisplayMode::Slider { format!("{slider} {empty_percent}") } else { slider };
                return Some(raw_or_labeled(item, label, &text));
            }
            return Some(raw_or_labeled(item, label, if compact { "0h" } else { "0hr 0m" }));
        }
        if let Some(e) = data.error {
            return Some(e.message().to_string());
        }
        return Some(raw_or_labeled(item, label, "[Loading]"));
    };
    if is_progress(mode) || is_slider(mode) {
        let percent = if inverted { w.remaining_percent } else { w.elapsed_percent };
        return Some(raw_or_labeled(item, label, &bar_display(mode, percent, &f, None)));
    }
    match kind {
        TimerKind::Block => Some(raw_or_labeled(item, label, &windows::format_duration(w.elapsed_ms, compact, true))),
        TimerKind::BlockReset | TimerKind::WeeklyReset => {
            let weekly = kind == TimerKind::WeeklyReset;
            if is_flag(item, "absolute") {
                let reset_at = if weekly { data.weekly_reset_at.as_deref() } else { data.session_reset_at.as_deref() };
                let weekday = weekly && is_flag(item, "weekday");
                if let Some(r) = windows::format_reset_at(
                    reset_at,
                    compact,
                    usage_timezone(item),
                    usage_locale(item),
                    is_flag(item, "hour12"),
                    weekday,
                ) {
                    return Some(raw_or_labeled(item, label, &r));
                }
            }
            let use_days = !(weekly && is_flag(item, "hours"));
            Some(raw_or_labeled(item, label, &windows::format_duration(w.remaining_ms, compact, use_days)))
        }
    }
}

pub struct TimerWidget(TimerKind);

impl Widget for TimerWidget {
    fn default_color(&self) -> &'static str {
        match self.0 {
            TimerKind::Block => "yellow",
            _ => "brightBlue",
        }
    }
    fn display_name(&self) -> &'static str {
        match self.0 {
            TimerKind::BlockReset => "Block Reset Timer",
            TimerKind::WeeklyReset => "Weekly Reset Timer",
            TimerKind::Block => "Block Timer",
        }
    }
    fn description(&self) -> &'static str {
        match self.0 {
            TimerKind::BlockReset => "Shows time remaining until current 5hr block reset window",
            TimerKind::WeeklyReset => "Shows time remaining until weekly usage reset",
            TimerKind::Block => "Shows current 5hr block elapsed time or progress",
        }
    }
    fn category(&self) -> &'static str {
        "Usage"
    }
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        let data = if ctx.is_preview { None } else { crate::usage::usage_data(ctx) };
        render_timer(self.0, item, ctx, data, true, NoWindow::Upstream)
    }
}

// ------------------------------------------------------------ extra usage

fn usage_or_empty(ctx: &RenderContext) -> UsageData {
    crate::usage::usage_data(ctx).cloned().unwrap_or_default()
}

/// The shared head of the extra-usage widgets: `n/a` when disabled, the
/// error placeholder or nothing when the figures are missing.
fn extra_usage_unavailable(item: &WidgetItem, label: &str, d: &UsageData, have: bool) -> Option<Option<String>> {
    if d.extra_usage_enabled == Some(false) {
        return Some(if is_hidden(item, "disabled", false) { None } else { Some(raw_or_labeled(item, label, "n/a")) });
    }
    if d.extra_usage_enabled != Some(true) || !have {
        return Some(match d.error {
            Some(e) if !is_hidden(item, "no-data", false) => Some(e.message().to_string()),
            _ => None,
        });
    }
    None
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AmountKind {
    Used,
    Remaining,
}

pub struct ExtraUsageAmountWidget(AmountKind);

impl Widget for ExtraUsageAmountWidget {
    fn default_color(&self) -> &'static str {
        "green"
    }
    fn display_name(&self) -> &'static str {
        match self.0 {
            AmountKind::Used => "Extra Usage Used",
            AmountKind::Remaining => "Extra Usage Remaining",
        }
    }
    fn description(&self) -> &'static str {
        match self.0 {
            AmountKind::Used => {
                "Shows extra usage spent: overage beyond Pro/Max plan limits, or your spend on Enterprise"
            }
            AmountKind::Remaining => {
                "Shows what's left of your monthly extra usage limit (Pro/Max overage or Enterprise spend)"
            }
        }
    }
    fn category(&self) -> &'static str {
        "Usage"
    }
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        let label = match self.0 {
            AmountKind::Used => "Overage Used: ",
            AmountKind::Remaining => "Overage Left: ",
        };
        let f = resolve_number_format("cost", item, ctx.settings);
        if ctx.is_preview {
            let dollars = if self.0 == AmountKind::Used { 106.0 } else { 3894.0 };
            return Some(raw_or_labeled(item, label, &format_usage_currency(dollars, None, &f)));
        }
        let d = usage_or_empty(ctx);
        let dollars = match self.0 {
            AmountKind::Used => d.extra_usage_used.map(|u| u / 100.0),
            AmountKind::Remaining => match (d.extra_usage_limit, d.extra_usage_used) {
                (Some(l), Some(u)) => Some((l / 100.0 - u / 100.0).max(0.0)),
                _ => None,
            },
        };
        if let Some(out) = extra_usage_unavailable(item, label, &d, dollars.is_some()) {
            return out;
        }
        let text = format_usage_currency(dollars?, d.extra_usage_currency.as_deref(), &f);
        Some(raw_or_labeled(item, label, &text))
    }
}

pub struct ExtraUsageUtilizationWidget;

impl Widget for ExtraUsageUtilizationWidget {
    fn default_color(&self) -> &'static str {
        "green"
    }
    fn display_name(&self) -> &'static str {
        "Extra Usage Utilization"
    }
    fn description(&self) -> &'static str {
        "Shows extra usage as a percentage of your monthly limit (Pro/Max overage or Enterprise spend)"
    }
    fn category(&self) -> &'static str {
        "Usage"
    }
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        const LABEL: &str = "Overage: ";
        let mode = display_mode(item);
        let inverted = is_flag(item, "invert");
        let f = resolve_number_format("percent", item, ctx.settings);
        let show = |p: f64| {
            if is_progress(mode) || is_slider(mode) {
                raw_or_labeled(item, LABEL, &bar_display(mode, p, &f, None))
            } else {
                raw_or_labeled(item, LABEL, &format_percent(p, &f, 1))
            }
        };
        if ctx.is_preview {
            return Some(show(if inverted { 100.0 - 2.6 } else { 2.6 }));
        }
        let d = usage_or_empty(ctx);
        let utilization = d.extra_usage_utilization.or(match (d.extra_usage_used, d.extra_usage_limit) {
            (Some(u), Some(l)) if l > 0.0 => Some(u / l * 100.0),
            _ => None,
        });
        if let Some(out) = extra_usage_unavailable(item, LABEL, &d, utilization.is_some()) {
            return out;
        }
        let percent = utilization?.clamp(0.0, 100.0);
        Some(show(if inverted { 100.0 - percent } else { percent }))
    }
}

/// `countBudgetDaysLeft(nowMs, weekdaysOnly)`
pub fn budget_days_left(now_ms: f64, weekdays_only: bool) -> f64 {
    const DAY_MS: f64 = 86_400_000.0;
    let c = crate::timefmt::utc(now_ms);
    let today = crate::timefmt::days_from_civil(c.year, c.month, c.day) as f64 * DAY_MS;
    let next_month = if c.month == 12 {
        crate::timefmt::days_from_civil(c.year + 1, 1, 1)
    } else {
        crate::timefmt::days_from_civil(c.year, c.month + 1, 1)
    } as f64
        * DAY_MS;
    let mut days = 1.0;
    let mut day = today + DAY_MS;
    while day < next_month {
        let wd = crate::timefmt::utc(day).weekday;
        if !weekdays_only || (wd != 0 && wd != 6) {
            days += 1.0;
        }
        day += DAY_MS;
    }
    days
}

pub struct ExtraUsageDailyBudgetWidget;

impl Widget for ExtraUsageDailyBudgetWidget {
    fn default_color(&self) -> &'static str {
        "green"
    }
    fn display_name(&self) -> &'static str {
        "Extra Usage Daily Budget"
    }
    fn description(&self) -> &'static str {
        "Shows what's left of your monthly extra usage limit per day left in the month, optionally counting weekdays only"
    }
    fn category(&self) -> &'static str {
        "Usage"
    }
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        const LABEL: &str = "Daily Budget: ";
        let f = resolve_number_format("cost", item, ctx.settings);
        if ctx.is_preview {
            return Some(raw_or_labeled(item, LABEL, &format_usage_currency(3894.0 / 20.0, None, &f)));
        }
        let d = usage_or_empty(ctx);
        let have = d.extra_usage_limit.is_some() && d.extra_usage_used.is_some();
        if let Some(out) = extra_usage_unavailable(item, LABEL, &d, have) {
            return out;
        }
        let remaining = (d.extra_usage_limit? - d.extra_usage_used?).max(0.0) / 100.0;
        let days = budget_days_left(crate::clock::now_ms(), is_flag(item, "weekdaysOnly"));
        let text = format_usage_currency(remaining / days, d.extra_usage_currency.as_deref(), &f);
        Some(raw_or_labeled(item, LABEL, &text))
    }
}

// ------------------------------------------------------------ context bar

pub struct ContextBarWidget;

impl Widget for ContextBarWidget {
    fn default_color(&self) -> &'static str {
        "blue"
    }
    fn display_name(&self) -> &'static str {
        "Context Bar"
    }
    fn description(&self) -> &'static str {
        "Shows context usage as a progress bar"
    }
    fn category(&self) -> &'static str {
        "Context"
    }
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        const LABEL: &str = "Context: ";
        // progress-short is the default here
        let mode = match item.meta("display") {
            Some("progress") => DisplayMode::Progress,
            Some("slider") => DisplayMode::Slider,
            Some("slider-only") => DisplayMode::SliderOnly,
            _ => DisplayMode::ProgressShort,
        };
        let tf = resolve_number_format("token", item, ctx.settings);
        let pf = resolve_number_format("percent", item, ctx.settings);
        let show = |used: f64, total: f64, percent: f64| {
            let used = format_tokens(used, &tf, 0);
            let total = format_tokens(total, &tf, 0);
            let pd = format_percent(percent, &pf, 0);
            let text = if is_slider(mode) {
                let slider = slider_bar(percent, None);
                if mode == DisplayMode::Slider {
                    format!("{slider} {used}/{total} ({pd})")
                } else {
                    slider
                }
            } else {
                format!("{} {used}/{total} ({pd})", windows::usage_progress_bar(percent, progress_width(mode)))
            };
            raw_or_labeled(item, LABEL, &text)
        };
        if ctx.is_preview {
            return Some(show(50000.0, 200000.0, 25.0));
        }
        let m = super::shared::context_window_metrics(ctx.data);
        let mut total = m.window_size;
        let mut used = m.context_length_tokens;
        let tm = if used.is_none() || total.is_none() { crate::transcript::token_metrics(ctx) } else { None };
        if used.is_none() {
            if let Some(t) = &tm {
                used = Some(t.context_length);
            }
        }
        if total.is_none() && tm.is_some() {
            let id = super::shared::model_context_identifier(ctx.data);
            total = Some(super::shared::context_config(id.as_deref(), None).max_tokens);
        }
        let (used, total) = (used?, total?);
        if total <= 0.0 {
            return None;
        }
        let percent = ((used / total) * 100.0).clamp(0.0, 100.0);
        Some(show(used, total, percent))
    }
}

// ------------------------------------------------------------ claude status

fn status_color_code(key: &str, level: u8) -> String {
    use crate::colors::Level;
    let lvl = Level::from_color_level(level);
    let spec: (&str, &str, &str) = match key {
        "none" => ("\x1b[32m", "ansi256:70", "hex:4e9a06"),
        "minor" => ("\x1b[33m", "ansi256:178", "hex:c4a000"),
        "major" => ("\x1b[31m", "ansi256:208", "hex:ff8700"),
        "critical" => ("\x1b[91m", "ansi256:160", "hex:cc0000"),
        "maintenance" => ("\x1b[36m", "ansi256:30", "hex:06989a"),
        _ => ("\x1b[90m", "ansi256:59", "hex:555753"),
    };
    let s = match lvl {
        Level::Ansi16 => spec.0,
        Level::Ansi256 => spec.1,
        Level::Truecolor => spec.2,
    };
    if s.starts_with('\x1b') {
        s.to_string()
    } else {
        crate::colors::color_ansi_code(Some(s), lvl, false)
    }
}

fn indicator_color_key(indicator: &str) -> &'static str {
    match indicator {
        "none" => "none",
        "minor" => "minor",
        "major" => "major",
        "critical" => "critical",
        "maintenance" => "maintenance",
        _ => "unknown",
    }
}

fn impact_key(i: Impact) -> &'static str {
    match i {
        Impact::None => "none",
        Impact::Minor => "minor",
        Impact::Major => "major",
        Impact::Critical => "critical",
    }
}

pub struct ClaudeStatusWidget;

impl Widget for ClaudeStatusWidget {
    fn default_color(&self) -> &'static str {
        "green"
    }
    fn display_name(&self) -> &'static str {
        "Claude Status"
    }
    fn description(&self) -> &'static str {
        "Shows Claude service status from status.claude.com with an optional 48h incident history strip"
    }
    fn category(&self) -> &'static str {
        "Core"
    }
    fn preserves_rendered_colors(&self, item: &WidgetItem) -> bool {
        service_status::history_enabled(item)
    }
    fn supports_colors(&self, item: &WidgetItem) -> bool {
        !service_status::history_enabled(item)
    }
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        const LABEL: &str = "Claude: ";
        const BAR: &str = "▮";
        let history = service_status::history_enabled(item);
        let level = ctx.settings.color_level;
        let colorize = |text: &str, key: &str| -> String {
            if level == 0 {
                return text.to_string();
            }
            let code = status_color_code(key, level);
            if code.is_empty() {
                text.to_string()
            } else {
                format!("{code}{text}\x1b[39m")
            }
        };
        if ctx.is_preview {
            if !history {
                return Some(raw_or_labeled(item, LABEL, "ok"));
            }
            let keys = ["none", "none", "minor", "none", "major", "none", "critical", "none"];
            let bar: String = keys.iter().map(|k| colorize(BAR, k)).collect();
            return Some(format!("{} {bar}", colorize(&raw_or_labeled(item, LABEL, "ok"), "none")));
        }
        let data = service_status::status_data(&ctx.settings.lines);
        let indicator = data.filter(|d| !d.error).and_then(|d| d.indicator.as_deref());
        let Some(indicator) = indicator else {
            let text = raw_or_labeled(item, LABEL, "?");
            return Some(if history { colorize(&text, "unknown") } else { text });
        };
        let status_text = match indicator {
            "none" => "ok",
            other => other,
        };
        if !history {
            return Some(raw_or_labeled(item, LABEL, status_text));
        }
        let colored = colorize(&raw_or_labeled(item, LABEL, status_text), indicator_color_key(indicator));
        let incidents = data.and_then(|d| d.incidents.clone()).unwrap_or_default();
        let bar: String = service_status::history_buckets(&incidents, crate::clock::now_ms())
            .into_iter()
            .map(|b| colorize(BAR, impact_key(b)))
            .collect();
        Some(format!("{colored} {bar}"))
    }
}

// ------------------------------------------------------------ account email

pub struct ClaudeAccountEmailWidget;

impl Widget for ClaudeAccountEmailWidget {
    fn default_color(&self) -> &'static str {
        "blue"
    }
    fn display_name(&self) -> &'static str {
        "Claude Account Email"
    }
    fn description(&self) -> &'static str {
        "Displays the email of the currently logged-in Claude account"
    }
    fn category(&self) -> &'static str {
        "Session"
    }
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        const LABEL: &str = "Account: ";
        if ctx.is_preview {
            return Some(raw_or_labeled(item, LABEL, "you@example.com"));
        }
        let raw = std::fs::read(crate::claude_config::claude_json_path()).ok()?;
        let v: serde_json::Value = serde_json::from_str(&String::from_utf8_lossy(&raw)).ok()?;
        let email = v.get("oauthAccount")?.get("emailAddress")?.as_str()?;
        if email.is_empty() {
            return None;
        }
        Some(raw_or_labeled(item, LABEL, email))
    }
}

// ------------------------------------------------------------ registry

static SESSION: UsagePercentWidget = UsagePercentWidget(PercentKind::Session);
static WEEKLY: UsagePercentWidget = UsagePercentWidget(PercentKind::Weekly);
static WEEKLY_SONNET: UsagePercentWidget = UsagePercentWidget(PercentKind::WeeklySonnet);
static WEEKLY_OPUS: UsagePercentWidget = UsagePercentWidget(PercentKind::WeeklyOpus);
static FABLE_WEEKLY: UsagePercentWidget = UsagePercentWidget(PercentKind::FableWeekly);
static RESET_TIMER: TimerWidget = TimerWidget(TimerKind::BlockReset);
static WEEKLY_RESET_TIMER: TimerWidget = TimerWidget(TimerKind::WeeklyReset);
static BLOCK_TIMER: TimerWidget = TimerWidget(TimerKind::Block);
static EXTRA_USED: ExtraUsageAmountWidget = ExtraUsageAmountWidget(AmountKind::Used);
static EXTRA_REMAINING: ExtraUsageAmountWidget = ExtraUsageAmountWidget(AmountKind::Remaining);
static EXTRA_UTILIZATION: ExtraUsageUtilizationWidget = ExtraUsageUtilizationWidget;
static EXTRA_DAILY_BUDGET: ExtraUsageDailyBudgetWidget = ExtraUsageDailyBudgetWidget;
static CONTEXT_BAR: ContextBarWidget = ContextBarWidget;
static CLAUDE_STATUS: ClaudeStatusWidget = ClaudeStatusWidget;
static ACCOUNT_EMAIL: ClaudeAccountEmailWidget = ClaudeAccountEmailWidget;

pub fn lookup(kind: &str) -> Option<&'static dyn Widget> {
    Some(match kind {
        "session-usage" => &SESSION,
        "weekly-usage" => &WEEKLY,
        "weekly-sonnet-usage" => &WEEKLY_SONNET,
        "weekly-opus-usage" => &WEEKLY_OPUS,
        "fable-weekly-usage" => &FABLE_WEEKLY,
        "reset-timer" => &RESET_TIMER,
        "weekly-reset-timer" => &WEEKLY_RESET_TIMER,
        "block-timer" => &BLOCK_TIMER,
        "extra-usage-used" => &EXTRA_USED,
        "extra-usage-remaining" => &EXTRA_REMAINING,
        "extra-usage-utilization" => &EXTRA_UTILIZATION,
        "extra-usage-daily-budget" => &EXTRA_DAILY_BUDGET,
        "context-bar" => &CONTEXT_BAR,
        "claude-status" => &CLAUDE_STATUS,
        "claude-account-email" => &ACCOUNT_EMAIL,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slider() {
        assert_eq!(slider_bar(25.0, None), "▓▓▓░░░░░░░");
        assert_eq!(slider_bar(25.0, Some(100.0)), "▓▓▓░░░░░░│");
    }

    #[test]
    fn budget_days() {
        let now = crate::timefmt::parse_iso("2026-10-21T12:00:00Z").unwrap();
        assert_eq!(budget_days_left(now, false), 11.0);
        // Wed 21st: Thu-Fri 22-23, Mon-Fri 26-30 → 7 weekdays + today
        assert_eq!(budget_days_left(now, true), 8.0);
    }
}
