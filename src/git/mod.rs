//! Git provider (upstream `src/utils/git.ts`): every git call a widget makes
//! goes through `run_git_args`, which keeps upstream's two caches (in-process,
//! and one persistent JSON file per (repo, cwd) under
//! `~/.cache/ccstatusline/git-cache/`, invalidated by HEAD/index mtimes and
//! `gitCacheTtlSeconds`).
//!
//! On top of upstream: the first git query of a render prefetches every
//! command the configured widgets will ask for, running the missing ones
//! concurrently (the `rev-parse` queries folded into one process), and
//! writes the persistent cache once. `symbolic-ref HEAD` is answered from the
//! HEAD file when that is unambiguous. Results and cache contents are what
//! upstream's sequential calls produce.

pub mod exec;
pub mod hardening;
pub mod jj;
pub mod paths;
pub mod remote;
pub mod review;
pub mod sha256;
pub mod url;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::context::RenderContext;
use crate::status::StatusJson;
use exec::{Cmd, Failure, Pending};
use paths::{dirname, join, resolve, resolve_from};

const DEFAULT_TTL_SECONDS: f64 = 5.0;
pub const COMMAND_TIMEOUT_MS: u64 = 5_000;

pub const K_INSIDE: &str = "rev-parse --is-inside-work-tree";
pub const K_TOPLEVEL: &str = "rev-parse --show-toplevel";
pub const K_WORKTREE_DIRS: &str = "rev-parse --git-dir --git-common-dir";
/// Not an upstream query: the git dir, so HEAD can be read without a spawn.
const K_GIT_DIR: &str = "rev-parse --git-dir";
pub const K_SYMBOLIC_REF: &str = "symbolic-ref HEAD";
pub const K_SHORT_SHA: &str = "rev-parse --short HEAD";
pub const K_DIFF: &str = "diff --shortstat";
pub const K_DIFF_CACHED: &str = "diff --cached --shortstat";
pub const K_STATUS: &str = "status --porcelain -z";
pub const K_AHEAD_BEHIND: &str = "rev-list --left-right --count HEAD...@{upstream}";
pub const K_CONFLICTS: &str = "ls-files --unmerged";
pub const K_UPSTREAM_REF: &str = "rev-parse --abbrev-ref --symbolic-full-name @{upstream}";
pub const K_REMOTES: &str = "remote";

#[derive(Clone, Debug)]
struct Entry {
    output: Option<String>,
    created_at: f64,
    head: Option<f64>,
    index: Option<f64>,
}

#[derive(Clone)]
struct Metadata {
    cache_path: PathBuf,
    head: Option<f64>,
    index: Option<f64>,
    git_dir: String,
}

struct Persistent {
    cwd: Option<String>,
    entries: Vec<(String, Entry)>,
}

