//! PR/MR and CI data for the current branch (upstream
//! `src/utils/git-review-cache.ts`). Rendering only reads the cache file; a
//! stale or missing entry schedules a detached refresh of this executable
//! (`--internal-refresh-git-review-cache`), which asks gh/glab and writes the
//! file. Nothing on the render path touches the network.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use super::exec::{Cmd, Failure};
use super::hardening::{hardening_env, HARDENING_ARGS};
use super::remote::parse_remote_url;
use crate::clock::now_ms;
use crate::jsnum::js_trim;

const CACHE_TTL_MS: f64 = 30_000.0;
const CLI_TIMEOUT_MS: f64 = 5_000.0;
const REFRESH_LOCK_STALE_MS: f64 = 30_000.0;
const DEFAULT_TITLE_MAX_WIDTH: usize = 30;
const GH_PR_METADATA_FIELDS: &str = "url,number,title,state,reviewDecision";
const GH_PR_WITH_CHECKS_FIELDS: &str = "url,number,title,state,reviewDecision,statusCheckRollup";
pub const REFRESH_FLAG: &str = "--internal-refresh-git-review-cache";

/// A decoded cache file. `data` is the stored object as is (it may carry
/// fields of any type; widgets read it with JS semantics).
#[derive(Clone)]
pub struct Cached {
    pub data: Option<Value>,
    pub checks_queried: bool,
    pub stale: bool,
}

fn review_dir() -> PathBuf {
    crate::settings::home_dir().join(".cache").join("ccstatusline").join("git-review")
}

fn run_git_for_cache(args: &[&str], cwd: &str) -> String {
    let mut a: Vec<String> = HARDENING_ARGS.iter().map(|s| s.to_string()).collect();
    a.extend(args.iter().map(|s| s.to_string()));
    match Cmd::new("git", a, Some(cwd), CLI_TIMEOUT_MS as u64).run() {
        Ok(o) => js_trim(&o).to_string(),
        Err(_) => String::new(),
    }
}

fn current_branch(cwd: &str) -> Option<String> {
    let b = run_git_for_cache(&["symbolic-ref", "--short", "HEAD"], cwd);
    (!b.is_empty()).then_some(b)
}

thread_local! {
    static REF_MEMO: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
    static READ_MEMO: RefCell<HashMap<PathBuf, Option<Cached>>> = RefCell::new(HashMap::new());
}

fn compute_cache_ref(cwd: &str) -> String {
    if let Some(b) = current_branch(cwd) {
        return format!("branch:{b}");
    }
    let head = run_git_for_cache(&["rev-parse", "--short", "HEAD"], cwd);
    if !head.is_empty() {
        return format!("head:{head}");
    }
    "unknown".into()
}

/// `getCacheRef(cwd)`, once per process on the render path, from the HEAD
/// file when the git dir is known and that is unambiguous.
fn cache_ref_memo(cwd: &str, git_dir: Option<&str>) -> String {
    if let Some(r) = REF_MEMO.with(|m| m.borrow().get(cwd).cloned()) {
        return r;
    }
    let r = match git_dir.and_then(super::native_short_branch) {
        Some(b) => format!("branch:{b}"),
        None => compute_cache_ref(cwd),
    };
    REF_MEMO.with(|m| m.borrow_mut().insert(cwd.to_string(), r.clone()));
    r
}

fn cache_path(cwd: &str, r: &str) -> PathBuf {
    let hash = super::sha256::hex16(format!("{cwd}\0{r}").as_bytes());
    review_dir().join(format!("git-review-{hash}.json"))
}

fn is_review_data(v: &Value) -> bool {
    v.as_object()
        .is_some_and(|o| o.get("number").is_some_and(Value::is_number) && o.get("url").is_some_and(Value::is_string))
}

