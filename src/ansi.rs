//! Port of upstream `src/utils/ansi.ts`: visible text/width, truncation, SGR
//! helpers and line gradients over styled strings.

use crate::gradient::{gradient_code_at, Rgb};
use crate::width::{cluster_width, consume_display_cluster, string_width, text_display_width};

const ESC: char = '\x1b';
const BEL: char = '\x07';
const C1_CSI: char = '\u{9b}';
const C1_OSC: char = '\u{9d}';
const ST: char = '\u{9c}';

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum OscTerm {
    Bel,
    St,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Osc8Action {
    Open,
    Close,
}

pub struct Escape {
    pub next: usize,
    pub start: usize,
    pub osc8: Option<(Osc8Action, OscTerm)>,
}

impl Escape {
    pub fn seq<'a>(&self, s: &'a str) -> &'a str {
        &s[self.start..self.next]
    }
}

fn char_at(s: &str, i: usize) -> Option<char> {
    s.get(i..).and_then(|r| r.chars().next())
}

fn parse_csi(input: &str, start: usize, body: usize) -> Escape {
    for (off, c) in input[body..].char_indices() {
        let cp = c as u32;
        if (0x40..=0x7e).contains(&cp) {
            return Escape { next: body + off + 1, start, osc8: None };
        }
    }
    Escape { next: input.len(), start, osc8: None }
}

fn osc8_action(body: &str) -> Option<Osc8Action> {
    let rest = body.strip_prefix("8;")?;
    let semi = rest.find(';')?;
    let url = &rest[semi + 1..];
    Some(if url.is_empty() { Osc8Action::Close } else { Osc8Action::Open })
}

fn parse_osc(input: &str, start: usize, body: usize) -> Escape {
    for (off, c) in input[body..].char_indices() {
        let idx = body + off;
        if c == BEL || c == ST {
            let term = if c == BEL { OscTerm::Bel } else { OscTerm::St };
            let action = osc8_action(&input[body..idx]);
            return Escape { next: idx + c.len_utf8(), start, osc8: action.map(|a| (a, term)) };
        }
        if c == ESC && input.as_bytes().get(idx + 1) == Some(&b'\\') {
            let action = osc8_action(&input[body..idx]);
            return Escape { next: idx + 2, start, osc8: action.map(|a| (a, OscTerm::St)) };
        }
    }
    Escape { next: input.len(), start, osc8: None }
}

pub fn parse_escape(input: &str, index: usize) -> Option<Escape> {
    let c = char_at(input, index)?;
    if c == ESC {
        return Some(match char_at(input, index + 1) {
            Some('[') => parse_csi(input, index, index + 2),
            Some(']') => parse_osc(input, index, index + 2),
            Some(n) => Escape { next: index + 1 + n.len_utf8(), start: index, osc8: None },
            None => Escape { next: input.len(), start: index, osc8: None },
        });
    }
    if c == C1_CSI {
        return Some(parse_csi(input, index, index + c.len_utf8()));
    }
    if c == C1_OSC {
        return Some(parse_osc(input, index, index + c.len_utf8()));
    }
    None
}

fn is_escape_introducer(c: char) -> bool {
    c == ESC || c == C1_CSI || c == C1_OSC
}

pub fn get_visible_text(text: &str) -> String {
    if !text.contains([ESC, C1_CSI, C1_OSC]) {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        if let Some(e) = parse_escape(text, i) {
            i = e.next;
            continue;
        }
        let c = char_at(text, i).unwrap();
        let mut end = i + c.len_utf8();
        while end < text.len() {
            let n = char_at(text, end).unwrap();
            if is_escape_introducer(n) {
                break;
            }
            end += n.len_utf8();
        }
        out.push_str(&text[i..end]);
        i = end;
    }
    out
}

pub fn get_visible_width(text: &str) -> usize {
    text_display_width(&get_visible_text(text))
}

fn is_sgr_at(text: &str, i: usize) -> Option<usize> {
    let b = text.as_bytes();
    if b.get(i) != Some(&0x1b) || b.get(i + 1) != Some(&b'[') {
        return None;
    }
    let mut j = i + 2;
    while j < b.len() && (b[j].is_ascii_digit() || b[j] == b';') {
        j += 1;
    }
    if b.get(j) == Some(&b'm') {
        Some(j + 1)
    } else {
        None
    }
}

/// Replace every SGR (`ESC [ [0-9;]* m`) via `f`.
fn replace_sgr(text: &str, mut f: impl FnMut(&str) -> String) -> String {
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut last = 0;
    let b = text.as_bytes();
    while i < b.len() {
        if b[i] == 0x1b {
            if let Some(end) = is_sgr_at(text, i) {
                out.push_str(&text[last..i]);
                out.push_str(&f(&text[i..end]));
                i = end;
                last = end;
                continue;
            }
        }
        i += 1;
    }
    out.push_str(&text[last..]);
    out
}

pub fn strip_sgr_codes(text: &str) -> String {
    replace_sgr(text, |_| String::new())
}

fn sgr_clears_background(seq: &str) -> bool {
    let inner = &seq[2..seq.len() - 1];
    let params: Vec<Option<i64>> = inner
        .split(';')
        .map(|p| crate::jsnum::parse_int(if p.is_empty() { "0" } else { p }).map(|v| v as i64))
        .collect();
    let mut clears = false;
    let mut i = 0;
    while i < params.len() {
        let p = params[i];
        if p == Some(0) || p == Some(49) {
            clears = true;
        } else if let Some(v) = p {
            if (40..=47).contains(&v) || (100..=107).contains(&v) || v == 48 {
                clears = false;
            }
        }
        if matches!(p, Some(38) | Some(48) | Some(58)) {
            i += if params.get(i + 1) == Some(&Some(5)) { 2 } else { 4 };
        }
        i += 1;
    }
    clears
}