#[derive(Default)]
struct State {
    mem: HashMap<String, Entry>,
    filter: HashMap<String, Option<Vec<String>>>,
    metadata: HashMap<Option<String>, Option<Metadata>>,
    outside: HashMap<String, bool>,
    persistent: HashMap<PathBuf, Option<Persistent>>,
    prefetched: bool,
    spawned: usize,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn with_state<T>(f: impl FnOnce(&mut State) -> T) -> T {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

/// Processes spawned by this module so far (perf reporting / tests).
pub fn spawn_count() -> usize {
    with_state(|s| s.spawned)
}

fn note_spawn() {
    with_state(|s| s.spawned += 1);
}

pub fn js_trim_end(s: &str) -> &str {
    s.trim_end_matches(crate::gradient::is_js_space)
}

/// JS `s.split(/\s+/)`
pub fn js_split_ws(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut in_ws = false;
    let mut ws_start = 0;
    for (i, c) in s.char_indices() {
        let ws = crate::gradient::is_js_space(c);
        if ws && !in_ws {
            ws_start = i;
            in_ws = true;
        } else if !ws && in_ws {
            out.push(&s[start..ws_start]);
            start = i;
            in_ws = false;
        }
    }
    if in_ws {
        out.push(&s[start..ws_start]);
        out.push("");
    } else {
        out.push(&s[start..]);
    }
    out
}

// ------------------------------------------------------------ cwd & discovery

/// `resolveGitCwd(context)`
pub fn resolve_git_cwd(data: &StatusJson) -> Option<&str> {
    let ws = data.workspace.as_ref();
    [data.cwd.as_deref(), ws.and_then(|w| w.current_dir.as_deref()), ws.and_then(|w| w.project_dir.as_deref())]
        .into_iter()
        .flatten()
        .find(|c| !crate::jsnum::js_trim(c).is_empty())
}

fn mtime_ms(p: &str) -> Option<f64> {
    use std::os::unix::fs::MetadataExt;
    let m = std::fs::metadata(p).ok()?;
    Some(m.mtime() as f64 * 1e3 + m.mtime_nsec() as f64 / 1e6)
}

fn read_text(p: &str) -> String {
    std::fs::read(p).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default()
}

fn normalize_directory(candidate: &str) -> Option<String> {
    let resolved = resolve(candidate);
    let m = std::fs::metadata(&resolved).ok()?;
    Some(if m.is_dir() { resolved.clone() } else { dirname(&resolved) })
}

fn is_line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// `/^gitdir:\s*(\S.*)$/i` on the trimmed `.git` file
fn read_git_dir_file(git_file: &str) -> Option<String> {
    let content = read_text(git_file);
    let content = crate::jsnum::js_trim(&content);
    if content.len() < 7 || !content[..7].eq_ignore_ascii_case("gitdir:") {
        return None;
    }
    let rest = content[7..].trim_start_matches(crate::gradient::is_js_space);
    if rest.is_empty() || rest.contains(is_line_terminator) {
        return None;
    }
    Some(resolve_from(&dirname(git_file), rest))
}

fn discover_git_dir(start: &str) -> Option<String> {
    let mut current = start.to_string();
    loop {
        let git_path = join(&current, ".git");
        if let Ok(m) = std::fs::metadata(&git_path) {
            if m.is_dir() {
                return Some(git_path);
            }
            if m.is_file() {
                return read_git_dir_file(&git_path);
            }
        }
        let parent = dirname(&current);
        if parent == current {
            return None;
        }
        current = parent;
    }
}

fn find_work_tree_root(start: &str) -> Option<String> {
    let mut current = start.to_string();
    loop {
        if std::fs::metadata(join(&current, ".git")).is_ok() {
            return Some(current);
        }
        let parent = dirname(&current);
        if parent == current {
            return None;
        }
        current = parent;
    }
}

fn cache_dir() -> PathBuf {
    crate::settings::home_dir().join(".cache").join("ccstatusline")
}

fn metadata(cwd: Option<&str>) -> Option<Metadata> {
    let key = cwd.map(String::from);
    if let Some(m) = with_state(|s| s.metadata.get(&key).cloned()) {
        return m;
    }
    let md = (|| {
        let cwd = cwd?;
        let start = normalize_directory(cwd)?;
        let git_dir = discover_git_dir(&start)?;
        let hash = sha256::hex16(format!("{git_dir}\0{cwd}").as_bytes());
        Some(Metadata {
            cache_path: cache_dir().join("git-cache").join(format!("git-{hash}.json")),
            head: mtime_ms(&join(&git_dir, "HEAD")),
            index: mtime_ms(&join(&git_dir, "index")),
            git_dir,
        })
    })();
    with_state(|s| s.metadata.insert(key, md.clone()));
    md
}

/// `isDefinitelyOutsideGitRepository(cwd)`
fn definitely_outside(cwd: &str) -> bool {
    if let Some(v) = with_state(|s| s.outside.get(cwd).copied()) {
        return v;
    }
    let v = (|| {
        if ["GIT_DIR", "GIT_WORK_TREE", "GIT_COMMON_DIR"].iter().any(|k| std::env::var_os(k).is_some()) {
            return false;
        }
        let Ok(real) = std::fs::canonicalize(cwd) else { return false };
        if !real.is_dir() {
            return false;
        }
        let mut current = real.to_string_lossy().into_owned();
        loop {
            for marker in [".git", "HEAD"] {
                if std::fs::symlink_metadata(join(&current, marker)).is_ok() {
                    return false;
                }
            }
            let parent = dirname(&current);
            if parent == current {
                return true;
            }
            current = parent;
        }
    })();
    with_state(|s| s.outside.insert(cwd.to_string(), v));
    v
}

// ------------------------------------------------------------- filter drivers

fn may_define_filters(git_dir: &str) -> bool {
    let common = read_text(&join(git_dir, "commondir"));
    let common = crate::jsnum::js_trim(&common);
    let mut dirs = vec![git_dir.to_string()];
    if !common.is_empty() {
        dirs.push(resolve_from(git_dir, common));
    }
    let text = dirs
        .iter()
        .flat_map(|d| [join(d, "config"), join(d, "config.worktree")])
        .map(|p| read_text(&p))
        .collect::<Vec<_>>()
        .join("\n")
        .to_lowercase();
    text.contains("filter") || text.contains("include")
}

fn git_cmd<'a>(args: Vec<String>, cwd: Option<&'a str>) -> Cmd<'a> {
    let mut c = Cmd::new("git", args, cwd, COMMAND_TIMEOUT_MS);
    c.env.push(("GIT_OPTIONAL_LOCKS".into(), "0".into()));
    c
}

fn run_cmd(c: &Cmd) -> Result<String, Failure> {
    note_spawn();
    c.run()
}

fn spawn_cmd(c: &Cmd) -> Result<Pending, Failure> {
    note_spawn();
    c.spawn()
}

/// '' when nothing matches; None when git couldn't read the config
fn read_filter_config(args: &[&str], cwd: Option<&str>) -> Option<String> {
    let mut a: Vec<String> = hardening::HARDENING_ARGS.iter().map(|s| s.to_string()).collect();
    a.push("config".into());
    a.extend(args.iter().map(|s| s.to_string()));
    a.extend(["-z", "--get-regexp", "^filter\\."].iter().map(|s| s.to_string()));
    match run_cmd(&git_cmd(a, cwd)) {
        Ok(o) => Some(o),
        Err(f) if f.status == Some(1) => Some(String::new()),
        Err(_) => None,
    }
}

