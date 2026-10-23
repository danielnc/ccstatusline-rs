//! The usage fetch state machine (upstream `src/utils/usage-fetch.ts`):
//! credentials, the shared file cache, the JSON lock and the API response
//! parser.
//!
//! Upstream awaits the HTTPS request inside the render. Here the render only
//! decides, with the same cache/lock rules, whether a fetch is due; the fetch
//! itself runs in a detached refresher (see `refresh.rs`) and this render
//! answers like a concurrent upstream render that finds the in-flight lock:
//! the stale cache, else `[Timeout]`. Under `CCSL_TEST_NO_NETWORK=1` the fetch
//! fails inline, as upstream's does when the harness blocks the request.

use super::{cache_dir, sha256, Field, UsageData, UsageError, ALL_FIELDS, WEEKLY_MODEL_USAGE_BUCKETS};
use serde_json::{Map, Value};
use std::path::PathBuf;

pub const CACHE_MAX_AGE: i64 = 180;
pub const LOCK_MAX_AGE: i64 = 30;
pub const DEFAULT_RATE_LIMIT_BACKOFF: i64 = 300;
const MAX_LOCK_HORIZON: i64 = 24 * 60 * 60;
pub const MACOS_USAGE_CREDENTIALS_SERVICE: &str = "Claude Code-credentials";
const MACOS_SECURITY_TIMEOUT_MS: u64 = 5000;
/// How long a keychain-derived identity is trusted before `security` runs again.
const IDENTITY_CACHE_TTL_MS: f64 = 60_000.0;

pub fn cache_file() -> PathBuf {
    cache_dir().join("usage.json")
}

pub fn lock_file() -> PathBuf {
    cache_dir().join("usage.lock")
}

fn no_credentials_lock_file() -> PathBuf {
    cache_dir().join("usage-credentials.lock")
}

/// The account a session's credentials come from, named without reading them:
/// a hash of the credential source (keychain service + config dir).
pub fn account_key(source: &str) -> String {
    sha256::hex_prefix(source, 16)
}

/// Per-account copy of the cache (not upstream): sessions of different
/// accounts sharing `usage.json` evict each other through the token hash, so
/// the refresher also keeps one file per account.
pub fn account_cache_file(key: &str) -> PathBuf {
    cache_dir().join("usage-by-account").join(format!("{key}.json"))
}

/// Per-account fetch lock (upstream: the single `usage.lock`).
pub fn account_lock_file(key: &str) -> PathBuf {
    cache_dir().join("usage-by-account").join(format!("{key}.lock"))
}

fn now_secs() -> i64 {
    (crate::clock::now_ms() / 1000.0).floor() as i64
}

// ------------------------------------------------------------ JSON helpers

fn num(o: &Map<String, Value>, k: &str) -> Result<Option<f64>, ()> {
    match o.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => Ok(n.as_f64()),
        Some(_) => Err(()),
    }
}

fn string(o: &Map<String, Value>, k: &str) -> Result<Option<String>, ()> {
    match o.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(()),
    }
}

fn boolean(o: &Map<String, Value>, k: &str) -> Result<Option<bool>, ()> {
    match o.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(()),
    }
}

fn parse_json(raw: &str) -> Option<Value> {
    // JSON.parse accepts no BOM; serde_json agrees
    serde_json::from_str(raw).ok()
}

// ------------------------------------------------------------ credentials

#[derive(Clone)]
pub struct Credentials {
    pub access_token: String,
    pub refresh_token: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Identity {
    pub preferred_hash: String,
    pub access_token_hash: String,
}

/// `parseUsageCredentials`
pub fn parse_credentials(raw: &str) -> Option<Credentials> {
    let v = parse_json(raw)?;
    let o = v.as_object()?;
    let oauth = match o.get("claudeAiOauth") {
        None => return None,
        Some(Value::Object(m)) => m,
        Some(_) => return None,
    };
    let access = string(oauth, "accessToken").ok()?;
    let refresh = string(oauth, "refreshToken").ok()?;
    let access = access.filter(|s| !s.is_empty())?;
    Some(Credentials { access_token: access, refresh_token: refresh })
}

fn fingerprint(token: &str) -> String {
    sha256::hex_prefix(token, 16)
}

/// `getUsageCacheIdentity`
pub fn identity_of(c: &Credentials) -> Identity {
    let access_token_hash = fingerprint(&c.access_token);
    let preferred_hash = match &c.refresh_token {
        Some(r) if !r.is_empty() => fingerprint(r),
        _ => access_token_hash.clone(),
    };
    Identity { preferred_hash, access_token_hash }
}

fn read_credentials_file() -> Option<Credentials> {
    let path = crate::claude_config::config_dir().join(".credentials.json");
    let raw = std::fs::read(path).ok()?;
    parse_credentials(&String::from_utf8_lossy(&raw))
}

/// `getMacKeychainConfigDirService` (the NFC normalization is skipped: config
/// dirs are ASCII in practice).
pub fn keychain_config_dir_service() -> Option<String> {
    let raw = match std::env::var("CLAUDE_SECURESTORAGE_CONFIG_DIR") {
        Ok(v) => v,
        Err(_) => std::env::var("CLAUDE_CONFIG_DIR").unwrap_or_default(),
    };
    if raw.is_empty() {
        return None;
    }
    Some(format!("{MACOS_USAGE_CREDENTIALS_SERVICE}-{}", sha256::hex_prefix(&raw, 8)))
}

fn keychain_available() -> bool {
    cfg!(target_os = "macos") && !super::test_no_keychain()
}

/// Run `security …` with a timeout; stdout on success.
fn run_security(args: &[&str]) -> Option<String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    if !keychain_available() {
        return None;
    }
    let mut child = Command::new("security")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut out = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out.read_to_end(&mut buf);
        buf
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(MACOS_SECURITY_TIMEOUT_MS);
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(5)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let buf = reader.join().ok()?;
    if !status?.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&buf).into_owned())
}

