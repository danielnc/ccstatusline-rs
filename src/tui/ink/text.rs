//! Text measurement and wrapping as Ink does it: string-width (through `crate::width`), widest-line,
//! wrap-ansi (`{trim: false, hard: true}`, the only options Ink passes), cli-truncate with its own slice-ansi, and
//! indent-string. Ported from the bundled copies in upstream's dist, quirks included.

use std::sync::OnceLock;

/// ansi-regex v6, as strip-ansi uses it.
fn ansi_regex() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        regex::Regex::new(
            "[\x1b\u{9b}][\\[\\]()#;?]*(?:(?:(?:(?:;[-a-zA-Z\\d/#&.:=?%@~_]+)*|[a-zA-Z\\d]+(?:;[-a-zA-Z\\d/#&.:=?%@~_]*)*)?(?:\x07|\x1b\\\\|\u{9c}))|(?:(?:\\d{1,4}(?:;\\d{0,4})*)?[\\dA-PR-TZcf-nq-uy=><~]))",
        )
        .unwrap()
    })
}

pub fn strip_ansi(s: &str) -> String {
    if !s.contains('\x1b') && !s.contains('\u{9b}') {
        return s.to_string();
    }
    ansi_regex().replace_all(s, "").into_owned()
}

/// string-width 7: `stringWidth(s)` with default options.
pub fn string_width(s: &str) -> usize {
    if s.is_empty() {
        return 0;
    }
    crate::width::string_width(&strip_ansi(s))
}

pub fn widest_line(s: &str) -> usize {
    s.split('\n').map(string_width).max().unwrap_or(0)
}

/// Ink's `measureText`: (width, height).
pub fn measure_text(s: &str) -> (usize, usize) {
    if s.is_empty() {
        return (0, 0);
    }
    (widest_line(s), s.split('\n').count())
}

/// indent-string without `includeEmptyLines`: lines that are empty or only whitespace stay unindented.
pub fn indent_string(s: &str, count: usize) -> String {
    if count == 0 {
        return s.to_string();
    }
    let indent = " ".repeat(count);
    s.split('\n')
        .map(|line| if crate::jsnum::js_trim(line).is_empty() { line.to_string() } else { format!("{indent}{line}") })
        .collect::<Vec<_>>()
        .join("\n")
}

/// ansi-styles `codes`: open code number to close code number.
pub fn ansi_close_code(code: u32) -> Option<u32> {
    Some(match code {
        0 => 0,
        1 | 2 => 22,
        3 => 23,
        4 => 24,
        53 => 55,
        7 => 27,
        8 => 28,
        9 => 29,
        30..=37 | 90..=97 => 39,
        40..=47 | 100..=107 => 49,
        _ => return None,
    })
}

const ESC: char = '\x1b';
const CSI_8BIT: char = '\u{9b}';
const LINK_PREFIX: &str = "]8;;";

fn is_escape(c: char) -> bool {
    c == ESC || c == CSI_8BIT
}

fn wrap_word(rows: &mut Vec<String>, word: &str, columns: f64) {
    let characters: Vec<char> = word.chars().collect();
    let mut inside_escape = false;
    let mut inside_link_escape = false;
    let mut visible = string_width(&strip_ansi(rows.last().unwrap()));
    for (index, &character) in characters.iter().enumerate() {
        let char_len = string_width(&character.to_string());
        if (visible + char_len) as f64 <= columns {
            rows.last_mut().unwrap().push(character);
        } else {
            rows.push(character.to_string());
            visible = 0;
        }
        if is_escape(character) {
            inside_escape = true;
            let candidate: String = characters.iter().skip(index + 1).take(LINK_PREFIX.chars().count()).collect();
            inside_link_escape = candidate == LINK_PREFIX;
        }
        if inside_escape {
            if inside_link_escape {
                if character == '\x07' {
                    inside_escape = false;
                    inside_link_escape = false;
                }
            } else if character == 'm' {
                inside_escape = false;
            }
            continue;
        }
        visible += char_len;
        if visible as f64 == columns && index < characters.len() - 1 {
            rows.push(String::new());
            visible = 0;
        }
    }
    if visible == 0 && !rows.last().unwrap().is_empty() && rows.len() > 1 {
        let last = rows.pop().unwrap();
        rows.last_mut().unwrap().push_str(&last);
    }
}

fn escape_scan_regex() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new("(?:\\[(?P<code>\\d+)m|\\]8;;(?P<uri>.*)\x07)").unwrap())
}