fn read_repository_filter_config(cwd: Option<&str>) -> Option<Vec<hardening::FilterEntry>> {
    if let Some(scoped) = read_filter_config(&["--includes", "--show-scope"], cwd) {
        return Some(hardening::parse_filter_config(&scoped, true));
    }
    let local = read_filter_config(&["--local", "--includes"], cwd)?;
    let worktree = read_filter_config(&["--worktree", "--includes"], cwd).unwrap_or_default();
    let mut v = hardening::parse_filter_config(&local, false);
    v.extend(hardening::parse_filter_config(&worktree, false));
    Some(v)
}

fn filter_override_args(cwd: Option<&str>) -> Option<Vec<String>> {
    let key = cwd.unwrap_or("").to_string();
    if let Some(v) = with_state(|s| s.filter.get(&key).cloned()) {
        return v;
    }
    let pcwd = paths::process_cwd();
    let start = normalize_directory(cwd.unwrap_or(&pcwd)).unwrap_or(pcwd);
    let mut overrides = Some(Vec::new());
    if let Some(git_dir) = discover_git_dir(&start) {
        if may_define_filters(&git_dir) {
            overrides = match read_repository_filter_config(cwd) {
                None => None,
                Some(entries) => {
                    hardening::filter_override_args(&entries, &find_work_tree_root(&start).unwrap_or(start.clone()))
                }
            };
        }
    }
    with_state(|s| s.filter.insert(key, overrides.clone()));
    overrides
}

/// `execGit(args, cwd)`, not yet waited on.
fn spawn_git(args: &[String], cwd: Option<&str>) -> Result<Pending, Failure> {
    let (sub, rest) = match args.split_first() {
        Some((s, r)) => (s.as_str(), r),
        None => ("", &[][..]),
    };
    let work_tree_args: &[&str] = match sub {
        "status" => &["--ignore-submodules=dirty"],
        "diff" => &["--no-ext-diff", "--no-textconv", "--ignore-submodules=dirty"],
        _ => &[],
    };
    let filter_args = if work_tree_args.is_empty() {
        Vec::new()
    } else {
        filter_override_args(cwd).ok_or_else(|| Failure {
            status: None,
            message: "The repository's filter drivers could not be turned off".into(),
            stderr: String::new(),
        })?
    };
    let mut full: Vec<String> = hardening::HARDENING_ARGS.iter().map(|s| s.to_string()).collect();
    full.extend(filter_args);
    full.push(sub.to_string());
    full.extend(work_tree_args.iter().map(|s| s.to_string()));
    full.extend(rest.iter().cloned());
    spawn_cmd(&git_cmd(full, cwd))
}

// ------------------------------------------------------------------ caches

fn ttl_ms(ctx: &RenderContext) -> f64 {
    let t = ctx.settings.git_cache_ttl_seconds;
    if !t.is_finite() {
        return DEFAULT_TTL_SECONDS * 1000.0;
    }
    t.clamp(0.0, 60.0) * 1000.0
}

fn is_fresh(e: &Entry, md: Option<&Metadata>, ttl: f64, now: f64) -> bool {
    if let Some(m) = md {
        if e.head != m.head || e.index != m.index {
            return false;
        }
    }
    ttl == 0.0 || now - e.created_at <= ttl
}

fn num_or_null(v: &Value) -> Option<Option<f64>> {
    match v {
        Value::Null => Some(None),
        Value::Number(n) => Some(n.as_f64()),
        _ => None,
    }
}

fn parse_entry(v: &Value) -> Option<Entry> {
    let o = v.as_object()?;
    let output = match o.get("output")? {
        Value::String(s) => Some(s.clone()),
        Value::Null => None,
        _ => return None,
    };
    let created_at = o.get("createdAt")?.as_f64()?;
    Some(Entry {
        output,
        created_at,
        head: num_or_null(o.get("headMtimeMs")?)?,
        index: num_or_null(o.get("indexMtimeMs")?)?,
    })
}

fn read_persistent(path: &Path) -> Option<Persistent> {
    let text = std::fs::read(path).ok()?;
    let v: Value = serde_json::from_slice(&text).ok()?;
    let o = v.as_object()?;
    if o.get("version").and_then(|x| x.as_f64()) != Some(1.0) {
        return None;
    }
    let cwd = match o.get("cwd") {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Null) => None,
        _ => return None,
    };
    let entries = match o.get("entries") {
        Some(Value::Object(m)) => m.iter().filter_map(|(k, v)| parse_entry(v).map(|e| (k.clone(), e))).collect(),
        Some(Value::Array(a)) => {
            a.iter().enumerate().filter_map(|(i, v)| parse_entry(v).map(|e| (i.to_string(), e))).collect()
        }
        _ => return None,
    };
    Some(Persistent { cwd, entries })
}

