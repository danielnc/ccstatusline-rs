//! JavaScript number semantics: `String(n)`, `n.toFixed(d)`, `Math.round`,
//! `parseInt`, `parseFloat` and `Number(str)`, so formatted output matches
//! upstream byte for byte.

/// Decimal digits and exponent of the shortest round-trip representation:
/// value = 0.d1d2d3... × 10^point (digits has no leading/trailing zeros).
fn shortest_digits(x: f64) -> (String, i32) {
    let mut buf = ryu::Buffer::new();
    let s = buf.format_finite(x.abs());
    let (mantissa, exp) = match s.find(['e', 'E']) {
        Some(i) => (&s[..i], s[i + 1..].parse::<i32>().unwrap_or(0)),
        None => (s, 0),
    };
    let (int_part, frac_part) = match mantissa.find('.') {
        Some(i) => (&mantissa[..i], &mantissa[i + 1..]),
        None => (mantissa, ""),
    };
    let mut digits = format!("{int_part}{frac_part}");
    let mut point = int_part.len() as i32 + exp;
    // strip leading zeros
    let lead = digits.len() - digits.trim_start_matches('0').len();
    digits.drain(..lead);
    point -= lead as i32;
    while digits.ends_with('0') {
        digits.pop();
    }
    if digits.is_empty() {
        return ("0".to_string(), 1);
    }
    (digits, point)
}

/// `String(x)` for a JS number.
pub fn to_js_string(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x == 0.0 {
        return "0".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    let sign = if x < 0.0 { "-" } else { "" };
    let (digits, n) = shortest_digits(x);
    let k = digits.len() as i32;
    let body = if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let e = n - 1;
        let es = if e >= 0 { format!("+{e}") } else { format!("{e}") };
        if k == 1 {
            format!("{digits}e{es}")
        } else {
            format!("{}.{}e{es}", &digits[..1], &digits[1..])
        }
    };
    format!("{sign}{body}")
}

/// `x.toFixed(digits)` (digits 0..=100).
pub fn to_fixed(x: f64, digits: usize) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.abs() >= 1e21 || x.is_infinite() {
        return to_js_string(x);
    }
    let negative = x < 0.0;
    let ax = x.abs();
    // Exact decimal expansion well past the requested digits, then round half up
    // on that expansion, which is what the spec's "pick the larger n" means.
    let exact = format!("{:.*}", digits + 30, ax);
    let (int_part, frac_part) = exact.split_once('.').unwrap_or((&exact, ""));
    let mut kept: Vec<u8> = int_part.bytes().chain(frac_part.bytes().take(digits)).collect();
    let next = frac_part.as_bytes().get(digits).copied().unwrap_or(b'0');
    if next >= b'5' {
        let mut i = kept.len();
        loop {
            if i == 0 {
                kept.insert(0, b'1');
                break;
            }
            i -= 1;
            if kept[i] == b'9' {
                kept[i] = b'0';
            } else {
                kept[i] += 1;
                break;
            }
        }
    }
    let int_len = kept.len() - digits;
    let mut out = String::with_capacity(kept.len() + 2);
    // -0.00 keeps its sign only when x itself is negative and nonzero
    if negative && x != 0.0 {
        out.push('-');
    }
    out.push_str(std::str::from_utf8(&kept[..int_len]).unwrap());
    if digits > 0 {
        out.push('.');
        out.push_str(std::str::from_utf8(&kept[int_len..]).unwrap());
    }
    out
}

/// `Math.round(x)`: rounds half toward +Infinity.
pub fn js_round(x: f64) -> f64 {
    if !x.is_finite() || x == 0.0 {
        return x;
    }
    let f = x.floor();
    let r = if x - f >= 0.5 { f + 1.0 } else { f };
    if r == 0.0 && x < 0.0 {
        -0.0
    } else {
        r
    }
}

fn is_js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\u{9}' | '\u{a}' | '\u{b}' | '\u{c}' | '\u{d}' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'
    )
}

pub fn js_trim(s: &str) -> &str {
    s.trim_matches(is_js_whitespace)
}

/// `Number.parseInt(s, 10)`; None means NaN.
pub fn parse_int(s: &str) -> Option<f64> {
    let t = s.trim_start_matches(is_js_whitespace);
    let (neg, rest) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    let v: f64 = digits.parse().ok()?;
    Some(if neg { -v } else { v })
}