fn keychain_secret(service: &str) -> Option<String> {
    run_security(&["find-generic-password", "-s", service, "-w"]).map(|s| crate::jsnum::js_trim(&s).to_string())
}

fn keychain_credentials(service: &str) -> Option<Credentials> {
    keychain_secret(service).filter(|s| !s.is_empty()).and_then(|s| parse_credentials(&s))
}

/// `parseMacKeychainCredentialCandidates`
pub fn parse_keychain_candidates(dump: &str) -> Vec<String> {
    struct Cand {
        modified: Option<String>,
        order: usize,
        service: String,
    }
    fn cmp(a: &Cand, b: &Cand) -> std::cmp::Ordering {
        use std::cmp::Ordering::*;
        match (&a.modified, &b.modified) {
            (Some(x), Some(y)) if x != y => y.cmp(x),
            (Some(_), None) => Less,
            (None, Some(_)) => Greater,
            _ => a.order.cmp(&b.order),
        }
    }
    let mut blocks: Vec<&str> = Vec::new();
    let mut start = 0;
    for (i, _) in dump.match_indices("keychain:") {
        let at_line_start = i == 0 || dump.as_bytes()[i - 1] == b'\n';
        let followed_by_space = dump[i + 9..].starts_with(|c: char| c.is_whitespace());
        if at_line_start && followed_by_space && i > start {
            blocks.push(&dump[start..i]);
            start = i;
        }
    }
    blocks.push(&dump[start..]);
    let mut out: Vec<Cand> = Vec::new();
    let mut order = 0;
    for block in blocks.into_iter().filter(|b| !crate::jsnum::js_trim(b).is_empty()) {
        let Some(service) = capture_after(block, "\"svce\"<blob>=\"") else { continue };
        if !service.starts_with(MACOS_USAGE_CREDENTIALS_SERVICE) || service == MACOS_USAGE_CREDENTIALS_SERVICE {
            continue;
        }
        let c = Cand { modified: keychain_modified(block), order, service };
        order += 1;
        match out.iter().position(|e| e.service == c.service) {
            Some(i) => {
                if cmp(&c, &out[i]) == std::cmp::Ordering::Less {
                    out[i] = c;
                }
            }
            None => out.push(c),
        }
    }
    out.sort_by(cmp);
    out.into_iter().map(|c| c.service).collect()
}

fn capture_after(block: &str, prefix: &str) -> Option<String> {
    let i = block.find(prefix)? + prefix.len();
    let rest = &block[i..];
    let end = rest.find('"')?;
    (end > 0).then(|| rest[..end].to_string())
}

fn normalize_timedate(raw: &str) -> Option<String> {
    let cleaned = raw.replace("\\000", "").replace('\0', "");
    let t = crate::jsnum::js_trim(&cleaned);
    (t.len() == 15 && t.as_bytes()[..14].iter().all(|b| b.is_ascii_digit()) && t.ends_with('Z')).then(|| t.to_string())
}

fn keychain_modified(block: &str) -> Option<String> {
    const KEY: &str = "\"mdat\"<timedate>=";
    let i = block.find(KEY)? + KEY.len();
    let rest = &block[i..];
    // quoted form, optionally after a hex blob
    let quoted = {
        let mut r = rest;
        if let Some(hex) = r.strip_prefix("0x") {
            let n = hex.bytes().take_while(|b| b.is_ascii_hexdigit()).count();
            let after = &hex[n..];
            let ws = after.len() - after.trim_start().len();
            if n > 0 && ws > 0 {
                r = &after[ws..];
            }
        }
        r.strip_prefix('"').and_then(|q| q.find('"').filter(|e| *e > 0).map(|e| q[..e].to_string()))
    };
    if let Some(v) = quoted.and_then(|q| normalize_timedate(&q)) {
        return Some(v);
    }
    let hex = rest.strip_prefix("0x")?;
    let n = hex.bytes().take_while(|b| b.is_ascii_hexdigit()).count();
    let hex = &hex[..n];
    if hex.is_empty() || hex.len() % 2 != 0 {
        return None;
    }
    let decoded: String =
        (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(0) as char).collect();
    normalize_timedate(&decoded)
}