/// wrap-ansi's `exec` with `{trim: false, hard: true}` and word wrapping on.
fn wrap_line(string: &str, columns: f64) -> String {
    let words: Vec<&str> = string.split(' ').collect();
    let lengths: Vec<usize> = words.iter().map(|w| string_width(w)).collect();
    let mut rows: Vec<String> = vec![String::new()];
    for (index, word) in words.iter().enumerate() {
        let mut row_length = string_width(rows.last().unwrap());
        if index != 0 {
            // `trim === false`: a full row starts a new one, and the space is always kept.
            if row_length as f64 >= columns {
                rows.push(String::new());
                row_length = 0;
            }
            rows.last_mut().unwrap().push(' ');
            row_length += 1;
        }
        if lengths[index] as f64 > columns {
            let len = lengths[index] as f64;
            let remaining = columns - row_length as f64;
            let breaks_this_line = 1.0 + ((len - remaining - 1.0) / columns).floor();
            let breaks_next_line = ((len - 1.0) / columns).floor();
            if breaks_next_line < breaks_this_line {
                rows.push(String::new());
            }
            wrap_word(&mut rows, word, columns);
            continue;
        }
        if (row_length + lengths[index]) as f64 > columns && row_length > 0 && lengths[index] > 0 {
            rows.push(String::new());
        }
        rows.last_mut().unwrap().push_str(word);
    }

    let pre_string = rows.join("\n");
    let pre: Vec<char> = pre_string.chars().collect();
    let mut out = String::new();
    let mut escape_code: Option<f64> = None;
    let mut escape_url: Option<String> = None;
    let mut byte_index = 0;
    for (index, &character) in pre.iter().enumerate() {
        out.push(character);
        if is_escape(character) {
            if let Some(caps) = escape_scan_regex().captures(&pre_string[byte_index..]) {
                if let Some(code) = caps.name("code") {
                    let code: f64 = code.as_str().parse().unwrap_or(f64::NAN);
                    escape_code = if code == 39.0 { None } else { Some(code) };
                } else if let Some(uri) = caps.name("uri") {
                    escape_url = if uri.as_str().is_empty() { None } else { Some(uri.as_str().to_string()) };
                }
            }
        }
        let close = escape_code.and_then(|c| ansi_close_code(c as u32));
        let truthy_code = escape_code.filter(|c| *c != 0.0 && !c.is_nan());
        if pre.get(index + 1) == Some(&'\n') {
            if escape_url.is_some() {
                out.push_str("\x1b]8;;\x07");
            }
            if let (Some(_), Some(close)) = (truthy_code, close) {
                out.push_str(&format!("\x1b[{close}m"));
            }
        } else if character == '\n' {
            if let (Some(code), Some(_)) = (truthy_code, close) {
                out.push_str(&format!("\x1b[{}m", crate::jsnum::to_js_string(code)));
            }
            if let Some(url) = &escape_url {
                out.push_str(&format!("\x1b]8;;{url}\x07"));
            }
        }
        byte_index += character.len_utf8();
    }
    out
}

/// `wrapAnsi(text, columns, {trim: false, hard: true})`. `columns` can be fractional (yoga hands Ink fractional
/// widths), so it is compared as a number like JavaScript does.
pub fn wrap_ansi(text: &str, columns: f64) -> String {
    text.replace("\r\n", "\n").split('\n').map(|line| wrap_line(line, columns)).collect::<Vec<_>>().join("\n")
}

/// The old is-fullwidth-code-point table that cli-truncate's slice-ansi and ansi-tokenize use.
pub fn is_fullwidth_code_point(cp: u32) -> bool {
    cp >= 4352
        && (cp <= 4447
            || cp == 9001
            || cp == 9002
            || (11904..=12871).contains(&cp) && cp != 12351
            || (12880..=19903).contains(&cp)
            || (19968..=42182).contains(&cp)
            || (43360..=43388).contains(&cp)
            || (44032..=55203).contains(&cp)
            || (63744..=64255).contains(&cp)
            || (65040..=65049).contains(&cp)
            || (65072..=65131).contains(&cp)
            || (65281..=65376).contains(&cp)
            || (65504..=65510).contains(&cp)
            || (110592..=110593).contains(&cp)
            || (127488..=127569).contains(&cp)
            || (131072..=262141).contains(&cp))
}

