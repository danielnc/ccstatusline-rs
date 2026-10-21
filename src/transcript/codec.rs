//! Binary encoding of the per-transcript cache (`Cache`). Little-endian,
//! length-prefixed; any decoding problem means "no cache".

use super::json::{Stop, Usage};
use super::state::{Acc, Aggregates, Compaction, Effort, Entry, Request, SpeedCursor, SpeedTotals, TokenState};
use super::{Cache, FileCursor, SubagentCursor};

const MAGIC: &[u8; 8] = b"CCSLRT03";

#[derive(Default)]
struct W(Vec<u8>);

impl W {
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f64(&mut self, v: f64) {
        self.u64(v.to_bits());
    }
    fn bool(&mut self, v: bool) {
        self.0.push(v as u8);
    }
    fn of64(&mut self, v: Option<f64>) {
        self.bool(v.is_some());
        if let Some(v) = v {
            self.f64(v);
        }
    }
    fn str(&mut self, s: &str) {
        self.u64(s.len() as u64);
        self.0.extend_from_slice(s.as_bytes());
    }
    fn usage(&mut self, u: &Usage) {
        for v in [u.input, u.output, u.creation, u.read] {
            self.f64(v);
        }
    }
    fn ousage(&mut self, u: &Option<Usage>) {
        self.bool(u.is_some());
        if let Some(u) = u {
            self.usage(u);
        }
    }
    fn cursor(&mut self, c: &FileCursor) {
        for v in [c.dev, c.ino, c.offset, c.fingerprint] {
            self.u64(v);
        }
    }
}

struct Rd<'a> {
    b: &'a [u8],
    i: usize,
}

type R<T> = Option<T>;

impl Rd<'_> {
    fn take(&mut self, n: usize) -> R<&[u8]> {
        let s = self.b.get(self.i..self.i.checked_add(n)?)?;
        self.i += n;
        Some(s)
    }
    fn u64(&mut self) -> R<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    fn f64(&mut self) -> R<f64> {
        Some(f64::from_bits(self.u64()?))
    }
    fn bool(&mut self) -> R<bool> {
        match self.take(1)?[0] {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }
    fn of64(&mut self) -> R<Option<f64>> {
        Some(if self.bool()? { Some(self.f64()?) } else { None })
    }
    fn len(&mut self) -> R<usize> {
        let n = self.u64()? as usize;
        // every element takes at least one byte
        (n <= self.b.len() - self.i).then_some(n)
    }
    fn str(&mut self) -> R<String> {
        let n = self.len()?;
        String::from_utf8(self.take(n)?.to_vec()).ok()
    }
    fn usage(&mut self) -> R<Usage> {
        Some(Usage { input: self.f64()?, output: self.f64()?, creation: self.f64()?, read: self.f64()? })
    }
    fn ousage(&mut self) -> R<Option<Usage>> {
        Some(if self.bool()? { Some(self.usage()?) } else { None })
    }
    fn cursor(&mut self) -> R<FileCursor> {
        Some(FileCursor { dev: self.u64()?, ino: self.u64()?, offset: self.u64()?, fingerprint: self.u64()? })
    }
}

fn stop_code(s: Stop) -> u8 {
    match s {
        Stop::Absent => 0,
        Stop::Null => 1,
        Stop::Truthy => 2,
        Stop::Falsy => 3,
    }
}

fn stop_of(c: u8) -> R<Stop> {
    Some(match c {
        0 => Stop::Absent,
        1 => Stop::Null,
        2 => Stop::Truthy,
        3 => Stop::Falsy,
        _ => return None,
    })
}

