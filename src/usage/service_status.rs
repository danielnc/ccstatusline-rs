//! Claude service status from status.claude.com (upstream
//! `src/utils/claude-service-status.ts`): a 5-minute shared disk cache plus a
//! 30 s failure lock. Fetches run in the detached refresher.

use super::cache_dir;
use crate::settings::WidgetItem;
use serde_json::{Map, Value};
use std::path::PathBuf;
use std::sync::OnceLock;

const CACHE_MAX_AGE: i64 = 300;
const FAILURE_BACKOFF: i64 = 30;
const STATUS_PATH: &str = "/api/v2/status.json";
const INCIDENTS_PATH: &str = "/api/v2/incidents.json";
pub const BUCKET_COUNT: usize = 8;
pub const BUCKET_MS: f64 = 6.0 * 60.0 * 60.0 * 1000.0;

fn cache_file() -> PathBuf {
    cache_dir().join("claude-status.json")
}

fn lock_file() -> PathBuf {
    cache_dir().join("claude-status.lock")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Impact {
    None,
    Minor,
    Major,
    Critical,
}

impl Impact {
    fn parse(s: &str) -> Option<Impact> {
        Some(match s {
            "minor" => Impact::Minor,
            "major" => Impact::Major,
            "critical" => Impact::Critical,
            _ => return None,
        })
    }
    fn as_str(self) -> &'static str {
        match self {
            Impact::None => "none",
            Impact::Minor => "minor",
            Impact::Major => "major",
            Impact::Critical => "critical",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Incident {
    pub impact: Impact,
    pub start_ms: f64,
    pub end_ms: Option<f64>,
}

/// `ClaudeServiceStatusData`
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StatusData {
    pub indicator: Option<String>,
    pub incidents: Option<Vec<Incident>>,
    pub error: bool,
}

/// `computeIncidentHistoryBuckets`
pub fn history_buckets(incidents: &[Incident], now_ms: f64) -> Vec<Impact> {
    (0..BUCKET_COUNT)
        .map(|i| {
            let start = now_ms - (BUCKET_COUNT - i) as f64 * BUCKET_MS;
            let end = start + BUCKET_MS;
            incidents
                .iter()
                .filter(|inc| inc.start_ms < end && inc.end_ms.unwrap_or(now_ms) > start)
                .map(|inc| inc.impact)
                .max()
                .unwrap_or(Impact::None)
        })
        .collect()
}

fn parse_json(raw: &str) -> Option<Value> {
    serde_json::from_str(raw).ok()
}

/// `parseClaudeStatusResponse`
pub fn parse_status_response(raw: &str) -> Option<String> {
    let v = parse_json(raw)?;
    let o = v.as_object()?;
    match o.get("status") {
        None | Some(Value::Null) => None,
        Some(Value::Object(s)) => match s.get("indicator") {
            None | Some(Value::Null) => None,
            Some(Value::String(i)) => Some(i.clone()),
            Some(_) => None,
        },
        Some(_) => None,
    }
}

fn opt_str<'a>(o: &'a Map<String, Value>, k: &str) -> Result<Option<&'a str>, ()> {
    match o.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(()),
    }
}

fn ts(v: Option<&str>) -> Option<f64> {
    v.filter(|s| !s.is_empty()).and_then(crate::timefmt::parse_iso)
}

/// `parseClaudeIncidentsResponse`
pub fn parse_incidents_response(raw: &str) -> Option<Vec<Incident>> {
    let v = parse_json(raw)?;
    let o = v.as_object()?;
    let list = match o.get("incidents") {
        None | Some(Value::Null) => return Some(vec![]),
        Some(Value::Array(a)) => a,
        Some(_) => return None,
    };
    let mut parsed = Vec::new();
    for inc in list {
        let io = inc.as_object()?;
        let impact = opt_str(io, "impact").ok()?;
        let started = opt_str(io, "started_at").ok()?;
        let created = opt_str(io, "created_at").ok()?;
        let resolved = opt_str(io, "resolved_at").ok()?;
        parsed.push((impact, started, created, resolved));
    }
    let mut out = Vec::new();
    for (impact, started, created, resolved) in parsed {
        let Some(impact) = Impact::parse(impact.unwrap_or("")) else { continue };
        let Some(start_ms) = ts(started).or_else(|| ts(created)) else { continue };
        out.push(Incident { impact, start_ms, end_ms: ts(resolved) });
    }
    Some(out)
}