fn entry_json(e: &Entry) -> Value {
    let mut m = Map::new();
    m.insert("output".into(), e.output.clone().map(Value::String).unwrap_or(Value::Null));
    m.insert("createdAt".into(), crate::jsjson::num(e.created_at));
    m.insert("headMtimeMs".into(), e.head.map(crate::jsjson::num).unwrap_or(Value::Null));
    m.insert("indexMtimeMs".into(), e.index.map(crate::jsjson::num).unwrap_or(Value::Null));
    Value::Object(m)
}

fn persistent_lookup(md: &Metadata, key: &str, cwd: Option<&str>, ttl: f64, now: f64) -> Option<Entry> {
    let path = md.cache_path.clone();
    let loaded = with_state(|s| s.persistent.contains_key(&path));
    if !loaded {
        let p = read_persistent(&path);
        with_state(|s| s.persistent.insert(path.clone(), p));
    }
    with_state(|s| {
        let p = s.persistent.get(&path)?.as_ref()?;
        if p.cwd.as_deref() != cwd {
            return None;
        }
        let e = p.entries.iter().rev().find(|(k, _)| k == key).map(|(_, e)| e.clone())?;
        is_fresh(&e, Some(md), ttl, now).then_some(e)
    })
}

/// `writePersistentCacheEntry` for several entries at once: re-read, merge, write.
fn persistent_write(md: &Metadata, cwd: Option<&str>, new: Vec<(String, Entry)>) {
    if new.is_empty() {
        return;
    }
    let mut cache = match read_persistent(&md.cache_path) {
        Some(p) if p.cwd.as_deref() == cwd => p,
        _ => Persistent { cwd: cwd.map(String::from), entries: Vec::new() },
    };
    for (k, e) in new {
        match cache.entries.iter_mut().find(|(ek, _)| *ek == k) {
            Some(slot) => slot.1 = e,
            None => cache.entries.push((k, e)),
        }
    }
    let mut entries = Map::new();
    for (k, e) in &cache.entries {
        entries.insert(k.clone(), entry_json(e));
    }
    let mut root = Map::new();
    root.insert("version".into(), Value::from(1));
    root.insert("cwd".into(), cache.cwd.clone().map(Value::String).unwrap_or(Value::Null));
    root.insert("entries".into(), Value::Object(entries));
    let _ = crate::cachefile::atomic_write(&md.cache_path, crate::jsjson::stringify(&Value::Object(root)).as_bytes());
    let path = md.cache_path.clone();
    with_state(|s| s.persistent.insert(path, Some(cache)));
}

fn new_entry(output: Option<String>, md: Option<&Metadata>, now: f64) -> Entry {
    Entry { output, created_at: now, head: md.and_then(|m| m.head), index: md.and_then(|m| m.index) }
}

fn output_of(r: Result<String, Failure>) -> Option<String> {
    match r {
        Ok(o) => {
            let t = js_trim_end(&o);
            (!t.is_empty()).then(|| t.to_string())
        }
        Err(_) => None,
    }
}

/// A fresh cached result for `key`: Some(output) on a hit.
fn cached(ctx: &RenderContext, key: &str) -> Option<Option<String>> {
    let cwd = resolve_git_cwd(ctx.data);
    let mem_key = format!("{key}|{}", cwd.unwrap_or(""));
    let md = metadata(cwd);
    let (ttl, now) = (ttl_ms(ctx), crate::clock::now_ms());
    if let Some(e) = with_state(|s| s.mem.get(&mem_key).cloned()) {
        if is_fresh(&e, md.as_ref(), ttl, now) {
            return Some(e.output);
        }
    }
    let e = persistent_lookup(md.as_ref()?, key, cwd, ttl, now)?;
    with_state(|s| s.mem.insert(mem_key, e.clone()));
    Some(e.output)
}

fn store(ctx: &RenderContext, results: Vec<(String, Option<String>)>, now: f64) {
    let cwd = resolve_git_cwd(ctx.data);
    let md = metadata(cwd);
    let mut new = Vec::new();
    for (key, out) in results {
        let e = new_entry(out, md.as_ref(), now);
        with_state(|s| s.mem.insert(format!("{key}|{}", cwd.unwrap_or("")), e.clone()));
        new.push((key, e));
    }
    if let Some(m) = md {
        persistent_write(&m, cwd, new);
    }
}

/// `runGit(command, context)`
pub fn run_git(ctx: &RenderContext, command: &str) -> Option<String> {
    let args: Vec<&str> = command.split_whitespace().collect();
    run_git_args(ctx, &args, Some(command))
}

/// `runGitArgs(args, context, cacheCommand)`
pub fn run_git_args(ctx: &RenderContext, args: &[&str], cache_command: Option<&str>) -> Option<String> {
    prefetch(ctx);
    let cwd = resolve_git_cwd(ctx.data);
    let token = cache_command.map(String::from).unwrap_or_else(|| args.join("\0"));
    let md = metadata(cwd);
    if md.is_none() {
        if let Some(c) = cwd {
            if definitely_outside(c) {
                return None;
            }
        }
    }
    if let Some(hit) = cached(ctx, &token) {
        return hit;
    }
    let now = crate::clock::now_ms();
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let out = output_of(spawn_git(&args, cwd).and_then(|p| p.wait()));
    store(ctx, vec![(token, out.clone())], now);
    out
}