/// `Number.parseFloat(s)`; None means NaN.
pub fn parse_float(s: &str) -> Option<f64> {
    let t = s.trim_start_matches(is_js_whitespace);
    let b = t.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    if t[i..].starts_with("Infinity") {
        return Some(if b.first() == Some(&b'-') { f64::NEG_INFINITY } else { f64::INFINITY });
    }
    let start_digits = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let mut had_digits = i > start_digits;
    if i < b.len() && b[i] == b'.' {
        let j = i + 1;
        let mut k = j;
        while k < b.len() && b[k].is_ascii_digit() {
            k += 1;
        }
        if k > j || had_digits {
            had_digits = had_digits || k > j;
            i = k;
        }
    }
    if !had_digits {
        return None;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut k = i + 1;
        if k < b.len() && (b[k] == b'+' || b[k] == b'-') {
            k += 1;
        }
        let ds = k;
        while k < b.len() && b[k].is_ascii_digit() {
            k += 1;
        }
        if k > ds {
            i = k;
        }
    }
    let lit = &t[..i];
    let lit = lit.strip_suffix('.').unwrap_or(lit);
    lit.parse::<f64>().ok()
}

/// `Number(s)` for a string; None means NaN.
pub fn number_from_str(s: &str) -> Option<f64> {
    let t = js_trim(s);
    if t.is_empty() {
        return Some(0.0);
    }
    let (sign, body) = match t.as_bytes()[0] {
        b'-' => (-1.0, &t[1..]),
        b'+' => (1.0, &t[1..]),
        _ => (1.0, t),
    };
    if body == "Infinity" {
        return Some(sign * f64::INFINITY);
    }
    if sign == 1.0 && t.as_bytes()[0] != b'+' {
        let lower = body.to_ascii_lowercase();
        for (prefix, radix) in [("0x", 16), ("0o", 8), ("0b", 2)] {
            if let Some(rest) = lower.strip_prefix(prefix) {
                if rest.is_empty() || !rest.chars().all(|c| c.is_digit(radix)) {
                    return None;
                }
                let mut v = 0f64;
                for c in rest.chars() {
                    v = v * radix as f64 + c.to_digit(radix).unwrap() as f64;
                }
                return Some(v);
            }
        }
    }
    // StrDecimalLiteral: digits [. digits] [e[+-]digits] | . digits [...]
    let b = body.as_bytes();
    let mut i = 0;
    let mut nd = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
        nd += 1;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
            nd += 1;
        }
    }
    if nd == 0 {
        return None;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let ds = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == ds {
            return None;
        }
    }
    if i != b.len() {
        return None;
    }
    let lit = body.strip_suffix('.').unwrap_or(body);
    let lit = if lit.starts_with('.') { format!("0{lit}") } else { lit.to_string() };
    lit.replace(".e", "e").replace(".E", "E").parse::<f64>().ok().map(|v| sign * v)
}

/// `Number.isInteger`-style check used by zod's `.int()`.
pub fn is_integer(x: f64) -> bool {
    x.is_finite() && x.trunc() == x
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_string() {
        assert_eq!(to_js_string(1.0), "1");
        assert_eq!(to_js_string(-1.5), "-1.5");
        assert_eq!(to_js_string(0.1 + 0.2), "0.30000000000000004");
        assert_eq!(to_js_string(1e21), "1e+21");
        assert_eq!(to_js_string(123456789012345680000.0), "123456789012345680000");
        assert_eq!(to_js_string(1e-7), "1e-7");
        assert_eq!(to_js_string(0.000001), "0.000001");
        assert_eq!(to_js_string(1.5e-7), "1.5e-7");
        assert_eq!(to_js_string(100.0), "100");
    }

    #[test]
    fn fixed() {
        assert_eq!(to_fixed(2.5, 0), "3");
        assert_eq!(to_fixed(1.005, 2), "1.00");
        assert_eq!(to_fixed(1.45, 1), "1.4");
        assert_eq!(to_fixed(0.125, 2), "0.13");
        assert_eq!(to_fixed(-0.0, 2), "0.00");
        assert_eq!(to_fixed(-0.001, 2), "-0.00");
        assert_eq!(to_fixed(999.96, 1), "1000.0");
        assert_eq!(to_fixed(12.0, 1), "12.0");
    }

    #[test]
    fn rounding() {
        assert_eq!(js_round(2.5), 3.0);
        assert_eq!(js_round(-2.5), -2.0);
        assert_eq!(js_round(0.49999999999999994), 0.0);
    }

    #[test]
    fn parsing() {
        assert_eq!(parse_int(" 42px"), Some(42.0));
        assert_eq!(parse_int("x"), None);
        assert_eq!(parse_float("3.5e2x"), Some(350.0));
        assert_eq!(parse_float(".5"), Some(0.5));
        assert_eq!(number_from_str(" 12 "), Some(12.0));
        assert_eq!(number_from_str("12px"), None);
        assert_eq!(number_from_str(""), Some(0.0));
        assert_eq!(number_from_str("0x1f"), Some(31.0));
        assert_eq!(number_from_str("5."), Some(5.0));
    }
}