#[derive(Clone, Debug, PartialEq)]
struct Cached {
    fetched_at: f64,
    indicator: Option<String>,
    incidents: Option<Vec<Incident>>,
    incidents_queried: bool,
}

/// `CachedClaudeStatusSchema`
fn parse_cached(raw: &str) -> Option<Cached> {
    let v = parse_json(raw)?;
    let o = v.as_object()?;
    let fetched_at = o.get("fetchedAt")?.as_f64()?;
    let indicator = opt_str(o, "indicator").ok()?.map(String::from);
    let incidents = match o.get("incidents") {
        None | Some(Value::Null) => None,
        Some(Value::Array(a)) => {
            let mut out = Vec::new();
            for i in a {
                let io = i.as_object()?;
                let impact = Impact::parse(io.get("impact")?.as_str()?)?;
                let start_ms = io.get("startMs")?.as_f64()?;
                let end_ms = match io.get("endMs")? {
                    Value::Null => None,
                    v => Some(v.as_f64()?),
                };
                out.push(Incident { impact, start_ms, end_ms });
            }
            Some(out)
        }
        Some(_) => return None,
    };
    let incidents_queried = o.get("incidentsQueried")?.as_bool()?;
    Some(Cached { fetched_at, indicator, incidents, incidents_queried })
}

fn serialize_cached(c: &Cached) -> String {
    let mut o = Map::new();
    o.insert("fetchedAt".into(), crate::jsjson::num(c.fetched_at));
    o.insert("indicator".into(), c.indicator.clone().map(Value::String).unwrap_or(Value::Null));
    let incidents = c.incidents.as_ref().map(|list| {
        Value::Array(
            list.iter()
                .map(|i| {
                    let mut m = Map::new();
                    m.insert("impact".into(), Value::String(i.impact.as_str().into()));
                    m.insert("startMs".into(), crate::jsjson::num(i.start_ms));
                    m.insert("endMs".into(), i.end_ms.map(crate::jsjson::num).unwrap_or(Value::Null));
                    Value::Object(m)
                })
                .collect(),
        )
    });
    o.insert("incidents".into(), incidents.unwrap_or(Value::Null));
    o.insert("incidentsQueried".into(), Value::Bool(c.incidents_queried));
    crate::jsjson::stringify(&Value::Object(o))
}

fn to_status(c: &Cached) -> StatusData {
    StatusData { indicator: c.indicator.clone(), incidents: c.incidents.clone(), error: false }
}

fn usable(c: &Cached, include_incidents: bool) -> bool {
    !include_incidents || c.incidents_queried
}

fn mtime_secs(path: &std::path::Path) -> Option<i64> {
    let m = std::fs::metadata(path).ok()?.modified().ok()?;
    let ms = match m.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_secs_f64() * 1000.0,
        Err(e) => -(e.duration().as_secs_f64() * 1000.0),
    };
    Some((ms / 1000.0).floor() as i64)
}

fn failure_lock_active(now_secs: i64) -> bool {
    mtime_secs(&lock_file()).is_some_and(|m| now_secs - m < FAILURE_BACKOFF)
}

fn write_failure_lock() {
    let _ = crate::cachefile::atomic_write(&lock_file(), b"");
}

fn fresh(c: &Cached, now_secs: i64) -> bool {
    now_secs - ((c.fetched_at / 1000.0).floor() as i64) < CACHE_MAX_AGE
}

/// `fetchClaudeServiceStatus(includeIncidents)`, render side.
pub fn fetch_status(include_incidents: bool) -> StatusData {
    let now_ms = crate::clock::now_ms();
    let now_secs = (now_ms / 1000.0).floor() as i64;
    let disk = std::fs::read(cache_file()).ok().and_then(|b| parse_cached(&String::from_utf8_lossy(&b)));
    if let Some(c) = &disk {
        if fresh(c, now_secs) && usable(c, include_incidents) {
            return to_status(c);
        }
    }
    let stale_or_error = || match &disk {
        Some(c) if usable(c, include_incidents) => to_status(c),
        _ => StatusData { error: true, ..Default::default() },
    };
    if failure_lock_active(now_secs) {
        return stale_or_error();
    }
    if super::test_no_network() {
        // both requests fail: indicator null
        write_failure_lock();
        return stale_or_error();
    }
    // one refresher machine-wide; upstream fetches from every render that
    // finds the cache expired
    if !super::test_no_background() && super::refresh::take_status_refresh_lock() {
        super::refresh::spawn_claude_status_refresh(include_incidents);
    }
    stale_or_error()
}