// ---------------------------------------------------------------- prefetch

struct Plan {
    inside: bool,
    toplevel: bool,
    worktree_dirs: bool,
    /// HEAD looks readable without git: answer `symbolic-ref HEAD` from it
    native_head: bool,
    /// git-review/ci: the git dir, to name the review cache without a spawn
    git_dir_hint: bool,
    keys: Vec<(String, Vec<String>)>,
}

impl Plan {
    fn add(&mut self, key: &str) {
        if !self.keys.iter().any(|(k, _)| k == key) {
            self.keys.push((key.to_string(), key.split_whitespace().map(String::from).collect()));
        }
    }
    fn add_remote(&mut self, name: &str) {
        let key = format!("remote get-url -- {name}");
        if !self.keys.iter().any(|(k, _)| *k == key) {
            self.keys.push((key, ["remote", "get-url", "--", name].iter().map(|s| s.to_string()).collect()));
        }
    }
}

const GATED: &[&str] = &[
    "git-branch",
    "git-changes",
    "git-insertions",
    "git-deletions",
    "git-staged-files",
    "git-unstaged-files",
    "git-untracked-files",
    "git-clean-status",
    "git-root-dir",
    "git-review",
    "git-ci-status",
    "git-worktree",
    "git-status",
    "git-staged",
    "git-unstaged",
    "git-untracked",
    "git-ahead-behind",
    "git-conflicts",
    "git-sha",
];

fn make_plan(ctx: &RenderContext, md: &Metadata) -> Plan {
    let mut p = Plan {
        inside: false,
        toplevel: false,
        worktree_dirs: false,
        native_head: false,
        git_dir_hint: false,
        keys: Vec::new(),
    };
    for item in ctx.settings.lines.iter().flatten() {
        let kind = crate::widgets::resolve_legacy_type(&item.kind);
        if GATED.contains(&kind) {
            p.inside = true;
        }
        match kind {
            "git-branch" => {
                if head_file_ref(&md.git_dir).is_some() {
                    p.native_head = true;
                } else {
                    p.add(K_SYMBOLIC_REF);
                    p.add(K_SHORT_SHA);
                }
                if crate::widgets::git::branch_link_enabled(item) {
                    p.add_remote("origin");
                }
            }
            "git-root-dir" => p.toplevel = true,
            "git-changes" | "git-insertions" | "git-deletions" => {
                p.add(K_DIFF);
                p.add(K_DIFF_CACHED);
            }
            "git-staged-files"
            | "git-unstaged-files"
            | "git-untracked-files"
            | "git-clean-status"
            | "git-status"
            | "git-staged"
            | "git-unstaged"
            | "git-untracked" => p.add(K_STATUS),
            "git-ahead-behind" => p.add(K_AHEAD_BEHIND),
            "git-conflicts" => p.add(K_CONFLICTS),
            "git-sha" => p.add(K_SHORT_SHA),
            "git-worktree" => p.worktree_dirs = true,
            "git-review" | "git-ci-status" => p.git_dir_hint = true,
            "git-origin-owner" | "git-origin-repo" => p.add_remote("origin"),
            "git-origin-owner-repo" => {
                p.add_remote("origin");
                if crate::widgets::shared::is_flag(item, "ownerOnlyWhenFork") {
                    p.add_remote("upstream");
                    p.add_remote("fork");
                }
            }
            "git-upstream-owner" | "git-upstream-repo" | "git-upstream-owner-repo" => {
                p.add_remote("upstream");
                p.add(K_UPSTREAM_REF);
                p.add(K_REMOTES);
            }
            "git-is-fork" => {
                p.add_remote("origin");
                p.add_remote("upstream");
                p.add_remote("fork");
            }
            _ => {}
        }
    }
    p
}

