//! Usage data provider: Claude subscription usage (upstream
//! `src/utils/{usage-prefetch,usage-fetch,usage-types,usage-windows}.ts`),
//! the 5h block scan fallback (`jsonl-blocks.ts`, `jsonl-cache.ts`), the
//! status.claude.com feed (`claude-service-status.ts`) and clauth's
//! `~/.clauth/status.json` (the clauth widgets).
//!
//! Nothing here touches the network on the render path. Where upstream awaits
//! an HTTPS request, the render reads the shared cache files and, when they are
//! stale, starts at most one detached refresher (`--usage-refresh`,
//! `--claude-status-refresh`) that fetches with curl and writes the caches
//! atomically. Under the parity harness (`CCSL_TEST_NO_NETWORK=1`) the fetch
//! fails inline exactly like upstream's blocked request.

pub mod clauth;
pub mod currency;
pub mod fetch;
pub mod refresh;
pub mod service_status;
pub mod sha256;
pub mod tz;
pub mod windows;

use crate::context::RenderContext;
use crate::settings::WidgetItem;
use crate::status::{RateLimitPeriod, StatusJson};
use std::sync::OnceLock;

pub use refresh::handle_refresh_cli;

pub const FIVE_HOUR_BLOCK_MS: f64 = 5.0 * 60.0 * 60.0 * 1000.0;
pub const SEVEN_DAY_WINDOW_MS: f64 = 7.0 * 24.0 * 60.0 * 60.0 * 1000.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsageError {
    NoCredentials,
    Timeout,
    RateLimited,
    ApiError,
    ParseError,
}

impl UsageError {
    pub fn parse(s: &str) -> Option<UsageError> {
        Some(match s {
            "no-credentials" => UsageError::NoCredentials,
            "timeout" => UsageError::Timeout,
            "rate-limited" => UsageError::RateLimited,
            "api-error" => UsageError::ApiError,
            "parse-error" => UsageError::ParseError,
            _ => return None,
        })
    }
    pub fn as_str(self) -> &'static str {
        match self {
            UsageError::NoCredentials => "no-credentials",
            UsageError::Timeout => "timeout",
            UsageError::RateLimited => "rate-limited",
            UsageError::ApiError => "api-error",
            UsageError::ParseError => "parse-error",
        }
    }
    /// `getUsageErrorMessage`
    pub fn message(self) -> &'static str {
        match self {
            UsageError::NoCredentials => "[No credentials]",
            UsageError::Timeout => "[Timeout]",
            UsageError::RateLimited => "[Rate limited]",
            UsageError::ApiError => "[API Error]",
            UsageError::ParseError => "[Parse Error]",
        }
    }
}

/// `UsageData` / `RenderUsageData`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UsageData {
    pub session_usage: Option<f64>,
    pub session_reset_at: Option<String>,
    pub weekly_usage: Option<f64>,
    pub weekly_reset_at: Option<String>,
    pub weekly_sonnet_usage: Option<f64>,
    pub weekly_sonnet_reset_at: Option<String>,
    pub weekly_opus_usage: Option<f64>,
    pub weekly_opus_reset_at: Option<String>,
    pub fable_usage: Option<f64>,
    pub fable_reset_at: Option<String>,
    pub extra_usage_enabled: Option<bool>,
    pub extra_usage_limit: Option<f64>,
    pub extra_usage_used: Option<f64>,
    pub extra_usage_utilization: Option<f64>,
    pub extra_usage_currency: Option<String>,
    pub error: Option<UsageError>,
}

/// `UsageDataField`
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Field {
    SessionUsage,
    SessionResetAt,
    WeeklyUsage,
    WeeklyResetAt,
    WeeklySonnetUsage,
    WeeklySonnetResetAt,
    WeeklyOpusUsage,
    WeeklyOpusResetAt,
    FableUsage,
    FableResetAt,
    ExtraUsageEnabled,
    ExtraUsageLimit,
    ExtraUsageUsed,
    ExtraUsageUtilization,
    ExtraUsageCurrency,
}

/// `USAGE_DATA_FIELDS` order.
pub const ALL_FIELDS: [Field; 15] = [
    Field::SessionUsage,
    Field::SessionResetAt,
    Field::WeeklyUsage,
    Field::WeeklyResetAt,
    Field::WeeklySonnetUsage,
    Field::WeeklySonnetResetAt,
    Field::WeeklyOpusUsage,
    Field::WeeklyOpusResetAt,
    Field::FableUsage,
    Field::FableResetAt,
    Field::ExtraUsageEnabled,
    Field::ExtraUsageLimit,
    Field::ExtraUsageUsed,
    Field::ExtraUsageUtilization,
    Field::ExtraUsageCurrency,
];

