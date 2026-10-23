//! Widget group: extra (not in upstream). The clauth family: the session's own
//! clauth profile and its usage, read from `~/.clauth/status.json` with zero
//! network (see `crate::usage::clauth`).
//!
//! Upstream's usage widgets read the account the global credentials belong
//! to, so a session running under another clauth profile shows the wrong
//! account's numbers. These take the account from the session's
//! `CLAUDE_CONFIG_DIR` instead, and accept the same options as the widgets
//! they replace:
//!
//! - `clauth-session-usage`, `clauth-weekly-usage`, `clauth-fable-weekly-usage`,
//!   `clauth-weekly-opus-usage`, `clauth-weekly-sonnet-usage`: as
//!   `session-usage` & co. (display, invert, cursor, numberFormat, rawValue).
//! - `clauth-reset-timer`, `clauth-weekly-reset-timer`: as `reset-timer` /
//!   `weekly-reset-timer` (display, invert, compact, absolute, hour12,
//!   weekday, hours, timezone, locale).
//! - `clauth-profile`: the profile name. Metadata `tier: "true"` adds the plan
//!   (`profile-a (Max 20x)`), `chain: "true"` the fallback-chain slot (`#2`);
//!   a global session with a switch in flight shows `old → new`.
//! - `clauth-account`: one compact widget, `profile-a 5h 12% · 7d 0%`
//!   (percent numberFormat applies, whole percents by default); `fable:
//!   "true"` adds the Fable week, `resets: "true"` the time to each reset.
//!   Marks `(stale)` / `(auth expired)` / `(auth broken)` when clauth does.
//! - `clauth-chain`: the fallback chain, `#2/8 · 90% · active: work`.
//!   `show` picks and orders comma-separated parts: `pending` (a global
//!   session's in-flight switch, `⇄ next`), `position` (slot/chain length and
//!   threshold), `next` (the next chain account under its threshold) and
//!   `active` (the global account, when this session runs on another one).
//!
//! Stdin `rate_limits`, when Claude Code sends them, win over the feed (they
//! are the session's own account and newer), as they win over the API
//! upstream. `CCSTATUSLINE_RS_USAGE_SOURCE=clauth` serves the upstream usage
//! widgets from the same feed (see `crate::usage::clauth_source_enabled`).
//!
//! When clauth's feed is missing, unparseable or older than ~10 minutes, or
//! the session's profile is not in it (or the config dir is not clauth's),
//! every clauth widget renders nothing. Profile names that look like email
//! addresses show only their local part.

use super::shared::{format_percent, raw_or_labeled, resolve_number_format};
use super::usage::{render_percent, render_timer, NoWindow, PercentKind, TimerKind};
use super::Widget;
use crate::context::RenderContext;
use crate::settings::{NumberFormat, WidgetItem};
use crate::usage::clauth::{self, display_name, Resolved};
use crate::usage::{windows, UsageData};

/// The profile's figures with stdin `rate_limits` on top: when Claude Code
/// sends them they are this session's own account and fresher than the feed.
pub fn session_usage(r: &Resolved, rate: Option<&UsageData>, now: f64) -> Option<UsageData> {
    let feed = r.profile.usage_data(now)?;
    Some(crate::usage::merge(rate, &feed))
}

fn stdin_usage(ctx: &RenderContext) -> Option<UsageData> {
    crate::usage::usage_from_rate_limits(ctx.data)
}

fn current(ctx: &RenderContext) -> Option<&'static Resolved> {
    if ctx.is_preview {
        return None;
    }
    clauth::current()
}

pub struct ClauthPercentWidget(PercentKind);