/// Runs every not-yet-cached command the configured widgets need, at once.
fn prefetch(ctx: &RenderContext) {
    if ctx.is_preview || with_state(|s| std::mem::replace(&mut s.prefetched, true)) {
        return;
    }
    let cwd = resolve_git_cwd(ctx.data);
    let Some(md) = metadata(cwd) else { return };
    let plan = make_plan(ctx, &md);
    let missing = |k: &str| cached(ctx, k).is_none();

    // rev-parse queries, in output-line order
    let mut flags: Vec<(&str, &str)> = Vec::new();
    if plan.inside && missing(K_INSIDE) {
        flags.push(("--is-inside-work-tree", K_INSIDE));
    }
    if plan.toplevel && missing(K_TOPLEVEL) {
        flags.push(("--show-toplevel", K_TOPLEVEL));
    }
    let wt = plan.worktree_dirs && missing(K_WORKTREE_DIRS);
    let native = plan.native_head && missing(K_SYMBOLIC_REF);
    let gd = (native || plan.git_dir_hint) && missing(K_GIT_DIR);
    if wt || gd {
        flags.push(("--git-dir", K_GIT_DIR));
    }
    if wt {
        flags.push(("--git-common-dir", K_WORKTREE_DIRS));
    }
    let mut others: Vec<(String, Vec<String>)> = plan.keys.into_iter().filter(|(k, _)| missing(k)).collect();
    if flags.is_empty() && others.is_empty() {
        return;
    }

    let now = crate::clock::now_ms();
    let mut combined: Option<Result<Pending, Failure>> = None;
    let mut rev_keys: Vec<(String, Vec<String>)> = Vec::new();
    if flags.len() >= 2 {
        let mut args: Vec<String> = vec!["rev-parse".into()];
        args.extend(flags.iter().map(|(f, _)| f.to_string()));
        combined = Some(spawn_git(&args, cwd));
    } else {
        for (_, k) in &flags {
            rev_keys.push((k.to_string(), k.split_whitespace().map(String::from).collect()));
        }
    }
    // commands that don't re-read the work tree first; status/diff may need
    // `git config` to settle their filter overrides
    others.sort_by_key(|(_, a)| matches!(a[0].as_str(), "status" | "diff"));
    others.extend(rev_keys);
    let pending: Vec<(String, Result<Pending, Failure>)> =
        others.into_iter().map(|(k, a)| (k, spawn_git(&a, cwd))).collect();

    let mut results: Vec<(String, Option<String>)> = Vec::new();
    if let Some(c) = combined {
        let parsed = c.and_then(|p| p.wait()).ok().and_then(|out| {
            let lines: Vec<&str> = out.split('\n').collect();
            (lines.len() == flags.len() + 1 && lines.last() == Some(&""))
                .then(|| lines[..flags.len()].iter().map(|s| s.to_string()).collect::<Vec<_>>())
        });
        match parsed {
            Some(lines) => {
                let line = |flag: &str| flags.iter().position(|(f, _)| *f == flag).map(|i| lines[i].clone());
                let single = |s: String| {
                    let t = js_trim_end(&s);
                    (!t.is_empty()).then(|| t.to_string())
                };
                for (flag, key) in &flags {
                    match *flag {
                        "--git-dir" => {
                            if gd {
                                results.push((K_GIT_DIR.into(), single(line("--git-dir").unwrap())));
                            }
                        }
                        "--git-common-dir" => {
                            let both = format!("{}\n{}", line("--git-dir").unwrap(), line("--git-common-dir").unwrap());
                            results.push((key.to_string(), single(both)));
                        }
                        f => results.push((key.to_string(), single(line(f).unwrap()))),
                    }
                }
            }
            None => {
                // run them one by one, exactly as upstream would
                let mut keys: Vec<&str> = flags.iter().map(|(_, k)| *k).collect();
                if wt && !gd {
                    keys.retain(|k| *k != K_GIT_DIR);
                }
                let spawned: Vec<(&str, Result<Pending, Failure>)> = keys
                    .iter()
                    .map(|k| (*k, spawn_git(&k.split_whitespace().map(String::from).collect::<Vec<_>>(), cwd)))
                    .collect();
                for (k, p) in spawned {
                    results.push((k.to_string(), output_of(p.and_then(|p| p.wait()))));
                }
            }
        }
    }
    for (k, p) in pending {
        results.push((k, output_of(p.and_then(|p| p.wait()))));
    }
    if native {
        let known = |key: &str| match results.iter().find(|(k, _)| k == key) {
            Some((_, v)) => v.clone(),
            None => cached(ctx, key).flatten(),
        };
        if let (Some("true"), Some(git_dir), Some(c)) = (known(K_INSIDE).as_deref(), known(K_GIT_DIR), cwd) {
            // what `git symbolic-ref HEAD` prints for this HEAD file
            if let Some(r) = head_file_ref(&resolve_from(&resolve(c), &git_dir)) {
                results.push((K_SYMBOLIC_REF.to_string(), Some(r)));
            }
        }
    }
    store(ctx, results, now);
}

// ------------------------------------------------------------- HEAD file

/// The branch ref HEAD points at, read from `<git dir>/HEAD`, when the file
/// is an ordinary `ref: refs/...` line in a files-backend repository.
fn head_file_ref(git_dir: &str) -> Option<String> {
    let head = join(git_dir, "HEAD");
    if !std::fs::symlink_metadata(&head).ok()?.is_file() {
        return None;
    }
    let content = std::fs::read_to_string(&head).ok()?;
    let target = content.strip_prefix("ref: ")?.strip_suffix('\n')?;
    if !target.starts_with("refs/") || target == "refs/heads/.invalid" {
        return None;
    }
    // only names `git check-ref-format` surely accepts; git decides the rest
    let plain = |c: char| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '.' | '-' | '+') || (c as u32) > 0x7f;
    if !target.chars().all(|c| plain(c) && !c.is_whitespace() && !c.is_control())
        || target.contains("..")
        || target.contains("//")
        || target.ends_with('/')
        || target.ends_with('.')
        || target.split('/').any(|seg| seg.is_empty() || seg.starts_with('.') || seg.ends_with(".lock"))
    {
        return None;
    }
    // reftable keeps the real HEAD elsewhere
    let common = read_text(&join(git_dir, "commondir"));
    let common = crate::jsnum::js_trim(&common);
    let mut dirs = vec![git_dir.to_string()];
    if !common.is_empty() {
        dirs.push(resolve_from(git_dir, common));
    }
    for d in &dirs {
        for f in ["config", "config.worktree"] {
            if read_text(&join(d, f)).to_ascii_lowercase().contains("refstorage") {
                return None;
            }
        }
    }
    Some(target.to_string())
}