/// The detached refresher: fetch, then write the cache or the failure lock.
pub fn run_refresh(include_incidents: bool) {
    refresh_once(include_incidents);
    super::refresh::release_status_refresh_lock();
}

fn refresh_once(include_incidents: bool) {
    let now_ms = crate::clock::now_ms();
    let status = super::refresh::fetch_status_page(STATUS_PATH);
    let incidents_body = if include_incidents { super::refresh::fetch_status_page(INCIDENTS_PATH) } else { None };
    let indicator = status.as_deref().and_then(parse_status_response);
    let incidents = incidents_body.as_deref().and_then(parse_incidents_response);
    if indicator.is_none() || (include_incidents && incidents.is_none()) {
        write_failure_lock();
        return;
    }
    let _ = std::fs::remove_file(lock_file());
    let c = Cached { fetched_at: now_ms, indicator, incidents, incidents_queried: include_incidents };
    let _ = crate::cachefile::atomic_write(&cache_file(), serialize_cached(&c).as_bytes());
}

pub const WIDGET_TYPE: &str = "claude-status";

pub fn history_enabled(item: &WidgetItem) -> bool {
    item.kind == WIDGET_TYPE && item.meta("history") == Some("true")
}

static STATUS: OnceLock<Option<StatusData>> = OnceLock::new();

/// `context.claudeStatusData` (`prefetchClaudeStatusIfNeeded`).
pub fn status_data(lines: &[Vec<WidgetItem>]) -> Option<&'static StatusData> {
    STATUS
        .get_or_init(|| {
            if !lines.iter().any(|l| l.iter().any(|i| i.kind == WIDGET_TYPE)) {
                return None;
            }
            let incidents = lines.iter().any(|l| l.iter().any(history_enabled));
            Some(fetch_status(incidents))
        })
        .as_ref()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets() {
        let now = 1_000_000_000_000.0;
        let inc = vec![
            Incident {
                impact: Impact::Minor,
                start_ms: now - 47.0 * 3_600_000.0,
                end_ms: Some(now - 46.0 * 3_600_000.0),
            },
            Incident { impact: Impact::Critical, start_ms: now - 1000.0, end_ms: None },
            Incident {
                impact: Impact::Major,
                start_ms: now - 5.0 * 3_600_000.0,
                end_ms: Some(now - 4.0 * 3_600_000.0),
            },
        ];
        let b = history_buckets(&inc, now);
        assert_eq!(b[0], Impact::Minor);
        assert_eq!(b[7], Impact::Critical);
        assert_eq!(b[1], Impact::None);
    }

    #[test]
    fn cache_roundtrip() {
        let c = Cached {
            fetched_at: 1792584000000.0,
            indicator: Some("minor".into()),
            incidents: Some(vec![Incident { impact: Impact::Major, start_ms: 1.0, end_ms: None }]),
            incidents_queried: true,
        };
        let s = serialize_cached(&c);
        assert_eq!(
            s,
            r#"{"fetchedAt":1792584000000,"indicator":"minor","incidents":[{"impact":"major","startMs":1,"endMs":null}],"incidentsQueried":true}"#
        );
        assert_eq!(parse_cached(&s), Some(c));
    }

    #[test]
    fn responses() {
        assert_eq!(parse_status_response(r#"{"status":{"indicator":"none"}}"#).as_deref(), Some("none"));
        assert_eq!(parse_status_response(r#"{"status":5}"#), None);
        let inc = parse_incidents_response(
            r#"{"incidents":[{"impact":"none","started_at":"2026-10-21T00:00:00Z"},{"impact":"major","created_at":"2026-10-21T00:00:00Z","resolved_at":null}]}"#,
        )
        .unwrap();
        assert_eq!(inc.len(), 1);
        assert_eq!(inc[0].end_ms, None);
    }
}