impl Field {
    pub fn name(self) -> &'static str {
        match self {
            Field::SessionUsage => "sessionUsage",
            Field::SessionResetAt => "sessionResetAt",
            Field::WeeklyUsage => "weeklyUsage",
            Field::WeeklyResetAt => "weeklyResetAt",
            Field::WeeklySonnetUsage => "weeklySonnetUsage",
            Field::WeeklySonnetResetAt => "weeklySonnetResetAt",
            Field::WeeklyOpusUsage => "weeklyOpusUsage",
            Field::WeeklyOpusResetAt => "weeklyOpusResetAt",
            Field::FableUsage => "fableUsage",
            Field::FableResetAt => "fableResetAt",
            Field::ExtraUsageEnabled => "extraUsageEnabled",
            Field::ExtraUsageLimit => "extraUsageLimit",
            Field::ExtraUsageUsed => "extraUsageUsed",
            Field::ExtraUsageUtilization => "extraUsageUtilization",
            Field::ExtraUsageCurrency => "extraUsageCurrency",
        }
    }
    pub fn from_name(s: &str) -> Option<Field> {
        ALL_FIELDS.iter().copied().find(|f| f.name() == s)
    }
}

impl UsageData {
    pub fn has(&self, f: Field) -> bool {
        match f {
            Field::SessionUsage => self.session_usage.is_some(),
            Field::SessionResetAt => self.session_reset_at.is_some(),
            Field::WeeklyUsage => self.weekly_usage.is_some(),
            Field::WeeklyResetAt => self.weekly_reset_at.is_some(),
            Field::WeeklySonnetUsage => self.weekly_sonnet_usage.is_some(),
            Field::WeeklySonnetResetAt => self.weekly_sonnet_reset_at.is_some(),
            Field::WeeklyOpusUsage => self.weekly_opus_usage.is_some(),
            Field::WeeklyOpusResetAt => self.weekly_opus_reset_at.is_some(),
            Field::FableUsage => self.fable_usage.is_some(),
            Field::FableResetAt => self.fable_reset_at.is_some(),
            Field::ExtraUsageEnabled => self.extra_usage_enabled.is_some(),
            Field::ExtraUsageLimit => self.extra_usage_limit.is_some(),
            Field::ExtraUsageUsed => self.extra_usage_used.is_some(),
            Field::ExtraUsageUtilization => self.extra_usage_utilization.is_some(),
            Field::ExtraUsageCurrency => self.extra_usage_currency.is_some(),
        }
    }

    /// Copy field `f` from `src` when it is defined there (`{...a, ...b}`).
    fn take_from(&mut self, src: &UsageData, f: Field) {
        match f {
            Field::SessionUsage => self.session_usage = src.session_usage.or(self.session_usage),
            Field::SessionResetAt => {
                if src.session_reset_at.is_some() {
                    self.session_reset_at = src.session_reset_at.clone()
                }
            }
            Field::WeeklyUsage => self.weekly_usage = src.weekly_usage.or(self.weekly_usage),
            Field::WeeklyResetAt => {
                if src.weekly_reset_at.is_some() {
                    self.weekly_reset_at = src.weekly_reset_at.clone()
                }
            }
            Field::WeeklySonnetUsage => self.weekly_sonnet_usage = src.weekly_sonnet_usage.or(self.weekly_sonnet_usage),
            Field::WeeklySonnetResetAt => {
                if src.weekly_sonnet_reset_at.is_some() {
                    self.weekly_sonnet_reset_at = src.weekly_sonnet_reset_at.clone()
                }
            }
            Field::WeeklyOpusUsage => self.weekly_opus_usage = src.weekly_opus_usage.or(self.weekly_opus_usage),
            Field::WeeklyOpusResetAt => {
                if src.weekly_opus_reset_at.is_some() {
                    self.weekly_opus_reset_at = src.weekly_opus_reset_at.clone()
                }
            }
            Field::FableUsage => self.fable_usage = src.fable_usage.or(self.fable_usage),
            Field::FableResetAt => {
                if src.fable_reset_at.is_some() {
                    self.fable_reset_at = src.fable_reset_at.clone()
                }
            }
            Field::ExtraUsageEnabled => self.extra_usage_enabled = src.extra_usage_enabled.or(self.extra_usage_enabled),
            Field::ExtraUsageLimit => self.extra_usage_limit = src.extra_usage_limit.or(self.extra_usage_limit),
            Field::ExtraUsageUsed => self.extra_usage_used = src.extra_usage_used.or(self.extra_usage_used),
            Field::ExtraUsageUtilization => {
                self.extra_usage_utilization = src.extra_usage_utilization.or(self.extra_usage_utilization)
            }
            Field::ExtraUsageCurrency => {
                if src.extra_usage_currency.is_some() {
                    self.extra_usage_currency = src.extra_usage_currency.clone()
                }
            }
        }
    }
}

