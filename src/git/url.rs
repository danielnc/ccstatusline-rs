//! The part of the WHATWG URL parser (`new URL(input)`) that remote URL
//! parsing needs: http(s) (special) and ssh/git (non-special) URLs without a
//! base, giving protocol, host, hostname and pathname as node serializes them.

pub struct ParsedUrl {
    /// e.g. "https:"
    pub protocol: String,
    /// hostname plus ":port" when a non-default port is present
    pub host: String,
    pub hostname: String,
    pub pathname: String,
}

fn is_c0_control_or_space(c: char) -> bool {
    (c as u32) <= 0x20
}

fn is_forbidden_host(c: char) -> bool {
    matches!(
        c,
        '\0' | '\t' | '\n' | '\r' | ' ' | '#' | '/' | ':' | '<' | '>' | '?' | '@' | '[' | '\\' | ']' | '^' | '|'
    )
}

fn is_forbidden_domain(c: char) -> bool {
    is_forbidden_host(c) || (c as u32) <= 0x1f || c == '%' || c == '\u{7f}'
}

fn in_c0_set(c: char) -> bool {
    (c as u32) <= 0x1f || (c as u32) > 0x7e
}

fn in_path_set(c: char) -> bool {
    in_c0_set(c) || matches!(c, ' ' | '"' | '#' | '<' | '>' | '?' | '`' | '{' | '}')
}

fn percent_encode(s: &str, set: fn(char) -> bool, out: &mut String) {
    let mut buf = [0u8; 4];
    for c in s.chars() {
        if set(c) {
            for b in c.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        } else {
            out.push(c);
        }
    }
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

pub fn parse(input: &str) -> Option<ParsedUrl> {
    let trimmed = input.trim_matches(is_c0_control_or_space);
    let s: String = trimmed.chars().filter(|c| !matches!(c, '\t' | '\n' | '\r')).collect();
    // scheme
    let mut chars = s.char_indices();
    let (_, first) = chars.next()?;
    if !first.is_ascii_alphabetic() {
        return None;
    }
    let mut colon = None;
    for (i, c) in chars {
        if c == ':' {
            colon = Some(i);
            break;
        }
        if !(c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) {
            return None;
        }
    }
    let colon = colon?;
    let scheme = s[..colon].to_ascii_lowercase();
    let rest = &s[colon + 1..];
    let special = match scheme.as_str() {
        "http" | "https" => true,
        "ssh" | "git" => false,
        // Other schemes (valid or not) never yield a usable remote
        _ => return None,
    };
    let protocol = format!("{scheme}:");
    if special {
        let after = rest.trim_start_matches(['/', '\\']);
        let end = after.find(['/', '\\', '?', '#']).unwrap_or(after.len());
        let (hostname, port) = parse_authority(&after[..end], true, &scheme)?;
        let pathname = parse_path(&after[end..], true);
        let host = match port {
            Some(p) => format!("{hostname}:{p}"),
            None => hostname.clone(),
        };
        return Some(ParsedUrl { protocol, host, hostname, pathname });
    }
    if let Some(after) = rest.strip_prefix("//") {
        let end = after.find(['/', '?', '#']).unwrap_or(after.len());
        let (hostname, port) = parse_authority(&after[..end], false, &scheme)?;
        let tail = &after[end..];
        let pathname = if tail.starts_with('/') { parse_path(tail, false) } else { String::new() };
        let host = match port {
            Some(p) => format!("{hostname}:{p}"),
            None => hostname.clone(),
        };
        return Some(ParsedUrl { protocol, host, hostname, pathname });
    }
    if rest.starts_with('/') {
        return Some(ParsedUrl {
            protocol,
            host: String::new(),
            hostname: String::new(),
            pathname: parse_path(rest, false),
        });
    }
    // opaque path
    let end = rest.find(['?', '#']).unwrap_or(rest.len());
    let mut pathname = String::new();
    percent_encode(&rest[..end], in_c0_set, &mut pathname);
    Some(ParsedUrl { protocol, host: String::new(), hostname: String::new(), pathname })
}

/// Host and port of an authority (userinfo is dropped).
fn parse_authority(auth: &str, special: bool, scheme: &str) -> Option<(String, Option<String>)> {
    let host_port = match auth.rfind('@') {
        Some(i) => {
            let hp = &auth[i + 1..];
            if hp.is_empty() {
                return None;
            }
            hp
        }
        None => auth,
    };
    let mut inside = false;
    let mut split = None;
    for (i, c) in host_port.char_indices() {
        match c {
            '[' => inside = true,
            ']' => inside = false,
            ':' if !inside => {
                split = Some(i);
                break;
            }
            _ => {}
        }
    }
    let (host_raw, port_raw) = match split {
        Some(i) => (&host_port[..i], Some(&host_port[i + 1..])),
        None => (host_port, None),
    };
    if host_raw.is_empty() && (special || port_raw.is_some() || auth.contains('@')) {
        return None;
    }
    let host = if special { parse_special_host(host_raw)? } else { parse_opaque_host(host_raw)? };
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
            let v: u32 = if digits.is_empty() { 0 } else { digits.parse().ok()? };
            if v > 65535 {
                return None;
            }
            let default = match scheme {
                "http" => Some(80),
                "https" => Some(443),
                _ => None,
            };
            if default == Some(v) {
                None
            } else {
                Some(v.to_string())
            }
        }
    };
    Some((host, port))
}