pub fn encode(c: &Cache) -> Vec<u8> {
    let mut w = W::default();
    w.0.extend_from_slice(MAGIC);
    w.str(&c.path);
    w.cursor(&c.main);
    let a = &c.agg;
    let t = &a.tokens;
    for v in [t.acc.input, t.acc.output, t.acc.read, t.acc.creation] {
        w.f64(v);
    }
    w.ousage(&t.acc.recent_main);
    w.of64(t.acc.recent_ts);
    w.ousage(&t.acc.recent_post);
    w.of64(t.acc.recent_post_ts);
    w.bool(t.has_stop_field);
    w.bool(t.last_entry.is_some());
    if let Some(e) = &t.last_entry {
        w.usage(&e.usage);
        w.0.push(stop_code(e.stop));
        w.of64(e.ts);
        w.bool(e.main);
    }
    w.bool(t.saw_boundary);
    w.bool(t.boundary_after_last);
    w.of64(t.last_boundary_post);
    let cp = &a.compaction;
    for v in [cp.count, cp.auto, cp.manual, cp.unknown, cp.reclaimed] {
        w.f64(v);
    }
    w.of64(a.first_ts);
    w.of64(a.last_ts);
    w.bool(a.thinking.is_some());
    if let Some(e) = &a.thinking {
        w.str(&e.value);
        w.bool(e.known);
    }
    w.bool(a.session_name.is_some());
    if let Some(n) = &a.session_name {
        w.str(n);
    }
    w.u64(a.agent_ids.len() as u64);
    for id in &a.agent_ids {
        w.str(id);
    }
    w.bool(a.speed_on);
    w.of64(a.speed_cursor.last_user_ts);
    let s = &a.speed;
    w.f64(s.input);
    w.f64(s.output);
    w.f64(s.count);
    w.of64(s.latest);
    w.u64(s.active.len() as u64);
    for (x, y) in &s.active {
        w.f64(*x);
        w.f64(*y);
    }
    w.f64(s.frozen_ms);
    w.of64(s.frozen_end);
    w.bool(s.no_freeze);
    w.bool(s.needs_rebuild);
    w.u64(s.recent.len() as u64);
    for r in &s.recent {
        w.f64(r.input);
        w.f64(r.output);
        w.of64(r.ts);
        w.bool(r.interval.is_some());
        if let Some((x, y)) = r.interval {
            w.f64(x);
            w.f64(y);
        }
    }
    w.u64(c.subagents.len() as u64);
    for sa in &c.subagents {
        w.str(&sa.path);
        w.cursor(&sa.file);
        w.of64(sa.speed.last_user_ts);
    }
    w.0
}

pub fn decode(b: &[u8]) -> Option<Cache> {
    let mut r = Rd { b, i: 0 };
    if r.take(8)? != MAGIC {
        return None;
    }
    let path = r.str()?;
    let main = r.cursor()?;
    let acc = Acc {
        input: r.f64()?,
        output: r.f64()?,
        read: r.f64()?,
        creation: r.f64()?,
        recent_main: r.ousage()?,
        recent_ts: r.of64()?,
        recent_post: r.ousage()?,
        recent_post_ts: r.of64()?,
    };
    let has_stop_field = r.bool()?;
    let last_entry = if r.bool()? {
        Some(Entry { usage: r.usage()?, stop: stop_of(r.take(1)?[0])?, ts: r.of64()?, main: r.bool()? })
    } else {
        None
    };
    let tokens = TokenState {
        acc,
        has_stop_field,
        last_entry,
        saw_boundary: r.bool()?,
        boundary_after_last: r.bool()?,
        last_boundary_post: r.of64()?,
    };
    let compaction =
        Compaction { count: r.f64()?, auto: r.f64()?, manual: r.f64()?, unknown: r.f64()?, reclaimed: r.f64()? };
    let first_ts = r.of64()?;
    let last_ts = r.of64()?;
    let thinking = if r.bool()? { Some(Effort { value: r.str()?, known: r.bool()? }) } else { None };
    let session_name = if r.bool()? { Some(r.str()?) } else { None };
    let mut agent_ids = std::collections::BTreeSet::new();
    for _ in 0..r.len()? {
        agent_ids.insert(r.str()?);
    }
    let speed_on = r.bool()?;
    let speed_cursor = SpeedCursor { last_user_ts: r.of64()? };
    let input = r.f64()?;
    let output = r.f64()?;
    let count = r.f64()?;
    let latest = r.of64()?;
    let mut active = Vec::new();
    for _ in 0..r.len()? {
        active.push((r.f64()?, r.f64()?));
    }
    let frozen_ms = r.f64()?;
    let frozen_end = r.of64()?;
    let no_freeze = r.bool()?;
    let needs_rebuild = r.bool()?;
    let mut recent = std::collections::VecDeque::new();
    for _ in 0..r.len()? {
        let input = r.f64()?;
        let output = r.f64()?;
        let ts = r.of64()?;
        let interval = if r.bool()? { Some((r.f64()?, r.f64()?)) } else { None };
        recent.push_back(Request { input, output, ts, interval });
    }
    let speed =
        SpeedTotals { input, output, count, latest, active, frozen_ms, frozen_end, no_freeze, needs_rebuild, recent };
    let mut subagents = Vec::new();
    for _ in 0..r.len()? {
        subagents.push(SubagentCursor {
            path: r.str()?,
            file: r.cursor()?,
            speed: SpeedCursor { last_user_ts: r.of64()? },
        });
    }
    if r.i != b.len() {
        return None;
    }
    let agg = Aggregates {
        tokens,
        compaction,
        first_ts,
        last_ts,
        thinking,
        session_name,
        agent_ids,
        speed_on,
        speed_cursor,
        speed,
    };
    Some(Cache { path, main, agg, subagents })
}
