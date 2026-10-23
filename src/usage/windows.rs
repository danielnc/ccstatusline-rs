//! Usage windows and their formatting (upstream `src/utils/usage-windows.ts`).

use super::{tz, UsageData, FIVE_HOUR_BLOCK_MS, SEVEN_DAY_WINDOW_MS};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Window {
    pub duration_ms: f64,
    pub elapsed_ms: f64,
    pub remaining_ms: f64,
    pub elapsed_percent: f64,
    pub remaining_percent: f64,
}

fn build(reset_at_ms: f64, now_ms: f64, duration_ms: f64) -> Option<Window> {
    if !reset_at_ms.is_finite() || !now_ms.is_finite() || !duration_ms.is_finite() || duration_ms <= 0.0 {
        return None;
    }
    let start = reset_at_ms - duration_ms;
    let elapsed = (now_ms - start).min(duration_ms).max(0.0);
    let elapsed_percent = (elapsed / duration_ms) * 100.0;
    Some(Window {
        duration_ms,
        elapsed_ms: elapsed,
        remaining_ms: duration_ms - elapsed,
        elapsed_percent,
        remaining_percent: 100.0 - elapsed_percent,
    })
}

/// `Date.parse` of a reset stamp; None for NaN or an empty string.
pub fn parse_reset_at(s: Option<&str>) -> Option<f64> {
    s.filter(|s| !s.is_empty()).and_then(crate::timefmt::parse_iso)
}

/// `getUsageWindowFromResetAt`
pub fn from_reset_at(reset_at: Option<&str>, now_ms: f64) -> Option<Window> {
    build(parse_reset_at(reset_at)?, now_ms, FIVE_HOUR_BLOCK_MS)
}

/// `getWeeklyUsageWindowFromResetAt`
pub fn weekly_from_reset_at(reset_at: Option<&str>, now_ms: f64) -> Option<Window> {
    build(parse_reset_at(reset_at)?, now_ms, SEVEN_DAY_WINDOW_MS)
}

/// `resolveUsageWindowWithFallback(usageData, undefined)`: the reset stamp,
/// else the transcript block scan.
pub fn session_window(d: &UsageData, now_ms: f64) -> Option<Window> {
    if let Some(w) = from_reset_at(d.session_reset_at.as_deref(), now_ms) {
        return Some(w);
    }
    let start = crate::transcript::blocks::block_metrics()?.start_ms;
    build(start + FIVE_HOUR_BLOCK_MS, now_ms, FIVE_HOUR_BLOCK_MS)
}

pub fn weekly_window(d: &UsageData, now_ms: f64) -> Option<Window> {
    weekly_from_reset_at(d.weekly_reset_at.as_deref(), now_ms)
}

/// `resolveWeekly{Sonnet,Opus}UsageWindow` / `resolveFableUsageWindow`:
/// `specific ?? weeklyResetAt`.
pub fn model_window(specific: Option<&str>, d: &UsageData, now_ms: f64) -> Option<Window> {
    weekly_from_reset_at(specific.or(d.weekly_reset_at.as_deref()), now_ms)
}

/// `formatUsageDuration(ms, compact, useDays)`
pub fn format_duration(duration_ms: f64, compact: bool, use_days: bool) -> String {
    let ms = duration_ms.max(0.0);
    let total_hours = (ms / 3_600_000.0).floor();
    let m = ((ms % 3_600_000.0) / 60_000.0).floor();
    let h_label = if compact { "h" } else { "hr" };
    let sep = if compact { "" } else { " " };
    let d = if use_days { (total_hours / 24.0).floor() } else { 0.0 };
    let h = if use_days { total_hours % 24.0 } else { total_hours };
    let mut parts = Vec::new();
    if d > 0.0 {
        parts.push(format!("{}d", crate::jsnum::to_js_string(d)));
    }
    if h > 0.0 {
        parts.push(format!("{}{h_label}", crate::jsnum::to_js_string(h)));
    }
    if m > 0.0 {
        parts.push(format!("{}m", crate::jsnum::to_js_string(m)));
    }
    if parts.is_empty() {
        "0m".to_string()
    } else {
        parts.join(sep)
    }
}

const UTC_WEEKDAY_NAMES: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