fn parse_opaque_host(h: &str) -> Option<String> {
    if let Some(inner) = h.strip_prefix('[') {
        let inner = inner.strip_suffix(']')?;
        return Some(format!("[{}]", serialize_ipv6(&parse_ipv6(inner)?)));
    }
    if h.chars().any(is_forbidden_host) {
        return None;
    }
    let mut out = String::new();
    percent_encode(h, in_c0_set, &mut out);
    Some(out)
}

fn parse_special_host(h: &str) -> Option<String> {
    if let Some(inner) = h.strip_prefix('[') {
        let inner = inner.strip_suffix(']')?;
        return Some(format!("[{}]", serialize_ipv6(&parse_ipv6(inner)?)));
    }
    let decoded = String::from_utf8(percent_decode(h)).ok()?;
    let ascii = domain_to_ascii(&decoded)?;
    if ascii.is_empty() || ascii.chars().any(is_forbidden_domain) {
        return None;
    }
    if ends_in_number(&ascii) {
        return parse_ipv4(&ascii).map(|v| {
            let o = v.to_be_bytes();
            format!("{}.{}.{}.{}", o[0], o[1], o[2], o[3])
        });
    }
    Some(ascii)
}

/// UTS #46 ToASCII, approximated: lowercase and punycode non-ASCII labels.
fn domain_to_ascii(d: &str) -> Option<String> {
    if d.is_ascii() {
        return Some(d.to_ascii_lowercase());
    }
    let mapped: String =
        d.chars().map(|c| if matches!(c, '\u{3002}' | '\u{ff0e}' | '\u{ff61}') { '.' } else { c }).collect();
    let lower = mapped.to_lowercase();
    let mut labels = Vec::new();
    for label in lower.split('.') {
        if label.is_ascii() {
            labels.push(label.to_string());
        } else {
            labels.push(format!("xn--{}", punycode(label)?));
        }
    }
    Some(labels.join("."))
}

