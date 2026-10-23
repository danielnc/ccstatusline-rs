//! A `JSON.parse`-compatible scanner for transcript lines.
//!
//! Upstream runs `JSON.parse` on every line and then reads a handful of
//! fields. Building a full value for a 1 MB tool result just to read
//! `type`/`timestamp` is the dominant cost, so this scanner validates the
//! whole line with the exact JSON grammar (a line `JSON.parse` rejects is
//! rejected here too) but only materializes the fields the analyses read.
//! Everything else is skipped without allocating, except that every
//! `"agentId": "<id>"` pair at any depth is collected (`collectAgentIds`).
//!
//! JS semantics kept: lone surrogate escapes are accepted (and decode to
//! U+FFFD, which is what Node prints for them), invalid UTF-8 never fails a
//! parse inside strings (Node decodes it to U+FFFD before parsing), numbers
//! overflow to ±Infinity, duplicate keys resolve to the last occurrence.

/// A small JS value, for the fields that are read.
#[derive(Clone, Debug, PartialEq)]
pub enum JVal {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<JVal>),
    Obj(Vec<(String, JVal)>),
}

impl JVal {
    /// JS truthiness.
    pub fn truthy(&self) -> bool {
        match self {
            JVal::Null => false,
            JVal::Bool(b) => *b,
            JVal::Num(n) => *n != 0.0 && !n.is_nan(),
            JVal::Str(s) => !s.is_empty(),
            JVal::Arr(_) | JVal::Obj(_) => true,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            JVal::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_num(&self) -> Option<f64> {
        match self {
            JVal::Num(n) => Some(*n),
            _ => None,
        }
    }
    /// `obj[key]` with last-wins duplicate semantics; None is `undefined`.
    pub fn get(&self, key: &str) -> Option<&JVal> {
        match self {
            JVal::Obj(m) => m.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn is_true(&self) -> bool {
        matches!(self, JVal::Bool(true))
    }
}

/// `message.usage` coerced like `parseUsageTokens`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Usage {
    pub input: f64,
    pub output: f64,
    pub creation: f64,
    pub read: f64,
}

/// `toFiniteNonNegativeNumber`
fn finite_non_neg(v: Option<&JVal>) -> f64 {
    match v {
        Some(JVal::Num(n)) if n.is_finite() && *n > 0.0 => *n,
        _ => 0.0,
    }
}

impl Usage {
    pub fn from_jval(v: &JVal) -> Usage {
        Usage {
            input: finite_non_neg(v.get("input_tokens")),
            output: finite_non_neg(v.get("output_tokens")),
            creation: finite_non_neg(v.get("cache_creation_input_tokens")),
            read: finite_non_neg(v.get("cache_read_input_tokens")),
        }
    }
    /// `contextLengthFromUsageTokens`
    pub fn context_length(&self) -> f64 {
        self.input + self.creation + self.read
    }
}

/// `message.stop_reason`, as far as the token metrics care.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Stop {
    /// no own `stop_reason` property
    #[default]
    Absent,
    Null,
    Truthy,
    Falsy,
}

#[derive(Clone, Debug, Default)]
pub struct Message {
    /// the raw `usage` value (None when absent)
    pub usage: Option<JVal>,
    pub stop: Stop,
    /// `content` when it is a string
    pub content_str: Option<String>,
    /// `content.length` when content is an array
    pub content_len: Option<usize>,
    /// `content[0]` when content is an array (only in cache-timer mode)
    pub content_first: Option<JVal>,
}

impl Message {
    /// `message.usage` when truthy, coerced.
    pub fn usage_tokens(&self) -> Option<Usage> {
        self.usage.as_ref().filter(|u| u.truthy()).map(Usage::from_jval)
    }
}

/// What a line's analyses read. `is_object` is false for arrays and
/// primitives (whose field lookups are all `undefined`).
#[derive(Clone, Debug, Default)]
pub struct Line {
    pub is_object: bool,
    pub kind: Option<JVal>,
    pub subtype: Option<JVal>,
    pub is_sidechain: Option<JVal>,
    pub is_api_error: Option<JVal>,
    pub timestamp: Option<JVal>,
    pub custom_title: Option<JVal>,
    pub compact_metadata: Option<JVal>,
    /// `message` when it is an object
    pub message: Option<Message>,
}

impl Line {
    pub fn type_is(&self, t: &str) -> bool {
        self.kind.as_ref().and_then(JVal::as_str) == Some(t)
    }
    pub fn sidechain(&self) -> bool {
        self.is_sidechain.as_ref().is_some_and(JVal::is_true)
    }
    pub fn api_error(&self) -> bool {
        self.is_api_error.as_ref().is_some_and(JVal::truthy)
    }
}

pub struct Options {
    /// collect `agentId` values
    pub agent_ids: bool,
    /// read `message.content[0]` (cache timer)
    pub content_first: bool,
}

struct P<'a, 'o> {
    b: &'a [u8],
    i: usize,
    opts: &'o Options,
    ids: &'o mut Vec<String>,
}

type R<T> = Result<T, ()>;

const MAX_DEPTH: usize = 512;

#[inline]
fn is_ws(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r')
}

const LO: u64 = 0x0101_0101_0101_0101;
const HI: u64 = 0x8080_8080_8080_8080;

/// Whether any byte of the 8-byte word is `"`, `\` or a control character.
#[inline]
fn word_has_special(w: u64) -> bool {
    let q = w ^ (LO * b'"' as u64);
    let s = w ^ (LO * b'\\' as u64);
    let has_q = q.wrapping_sub(LO) & !q & HI;
    let has_s = s.wrapping_sub(LO) & !s & HI;
    let has_ctl = w.wrapping_sub(LO * 0x20) & !w & HI;
    (has_q | has_s | has_ctl) != 0
}

/// Index of the first `"`, `\` or control byte at or after `i`.
#[inline]
fn find_special(b: &[u8], mut i: usize) -> usize {
    while i + 8 <= b.len() {
        let w = u64::from_le_bytes(b[i..i + 8].try_into().unwrap());
        if word_has_special(w) {
            break;
        }
        i += 8;
    }
    while i < b.len() {
        let c = b[i];
        if c == b'"' || c == b'\\' || c < 0x20 {
            return i;
        }
        i += 1;
    }
    i
}

fn hex4(b: &[u8], i: usize) -> R<u32> {
    let s = b.get(i..i + 4).ok_or(())?;
    let mut n = 0u32;
    for &c in s {
        let d = match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => return Err(()),
        };
        n = n * 16 + d as u32;
    }
    Ok(n)
}

fn push_char(out: &mut Vec<u8>, c: char) {
    let mut buf = [0u8; 4];
    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
}

impl<'a, 'o> P<'a, 'o> {
    #[inline]
    fn ws(&mut self) {
        while self.i < self.b.len() && is_ws(self.b[self.i]) {
            self.i += 1;
        }
    }