/// The git dir git reported for this render's cwd, when already known.
pub fn known_git_dir(ctx: &RenderContext) -> Option<String> {
    let cwd = resolve_git_cwd(ctx.data)?;
    let v = cached(ctx, K_GIT_DIR)??;
    Some(resolve_from(&resolve(cwd), &v))
}

/// What `git symbolic-ref --short HEAD` prints, from the HEAD file, when no
/// other ref could make git shorten it differently (`refs/<name>`,
/// `refs/tags/<name>`, or a file `<name>` in the git dir).
pub fn native_short_branch(git_dir: &str) -> Option<String> {
    let target = head_file_ref(git_dir)?;
    let name = target.strip_prefix("refs/heads/")?;
    if name.is_empty() || name.contains("..") || name.contains("//") || name.ends_with('/') {
        return None;
    }
    let common = read_text(&join(git_dir, "commondir"));
    let common = crate::jsnum::js_trim(&common);
    let common = if common.is_empty() { git_dir.to_string() } else { resolve_from(git_dir, common) };
    let rivals = [name.to_string(), format!("refs/{name}"), format!("refs/tags/{name}")];
    for base in [git_dir, common.as_str()] {
        for r in &rivals {
            if std::fs::symlink_metadata(join(base, r)).is_ok() {
                return None;
            }
        }
        let packed = read_text(&join(base, "packed-refs"));
        for line in packed.lines() {
            if let Some((_, refname)) = line.split_once(' ') {
                if rivals[1..].iter().any(|r| r == refname) {
                    return None;
                }
            }
        }
    }
    Some(name.to_string())
}

/// `runGit('symbolic-ref HEAD', context)`
pub fn symbolic_ref_head(ctx: &RenderContext) -> Option<String> {
    run_git(ctx, K_SYMBOLIC_REF)
}

// ------------------------------------------------------------- queries

/// `isInsideGitWorkTree(context)`
pub fn is_inside_work_tree(ctx: &RenderContext) -> bool {
    run_git(ctx, K_INSIDE).as_deref() == Some("true")
}

/// `/(\d+)\s+insertions?/`-style count, or 0
fn stat_count(stat: &str, word: &str) -> f64 {
    let b = stat.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_digit() {
            let start = i;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            let rest = &stat[i..];
            let after_ws = rest.trim_start_matches(crate::gradient::is_js_space);
            if after_ws.len() < rest.len() && after_ws.starts_with(word) {
                return crate::jsnum::parse_int(&stat[start..i]).unwrap_or(0.0);
            }
            // a later start inside the same run of digits can't match
            // differently: the run is followed by the same text
            continue;
        }
        i += 1;
    }
    0.0
}

pub fn parse_diff_stat(stat: &str) -> (f64, f64) {
    (stat_count(stat, "insertion"), stat_count(stat, "deletion"))
}

/// `getGitChangeCounts(context)`: (insertions, deletions)
pub fn change_counts(ctx: &RenderContext) -> (f64, f64) {
    let unstaged = run_git(ctx, K_DIFF).unwrap_or_default();
    let staged = run_git(ctx, K_DIFF_CACHED).unwrap_or_default();
    let (ui, ud) = parse_diff_stat(&unstaged);
    let (si, sd) = parse_diff_stat(&staged);
    (ui + si, ud + sd)
}

#[derive(Clone, Copy, Default)]
pub struct GitStatus {
    pub staged: bool,
    pub unstaged: bool,
    pub untracked: bool,
    pub conflicts: bool,
}

/// The status line's first char and its second UTF-16 unit (None when the
/// first char is astral, so that unit is a low surrogate).
fn status_chars(line: &str) -> (Option<char>, Option<char>) {
    let mut it = line.chars();
    let a = it.next();
    let b = match a {
        Some(c) if c.len_utf16() == 2 => None,
        _ => it.next(),
    };
    (a, b)
}

fn is_status_code(c: Option<char>) -> bool {
    matches!(c, Some('M' | 'A' | 'D' | 'R' | 'C' | 'T' | 'U'))
}

fn has_rename_or_copy(line: &str) -> bool {
    let (a, b) = status_chars(line);
    matches!(a, Some('R' | 'C')) || matches!(b, Some('R' | 'C'))
}

/// `/^.[MADRCTU]/`
fn second_is_code(line: &str) -> bool {
    let (a, b) = status_chars(line);
    a.is_some_and(|c| !is_line_terminator(c)) && is_status_code(b)
}

fn utf16_len_lt2(line: &str) -> bool {
    line.encode_utf16().nth(1).is_none()
}

