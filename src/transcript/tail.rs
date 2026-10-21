//! The Cache Timer's view of a transcript (port of the tail scan in upstream
//! `src/widgets/CacheTimer.ts`).
//!
//! Upstream reads the last 32 KiB, scans its lines newest-first, and doubles
//! the window until a record decides the state, re-reading and re-scanning
//! everything each time (a tail of large non-chat records costs it several
//! MB). Here the file is read backwards once, line by line: every window
//! scan upstream repeats gives the same answers for the lines it shares with
//! the previous one, so only the record cut by each window's first byte (the
//! fragment upstream sees there) needs evaluating separately.

use super::json::{self, JVal, Line};
use crate::jsnum::{js_trim, number_from_str, to_js_string};
use std::os::unix::fs::FileExt;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TimerState {
    /// a request is in flight
    Working,
    /// the last assistant row that touched the cache (epoch ms), if any
    Idle(Option<f64>),
}

const INITIAL_TAIL_BYTES: u64 = 32768;
const INTERRUPT_MARKER_PREFIX: &str = "[Request interrupted by user";
const LOCAL_COMMAND_PREFIXES: [&str; 4] =
    ["<command-name>", "<local-command-stdout>", "<local-command-stderr>", "<local-command-caveat>"];

const READ_CHUNK: u64 = 64 * 1024;

/// `getTranscriptState`
pub fn timer_state(path: &str) -> TimerState {
    timer_state_inner(path).unwrap_or(TimerState::Idle(None))
}

fn timer_state_inner(path: &str) -> Option<TimerState> {
    let f = std::fs::File::open(path).ok()?;
    let size = f.metadata().ok()?.len();
    if size == 0 {
        return None;
    }
    let mut scan = TailScan::default();
    // bytes of the record being assembled, oldest chunk last
    let mut pending: Vec<Vec<u8>> = Vec::new();
    let assemble = |parts: &[Vec<u8>]| -> Vec<u8> { parts.iter().rev().flatten().copied().collect() };
    let mut pos = size;
    let mut window = INITIAL_TAIL_BYTES;
    loop {
        let start = size - window.min(size);
        while pos > start {
            let n = (pos - start).min(READ_CHUNK);
            let mut buf = vec![0u8; n as usize];
            f.read_exact_at(&mut buf, pos - n).ok()?;
            pos -= n;
            let mut end = buf.len();
            while let Some(i) = buf[..end].iter().rposition(|&b| b == b'\n') {
                pending.push(buf[i + 1..end].to_vec());
                let record = assemble(&pending);
                pending.clear();
                if let Some(s) = scan.line(&String::from_utf8_lossy(&record)) {
                    return Some(s);
                }
                end = i;
            }
            buf.truncate(end);
            pending.push(buf);
        }
        // the window's first segment, as upstream sees it in this window
        let fragment = assemble(&pending);
        if let Some(s) = scan.clone().line(&String::from_utf8_lossy(&fragment)) {
            return Some(s);
        }
        if start == 0 {
            return None;
        }
        window *= 2;
    }
}

/// `String(v)` as `+` and `Date` coerce objects.
fn js_string(v: &JVal) -> String {
    match v {
        JVal::Null => "null".into(),
        JVal::Bool(b) => b.to_string(),
        JVal::Num(n) => to_js_string(*n),
        JVal::Str(s) => s.clone(),
        JVal::Arr(a) => a
            .iter()
            .map(|e| if matches!(e, JVal::Null) { String::new() } else { js_string(e) })
            .collect::<Vec<_>>()
            .join(","),
        JVal::Obj(_) => "[object Object]".into(),
    }
}