impl Widget for ClauthPercentWidget {
    fn default_color(&self) -> &'static str {
        "brightBlue"
    }
    fn display_name(&self) -> &'static str {
        match self.0 {
            PercentKind::Session => "clauth Session Usage",
            PercentKind::Weekly => "clauth Weekly Usage",
            PercentKind::WeeklySonnet => "clauth Weekly Sonnet Usage",
            PercentKind::WeeklyOpus => "clauth Weekly Opus Usage",
            PercentKind::FableWeekly => "clauth Weekly Fable Usage",
        }
    }
    fn description(&self) -> &'static str {
        "Usage percentage of this session's own clauth profile, from ~/.clauth/status.json"
    }
    fn category(&self) -> &'static str {
        "Usage"
    }
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        if ctx.is_preview {
            return render_percent(self.0, item, ctx, None, false);
        }
        let r = current(ctx)?;
        let data = session_usage(r, stdin_usage(ctx).as_ref(), crate::clock::now_ms())?;
        render_percent(self.0, item, ctx, Some(&data), false)
    }
}

pub struct ClauthTimerWidget(TimerKind);

impl Widget for ClauthTimerWidget {
    fn default_color(&self) -> &'static str {
        "brightBlue"
    }
    fn display_name(&self) -> &'static str {
        match self.0 {
            TimerKind::WeeklyReset => "clauth Weekly Reset Timer",
            _ => "clauth Block Reset Timer",
        }
    }
    fn description(&self) -> &'static str {
        "Time until this session's clauth profile's usage window resets, from ~/.clauth/status.json"
    }
    fn category(&self) -> &'static str {
        "Usage"
    }
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        if ctx.is_preview {
            return render_timer(self.0, item, ctx, None, false, NoWindow::Nothing);
        }
        let r = current(ctx)?;
        let data = session_usage(r, stdin_usage(ctx).as_ref(), crate::clock::now_ms())?;
        render_timer(self.0, item, ctx, Some(&data), false, NoWindow::Nothing)
    }
}

fn profile_marks(r: &Resolved) -> String {
    let mut s = String::new();
    match r.profile.auth_status.as_str() {
        "expired" => s.push_str(" (auth expired)"),
        "broken" => s.push_str(" (auth broken)"),
        _ => {}
    }
    if r.profile.stale {
        s.push_str(" (stale)");
    }
    s
}

/// `clauth-profile` text for a resolved session.
pub fn profile_text(item: &WidgetItem, r: &Resolved) -> String {
    let mut text = display_name(&r.profile.name);
    if r.global {
        if let Some(p) = r.status.pending_switch.as_deref().filter(|p| *p != r.profile.name) {
            text = format!("{text} → {}", display_name(p));
        }
    }
    if item.meta("tier") == Some("true") {
        if let Some(t) = &r.profile.tier {
            text = format!("{text} ({t})");
        }
    }
    if item.meta("chain") == Some("true") {
        if let Some(f) = &r.profile.fallback {
            text = format!("{text} #{}", f.position);
        }
    }
    text.push_str(&profile_marks(r));
    text
}

pub struct ClauthProfileWidget;

impl Widget for ClauthProfileWidget {
    fn default_color(&self) -> &'static str {
        "magenta"
    }
    fn display_name(&self) -> &'static str {
        "clauth Profile"
    }
    fn description(&self) -> &'static str {
        "The clauth profile this session runs on"
    }
    fn category(&self) -> &'static str {
        "Session"
    }
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        if ctx.is_preview {
            return Some(raw_or_labeled(item, "Profile: ", "profile-a"));
        }
        let r = current(ctx)?;
        Some(raw_or_labeled(item, "Profile: ", &profile_text(item, r)))
    }
}

fn whole_percent(v: f64, f: &NumberFormat) -> String {
    format_percent(v.clamp(0.0, 100.0), f, 0)
}