/// `decodeCache(content)`; None is a miss.
fn decode(content: &str) -> Option<(Option<Value>, bool)> {
    if content.is_empty() {
        return Some((None, true));
    }
    let parsed: Value = serde_json::from_str(content).ok()?;
    if let Some(o) = parsed.as_object() {
        let version_ok = o.get("version").and_then(Value::as_f64) == Some(1.0);
        if let (true, Some(Value::Bool(cq))) = (version_ok, o.get("checksQueried")) {
            match o.get("data") {
                Some(Value::Null) => return Some((None, true)),
                Some(d) if is_review_data(d) => return Some((Some(d.clone()), *cq)),
                _ => {}
            }
        }
    }
    if is_review_data(&parsed) {
        let has_checks = parsed.get("checks").is_some();
        return Some((Some(parsed), has_checks));
    }
    None
}

fn mtime_ms(p: &Path) -> Option<f64> {
    use std::os::unix::fs::MetadataExt;
    let m = std::fs::metadata(p).ok()?;
    Some(m.mtime() as f64 * 1e3 + m.mtime_nsec() as f64 / 1e6)
}

/// `readCache(cachePath)`
fn read_cache(path: &Path) -> Option<Cached> {
    let mtime = mtime_ms(path)?;
    let age = now_ms() - mtime;
    let bytes = std::fs::read(path).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let (data, checks_queried) = decode(js_trim(&text))?;
    Some(Cached { data, checks_queried, stale: age > CACHE_TTL_MS })
}

fn write_cache(path: &Path, data: Option<&Value>, checks_queried: bool) {
    let mut m = Map::new();
    m.insert("version".into(), Value::from(1));
    m.insert("data".into(), data.cloned().unwrap_or(Value::Null));
    m.insert("checksQueried".into(), Value::Bool(data.is_none() || checks_queried));
    let _ = crate::cachefile::atomic_write(path, crate::jsjson::stringify(&Value::Object(m)).as_bytes());
}

fn lock_path(cache: &Path) -> PathBuf {
    let mut s = cache.as_os_str().to_os_string();
    s.push(".lock");
    PathBuf::from(s)
}

fn create_refresh_lock(cache: &Path) -> Option<PathBuf> {
    std::fs::create_dir_all(review_dir()).ok()?;
    let lock = lock_path(cache);
    for _ in 0..2 {
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&lock) {
            Ok(_) => return Some(lock),
            Err(_) => {
                let age = now_ms() - mtime_ms(&lock)?;
                if age <= REFRESH_LOCK_STALE_MS {
                    return None;
                }
                std::fs::remove_file(&lock).ok()?;
            }
        }
    }
    None
}

fn schedule_refresh(cwd: &str, cache: &Path, include_checks: bool) {
    let Ok(exe) = std::env::current_exe() else { return };
    let Some(lock) = create_refresh_lock(cache) else { return };
    // The parity harness turns detached refreshes off (the lock stays, as
    // upstream's no-op spawn leaves it)
    if std::env::var("CCSL_TEST_NO_BACKGROUND").is_ok_and(|v| v == "1") {
        return;
    }
    let args = vec![
        REFRESH_FLAG.to_string(),
        cwd.to_string(),
        if include_checks { "checks" } else { "metadata" }.to_string(),
        lock.to_string_lossy().into_owned(),
    ];
    if super::exec::spawn_detached(&exe, &args).is_err() {
        let _ = std::fs::remove_file(&lock);
    }
}

/// `getCachedGitReviewData(cwd, { includeChecks })`; `git_dir` is the git
/// dir git reported for `cwd`, if known.
pub fn cached_review_data(cwd: &str, include_checks: bool, git_dir: Option<&str>) -> Option<Value> {
    let path = cache_path(cwd, &cache_ref_memo(cwd, git_dir));
    let cached = match READ_MEMO.with(|m| m.borrow().get(&path).cloned()) {
        Some(c) => c,
        None => {
            let c = read_cache(&path);
            READ_MEMO.with(|m| m.borrow_mut().insert(path.clone(), c.clone()));
            c
        }
    };
    let needs_refresh = match &cached {
        None => true,
        Some(c) => c.stale || (include_checks && !c.checks_queried),
    };
    if needs_refresh {
        schedule_refresh(cwd, &path, include_checks);
    }
    cached.and_then(|c| c.data)
}