/// `(a ?? 0) + (b ?? 0) > 0` with JS coercions.
fn js_sum_positive(a: Option<&JVal>, b: Option<&JVal>) -> bool {
    let zero = JVal::Num(0.0);
    let norm = |v: Option<&JVal>| match v {
        None | Some(JVal::Null) => zero.clone(),
        Some(v) => v.clone(),
    };
    let (a, b) = (norm(a), norm(b));
    let to_num = |v: &JVal| match v {
        JVal::Num(n) => *n,
        JVal::Bool(b) => *b as u8 as f64,
        _ => f64::NAN,
    };
    let stringy = |v: &JVal| matches!(v, JVal::Str(_) | JVal::Arr(_) | JVal::Obj(_));
    if stringy(&a) || stringy(&b) {
        let s = js_string(&a) + &js_string(&b);
        number_from_str(&s).is_some_and(|n| n > 0.0)
    } else {
        to_num(&a) + to_num(&b) > 0.0
    }
}

/// `hasCacheActivity`
fn has_cache_activity(l: &Line) -> bool {
    let usage = l.message.as_ref().and_then(|m| m.usage.as_ref());
    match usage {
        None => true,
        Some(u) if !u.truthy() => true,
        Some(u) => js_sum_positive(u.get("cache_read_input_tokens"), u.get("cache_creation_input_tokens")),
    }
}

/// `endsTurnWithoutResponse`
fn ends_turn(l: &Line) -> bool {
    let Some(m) = &l.message else { return false };
    let text = if let Some(s) = &m.content_str {
        Some(s.as_str())
    } else if m.content_len == Some(1) {
        let first = m.content_first.as_ref();
        if first.and_then(|f| f.get("type")).and_then(JVal::as_str) == Some("text") {
            first.and_then(|f| f.get("text")).and_then(JVal::as_str)
        } else {
            None
        }
    } else {
        None
    };
    let Some(text) = text else { return false };
    let t = text.trim_start_matches(crate::gradient::is_js_space);
    t.starts_with(INTERRUPT_MARKER_PREFIX) || LOCAL_COMMAND_PREFIXES.iter().any(|p| t.starts_with(p))
}

/// `new Date(entry.timestamp).getTime()` for a truthy timestamp.
fn date_ms(v: &JVal) -> Option<f64> {
    match v {
        JVal::Num(n) => {
            if !n.is_finite() || n.abs() > 8.64e15 {
                None
            } else {
                Some(n.trunc() + 0.0)
            }
        }
        JVal::Bool(true) => Some(1.0),
        JVal::Str(s) => crate::timefmt::parse_iso(s),
        JVal::Arr(_) => crate::timefmt::parse_iso(&js_string(v)),
        _ => None,
    }
}

/// `scanTailForState`, one line at a time (newest first).
#[derive(Clone, Default)]
struct TailScan {
    turn_finished: bool,
}

impl TailScan {
    fn line(&mut self, line: &str) -> Option<TimerState> {
        let t = js_trim(line);
        if t.is_empty() {
            return None;
        }
        let opts = json::Options { agent_ids: false, content_first: true };
        let e = json::parse_line(t.as_bytes(), &opts, &mut Vec::new())?;
        if !e.is_object || e.sidechain() {
            return None;
        }
        if e.type_is("assistant") {
            self.turn_finished = true;
            let api_error = e.is_api_error.as_ref().is_some_and(JVal::is_true);
            if !api_error && has_cache_activity(&e) {
                if let Some(ts) = e.timestamp.as_ref().filter(|t| t.truthy()) {
                    if let Some(ms) = date_ms(ts) {
                        return Some(TimerState::Idle(Some(ms)));
                    }
                }
            }
            return None;
        }
        if e.type_is("user") {
            if ends_turn(&e) {
                self.turn_finished = true;
                return None;
            }
            if !self.turn_finished {
                return Some(TimerState::Working);
            }
        }
        None
    }
}

