//! The 5-hour usage block from transcript activity (port of upstream
//! `src/utils/jsonl-blocks.ts` `getBlockMetrics` and `jsonl-cache.ts`
//! `getCachedBlockMetrics`), the fallback block timers use when the usage
//! API gives no reset time.
//!
//! Upstream globs `<config dir>/projects/**/*.jsonl` and reads every file
//! changed in the last 10–48 h in full whenever its block cache is empty or
//! expired (every minute while there is no block). Here each file's usage
//! timestamps are cached under `~/.cache/ccstatusline-rs/blocks/files/`,
//! keyed by device + inode (so config dirs whose `projects` link to the same
//! tree share them) and extended with the bytes appended since. The result
//! cache (upstream's `block-cache-<config dir hash>.json`: a start time valid
//! for 5 h, or "no block" for a minute) is keyed by the real path of the
//! projects directory, so it is shared the same way.

use super::{fingerprint, record, scan_lines, FileCursor};
use std::fs::File;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const SESSION_MS: f64 = 5.0 * 3600.0 * 1000.0;
const EMPTY_RESULT_TTL_MS: f64 = 60.0 * 1000.0;
const LOOKBACK_HOURS: [f64; 3] = [10.0, 20.0, 48.0];
const SCAN_BUDGET: Duration = Duration::from_millis(300);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockMetrics {
    /// epoch ms
    pub start_ms: f64,
    /// epoch ms
    pub last_activity_ms: f64,
}

/// `getCachedBlockMetrics()` for this process (memoized).
pub fn block_metrics() -> Option<BlockMetrics> {
    use std::sync::OnceLock;
    static M: OnceLock<Option<BlockMetrics>> = OnceLock::new();
    *M.get_or_init(|| {
        let projects = crate::claude_config::config_dir().join("projects");
        cached_block_metrics(&projects, &blocks_dir(), crate::clock::now_ms(), Some(SCAN_BUDGET))
    })
}

fn blocks_dir() -> PathBuf {
    crate::settings::home_dir().join(".cache").join("ccstatusline-rs").join("blocks")
}

enum Saved {
    Start(f64),
    EmptyUntil(f64),
}

fn state_path(dir: &Path, projects: &Path) -> PathBuf {
    let key = std::fs::canonicalize(projects).unwrap_or_else(|_| projects.to_path_buf());
    dir.join(format!("state-{:016x}", super::fnv64(key.as_os_str().as_encoded_bytes())))
}

fn read_state(path: &Path) -> Option<Saved> {
    let text = std::fs::read_to_string(path).ok()?;
    let (kind, ms) = text.trim().split_once(' ')?;
    let ms: f64 = ms.parse().ok().filter(|v: &f64| v.is_finite())?;
    match kind {
        "start" => Some(Saved::Start(ms)),
        "empty" => Some(Saved::EmptyUntil(ms)),
        _ => None,
    }
}

fn write_state(path: &Path, s: Saved) {
    let text = match s {
        Saved::Start(ms) => format!("start {ms}\n"),
        Saved::EmptyUntil(ms) => format!("empty {ms}\n"),
    };
    let _ = crate::cachefile::atomic_write(path, text.as_bytes());
}

/// `getCachedBlockMetrics`
pub fn cached_block_metrics(
    projects: &Path,
    cache_dir: &Path,
    now: f64,
    budget: Option<Duration>,
) -> Option<BlockMetrics> {
    let state = state_path(cache_dir, projects);
    match read_state(&state) {
        Some(Saved::Start(start)) if now <= start + SESSION_MS => {
            return Some(BlockMetrics { start_ms: start, last_activity_ms: now });
        }
        Some(Saved::EmptyUntil(until)) if now < until => return None,
        _ => {}
    }
    let deadline = budget.map(|b| Instant::now() + b);
    let files = cache_dir.join("files");
    let scan = find_block(projects, &files, now, deadline);
    if !matches!(scan, Scan::Incomplete) {
        super::prune_cache_dir(&files);
    }
    match scan {
        // the timestamps are not all indexed yet: no answer, and nothing
        // saved, until a later render finishes
        Scan::Incomplete => None,
        Scan::Done(Some(m)) => {
            write_state(&state, Saved::Start(m.start_ms));
            Some(m)
        }
        Scan::Done(None) => {
            write_state(&state, Saved::EmptyUntil(now + EMPTY_RESULT_TTL_MS));
            None
        }
    }
}

enum Scan {
    Done(Option<BlockMetrics>),
    Incomplete,
}

