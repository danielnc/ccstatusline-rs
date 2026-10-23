//! Transcript-derived data (token metrics, speed, session duration,
//! compaction stats, thinking effort, session name), computed incrementally.
//!
//! Upstream (`getTranscriptAnalysis`) reads and `JSON.parse`s the whole
//! transcript on every render; long sessions reach hundreds of MB. Here the
//! running aggregates (`state::Aggregates`) are cached per transcript under
//! `~/.cache/ccstatusline-rs/transcripts/`, keyed by the resolved path and
//! validated by device + inode + a fingerprint of the bytes before the saved
//! offset. A render only parses the complete lines appended since; a missing
//! or invalid cache is rebuilt by a streaming pass capped at a wall-time
//! budget, whose progress is saved so the next render continues from there.
//! Until the pass is complete the accessors return None and the widgets fall
//! back to the status JSON, as upstream does when there is no transcript.
//!
//! Nothing here runs unless a widget asks for it (`analysis` is lazy and
//! memoized for the process).

pub mod blocks;
pub mod codec;
pub mod json;
pub mod skills;
pub mod state;
pub mod tail;

use crate::context::RenderContext;
use state::{Aggregates, Compaction, Effort, SpeedCursor, SpeedMetrics};
use std::collections::BTreeSet;
use std::fs::File;
use std::os::unix::fs::{FileExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TokenMetrics {
    pub input_tokens: f64,
    pub output_tokens: f64,
    pub cached_tokens: f64,
    pub cache_read_tokens: f64,
    pub cache_creation_tokens: f64,
    pub total_tokens: f64,
    pub context_length: f64,
}

/// Where a file has been read up to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileCursor {
    pub dev: u64,
    pub ino: u64,
    /// just past the last complete line read
    pub offset: u64,
    /// `fingerprint(file, offset)`
    pub fingerprint: u64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SubagentCursor {
    pub path: String,
    pub file: FileCursor,
    pub speed: SpeedCursor,
}

/// The persisted state for one transcript.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cache {
    pub path: String,
    pub main: FileCursor,
    pub agg: Aggregates,
    pub subagents: Vec<SubagentCursor>,
}

/// One render's view of a transcript.
#[derive(Clone, Debug, Default)]
pub struct Analysis {
    /// upstream's `emptyResult`: the transcript is missing or unreadable
    pub empty: bool,
    /// every line of the main transcript is in `agg`
    pub main_complete: bool,
    /// ...and of the referenced subagent transcripts (speed is exact)
    pub speed_complete: bool,
    pub agg: Aggregates,
}

const DEFAULT_BUDGET_MS: u64 = 150;
const READ_CHUNK: usize = 1 << 20;
const FINGERPRINT_BYTES: u64 = 64;

fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// Hash of the file's first bytes and of the bytes just before `offset`:
/// catches a transcript replaced in place (same inode, not shorter).
fn fingerprint(file: &File, offset: u64) -> std::io::Result<u64> {
    let mut head = vec![0u8; offset.min(FINGERPRINT_BYTES) as usize];
    file.read_exact_at(&mut head, 0)?;
    let start = offset.saturating_sub(FINGERPRINT_BYTES);
    let mut tail = vec![0u8; (offset - start) as usize];
    file.read_exact_at(&mut tail, start)?;
    Ok(fnv64(&head) ^ fnv64(&tail).rotate_left(1))
}

fn cursor_is_valid(c: &FileCursor, file: &File, meta: &std::fs::Metadata) -> bool {
    c.dev == meta.dev()
        && c.ino == meta.ino()
        && c.offset <= meta.len()
        && fingerprint(file, c.offset).is_ok_and(|f| f == c.fingerprint)
}

#[inline]
fn find_newline(b: &[u8], mut i: usize) -> Option<usize> {
    const LO: u64 = 0x0101_0101_0101_0101;
    const HI: u64 = 0x8080_8080_8080_8080;
    while i + 8 <= b.len() {
        let w = u64::from_le_bytes(b[i..i + 8].try_into().unwrap()) ^ (LO * b'\n' as u64);
        if w.wrapping_sub(LO) & !w & HI != 0 {
            break;
        }
        i += 8;
    }
    b[i..].iter().position(|&c| c == b'\n').map(|k| i + k)
}