/// `WEEKLY_MODEL_USAGE_BUCKETS`
pub struct ModelBucket {
    pub widget_type: &'static str,
    pub model_display_name: &'static str,
    pub api_bucket_key: Option<&'static str>,
    pub usage_field: Field,
    pub reset_field: Field,
}

pub const WEEKLY_MODEL_USAGE_BUCKETS: [ModelBucket; 3] = [
    ModelBucket {
        widget_type: "weekly-sonnet-usage",
        model_display_name: "Sonnet",
        api_bucket_key: Some("seven_day_sonnet"),
        usage_field: Field::WeeklySonnetUsage,
        reset_field: Field::WeeklySonnetResetAt,
    },
    ModelBucket {
        widget_type: "weekly-opus-usage",
        model_display_name: "Opus",
        api_bucket_key: Some("seven_day_opus"),
        usage_field: Field::WeeklyOpusUsage,
        reset_field: Field::WeeklyOpusResetAt,
    },
    ModelBucket {
        widget_type: "fable-weekly-usage",
        model_display_name: "Fable",
        api_bucket_key: None,
        usage_field: Field::FableUsage,
        reset_field: Field::FableResetAt,
    },
];

pub fn set_usage(d: &mut UsageData, f: Field, v: Option<f64>) {
    match f {
        Field::WeeklySonnetUsage => d.weekly_sonnet_usage = v,
        Field::WeeklyOpusUsage => d.weekly_opus_usage = v,
        Field::FableUsage => d.fable_usage = v,
        Field::SessionUsage => d.session_usage = v,
        Field::WeeklyUsage => d.weekly_usage = v,
        _ => {}
    }
}

pub fn set_reset(d: &mut UsageData, f: Field, v: Option<String>) {
    match f {
        Field::WeeklySonnetResetAt => d.weekly_sonnet_reset_at = v,
        Field::WeeklyOpusResetAt => d.weekly_opus_reset_at = v,
        Field::FableResetAt => d.fable_reset_at = v,
        Field::SessionResetAt => d.session_reset_at = v,
        Field::WeeklyResetAt => d.weekly_reset_at = v,
        _ => {}
    }
}

// ------------------------------------------------------------ prefetching

const BASE_USAGE_WIDGET_TYPES: [&str; 9] = [
    "session-usage",
    "weekly-usage",
    "block-timer",
    "reset-timer",
    "weekly-reset-timer",
    "extra-usage-utilization",
    "extra-usage-remaining",
    "extra-usage-used",
    "extra-usage-daily-budget",
];

fn is_usage_widget_type(kind: &str) -> bool {
    BASE_USAGE_WIDGET_TYPES.contains(&kind) || WEEKLY_MODEL_USAGE_BUCKETS.iter().any(|b| b.widget_type == kind)
}

pub fn has_usage_dependent_widgets(lines: &[Vec<WidgetItem>]) -> bool {
    lines.iter().any(|l| l.iter().any(|i| is_usage_widget_type(&i.kind)))
}

struct Requirement {
    field: Field,
    alternatives: &'static [Field],
    suppress_fetch_error: bool,
}

const fn req(field: Field) -> Requirement {
    Requirement { field, alternatives: &[], suppress_fetch_error: false }
}

const fn req_quiet(field: Field) -> Requirement {
    Requirement { field, alternatives: &[], suppress_fetch_error: true }
}

fn widget_requirements(kind: &str) -> Vec<Requirement> {
    match kind {
        "session-usage" => vec![req(Field::SessionUsage)],
        "weekly-usage" => vec![req(Field::WeeklyUsage)],
        "weekly-sonnet-usage" => vec![req(Field::WeeklySonnetUsage)],
        "weekly-opus-usage" => vec![req(Field::WeeklyOpusUsage)],
        "fable-weekly-usage" => vec![req(Field::FableUsage)],
        "block-timer" | "reset-timer" => vec![req_quiet(Field::SessionResetAt)],
        "weekly-reset-timer" => vec![req_quiet(Field::WeeklyResetAt)],
        "extra-usage-utilization" => vec![req(Field::ExtraUsageEnabled), req(Field::ExtraUsageUtilization)],
        "extra-usage-remaining" | "extra-usage-daily-budget" => {
            vec![req(Field::ExtraUsageEnabled), req(Field::ExtraUsageLimit), req(Field::ExtraUsageUsed)]
        }
        "extra-usage-used" => vec![req(Field::ExtraUsageEnabled), req(Field::ExtraUsageUsed)],
        _ => vec![],
    }
}

