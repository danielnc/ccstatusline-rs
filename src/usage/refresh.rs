//! Detached background refreshers: the only code that talks to the network.
//!
//! A render that finds a cache due starts `ccstatusline-rs --usage-refresh` or
//! `--claude-status-refresh` in its own session (so `timeout`/Claude Code
//! killing the render's process group does not kill it) and returns at once.
//! Each refresher holds an O_EXCL lock for its run, so at most one fetch per
//! kind is in flight machine-wide; it fetches with curl and writes the cache
//! atomically.

use super::fetch::{self, ApiResult};
use super::{cache_dir, service_status, Field};
use std::io::Write;
use std::process::{Command, Stdio};

const USAGE_API_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const USAGE_API_HOST: &str = "api.anthropic.com";
const STATUS_HOST: &str = "status.claude.com";
/// A refresher lock older than this belongs to a crashed refresher.
const REFRESH_LOCK_STALE_MS: f64 = 60_000.0;

/// Handle the refresher re-entry flags; true when one was handled.
pub fn handle_refresh_cli(args: &[String]) -> bool {
    match args.first().map(String::as_str) {
        Some("--usage-refresh") => {
            let in_flight = args.get(1).cloned().unwrap_or_default();
            let required = fetch::fields_from_csv(args.get(2).map(String::as_str).unwrap_or(""));
            run_usage_refresh(&in_flight, &required);
            true
        }
        Some("--claude-status-refresh") => {
            let incidents = args.get(1).is_some_and(|a| a == "1");
            service_status::run_refresh(incidents);
            true
        }
        _ => false,
    }
}

fn spawn_self(args: &[&str]) {
    let Ok(exe) = std::env::current_exe() else { return };
    let mut cmd = Command::new(exe);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(unix)]
    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let _ = cmd.spawn();
}

pub fn spawn_usage_refresh(in_flight: &str, required: &[Field]) {
    let csv = fetch::fields_csv(required);
    spawn_self(&["--usage-refresh", in_flight, &csv]);
}

pub fn spawn_claude_status_refresh(include_incidents: bool) {
    spawn_self(&["--claude-status-refresh", if include_incidents { "1" } else { "0" }]);
}

/// `isExcludedFromProxy(host)`: NO_PROXY / no_proxy.
pub fn is_excluded_from_proxy(host: &str) -> bool {
    let target = host.to_lowercase();
    let mut entries: Vec<String> = Vec::new();
    for var in ["NO_PROXY", "no_proxy"] {
        if let Ok(v) = std::env::var(var) {
            entries.extend(v.split(|c: char| c.is_whitespace() || c == ',').map(String::from));
        }
    }
    entries.iter().any(|raw| {
        let mut entry = raw.to_lowercase();
        if let Some(i) = entry.rfind(':') {
            if &entry[i + 1..] != "443" {
                return false;
            }
            entry.truncate(i);
        }
        if entry == "*" {
            return true;
        }
        let domain = entry.strip_prefix("*.").or_else(|| entry.strip_prefix('.')).unwrap_or(&entry);
        !domain.is_empty() && (target == domain || target.ends_with(&format!(".{domain}")))
    })
}

/// The proxy upstream would use for `host` (HTTPS_PROXY only, like upstream).
fn proxy_for(host: &str) -> Option<String> {
    let p = std::env::var("HTTPS_PROXY").ok()?;
    let p = crate::jsnum::js_trim(&p).to_string();
    (!p.is_empty() && !is_excluded_from_proxy(host)).then_some(p)
}

fn curl_quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '\n' | '\r' => {}
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

pub struct HttpResponse {
    pub status: u16,
    pub body: String,
    pub retry_after: Option<String>,
}

const WRITE_OUT_MARKER: &str = "\n__ccsl_curl__";

/// GET `url` with curl, 5 s deadline. The request (URL, headers, proxy) goes in
/// through `-K -` so no token ever appears in the process list.
pub fn curl_get(url: &str, host: &str, headers: &[String]) -> Option<HttpResponse> {
    let mut config = format!("url = {}\n", curl_quote(url));
    for h in headers {
        config.push_str(&format!("header = {}\n", curl_quote(h)));
    }
    match proxy_for(host) {
        Some(p) => config.push_str(&format!("proxy = {}\n", curl_quote(&p))),
        None => config.push_str("noproxy = \"*\"\n"),
    }
    let mut child = Command::new("curl")
        .args([
            "-s",
            "--max-time",
            "5",
            "-K",
            "-",
            "-w",
            &format!("{WRITE_OUT_MARKER}%{{http_code}} %header{{retry-after}}"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    child.stdin.take()?.write_all(config.as_bytes()).ok()?;
    let out = child.wait_with_output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let i = text.rfind(WRITE_OUT_MARKER)?;
    let tail = &text[i + WRITE_OUT_MARKER.len()..];
    let (code, retry) = tail.split_once(' ').unwrap_or((tail, ""));
    let status: u16 = code.trim().parse().ok()?;
    let retry = retry.trim();
    Some(HttpResponse {
        status,
        body: text[..i].to_string(),
        retry_after: (!retry.is_empty()).then(|| retry.to_string()),
    })
}

/// The render that spawned this holds the account's in-flight lock (taken
/// with O_EXCL), so this is the only fetch for the account.
fn run_usage_refresh(in_flight: &str, required: &[Field]) {
    let Some(creds) = fetch::usage_credentials() else { return };
    let headers =
        [format!("Authorization: Bearer {}", creds.access_token), "anthropic-beta: oauth-2025-04-20".to_string()];
    let result = match curl_get(USAGE_API_URL, USAGE_API_HOST, &headers) {
        Some(r) if r.status == 200 && !r.body.is_empty() => ApiResult::Success(r.body),
        Some(r) if r.status == 429 => ApiResult::RateLimited(
            fetch::parse_retry_after(r.retry_after.as_deref(), crate::clock::now_ms())
                .unwrap_or(fetch::DEFAULT_RATE_LIMIT_BACKOFF),
        ),
        _ => ApiResult::Error,
    };
    fetch::apply_api_result(result, &creds, in_flight, required);
}

/// GET a status.claude.com path; the body on a 200.
pub fn fetch_status_page(path: &str) -> Option<String> {
    let r = curl_get(&format!("https://{STATUS_HOST}{path}"), STATUS_HOST, &[])?;
    (r.status == 200 && !r.body.is_empty()).then_some(r.body)
}

fn status_refresh_lock_file() -> std::path::PathBuf {
    cache_dir().join("claude-status-refresh.lock")
}

/// Take the status refresher's O_EXCL lock for a refresher about to be
/// spawned; the refresher releases it when done (a crashed one's lock goes
/// stale after a minute).
pub fn take_status_refresh_lock() -> bool {
    match crate::cachefile::try_lock(&status_refresh_lock_file(), REFRESH_LOCK_STALE_MS) {
        Some(guard) => {
            std::mem::forget(guard);
            true
        }
        None => false,
    }
}

pub fn release_status_refresh_lock() {
    let _ = std::fs::remove_file(status_refresh_lock_file());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting() {
        assert_eq!(curl_quote(r#"a"b\c"#), r#""a\"b\\c""#);
    }
}