// ------------------------------------------------------------------ refresh

/// `handleGitReviewRefresh()`: true when the flag was present.
pub fn handle_refresh_cli(args: &[String]) -> bool {
    let Some(i) = args.iter().position(|a| a == REFRESH_FLAG) else { return false };
    let get = |k: usize| args.get(i + k).map(String::as_str).unwrap_or("");
    let (cwd, mode, lock) = (get(1), get(2), get(3));
    if cwd.is_empty() || (mode != "metadata" && mode != "checks") || lock.is_empty() {
        return true;
    }
    let expected = lock_path(&cache_path(cwd, &compute_cache_ref(cwd)));
    fetch_review_data(cwd, mode == "checks");
    if Path::new(lock) == expected {
        let _ = std::fs::remove_file(lock);
    }
    true
}

#[derive(Clone, Copy, PartialEq)]
enum Provider {
    Gh,
    Glab,
}

impl Provider {
    fn cli(self) -> &'static str {
        match self {
            Provider::Gh => "gh",
            Provider::Glab => "glab",
        }
    }
}

fn origin_url(cwd: &str) -> Option<String> {
    let u = run_git_for_cache(&["remote", "get-url", "--", "origin"], cwd);
    (!u.is_empty()).then_some(u)
}

fn is_ssh_remote_url(url: &str) -> bool {
    let t = js_trim(url).to_lowercase();
    t.starts_with("ssh://") || !t.contains("://")
}

fn resolve_ssh_host_alias(host: &str) -> String {
    let c = Cmd::new("ssh", vec!["-G".into(), "--".into(), host.into()], None, CLI_TIMEOUT_MS as u64);
    if let Ok(out) = c.run() {
        for line in js_trim(&out).split('\n') {
            let line = js_trim(line.strip_suffix('\r').unwrap_or(line));
            if line.get(..8).is_some_and(|h| h.eq_ignore_ascii_case("hostname")) {
                let rest = &line[8..];
                let value = rest.trim_start_matches(crate::gradient::is_js_space);
                if value.len() < rest.len() && !value.is_empty() {
                    return value.to_lowercase();
                }
            }
        }
    }
    host.to_lowercase()
}

fn named_forge(host: &str) -> Option<Provider> {
    if host.contains("github") {
        Some(Provider::Gh)
    } else if host.contains("gitlab") {
        Some(Provider::Glab)
    } else {
        None
    }
}

fn effective_remote_host(url: &str, host: &str) -> String {
    let normalized = host.to_lowercase();
    if !is_ssh_remote_url(url) || named_forge(&normalized).is_some() {
        return normalized;
    }
    resolve_ssh_host_alias(&normalized)
}

fn origin_host(cwd: &str) -> Option<String> {
    let url = origin_url(cwd)?;
    let p = parse_remote_url(&url)?;
    Some(effective_remote_host(&url, &p.host))
}

fn origin_repo_ref(cwd: &str) -> Option<String> {
    let url = origin_url(cwd)?;
    let p = parse_remote_url(&url)?;
    Some(format!("https://{}/{}/{}", effective_remote_host(&url, &p.host), p.owner, p.repo))
}

fn cli_authed_for_host(cli: Provider, host: &str) -> bool {
    Cmd::new(
        cli.cli(),
        vec!["auth".into(), "status".into(), "--hostname".into(), host.into()],
        None,
        CLI_TIMEOUT_MS as u64,
    )
    .run()
    .is_ok()
}

fn provider_candidates(cwd: &str) -> Vec<Provider> {
    let Some(host) = origin_host(cwd) else { return vec![Provider::Gh, Provider::Glab] };
    if let Some(p) = named_forge(&host) {
        return vec![p];
    }
    let mut authed = Vec::new();
    if cli_authed_for_host(Provider::Glab, &host) {
        authed.push(Provider::Glab);
    }
    if cli_authed_for_host(Provider::Gh, &host) {
        authed.push(Provider::Gh);
    }
    authed
}