fn punycode(input: &str) -> Option<String> {
    const BASE: u32 = 36;
    let cps: Vec<u32> = input.chars().map(|c| c as u32).collect();
    let mut out: String = input.chars().filter(|c| c.is_ascii()).collect();
    let basic = out.len() as u32;
    let mut h = basic;
    if basic > 0 {
        out.push('-');
    }
    let (mut n, mut delta, mut bias) = (128u32, 0u32, 72u32);
    let digit = |d: u32| -> char {
        if d < 26 {
            (b'a' + d as u8) as char
        } else {
            (b'0' + (d - 26) as u8) as char
        }
    };
    let adapt = |mut delta: u32, num: u32, first: bool| -> u32 {
        delta = if first { delta / 700 } else { delta / 2 };
        delta += delta / num;
        let mut k = 0;
        while delta > ((BASE - 1) * 26) / 2 {
            delta /= BASE - 1;
            k += BASE;
        }
        k + (BASE * delta) / (delta + 38)
    };
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
                        1
                    } else if k >= bias + 26 {
                        26
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
                bias = adapt(delta, h + 1, h == basic);
                delta = 0;
                h += 1;
            }
        }
        delta += 1;
        n += 1;
    }
    Some(out)
}

fn ends_in_number(s: &str) -> bool {
    let mut parts: Vec<&str> = s.split('.').collect();
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
    // Saturate: anything this long is out of range anyway
    Some(u64::from_str_radix(digits, radix).unwrap_or(u64::MAX))
}

fn parse_ipv4(s: &str) -> Option<u32> {
    let mut parts: Vec<&str> = s.split('.').collect();
    if parts.last() == Some(&"") && parts.len() > 1 {
        parts.pop();
    }
    if parts.len() > 4 {
        return None;
    }
    let nums: Vec<u64> = parts.iter().map(|p| parse_ipv4_number(p)).collect::<Option<_>>()?;
    let (last, init) = nums.split_last()?;
    if init.iter().any(|&n| n > 255) {
        return None;
    }
    if *last >= 256u64.pow(5 - nums.len() as u32) {
        return None;
    }
    let mut v = *last;
    for (i, &n) in init.iter().enumerate() {
        v += n * 256u64.pow(3 - i as u32);
    }
    Some(v as u32)
}

fn parse_ipv6(s: &str) -> Option<[u16; 8]> {
    let c: Vec<char> = s.chars().collect();
    let mut addr = [0u16; 8];
    let mut piece = 0usize;
    let mut compress: Option<usize> = None;
    let mut p = 0usize;
    if c.first() == Some(&':') {
        if c.get(1) != Some(&':') {
            return None;
        }
        p += 2;
        piece += 1;
        compress = Some(piece);
    }
    while p < c.len() {
        if piece == 8 {
            return None;
        }
        if c[p] == ':' {
            if compress.is_some() {
                return None;
            }
            p += 1;
            piece += 1;
            compress = Some(piece);
            continue;
        }
        let mut value: u32 = 0;
        let mut length = 0;
        while length < 4 && p < c.len() && c[p].is_ascii_hexdigit() {
            value = value * 0x10 + c[p].to_digit(16).unwrap();
            p += 1;
            length += 1;
        }
        if p < c.len() && c[p] == '.' {
            if length == 0 {
                return None;
            }
            p -= length;
            if piece > 6 {
                return None;
            }
            let mut seen = 0;
            while p < c.len() {
                let mut v4: Option<u32> = None;
                if seen > 0 {
                    if c[p] == '.' && seen < 4 {
                        p += 1;
                    } else {
                        return None;
                    }
                }
                if p >= c.len() || !c[p].is_ascii_digit() {
                    return None;
                }
                while p < c.len() && c[p].is_ascii_digit() {
                    let n = c[p].to_digit(10).unwrap();
                    v4 = match v4 {
                        None => Some(n),
                        Some(0) => return None,
                        Some(x) => Some(x * 10 + n),
                    };
                    if v4.unwrap() > 255 {
                        return None;
                    }
                    p += 1;
                }
                addr[piece] = (addr[piece] as u32 * 0x100 + v4.unwrap()) as u16;
                seen += 1;
                if seen == 2 || seen == 4 {
                    piece += 1;
                }
            }
            if seen != 4 {
                return None;
            }
            break;
        } else if p < c.len() && c[p] == ':' {
            p += 1;
            if p >= c.len() {
                return None;
            }
        } else if p < c.len() {
            return None;
        }
        addr[piece] = value as u16;
        piece += 1;
    }
    if let Some(cp) = compress {
        let mut swaps = piece - cp;
        piece = 7;
        while piece != 0 && swaps > 0 {
            addr.swap(piece, cp + swaps - 1);
            piece -= 1;
            swaps -= 1;
        }
    } else if piece != 8 {
        return None;
    }
    Some(addr)
}