/// `globSync('<projects>/**/*.jsonl')`: follows symlinks, skips dot entries.
fn glob_jsonl(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, real_stack: &mut Vec<PathBuf>, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for entry in rd.flatten() {
            let name = entry.file_name();
            let Some(n) = name.to_str() else { continue };
            if n.starts_with('.') {
                continue;
            }
            let Ok(ft) = entry.file_type() else { continue };
            let path = dir.join(n);
            let is_dir = if ft.is_symlink() {
                match std::fs::metadata(&path) {
                    Ok(m) if m.is_dir() => true,
                    Ok(m) if m.is_file() => false,
                    _ => continue,
                }
            } else if ft.is_dir() {
                true
            } else if ft.is_file() {
                false
            } else {
                continue;
            };
            if is_dir {
                // real paths: only links need resolving
                let real = if ft.is_symlink() {
                    let Ok(r) = std::fs::canonicalize(&path) else { continue };
                    r
                } else {
                    real_stack.last().map(|r| r.join(n)).unwrap_or_else(|| path.clone())
                };
                // a link back into the current walk would loop
                if real_stack.iter().any(|r| r.starts_with(&real)) {
                    continue;
                }
                real_stack.push(real);
                walk(&path, real_stack, out);
                real_stack.pop();
            } else if n.ends_with(".jsonl") {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    let mut stack = vec![std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf())];
    walk(root, &mut stack, &mut out);
    out
}

/// Usage timestamps of one transcript, as `getAllTimestampsFromFile` finds
/// them, from the per-file cache plus the bytes appended since.
fn file_timestamps(path: &Path, cache_dir: &Path, deadline: Option<Instant>) -> Result<Vec<f64>, ()> {
    let Ok(file) = File::open(path) else { return Ok(vec![]) };
    let Ok(meta) = file.metadata() else { return Ok(vec![]) };
    let cache_path = cache_dir.join(format!("{:x}-{:x}", meta.dev(), meta.ino()));
    let (mut cursor, mut stamps) = match load(&cache_path) {
        Some((c, s))
            if c.dev == meta.dev()
                && c.ino == meta.ino()
                && c.offset <= meta.len()
                && fingerprint(&file, c.offset).is_ok_and(|f| f == c.fingerprint) =>
        {
            (c, s)
        }
        _ => (FileCursor { dev: meta.dev(), ino: meta.ino(), offset: 0, fingerprint: 0 }, vec![]),
    };
    let before = cursor.offset;
    let opts = super::json::Options { agent_ids: false, content_first: false };
    let mut ids = Vec::new();
    let mut add = |raw: &[u8], at0: bool, out: &mut Vec<f64>| {
        let Some(rec) = record(raw, at0) else { return };
        if let Some(t) = usage_timestamp(rec, &opts, &mut ids) {
            out.push(t);
        }
    };
    let mut fresh = Vec::new();
    let Ok(scan) = scan_lines(&file, cursor.offset, meta.len(), deadline, |raw, at0| add(raw, at0, &mut fresh)) else {
        // a read error: upstream reports no timestamps for the file
        return Ok(vec![]);
    };
    stamps.extend(fresh);
    cursor.offset = scan.offset;
    cursor.fingerprint = fingerprint(&file, scan.offset).unwrap_or(0);
    if cursor.offset != before || !cache_path.exists() {
        stamps.sort_by(f64::total_cmp);
        save(&cache_path, &cursor, &stamps);
    }
    if scan.timed_out {
        return Err(());
    }
    if let Some(p) = scan.partial {
        add(&p, cursor.offset == 0, &mut stamps);
    }
    Ok(stamps)
}

/// One record's block activity timestamp.
fn usage_timestamp(rec: &[u8], opts: &super::json::Options, ids: &mut Vec<String>) -> Option<f64> {
    use super::json::JVal;
    let l = super::json::parse_line(rec, opts, ids)?;
    if !l.is_object {
        return None;
    }
    let usage = l.message.as_ref()?.usage.as_ref().filter(|u| u.truthy())?;
    if !matches!(usage.get("input_tokens"), Some(JVal::Num(_)))
        || !matches!(usage.get("output_tokens"), Some(JVal::Num(_)))
    {
        return None;
    }
    if l.sidechain() {
        return None;
    }
    match l.timestamp.as_ref()? {
        JVal::Str(s) => crate::timefmt::parse_iso(s),
        _ => None,
    }
}

fn load(path: &Path) -> Option<(FileCursor, Vec<f64>)> {
    let b = std::fs::read(path).ok()?;
    let u = |i: usize| -> Option<u64> { Some(u64::from_le_bytes(b.get(i..i + 8)?.try_into().ok()?)) };
    if b.get(..8)? != b"CCSLBK01" {
        return None;
    }
    let c = FileCursor { dev: u(8)?, ino: u(16)?, offset: u(24)?, fingerprint: u(32)? };
    let n = u(40)? as usize;
    let mut out = Vec::with_capacity(n.min(b.len() / 8));
    let mut i = 48;
    let mut prev = 0i64;
    for _ in 0..n {
        // zigzag varint deltas of integer ms
        let mut v: u64 = 0;
        let mut shift = 0;
        loop {
            let byte = *b.get(i)?;
            i += 1;
            v |= ((byte & 0x7f) as u64) << shift;
            if byte & 0x80 == 0 {
                break;
            }
            shift += 7;
            if shift > 63 {
                return None;
            }
        }
        let d = ((v >> 1) as i64) ^ -((v & 1) as i64);
        prev = prev.wrapping_add(d);
        out.push(prev as f64);
    }
    (i == b.len()).then_some((c, out))
}

fn save(path: &Path, c: &FileCursor, stamps: &[f64]) {
    let mut b = Vec::with_capacity(48 + stamps.len() * 3);
    b.extend_from_slice(b"CCSLBK01");
    for v in [c.dev, c.ino, c.offset, c.fingerprint, stamps.len() as u64] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    let mut prev = 0i64;
    for &t in stamps {
        // Date.parse results are integer ms
        let t = t as i64;
        let d = t.wrapping_sub(prev);
        prev = t;
        let mut v = ((d << 1) ^ (d >> 63)) as u64;
        loop {
            let byte = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                b.push(byte);
                break;
            }
            b.push(byte | 0x80);
        }
    }
    let _ = crate::cachefile::atomic_write(path, &b);
}