/// The text of a JSONL record as `JsonlLineSplitter` yields it: BOM dropped
/// on the file's first line, one trailing CR dropped; None when empty.
fn record(line: &[u8], at_file_start: bool) -> Option<&[u8]> {
    let mut l = line;
    if at_file_start {
        l = l.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(l);
    }
    l = l.strip_suffix(b"\r").unwrap_or(l);
    (!l.is_empty()).then_some(l)
}

struct ScanEnd {
    /// just past the last complete line handed to the callback
    offset: u64,
    /// bytes after the last newline (when the scan reached `to`)
    partial: Option<Vec<u8>>,
    timed_out: bool,
}

/// Feed the complete lines in `[from, to)` to `f` (with whether the line
/// starts the file), reading in bounded chunks; stops after the deadline.
fn scan_lines(
    file: &File,
    from: u64,
    to: u64,
    deadline: Option<Instant>,
    mut f: impl FnMut(&[u8], bool),
) -> std::io::Result<ScanEnd> {
    let mut buf = vec![0u8; (to.saturating_sub(from)).clamp(1, READ_CHUNK as u64) as usize];
    let mut carry: Vec<u8> = Vec::new();
    let mut pos = from;
    let mut line_start = from;
    let mut lines = 0u32;
    while pos < to {
        let n = (buf.len() as u64).min(to - pos) as usize;
        let got = file.read_at(&mut buf[..n], pos)?;
        if got == 0 {
            break;
        }
        let chunk = &buf[..got];
        let mut s = 0;
        while let Some(end) = find_newline(chunk, s) {
            if carry.is_empty() {
                f(&chunk[s..end], line_start == 0);
            } else {
                carry.extend_from_slice(&chunk[s..end]);
                f(&carry, line_start == 0);
                carry.clear();
            }
            line_start = pos + end as u64 + 1;
            s = end + 1;
            lines += 1;
            if lines.is_multiple_of(32) && deadline.is_some_and(|d| Instant::now() >= d) {
                return Ok(ScanEnd { offset: line_start, partial: None, timed_out: true });
            }
        }
        carry.extend_from_slice(&chunk[s..]);
        pos += got as u64;
    }
    Ok(ScanEnd { offset: line_start, partial: (!carry.is_empty()).then_some(carry), timed_out: false })
}

/// Apply one raw main-transcript line.
fn apply_main_line(agg: &mut Aggregates, raw: &[u8], at_start: bool, ids: &mut Vec<String>) {
    if let Some(rec) = record(raw, at_start) {
        // agent ids only matter for speed (which subagent files to read)
        let opts = json::Options { agent_ids: agg.speed_on, content_first: false };
        let line = json::parse_line(rec, &opts, ids);
        agg.apply_main(line.as_ref(), ids);
    }
}

fn apply_subagent_line(agg: &mut Aggregates, cur: &mut SpeedCursor, raw: &[u8], at_start: bool, ids: &mut Vec<String>) {
    if let Some(rec) = record(raw, at_start) {
        let line = json::parse_line(rec, &json::Options { agent_ids: false, content_first: false }, ids);
        agg.apply_subagent(cur, line.as_ref());
    }
}