fn keychain_candidates_credentials() -> Option<Credentials> {
    let dump = run_security(&["dump-keychain"])?;
    parse_keychain_candidates(&dump).iter().find_map(|s| keychain_credentials(s))
}

/// `getUsageCredentials`
pub fn usage_credentials() -> Option<Credentials> {
    if !cfg!(target_os = "macos") {
        return read_credentials_file();
    }
    if let Some(service) = keychain_config_dir_service() {
        return keychain_credentials(&service).or_else(read_credentials_file);
    }
    keychain_credentials(MACOS_USAGE_CREDENTIALS_SERVICE)
        .or_else(keychain_candidates_credentials)
        .or_else(read_credentials_file)
}

/// `getUsageCredentialsSource`: `JSON.stringify([service, configDir])`
pub fn credentials_source() -> String {
    let service = keychain_config_dir_service().map(Value::String).unwrap_or(Value::Null);
    let dir = Value::String(crate::claude_config::config_dir().to_string_lossy().into_owned());
    crate::jsjson::stringify(&Value::Array(vec![service, dir]))
}

/// What a render needs to know about the credentials: whether there is a token
/// and its cache identity. Never the token itself.
#[derive(Clone, Debug, PartialEq)]
pub struct CredentialState {
    pub identity: Option<Identity>,
}

fn identity_cache_file(source: &str) -> PathBuf {
    cache_dir().join(format!("usage-identity-{}.json", sha256::hex_prefix(source, 16)))
}

/// Credentials lookup for the render path. A lookup that needs the keychain
/// (a `security` subprocess) is remembered per credential source for a minute.
fn credential_state(source: &str) -> CredentialState {
    if !keychain_available() {
        return CredentialState { identity: usage_credentials().map(|c| identity_of(&c)) };
    }
    let path = identity_cache_file(source);
    let now = crate::clock::now_ms();
    if let Some(v) = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) {
        let at = v.get("checkedAt").and_then(Value::as_f64);
        if at.is_some_and(|t| t <= now && now - t < IDENTITY_CACHE_TTL_MS) {
            let p = v.get("preferredHash").and_then(Value::as_str);
            let a = v.get("accessTokenHash").and_then(Value::as_str);
            let identity = match (p, a) {
                (Some(p), Some(a)) => Some(Identity { preferred_hash: p.into(), access_token_hash: a.into() }),
                _ => None,
            };
            return CredentialState { identity };
        }
    }
    let identity = usage_credentials().map(|c| identity_of(&c));
    let mut o = Map::new();
    o.insert("checkedAt".into(), Value::from(now));
    if let Some(i) = &identity {
        o.insert("preferredHash".into(), Value::String(i.preferred_hash.clone()));
        o.insert("accessTokenHash".into(), Value::String(i.access_token_hash.clone()));
    }
    let _ = crate::cachefile::atomic_write(&path, Value::Object(o).to_string().as_bytes());
    CredentialState { identity }
}

/// `getUsageCredentialsWithBackoff`
fn credentials_with_backoff(now: i64, source: &str) -> CredentialState {
    let source = source.to_string();
    if let Ok(raw) = std::fs::read(no_credentials_lock_file()) {
        if let Some(Value::Object(o)) = parse_json(&String::from_utf8_lossy(&raw)) {
            let blocked = o.get("blockedUntil").and_then(Value::as_f64);
            let src = o.get("source").and_then(Value::as_str);
            if let (Some(b), Some(s)) = (blocked, src) {
                if s == source && b > now as f64 && b <= (now + LOCK_MAX_AGE) as f64 {
                    return CredentialState { identity: None };
                }
            }
        }
    }
    let state = credential_state(&source);
    if state.identity.is_none() {
        let mut o = Map::new();
        o.insert("blockedUntil".into(), Value::from(now + LOCK_MAX_AGE));
        o.insert("source".into(), Value::String(source));
        let _ = crate::cachefile::atomic_write(&no_credentials_lock_file(), Value::Object(o).to_string().as_bytes());
    }
    state
}

// ------------------------------------------------------------ cache file

/// `parseCachedUsageData`
pub fn parse_cached(raw: &str) -> Option<UsageData> {
    let v = parse_json(raw)?;
    let o = v.as_object()?;
    let n = |k: &str| num(o, k);
    let s = |k: &str| string(o, k);
    let d = UsageData {
        session_usage: n("sessionUsage").ok()?,
        session_reset_at: s("sessionResetAt").ok()?,
        weekly_usage: n("weeklyUsage").ok()?,
        weekly_reset_at: s("weeklyResetAt").ok()?,
        weekly_sonnet_usage: n("weeklySonnetUsage").ok()?,
        weekly_sonnet_reset_at: s("weeklySonnetResetAt").ok()?,
        weekly_opus_usage: n("weeklyOpusUsage").ok()?,
        weekly_opus_reset_at: s("weeklyOpusResetAt").ok()?,
        fable_usage: n("fableUsage").ok()?,
        fable_reset_at: s("fableResetAt").ok()?,
        extra_usage_enabled: boolean(o, "extraUsageEnabled").ok()?,
        extra_usage_limit: n("extraUsageLimit").ok()?,
        extra_usage_used: n("extraUsageUsed").ok()?,
        extra_usage_utilization: n("extraUsageUtilization").ok()?,
        extra_usage_currency: s("extraUsageCurrency").ok()?,
        error: s("error").ok()?.as_deref().and_then(UsageError::parse),
    };
    Some(d)
}