    #[inline]
    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn expect(&mut self, c: u8) -> R<()> {
        if self.peek() == Some(c) {
            self.i += 1;
            Ok(())
        } else {
            Err(())
        }
    }

    /// Skip a string; `self.i` is at the opening quote. Returns the raw
    /// contents range and whether it had escapes.
    fn skip_string(&mut self) -> R<(usize, usize, bool)> {
        self.i += 1;
        let start = self.i;
        let mut escaped = false;
        loop {
            let j = find_special(self.b, self.i);
            let c = *self.b.get(j).ok_or(())?;
            if c == b'"' {
                self.i = j + 1;
                return Ok((start, j, escaped));
            }
            if c < 0x20 {
                return Err(());
            }
            // backslash
            escaped = true;
            match self.b.get(j + 1) {
                Some(b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't') => self.i = j + 2,
                Some(b'u') => {
                    hex4(self.b, j + 2)?;
                    self.i = j + 6;
                }
                _ => return Err(()),
            }
        }
    }

    /// Decode a string's raw contents (already validated by `skip_string`).
    fn decode(&self, start: usize, end: usize, escaped: bool) -> String {
        let raw = &self.b[start..end];
        if !escaped {
            return String::from_utf8_lossy(raw).into_owned();
        }
        let mut out = Vec::with_capacity(raw.len());
        let mut i = 0;
        while i < raw.len() {
            let c = raw[i];
            if c != b'\\' {
                out.push(c);
                i += 1;
                continue;
            }
            let e = raw[i + 1];
            i += 2;
            match e {
                b'b' => out.push(8),
                b'f' => out.push(12),
                b'n' => out.push(b'\n'),
                b'r' => out.push(b'\r'),
                b't' => out.push(b'\t'),
                b'u' => {
                    let n = hex4(raw, i).unwrap_or(0xFFFD);
                    i += 4;
                    if (0xD800..0xDC00).contains(&n) && raw.get(i) == Some(&b'\\') && raw.get(i + 1) == Some(&b'u') {
                        if let Ok(lo) = hex4(raw, i + 2) {
                            if (0xDC00..0xE000).contains(&lo) {
                                let cp = 0x10000 + ((n - 0xD800) << 10) + (lo - 0xDC00);
                                push_char(&mut out, char::from_u32(cp).unwrap_or('\u{FFFD}'));
                                i += 6;
                                continue;
                            }
                        }
                    }
                    push_char(&mut out, char::from_u32(n).unwrap_or('\u{FFFD}'));
                }
                other => out.push(other),
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    fn string(&mut self) -> R<String> {
        let (s, e, esc) = self.skip_string()?;
        Ok(self.decode(s, e, esc))
    }

    /// Object key; compares cheaply when unescaped.
    fn key(&mut self) -> R<KeyRef> {
        if self.peek() != Some(b'"') {
            return Err(());
        }
        let (s, e, esc) = self.skip_string()?;
        Ok(KeyRef { s, e, decoded: if esc { Some(self.decode(s, e, true)) } else { None } })
    }

    fn key_is(&self, k: &KeyRef, name: &str) -> bool {
        match &k.decoded {
            Some(d) => d == name,
            None => &self.b[k.s..k.e] == name.as_bytes(),
        }
    }

    fn key_string(&self, k: &KeyRef) -> String {
        match &k.decoded {
            Some(d) => d.clone(),
            None => String::from_utf8_lossy(&self.b[k.s..k.e]).into_owned(),
        }
    }

    /// Validate a number and return its text range.
    fn number_span(&mut self) -> R<(usize, usize)> {
        let b = self.b;
        let start = self.i;
        let mut i = self.i;
        if b.get(i) == Some(&b'-') {
            i += 1;
        }
        match b.get(i) {
            Some(b'0') => i += 1,
            Some(b'1'..=b'9') => {
                while matches!(b.get(i), Some(b'0'..=b'9')) {
                    i += 1;
                }
            }
            _ => return Err(()),
        }
        if b.get(i) == Some(&b'.') {
            i += 1;
            if !matches!(b.get(i), Some(b'0'..=b'9')) {
                return Err(());
            }
            while matches!(b.get(i), Some(b'0'..=b'9')) {
                i += 1;
            }
        }
        if matches!(b.get(i), Some(b'e' | b'E')) {
            i += 1;
            if matches!(b.get(i), Some(b'+' | b'-')) {
                i += 1;
            }
            if !matches!(b.get(i), Some(b'0'..=b'9')) {
                return Err(());
            }
            while matches!(b.get(i), Some(b'0'..=b'9')) {
                i += 1;
            }
        }
        self.i = i;
        Ok((start, i))
    }

    fn number(&mut self) -> R<f64> {
        let (s, e) = self.number_span()?;
        // the grammar is ASCII; Rust's parse is correctly rounded like JS
        let t = std::str::from_utf8(&self.b[s..e]).map_err(|_| ())?;
        t.parse::<f64>().map_err(|_| ())
    }

    fn literal(&mut self, word: &[u8]) -> R<()> {
        if self.b.get(self.i..self.i + word.len()) == Some(word) {
            self.i += word.len();
            Ok(())
        } else {
            Err(())
        }
    }

    fn add_agent_id(&mut self, id: String) {
        if !crate::jsnum::js_trim(&id).is_empty() {
            self.ids.push(id);
        }
    }

    /// After an `"agentId"` key: a string value is collected, anything else
    /// is walked (it may hold nested ids).
    fn agent_id_value(&mut self) -> R<()> {
        if self.peek() == Some(b'"') {
            let s = self.string()?;
            if self.opts.agent_ids {
                self.add_agent_id(s);
            }
            Ok(())
        } else {
            self.skip_value()
        }
    }

    /// Validate and skip any value, collecting nested agent ids. Iterative,
    /// so nesting depth is unbounded like `JSON.parse`.
    fn skip_value(&mut self) -> R<()> {
        // stack of open containers: true = object, false = array
        let mut stack: Vec<bool> = Vec::new();
        loop {
            // parse one value at self.i
            self.ws();
            match self.peek().ok_or(())? {
                b'{' => {
                    self.i += 1;
                    self.ws();
                    if self.peek() == Some(b'}') {
                        self.i += 1;
                    } else {
                        stack.push(true);
                        if self.object_member_key()? {
                            continue;
                        }
                        // agentId with a string value was consumed
                        if !self.after_member(&mut stack)? {
                            return Ok(());
                        }
                        continue;
                    }
                }
                b'[' => {
                    self.i += 1;
                    self.ws();
                    if self.peek() == Some(b']') {
                        self.i += 1;
                    } else {
                        stack.push(false);
                        continue;
                    }
                }
                b'"' => {
                    self.skip_string()?;
                }
                b't' => self.literal(b"true")?,
                b'f' => self.literal(b"false")?,
                b'n' => self.literal(b"null")?,
                _ => {
                    self.number_span()?;
                }
            }
            // a value just ended
            if !self.after_member(&mut stack)? {
                return Ok(());
            }
        }
    }

    /// Reads `"key":` inside an object being skipped. Returns true when the
    /// caller should go on to parse the member's value, false when the value
    /// was already consumed (an `agentId` string).
    fn object_member_key(&mut self) -> R<bool> {
        let k = self.key()?;
        self.ws();
        self.expect(b':')?;
        self.ws();
        if self.opts.agent_ids && self.key_is(&k, "agentId") && self.peek() == Some(b'"') {
            let s = self.string()?;
            self.add_agent_id(s);
            return Ok(false);
        }
        Ok(true)
    }

    /// After a value inside the containers on `stack`: consume `,` or the
    /// closers. Returns false when the outermost container closed (the skip
    /// is done), true when another value follows.
    fn after_member(&mut self, stack: &mut Vec<bool>) -> R<bool> {
        loop {
            let Some(&is_obj) = stack.last() else { return Ok(false) };
            self.ws();
            match self.peek().ok_or(())? {
                b',' => {
                    self.i += 1;
                    self.ws();
                    if is_obj {
                        if self.object_member_key()? {
                            return Ok(true);
                        }
                        continue;
                    }
                    return Ok(true);
                }
                b'}' if is_obj => {
                    self.i += 1;
                    stack.pop();
                }
                b']' if !is_obj => {
                    self.i += 1;
                    stack.pop();
                }
                _ => return Err(()),
            }
            if stack.is_empty() {
                return Ok(false);
            }
        }
    }

    /// Parse a value fully (fields that are read are small).
    fn jval(&mut self, depth: usize) -> R<JVal> {
        self.ws();
        if depth > MAX_DEPTH {
            self.skip_value()?;
            return Ok(JVal::Null);
        }
        Ok(match self.peek().ok_or(())? {
            b'{' => {
                self.i += 1;
                let mut m: Vec<(String, JVal)> = Vec::new();
                self.ws();
                if self.peek() == Some(b'}') {
                    self.i += 1;
                    return Ok(JVal::Obj(m));
                }
                loop {
                    self.ws();
                    let k = self.key()?;
                    self.ws();
                    self.expect(b':')?;
                    let v = self.jval(depth + 1)?;
                    if self.opts.agent_ids && self.key_is(&k, "agentId") {
                        if let JVal::Str(s) = &v {
                            self.add_agent_id(s.clone());
                        }
                    }
                    let ks = self.key_string(&k);
                    if let Some(slot) = m.iter_mut().find(|(kk, _)| *kk == ks) {
                        slot.1 = v;
                    } else {
                        m.push((ks, v));
                    }
                    self.ws();
                    match self.peek().ok_or(())? {
                        b',' => self.i += 1,
                        b'}' => {
                            self.i += 1;
                            break;
                        }
                        _ => return Err(()),
                    }
                }
                JVal::Obj(m)
            }
            b'[' => {
                self.i += 1;
                let mut a = Vec::new();
                self.ws();
                if self.peek() == Some(b']') {
                    self.i += 1;
                    return Ok(JVal::Arr(a));
                }
                loop {
                    a.push(self.jval(depth + 1)?);
                    self.ws();
                    match self.peek().ok_or(())? {
                        b',' => self.i += 1,
                        b']' => {
                            self.i += 1;
                            break;
                        }
                        _ => return Err(()),
                    }
                }
                JVal::Arr(a)
            }
            b'"' => JVal::Str(self.string()?),
            b't' => {
                self.literal(b"true")?;
                JVal::Bool(true)
            }
            b'f' => {
                self.literal(b"false")?;
                JVal::Bool(false)
            }
            b'n' => {
                self.literal(b"null")?;
                JVal::Null
            }
            _ => JVal::Num(self.number()?),
        })
    }

    /// Iterate an object's members; `f` parses each value (self.i at the
    /// value, whitespace skipped). `self.i` is at `{`.
    fn object(&mut self, mut f: impl FnMut(&mut Self, &KeyRef) -> R<()>) -> R<()> {
        self.expect(b'{')?;
        self.ws();
        if self.peek() == Some(b'}') {
            self.i += 1;
            return Ok(());
        }
        loop {
            self.ws();
            let k = self.key()?;
            self.ws();
            self.expect(b':')?;
            self.ws();
            if self.key_is(&k, "agentId") {
                self.agent_id_value()?;
            } else {
                f(self, &k)?;
            }
            self.ws();
            match self.peek().ok_or(())? {
                b',' => self.i += 1,
                b'}' => {
                    self.i += 1;
                    return Ok(());
                }
                _ => return Err(()),
            }
        }
    }

    fn message(&mut self) -> R<Message> {
        let mut m = Message::default();
        self.object(|p, k| {
            if p.key_is(k, "usage") {
                m.usage = Some(p.jval(1)?);
            } else if p.key_is(k, "stop_reason") {
                let v = p.jval(1)?;
                m.stop = match v {
                    JVal::Null => Stop::Null,
                    v if v.truthy() => Stop::Truthy,
                    _ => Stop::Falsy,
                };
            } else if p.key_is(k, "content") {
                m.content_str = None;
                m.content_len = None;
                m.content_first = None;
                match p.peek() {
                    Some(b'"') => m.content_str = Some(p.string()?),
                    Some(b'[') if p.opts.content_first => {
                        let (len, first) = p.array_first()?;
                        m.content_len = Some(len);
                        m.content_first = first;
                    }
                    Some(b'[') => {
                        // length matters only in cache-timer mode
                        p.skip_value()?;
                    }
                    _ => p.skip_value()?,
                }
            } else {
                p.skip_value()?;
            }
            Ok(())
        })?;
        Ok(m)
    }

    /// An array's length and its first element (others skipped).
    fn array_first(&mut self) -> R<(usize, Option<JVal>)> {
        self.expect(b'[')?;
        self.ws();
        if self.peek() == Some(b']') {
            self.i += 1;
            return Ok((0, None));
        }
        let mut len = 0;
        let mut first = None;
        loop {
            if len == 0 {
                first = Some(self.jval(1)?);
            } else {
                self.skip_value()?;
            }
            len += 1;
            self.ws();
            match self.peek().ok_or(())? {
                b',' => {
                    self.i += 1;
                    self.ws();
                }
                b']' => {
                    self.i += 1;
                    return Ok((len, first));
                }
                _ => return Err(()),
            }
        }
    }

    fn top(&mut self) -> R<Line> {
        let mut line = Line::default();
        self.ws();
        if self.peek() != Some(b'{') {
            self.skip_value()?;
        } else {
            line.is_object = true;
            const SMALL: [&str; 7] =
                ["type", "subtype", "isSidechain", "isApiErrorMessage", "timestamp", "customTitle", "compactMetadata"];
            self.object(|p, k| {
                if let Some(idx) = SMALL.iter().position(|name| p.key_is(k, name)) {
                    let v = Some(p.jval(1)?);
                    match idx {
                        0 => line.kind = v,
                        1 => line.subtype = v,
                        2 => line.is_sidechain = v,
                        3 => line.is_api_error = v,
                        4 => line.timestamp = v,
                        5 => line.custom_title = v,
                        _ => line.compact_metadata = v,
                    }
                } else if p.key_is(k, "message") {
                    line.message = if p.peek() == Some(b'{') {
                        Some(p.message()?)
                    } else {
                        p.skip_value()?;
                        None
                    };
                } else {
                    p.skip_value()?;
                }
                Ok(())
            })?;
        }
        self.ws();
        if self.i != self.b.len() {
            return Err(());
        }
        Ok(line)
    }
}

struct KeyRef {
    s: usize,
    e: usize,
    decoded: Option<String>,
}

/// `JSON.parse(line)` reduced to what the analyses read; None when the line
/// is not valid JSON. Agent ids found anywhere are appended to `ids`.
pub fn parse_line(bytes: &[u8], opts: &Options, ids: &mut Vec<String>) -> Option<Line> {
    let mark = ids.len();
    let mut p = P { b: bytes, i: 0, opts, ids };
    match p.top() {
        Ok(l) => Some(l),
        Err(()) => {
            // a line that fails to parse contributes nothing
            ids.truncate(mark);
            None
        }
    }
}

/// Full `JSON.parse` into a `JVal` (small inputs: hook payloads, skills logs).
pub fn parse_value(bytes: &[u8]) -> Option<JVal> {
    let opts = Options { agent_ids: false, content_first: false };
    let mut ids = Vec::new();
    let mut p = P { b: bytes, i: 0, opts: &opts, ids: &mut ids };
    let v = p.jval(0).ok()?;
    p.ws();
    (p.i == bytes.len()).then_some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> (Option<Line>, Vec<String>) {
        let mut ids = Vec::new();
        let l = parse_line(s.as_bytes(), &Options { agent_ids: true, content_first: true }, &mut ids);
        (l, ids)
    }

    #[test]
    fn grammar() {
        for ok in [
            "{}",
            "[]",
            "null",
            "  {\"a\" : [1, -0.5e+3, true, false, null, \"x\\u00e9\\n\"]}\t",
            "\"\\ud800\"",
            "1e400",
            "{\"a\":{\"b\":[[[]]]}}",
        ] {
            assert!(parse(ok).0.is_some(), "{ok}");
        }
        for bad in [
            "",
            " ",
            "{",
            "{\"a\"}",
            "{\"a\":}",
            "[1,]",
            "{\"a\":1,}",
            "01",
            "1.",
            ".5",
            "-",
            "1e",
            "tru",
            "\"\\x\"",
            "\"a\u{1}\"",
            "{} x",
            "[1 2]",
            "{'a':1}",
            "\"\\u12\"",
            "NaN",
        ] {
            assert!(parse(bad).0.is_none(), "{bad:?}");
        }
    }

    #[test]
    fn fields() {
        let (l, ids) = parse(
            r#"{"type":"assistant","timestamp":"2026-01-01T00:00:00Z","agentId":"top","message":{"usage":{"input_tokens":5,"output_tokens":-1,"cache_read_input_tokens":"3"},"stop_reason":null,"content":[{"type":"text","text":"hi","x":{"agentId":"deep"}}]},"toolUseResult":{"agentId":"  "},"type":"user"}"#,
        );
        let l = l.unwrap();
        assert!(l.type_is("user"));
        let m = l.message.unwrap();
        assert_eq!(m.stop, Stop::Null);
        assert_eq!(m.usage_tokens(), Some(Usage { input: 5.0, output: 0.0, creation: 0.0, read: 0.0 }));
        assert_eq!(m.content_len, Some(1));
        assert_eq!(m.content_first.unwrap().get("text").and_then(JVal::as_str), Some("hi"));
        assert_eq!(ids, vec!["top".to_string(), "deep".to_string()]);
    }

    #[test]
    fn surrogates_and_lossy() {
        let (l, _) = parse("{\"customTitle\":\"a\\ud83d\\ude00b\\ud800c\"}");
        assert_eq!(l.unwrap().custom_title.unwrap(), JVal::Str("a\u{1F600}b\u{FFFD}c".into()));
        let mut bytes = b"{\"customTitle\":\"x".to_vec();
        bytes.push(0xff);
        bytes.extend_from_slice(b"y\"}");
        let mut ids = Vec::new();
        let l = parse_line(&bytes, &Options { agent_ids: false, content_first: false }, &mut ids).unwrap();
        assert_eq!(l.custom_title.unwrap(), JVal::Str("x\u{FFFD}y".into()));
    }

    #[test]
    fn deep_nesting() {
        let s = format!("{{\"x\":{}{}}}", "[".repeat(100_000), "]".repeat(100_000));
        assert!(parse(&s).0.is_some());
        let s = format!("{{\"x\":{}{}}}", "[".repeat(100_000), "]".repeat(99_999));
        assert!(parse(&s).0.is_none());
    }

    #[test]
    fn failed_line_keeps_no_ids() {
        let (l, ids) = parse(r#"{"agentId":"a","x":"#);
        assert!(l.is_none());
        assert!(ids.is_empty());
    }
}