/// What a lookup step can throw: an exec failure, a deadline, bad JSON.
struct Thrown {
    text: String,
}

impl From<Failure> for Thrown {
    fn from(f: Failure) -> Self {
        Thrown { text: format!("{}\n{}", f.message, f.stderr).to_lowercase() }
    }
}

fn remaining_timeout(deadline: f64) -> Result<u64, Thrown> {
    let remaining = deadline - now_ms();
    if remaining <= 0.0 {
        return Err(Thrown { text: "git review lookup deadline exceeded\n".into() });
    }
    Ok(remaining.clamp(1.0, CLI_TIMEOUT_MS) as u64)
}

fn cli_available(cli: Provider, deadline: f64) -> bool {
    let Ok(t) = remaining_timeout(deadline) else { return false };
    Cmd::new(cli.cli(), vec!["--version".into()], None, t).run().is_ok()
}

fn hardened_cmd<'a>(cli: &'a str, args: Vec<String>, cwd: &'a str, timeout: u64) -> Cmd<'a> {
    let mut c = Cmd::new(cli, args, Some(cwd), timeout);
    c.env.extend(hardening_env());
    c
}

fn parse_json_output(out: &str) -> Result<Option<Value>, Thrown> {
    let t = js_trim(out);
    if t.is_empty() {
        return Ok(None);
    }
    serde_json::from_str(t).map(Some).map_err(|e| Thrown { text: format!("syntaxerror: {e}\n").to_lowercase() })
}

fn query_gh_pr(
    cwd: &str,
    args: &[String],
    branch: Option<&str>,
    fields: &str,
    deadline: f64,
) -> Result<Option<Value>, Thrown> {
    let mut a = args.to_vec();
    a.push("--json".into());
    a.push(fields.into());
    if let Some(b) = branch {
        a.push("--".into());
        a.push(b.into());
    }
    let mut c = hardened_cmd("gh", a, cwd, remaining_timeout(deadline)?);
    c.capture_stderr = true;
    parse_json_output(&c.run()?)
}

fn is_ci_field_unavailable(t: &Thrown) -> bool {
    t.text.contains("statuscheckrollup") || t.text.contains("resource not accessible by integration")
}

fn str_field(o: &Map<String, Value>, k: &str) -> String {
    match o.get(k) {
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    }
}

fn upper_field(e: &Map<String, Value>, k: &str) -> String {
    match e.get(k) {
        Some(Value::String(s)) => s.to_uppercase(),
        _ => String::new(),
    }
}

/// `computeCiRollup(rollup)` as the stored `checks` object
fn ci_rollup(rollup: Option<&Value>) -> Option<Value> {
    let arr = rollup?.as_array()?;
    let (mut failing, mut pending, mut success, mut seen) = (0u64, 0u64, 0u64, 0u64);
    for entry in arr {
        let e = match entry {
            Value::Object(o) => o,
            // arrays are objects too, with none of these fields
            Value::Array(_) => {
                seen += 1;
                failing += 1;
                continue;
            }
            _ => continue,
        };
        seen += 1;
        let kind = if let Some(Value::String(status)) = e.get("status") {
            if status.to_uppercase() != "COMPLETED" {
                "pending"
            } else {
                match upper_field(e, "conclusion").as_str() {
                    "SUCCESS" => "success",
                    "NEUTRAL" | "SKIPPED" => "ignored",
                    _ => "failed",
                }
            }
        } else {
            match upper_field(e, "state").as_str() {
                "SUCCESS" => "success",
                "PENDING" | "EXPECTED" => "pending",
                _ => "failed",
            }
        };
        match kind {
            "failed" => failing += 1,
            "pending" => pending += 1,
            "success" => success += 1,
            _ => {}
        }
    }
    if seen == 0 {
        return None;
    }
    let state = if failing > 0 {
        "failing"
    } else if pending > 0 {
        "pending"
    } else {
        "passing"
    };
    let mut m = Map::new();
    m.insert("state".into(), Value::from(state));
    m.insert("failing".into(), Value::from(failing));
    m.insert("pending".into(), Value::from(pending));
    m.insert("success".into(), Value::from(success));
    Some(Value::Object(m))
}