/// `readCachedTokenHash`: the hash when the file is an object whose `tokenHash`
/// is a string or absent; any other shape reads as no hash.
fn cached_token_hash(raw: &str) -> Option<String> {
    let v = parse_json(raw)?;
    let o = v.as_object()?;
    match o.get("tokenHash") {
        Some(Value::String(s)) => Some(s.clone()),
        _ => None,
    }
}

fn token_hash_matches(cached: Option<&str>, identity: Option<&Identity>) -> bool {
    match identity {
        None => true,
        Some(i) => cached.is_some_and(|h| h == i.preferred_hash || h == i.access_token_hash),
    }
}

/// Serialize as upstream's `JSON.stringify({...usageData, tokenHash})`.
pub fn serialize_cache(d: &UsageData, token_hash: Option<&str>) -> String {
    let mut o = Map::new();
    let put_n = |o: &mut Map<String, Value>, k: &str, v: Option<f64>| {
        if let Some(x) = v {
            o.insert(k.into(), crate::jsjson::num(x));
        }
    };
    put_n(&mut o, "sessionUsage", d.session_usage);
    if let Some(s) = &d.session_reset_at {
        o.insert("sessionResetAt".into(), Value::String(s.clone()));
    }
    put_n(&mut o, "weeklyUsage", d.weekly_usage);
    if let Some(s) = &d.weekly_reset_at {
        o.insert("weeklyResetAt".into(), Value::String(s.clone()));
    }
    if let Some(b) = d.extra_usage_enabled {
        o.insert("extraUsageEnabled".into(), Value::Bool(b));
    }
    put_n(&mut o, "extraUsageLimit", d.extra_usage_limit);
    put_n(&mut o, "extraUsageUsed", d.extra_usage_used);
    put_n(&mut o, "extraUsageUtilization", d.extra_usage_utilization);
    if let Some(s) = &d.extra_usage_currency {
        o.insert("extraUsageCurrency".into(), Value::String(s.clone()));
    }
    let pairs = [
        ("weeklySonnetUsage", d.weekly_sonnet_usage, "weeklySonnetResetAt", &d.weekly_sonnet_reset_at),
        ("weeklyOpusUsage", d.weekly_opus_usage, "weeklyOpusResetAt", &d.weekly_opus_reset_at),
        ("fableUsage", d.fable_usage, "fableResetAt", &d.fable_reset_at),
    ];
    for (uk, uv, rk, rv) in pairs {
        put_n(&mut o, uk, uv);
        if let Some(s) = rv {
            o.insert(rk.into(), Value::String(s.clone()));
        }
    }
    if let Some(h) = token_hash {
        o.insert("tokenHash".into(), Value::String(h.into()));
    }
    crate::jsjson::stringify(&Value::Object(o))
}

// ------------------------------------------------------------ API response

fn bucket_utilization(b: Option<&Value>) -> Option<f64> {
    match b {
        Some(Value::Null) => Some(0.0),
        Some(Value::Object(o)) => o.get("utilization").and_then(Value::as_f64),
        _ => None,
    }
}

fn bucket_resets_at(b: Option<&Value>) -> Option<String> {
    b.and_then(Value::as_object).and_then(|o| o.get("resets_at")).and_then(Value::as_str).map(String::from)
}

struct Limit<'a> {
    kind: Option<&'a str>,
    percent: Option<f64>,
    resets_at: Option<&'a str>,
    model_display_name: Option<&'a str>,
}

impl Limit<'_> {
    fn is_placeholder(&self) -> bool {
        if self.model_display_name.is_some_and(|s| !s.is_empty()) {
            return false;
        }
        self.percent.unwrap_or(0.0) == 0.0 && self.resets_at.is_none()
    }
    fn percent(&self) -> Option<f64> {
        if self.is_placeholder() {
            None
        } else {
            self.percent
        }
    }
    fn resets_at(&self) -> Option<String> {
        if self.is_placeholder() {
            None
        } else {
            self.resets_at.map(String::from)
        }
    }
}

fn opt_str_field<'a>(o: &'a Map<String, Value>, k: &str) -> Result<Option<&'a str>, ()> {
    match o.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.as_str())),
        Some(_) => Err(()),
    }
}

fn opt_num_field(o: &Map<String, Value>, k: &str) -> Result<Option<f64>, ()> {
    num(o, k)
}