fn serialize_ipv6(a: &[u16; 8]) -> String {
    // longest run (> 1) of zero pieces is compressed
    let (mut best, mut best_len, mut i) = (None, 1, 0);
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

fn is_single_dot(s: &str) -> bool {
    s == "." || s.eq_ignore_ascii_case("%2e")
}

fn is_double_dot(s: &str) -> bool {
    matches!(s.to_ascii_lowercase().as_str(), ".." | ".%2e" | "%2e." | "%2e%2e")
}

/// Path state over `tail` (which starts at the first path delimiter, or is
/// empty), stopping at `?`/`#`.
fn parse_path(tail: &str, special: bool) -> String {
    let end = tail.find(['?', '#']).unwrap_or(tail.len());
    let path = &tail[..end];
    if path.is_empty() {
        return if special { "/".into() } else { String::new() };
    }
    let is_sep = |c: char| c == '/' || (special && c == '\\');
    // path start state consumes the leading separator
    let body = &path[path.chars().next().map(|c| if is_sep(c) { c.len_utf8() } else { 0 }).unwrap_or(0)..];
    let mut segs: Vec<String> = Vec::new();
    let pieces: Vec<&str> = body.split(is_sep).collect();
    let n = pieces.len();
    for (i, raw) in pieces.iter().enumerate() {
        let last = i == n - 1;
        if is_double_dot(raw) {
            segs.pop();
            if last {
                segs.push(String::new());
            }
        } else if is_single_dot(raw) {
            if last {
                segs.push(String::new());
            }
        } else {
            let mut enc = String::new();
            percent_encode(raw, in_path_set, &mut enc);
            segs.push(enc);
        }
    }
    let mut out = String::new();
    for s in segs {
        out.push('/');
        out.push_str(&s);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::parse;

    fn hp(u: &str) -> (String, String, String) {
        let p = parse(u).unwrap();
        (p.host, p.hostname, p.pathname)
    }

    #[test]
    fn basics() {
        assert_eq!(hp("https://GitHub.com/a/b.git"), ("github.com".into(), "github.com".into(), "/a/b.git".into()));
        assert_eq!(hp("https://github.com:443/a/b"), ("github.com".into(), "github.com".into(), "/a/b".into()));
        assert_eq!(hp("http://h:8080/a b/c"), ("h:8080".into(), "h".into(), "/a%20b/c".into()));
        assert_eq!(hp("ssh://git@Host.X:22/o/r"), ("Host.X:22".into(), "Host.X".into(), "/o/r".into()));
        assert_eq!(hp("git://h/o/../r"), ("h".into(), "h".into(), "/r".into()));
        assert_eq!(hp("https://0x7f.1/a"), ("127.0.0.1".into(), "127.0.0.1".into(), "/a".into()));
        assert_eq!(hp("https://[::1]:8/a"), ("[::1]:8".into(), "[::1]".into(), "/a".into()));
        assert_eq!(hp("https://\\\\h\\o\\r"), ("h".into(), "h".into(), "/o/r".into()));
        assert_eq!(hp("https://bücher.de/a/b").0, "xn--bcher-kva.de");
        assert_eq!(hp("https:///a").0, "a");
        assert!(parse("https://h:99999/a").is_none());
        assert!(parse("ssh://h:/a").is_some());
    }
}