/// `clauth-account` text for a resolved session.
pub fn account_text(item: &WidgetItem, r: &Resolved, rate: Option<&UsageData>, f: &NumberFormat, now: f64) -> String {
    let mut text = display_name(&r.profile.name);
    if let Some(d) = session_usage(r, rate, now) {
        let resets = item.meta("resets") == Some("true");
        let with_reset = |pct: f64, reset_at: Option<&str>, weekly: bool| {
            let p = whole_percent(pct, f);
            let w = if weekly {
                windows::weekly_from_reset_at(reset_at, now)
            } else {
                windows::from_reset_at(reset_at, now)
            };
            match w {
                Some(w) if resets => format!("{p} ({})", windows::format_duration(w.remaining_ms, true, true)),
                _ => p,
            }
        };
        let mut parts = vec![
            format!("5h {}", with_reset(d.session_usage.unwrap_or(0.0), d.session_reset_at.as_deref(), false)),
            format!("7d {}", with_reset(d.weekly_usage.unwrap_or(0.0), d.weekly_reset_at.as_deref(), true)),
        ];
        if item.meta("fable") == Some("true") {
            if let Some(p) = d.fable_usage {
                parts.push(format!("fable {}", with_reset(p, d.fable_reset_at.as_deref(), true)));
            }
        }
        text = format!("{text} {}", parts.join(" · "));
    }
    text.push_str(&profile_marks(r));
    text
}

pub struct ClauthAccountWidget;

impl Widget for ClauthAccountWidget {
    fn default_color(&self) -> &'static str {
        "brightBlue"
    }
    fn display_name(&self) -> &'static str {
        "clauth Account"
    }
    fn description(&self) -> &'static str {
        "This session's clauth profile with its 5h and 7d usage, e.g. \"profile-a 5h 12% · 7d 0%\""
    }
    fn category(&self) -> &'static str {
        "Usage"
    }
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        if ctx.is_preview {
            return Some(raw_or_labeled(item, "", "profile-a 5h 12% · 7d 0%"));
        }
        let r = current(ctx)?;
        let f = resolve_number_format("percent", item, ctx.settings);
        Some(raw_or_labeled(item, "", &account_text(item, r, stdin_usage(ctx).as_ref(), &f, crate::clock::now_ms())))
    }
}

/// `clauth-chain` text: the parts `metadata.show` names (comma-separated
/// `pending`, `position`, `next`, `active`; default `pending,position,active`).
pub fn chain_text(item: &WidgetItem, r: &Resolved, f: &NumberFormat, now: f64) -> Option<String> {
    let show = item.meta("show").unwrap_or("pending,position,active");
    let chain_len = r.status.profiles.iter().filter(|p| p.harness == "claude" && p.fallback.is_some()).count();
    let mut parts: Vec<String> = Vec::new();
    for part in show.split(',').map(crate::jsnum::js_trim) {
        match part {
            // a pending switch moves the global login, so it concerns global sessions
            "pending" if r.global => {
                if let Some(p) = r.status.pending_switch.as_deref().filter(|p| *p != r.profile.name) {
                    parts.push(format!("⇄ {}", display_name(p)));
                }
            }
            "position" => {
                if let Some(fb) = &r.profile.fallback {
                    parts.push(format!("#{}/{chain_len} · {}", fb.position, format_percent(fb.threshold, f, 0)));
                }
            }
            "next" => {
                let Some(pos) = r.profile.fallback.as_ref().map(|fb| fb.position) else { continue };
                let mut later: Vec<_> = r
                    .status
                    .profiles
                    .iter()
                    .filter(|p| p.harness == "claude" && p.fallback.as_ref().is_some_and(|fb| fb.position > pos))
                    .collect();
                later.sort_by_key(|p| p.fallback.as_ref().map(|fb| fb.position));
                let has_headroom = |p: &&crate::usage::clauth::ClauthProfile| {
                    let threshold = p.fallback.as_ref().map(|fb| fb.threshold).unwrap_or(100.0);
                    ["5h", "7d"].iter().all(|l| p.window(l, now).is_none_or(|w| w.utilization_pct < threshold))
                };
                if let Some(next) = later.into_iter().find(has_headroom) {
                    parts.push(format!("→ {}", display_name(&next.name)));
                }
            }
            "active" if !r.global => {
                if let Some(a) = r.status.active_profile.as_deref().filter(|a| *a != r.profile.name) {
                    parts.push(format!("active: {}", display_name(a)));
                }
            }
            _ => {}
        }
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

pub struct ClauthChainWidget;

impl Widget for ClauthChainWidget {
    fn default_color(&self) -> &'static str {
        "brightBlack"
    }
    fn display_name(&self) -> &'static str {
        "clauth Chain"
    }
    fn description(&self) -> &'static str {
        "clauth fallback chain: pending switch, this profile's slot and threshold, next account, global active account"
    }
    fn category(&self) -> &'static str {
        "Session"
    }
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        if ctx.is_preview {
            return Some(raw_or_labeled(item, "", "#2/8 · 90% · active: work"));
        }
        let r = current(ctx)?;
        let f = resolve_number_format("percent", item, ctx.settings);
        Some(raw_or_labeled(item, "", &chain_text(item, r, &f, crate::clock::now_ms())?))
    }
}

