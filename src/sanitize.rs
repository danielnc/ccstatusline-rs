//! Port of upstream `src/utils/terminal-sanitize.ts`: keep only SGR colors and
//! safe OSC 8 hyperlinks; drop every other control character and sequence.

fn is_control(c: char) -> bool {
    let cp = c as u32;
    (cp <= 0x08) || (0x0b..=0x1f).contains(&cp) || (0x7f..=0x9f).contains(&cp)
}

fn match_sgr(b: &[u8], i: usize) -> Option<usize> {
    if b.get(i) != Some(&0x1b) || b.get(i + 1) != Some(&b'[') {
        return None;
    }
    let mut j = i + 2;
    while j < b.len() && (b[j].is_ascii_digit() || b[j] == b';' || b[j] == b':') {
        j += 1;
    }
    (b.get(j) == Some(&b'm')).then_some(j + 1)
}

fn match_osc8(text: &str, i: usize) -> Option<usize> {
    let rest = text.get(i..)?;
    let after = rest.strip_prefix("\x1b]8;")?;
    let params_len = after
        .bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'=' | b':' | b'_' | b'.' | b'-'))
        .count();
    let after_params = &after[params_len..];
    let url_part = after_params.strip_prefix(';')?;
    let mut url_len = 0;
    for c in url_part.chars() {
        let cp = c as u32;
        if cp <= 0x20 || (0x7f..=0x9f).contains(&cp) {
            break;
        }
        url_len += c.len_utf8();
    }
    let tail = &url_part[url_len..];
    let consumed_before_term = 4 + params_len + 1 + url_len;
    if tail.starts_with("\x1b\\") {
        Some(i + consumed_before_term + 2)
    } else if tail.starts_with('\x07') {
        Some(i + consumed_before_term + 1)
    } else {
        None
    }
}

fn skip_escape(text: &str, index: usize) -> usize {
    let b = text.as_bytes();
    let Some(intro) = text[index + 1..].chars().next() else { return index + 1 };
    if intro == '[' {
        let mut end = index + 2;
        while end < b.len() && (0x20..=0x3f).contains(&b[end]) {
            end += 1;
        }
        return if end < b.len() && (0x40..=0x7e).contains(&b[end]) { end + 1 } else { end };
    }
    if matches!(intro, ']' | 'P' | 'X' | '^' | '_') {
        let mut end = index + 2;
        while end < text.len() {
            let c = text[end..].chars().next().unwrap();
            if c == '\x07' || c == '\u{9c}' {
                return end + c.len_utf8();
            }
            if c == '\x1b' {
                return if b.get(end + 1) == Some(&b'\\') { end + 2 } else { end };
            }
            end += c.len_utf8();
        }
        return text.len();
    }
    let ic = intro as u32;
    if (0x20..=0x2f).contains(&ic) {
        let mut end = index + 2;
        while end < b.len() && (0x20..=0x2f).contains(&b[end]) {
            end += 1;
        }
        return if end < b.len() && (0x30..=0x7e).contains(&b[end]) { end + 1 } else { end };
    }
    // JS indexes UTF-16 units: ESC plus one unit; a non-ASCII introducer is one
    // char here (an astral one would leave a lone surrogate upstream, which the
    // output cannot carry anyway).
    index + 1 + intro.len_utf8()
}

pub fn sanitize_terminal_text(text: &str) -> String {
    if !text.chars().any(is_control) {
        return text.to_string();
    }
    let b = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        if b[i] == 0x1b {
            if let Some(end) = match_sgr(b, i).or_else(|| match_osc8(text, i)) {
                out.push_str(&text[i..end]);
                i = end;
            } else {
                i = skip_escape(text, i);
            }
            continue;
        }
        let end = text[i..].find('\x1b').map(|o| i + o).unwrap_or(text.len());
        out.extend(text[i..end].chars().filter(|c| !is_control(*c)));
        i = end;
    }
    out
}

pub fn is_safe_hyperlink_url(url: &str) -> bool {
    url.chars().all(|c| {
        let cp = c as u32;
        !(cp <= 0x20 || (0x7f..=0x9f).contains(&cp))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_sgr_and_links() {
        assert_eq!(sanitize_terminal_text("a\x1b[31mb\x1b[0m"), "a\x1b[31mb\x1b[0m");
        assert_eq!(sanitize_terminal_text("\x1b]0;title\x07x"), "x");
        assert_eq!(
            sanitize_terminal_text("\x1b]8;;http://a\x1b\\t\x1b]8;;\x1b\\"),
            "\x1b]8;;http://a\x1b\\t\x1b]8;;\x1b\\"
        );
        assert_eq!(sanitize_terminal_text("a\x07b\x1b[2Jc"), "abc");
        assert_eq!(sanitize_terminal_text("a\tb\nc"), "a\tb\nc");
    }
}