pub fn restore_background_after_resets(text: &str, bg_code: &str) -> String {
    replace_sgr(text, |seq| if sgr_clears_background(seq) { format!("{seq}{bg_code}") } else { seq.to_string() })
}

pub fn strip_osc_codes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        if let Some(e) = parse_escape(text, i) {
            let s = e.seq(text);
            if !(s.starts_with("\x1b]") || s.starts_with(C1_OSC)) {
                out.push_str(s);
            }
            i = e.next;
            continue;
        }
        let c = char_at(text, i).unwrap();
        out.push(c);
        i += c.len_utf8();
    }
    out
}

fn next_osc8(e: &Escape, current: Option<OscTerm>) -> Option<OscTerm> {
    match e.osc8 {
        Some((Osc8Action::Open, t)) => Some(t),
        Some((Osc8Action::Close, _)) => None,
        None => current,
    }
}

pub fn truncate_styled_text(text: &str, max_width: usize, ellipsis: bool) -> String {
    if max_width == 0 {
        return String::new();
    }
    if get_visible_width(text) <= max_width {
        return text.to_string();
    }
    let ell = if ellipsis { "..." } else { "" };
    let ell_w = if ellipsis { string_width(ell) } else { 0 };
    if ellipsis && max_width <= ell_w {
        return ".".repeat(max_width);
    }
    let target = max_width.saturating_sub(ell_w);
    let mut out = String::new();
    let mut width = 0;
    let mut index = 0;
    let mut truncated = false;
    let mut open_osc8: Option<OscTerm> = None;
    let visible = get_visible_text(text);
    let mut vis_index = 0;

    while index < text.len() {
        if let Some(e) = parse_escape(text, index) {
            out.push_str(e.seq(text));
            index = e.next;
            open_osc8 = next_osc8(&e, open_osc8);
            continue;
        }
        let Some(cend) = consume_display_cluster(&visible, vis_index) else { break };
        let cluster = &visible[vis_index..cend];
        let cw = cluster_width(cluster);
        if width + cw > target {
            truncated = true;
            break;
        }
        // copy the cluster's characters, and any escapes inside it
        let mut remaining = cluster.chars().map(|c| c.len_utf16()).sum::<usize>() as isize;
        while remaining > 0 && index < text.len() {
            if let Some(e) = parse_escape(text, index) {
                out.push_str(e.seq(text));
                index = e.next;
                open_osc8 = next_osc8(&e, open_osc8);
                continue;
            }
            let c = char_at(text, index).unwrap();
            out.push(c);
            index += c.len_utf8();
            remaining -= c.len_utf16() as isize;
        }
        width += cw;
        vis_index = cend;
    }
    if !truncated {
        return text.to_string();
    }
    if let Some(t) = open_osc8 {
        out.push_str(match t {
            OscTerm::Bel => "\x1b]8;;\x07",
            OscTerm::St => "\x1b]8;;\x1b\\",
        });
    }
    out.push_str(ell);
    out
}

pub struct GradientSegment {
    pub text: String,
    pub next_column: usize,
}

pub fn apply_line_gradient_segment(
    text: &str,
    stops: &[Rgb],
    level: crate::colors::Level,
    start_column: usize,
    total_width: usize,
) -> GradientSegment {
    let visible_width = get_visible_width(text);
    if stops.is_empty() || level == crate::colors::Level::Ansi16 || total_width <= 1 {
        return GradientSegment { text: text.to_string(), next_column: start_column + visible_width };
    }
    let denom = (total_width - 1) as f64;
    let mut out = String::new();
    let mut column = start_column;
    let mut index = 0;
    while index < text.len() {
        if let Some(e) = parse_escape(text, index) {
            out.push_str(e.seq(text));
            index = e.next;
            continue;
        }
        let Some(end) = consume_display_cluster(text, index) else { break };
        let cluster = &text[index..end];
        out.push_str(&gradient_code_at(stops, column as f64 / denom, level));
        out.push_str(cluster);
        column += cluster_width(cluster);
        index = end;
    }
    GradientSegment { text: out, next_column: column }
}

pub fn apply_line_gradient(text: &str, stops: &[Rgb], level: crate::colors::Level) -> String {
    let total = get_visible_width(text);
    let r = apply_line_gradient_segment(text, stops, level, 0, total);
    if r.text == text {
        return text.to_string();
    }
    format!("{}\x1b[39m", r.text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible() {
        assert_eq!(get_visible_text("\x1b[31mab\x1b[39m"), "ab");
        assert_eq!(get_visible_width("\x1b]8;;http://x\x1b\\ab\x1b]8;;\x1b\\"), 2);
    }

    #[test]
    fn truncate() {
        assert_eq!(truncate_styled_text("\x1b[31mabcdef\x1b[39m", 5, true), "\x1b[31mab...");
        assert_eq!(truncate_styled_text("abc", 2, true), "..");
        assert_eq!(truncate_styled_text("abcdef", 3, false), "abc");
    }

    #[test]
    fn restore_bg() {
        assert_eq!(restore_background_after_resets("a\x1b[0mb", "\x1b[44m"), "a\x1b[0m\x1b[44mb");
        assert_eq!(restore_background_after_resets("a\x1b[38;5;0mb", "X"), "a\x1b[38;5;0mb");
    }
}