static SESSION: ClauthPercentWidget = ClauthPercentWidget(PercentKind::Session);
static WEEKLY: ClauthPercentWidget = ClauthPercentWidget(PercentKind::Weekly);
static WEEKLY_SONNET: ClauthPercentWidget = ClauthPercentWidget(PercentKind::WeeklySonnet);
static WEEKLY_OPUS: ClauthPercentWidget = ClauthPercentWidget(PercentKind::WeeklyOpus);
static FABLE: ClauthPercentWidget = ClauthPercentWidget(PercentKind::FableWeekly);
static RESET: ClauthTimerWidget = ClauthTimerWidget(TimerKind::BlockReset);
static WEEKLY_RESET: ClauthTimerWidget = ClauthTimerWidget(TimerKind::WeeklyReset);
static PROFILE: ClauthProfileWidget = ClauthProfileWidget;
static ACCOUNT: ClauthAccountWidget = ClauthAccountWidget;
static CHAIN: ClauthChainWidget = ClauthChainWidget;

pub fn lookup(kind: &str) -> Option<&'static dyn Widget> {
    Some(match kind {
        "clauth-session-usage" => &SESSION,
        "clauth-weekly-usage" => &WEEKLY,
        "clauth-weekly-sonnet-usage" => &WEEKLY_SONNET,
        "clauth-weekly-opus-usage" => &WEEKLY_OPUS,
        "clauth-fable-weekly-usage" => &FABLE,
        "clauth-reset-timer" => &RESET,
        "clauth-weekly-reset-timer" => &WEEKLY_RESET,
        "clauth-profile" => &PROFILE,
        "clauth-account" => &ACCOUNT,
        "clauth-chain" => &CHAIN,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;
    use crate::status::StatusJson;
    use crate::usage::clauth::{parse_status, resolve, SessionAccount, TEST_FIXTURE};

    const NOW: f64 = 1792584000000.0;

    fn item(kind: &str, meta: &[(&str, &str)]) -> WidgetItem {
        let mut m = indexmap::IndexMap::new();
        for (k, v) in meta {
            m.insert(k.to_string(), v.to_string());
        }
        WidgetItem { kind: kind.into(), metadata: Some(m), ..Default::default() }
    }

    fn resolved(account: SessionAccount) -> Resolved {
        resolve(parse_status(TEST_FIXTURE).unwrap(), &account, NOW).unwrap()
    }

    #[test]
    fn percent_and_timers_use_the_sessions_profile() {
        let data = StatusJson::default();
        let settings = Settings::default();
        let ctx = RenderContext::new(&data, &settings, None);
        let r = resolved(SessionAccount::Profile("profile-a".into()));
        let d = r.profile.usage_data(NOW).unwrap();
        let weekly = render_percent(PercentKind::Weekly, &item("clauth-weekly-usage", &[]), &ctx, Some(&d), false);
        assert_eq!(weekly.as_deref(), Some("Weekly: 0.0%"));
        let session =
            render_percent(PercentKind::Session, &item("x", &[("display", "progress-short")]), &ctx, Some(&d), false);
        assert_eq!(session.as_deref(), Some("Session: [██░░░░░░░░░░░░░░] 12.0%"));
        let fable = render_percent(PercentKind::FableWeekly, &item("x", &[]), &ctx, Some(&d), false);
        assert_eq!(fable.as_deref(), Some("Weekly Fable: 7.5%"));
        let opus = render_percent(PercentKind::WeeklyOpus, &item("x", &[]), &ctx, Some(&d), false);
        assert_eq!(opus, None);

        // timers read the fixed clock; just check the shapes
        let reset = render_timer(
            TimerKind::BlockReset,
            &item("x", &[("absolute", "true")]),
            &ctx,
            Some(&d),
            false,
            NoWindow::Nothing,
        );
        assert_eq!(reset.as_deref(), Some("Reset: 2026-10-21 14:30 UTC"));
        let weekly_reset = render_timer(
            TimerKind::WeeklyReset,
            &item("x", &[("absolute", "true"), ("weekday", "true"), ("compact", "true")]),
            &ctx,
            Some(&d),
            false,
            NoWindow::Nothing,
        );
        assert_eq!(weekly_reset.as_deref(), Some("Weekly Reset: Tue 09:00Z"));

        // the global session's 5h window lapsed: no reset timer at all
        let g = resolved(SessionAccount::Global);
        let gd = g.profile.usage_data(NOW).unwrap();
        assert_eq!(
            render_timer(TimerKind::BlockReset, &item("x", &[]), &ctx, Some(&gd), false, NoWindow::Nothing),
            None
        );
    }

    #[test]
    fn chain() {
        let f = NumberFormat::default();
        let r = resolved(SessionAccount::Profile("profile-a".into()));
        assert_eq!(chain_text(&item("clauth-chain", &[]), &r, &f, NOW).as_deref(), Some("#2/1 · 90% · active: work"));
        assert_eq!(chain_text(&item("clauth-chain", &[("show", "next")]), &r, &f, NOW), None);
        let mut st = parse_status(TEST_FIXTURE).unwrap();
        st.pending_switch = Some("profile-a".into());
        st.profiles[1].fallback = Some(crate::usage::clauth::Fallback { position: 3, threshold: 99.0, armed: true });
        let g = resolve(st.clone(), &SessionAccount::Global, NOW).unwrap();
        assert_eq!(chain_text(&item("clauth-chain", &[]), &g, &f, NOW).as_deref(), Some("⇄ profile-a · #3/2 · 99%"));
        let p = resolve(st, &SessionAccount::Profile("profile-a".into()), NOW).unwrap();
        // work (slot 3) is at 98 % of 7d, under its 99 % threshold
        assert_eq!(chain_text(&item("clauth-chain", &[("show", "next")]), &p, &f, NOW).as_deref(), Some("→ work"));
    }

    #[test]
    fn stdin_rate_limits_win() {
        let r = resolved(SessionAccount::Profile("profile-a".into()));
        let rate = UsageData { weekly_usage: Some(4.0), ..Default::default() };
        let f = NumberFormat::default();
        assert_eq!(account_text(&item("clauth-account", &[]), &r, Some(&rate), &f, NOW), "profile-a 5h 12% · 7d 4%");
    }

    #[test]
    fn profile_and_account_text() {
        let r = resolved(SessionAccount::Profile("profile-a".into()));
        assert_eq!(profile_text(&item("clauth-profile", &[]), &r), "profile-a");
        assert_eq!(
            profile_text(&item("clauth-profile", &[("tier", "true"), ("chain", "true")]), &r),
            "profile-a (Max 20x) #2"
        );
        let f = NumberFormat::default();
        assert_eq!(account_text(&item("clauth-account", &[]), &r, None, &f, NOW), "profile-a 5h 12% · 7d 0%");
        assert_eq!(
            account_text(&item("clauth-account", &[("fable", "true"), ("resets", "true")]), &r, None, &f, NOW),
            "profile-a 5h 12% (2h30m) · 7d 0% (5d21h) · fable 8% (5d21h)"
        );

        let mut st = parse_status(TEST_FIXTURE).unwrap();
        st.pending_switch = Some("profile-a".into());
        st.profiles[1].stale = true;
        let g = resolve(st, &SessionAccount::Global, NOW).unwrap();
        assert_eq!(profile_text(&item("clauth-profile", &[]), &g), "work → profile-a (stale)");
        assert_eq!(account_text(&item("clauth-account", &[]), &g, None, &f, NOW), "work 5h 0% · 7d 98% (stale)");
    }
}