/// `getSubagentTranscriptPaths`: files named `agent-<id>.jsonl` (with a
/// referenced id) in `<dir>/subagents` and `<dir>/<stem>/subagents`.
fn subagent_paths(transcript: &str, ids: &BTreeSet<String>) -> Vec<String> {
    if ids.is_empty() {
        return vec![];
    }
    let p = Path::new(transcript);
    let dir = p.parent().unwrap_or(Path::new(""));
    let stem = p.file_stem().map(|s| s.to_os_string()).unwrap_or_default();
    let mut out: Vec<String> = Vec::new();
    for sub in [dir.join("subagents"), dir.join(stem).join("subagents")] {
        let Ok(rd) = std::fs::read_dir(&sub) else { continue };
        for entry in rd.flatten() {
            if !entry.file_type().is_ok_and(|t| t.is_file()) {
                continue;
            }
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            // /^agent-(.+)\.jsonl$/ (`.` excludes line terminators)
            let Some(id) = name.strip_prefix("agent-").and_then(|r| r.strip_suffix(".jsonl")) else { continue };
            if id.is_empty() || id.contains(['\n', '\r', '\u{2028}', '\u{2029}']) || !ids.contains(id) {
                continue;
            }
            let full = sub.join(name).to_string_lossy().into_owned();
            if !out.contains(&full) {
                out.push(full);
            }
        }
    }
    out
}

struct Ingest {
    deadline: Option<Instant>,
}

enum Sub {
    Done {
        partial: Option<Vec<u8>>,
    },
    TimedOut,
    /// a tracked file changed under us: drop the cache
    Invalid,
    /// unreadable: upstream skips the file
    Skip,
}

impl Ingest {
    /// Bring one subagent file up to date in `cache`.
    fn subagent(&self, cache: &mut Cache, path: &str, ids: &mut Vec<String>) -> Sub {
        let Ok(file) = File::open(path) else { return Sub::Skip };
        let Ok(meta) = file.metadata() else { return Sub::Skip };
        let idx = cache.subagents.iter().position(|s| s.path == path);
        let mut sa = match idx {
            Some(i) => {
                if !cursor_is_valid(&cache.subagents[i].file, &file, &meta) {
                    return Sub::Invalid;
                }
                cache.subagents[i].clone()
            }
            None => SubagentCursor {
                path: path.to_string(),
                file: FileCursor { dev: meta.dev(), ino: meta.ino(), offset: 0, fingerprint: fnv64(&[]) },
                speed: SpeedCursor::default(),
            },
        };
        let agg = &mut cache.agg;
        let scan = scan_lines(&file, sa.file.offset, meta.len(), self.deadline, |raw, at0| {
            apply_subagent_line(agg, &mut sa.speed, raw, at0, ids)
        });
        let Ok(scan) = scan else {
            // a read error after some lines were applied leaves no way back
            return Sub::Invalid;
        };
        sa.file.offset = scan.offset;
        sa.file.fingerprint = fingerprint(&file, scan.offset).unwrap_or(0);
        match idx {
            Some(i) => cache.subagents[i] = sa,
            None => cache.subagents.push(sa),
        }
        if scan.timed_out {
            Sub::TimedOut
        } else {
            Sub::Done { partial: scan.partial }
        }
    }
}

