//! Observed usage (not upstream): what this session's last API reply said about
//! its account's limits, recorded per clauth profile so clauth and every other
//! session can read it.
//!
//! Claude Code reads the `anthropic-ratelimit-unified-*` headers of every
//! `/v1/messages` reply and hands them to the status line as `rate_limits`.
//! clauth instead polls `/api/oauth/usage`, which answers 429 for long stretches
//! once many sessions share an account, and its figures go stale. Each render
//! folds this session's `rate_limits` into
//! `~/.clauth/observed-usage/<profile>.json`, so a fresh reading from any session
//! on an account reaches clauth without one extra request.
//!
//! File contract, schema 1 (read by clauth's observed-usage fallback):
//!
//! ```json
//! {
//!   "schema": 1,
//!   "profile": "personal",
//!   "source": "ccstatusline-rs",
//!   "updated_at": 1730930000,
//!   "windows": {
//!     "5h": { "used_percentage": 12.0, "resets_at": 1730940000, "seen_at": 1730930000 },
//!     "7d": { "used_percentage": 35.0, "resets_at": 1731229200, "seen_at": 1730930000 }
//!   }
//! }
//! ```
//!
//! `used_percentage` is 0-100, `resets_at` and `seen_at` are epoch seconds.
//! A window's figure is a FLOOR as of `seen_at`: usage inside one window never
//! goes down, so a render that repeats an older reply can only confirm a value,
//! never lower it. Within a window (same `resets_at`) the higher figure wins; a
//! later `resets_at` starts a new window; an earlier one is an old reply and is
//! ignored.
//!
//! Which account a reading belongs to: the status line input names none, so the
//! candidate comes from `CLAUDE_CONFIG_DIR` (a `clauth start` session), then
//! clauth's `session_profiles.json` owner stamp, then clauth's active profile for
//! a global session. The 7d `resets_at` then confirms it: every account's weekly
//! reset falls at its own hour of the week, and clauth's feed carries each
//! profile's. A reading whose reset matches another profile goes to that one;
//! one that matches none or several, with no confirmed candidate, is dropped.

use super::clauth::{self, ClauthStatus, SessionAccount};
use crate::status::{RateLimitPeriod, StatusJson};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

pub const SCHEMA: u64 = 1;
const WEEK_SECS: i64 = 7 * 24 * 60 * 60;
/// How far two weekly resets may sit apart (mod one week) and still name the
/// same account. Header resets are whole seconds, the feed's carry a fraction,
/// and both sit on the hour, so ten minutes is generous without letting two
/// accounts an hour apart collide.
const FINGERPRINT_TOLERANCE_SECS: i64 = 600;
/// Two readings of one window whose resets are this close are the same window.
const SAME_WINDOW_TOLERANCE_SECS: i64 = 600;
/// Re-write an unchanged window at most this often, just to advance `seen_at`.
const CONFIRM_EVERY_SECS: i64 = 30;

/// Where the candidate profile came from, strongest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    ConfigDir,
    SessionOwner,
    Global,
}

impl Source {
    fn as_str(self) -> &'static str {
        match self {
            Source::ConfigDir => "config_dir",
            Source::SessionOwner => "session_owner",
            Source::Global => "global",
        }
    }
}

/// One window of one reading.
#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    pub used_percentage: f64,
    pub resets_at: i64,
}

fn reading(p: &Option<RateLimitPeriod>) -> Option<Reading> {
    let p = p.as_ref()?;
    let used = p.used_percentage.filter(|v| v.is_finite() && *v >= 0.0)?;
    let reset = p.resets_at.filter(|v| v.is_finite() && *v > 0.0)?;
    Some(Reading { used_percentage: used, resets_at: reset.floor() as i64 })
}

/// The windows this reply reported, keyed by clauth's window labels.
pub fn readings(data: &StatusJson) -> Vec<(&'static str, Reading)> {
    let Some(rl) = data.rate_limits.as_ref() else { return Vec::new() };
    [
        ("5h", reading(&rl.five_hour)),
        ("7d", reading(&rl.seven_day)),
        ("7d sonnet", reading(&rl.seven_day_sonnet)),
        ("7d opus", reading(&rl.seven_day_opus)),
    ]
    .into_iter()
    .filter_map(|(label, r)| r.map(|r| (label, r)))
    .collect()
}