fn fetch_from_gh(
    cwd: &str,
    repo_ref: Option<&str>,
    include_checks: bool,
    deadline: f64,
) -> Result<Option<Value>, Thrown> {
    let mut args: Vec<String> = vec!["pr".into(), "view".into()];
    let mut branch = None;
    if let Some(r) = repo_ref {
        let Some(b) = current_branch(cwd) else { return Ok(None) };
        branch = Some(b);
        args.push("--repo".into());
        args.push(r.into());
    }
    let parsed = if include_checks {
        match query_gh_pr(cwd, &args, branch.as_deref(), GH_PR_WITH_CHECKS_FIELDS, deadline) {
            Ok(p) => p,
            Err(e) if is_ci_field_unavailable(&e) => {
                query_gh_pr(cwd, &args, branch.as_deref(), GH_PR_METADATA_FIELDS, deadline)?
            }
            Err(e) => return Err(e),
        }
    } else {
        query_gh_pr(cwd, &args, branch.as_deref(), GH_PR_METADATA_FIELDS, deadline)?
    };
    let Some(Value::Object(p)) = parsed else { return Ok(None) };
    let (Some(Value::Number(number)), Some(Value::String(url))) = (p.get("number"), p.get("url")) else {
        return Ok(None);
    };
    let mut m = Map::new();
    m.insert("number".into(), crate::jsjson::num(number.as_f64().unwrap_or(0.0)));
    m.insert("url".into(), Value::String(url.clone()));
    m.insert("title".into(), Value::String(str_field(&p, "title")));
    m.insert("state".into(), Value::String(str_field(&p, "state")));
    m.insert("reviewDecision".into(), Value::String(str_field(&p, "reviewDecision")));
    m.insert("provider".into(), Value::from("gh"));
    if let Some(c) = ci_rollup(p.get("statusCheckRollup")) {
        m.insert("checks".into(), c);
    }
    Ok(Some(Value::Object(m)))
}

fn map_glab_state(s: &str) -> String {
    match s {
        "opened" => "OPEN".into(),
        "closed" => "CLOSED".into(),
        "merged" => "MERGED".into(),
        "locked" => "LOCKED".into(),
        s => s.to_uppercase(),
    }
}

fn fetch_from_glab(cwd: &str, repo_ref: Option<&str>, deadline: f64) -> Result<Option<Value>, Thrown> {
    let mut args: Vec<String> = ["mr", "view", "--output", "json"].iter().map(|s| s.to_string()).collect();
    if let Some(r) = repo_ref {
        let Some(b) = current_branch(cwd) else { return Ok(None) };
        args.extend(["--repo".to_string(), r.to_string(), "--".to_string(), b]);
    }
    let c = hardened_cmd("glab", args, cwd, remaining_timeout(deadline)?);
    let Some(Value::Object(p)) = parse_json_output(&c.run()?)? else { return Ok(None) };
    let (Some(Value::Number(iid)), Some(Value::String(url))) = (p.get("iid"), p.get("web_url")) else {
        return Ok(None);
    };
    let mut m = Map::new();
    m.insert("number".into(), crate::jsjson::num(iid.as_f64().unwrap_or(0.0)));
    m.insert("url".into(), Value::String(url.clone()));
    m.insert("title".into(), Value::String(str_field(&p, "title")));
    let state = match p.get("state") {
        Some(Value::String(s)) => map_glab_state(s),
        _ => String::new(),
    };
    m.insert("state".into(), Value::String(state));
    m.insert("reviewDecision".into(), Value::from(""));
    m.insert("provider".into(), Value::from("glab"));
    Ok(Some(Value::Object(m)))
}

