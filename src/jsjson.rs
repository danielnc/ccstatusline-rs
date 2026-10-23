//! `JSON.stringify` with JS number formatting, so files we write are
//! byte-identical to the ones upstream writes.

use serde_json::Value;

pub fn quote(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

pub fn number(n: &serde_json::Number) -> String {
    if let Some(u) = n.as_u64() {
        if u < (1u64 << 53) {
            return u.to_string();
        }
    }
    if let Some(i) = n.as_i64() {
        if i.unsigned_abs() < (1u64 << 53) {
            return i.to_string();
        }
    }
    let f = n.as_f64().unwrap_or(0.0);
    if f.is_finite() {
        crate::jsnum::to_js_string(f)
    } else {
        "null".into()
    }
}

fn write(v: &Value, indent: Option<&str>, depth: usize, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&number(n)),
        Value::String(s) => quote(s, out),
        Value::Array(a) => {
            if a.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (i, item) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(indent, depth + 1, out);
                write(item, indent, depth + 1, out);
            }
            newline(indent, depth, out);
            out.push(']');
        }
        Value::Object(m) => {
            if m.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push('{');
            for (i, (k, item)) in m.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(indent, depth + 1, out);
                quote(k, out);
                out.push(':');
                if indent.is_some() {
                    out.push(' ');
                }
                write(item, indent, depth + 1, out);
            }
            newline(indent, depth, out);
            out.push('}');
        }
    }
}

fn newline(indent: Option<&str>, depth: usize, out: &mut String) {
    if let Some(ind) = indent {
        out.push('\n');
        for _ in 0..depth {
            out.push_str(ind);
        }
    }
}

/// `JSON.stringify(v)`.
pub fn stringify(v: &Value) -> String {
    let mut s = String::new();
    write(v, None, 0, &mut s);
    s
}

/// `JSON.stringify(v, null, 2)`.
pub fn stringify_pretty(v: &Value) -> String {
    let mut s = String::new();
    write(v, Some("  "), 0, &mut s);
    s
}

/// Build a JSON number from an f64 the way JS would store it.
pub fn num(f: f64) -> Value {
    if crate::jsnum::is_integer(f) && f.abs() < 9007199254740992.0 {
        if f >= 0.0 {
            Value::from(f as u64)
        } else {
            Value::from(f as i64)
        }
    } else {
        serde_json::Number::from_f64(f).map(Value::Number).unwrap_or(Value::Null)
    }
}
