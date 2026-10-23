//! Running aggregates for one transcript, fed one line at a time (port of
//! upstream `src/utils/jsonl-metrics.ts` `scanTranscript`, `compaction.ts`,
//! `jsonl-metadata.ts`, `jsonl-session.ts`).
//!
//! Upstream re-reads the whole file on every render. Every aggregate it
//! derives is a left fold over the lines, so the state after N lines plus
//! the lines appended since gives the same result; `finish_*` produce the
//! upstream values without mutating the state.
//!
//! Speed is the one aggregate that is not a plain sum: the session average
//! divides by the length of the *union* of all request intervals (main file
//! and subagent files), and the windowed speeds need the requests of the
//! last ≤120 s. The union is kept as a sorted list of disjoint intervals.
//! Once a pass has read every file (`freeze`, never mid-pass, so a cold
//! rebuild can merge subagent intervals from any point in the session), the
//! oldest ones beyond `ACTIVE_HIGH` are folded into `frozen_ms`. A later
//! interval reaching back into the frozen part would need them again: that
//! sets `needs_rebuild` (the caller rescans without freezing). Real
//! transcripts reach back a few dozen intervals at most, and subagents are
//! referenced when they start.

use super::json::{JVal, Line, Stop, Usage};
use super::TokenMetrics;
use std::collections::{BTreeSet, VecDeque};