fn fetch_from_provider(
    p: Provider,
    cwd: &str,
    repo_ref: Option<&str>,
    include_checks: bool,
    deadline: f64,
) -> Result<Option<Value>, Thrown> {
    let fetch = |r: Option<&str>| match p {
        Provider::Gh => fetch_from_gh(cwd, r, include_checks, deadline),
        Provider::Glab => fetch_from_glab(cwd, r, deadline),
    };
    if let Ok(Some(d)) = fetch(None) {
        return Ok(Some(d));
    }
    match repo_ref {
        Some(r) => fetch(Some(r)),
        None => Ok(None),
    }
}

/// `fetchGitReviewData(cwd, deps, { includeChecks })`
pub fn fetch_review_data(cwd: &str, include_checks: bool) -> Option<Value> {
    let path = cache_path(cwd, &compute_cache_ref(cwd));
    let cached = read_cache(&path);
    if let Some(c) = &cached {
        if !c.stale && (!include_checks || c.checks_queried) {
            return c.data.clone();
        }
    }
    let repo_ref = origin_repo_ref(cwd);
    let deadline = now_ms() + CLI_TIMEOUT_MS;
    for p in provider_candidates(cwd) {
        if !cli_available(p, deadline) {
            continue;
        }
        if let Ok(Some(d)) = fetch_from_provider(p, cwd, repo_ref.as_deref(), include_checks, deadline) {
            write_cache(&path, Some(&d), include_checks);
            return Some(d);
        }
    }
    if let Some(Cached { data: Some(d), .. }) = cached {
        return Some(d);
    }
    write_cache(&path, None, true);
    None
}

// ------------------------------------------------------------ display

/// `getGitReviewStatusLabel(state, reviewDecision)` on JS values; None is
/// `undefined`/`null` passed through.
pub fn status_label(state: Option<&Value>, review_decision: Option<&Value>) -> Option<Value> {
    let is = |v: Option<&Value>, s: &str| matches!(v, Some(Value::String(x)) if x == s);
    if is(state, "MERGED") {
        return Some(Value::from("MERGED"));
    }
    if is(state, "CLOSED") {
        return Some(Value::from("CLOSED"));
    }
    if is(review_decision, "APPROVED") {
        return Some(Value::from("APPROVED"));
    }
    if is(review_decision, "CHANGES_REQUESTED") {
        return Some(Value::from("CHANGES_REQ"));
    }
    if is(state, "OPEN") {
        return Some(Value::from("OPEN"));
    }
    state.cloned()
}

/// `truncateTitle(title)`
pub fn truncate_title(title: &str) -> String {
    let limit = DEFAULT_TITLE_MAX_WIDTH;
    if crate::ansi::get_visible_width(title) <= limit {
        return title.to_string();
    }
    format!("{}…", crate::ansi::truncate_styled_text(title, limit - 1, false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_formats() {
        assert!(matches!(decode(""), Some((None, true))));
        let (d, cq) = decode(r#"{"version":1,"data":{"number":1,"url":"u"},"checksQueried":false}"#).unwrap();
        assert!(d.is_some() && !cq);
        let (d, cq) = decode(r#"{"number":1,"url":"u","checks":{}}"#).unwrap();
        assert!(d.is_some() && cq);
        assert!(decode("null").is_none());
        assert!(decode("{").is_none());
    }

    #[test]
    fn rollup() {
        let v: Value = serde_json::from_str(
            r#"[{"status":"COMPLETED","conclusion":"SUCCESS"},{"status":"IN_PROGRESS"},{"state":"FAILURE"},{"status":"COMPLETED","conclusion":"SKIPPED"},3]"#,
        )
        .unwrap();
        let c = ci_rollup(Some(&v)).unwrap();
        assert_eq!(crate::jsjson::stringify(&c), r#"{"state":"failing","failing":1,"pending":1,"success":1}"#);
        assert!(ci_rollup(Some(&serde_json::json!([]))).is_none());
    }
}