fn floor_to_hour(t: f64) -> f64 {
    t - t.rem_euclid(3_600_000.0)
}

/// `findMostRecentBlockStartTime`
fn find_block(projects: &Path, cache_dir: &Path, now: f64, deadline: Option<Instant>) -> Scan {
    let files = glob_jsonl(projects);
    if files.is_empty() {
        return Scan::Done(None);
    }
    let mut with_mtime = Vec::with_capacity(files.len());
    for f in files {
        // statSync throwing ends the whole scan with no block
        let Ok(m) = std::fs::metadata(&f) else { return Scan::Done(None) };
        let mtime = (m.mtime() as f64) * 1000.0 + (m.mtime_nsec() / 1_000_000) as f64;
        with_mtime.push((f, mtime));
    }
    with_mtime.sort_by(|a, b| b.1.total_cmp(&a.1));

    let mut loaded: Vec<Option<Vec<f64>>> = vec![None; with_mtime.len()];
    let mut timestamps: Vec<f64> = Vec::new();
    let mut most_recent: Option<f64> = None;
    let mut work_start: Option<f64> = None;
    for (ci, hours) in LOOKBACK_HOURS.iter().enumerate() {
        let cutoff = now - hours * 3_600_000.0;
        timestamps.clear();
        for (i, (f, mtime)) in with_mtime.iter().enumerate() {
            if *mtime < cutoff {
                break;
            }
            if loaded[i].is_none() {
                match file_timestamps(f, cache_dir, deadline) {
                    Ok(v) => loaded[i] = Some(v),
                    Err(()) => return Scan::Incomplete,
                }
            }
            timestamps.extend_from_slice(loaded[i].as_ref().unwrap());
        }
        if timestamps.is_empty() {
            continue;
        }
        // most recent first
        timestamps.sort_by(|a, b| b.total_cmp(a));
        if most_recent.is_none() {
            most_recent = Some(timestamps[0]);
            if now - timestamps[0] > SESSION_MS {
                return Scan::Done(None);
            }
        }
        work_start = most_recent;
        let mut found_gap = false;
        for w in timestamps.windows(2) {
            if w[0] - w[1] >= SESSION_MS {
                found_gap = true;
                break;
            }
            work_start = Some(w[1]);
        }
        if found_gap || ci == LOOKBACK_HOURS.len() - 1 {
            break;
        }
    }
    let (Some(most_recent), Some(work_start)) = (most_recent, work_start) else { return Scan::Done(None) };

    // build blocks forward from the start of the continuous work
    let mut blocks: Vec<(f64, f64)> = Vec::new();
    for &t in timestamps.iter().rev() {
        if t < work_start {
            continue;
        }
        if blocks.last().is_none_or(|b| t > b.1) {
            let start = floor_to_hour(t);
            blocks.push((start, start + SESSION_MS));
        }
    }
    for (start, end) in blocks {
        if now >= start && now <= end && timestamps.iter().any(|t| *t >= start && *t <= end) {
            return Scan::Done(Some(BlockMetrics { start_ms: start, last_activity_ms: most_recent }));
        }
    }
    Scan::Done(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(ts: &str) -> String {
        format!(
            r#"{{"type":"assistant","timestamp":"{ts}","message":{{"usage":{{"input_tokens":1,"output_tokens":2}}}}}}"#
        )
    }

    #[test]
    fn block_from_transcripts_and_cache() {
        let root = std::env::temp_dir().join(format!("ccsl-rs-blocks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let projects = root.join("projects");
        std::fs::create_dir_all(projects.join("p").join("s").join("subagents")).unwrap();
        std::fs::create_dir_all(projects.join(".hidden")).unwrap();
        let now = crate::timefmt::parse_iso("2026-10-21T12:00:00.000Z").unwrap();
        let a = projects.join("p").join("a.jsonl");
        std::fs::write(
            &a,
            [line("2026-10-21T02:10:00.000Z"), line("2026-10-21T09:40:00.000Z"), line("2026-10-21T11:30:00.000Z")]
                .join("\n"),
        )
        .unwrap();
        std::fs::write(
            projects.join("p").join("s").join("subagents").join("agent-1.jsonl"),
            format!("{}\n", line("2026-10-21T07:20:00.000Z")),
        )
        .unwrap();
        // hidden and sidechain records are not activity
        std::fs::write(projects.join(".hidden").join("x.jsonl"), line("2026-10-21T03:00:00.000Z")).unwrap();
        std::fs::write(
            projects.join("p").join("side.jsonl"),
            r#"{"isSidechain":true,"timestamp":"2026-10-21T04:00:00.000Z","message":{"usage":{"input_tokens":1,"output_tokens":1}}}"#,
        )
        .unwrap();
        let cache = root.join("cache");
        // 07:20 → block 07:00–12:00 holds now; 02:10 is cut off by the 5 h gap
        let m = cached_block_metrics(&projects, &cache, now, None).unwrap();
        assert_eq!(m.start_ms, crate::timefmt::parse_iso("2026-10-21T07:00:00.000Z").unwrap());
        assert_eq!(m.last_activity_ms, crate::timefmt::parse_iso("2026-10-21T11:30:00.000Z").unwrap());
        // per-file caches give the same scan
        match find_block(&projects, &cache.join("files"), now, None) {
            Scan::Done(Some(m2)) => assert_eq!(m2, m),
            _ => panic!(),
        }
        // the saved start holds for 5 h, with "now" as last activity
        assert_eq!(cached_block_metrics(&projects, &cache, now, None).unwrap().last_activity_ms, now);
        // past the block's end: rescan, and no block holds 13:00
        assert_eq!(cached_block_metrics(&projects, &cache, now + 3_600_000.0, None), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// `CCSL_BLOCKS_EXPECTED=<dir>/expected.json cargo test -- --ignored upstream_blocks`
    /// with expectations written by upstream's getBlockMetrics (now = 1792584000000).
    #[test]
    #[ignore]
    fn upstream_blocks() {
        let path = std::env::var("CCSL_BLOCKS_EXPECTED").unwrap();
        let cases: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let now = 1792584000000.0;
        for (i, c) in cases.as_array().unwrap().iter().enumerate() {
            let projects = Path::new(c["projects"].as_str().unwrap());
            let cache = projects.parent().unwrap().join("rs-cache");
            let _ = std::fs::remove_dir_all(&cache);
            for round in 0..2 {
                let got = match find_block(projects, &cache, now, None) {
                    Scan::Done(m) => m,
                    Scan::Incomplete => panic!(),
                };
                let want = c["start"]
                    .as_f64()
                    .map(|s| BlockMetrics { start_ms: s, last_activity_ms: c["last"].as_f64().unwrap() });
                assert_eq!(got, want, "case {i} round {round}");
            }
        }
    }

    /// Timing on a real projects tree: `CCSL_BLOCKS_PROJECTS=… CCSL_BLOCKS_CACHE=… cargo test --release -- --ignored real_blocks --nocapture`
    #[test]
    #[ignore]
    fn real_blocks() {
        let projects = PathBuf::from(std::env::var("CCSL_BLOCKS_PROJECTS").unwrap());
        let cache = PathBuf::from(std::env::var("CCSL_BLOCKS_CACHE").unwrap());
        let now = crate::clock::now_ms();
        for round in 0..3 {
            let t = Instant::now();
            let m = match find_block(&projects, &cache, now, None) {
                Scan::Done(m) => m,
                Scan::Incomplete => None,
            };
            println!("round {round}: {:?} in {:?}", m, t.elapsed());
        }
    }

    #[test]
    fn varint_round_trip() {
        let dir = std::env::temp_dir().join(format!("ccsl-rs-blk-codec-{}", std::process::id()));
        let p = dir.join("f");
        let c = FileCursor { dev: 1, ino: 2, offset: 3, fingerprint: 4 };
        let v = vec![-5.0, 0.0, 1792584000000.0, 1792584000001.0, 1792584999999.0];
        save(&p, &c, &v);
        assert_eq!(load(&p), Some((c, v)));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
