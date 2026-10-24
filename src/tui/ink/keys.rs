//! Ink's `parse-keypress` (with upstream's patch: `\x7f` is backspace) and the `(input, key)` pair `useInput`
//! hands to handlers.

use std::sync::OnceLock;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Key {
    pub up_arrow: bool,
    pub down_arrow: bool,
    pub left_arrow: bool,
    pub right_arrow: bool,
    pub page_down: bool,
    pub page_up: bool,
    pub return_: bool,
    pub escape: bool,
    pub ctrl: bool,
    pub shift: bool,
    pub tab: bool,
    pub backspace: bool,
    pub delete: bool,
    pub meta: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Input {
    pub input: String,
    pub key: Key,
}

struct Parsed {
    name: String,
    ctrl: bool,
    meta: bool,
    shift: bool,
    option: bool,
    sequence: String,
}

fn key_name(code: &str) -> Option<&'static str> {
    Some(match code {
        "OP" | "[11~" | "[[A" => "f1",
        "OQ" | "[12~" | "[[B" => "f2",
        "OR" | "[13~" | "[[C" => "f3",
        "OS" | "[14~" | "[[D" => "f4",
        "[[E" | "[15~" => "f5",
        "[17~" => "f6",
        "[18~" => "f7",
        "[19~" => "f8",
        "[20~" => "f9",
        "[21~" => "f10",
        "[23~" => "f11",
        "[24~" => "f12",
        "[A" | "OA" | "[a" | "Oa" => "up",
        "[B" | "OB" | "[b" | "Ob" => "down",
        "[C" | "OC" | "[c" | "Oc" => "right",
        "[D" | "OD" | "[d" | "Od" => "left",
        "[E" | "OE" | "[e" | "Oe" => "clear",
        "[F" | "OF" | "[4~" | "[8~" | "[8$" | "[8^" => "end",
        "[H" | "OH" | "[1~" | "[7~" | "[7$" | "[7^" => "home",
        "[2~" | "[2$" | "[2^" => "insert",
        "[3~" | "[3$" | "[3^" => "delete",
        "[5~" | "[[5~" | "[5$" | "[5^" => "pageup",
        "[6~" | "[[6~" | "[6$" | "[6^" => "pagedown",
        "[Z" => "tab",
        _ => return None,
    })
}

const NON_ALPHANUMERIC: &[&str] = &[
    "f1",
    "f2",
    "f3",
    "f4",
    "f5",
    "f6",
    "f7",
    "f8",
    "f9",
    "f10",
    "f11",
    "f12",
    "up",
    "down",
    "right",
    "left",
    "clear",
    "end",
    "home",
    "insert",
    "delete",
    "pageup",
    "pagedown",
    "tab",
    "backspace",
];

fn is_shift_code(code: &str) -> bool {
    matches!(code, "[a" | "[b" | "[c" | "[d" | "[e" | "[2$" | "[3$" | "[5$" | "[6$" | "[7$" | "[8$" | "[Z")
}

fn is_ctrl_code(code: &str) -> bool {
    matches!(code, "Oa" | "Ob" | "Oc" | "Od" | "Oe" | "[2^" | "[3^" | "[5^" | "[6^" | "[7^" | "[8^")
}

fn fn_key_re() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        regex::Regex::new(r"^(?:\x1b+)(O|N|\[|\[\[)(?:(\d+)(?:;(\d+))?([~^$])|(?:1;)?(\d+)?([a-zA-Z]))").unwrap()
    })
}