/// `getGitStatus(context)`
pub fn git_status(ctx: &RenderContext) -> GitStatus {
    let mut st = GitStatus::default();
    let Some(output) = run_git(ctx, K_STATUS) else { return st };
    let entries: Vec<&str> = output.split('\0').collect();
    let mut i = 0;
    while i < entries.len() {
        let line = entries[i];
        i += 1;
        if utf16_len_lt2(line) {
            continue;
        }
        if !st.conflicts && ["DD", "AU", "UD", "UA", "DU", "AA", "UU"].iter().any(|p| line.starts_with(p)) {
            st.conflicts = true;
        }
        if !st.staged && is_status_code(line.chars().next()) {
            st.staged = true;
        }
        if !st.unstaged && second_is_code(line) {
            st.unstaged = true;
        }
        if !st.untracked && line.starts_with("??") {
            st.untracked = true;
        }
        if st.staged && st.unstaged && st.untracked && st.conflicts {
            break;
        }
        if has_rename_or_copy(line) {
            i += 1;
        }
    }
    st
}

/// `getGitFileStatusCounts(context)`: (staged, unstaged, untracked)
pub fn file_status_counts(ctx: &RenderContext) -> (usize, usize, usize) {
    let Some(output) = run_git(ctx, K_STATUS) else { return (0, 0, 0) };
    let (mut staged, mut unstaged, mut untracked) = (0, 0, 0);
    let entries: Vec<&str> = output.split('\0').collect();
    let mut i = 0;
    while i < entries.len() {
        let line = entries[i];
        i += 1;
        if utf16_len_lt2(line) {
            continue;
        }
        if line.starts_with("??") {
            untracked += 1;
        } else {
            if is_status_code(line.chars().next()) {
                staged += 1;
            }
            if second_is_code(line) {
                unstaged += 1;
            }
        }
        if has_rename_or_copy(line) {
            i += 1;
        }
    }
    (staged, unstaged, untracked)
}

/// `getGitAheadBehind(context)`
pub fn ahead_behind(ctx: &RenderContext) -> Option<(f64, f64)> {
    let output = run_git(ctx, K_AHEAD_BEHIND)?;
    let parts = js_split_ws(&output);
    if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
        return None;
    }
    let ahead = crate::jsnum::parse_int(parts[0])?;
    let behind = crate::jsnum::parse_int(parts[1])?;
    Some((ahead, behind))
}

/// `getGitConflictCount(context)`
pub fn conflict_count(ctx: &RenderContext) -> usize {
    let Some(output) = run_git(ctx, K_CONFLICTS) else { return 0 };
    let mut seen: Vec<String> = Vec::new();
    for line in output.split('\n') {
        let parts = js_split_ws(line);
        let path = parts.iter().skip(3).copied().collect::<Vec<_>>().join(" ");
        if !path.is_empty() && !seen.contains(&path) {
            seen.push(path);
        }
    }
    seen.len()
}

/// `getGitShortSha(context)`
pub fn short_sha(ctx: &RenderContext) -> Option<String> {
    run_git(ctx, K_SHORT_SHA)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_ws() {
        assert_eq!(js_split_ws("2\t1"), vec!["2", "1"]);
        assert_eq!(js_split_ws(" a  b "), vec!["", "a", "b", ""]);
        assert_eq!(js_split_ws(""), vec![""]);
    }

    #[test]
    fn head_file() {
        let dir = std::env::temp_dir().join(format!("ccsl-git-head-{}", std::process::id()));
        let g = dir.to_string_lossy().into_owned();
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("refs/tags")).unwrap();
        std::fs::write(dir.join("config"), "[core]\n\tbare = false\n").unwrap();
        let head = |s: &str| std::fs::write(dir.join("HEAD"), s).unwrap();
        head("ref: refs/heads/feature/x\n");
        assert_eq!(head_file_ref(&g).as_deref(), Some("refs/heads/feature/x"));
        assert_eq!(native_short_branch(&g).as_deref(), Some("feature/x"));
        std::fs::create_dir_all(dir.join("refs/tags/feature")).unwrap();
        std::fs::write(dir.join("refs/tags/feature/x"), "0\n").unwrap();
        assert_eq!(native_short_branch(&g), None);
        head("ref: refs/heads/main\n");
        std::fs::write(dir.join("packed-refs"), "# pack-refs\nabc refs/tags/main\n").unwrap();
        assert_eq!(native_short_branch(&g), None);
        head("ref: refs/heads/hooks\n");
        std::fs::create_dir_all(dir.join("hooks")).unwrap();
        assert_eq!(native_short_branch(&g), None);
        for bad in ["ref:refs/heads/main\n", "ref: refs/heads/main", "ref: refs/heads/a b\n", "0123abcd\n"] {
            head(bad);
            assert_eq!(head_file_ref(&g), None, "{bad:?}");
        }
        head("ref: refs/heads/main\n");
        std::fs::write(dir.join("config"), "[extensions]\n\trefStorage = reftable\n").unwrap();
        assert_eq!(head_file_ref(&g), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn diff_stat() {
        assert_eq!(parse_diff_stat(" 3 files changed, 10 insertions(+), 1 deletion(-)"), (10.0, 1.0));
        assert_eq!(parse_diff_stat(" 1 file changed, 2 deletions(-)"), (0.0, 2.0));
        assert_eq!(parse_diff_stat(""), (0.0, 0.0));
    }
}