/// zod validation of one `limits[]` entry; `scope` falls back to null on any
/// invalid shape (`.catch(null)`).
fn parse_limit(v: &Value) -> Result<Limit<'_>, ()> {
    let o = v.as_object().ok_or(())?;
    let kind = opt_str_field(o, "kind")?;
    let percent = opt_num_field(o, "percent")?;
    let resets_at = opt_str_field(o, "resets_at")?;
    let model_display_name = (|| -> Result<Option<&str>, ()> {
        let scope = match o.get("scope") {
            None | Some(Value::Null) => return Ok(None),
            Some(Value::Object(s)) => s,
            Some(_) => return Err(()),
        };
        let model = match scope.get("model") {
            None | Some(Value::Null) => return Ok(None),
            Some(Value::Object(m)) => m,
            Some(_) => return Err(()),
        };
        opt_str_field(model, "display_name")
    })()
    .unwrap_or(None);
    Ok(Limit { kind, percent, resets_at, model_display_name })
}

fn valid_bucket(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => true,
        Some(Value::Object(o)) => opt_num_field(o, "utilization").is_ok() && opt_str_field(o, "resets_at").is_ok(),
        Some(_) => false,
    }
}

/// `parseUsageApiResponse`
pub fn parse_api_response(raw: &str) -> Option<UsageData> {
    let v = parse_json(raw)?;
    let o = v.as_object()?;
    for k in ["five_hour", "seven_day", "seven_day_sonnet", "seven_day_opus"] {
        if !valid_bucket(o.get(k)) {
            return None;
        }
    }
    let limits: Vec<Limit> = match o.get("limits") {
        None | Some(Value::Null) => vec![],
        Some(Value::Array(a)) => a.iter().map(parse_limit).collect::<Result<_, _>>().ok()?,
        Some(_) => return None,
    };
    let extra = match o.get("extra_usage") {
        None | Some(Value::Null) => None,
        Some(Value::Object(e)) => Some(e),
        Some(_) => return None,
    };
    let (enabled, limit, used, util, currency) = match extra {
        None => (None, None, None, None, None),
        Some(e) => (
            boolean(e, "is_enabled").ok()?,
            num(e, "monthly_limit").ok()?,
            num(e, "used_credits").ok()?,
            num(e, "utilization").ok()?,
            string(e, "currency").ok()?,
        ),
    };
    let find = |kind: &str| limits.iter().find(|l| l.kind == Some(kind));
    let session = find("session");
    let weekly = find("weekly_all");
    let mut r = UsageData {
        session_usage: bucket_utilization(o.get("five_hour")).or_else(|| session.and_then(|l| l.percent())),
        session_reset_at: bucket_resets_at(o.get("five_hour")).or_else(|| session.and_then(|l| l.resets_at())),
        weekly_usage: bucket_utilization(o.get("seven_day")).or_else(|| weekly.and_then(|l| l.percent())),
        weekly_reset_at: bucket_resets_at(o.get("seven_day")).or_else(|| weekly.and_then(|l| l.resets_at())),
        extra_usage_enabled: enabled,
        extra_usage_limit: limit,
        extra_usage_used: used,
        extra_usage_utilization: util,
        extra_usage_currency: currency,
        ..Default::default()
    };
    for b in &WEEKLY_MODEL_USAGE_BUCKETS {
        let needle = b.model_display_name.to_lowercase();
        let scoped = limits.iter().find(|l| {
            l.kind == Some("weekly_scoped") && l.model_display_name.unwrap_or("").to_lowercase().contains(&needle)
        });
        let legacy = b.api_bucket_key.and_then(|k| o.get(k));
        super::set_usage(
            &mut r,
            b.usage_field,
            scoped.and_then(|l| l.percent()).or_else(|| bucket_utilization(legacy)),
        );
        super::set_reset(
            &mut r,
            b.reset_field,
            scoped.and_then(|l| l.resets_at()).or_else(|| bucket_resets_at(legacy)),
        );
    }
    Some(r)
}

// ------------------------------------------------------------ requirements

fn window_sentinel(f: Field) -> Option<Field> {
    match f {
        Field::SessionResetAt => Some(Field::SessionUsage),
        Field::WeeklyResetAt => Some(Field::WeeklyUsage),
        _ => WEEKLY_MODEL_USAGE_BUCKETS.iter().find(|b| b.reset_field == f).map(|b| b.usage_field),
    }
}

pub fn has_required_field(d: &UsageData, f: Field) -> bool {
    if d.has(f) {
        return true;
    }
    if window_sentinel(f).is_some_and(|s| d.has(s)) {
        return true;
    }
    if d.extra_usage_enabled.is_some()
        && matches!(f, Field::ExtraUsageLimit | Field::ExtraUsageUsed | Field::ExtraUsageUtilization)
    {
        return true;
    }
    (d.session_usage.is_some() || d.weekly_usage.is_some()) && matches!(f, Field::FableUsage | Field::FableResetAt)
}