fn parse_keypress(s: &str) -> Parsed {
    let mut k =
        Parsed { name: String::new(), ctrl: false, meta: false, shift: false, option: false, sequence: s.to_string() };
    let utf16_len = s.encode_utf16().count();
    let first = s.chars().next();
    if s == "\r" {
        k.name = "return".into();
    } else if s == "\n" {
        k.name = "enter".into();
    } else if s == "\t" {
        k.name = "tab".into();
    } else if s == "\x08" || s == "\x1b\x08" || s == "\x7f" || s == "\x1b\x7f" {
        k.name = "backspace".into();
        k.meta = first == Some('\x1b');
    } else if s == "\x1b" || s == "\x1b\x1b" {
        k.name = "escape".into();
        k.meta = utf16_len == 2;
    } else if s == " " || s == "\x1b " {
        k.name = "space".into();
        k.meta = utf16_len == 2;
    } else if utf16_len == 1 && first.is_some_and(|c| c <= '\x1a') {
        k.name = char::from_u32(first.unwrap() as u32 + 97 - 1).map(String::from).unwrap_or_default();
        k.ctrl = true;
    } else if utf16_len == 1 && first.is_some_and(|c| c.is_ascii_digit()) {
        k.name = "number".into();
    } else if utf16_len == 1 && first.is_some_and(|c| c.is_ascii_lowercase()) {
        k.name = s.to_string();
    } else if utf16_len == 1 && first.is_some_and(|c| c.is_ascii_uppercase()) {
        k.name = s.to_ascii_lowercase();
        k.shift = true;
    } else if s.len() == 2 && first == Some('\x1b') && s.as_bytes()[1].is_ascii_alphanumeric() {
        k.meta = true;
        k.shift = s.as_bytes()[1].is_ascii_uppercase();
    } else if let Some(c) = fn_key_re().captures(s) {
        if s.starts_with("\x1b\x1b") {
            k.option = true;
        }
        let code: String = [c.get(1), c.get(2), c.get(4), c.get(6)].iter().flatten().map(|m| m.as_str()).collect();
        let modifier = c.get(3).or(c.get(5)).map(|m| m.as_str().parse::<i64>().unwrap_or(0)).unwrap_or(1) - 1;
        k.ctrl = modifier & 4 != 0;
        k.meta = modifier & 10 != 0;
        k.shift = modifier & 1 != 0;
        k.name = key_name(&code).unwrap_or("").to_string();
        k.shift = is_shift_code(&code) || k.shift;
        k.ctrl = is_ctrl_code(&code) || k.ctrl;
    }
    k
}

/// What `useInput` passes to a handler for one stdin chunk.
pub fn to_input(chunk: &str) -> Input {
    let p = parse_keypress(chunk);
    let mut key = Key {
        up_arrow: p.name == "up",
        down_arrow: p.name == "down",
        left_arrow: p.name == "left",
        right_arrow: p.name == "right",
        page_down: p.name == "pagedown",
        page_up: p.name == "pageup",
        return_: p.name == "return",
        escape: p.name == "escape",
        ctrl: p.ctrl,
        shift: p.shift,
        tab: p.name == "tab",
        backspace: p.name == "backspace",
        delete: p.name == "delete",
        meta: p.meta || p.name == "escape" || p.option,
    };
    let mut input = if p.ctrl { p.name.clone() } else { p.sequence.clone() };
    if NON_ALPHANUMERIC.contains(&p.name.as_str()) {
        input.clear();
    }
    if let Some(rest) = input.strip_prefix('\x1b') {
        input = rest.to_string();
    }
    if input.encode_utf16().count() == 1 && input.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
        key.shift = true;
    }
    Input { input, key }
}

impl Input {
    /// Ink skips handlers for Ctrl+C when `exitOnCtrlC` is on (always, for this app).
    pub fn is_ctrl_c(&self) -> bool {
        self.input == "c" && self.key.ctrl
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrows_and_specials() {
        assert!(to_input("\x1b[A").key.up_arrow);
        assert_eq!(to_input("\x1b[A").input, "");
        assert!(to_input("\r").key.return_);
        let esc = to_input("\x1b");
        assert!(esc.key.escape && esc.key.meta && esc.input.is_empty());
        assert!(to_input("\x7f").key.backspace);
        assert!(to_input("\x1b[3~").key.delete);
    }

    #[test]
    fn ctrl_and_letters() {
        let s = to_input("\x13");
        assert!(s.key.ctrl);
        assert_eq!(s.input, "s");
        assert_eq!(to_input("a").input, "a");
        let upper = to_input("A");
        assert!(upper.key.shift);
        assert_eq!(upper.input, "A");
        assert_eq!(to_input(" ").input, " ");
    }
}