fn check_ansi(codes: &[String], is_escapes: bool, end_code: Option<&str>) -> String {
    let mut output: Vec<String> = Vec::new();
    let mut codes: Vec<String> = codes.to_vec();
    let mut i = 0;
    while i < codes.len() {
        let origin = codes[i].clone();
        let mut code = origin.clone();
        if code.contains(';') {
            code = format!("{}0", code.split(';').next().unwrap().chars().next().unwrap_or('0'));
        }
        let item = crate::jsnum::parse_int(&code).and_then(|n| ansi_close_code(n as u32));
        match item {
            Some(item) => {
                let s = item.to_string();
                if let Some(pos) = codes.iter().position(|c| *c == s) {
                    codes.remove(pos);
                    if pos < i {
                        i -= 1;
                    }
                } else {
                    output.push(format!("\x1b[{}m", if is_escapes { s } else { origin }));
                }
            }
            None if is_escapes => {
                output.push("\x1b[0m".into());
                break;
            }
            None => output.push(format!("\x1b[{origin}m")),
        }
        i += 1;
    }
    if is_escapes {
        let mut dedup: Vec<String> = Vec::new();
        for o in output {
            if !dedup.contains(&o) {
                dedup.push(o);
            }
        }
        output = dedup;
        if let Some(end) = end_code {
            if let Some(close) = crate::jsnum::parse_int(end).and_then(|n| ansi_close_code(n as u32)) {
                let first = format!("\x1b[{close}m");
                let mut reordered: Vec<String> = Vec::new();
                for next in output {
                    if next == first {
                        reordered.insert(0, next);
                    } else {
                        reordered.push(next);
                    }
                }
                output = reordered;
            }
        }
    }
    output.concat()
}

/// cli-truncate's bundled slice-ansi (the older one, counting columns with `is_fullwidth_code_point`).
pub fn slice_ansi_legacy(string: &str, begin: usize, end: Option<usize>) -> String {
    let characters: Vec<char> = string.chars().collect();
    let mut ansi_codes: Vec<String> = Vec::new();
    let mut string_end = end.unwrap_or(characters.len());
    let mut inside_escape = false;
    let mut ansi_code: Option<String> = None;
    let mut visible = 0usize;
    let mut output = String::new();
    let code_re = regex::Regex::new(r"\d[^m]*").unwrap();
    // `string.slice(index, index + 18)` indexes UTF-16 units; the escape text it looks at is ASCII.
    let utf16: Vec<u16> = string.encode_utf16().collect();
    let mut unit = 0usize;
    for &character in &characters {
        let mut left_escape = false;
        if is_escape(character) {
            let window = String::from_utf16_lossy(&utf16[unit..(unit + 18).min(utf16.len())]);
            ansi_code = code_re.find(&window).map(|m| m.as_str().to_string());
            if visible < string_end {
                inside_escape = true;
                if let Some(c) = &ansi_code {
                    ansi_codes.push(c.clone());
                }
            }
        } else if inside_escape && character == 'm' {
            inside_escape = false;
            left_escape = true;
        }
        if !inside_escape && !left_escape {
            visible += 1;
        }
        let astral = (character as u32) > 0xffff;
        if !astral && is_fullwidth_code_point(character as u32) {
            visible += 1;
            if end.is_none() {
                string_end += 1;
            }
        }
        if visible > begin && visible <= string_end {
            output.push(character);
        } else if visible == begin && !inside_escape && ansi_code.is_some() {
            output = check_ansi(&ansi_codes, false, None);
        } else if visible >= string_end {
            output.push_str(&check_ansi(&ansi_codes, true, ansi_code.as_deref()));
            break;
        }
        unit += character.len_utf16();
    }
    output
}

/// cli-truncate with `position: 'end'` and the default `…`.
pub fn cli_truncate_end(text: &str, columns: f64) -> String {
    if columns < 1.0 {
        return String::new();
    }
    if columns == 1.0 {
        return "…".into();
    }
    let length = string_width(text);
    if (length as f64) <= columns {
        return text.to_string();
    }
    let cols = columns.floor() as usize;
    slice_ansi_legacy(text, 0, Some(cols - 1)) + "…"
}

/// Ink's `wrapText(text, maxWidth, wrapType)`.
pub fn wrap_text(text: &str, max_width: f64, wrap: TextWrap) -> String {
    match wrap {
        TextWrap::Wrap => wrap_ansi(text, max_width),
        TextWrap::Truncate => cli_truncate_end(text, max_width),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextWrap {
    #[default]
    Wrap,
    Truncate,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_on_spaces_and_hard_breaks_long_words() {
        assert_eq!(wrap_ansi("aaa bbb ccc", 7.0), "aaa bbb\n ccc");
        assert_eq!(wrap_ansi("abcdefghij", 4.0), "abcd\nefgh\nij");
    }

    #[test]
    fn wrapping_reopens_simple_codes() {
        // With `trim: false` the space after a full row gets a row of its own.
        assert_eq!(
            wrap_ansi("\x1b[2maaa bbb\x1b[22m", 3.0),
            "\x1b[2maaa\x1b[22m\n\x1b[2m \x1b[22m\n\x1b[2mbbb\x1b[22m"
        );
    }

    #[test]
    fn truncates_with_ellipsis() {
        assert_eq!(cli_truncate_end("hello world", 6.0), "hello…");
        assert_eq!(cli_truncate_end("hi", 6.0), "hi");
    }

    #[test]
    fn indents_non_blank_lines() {
        assert_eq!(indent_string("a\n\nb", 2), "  a\n\n  b");
    }
}