pub fn has_required_fields(d: &UsageData, required: &[Field]) -> bool {
    required.iter().all(|f| has_required_field(d, *f))
}

// ------------------------------------------------------------ locks

#[derive(Clone, Copy)]
pub enum LockError {
    Timeout,
    RateLimited,
    ParseError,
}

impl LockError {
    fn as_str(self) -> &'static str {
        match self {
            LockError::Timeout => "timeout",
            LockError::RateLimited => "rate-limited",
            LockError::ParseError => "parse-error",
        }
    }
}

fn lock_contents(blocked_until: i64, error: LockError) -> String {
    format!("{{\"blockedUntil\":{blocked_until},\"error\":\"{}\"}}", error.as_str())
}

/// `writeUsageLock`, into this account's lock file (upstream has one lock for
/// every account, so one profile's 429 blocked them all).
pub fn write_account_lock(key: &str, blocked_until: i64, error: LockError) {
    let _ = crate::cachefile::atomic_write(&account_lock_file(key), lock_contents(blocked_until, error).as_bytes());
}

/// `clearOwnUsageLock`
pub fn clear_own_account_lock(key: &str, own: &str) {
    let path = account_lock_file(key);
    if std::fs::read(&path).is_ok_and(|b| b == own.as_bytes()) {
        let _ = std::fs::remove_file(path);
    }
}

fn mtime_secs(path: &std::path::Path) -> Option<i64> {
    let m = std::fs::metadata(path).ok()?.modified().ok()?;
    let ms = match m.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_secs_f64() * 1000.0,
        Err(e) => -(e.duration().as_secs_f64() * 1000.0),
    };
    Some((ms / 1000.0).floor() as i64)
}

/// `readActiveUsageLock` for one lock file.
fn read_lock(path: &std::path::Path, now: i64) -> Option<(i64, UsageError)> {
    if let Ok(raw) = std::fs::read(path) {
        if let Some(Value::Object(o)) = parse_json(&String::from_utf8_lossy(&raw)) {
            let blocked = o.get("blockedUntil").and_then(Value::as_f64);
            let err_ok = match o.get("error") {
                None => Some(None),
                Some(Value::String(s)) if matches!(s.as_str(), "timeout" | "rate-limited" | "parse-error") => {
                    Some(UsageError::parse(s))
                }
                _ => None,
            };
            if let (Some(b), Some(err)) = (blocked, err_ok) {
                if b > now as f64 && b <= (now + MAX_LOCK_HORIZON) as f64 {
                    return Some((b as i64, err.unwrap_or(UsageError::Timeout)));
                }
                return None;
            }
        }
    }
    let m = mtime_secs(path)?;
    let blocked = m + LOCK_MAX_AGE;
    (blocked > now).then_some((blocked, UsageError::Timeout))
}

/// The active lock for this account: its own lock, or upstream's global
/// `usage.lock` (still honoured, since upstream processes may share the cache
/// dir; this port never writes it).
fn read_active_lock(now: i64, key: &str) -> Option<(i64, UsageError)> {
    read_lock(&lock_file(), now).or_else(|| read_lock(&account_lock_file(key), now))
}

/// Take this account's in-flight lock atomically (O_EXCL), so concurrent
/// renders start one fetch, not one each. Returns the contents written.
fn acquire_in_flight(now: i64, key: &str) -> Option<String> {
    let path = account_lock_file(key);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let contents = lock_contents(now + LOCK_MAX_AGE, LockError::Timeout);
    let create = || {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&path)?;
        f.write_all(contents.as_bytes())
    };
    match create() {
        Ok(()) => Some(contents),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            if read_lock(&path, now).is_some() {
                return None;
            }
            // expired: replace it, still exclusively
            let _ = std::fs::remove_file(&path);
            create().ok().map(|_| contents)
        }
        Err(_) => None,
    }
}

// ------------------------------------------------------------ fetchUsageData