fn cursor_requirement(kind: &str) -> Option<Requirement> {
    match kind {
        "session-usage" => Some(req(Field::SessionResetAt)),
        "weekly-usage" => Some(req(Field::WeeklyResetAt)),
        _ => WEEKLY_MODEL_USAGE_BUCKETS.iter().find(|b| b.widget_type == kind).map(|b| Requirement {
            field: b.reset_field,
            alternatives: &[Field::WeeklyResetAt],
            suppress_fetch_error: false,
        }),
    }
}

fn usage_field_requirements(lines: &[Vec<WidgetItem>]) -> Vec<Requirement> {
    let mut out = Vec::new();
    for line in lines {
        for item in line {
            out.extend(widget_requirements(&item.kind));
            if let Some(c) = cursor_requirement(&item.kind) {
                if item.meta("cursor") == Some("true") {
                    out.push(c);
                }
            }
        }
    }
    out
}

/// `getMissingFetchRequirements`: (missing fields in first-seen order, suppressFetchError)
fn missing_fetch_requirements(data: Option<&UsageData>, reqs: &[Requirement]) -> (Vec<Field>, bool) {
    let has = |f: Field| data.is_some_and(|d| d.has(f));
    let mut missing: Vec<Field> = Vec::new();
    let mut unsuppressed = false;
    for r in reqs {
        let ok = has(r.field) || r.alternatives.iter().any(|a| has(*a));
        if !ok {
            if !missing.contains(&r.field) {
                missing.push(r.field);
            }
            if !r.suppress_fetch_error {
                unsuppressed = true;
            }
        }
    }
    let suppress = !missing.is_empty() && !unsuppressed;
    (missing, suppress)
}

fn epoch_seconds_to_iso(v: Option<f64>) -> Option<String> {
    let s = v.filter(|x| x.is_finite())?;
    let ms = s * 1000.0;
    // `new Date(ms).toISOString()` throws RangeError past ±8.64e15 ms
    if ms.abs() > 8.64e15 {
        return None;
    }
    Some(crate::timefmt::iso_string(ms))
}

/// `extractUsageDataFromRateLimits`
pub fn usage_from_rate_limits(data: &StatusJson) -> Option<UsageData> {
    let rl = data.rate_limits.as_ref()?;
    let period = |p: &Option<RateLimitPeriod>| p.clone();
    let five = period(&rl.five_hour);
    let seven = period(&rl.seven_day);
    let mut u = UsageData {
        session_usage: five.as_ref().and_then(|p| p.used_percentage),
        session_reset_at: epoch_seconds_to_iso(five.as_ref().and_then(|p| p.resets_at)),
        weekly_usage: seven.as_ref().and_then(|p| p.used_percentage),
        weekly_reset_at: epoch_seconds_to_iso(seven.as_ref().and_then(|p| p.resets_at)),
        ..Default::default()
    };
    for b in &WEEKLY_MODEL_USAGE_BUCKETS {
        let bucket = match b.api_bucket_key {
            Some("seven_day_sonnet") => period(&rl.seven_day_sonnet),
            Some("seven_day_opus") => period(&rl.seven_day_opus),
            _ => continue,
        };
        set_usage(&mut u, b.usage_field, bucket.as_ref().and_then(|p| p.used_percentage));
        set_reset(&mut u, b.reset_field, epoch_seconds_to_iso(bucket.as_ref().and_then(|p| p.resets_at)));
    }
    ALL_FIELDS.iter().any(|f| u.has(*f)).then_some(u)
}

/// `mergeUsageData(rateLimitsData, apiData)`
pub fn merge(rate: Option<&UsageData>, api: &UsageData) -> UsageData {
    let mut out = UsageData::default();
    for f in ALL_FIELDS {
        out.take_from(api, f);
    }
    if let Some(r) = rate {
        for f in ALL_FIELDS {
            out.take_from(r, f);
        }
    }
    out.error = api.error;
    out
}