fn format_utc(ms: f64, compact: bool, hour12: bool, weekday: bool) -> String {
    let c = crate::timefmt::utc(ms);
    let year = c.year;
    let month = format!("{:02}", c.month);
    let day = format!("{:02}", c.day);
    let hours = format!("{:02}", c.hour);
    let minutes = format!("{:02}", c.minute);
    let wd = UTC_WEEKDAY_NAMES[c.weekday as usize];
    if hour12 {
        let display = if c.hour.is_multiple_of(12) { 12 } else { c.hour % 12 };
        let period = if c.hour >= 12 { "PM" } else { "AM" };
        if weekday {
            return if compact {
                format!("{wd} {display}:{minutes} {period}Z")
            } else {
                format!("{wd} {display}:{minutes} {period} UTC")
            };
        }
        return if compact {
            format!("{month}-{day} {display}:{minutes} {period}Z")
        } else {
            format!("{year}-{month}-{day} {display}:{minutes} {period} UTC")
        };
    }
    if weekday {
        return if compact { format!("{wd} {hours}:{minutes}Z") } else { format!("{wd} {hours}:{minutes} UTC") };
    }
    if compact {
        format!("{month}-{day} {hours}:{minutes}Z")
    } else {
        format!("{year}-{month}-{day} {hours}:{minutes} UTC")
    }
}

/// `formatResetAtInTimezone` for en-US (see `tz.rs` for what is covered).
fn format_in_zone(ms: f64, compact: bool, zone: Option<&str>, hour12: bool, weekday: bool) -> Option<String> {
    let p = tz::date_parts(ms, zone)?;
    let (hour, day_period) = if hour12 {
        let h = if p.hour % 12 == 0 { 12 } else { p.hour % 12 };
        // the 2-digit hour with its leading zero stripped
        (format!("{h}"), Some(if p.hour >= 12 { "PM" } else { "AM" }))
    } else {
        (format!("{:02}", p.hour), None)
    };
    let minute = format!("{:02}", p.minute);
    let time = match day_period {
        Some(dp) => format!("{hour}:{minute} {dp}"),
        None => format!("{hour}:{minute}"),
    };
    if weekday {
        let wd = UTC_WEEKDAY_NAMES[p.weekday as usize];
        return Some(if compact { format!("{wd} {time}") } else { format!("{wd} {time} {}", p.zone_name) });
    }
    let (y, mo, d) = (p.year, format!("{:02}", p.month), format!("{:02}", p.day));
    Some(if compact { format!("{mo}-{d} {time}") } else { format!("{y}-{mo}-{d} {time} {}", p.zone_name) })
}

/// `formatUsageResetAt(resetAt, compact, timezone, locale, hour12, weekday)`
pub fn format_reset_at(
    reset_at: Option<&str>,
    compact: bool,
    timezone: Option<&str>,
    _locale: Option<&str>,
    hour12: bool,
    weekday: bool,
) -> Option<String> {
    let ms = parse_reset_at(reset_at)?;
    // `new Date(ms)` past ±8.64e15 is an Invalid Date; upstream's formatters
    // then print NaN fields. Not reachable from real stamps.
    match timezone {
        None | Some("UTC") => return Some(format_utc(ms, compact, hour12, weekday)),
        _ => {}
    }
    let zone = match timezone {
        Some("local") => None,
        other => other,
    };
    format_in_zone(ms, compact, zone, hour12, weekday).or_else(|| Some(format_utc(ms, compact, hour12, weekday)))
}

/// `makeUsageProgressBar(percent, width)`
pub fn usage_progress_bar(percent: f64, width: usize) -> String {
    let filled = crate::jsnum::js_round((percent / 100.0) * width as f64);
    let filled = filled.max(0.0) as usize;
    let empty = width.saturating_sub(filled);
    // `'█'.repeat(n)` throws for a negative count; a context share past 100 %
    // is clamped by the caller.
    format!("[{}{}]", "█".repeat(filled), "░".repeat(empty))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(format_duration(0.0, false, true), "0m");
        assert_eq!(format_duration(4.5 * 3_600_000.0, false, true), "4hr 30m");
        assert_eq!(format_duration(4.5 * 3_600_000.0, true, true), "4h30m");
        assert_eq!(format_duration(36.5 * 3_600_000.0, false, true), "1d 12hr 30m");
        assert_eq!(format_duration(36.5 * 3_600_000.0, true, false), "36h30m");
    }

    #[test]
    fn utc_formats() {
        let s = Some("2026-03-12T08:30:00.000Z");
        assert_eq!(format_reset_at(s, false, None, None, false, false).unwrap(), "2026-03-12 08:30 UTC");
        assert_eq!(format_reset_at(s, true, None, None, true, false).unwrap(), "03-12 8:30 AMZ");
        assert_eq!(format_reset_at(s, false, Some("UTC"), None, true, true).unwrap(), "Thu 8:30 AM UTC");
        assert_eq!(format_reset_at(Some("nope"), false, None, None, false, false), None);
    }

    #[test]
    fn window_math() {
        let w = from_reset_at(Some("2026-10-21T14:00:00Z"), 1792584000000.0).unwrap();
        assert_eq!(w.remaining_ms, 2.0 * 3_600_000.0);
        assert_eq!(w.elapsed_percent, 60.0);
    }
}