/// A profile name safe to use as a file name.
pub fn valid_profile(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('.')
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// The owner clauth stamped on a session in `session_profiles.json`, when it is
/// a single known profile (a `contested` session touched two).
pub fn session_owner(raw: &str, session_id: &str) -> Option<String> {
    let v: Value = serde_json::from_str(raw).ok()?;
    let owner = v.get("sessions")?.get(session_id)?;
    owner.get("known")?.as_str().map(String::from)
}

/// Whether two weekly resets fall at the same moment of the week.
pub fn same_hour_of_week(a: i64, b: i64) -> bool {
    let d = (a - b).rem_euclid(WEEK_SECS);
    d <= FINGERPRINT_TOLERANCE_SECS || d >= WEEK_SECS - FINGERPRINT_TOLERANCE_SECS
}

/// Each Claude profile's 7d reset from clauth's feed, in epoch seconds.
fn weekly_resets(status: &ClauthStatus) -> Vec<(String, i64)> {
    status
        .profiles
        .iter()
        .filter(|p| p.harness == "claude")
        .filter_map(|p| {
            let w = p.windows.iter().find(|w| w.label == "7d")?;
            let ms = crate::timefmt::parse_iso(w.resets_at.as_deref()?)?;
            Some((p.name.clone(), (ms / 1000.0).floor() as i64))
        })
        .collect()
}

/// The profile this reading belongs to and how that was decided, or None when
/// it cannot be told.
pub fn attribute(
    candidate: Option<(String, Source)>,
    weekly_reset: Option<i64>,
    status: Option<&ClauthStatus>,
) -> Option<(String, String)> {
    let prints = status.map(weekly_resets).unwrap_or_default();
    let Some(reset) = weekly_reset.filter(|_| !prints.is_empty()) else {
        // Nothing to confirm against: only a session that names its own
        // account is trusted; a global one may have moved with a switch.
        return candidate.filter(|(_, s)| *s != Source::Global).map(|(name, s)| (name, s.as_str().to_string()));
    };
    let matches: Vec<&str> =
        prints.iter().filter(|(_, r)| same_hour_of_week(*r, reset)).map(|(n, _)| n.as_str()).collect();
    if let Some((name, source)) = &candidate {
        if matches.contains(&name.as_str()) {
            return Some((name.clone(), format!("{}+fingerprint", source.as_str())));
        }
        let candidate_has_print = prints.iter().any(|(n, _)| n == name);
        if !candidate_has_print && matches.is_empty() && *source != Source::Global {
            return Some((name.clone(), source.as_str().to_string()));
        }
    }
    match matches.as_slice() {
        [only] => Some((only.to_string(), "fingerprint".to_string())),
        _ => None,
    }
}

fn window_obj(r: &Reading, seen_at: i64) -> Value {
    json!({ "used_percentage": r.used_percentage, "resets_at": r.resets_at, "seen_at": seen_at })
}

fn stored_reading(w: &Value) -> Option<(Reading, i64)> {
    Some((
        Reading { used_percentage: w.get("used_percentage")?.as_f64()?, resets_at: w.get("resets_at")?.as_i64()? },
        w.get("seen_at").and_then(Value::as_i64).unwrap_or(0),
    ))
}

/// Fold this reply's windows into the stored record. Returns the record to
/// write, or None when nothing changed enough to be worth a write.
pub fn merge(existing: Option<&Value>, profile: &str, incoming: &[(&str, Reading)], now: i64) -> Option<Value> {
    let mut windows: Map<String, Value> = existing
        .filter(|v| v.get("schema").and_then(Value::as_u64) == Some(SCHEMA))
        .and_then(|v| v.get("windows"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut changed = existing.is_none();
    for (label, new) in incoming {
        let old = windows.get(*label).and_then(stored_reading);
        let next = match old {
            None => Some(window_obj(new, now)),
            Some((old, seen)) => {
                let gap = new.resets_at - old.resets_at;
                if gap > SAME_WINDOW_TOLERANCE_SECS {
                    // a later window began
                    Some(window_obj(new, now))
                } else if gap < -SAME_WINDOW_TOLERANCE_SECS {
                    // an older reply about an earlier window
                    None
                } else if new.used_percentage > old.used_percentage {
                    Some(window_obj(&Reading { used_percentage: new.used_percentage, resets_at: old.resets_at }, now))
                } else if new.used_percentage == old.used_percentage && now - seen >= CONFIRM_EVERY_SECS {
                    Some(window_obj(&old, now))
                } else {
                    None
                }
            }
        };
        if let Some(w) = next {
            windows.insert(label.to_string(), w);
            changed = true;
        }
    }
    changed.then(|| {
        json!({
            "schema": SCHEMA,
            "profile": profile,
            "source": "ccstatusline-rs",
            "updated_at": now,
            "windows": Value::Object(windows),
        })
    })
}

pub fn dir(home: &Path) -> PathBuf {
    home.join(".clauth").join("observed-usage")
}

/// On whenever the status line already works with clauth
/// (`CCSTATUSLINE_RS_USAGE_SOURCE=clauth`), or with
/// `CCSTATUSLINE_RS_OBSERVE_USAGE=1`; `=0` turns it off. Never under the parity
/// harness, which must not write outside its sandbox.
pub fn enabled() -> bool {
    if super::test_no_network() || super::test_no_background() {
        return false;
    }
    match std::env::var("CCSTATUSLINE_RS_OBSERVE_USAGE").ok().as_deref() {
        Some("0") => false,
        Some("1") => true,
        _ => super::clauth_source_enabled(),
    }
}

/// Record this render's `rate_limits`. Best effort and silent: a status line
/// never fails over bookkeeping.
pub fn record(data: &StatusJson) {
    if !enabled() {
        return;
    }
    let incoming = readings(data);
    if incoming.is_empty() {
        return;
    }
    let home = crate::settings::home_dir();
    if !home.join(".clauth").is_dir() {
        return;
    }
    let status = std::fs::read(clauth::status_path(&home))
        .ok()
        .and_then(|raw| clauth::parse_status(&String::from_utf8_lossy(&raw)));
    let config_dir = std::env::var("CLAUDE_CONFIG_DIR").ok();
    let candidate = match clauth::session_account(config_dir.as_deref(), &home) {
        SessionAccount::Profile(name) => Some((name, Source::ConfigDir)),
        SessionAccount::Foreign => return,
        SessionAccount::Global => data
            .session_id
            .as_deref()
            .and_then(|sid| {
                let raw = std::fs::read(home.join(".clauth").join("session_profiles.json")).ok()?;
                session_owner(&String::from_utf8_lossy(&raw), sid)
            })
            .map(|name| (name, Source::SessionOwner))
            .or_else(|| status.as_ref()?.active_profile.clone().map(|n| (n, Source::Global))),
    };
    let weekly = incoming.iter().find(|(l, _)| *l == "7d").map(|(_, r)| r.resets_at);
    let Some((profile, _how)) = attribute(candidate, weekly, status.as_ref()) else { return };
    if !valid_profile(&profile) {
        return;
    }
    let path = dir(&home).join(format!("{profile}.json"));
    let existing = std::fs::read(&path).ok().and_then(|raw| serde_json::from_slice::<Value>(&raw).ok());
    let now = (crate::clock::now_ms() / 1000.0).floor() as i64;
    if let Some(record) = merge(existing.as_ref(), &profile, &incoming, now) {
        if let Ok(bytes) = serde_json::to_vec(&record) {
            let _ = crate::cachefile::atomic_write(&path, &bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::clauth::{ClauthProfile, ClauthWindow};

    fn profile(name: &str, weekly_reset_iso: Option<&str>) -> ClauthProfile {
        ClauthProfile {
            name: name.into(),
            active: false,
            tier: None,
            harness: "claude".into(),
            auth_status: "ok".into(),
            fetch_status: Some("Fresh".into()),
            stale: false,
            fallback: None,
            windows: weekly_reset_iso
                .map(|r| vec![ClauthWindow { label: "7d".into(), utilization_pct: 10.0, resets_at: Some(r.into()) }])
                .unwrap_or_default(),
        }
    }

    fn status(profiles: Vec<ClauthProfile>) -> ClauthStatus {
        ClauthStatus {
            generated_at_ms: None,
            active_profile: Some("personal".into()),
            pending_switch: None,
            refresh_interval_ms: 90_000.0,
            profiles,
        }
    }

    // 2026-11-10T09:00:00Z and 2026-11-11T07:00:00Z
    const PERSONAL_RESET: i64 = 1_794_301_200;
    const CONTRATAPJ_RESET: i64 = 1_794_380_400;

    fn feed() -> ClauthStatus {
        status(vec![
            profile("personal", Some("2026-11-10T09:00:00.783034+00:00")),
            profile("contratapj", Some("2026-11-11T07:00:00.501590+00:00")),
        ])
    }

    #[test]
    fn should_confirm_a_candidate_whose_weekly_reset_matches() {
        let got = attribute(Some(("personal".into(), Source::Global)), Some(PERSONAL_RESET), Some(&feed()));
        assert_eq!(got, Some(("personal".into(), "global+fingerprint".into())));
    }

    #[test]
    fn should_move_a_reading_to_the_profile_its_weekly_reset_names() {
        // a global session still on contratapj after the global switch to personal
        let got = attribute(Some(("personal".into(), Source::Global)), Some(CONTRATAPJ_RESET), Some(&feed()));
        assert_eq!(got, Some(("contratapj".into(), "fingerprint".into())));
    }

    #[test]
    fn should_match_a_reset_one_week_later() {
        let got = attribute(None, Some(PERSONAL_RESET + WEEK_SECS), Some(&feed()));
        assert_eq!(got, Some(("personal".into(), "fingerprint".into())));
    }

    #[test]
    fn should_drop_a_reading_no_profile_matches() {
        let got = attribute(Some(("personal".into(), Source::Global)), Some(PERSONAL_RESET + 3600), Some(&feed()));
        assert_eq!(got, None);
    }

    #[test]
    fn should_drop_a_reading_two_profiles_match() {
        let twins = status(vec![
            profile("a", Some("2026-11-10T09:00:00+00:00")),
            profile("b", Some("2026-11-03T09:00:00+00:00")),
        ]);
        assert_eq!(attribute(None, Some(PERSONAL_RESET), Some(&twins)), None);
    }

    #[test]
    fn should_trust_a_named_session_when_nothing_can_confirm_it() {
        let got = attribute(Some(("tgp".into(), Source::ConfigDir)), None, Some(&feed()));
        assert_eq!(got, Some(("tgp".into(), "config_dir".into())));
        let got = attribute(Some(("tgp".into(), Source::SessionOwner)), Some(PERSONAL_RESET + 3600), Some(&feed()));
        assert_eq!(got, Some(("tgp".into(), "session_owner".into())));
    }

    #[test]
    fn should_not_trust_a_global_session_without_a_fingerprint() {
        assert_eq!(attribute(Some(("personal".into(), Source::Global)), None, Some(&feed())), None);
        assert_eq!(attribute(Some(("personal".into(), Source::Global)), Some(PERSONAL_RESET), None), None);
    }

    #[test]
    fn should_not_trust_a_named_session_another_profile_fingerprints() {
        // tgp has no 7d in the feed, but the reading's reset is personal's
        let got = attribute(Some(("tgp".into(), Source::ConfigDir)), Some(PERSONAL_RESET), Some(&feed()));
        assert_eq!(got, Some(("personal".into(), "fingerprint".into())));
    }

    #[test]
    fn should_read_a_known_owner_and_skip_a_contested_one() {
        let raw = r#"{"sessions":{"a":{"known":"tgp"},"b":"contested"}}"#;
        assert_eq!(session_owner(raw, "a"), Some("tgp".into()));
        assert_eq!(session_owner(raw, "b"), None);
        assert_eq!(session_owner(raw, "c"), None);
    }

    fn r(used: f64, reset: i64) -> Reading {
        Reading { used_percentage: used, resets_at: reset }
    }

    fn window(v: &Value, label: &str) -> (f64, i64, i64) {
        let w = &v["windows"][label];
        (w["used_percentage"].as_f64().unwrap(), w["resets_at"].as_i64().unwrap(), w["seen_at"].as_i64().unwrap())
    }

    #[test]
    fn should_create_a_record_from_the_first_reading() {
        let v = merge(None, "personal", &[("7d", r(35.0, PERSONAL_RESET))], 100).unwrap();
        assert_eq!(v["schema"], 1);
        assert_eq!(v["profile"], "personal");
        assert_eq!(window(&v, "7d"), (35.0, PERSONAL_RESET, 100));
    }

    #[test]
    fn should_never_lower_a_window_from_an_older_reply() {
        let v = merge(None, "p", &[("7d", r(40.0, PERSONAL_RESET))], 100).unwrap();
        // a session whose last reply predates the other's repeats 35%
        assert_eq!(merge(Some(&v), "p", &[("7d", r(35.0, PERSONAL_RESET))], 200), None);
        let up = merge(Some(&v), "p", &[("7d", r(41.0, PERSONAL_RESET))], 200).unwrap();
        assert_eq!(window(&up, "7d"), (41.0, PERSONAL_RESET, 200));
    }

    #[test]
    fn should_start_a_new_window_on_a_later_reset_and_ignore_an_earlier_one() {
        let v = merge(None, "p", &[("5h", r(90.0, 10_000))], 100).unwrap();
        let next = merge(Some(&v), "p", &[("5h", r(3.0, 28_000))], 200).unwrap();
        assert_eq!(window(&next, "5h"), (3.0, 28_000, 200));
        assert_eq!(merge(Some(&next), "p", &[("5h", r(95.0, 10_000))], 300), None);
    }

    #[test]
    fn should_confirm_an_unchanged_window_only_every_thirty_seconds() {
        let v = merge(None, "p", &[("7d", r(35.0, PERSONAL_RESET))], 100).unwrap();
        assert_eq!(merge(Some(&v), "p", &[("7d", r(35.0, PERSONAL_RESET))], 129), None);
        let later = merge(Some(&v), "p", &[("7d", r(35.0, PERSONAL_RESET))], 130).unwrap();
        assert_eq!(window(&later, "7d"), (35.0, PERSONAL_RESET, 130));
    }

    #[test]
    fn should_keep_windows_the_reply_did_not_report() {
        let v = merge(None, "p", &[("5h", r(10.0, 20_000)), ("7d", r(35.0, PERSONAL_RESET))], 100).unwrap();
        let next = merge(Some(&v), "p", &[("7d", r(36.0, PERSONAL_RESET))], 200).unwrap();
        assert_eq!(window(&next, "5h"), (10.0, 20_000, 100));
        assert_eq!(window(&next, "7d"), (36.0, PERSONAL_RESET, 200));
    }

    #[test]
    fn should_restart_from_a_record_of_another_schema() {
        let old = json!({"schema": 99, "windows": {"7d": {"used_percentage": 80.0, "resets_at": PERSONAL_RESET}}});
        let v = merge(Some(&old), "p", &[("7d", r(35.0, PERSONAL_RESET))], 100).unwrap();
        assert_eq!(window(&v, "7d"), (35.0, PERSONAL_RESET, 100));
    }

    #[test]
    fn should_refuse_profile_names_that_are_not_file_names() {
        assert!(valid_profile("brane-danielnaves"));
        assert!(!valid_profile("../x"));
        assert!(!valid_profile(".hidden"));
        assert!(!valid_profile("a/b"));
        assert!(!valid_profile(""));
    }

    #[test]
    fn should_read_only_finite_windows() {
        let d = StatusJson {
            rate_limits: Some(crate::status::RateLimits {
                five_hour: Some(RateLimitPeriod { used_percentage: Some(f64::NAN), resets_at: Some(1.0) }),
                seven_day: Some(RateLimitPeriod {
                    used_percentage: Some(35.0),
                    resets_at: Some(PERSONAL_RESET as f64),
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(readings(&d), vec![("7d", r(35.0, PERSONAL_RESET))]);
    }
}