/// `prefetchUsageDataIfNeeded(lines, data)`
pub fn prefetch(lines: &[Vec<WidgetItem>], data: &StatusJson) -> Option<UsageData> {
    if !has_usage_dependent_widgets(lines) {
        return None;
    }
    let rate = usage_from_rate_limits(data);
    let reqs = usage_field_requirements(lines);
    let (missing, _) = missing_fetch_requirements(rate.as_ref(), &reqs);
    if missing.is_empty() {
        return rate;
    }
    // Opt-in (not upstream): take missing fields from this session's own clauth
    // profile instead of the API and keychain. Stdin still wins, as upstream.
    let rate = if clauth_source_enabled() {
        match clauth::current().and_then(|r| r.profile.usage_data(crate::clock::now_ms())) {
            Some(c) => Some(merge(rate.as_ref(), &c)),
            None => rate,
        }
    } else {
        rate
    };
    let (mut missing, suppress) = missing_fetch_requirements(rate.as_ref(), &reqs);
    if clauth_source_enabled() {
        // what the feed settles (a window it reports with no reset, a Fable
        // week it does not report) needs no fetch either
        if let Some(r) = &rate {
            missing.retain(|f| !fetch::has_required_field(r, *f));
        }
    }
    if missing.is_empty() {
        return rate;
    }
    let api = fetch::fetch_usage_data(&missing);
    if api.error.is_some() && suppress {
        return rate;
    }
    Some(merge(rate.as_ref(), &api))
}

static USAGE: OnceLock<Option<UsageData>> = OnceLock::new();

/// `context.usageData`, computed on first use and shared by every widget of
/// the render.
pub fn usage_data(ctx: &RenderContext) -> Option<&'static UsageData> {
    USAGE.get_or_init(|| prefetch(&ctx.settings.lines, ctx.data)).as_ref()
}

/// `CCSTATUSLINE_RS_USAGE_SOURCE=clauth`: upstream usage widgets read the
/// session's clauth profile (~/.clauth/status.json) for fields stdin lacks.
pub fn clauth_source_enabled() -> bool {
    std::env::var("CCSTATUSLINE_RS_USAGE_SOURCE").is_ok_and(|v| v == "clauth")
}

/// Environment switches shared with the parity harness.
pub fn test_no_network() -> bool {
    std::env::var("CCSL_TEST_NO_NETWORK").is_ok_and(|v| v == "1")
}

pub fn test_no_keychain() -> bool {
    std::env::var("CCSL_TEST_NO_KEYCHAIN").is_ok_and(|v| v == "1")
}

pub fn test_no_background() -> bool {
    std::env::var("CCSL_TEST_NO_BACKGROUND").is_ok_and(|v| v == "1")
}

/// `~/.cache/ccstatusline`
pub fn cache_dir() -> std::path::PathBuf {
    crate::settings::home_dir().join(".cache").join("ccstatusline")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(kind: &str, cursor: bool) -> WidgetItem {
        let mut it = WidgetItem { kind: kind.into(), ..Default::default() };
        if cursor {
            let mut m = indexmap::IndexMap::new();
            m.insert("cursor".to_string(), "true".to_string());
            it.metadata = Some(m);
        }
        it
    }

    #[test]
    fn requirements_and_suppression() {
        let lines = vec![vec![item("reset-timer", false), item("weekly-reset-timer", false)]];
        let reqs = usage_field_requirements(&lines);
        let (missing, suppress) = missing_fetch_requirements(None, &reqs);
        assert_eq!(missing, vec![Field::SessionResetAt, Field::WeeklyResetAt]);
        assert!(suppress);

        let lines = vec![vec![item("weekly-opus-usage", true), item("reset-timer", false)]];
        let reqs = usage_field_requirements(&lines);
        let d = UsageData { weekly_opus_usage: Some(1.0), weekly_reset_at: Some("x".into()), ..Default::default() };
        let (missing, suppress) = missing_fetch_requirements(Some(&d), &reqs);
        assert_eq!(missing, vec![Field::SessionResetAt]);
        assert!(suppress);
    }

    #[test]
    fn merge_prefers_rate_limits() {
        let rate = UsageData { session_usage: Some(5.0), ..Default::default() };
        let api = UsageData {
            session_usage: Some(9.0),
            weekly_usage: Some(3.0),
            error: Some(UsageError::Timeout),
            ..Default::default()
        };
        let m = merge(Some(&rate), &api);
        assert_eq!(m.session_usage, Some(5.0));
        assert_eq!(m.weekly_usage, Some(3.0));
        assert_eq!(m.error, Some(UsageError::Timeout));
    }
}
