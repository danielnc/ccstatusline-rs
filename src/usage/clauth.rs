//! clauth's published feed, `~/.clauth/status.json` (schema 2, written by the
//! clauth daemon every ~90 s; contract in clauth's wiki/Daemon.md and
//! `src/daemon/status_json.rs`). Zero network: the clauth widgets read only
//! this file.
//!
//! A session belongs to the profile its `CLAUDE_CONFIG_DIR` names
//! (`~/.clauth/profiles/<name>/runtime[-isolated][-<pid>-<seq>]`, or the
//! profile dir itself). A session without one (or pointing at `~/.claude`)
//! runs on the global credentials, i.e. clauth's `active_profile`. Any other
//! config dir is not clauth's, and the widgets render nothing.

use super::UsageData;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// A feed older than this (and older than 3 refresh intervals) means the
/// daemon is gone; its numbers are not shown.
const MIN_STALE_FEED_MS: f64 = 10.0 * 60.0 * 1000.0;

#[derive(Clone, Debug, PartialEq)]
pub struct ClauthWindow {
    pub label: String,
    pub utilization_pct: f64,
    pub resets_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Fallback {
    pub position: u64,
    pub threshold: f64,
    pub armed: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClauthProfile {
    pub name: String,
    pub active: bool,
    pub tier: Option<String>,
    pub harness: String,
    pub auth_status: String,
    pub fetch_status: Option<String>,
    pub stale: bool,
    pub fallback: Option<Fallback>,
    pub windows: Vec<ClauthWindow>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClauthStatus {
    pub generated_at_ms: Option<f64>,
    pub active_profile: Option<String>,
    pub pending_switch: Option<String>,
    pub refresh_interval_ms: f64,
    pub profiles: Vec<ClauthProfile>,
}

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(String::from)
}

fn parse_profile(v: &Value) -> Option<ClauthProfile> {
    let windows = v
        .get("windows")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|w| {
                    Some(ClauthWindow {
                        label: s(w, "label")?,
                        utilization_pct: w.get("utilization_pct")?.as_f64()?,
                        resets_at: s(w, "resets_at"),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let fallback = v.get("fallback").filter(|f| f.is_object()).map(|f| Fallback {
        position: f.get("position").and_then(Value::as_u64).unwrap_or(0),
        threshold: f.get("threshold").and_then(Value::as_f64).unwrap_or(0.0),
        armed: f.get("armed").and_then(Value::as_bool).unwrap_or(false),
    });
    Some(ClauthProfile {
        name: s(v, "name")?,
        active: v.get("active").and_then(Value::as_bool).unwrap_or(false),
        tier: s(v, "tier"),
        harness: s(v, "harness").unwrap_or_else(|| "claude".into()),
        auth_status: s(v, "auth_status").unwrap_or_else(|| "ok".into()),
        fetch_status: s(v, "fetch_status"),
        stale: v.get("stale").and_then(Value::as_bool).unwrap_or(false),
        fallback,
        windows,
    })
}

/// Parse a status.json body; None when it is not a schema-2 feed object.
pub fn parse_status(raw: &str) -> Option<ClauthStatus> {
    let v: Value = serde_json::from_str(raw).ok()?;
    if !v.is_object() {
        return None;
    }
    // Schema 2 renamed an auth_status value; anything newer may change shapes
    // readers rely on, so only 1 and 2 are read.
    match v.get("schema").and_then(Value::as_u64) {
        Some(1) | Some(2) => {}
        _ => return None,
    }
    let profiles = v.get("profiles").and_then(Value::as_array)?.iter().filter_map(parse_profile).collect();
    Some(ClauthStatus {
        generated_at_ms: s(&v, "generated_at").as_deref().and_then(crate::timefmt::parse_iso),
        active_profile: s(&v, "active_profile"),
        pending_switch: s(&v, "pending_switch"),
        refresh_interval_ms: v.get("refresh_interval_ms").and_then(Value::as_f64).unwrap_or(90_000.0),
        profiles,
    })
}

impl ClauthStatus {
    /// The feed is recent enough to trust.
    pub fn is_current(&self, now_ms: f64) -> bool {
        let max_age = (self.refresh_interval_ms * 3.0).max(MIN_STALE_FEED_MS);
        self.generated_at_ms.is_some_and(|g| now_ms - g <= max_age)
    }

    pub fn profile(&self, name: &str) -> Option<&ClauthProfile> {
        self.profiles.iter().find(|p| p.name == name && p.harness == "claude")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SessionAccount {
    /// A clauth profile's own session.
    Profile(String),
    /// The global `~/.claude` login: clauth's active profile.
    Global,
    /// A config dir clauth does not manage.
    Foreign,
}

/// `runtime`, `runtime-isolated`, `runtime-<pid>-<seq>`, `runtime-isolated-<pid>-<seq>`.
fn is_runtime_dir_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("runtime") else { return false };
    let rest = rest.strip_prefix("-isolated").unwrap_or(rest);
    rest.is_empty()
        || rest.strip_prefix('-').is_some_and(|sid| {
            sid.split_once('-').is_some_and(|(pid, seq)| {
                !pid.is_empty()
                    && !seq.is_empty()
                    && pid.bytes().all(|b| b.is_ascii_digit())
                    && seq.bytes().all(|b| b.is_ascii_digit())
            })
        })
}

fn file_name(p: &Path) -> Option<&str> {
    p.file_name().and_then(|n| n.to_str())
}

/// Which account a session with this `CLAUDE_CONFIG_DIR` runs on.
pub fn session_account(config_dir: Option<&str>, home: &Path) -> SessionAccount {
    let Some(dir) = config_dir.filter(|d| !d.is_empty()) else { return SessionAccount::Global };
    let p = Path::new(dir);
    if let (Some(name), Some(profile_dir)) = (file_name(p), p.parent()) {
        if is_runtime_dir_name(name) && profile_dir.parent().and_then(file_name) == Some("profiles") {
            if let Some(profile) = file_name(profile_dir) {
                return SessionAccount::Profile(profile.to_string());
            }
        }
        let profiles = profile_dir;
        if file_name(profiles) == Some("profiles") && profiles.parent().and_then(file_name) == Some(".clauth") {
            return SessionAccount::Profile(name.to_string());
        }
    }
    let claude = home.join(".claude");
    let same = match (std::fs::canonicalize(p), std::fs::canonicalize(&claude)) {
        (Ok(a), Ok(b)) => a == b,
        _ => p == claude,
    };
    if same {
        SessionAccount::Global
    } else {
        SessionAccount::Foreign
    }
}

/// What a clauth widget renders from.
#[derive(Clone, Debug)]
pub struct Resolved {
    pub status: ClauthStatus,
    pub profile: ClauthProfile,
    pub global: bool,
}

pub fn resolve(status: ClauthStatus, account: &SessionAccount, now_ms: f64) -> Option<Resolved> {
    if !status.is_current(now_ms) {
        return None;
    }
    let (name, global) = match account {
        SessionAccount::Profile(n) => (n.clone(), false),
        SessionAccount::Global => (status.active_profile.clone()?, true),
        SessionAccount::Foreign => return None,
    };
    let profile = status.profile(&name)?.clone();
    Some(Resolved { status, profile, global })
}

pub fn status_path(home: &Path) -> PathBuf {
    home.join(".clauth").join("status.json")
}

static RESOLVED: OnceLock<Option<Resolved>> = OnceLock::new();

/// This session's clauth profile, read once per render.
pub fn current() -> Option<&'static Resolved> {
    RESOLVED
        .get_or_init(|| {
            let home = crate::settings::home_dir();
            let raw = std::fs::read(status_path(&home)).ok()?;
            let status = parse_status(&String::from_utf8_lossy(&raw))?;
            let config_dir = std::env::var("CLAUDE_CONFIG_DIR").ok();
            let account = session_account(config_dir.as_deref(), &home);
            resolve(status, &account, crate::clock::now_ms())
        })
        .as_ref()
}

impl ClauthProfile {
    /// A window row still describing the current window: clauth drops a row
    /// past its reset when it writes the feed, and this drops one that lapsed
    /// since.
    pub fn window(&self, label: &str, now_ms: f64) -> Option<&ClauthWindow> {
        self.windows.iter().find(|w| {
            w.label == label && super::windows::parse_reset_at(w.resets_at.as_deref()).is_none_or(|r| r > now_ms)
        })
    }

    /// The profile's figures as upstream's usage record. A missing (lapsed) 5h
    /// or 7d row reads as 0 % used once the account has any figures at all;
    /// a missing per-model row stays unknown (the plan may have no such quota).
    pub fn usage_data(&self, now_ms: f64) -> Option<UsageData> {
        if self.windows.is_empty() {
            return None;
        }
        let pct = |label: &str| self.window(label, now_ms).map(|w| w.utilization_pct);
        let reset = |label: &str| self.window(label, now_ms).and_then(|w| w.resets_at.clone());
        Some(UsageData {
            session_usage: Some(pct("5h").unwrap_or(0.0)),
            session_reset_at: reset("5h"),
            weekly_usage: Some(pct("7d").unwrap_or(0.0)),
            weekly_reset_at: reset("7d"),
            weekly_sonnet_usage: pct("7d sonnet"),
            weekly_sonnet_reset_at: reset("7d sonnet"),
            weekly_opus_usage: pct("7d opus"),
            weekly_opus_reset_at: reset("7d opus"),
            fable_usage: pct("7d fable"),
            fable_reset_at: reset("7d fable"),
            ..Default::default()
        })
    }
}

/// A profile name fit for display: clauth names are user-chosen, so one that
/// is an email address shows only its local part.
pub fn display_name(name: &str) -> String {
    match name.split_once('@') {
        Some((local, _)) => format!("{local}@…"),
        None => name.to_string(),
    }
}

#[cfg(test)]
pub use tests::FIXTURE as TEST_FIXTURE;

#[cfg(test)]
mod tests {
    use super::*;

    pub const FIXTURE: &str = r#"{
      "schema": 2, "generated_at": "2026-10-21T11:59:00+00:00",
      "active_profile": "work", "pending_switch": null, "wrap_off": false,
      "refresh_interval_ms": 90000, "clauth_version": "0.17.0",
      "profiles": [
        {"name": "profile-a", "active": false, "provider": "anthropic", "tier": "Max 20x", "harness": "claude",
         "auth_status": "ok", "fetch_status": "Cached", "stale": false,
         "fallback": {"position": 2, "threshold": 90.0, "armed": false},
         "windows": [
           {"label": "5h", "utilization_pct": 12.0, "resets_at": "2026-10-21T14:30:00+00:00"},
           {"label": "7d", "utilization_pct": 0.0, "resets_at": "2026-10-27T09:00:00+00:00"},
           {"label": "7d fable", "utilization_pct": 7.5, "resets_at": "2026-10-27T09:00:00+00:00"}]},
        {"name": "work", "active": true, "tier": "Max 5x", "harness": "claude", "auth_status": "ok",
         "fetch_status": "Fresh", "stale": false, "fallback": null,
         "windows": [{"label": "7d", "utilization_pct": 98.0, "resets_at": "2026-10-23T14:00:00+00:00"},
                     {"label": "5h", "utilization_pct": 50.0, "resets_at": "2026-10-21T11:00:00+00:00"}]},
        {"name": "codexy", "harness": "codex", "windows": []}
      ]}"#;

    const NOW: f64 = 1792584000000.0; // 2026-10-21T12:00:00Z

    #[test]
    fn parses_feed() {
        let st = parse_status(FIXTURE).unwrap();
        assert_eq!(st.active_profile.as_deref(), Some("work"));
        assert_eq!(st.profiles.len(), 3);
        assert!(st.is_current(NOW));
        assert!(!st.is_current(NOW + 11.0 * 60_000.0));
        assert!(parse_status(r#"{"schema":3,"profiles":[]}"#).is_none());
        assert!(parse_status("not json").is_none());
        assert!(st.profile("codexy").is_none());
    }

    #[test]
    fn maps_config_dir_to_account() {
        let home = Path::new("/h");
        let acct = |d: &str| session_account(Some(d), home);
        assert_eq!(acct("/h/.clauth/profiles/profile-a/runtime-4242-0"), SessionAccount::Profile("profile-a".into()));
        assert_eq!(acct("/h/.clauth/profiles/profile-a/runtime-4242-0/"), SessionAccount::Profile("profile-a".into()));
        assert_eq!(acct("/h/.clauth/profiles/p/runtime-isolated-1-2"), SessionAccount::Profile("p".into()));
        assert_eq!(acct("/h/.clauth/profiles/p/runtime"), SessionAccount::Profile("p".into()));
        assert_eq!(acct("/h/.clauth/profiles/p"), SessionAccount::Profile("p".into()));
        assert_eq!(acct("/h/.clauth/profiles/p/runtime-x"), SessionAccount::Foreign);
        assert_eq!(acct("/h/.claude"), SessionAccount::Global);
        assert_eq!(acct("/elsewhere"), SessionAccount::Foreign);
        assert_eq!(session_account(None, home), SessionAccount::Global);
        assert_eq!(session_account(Some(""), home), SessionAccount::Global);
    }

    #[test]
    fn resolves_own_profile_not_active() {
        let st = parse_status(FIXTURE).unwrap();
        let r = resolve(st.clone(), &SessionAccount::Profile("profile-a".into()), NOW).unwrap();
        assert_eq!(r.profile.name, "profile-a");
        let d = r.profile.usage_data(NOW).unwrap();
        assert_eq!(d.weekly_usage, Some(0.0));
        assert_eq!(d.session_usage, Some(12.0));
        assert_eq!(d.fable_usage, Some(7.5));
        assert_eq!(d.weekly_opus_usage, None);

        let g = resolve(st.clone(), &SessionAccount::Global, NOW).unwrap();
        assert_eq!(g.profile.name, "work");
        // the 5h row lapsed at 11:00: a new window has not opened yet
        let d = g.profile.usage_data(NOW).unwrap();
        assert_eq!(d.session_usage, Some(0.0));
        assert_eq!(d.session_reset_at, None);
        assert_eq!(d.weekly_usage, Some(98.0));

        assert!(resolve(st.clone(), &SessionAccount::Profile("ghost".into()), NOW).is_none());
        assert!(resolve(st, &SessionAccount::Foreign, NOW).is_none());
    }

    #[test]
    fn masks_email_names() {
        assert_eq!(display_name("me@example.com"), "me@…");
        assert_eq!(display_name("profile-a"), "profile-a");
    }
}