/// Drop cache files of transcripts not rendered for two weeks (run only when
/// a new cache file is about to be created, so rarely).
fn prune_cache_dir(dir: &Path) {
    const MAX_AGE_MS: f64 = 14.0 * 24.0 * 3600.0 * 1000.0;
    let now = crate::clock::now_ms();
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for entry in rd.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        let mtime = meta.mtime() as f64 * 1000.0;
        if meta.is_file() && now - mtime > MAX_AGE_MS {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

fn load_cache(path: &Path, key: &str) -> Option<Cache> {
    let bytes = std::fs::read(path).ok()?;
    codec::decode(&bytes).filter(|c| c.path == key)
}

/// Analyze `transcript` (as given on stdin). `cache_dir` None disables the
/// cache; `budget` None means no time limit.
pub fn analyze(transcript: &str, with_subagents: bool, cache_dir: Option<&Path>, budget: Option<Duration>) -> Analysis {
    let deadline = budget.map(|b| Instant::now() + b);
    let empty = Analysis { empty: true, main_complete: true, speed_complete: true, agg: Aggregates::default() };
    let Ok(file) = File::open(transcript) else { return empty };
    let Ok(meta) = file.metadata() else { return empty };
    if !meta.is_file() {
        return empty;
    }
    let key = crate::settings::path_resolve(transcript).to_string_lossy().into_owned();
    let cache_path = cache_dir.map(|d| d.join(format!("{:016x}.bin", fnv64(key.as_bytes()))));
    let fresh = |no_freeze: bool| {
        let mut c = Cache { path: key.clone(), ..Default::default() };
        c.main = FileCursor { dev: meta.dev(), ino: meta.ino(), offset: 0, fingerprint: fnv64(&[]) };
        c.agg.speed.no_freeze = no_freeze;
        c.agg.speed_on = with_subagents;
        c
    };
    let loaded = cache_path.as_deref().and_then(|p| load_cache(p, &key));
    let mut cache = match loaded {
        Some(c) if c.agg.speed.needs_rebuild => fresh(true),
        Some(c) if !cursor_is_valid(&c.main, &file, &meta) => fresh(c.agg.speed.no_freeze),
        // speed was not tracked so far: it needs every line again
        Some(c) if with_subagents && !c.agg.speed_on => fresh(c.agg.speed.no_freeze),
        Some(mut c) => {
            if !with_subagents && c.agg.speed_on {
                c.agg.speed_on = false;
                c.agg.speed = Default::default();
                c.agg.speed_cursor = Default::default();
                c.subagents.clear();
            }
            c
        }
        None => {
            if let Some(d) = cache_dir {
                prune_cache_dir(d);
            }
            fresh(false)
        }
    };
    let original = cache.clone();
    let ing = Ingest { deadline };
    let mut ids = Vec::new();

    // main transcript
    let agg = &mut cache.agg;
    let scan =
        scan_lines(&file, cache.main.offset, meta.len(), deadline, |raw, at0| apply_main_line(agg, raw, at0, &mut ids));
    let Ok(scan) = scan else { return empty };
    cache.main.offset = scan.offset;
    cache.main.fingerprint = fingerprint(&file, scan.offset).unwrap_or(0);
    let main_complete = !scan.timed_out;
    let main_partial = scan.partial;

    // subagent transcripts (only speed reads them)
    let mut speed_complete = main_complete;
    let mut sub_partials: Vec<(String, Vec<u8>)> = Vec::new();
    if with_subagents && main_complete {
        let found = subagent_paths(transcript, &cache.agg.agent_ids);
        let vanished = cache.subagents.iter().any(|s| !found.contains(&s.path));
        if vanished {
            cache = fresh(cache.agg.speed.no_freeze);
            speed_complete = false;
        } else {
            for path in &found {
                match ing.subagent(&mut cache, path, &mut ids) {
                    Sub::Done { partial } => {
                        if let Some(p) = partial {
                            sub_partials.push((path.clone(), p));
                        }
                    }
                    Sub::Skip => {}
                    Sub::TimedOut => {
                        speed_complete = false;
                        break;
                    }
                    Sub::Invalid => {
                        cache = fresh(cache.agg.speed.no_freeze);
                        speed_complete = false;
                        break;
                    }
                }
            }
        }
    }
    let main_complete = main_complete && cache.main.offset == scan.offset;
    if main_complete && (!with_subagents || speed_complete) {
        cache.agg.speed.freeze();
    }

    if cache != original {
        if let Some(p) = &cache_path {
            let _ = crate::cachefile::atomic_write(p, &codec::encode(&cache));
        }
    }

    // this render's view: the unterminated last lines count too, as they do
    // upstream, but are not saved (they may still be growing)
    let mut view = cache;
    if let Some(p) = &main_partial {
        let at0 = view.main.offset == 0;
        apply_main_line(&mut view.agg, p, at0, &mut ids);
    }
    if with_subagents && speed_complete {
        for (path, p) in &sub_partials {
            if let Some(sa) = view.subagents.iter().find(|s| &s.path == path) {
                let mut cur = sa.speed.clone();
                let at0 = sa.file.offset == 0;
                apply_subagent_line(&mut view.agg, &mut cur, p, at0, &mut ids);
            }
        }
        if main_partial.is_some() {
            // ids only the unterminated line mentions
            for path in subagent_paths(transcript, &view.agg.agent_ids) {
                if view.subagents.iter().any(|s| s.path == path) {
                    continue;
                }
                match ing.subagent(&mut view, &path, &mut ids) {
                    Sub::Done { partial: Some(p) } => {
                        let mut cur = view.subagents.last().map(|s| s.speed.clone()).unwrap_or_default();
                        let at0 = view.subagents.last().is_some_and(|s| s.file.offset == 0);
                        apply_subagent_line(&mut view.agg, &mut cur, &p, at0, &mut ids);
                    }
                    Sub::Done { partial: None } | Sub::Skip => {}
                    Sub::TimedOut | Sub::Invalid => speed_complete = false,
                }
            }
        }
    }
    if view.agg.speed.needs_rebuild {
        speed_complete = false;
    }
    Analysis { empty: false, main_complete, speed_complete: speed_complete && main_complete, agg: view.agg }
}

fn budget() -> Option<Duration> {
    let ms = std::env::var("CCSTATUSLINE_RS_TRANSCRIPT_BUDGET_MS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_BUDGET_MS);
    (ms > 0).then(|| Duration::from_millis(ms))
}

pub fn cache_dir() -> PathBuf {
    crate::settings::home_dir().join(".cache").join("ccstatusline-rs").join("transcripts")
}

const SPEED_TYPES: [&str; 3] = ["output-speed", "input-speed", "total-speed"];

/// The transcript analysis for this render (None without a transcript path).
fn analysis(ctx: &RenderContext) -> Option<&'static Analysis> {
    static ANALYSIS: OnceLock<Option<Analysis>> = OnceLock::new();
    ANALYSIS
        .get_or_init(|| {
            let path = ctx.data.transcript_path.as_deref().filter(|p| !p.is_empty())?;
            let with_subagents = SPEED_TYPES.iter().any(|t| ctx.has_widget(t));
            Some(analyze(path, with_subagents, Some(&cache_dir()), budget()))
        })
        .as_ref()
}

/// `context.tokenMetrics`: None when there is no transcript (or it is still
/// being indexed).
pub fn token_metrics(ctx: &RenderContext) -> Option<TokenMetrics> {
    let a = analysis(ctx)?;
    if a.empty {
        return Some(TokenMetrics::default());
    }
    a.main_complete.then(|| a.agg.tokens.finish())
}

/// `context.sessionDuration`
pub fn session_duration(ctx: &RenderContext) -> Option<String> {
    let a = analysis(ctx)?;
    if a.empty || !a.main_complete {
        return None;
    }
    state::format_session_duration(a.agg.first_ts, a.agg.last_ts)
}

/// `context.compactionData` (None: the widget uses zeros)
pub fn compaction(ctx: &RenderContext) -> Option<Compaction> {
    let a = analysis(ctx)?;
    if a.empty {
        return Some(Compaction::default());
    }
    a.main_complete.then(|| a.agg.compaction.clone())
}

/// `context.transcriptThinkingEffort` (when the status JSON has no level)
pub fn thinking_effort(ctx: &RenderContext) -> Option<Effort> {
    let a = analysis(ctx)?;
    if a.empty || !a.main_complete {
        return None;
    }
    a.agg.thinking.clone()
}

/// `context.transcriptSessionName`
pub fn session_name(ctx: &RenderContext) -> Option<String> {
    let a = analysis(ctx)?;
    if a.empty || !a.main_complete {
        return None;
    }
    a.agg.session_name.clone()
}

/// `context.speedMetrics` (session average)
pub fn speed_session(ctx: &RenderContext) -> Option<SpeedMetrics> {
    let a = analysis(ctx)?;
    if a.empty {
        return Some(SpeedMetrics::default());
    }
    a.speed_complete.then(|| a.agg.speed.session_average())
}

/// `context.windowedSpeedMetrics[window]` (window in 1..=120 seconds)
pub fn speed_windowed(ctx: &RenderContext, window_seconds: f64) -> Option<SpeedMetrics> {
    let a = analysis(ctx)?;
    if a.empty {
        return Some(SpeedMetrics::default());
    }
    a.speed_complete.then(|| a.agg.speed.windowed(window_seconds))
}

#[cfg(test)]
mod tests;