fn read_cache_file(path: &std::path::Path) -> Option<String> {
    std::fs::read(path).ok().map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// `readStaleUsageCache`, then this account's copy.
fn read_stale(identity: Option<&Identity>, key: &str) -> Option<UsageData> {
    for path in [cache_file(), account_cache_file(key)] {
        if let Some(raw) = read_cache_file(&path) {
            if token_hash_matches(cached_token_hash(&raw).as_deref(), identity) {
                if let Some(d) = parse_cached(&raw) {
                    return Some(d);
                }
            }
        }
    }
    None
}

/// `getStaleUsageOrError`
fn stale_or_error(error: UsageError, identity: Option<&Identity>, key: &str, required: &[Field]) -> UsageData {
    if let Some(stale) = read_stale(identity, key) {
        if stale.error.is_none() && has_required_fields(&stale, required) {
            return stale;
        }
    }
    UsageData { error: Some(error), ..Default::default() }
}

/// A fresh cache file (|age| < 180 s) that satisfies the request.
fn fresh_cache(path: &std::path::Path, now: i64, identity: Option<&Identity>, required: &[Field]) -> Option<UsageData> {
    let age = now - mtime_secs(path)?;
    if age.abs() >= CACHE_MAX_AGE {
        return None;
    }
    let raw = read_cache_file(path)?;
    let d = parse_cached(&raw)?;
    (d.error.is_none()
        && token_hash_matches(cached_token_hash(&raw).as_deref(), identity)
        && has_required_fields(&d, required))
    .then_some(d)
}

fn start_refresh(in_flight: &str, required: &[Field]) {
    if !super::test_no_background() {
        super::refresh::spawn_usage_refresh(in_flight, required);
    }
}

/// `fetchUsageData({ requiredFields })`.
///
/// Differences from upstream, all for many concurrent sessions on several
/// accounts (none reachable under the parity harness, where this account's
/// cache copy never exists):
/// - A warm render reads this account's cache copy first, before any
///   credential lookup, so it never spawns `security`.
/// - With only a stale copy, it shows that copy and refreshes in the
///   background (upstream would block on the fetch).
/// - Locks are per account and taken with O_EXCL; upstream's global lock is
///   only read.
pub fn fetch_usage_data(required: &[Field]) -> UsageData {
    let now = now_secs();
    let source = credentials_source();
    let key = account_key(&source);

    if let Some(d) = fresh_cache(&account_cache_file(&key), now, None, required) {
        return d;
    }
    if !super::test_no_network() {
        let copy = read_cache_file(&account_cache_file(&key)).and_then(|raw| parse_cached(&raw));
        if let Some(d) = copy.filter(|d| d.error.is_none() && has_required_fields(d, required)) {
            if read_active_lock(now, &key).is_none() {
                if let Some(in_flight) = acquire_in_flight(now, &key) {
                    start_refresh(&in_flight, required);
                }
            }
            return d;
        }
    }

    let creds = credentials_with_backoff(now, &source);
    let identity = creds.identity.as_ref();

    if let Some(d) = fresh_cache(&cache_file(), now, identity, required) {
        return d;
    }
    if identity.is_none() {
        return stale_or_error(UsageError::NoCredentials, identity, &key, required);
    }
    if let Some((_, err)) = read_active_lock(now, &key) {
        return stale_or_error(err, identity, &key, required);
    }
    let Some(in_flight) = acquire_in_flight(now, &key) else {
        // another render took the lock between the check and here
        return stale_or_error(UsageError::Timeout, identity, &key, required);
    };
    if super::test_no_network() {
        // The request fails like an offline connection: `api-error`, and the
        // in-flight lock is left to expire.
        return stale_or_error(UsageError::ApiError, identity, &key, required);
    }
    start_refresh(&in_flight, required);
    stale_or_error(UsageError::Timeout, identity, &key, required)
}

// ------------------------------------------------------------ refresher side

pub enum ApiResult {
    Success(String),
    RateLimited(i64),
    Error,
}

/// The tail of `fetchUsageData` after the response arrives, run by the
/// detached refresher.
pub fn apply_api_result(result: ApiResult, creds: &Credentials, in_flight: &str, required: &[Field]) {
    let now = now_secs();
    let identity = identity_of(creds);
    let key = account_key(&credentials_source());
    match result {
        ApiResult::RateLimited(retry) => write_account_lock(&key, now + retry, LockError::RateLimited),
        ApiResult::Error => {}
        ApiResult::Success(body) => {
            let Some(d) = parse_api_response(&body).filter(|d| d.session_usage.is_some() || d.weekly_usage.is_some())
            else {
                write_account_lock(&key, now + LOCK_MAX_AGE, LockError::ParseError);
                return;
            };
            let json = serialize_cache(&d, Some(&identity.preferred_hash));
            let _ = crate::cachefile::atomic_write(&account_cache_file(&key), json.as_bytes());
            // upstream's shared file too, for any upstream process reading it
            let _ = crate::cachefile::atomic_write(&cache_file(), json.as_bytes());
            if has_required_fields(&d, required) {
                clear_own_account_lock(&key, in_flight);
            }
        }
    }
}

/// `parseRetryAfterSeconds`
pub fn parse_retry_after(v: Option<&str>, now_ms: f64) -> Option<i64> {
    let t = crate::jsnum::js_trim(v?);
    if t.is_empty() {
        return None;
    }
    if t.bytes().all(|b| b.is_ascii_digit()) {
        let n = crate::jsnum::parse_int(t)?;
        return (n > 0.0).then_some(n as i64);
    }
    let at = crate::timefmt::parse_iso(t).or_else(|| parse_http_date(t))?;
    let secs = ((at - now_ms) / 1000.0).ceil();
    (secs > 0.0).then_some(secs as i64)
}

/// RFC 7231 IMF-fixdate (`Sun, 06 Nov 1994 08:49:37 GMT`), the form servers
/// send in Retry-After.
fn parse_http_date(s: &str) -> Option<f64> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() != 6 || parts[5] != "GMT" {
        return None;
    }
    let day: u32 = parts[1].parse().ok()?;
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let month = MONTHS.iter().position(|m| *m == parts[2])? as u32 + 1;
    let year: i64 = parts[3].parse().ok()?;
    let hms: Vec<i64> = parts[4].split(':').map(|x| x.parse().ok()).collect::<Option<_>>()?;
    if hms.len() != 3 {
        return None;
    }
    let days = crate::timefmt::days_from_civil(year, month, day);
    Some(((days * 86400 + hms[0] * 3600 + hms[1] * 60 + hms[2]) * 1000) as f64)
}