pub const MAX_SPEED_WINDOW_MS: f64 = 120_000.0;
const ACTIVE_HIGH: usize = 2048;
const ACTIVE_LOW: usize = 1024;
const RECENT_REQUESTS_CAP: usize = 200_000;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Acc {
    pub input: f64,
    pub output: f64,
    pub read: f64,
    pub creation: f64,
    pub recent_main: Option<Usage>,
    pub recent_ts: Option<f64>,
    pub recent_post: Option<Usage>,
    pub recent_post_ts: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub usage: Usage,
    pub stop: Stop,
    pub ts: Option<f64>,
    pub main: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TokenState {
    pub acc: Acc,
    pub has_stop_field: bool,
    pub last_entry: Option<Entry>,
    pub saw_boundary: bool,
    pub boundary_after_last: bool,
    pub last_boundary_post: Option<f64>,
}

fn accumulate(acc: &mut Acc, e: &Entry, post: bool) {
    acc.input += e.usage.input;
    acc.output += e.usage.output;
    acc.read += e.usage.read;
    acc.creation += e.usage.creation;
    let (true, Some(ts)) = (e.main, e.ts) else { return };
    if acc.recent_ts.is_none_or(|r| ts > r) {
        acc.recent_ts = Some(ts);
        acc.recent_main = Some(e.usage);
    }
    if post && acc.recent_post_ts.is_none_or(|r| ts > r) {
        acc.recent_post_ts = Some(ts);
        acc.recent_post = Some(e.usage);
    }
}

impl TokenState {
    /// `collectTokenMetricRecord`
    fn collect(&mut self, l: &Line, ts: Option<f64>) {
        let boundary = is_compact_boundary(l);
        if boundary {
            self.saw_boundary = true;
            self.boundary_after_last = true;
            self.last_boundary_post = compact_post_tokens(l);
            self.acc.recent_post = None;
            self.acc.recent_post_ts = None;
        }
        let Some(m) = &l.message else { return };
        let Some(usage) = m.usage_tokens() else { return };
        let entry = Entry { usage, stop: m.stop, ts, main: !l.sidechain() && !l.api_error() };
        if m.stop != Stop::Absent && !self.has_stop_field {
            self.has_stop_field = true;
            self.acc = Acc::default();
        }
        if !self.has_stop_field || entry.stop == Stop::Truthy {
            accumulate(&mut self.acc, &entry, !boundary);
        }
        self.last_entry = Some(entry);
        self.boundary_after_last = boundary;
    }

    /// `finishTokenMetrics`
    pub fn finish(&self) -> TokenMetrics {
        let mut acc = self.acc.clone();
        if self.has_stop_field {
            if let Some(e) = self.last_entry.as_ref().filter(|e| e.stop == Stop::Null) {
                accumulate(&mut acc, e, !self.boundary_after_last);
            }
        }
        let context_length = if self.saw_boundary {
            acc.recent_post.map(|u| u.context_length()).or(self.last_boundary_post).unwrap_or(0.0)
        } else {
            acc.recent_main.map(|u| u.context_length()).unwrap_or(0.0)
        };
        let cached = acc.read + acc.creation;
        TokenMetrics {
            input_tokens: acc.input,
            output_tokens: acc.output,
            cached_tokens: cached,
            cache_read_tokens: acc.read,
            cache_creation_tokens: acc.creation,
            total_tokens: acc.input + acc.output + cached,
            context_length,
        }
    }
}

/// `isCompactBoundary`
pub fn is_compact_boundary(l: &Line) -> bool {
    l.is_object
        && l.type_is("system")
        && l.subtype.as_ref().and_then(JVal::as_str) == Some("compact_boundary")
        && !l.sidechain()
}

fn meta_obj(l: &Line) -> Option<&JVal> {
    l.compact_metadata.as_ref().filter(|m| matches!(m, JVal::Obj(_) | JVal::Arr(_)))
}

/// `getCompactBoundaryPostTokens` (only called on boundaries)
fn compact_post_tokens(l: &Line) -> Option<f64> {
    let post = meta_obj(l)?.get("postTokens")?.as_num()?;
    post.is_finite().then_some(if post > 0.0 { post } else { 0.0 })
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Compaction {
    pub count: f64,
    pub auto: f64,
    pub manual: f64,
    pub unknown: f64,
    pub reclaimed: f64,
}

impl Compaction {
    /// `accumulateCompactionStats`
    fn collect(&mut self, l: &Line) {
        if !is_compact_boundary(l) {
            return;
        }
        self.count += 1.0;
        let meta = meta_obj(l);
        match meta.and_then(|m| m.get("trigger")).and_then(JVal::as_str) {
            Some("auto") => self.auto += 1.0,
            Some("manual") => self.manual += 1.0,
            _ => self.unknown += 1.0,
        }
        let pre = meta.and_then(|m| m.get("preTokens")).and_then(JVal::as_num);
        let post = meta.and_then(|m| m.get("postTokens")).and_then(JVal::as_num);
        if let (Some(pre), Some(post)) = (pre, post) {
            let r = pre - post;
            if r.is_finite() {
                self.reclaimed += if r > 0.0 { r } else { 0.0 };
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Effort {
    pub value: String,
    pub known: bool,
}

/// `normalizeThinkingEffort`
pub fn normalize_effort(v: Option<&str>) -> Option<Effort> {
    let v = v.filter(|s| !s.is_empty())?;
    // toLowerCase; the accepted forms are ASCII, so other letters never match
    let n = v.to_lowercase();
    if ["low", "medium", "high", "xhigh", "max"].contains(&n.as_str()) {
        return Some(Effort { value: n, known: true });
    }
    // /^(?=.*[a-z0-9])[a-z0-9-]{2,20}$/
    let ok = (2..=20).contains(&n.len())
        && n.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && n.bytes().any(|b| b != b'-');
    ok.then_some(Effort { value: n, known: false })
}

const MODEL_PREFIX: &str = "<local-command-stdout>Set model to ";
const EFFORT_PREFIX: &str = "<local-command-stdout>Set effort level to ";

fn is_effort_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-'
}

/// `getThinkingEffortUpdate`: Some(update) when the record sets the effort.
pub fn thinking_effort_update(l: &Line) -> Option<Option<Effort>> {
    let content = l.message.as_ref()?.content_str.as_deref()?;
    if !content.contains(EFFORT_PREFIX) && !content.contains(MODEL_PREFIX) {
        return None;
    }
    let visible = crate::ansi::get_visible_text(content);
    let visible = crate::jsnum::js_trim(&visible);
    if let Some(rest) = visible.strip_prefix(EFFORT_PREFIX) {
        // /^<local-command-stdout>Set effort level to ([a-zA-Z0-9-]+)\b/i
        let n = rest.bytes().take_while(|b| is_effort_word(*b)).count();
        if n == 0 {
            return None;
        }
        // `\b` after the word: a '-' at the end backtracks to the last word char
        let mut end = n;
        let word_char = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
        loop {
            let before = word_char(rest.as_bytes()[end - 1]);
            let after = rest.as_bytes().get(end).is_some_and(|b| word_char(*b));
            if before != after {
                break;
            }
            end -= 1;
            if end == 0 {
                return None;
            }
        }
        return Some(normalize_effort(Some(&rest[..end])));
    }
    if !visible.starts_with(MODEL_PREFIX) {
        return None;
    }
    Some(normalize_effort(model_effort(visible).as_deref()))
}

/// `/^<local-command-stdout>Set model to[\s\S]*? with ([a-zA-Z0-9-]+) effort<\/local-command-stdout>$/i`
fn model_effort(s: &str) -> Option<String> {
    const HEAD: &str = "<local-command-stdout>Set model to";
    const TAIL: &str = " effort</local-command-stdout>";
    let lower = s.to_ascii_lowercase();
    if !lower.starts_with(&HEAD.to_ascii_lowercase()) || !lower.ends_with(&TAIL.to_ascii_lowercase()) {
        return None;
    }
    let body_end = s.len() - TAIL.len();
    // lazy `[\s\S]*?`: the first " with " whose remainder up to TAIL is one word
    let mut from = HEAD.len();
    while let Some(i) = lower[from..].find(" with ") {
        let start = from + i + 6;
        if start <= body_end {
            let word = &s[start..body_end];
            if !word.is_empty() && word.bytes().all(is_effort_word) {
                return Some(word.to_string());
            }
        }
        from = from + i + 1;
    }
    None
}

/// `getSessionNameFromRecord`
pub fn session_name_of(l: &Line) -> Option<String> {
    if !l.is_object || !l.type_is("custom-title") {
        return None;
    }
    l.custom_title.as_ref().and_then(JVal::as_str).filter(|s| !s.is_empty()).map(String::from)
}

/// `parseTimestampMs(data?.timestamp)`
pub fn timestamp_ms(l: &Line) -> Option<f64> {
    match l.timestamp.as_ref()? {
        JVal::Str(s) if !s.is_empty() => crate::timefmt::parse_iso(s),
        // Date.parse(String(v)) of a number/boolean/object: not ISO
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Request {
    pub input: f64,
    pub output: f64,
    pub ts: Option<f64>,
    pub interval: Option<(f64, f64)>,
}

/// Per-file part of `collectSpeedMetricRecord`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpeedCursor {
    pub last_user_ts: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpeedTotals {
    pub input: f64,
    pub output: f64,
    pub count: f64,
    pub latest: Option<f64>,
    /// sorted, disjoint (touching ones merged)
    pub active: Vec<(f64, f64)>,
    pub frozen_ms: f64,
    pub frozen_end: Option<f64>,
    pub no_freeze: bool,
    pub needs_rebuild: bool,
    /// requests whose timestamp may still fall in a ≤120 s window
    pub recent: VecDeque<Request>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SpeedMetrics {
    pub total_duration_ms: f64,
    pub input_tokens: f64,
    pub output_tokens: f64,
    pub total_tokens: f64,
}

impl SpeedMetrics {
    fn rate(&self, v: f64) -> Option<f64> {
        (self.total_duration_ms != 0.0).then(|| v / (self.total_duration_ms / 1000.0))
    }
    pub fn input_speed(&self) -> Option<f64> {
        self.rate(self.input_tokens)
    }
    pub fn output_speed(&self) -> Option<f64> {
        self.rate(self.output_tokens)
    }
    pub fn total_speed(&self) -> Option<f64> {
        self.rate(self.total_tokens)
    }
}

fn merged_length(mut iv: Vec<(f64, f64)>) -> f64 {
    iv.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut total = 0.0;
    let mut cur: Option<(f64, f64)> = None;
    for (s, e) in iv {
        match &mut cur {
            Some(c) if s <= c.1 => c.1 = c.1.max(e),
            _ => {
                if let Some(c) = cur {
                    total += c.1 - c.0;
                }
                cur = Some((s, e));
            }
        }
    }
    if let Some(c) = cur {
        total += c.1 - c.0;
    }
    total
}

impl SpeedTotals {
    /// `collectSpeedMetricRecord`
    fn collect(&mut self, cur: &mut SpeedCursor, l: Option<&Line>, ts: Option<f64>, ignore_sidechain: bool) {
        let Some(l) = l else { return };
        if !l.is_object {
            // truthy primitives and arrays have no fields: only `!data` matters,
            // and neither adds anything
            return;
        }
        if l.api_error() || (ignore_sidechain && l.sidechain()) {
            return;
        }
        if let Some(t) = ts {
            if self.latest.is_none_or(|x| t > x) {
                self.latest = Some(t);
                self.prune_recent();
            }
        }
        if l.type_is("user") {
            if let Some(t) = ts {
                cur.last_user_ts = Some(t);
                return;
            }
        }
        if !l.type_is("assistant") {
            return;
        }
        let Some(usage) = l.message.as_ref().and_then(|m| m.usage_tokens()) else { return };
        let interval = match (ts, cur.last_user_ts) {
            (Some(t), Some(u)) if t > u => Some((u, t)),
            _ => None,
        };
        self.input += usage.input;
        self.output += usage.output;
        self.count += 1.0;
        if let Some(iv) = interval {
            self.add_interval(iv);
        }
        let req = Request { input: usage.input, output: usage.output, ts, interval };
        if let (Some(t), Some(latest)) = (ts, self.latest) {
            if t >= latest - MAX_SPEED_WINDOW_MS && self.recent.len() < RECENT_REQUESTS_CAP {
                self.recent.push_back(req);
            }
        }
    }

    fn prune_recent(&mut self) {
        let Some(latest) = self.latest else { return };
        let floor = latest - MAX_SPEED_WINDOW_MS;
        self.recent.retain(|r| r.ts.is_some_and(|t| t >= floor));
    }

    fn add_interval(&mut self, (s, e): (f64, f64)) {
        if self.frozen_end.is_some_and(|fe| s < fe) {
            self.needs_rebuild = true;
        }
        // insert keeping the list sorted by start, then merge neighbours
        let pos = self.active.partition_point(|iv| iv.0 <= s);
        let mut lo = pos;
        let (mut ns, mut ne) = (s, e);
        if lo > 0 && self.active[lo - 1].1 >= ns {
            lo -= 1;
            ns = self.active[lo].0;
            ne = ne.max(self.active[lo].1);
        }
        let mut hi = pos;
        while hi < self.active.len() && self.active[hi].0 <= ne {
            ne = ne.max(self.active[hi].1);
            hi += 1;
        }
        self.active.splice(lo..hi, [(ns, ne)]);
    }

    /// Fold the oldest intervals into `frozen_ms` (between passes only).
    pub fn freeze(&mut self) {
        if self.no_freeze || self.active.len() <= ACTIVE_HIGH {
            return;
        }
        let n = self.active.len() - ACTIVE_LOW;
        for (fs, fe) in self.active.drain(..n) {
            self.frozen_ms += fe - fs;
            self.frozen_end = Some(fe);
        }
    }

    /// `buildSpeedMetrics(combined)` — the session average.
    pub fn session_average(&self) -> SpeedMetrics {
        let duration = self.frozen_ms + self.active.iter().map(|(s, e)| e - s).sum::<f64>();
        SpeedMetrics {
            total_duration_ms: duration,
            input_tokens: self.input,
            output_tokens: self.output,
            total_tokens: self.input + self.output,
        }
    }

    /// `buildSpeedMetrics(combined, windowSeconds)` (window already normalized, ≤120).
    pub fn windowed(&self, window_seconds: f64) -> SpeedMetrics {
        let Some(end) = self.latest else { return SpeedMetrics::default() };
        let start = end - window_seconds * 1000.0;
        let (mut input, mut output) = (0.0, 0.0);
        let mut iv = Vec::new();
        for r in &self.recent {
            let Some(t) = r.ts else { continue };
            if t < start || t > end {
                continue;
            }
            input += r.input;
            output += r.output;
            if let Some((s, e)) = r.interval {
                let (cs, ce) = (s.max(start), e.min(end));
                if ce > cs {
                    iv.push((cs, ce));
                }
            }
        }
        SpeedMetrics {
            total_duration_ms: merged_length(iv),
            input_tokens: input,
            output_tokens: output,
            total_tokens: input + output,
        }
    }
}

/// Everything upstream derives from the main transcript.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Aggregates {
    pub tokens: TokenState,
    pub compaction: Compaction,
    pub first_ts: Option<f64>,
    pub last_ts: Option<f64>,
    pub thinking: Option<Effort>,
    pub session_name: Option<String>,
    pub agent_ids: BTreeSet<String>,
    /// speed (and the agent ids it needs) is tracked
    pub speed_on: bool,
    pub speed_cursor: SpeedCursor,
    pub speed: SpeedTotals,
}

impl Aggregates {
    /// One main-transcript line (None: not valid JSON).
    pub fn apply_main(&mut self, l: Option<&Line>, ids: &mut Vec<String>) {
        for id in ids.drain(..) {
            self.agent_ids.insert(id);
        }
        let Some(l) = l else { return };
        let ts = timestamp_ms(l);
        self.tokens.collect(l, ts);
        if let Some(t) = ts {
            self.first_ts.get_or_insert(t);
            self.last_ts = Some(t);
        }
        if self.speed_on {
            let mut cur = std::mem::take(&mut self.speed_cursor);
            self.speed.collect(&mut cur, Some(l), ts, true);
            self.speed_cursor = cur;
        }
        self.compaction.collect(l);
        if let Some(update) = thinking_effort_update(l) {
            self.thinking = update;
        }
        if let Some(name) = session_name_of(l) {
            self.session_name = Some(name);
        }
    }

    /// One subagent-transcript line (`collectSpeedMetricsFromFile(path, false)`).
    pub fn apply_subagent(&mut self, cur: &mut SpeedCursor, l: Option<&Line>) {
        let ts = l.and_then(timestamp_ms);
        self.speed.collect(cur, l, ts, false);
    }
}

/// `formatSessionDuration`
pub fn format_session_duration(first: Option<f64>, last: Option<f64>) -> Option<String> {
    let (first, last) = (first?, last?);
    Some(format_duration_ms(last - first))
}

/// `formatDurationFromMs` (SessionClock) — same shape as `formatSessionDuration`.
pub fn format_duration_ms(ms: f64) -> String {
    let total = (ms / 60_000.0).floor();
    if total < 1.0 {
        return "<1m".into();
    }
    let hours = (total / 60.0).floor();
    let minutes = total % 60.0;
    use crate::jsnum::to_js_string as s;
    if hours == 0.0 {
        format!("{}m", s(minutes))
    } else if minutes == 0.0 {
        format!("{}hr", s(hours))
    } else {
        format!("{}hr {}m", s(hours), s(minutes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effort_parsing() {
        let mk = |c: &str| {
            let l = Line {
                is_object: true,
                message: Some(super::super::json::Message { content_str: Some(c.into()), ..Default::default() }),
                ..Default::default()
            };
            thinking_effort_update(&l)
        };
        assert_eq!(mk("hello"), None);
        assert_eq!(
            mk("<local-command-stdout>Set effort level to High (thinking)</local-command-stdout>"),
            Some(Some(Effort { value: "high".into(), known: true }))
        );
        assert_eq!(
            mk("<local-command-stdout>Set effort level to super-max-</local-command-stdout>"),
            Some(Some(Effort { value: "super-max".into(), known: false }))
        );
        assert_eq!(mk("<local-command-stdout>Set model to opus</local-command-stdout>"), Some(None));
        assert_eq!(
            mk("<local-command-stdout>Set model to opus with x with MAX effort</local-command-stdout>"),
            Some(Some(Effort { value: "max".into(), known: true }))
        );
        assert_eq!(mk("  <local-command-stdout>Set effort level to </local-command-stdout>"), None);
    }

    #[test]
    fn intervals_merge_and_freeze() {
        let mut s = SpeedTotals::default();
        for (a, b) in [(10.0, 20.0), (0.0, 5.0), (4.0, 11.0), (30.0, 40.0), (40.0, 41.0)] {
            s.add_interval((a, b));
        }
        assert_eq!(s.active, vec![(0.0, 20.0), (30.0, 41.0)]);
        let mut s = SpeedTotals::default();
        for i in 0..3000 {
            let t = i as f64 * 10.0;
            s.add_interval((t, t + 5.0));
        }
        assert_eq!(s.active.len(), 3000);
        s.freeze();
        assert_eq!(s.active.len(), ACTIVE_LOW);
        assert_eq!(s.session_average().total_duration_ms, 15_000.0);
        s.add_interval((29_995.0, 30_005.0));
        assert_eq!(s.session_average().total_duration_ms, 15_010.0);
        assert!(!s.needs_rebuild);
        s.add_interval((1.0, 2.0));
        assert!(s.needs_rebuild);
    }

    #[test]
    fn durations() {
        assert_eq!(format_duration_ms(59_999.0), "<1m");
        assert_eq!(format_duration_ms(3_723_000.0), "1hr 2m");
        assert_eq!(format_duration_ms(7_200_000.0), "2hr");
        assert_eq!(format_duration_ms(300_000.0), "5m");
    }
}
