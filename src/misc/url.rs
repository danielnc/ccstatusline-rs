//! The part of the WHATWG URL parser the link widget needs: whether
//! `new URL(input)` succeeds with an `http:`/`https:` protocol, and its `href`.
//!
//! Special-scheme rules only (credentials, host with IPv4/IPv6/domain parsing,
//! port, path dot-segments, percent-encode sets). Non-ASCII domains are
//! lowercased and Punycode-encoded per label; full UTS #46 mapping is not
//! implemented (exotic Unicode hosts may differ from Node).

/// `new URL(input)` for http(s) URLs: Some(href) when it parses and the
/// protocol is http: or https:.
pub fn parse_http_url(input: &str) -> Option<String> {
    // Strip leading/trailing C0 control or space; drop ASCII tab or newline.
    let trimmed = input.trim_matches(|c: char| c <= ' ');
    let s: String = trimmed.chars().filter(|c| !matches!(c, '\t' | '\n' | '\r')).collect();

    let colon = s.find(':')?;
    let scheme = &s[..colon];
    let sb = scheme.as_bytes();
    if sb.is_empty()
        || !sb[0].is_ascii_alphabetic()
        || !sb.iter().all(|b| b.is_ascii_alphanumeric() || b"+-.".contains(b))
    {
        return None;
    }
    let scheme = scheme.to_ascii_lowercase();
    let default_port = match scheme.as_str() {
        "http" => 80,
        "https" => 443,
        _ => return None,
    };

    // Special authority (ignore) slashes: any run of '/' or '\'.
    let rest = s[colon + 1..].trim_start_matches(['/', '\\']);
    let auth_end = rest.find(['/', '\\', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..auth_end];
    let tail = &rest[auth_end..];

    let (userinfo, hostport) = match authority.rfind('@') {
        Some(i) => (Some(&authority[..i]), &authority[i + 1..]),
        None => (None, authority),
    };
    let (mut username, mut password) = (String::new(), String::new());
    if let Some(ui) = userinfo {
        if hostport.is_empty() {
            return None;
        }
        let (u, p) = match ui.find(':') {
            Some(i) => (&ui[..i], Some(&ui[i + 1..])),
            None => (ui, None),
        };
        username = percent_encode(u, is_userinfo_set);
        if let Some(p) = p {
            password = percent_encode(p, is_userinfo_set);
        }
    }

    // Host ends at the first ':' outside brackets.
    let mut in_brackets = false;
    let mut split = None;
    for (i, c) in hostport.char_indices() {
        match c {
            '[' => in_brackets = true,
            ']' => in_brackets = false,
            ':' if !in_brackets => {
                split = Some(i);
                break;
            }
            _ => {}
        }
    }
    let (host_raw, port_raw) = match split {
        Some(i) => (&hostport[..i], Some(&hostport[i + 1..])),
        None => (hostport, None),
    };
    if host_raw.is_empty() {
        return None;
    }
    let host = parse_host(host_raw)?;
    let port = match port_raw {
        None | Some("") => None,
        Some(p) => {
            if !p.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let digits = p.trim_start_matches('0');
            if digits.len() > 5 {
                return None;
            }
            let n: u32 = if digits.is_empty() { 0 } else { digits.parse().ok()? };
            if n > 65535 {
                return None;
            }
            (n != default_port).then_some(n)
        }
    };

    let (path_part, query, fragment) = split_tail(tail);
    let path = serialize_path(path_part);

    let mut href = format!("{scheme}://");
    if !username.is_empty() || !password.is_empty() {
        href.push_str(&username);
        if !password.is_empty() {
            href.push(':');
            href.push_str(&password);
        }
        href.push('@');
    }
    href.push_str(&host);
    if let Some(p) = port {
        href.push_str(&format!(":{p}"));
    }
    href.push_str(&path);
    if let Some(q) = query {
        href.push('?');
        href.push_str(&percent_encode(q, is_special_query_set));
    }
    if let Some(f) = fragment {
        href.push('#');
        href.push_str(&percent_encode(f, is_fragment_set));
    }
    Some(href)
}

fn split_tail(tail: &str) -> (&str, Option<&str>, Option<&str>) {
    let (before_frag, fragment) = match tail.find('#') {
        Some(i) => (&tail[..i], Some(&tail[i + 1..])),
        None => (tail, None),
    };
    match before_frag.find('?') {
        Some(i) => (&before_frag[..i], Some(&before_frag[i + 1..]), fragment),
        None => (before_frag, None, fragment),
    }
}

fn is_single_dot(s: &str) -> bool {
    s == "." || s.eq_ignore_ascii_case("%2e")
}

fn is_double_dot(s: &str) -> bool {
    matches!(s.to_ascii_lowercase().as_str(), ".." | ".%2e" | "%2e." | "%2e%2e")
}

/// Path state for a special URL; `p` is empty or starts with '/' or '\'.
fn serialize_path(p: &str) -> String {
    let body = p.strip_prefix(['/', '\\']).unwrap_or(p);
    let mut segments: Vec<String> = Vec::new();
    let parts: Vec<&str> = body.split(['/', '\\']).collect();
    let last = parts.len() - 1;
    for (i, raw) in parts.iter().enumerate() {
        let buf = percent_encode(raw, is_path_set);
        let at_end = i == last;
        if is_double_dot(&buf) {
            segments.pop();
            if at_end {
                segments.push(String::new());
            }
        } else if is_single_dot(&buf) {
            if at_end {
                segments.push(String::new());
            }
        } else {
            segments.push(buf);
        }
    }
    segments.iter().map(|s| format!("/{s}")).collect()
}

fn is_c0_control_set(c: char) -> bool {
    (c as u32) < 0x20 || (c as u32) > 0x7e
}

fn is_fragment_set(c: char) -> bool {
    is_c0_control_set(c) || matches!(c, ' ' | '"' | '<' | '>' | '`')
}

fn is_query_set(c: char) -> bool {
    is_c0_control_set(c) || matches!(c, ' ' | '"' | '#' | '<' | '>')
}

fn is_special_query_set(c: char) -> bool {
    is_query_set(c) || c == '\''
}

fn is_path_set(c: char) -> bool {
    is_query_set(c) || matches!(c, '?' | '^' | '`' | '{' | '}')
}

fn is_userinfo_set(c: char) -> bool {
    is_path_set(c) || matches!(c, '/' | ':' | ';' | '=' | '@' | '[' | '\\' | ']' | '|')
}

fn percent_encode(s: &str, in_set: fn(char) -> bool) -> String {
    let mut out = String::with_capacity(s.len());
    let mut buf = [0u8; 4];
    for c in s.chars() {
        if in_set(c) {
            for b in c.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn percent_decode(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() && b[i + 1].is_ascii_hexdigit() && b[i + 2].is_ascii_hexdigit() {
            let hex = std::str::from_utf8(&b[i + 1..i + 3]).unwrap();
            out.push(u8::from_str_radix(hex, 16).unwrap());
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

fn parse_host(input: &str) -> Option<String> {
    if let Some(inner) = input.strip_prefix('[') {
        let inner = inner.strip_suffix(']')?;
        let pieces = parse_ipv6(inner)?;
        return Some(format!("[{}]", serialize_ipv6(&pieces)));
    }
    let decoded = String::from_utf8_lossy(&percent_decode(input)).into_owned();
    let ascii = domain_to_ascii(&decoded)?;
    if ascii.is_empty() || ascii.chars().any(is_forbidden_domain_code_point) {
        return None;
    }
    if ends_in_a_number(&ascii) {
        let v4 = parse_ipv4(&ascii)?;
        return Some(format!("{}.{}.{}.{}", v4 >> 24, (v4 >> 16) & 255, (v4 >> 8) & 255, v4 & 255));
    }
    Some(ascii)
}

fn is_forbidden_domain_code_point(c: char) -> bool {
    matches!(
        c,
        '\0' | '\t' | '\n' | '\r' | ' ' | '#' | '/' | ':' | '<' | '>' | '?' | '@' | '[' | '\\' | ']' | '^' | '|'
    ) || (c as u32) <= 0x1f
        || c == '%'
        || c == '\u{7f}'
}

fn domain_to_ascii(domain: &str) -> Option<String> {
    if domain.is_ascii() {
        return Some(domain.to_ascii_lowercase());
    }
    // Approximate UTS #46: ideographic full stops separate labels, labels are
    // lowercased, and non-ASCII labels are Punycode-encoded.
    let unified: String =
        domain.chars().map(|c| if matches!(c, '\u{3002}' | '\u{ff0e}' | '\u{ff61}') { '.' } else { c }).collect();
    let labels: Vec<String> = unified
        .split('.')
        .map(|l| {
            let lower = l.to_lowercase();
            if lower.is_ascii() {
                Some(lower)
            } else {
                punycode_encode(&lower).map(|p| format!("xn--{p}"))
            }
        })
        .collect::<Option<_>>()?;
    Some(labels.join("."))
}

/// RFC 3492 Punycode encoding.
fn punycode_encode(input: &str) -> Option<String> {
    const BASE: u32 = 36;
    const TMIN: u32 = 1;
    const TMAX: u32 = 26;
    const SKEW: u32 = 38;
    const DAMP: u32 = 700;
    let digit = |d: u32| -> char {
        if d < 26 {
            (b'a' + d as u8) as char
        } else {
            (b'0' + (d - 26) as u8) as char
        }
    };
    let adapt = |mut delta: u32, numpoints: u32, first: bool| -> u32 {
        delta = if first { delta / DAMP } else { delta / 2 };
        delta += delta / numpoints;
        let mut k = 0;
        while delta > ((BASE - TMIN) * TMAX) / 2 {
            delta /= BASE - TMIN;
            k += BASE;
        }
        k + (BASE - TMIN + 1) * delta / (delta + SKEW)
    };
    let cps: Vec<u32> = input.chars().map(|c| c as u32).collect();
    let mut out: String = input.chars().filter(|c| c.is_ascii()).collect();
    let b = out.len() as u32;
    let mut h = b;
    if b > 0 {
        out.push('-');
    }
    let (mut n, mut delta, mut bias) = (128u32, 0u32, 72u32);
    while (h as usize) < cps.len() {
        let m = *cps.iter().filter(|&&c| c >= n).min()?;
        delta = delta.checked_add((m - n).checked_mul(h + 1)?)?;
        n = m;
        for &c in &cps {
            if c < n {
                delta = delta.checked_add(1)?;
            }
            if c == n {
                let mut q = delta;
                let mut k = BASE;
                loop {
                    let t = if k <= bias {
                        TMIN
                    } else if k >= bias + TMAX {
                        TMAX
                    } else {
                        k - bias
                    };
                    if q < t {
                        break;
                    }
                    out.push(digit(t + (q - t) % (BASE - t)));
                    q = (q - t) / (BASE - t);
                    k += BASE;
                }
                out.push(digit(q));
                bias = adapt(delta, h + 1, h == b);
                delta = 0;
                h += 1;
            }
        }
        delta += 1;
        n += 1;
    }
    Some(out)
}

fn ends_in_a_number(host: &str) -> bool {
    let mut parts: Vec<&str> = host.split('.').collect();
    if parts.last() == Some(&"") {
        if parts.len() == 1 {
            return false;
        }
        parts.pop();
    }
    let last = parts.last().copied().unwrap_or("");
    if !last.is_empty() && last.bytes().all(|b| b.is_ascii_digit()) {
        return true;
    }
    parse_ipv4_number(last).is_some()
}

fn parse_ipv4_number(s: &str) -> Option<u64> {
    if s.is_empty() {
        return None;
    }
    let (digits, radix) = if s.len() >= 2 && (s.starts_with("0x") || s.starts_with("0X")) {
        (&s[2..], 16)
    } else if s.len() >= 2 && s.starts_with('0') {
        (&s[1..], 8)
    } else {
        (s, 10)
    };
    if digits.is_empty() {
        return Some(0);
    }
    if !digits.chars().all(|c| c.is_digit(radix)) {
        return None;
    }
    // Values beyond u64 are certainly out of range; saturate.
    Some(u64::from_str_radix(digits, radix).unwrap_or(u64::MAX))
}

fn parse_ipv4(host: &str) -> Option<u32> {
    let mut parts: Vec<&str> = host.split('.').collect();
    if parts.last() == Some(&"") && parts.len() > 1 {
        parts.pop();
    }
    if parts.len() > 4 {
        return None;
    }
    let nums: Vec<u64> = parts.iter().map(|p| parse_ipv4_number(p)).collect::<Option<_>>()?;
    let n = nums.len();
    if nums[..n - 1].iter().any(|&v| v > 255) {
        return None;
    }
    let last = nums[n - 1];
    if last >= 256u64.pow((5 - n) as u32) {
        return None;
    }
    let mut v = last;
    for (i, &x) in nums[..n - 1].iter().enumerate() {
        v += x * 256u64.pow((3 - i) as u32);
    }
    Some(v as u32)
}

/// WHATWG IPv6 parser.
fn parse_ipv6(input: &str) -> Option<[u16; 8]> {
    let c: Vec<char> = input.chars().collect();
    let mut addr = [0u16; 8];
    let mut piece = 0usize;
    let mut compress: Option<usize> = None;
    let mut p = 0usize;
    let at = |i: usize| c.get(i).copied();
    if at(p) == Some(':') {
        if at(p + 1) != Some(':') {
            return None;
        }
        p += 2;
        piece += 1;
        compress = Some(piece);
    }
    while at(p).is_some() {
        if piece == 8 {
            return None;
        }
        if at(p) == Some(':') {
            if compress.is_some() {
                return None;
            }
            p += 1;
            piece += 1;
            compress = Some(piece);
            continue;
        }
        let (mut value, mut length) = (0u32, 0);
        while length < 4 && at(p).is_some_and(|ch| ch.is_ascii_hexdigit()) {
            value = value * 16 + at(p).unwrap().to_digit(16).unwrap();
            p += 1;
            length += 1;
        }
        if at(p) == Some('.') {
            if length == 0 {
                return None;
            }
            p -= length;
            if piece > 6 {
                return None;
            }
            let mut numbers_seen = 0;
            while let Some(ch) = at(p) {
                let mut ipv4_piece: Option<u32> = None;
                if numbers_seen > 0 {
                    if ch == '.' && numbers_seen < 4 {
                        p += 1;
                    } else {
                        return None;
                    }
                }
                if !at(p).is_some_and(|d| d.is_ascii_digit()) {
                    return None;
                }
                while let Some(d) = at(p).and_then(|d| d.to_digit(10)) {
                    ipv4_piece = match ipv4_piece {
                        None => Some(d),
                        Some(0) => return None,
                        Some(v) => Some(v * 10 + d),
                    };
                    if ipv4_piece.unwrap() > 255 {
                        return None;
                    }
                    p += 1;
                }
                addr[piece] = (addr[piece] as u32 * 0x100 + ipv4_piece.unwrap()) as u16;
                numbers_seen += 1;
                if numbers_seen == 2 || numbers_seen == 4 {
                    piece += 1;
                }
            }
            if numbers_seen != 4 {
                return None;
            }
            break;
        } else if at(p) == Some(':') {
            p += 1;
            at(p)?;
        } else if at(p).is_some() {
            return None;
        }
        addr[piece] = value as u16;
        piece += 1;
    }
    if let Some(cmp) = compress {
        let mut swaps = piece - cmp;
        piece = 7;
        while piece != 0 && swaps > 0 {
            addr.swap(piece, cmp + swaps - 1);
            piece -= 1;
            swaps -= 1;
        }
    } else if piece != 8 {
        return None;
    }
    Some(addr)
}

fn serialize_ipv6(a: &[u16; 8]) -> String {
    // Longest run (length > 1) of zero pieces, first on ties.
    let (mut best, mut best_len) = (None, 1);
    let mut i = 0;
    while i < 8 {
        if a[i] == 0 {
            let start = i;
            while i < 8 && a[i] == 0 {
                i += 1;
            }
            if i - start > best_len {
                best = Some(start);
                best_len = i - start;
            }
        } else {
            i += 1;
        }
    }
    let mut out = String::new();
    let mut ignore0 = false;
    for (idx, piece) in a.iter().enumerate() {
        if ignore0 && *piece == 0 {
            continue;
        }
        ignore0 = false;
        if best == Some(idx) {
            out.push_str(if idx == 0 { "::" } else { ":" });
            ignore0 = true;
            continue;
        }
        out.push_str(&format!("{piece:x}"));
        if idx != 7 {
            out.push(':');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::parse_http_url as p;

    #[test]
    fn validity() {
        assert_eq!(p("https://example.com").as_deref(), Some("https://example.com/"));
        assert_eq!(
            p("HTTPS://Example.COM:443/a/./b/../c?x y#z w").as_deref(),
            Some("https://example.com/a/c?x%20y#z%20w")
        );
        assert_eq!(p("http:example.com").as_deref(), Some("http://example.com/"));
        assert_eq!(p("ftp://example.com"), None);
        assert_eq!(p("https://"), None);
        assert_eq!(p("example.com"), None);
        assert_eq!(p("http://exa mple.com"), None);
        assert_eq!(p("http://x:99999"), None);
        assert_eq!(p("http://x:abc"), None);
        assert_eq!(p("http://u:p@"), None);
        assert_eq!(p("http://999.1.1.1"), None);
        assert_eq!(p("http://0x7f.1/").as_deref(), Some("http://127.0.0.1/"));
        assert_eq!(p("http://[::1]:8080/x").as_deref(), Some("http://[::1]:8080/x"));
        assert_eq!(p("http://[1:0:0:0:0:0:0:1]").as_deref(), Some("http://[1::1]/"));
        assert_eq!(p("http://[::ffff:1.2.3.4]").as_deref(), Some("http://[::ffff:102:304]/"));
        assert_eq!(p("http://a b@host/my page").as_deref(), Some("http://a%20b@host/my%20page"));
        assert_eq!(p("http://bücher.de/").as_deref(), Some("http://xn--bcher-kva.de/"));
    }
}