pub fn fields_csv(fields: &[Field]) -> String {
    fields.iter().map(|f| f.name()).collect::<Vec<_>>().join(",")
}

pub fn fields_from_csv(s: &str) -> Vec<Field> {
    s.split(',').filter_map(Field::from_name).collect()
}

/// Every field, for callers that want the whole record.
pub fn all_fields() -> Vec<Field> {
    ALL_FIELDS.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_limits_fallback_and_placeholders() {
        let body = r#"{"five_hour":null,"seven_day":{"utilization":12.5,"resets_at":"2026-10-25T00:00:00Z"},
            "limits":[{"kind":"session","percent":0,"resets_at":null},
                      {"kind":"weekly_scoped","percent":0,"resets_at":null,"scope":{"model":{"display_name":"Claude Fable"}}},
                      {"kind":"weekly_scoped","percent":40,"resets_at":"2026-10-26T00:00:00Z","scope":{"model":{"display_name":"Opus"}}}],
            "extra_usage":{"is_enabled":true,"monthly_limit":5000,"used_credits":1234,"utilization":null,"currency":"EUR"}}"#;
        let d = parse_api_response(body).unwrap();
        assert_eq!(d.session_usage, Some(0.0));
        assert_eq!(d.session_reset_at, None);
        assert_eq!(d.weekly_usage, Some(12.5));
        assert_eq!(d.fable_usage, Some(0.0));
        assert_eq!(d.weekly_opus_usage, Some(40.0));
        assert_eq!(d.weekly_opus_reset_at.as_deref(), Some("2026-10-26T00:00:00Z"));
        assert_eq!(d.weekly_sonnet_usage, None);
        assert_eq!(d.extra_usage_currency.as_deref(), Some("EUR"));
        assert!(parse_api_response(r#"{"five_hour":5}"#).is_none());
        assert!(parse_api_response("[]").is_none());
    }

    #[test]
    fn cache_roundtrip_and_hash() {
        let d = UsageData {
            session_usage: Some(42.5),
            weekly_usage: Some(3.0),
            weekly_opus_usage: Some(0.0),
            extra_usage_enabled: Some(false),
            ..Default::default()
        };
        let s = serialize_cache(&d, Some("abc"));
        assert_eq!(
            s,
            r#"{"sessionUsage":42.5,"weeklyUsage":3,"extraUsageEnabled":false,"weeklyOpusUsage":0,"tokenHash":"abc"}"#
        );
        assert_eq!(parse_cached(&s).unwrap(), d);
        assert_eq!(cached_token_hash(&s).as_deref(), Some("abc"));
        assert!(parse_cached(r#"{"sessionUsage":"x"}"#).is_none());
    }

    #[test]
    fn credentials_and_identity() {
        let c = parse_credentials(r#"{"claudeAiOauth":{"accessToken":"at","refreshToken":"rt"}}"#).unwrap();
        let i = identity_of(&c);
        assert_eq!(i.preferred_hash, sha256::hex_prefix("rt", 16));
        assert_eq!(i.access_token_hash, sha256::hex_prefix("at", 16));
        assert!(parse_credentials(r#"{"claudeAiOauth":{"accessToken":""}}"#).is_none());
        assert!(parse_credentials(r#"{"claudeAiOauth":{"accessToken":5}}"#).is_none());
    }

    #[test]
    fn keychain_candidates() {
        let dump = "keychain: \"/k\"\n    \"svce\"<blob>=\"Claude Code-credentials-aaaa\"\n    \"mdat\"<timedate>=0x32303236313032313130303030305A00  \"20261021100000Z\\000\"\nkeychain: \"/k\"\n    \"svce\"<blob>=\"Claude Code-credentials-bbbb\"\n    \"mdat\"<timedate>=\"20261021120000Z\\000\"\nkeychain: \"/k\"\n    \"svce\"<blob>=\"Claude Code-credentials\"\n";
        assert_eq!(
            parse_keychain_candidates(dump),
            vec!["Claude Code-credentials-bbbb", "Claude Code-credentials-aaaa"]
        );
    }

    #[test]
    fn retry_after() {
        assert_eq!(parse_retry_after(Some(" 120 "), 0.0), Some(120));
        assert_eq!(parse_retry_after(Some("0"), 0.0), None);
        assert_eq!(parse_retry_after(Some("Wed, 21 Oct 2026 12:00:10 GMT"), 1792584000000.0), Some(10));
    }
}