#[cfg(test)]
fn scan_tail(tail: &str) -> Option<TimerState> {
    let mut scan = TailScan::default();
    tail.split('\n').rev().find_map(|l| scan.line(l))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_states() {
        let a = r#"{"type":"assistant","timestamp":"2026-10-21T11:58:00.000Z","message":{"usage":{"cache_read_input_tokens":5}}}"#;
        let u = r#"{"type":"user","timestamp":"2026-10-21T11:59:00.000Z","message":{"content":"hi"}}"#;
        let i = r#"{"type":"user","message":{"content":[{"type":"text","text":"  [Request interrupted by user]"}]}}"#;
        let nc = r#"{"type":"assistant","timestamp":"2026-10-21T11:59:30.000Z","message":{"usage":{"cache_read_input_tokens":0}}}"#;
        assert_eq!(scan_tail(&format!("{a}\n{u}\n")), Some(TimerState::Working));
        assert_eq!(scan_tail(&format!("{a}\n{u}\n{i}\n")), Some(TimerState::Idle(Some(1792583880000.0))));
        assert_eq!(scan_tail(&format!("{a}\n{u}\n{nc}")), Some(TimerState::Idle(Some(1792583880000.0))));
        assert_eq!(scan_tail("partial\n{\"type\":\"system\"}\n"), None);
    }

    /// Upstream's algorithm verbatim: re-read a doubling tail window.
    fn reference(path: &str) -> TimerState {
        let mut bytes = INITIAL_TAIL_BYTES;
        loop {
            let f = std::fs::File::open(path).unwrap();
            let size = f.metadata().unwrap().len();
            let n = bytes.min(size);
            let mut buf = vec![0u8; n as usize];
            f.read_exact_at(&mut buf, size - n).unwrap();
            let text = String::from_utf8_lossy(&buf).into_owned();
            if text.is_empty() {
                return TimerState::Idle(None);
            }
            if let Some(s) = scan_tail(&text) {
                return s;
            }
            if n == size {
                return TimerState::Idle(None);
            }
            bytes *= 2;
        }
    }

    #[test]
    fn backward_reader_matches_doubling_windows() {
        let dir = std::env::temp_dir().join(format!("ccsl-rs-tail-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.jsonl");
        let p = path.to_string_lossy().into_owned();
        let a = |ts: &str, read: u32| {
            format!(
                r#"{{"type":"assistant","timestamp":"{ts}","message":{{"usage":{{"cache_read_input_tokens":{read}}}}}}}"#
            )
        };
        let u = |text: &str| format!(r#"{{"type":"user","message":{{"content":"{text}"}}}}"#);
        let sys = |pad: usize| format!(r#"{{"type":"system","pad":"{}"}}"#, "p".repeat(pad));
        let mut seed = 12345u64;
        let mut rnd = |m: u64| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) % m
        };
        for case in 0..60 {
            let mut lines = Vec::new();
            for i in 0..(rnd(40) + 1) {
                let pad = [10, 3000, 20000, 40000, 70000, 140000][rnd(6) as usize] as usize;
                lines.push(match rnd(6) {
                    0 => a(&format!("2026-10-21T11:{:02}:00.000Z", i % 60), rnd(2) as u32),
                    1 => u("hi"),
                    2 => u("[Request interrupted by user]"),
                    3 => {
                        format!(r#"{{"type":"user","isSidechain":true,"message":{{"content":"{}"}}}}"#, "s".repeat(pad))
                    }
                    _ => sys(pad),
                });
            }
            let mut body = lines.join("\n");
            if rnd(2) == 0 {
                body.push('\n');
            }
            std::fs::write(&path, &body).unwrap();
            assert_eq!(timer_state(&p), reference(&p), "case {case}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn js_plus() {
        let n = |x: f64| JVal::Num(x);
        assert!(js_sum_positive(Some(&n(1.0)), None));
        assert!(!js_sum_positive(Some(&n(0.0)), Some(&JVal::Null)));
        assert!(js_sum_positive(Some(&JVal::Str("5".into())), None)); // "50"
        assert!(!js_sum_positive(Some(&JVal::Str("x".into())), None));
        assert!(!js_sum_positive(Some(&JVal::Arr(vec![])), None)); // "0"
        assert!(js_sum_positive(Some(&JVal::Bool(true)), None));
    }
}
