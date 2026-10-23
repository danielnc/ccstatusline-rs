//! Port of upstream `src/utils/colors.ts` plus the slice of chalk it relies on.

use crate::gradient::{apply_gradient_to_text, is_gradient_spec, parse_gradient_spec, rgb_to_ansi256};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Ansi16,
    Ansi256,
    Truecolor,
}

impl Level {
    pub fn from_color_level(level: u8) -> Level {
        match level {
            0 | 1 => Level::Ansi16,
            3 => Level::Truecolor,
            _ => Level::Ansi256,
        }
    }
}

/// The process-wide chalk level (`chalk.level = settings.colorLevel`).
static CHALK_LEVEL: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(2);

pub fn set_chalk_level(level: u8) {
    CHALK_LEVEL.store(level, std::sync::atomic::Ordering::Relaxed);
}

pub fn chalk_level() -> u8 {
    CHALK_LEVEL.load(std::sync::atomic::Ordering::Relaxed)
}

/// The chalk level `COLOR_MAP`'s entries were built at. The render path rebuilds the map after setting
/// `chalk.level` (`updateColorMap()`), so it follows the chalk level; the editor never does, so its entries keep
/// the level chalk detected for the terminal at import time.
static COLOR_MAP_LEVEL: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(u8::MAX);

pub fn set_color_map_level(level: u8) {
    COLOR_MAP_LEVEL.store(level, std::sync::atomic::Ordering::Relaxed);
}

fn color_map_level() -> u8 {
    match COLOR_MAP_LEVEL.load(std::sync::atomic::Ordering::Relaxed) {
        u8::MAX => chalk_level(),
        l => l,
    }
}

/// `chalk.<style>(text)` for a single open/close pair.
pub fn chalk_wrap(open: &str, close: &str, text: &str) -> String {
    if chalk_level() == 0 || text.is_empty() {
        return text.to_string();
    }
    let mut s = text.to_string();
    if s.contains('\x1b') {
        s = s.replace(close, open);
    }
    if let Some(lf) = s.find('\n') {
        s = encase_crlf(&s, close, open, lf);
    }
    format!("{open}{s}{close}")
}

fn encase_crlf(s: &str, prefix: &str, postfix: &str, mut index: usize) -> String {
    let mut end = 0;
    let mut out = String::new();
    loop {
        let got_cr = index > 0 && s.as_bytes()[index - 1] == b'\r';
        let upto = if got_cr { index - 1 } else { index };
        out.push_str(&s[end..upto]);
        out.push_str(prefix);
        out.push_str(if got_cr { "\r\n" } else { "\n" });
        out.push_str(postfix);
        end = index + 1;
        match s[end..].find('\n') {
            Some(i) => index = end + i,
            None => break,
        }
    }
    out.push_str(&s[end..]);
    out
}

pub fn chalk_reset(text: &str) -> String {
    chalk_wrap("\x1b[0m", "\x1b[0m", text)
}

pub fn chalk_gray(text: &str) -> String {
    chalk_wrap("\x1b[90m", "\x1b[39m", text)
}

pub fn chalk_red(text: &str) -> String {
    chalk_wrap("\x1b[31m", "\x1b[39m", text)
}

struct Entry {
    name: &'static str,
    ansi16: u8,
    ansi256: u8,
    rgb: (u8, u8, u8),
    bg: bool,
}

const fn e(name: &'static str, ansi16: u8, ansi256: u8, rgb: (u8, u8, u8), bg: bool) -> Entry {
    Entry { name, ansi16, ansi256, rgb, bg }
}

static COLOR_MAP: &[Entry] = &[
    e("black", 30, 16, (0x00, 0x00, 0x00), false),
    e("red", 31, 160, (0xcc, 0x00, 0x00), false),
    e("green", 32, 70, (0x4e, 0x9a, 0x06), false),
    e("yellow", 33, 178, (0xc4, 0xa0, 0x00), false),
    e("blue", 34, 26, (0x34, 0x65, 0xa4), false),
    e("magenta", 35, 96, (0x75, 0x50, 0x7b), false),
    e("cyan", 36, 30, (0x06, 0x98, 0x9a), false),
    e("white", 37, 188, (0xd3, 0xd7, 0xcf), false),
    e("brightBlack", 90, 59, (0x55, 0x57, 0x53), false),
    e("brightRed", 91, 203, (0xef, 0x29, 0x29), false),
    e("brightGreen", 92, 155, (0x8a, 0xe2, 0x34), false),
    e("brightYellow", 93, 227, (0xfc, 0xe9, 0x4f), false),
    e("brightBlue", 94, 111, (0x72, 0x9f, 0xcf), false),
    e("brightMagenta", 95, 140, (0xad, 0x7f, 0xa8), false),
    e("brightCyan", 96, 80, (0x34, 0xe2, 0xe2), false),
    e("brightWhite", 97, 231, (0xee, 0xee, 0xec), false),
    e("bgBlack", 40, 16, (0x00, 0x00, 0x00), true),
    e("bgRed", 41, 160, (0xcc, 0x00, 0x00), true),
    e("bgGreen", 42, 70, (0x4e, 0x9a, 0x06), true),
    e("bgYellow", 43, 178, (0xc4, 0xa0, 0x00), true),
    e("bgBlue", 44, 26, (0x34, 0x65, 0xa4), true),
    e("bgMagenta", 45, 96, (0x75, 0x50, 0x7b), true),
    e("bgCyan", 46, 30, (0x06, 0x98, 0x9a), true),
    e("bgWhite", 47, 188, (0xd3, 0xd7, 0xcf), true),
    e("bgBrightBlack", 100, 59, (0x55, 0x57, 0x53), true),
    e("bgBrightRed", 101, 203, (0xef, 0x29, 0x29), true),
    e("bgBrightGreen", 102, 155, (0x8a, 0xe2, 0x34), true),
    e("bgBrightYellow", 103, 227, (0xfc, 0xe9, 0x4f), true),
    e("bgBrightBlue", 104, 111, (0x72, 0x9f, 0xcf), true),
    e("bgBrightMagenta", 105, 140, (0xad, 0x7f, 0xa8), true),
    e("bgBrightCyan", 106, 80, (0x34, 0xe2, 0xe2), true),
    e("bgBrightWhite", 107, 231, (0xee, 0xee, 0xec), true),
];

pub fn color_names() -> impl Iterator<Item = &'static str> {
    COLOR_MAP.iter().map(|e| e.name)
}

pub fn is_background_color_name(name: &str) -> bool {
    COLOR_MAP.iter().any(|e| e.name == name && e.bg)
}

pub fn bg_to_fg(name: Option<&str>) -> Option<String> {
    let name = name?;
    if name.is_empty() {
        return None;
    }
    if name.starts_with("ansi256:") || name.starts_with("hex:") {
        return Some(name.to_string());
    }
    if let Some(base) = name.strip_prefix("bgBright") {
        let mut chars = base.chars();
        let first = chars.next().map(|c| c.to_uppercase().collect::<String>()).unwrap_or_default();
        return Some(format!("bright{first}{}", chars.as_str().to_lowercase()));
    }
    if let Some(base) = name.strip_prefix("bg") {
        let mut chars = base.chars();
        let first = chars.next().map(|c| c.to_lowercase().collect::<String>()).unwrap_or_default();
        return Some(format!("{first}{}", chars.as_str()));
    }
    Some(name.to_string())
}

fn parse_hex6(hex: &str) -> Option<(u8, u8, u8)> {
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some((
        u8::from_str_radix(&hex[0..2], 16).ok()?,
        u8::from_str_radix(&hex[2..4], 16).ok()?,
        u8::from_str_radix(&hex[4..6], 16).ok()?,
    ))
}

fn ansi256_code(spec: &str) -> Option<u32> {
    let v = crate::jsnum::parse_int(spec)?;
    if (0.0..=255.0).contains(&v) {
        Some(v as u32)
    } else {
        None
    }
}

/// Raw opening SGR for a color, or "" (upstream `getColorAnsiCode`).
pub fn color_ansi_code(name: Option<&str>, level: Level, background: bool) -> String {
    let Some(name) = name else { return String::new() };
    if name.is_empty() {
        return String::new();
    }
    if is_gradient_spec(Some(name)) {
        let Some(stops) = parse_gradient_spec(Some(name)) else { return String::new() };
        let first = stops[0];
        return match level {
            Level::Ansi16 => String::new(),
            Level::Ansi256 => {
                let code = rgb_to_ansi256(first);
                if background {
                    format!("\x1b[48;5;{code}m")
                } else {
                    format!("\x1b[38;5;{code}m")
                }
            }
            Level::Truecolor => {
                if background {
                    format!("\x1b[48;2;{};{};{}m", first.r, first.g, first.b)
                } else {
                    format!("\x1b[38;2;{};{};{}m", first.r, first.g, first.b)
                }
            }
        };
    }
    if level == Level::Ansi16 && (name.starts_with("ansi256:") || name.starts_with("hex:")) {
        return String::new();
    }
    if let Some(spec) = name.strip_prefix("ansi256:") {
        return match ansi256_code(spec) {
            Some(code) if background => format!("\x1b[48;5;{code}m"),
            Some(code) => format!("\x1b[38;5;{code}m"),
            None => String::new(),
        };
    }
    if let Some(hex) = name.strip_prefix("hex:") {
        return match parse_hex6(hex) {
            Some((r, g, b)) if background => format!("\x1b[48;2;{r};{g};{b}m"),
            Some((r, g, b)) => format!("\x1b[38;2;{r};{g};{b}m"),
            None => String::new(),
        };
    }
    let Some(entry) = COLOR_MAP.iter().find(|e| e.name == name) else { return String::new() };
    if chalk_level() == 0 {
        return String::new();
    }
    // The entry's own kind decides fg vs bg (bgRed is a background even when
    // asked for as a foreground), exactly as chalk.bgRed('TEST') would.
    let bg = entry.bg;
    let map_level = color_map_level();
    let sixteen = |fg_code: u32| format!("\x1b[{}m", fg_code + if bg { 10 } else { 0 });
    match level {
        Level::Ansi16 => format!("\x1b[{}m", entry.ansi16),
        // `chalk.ansi256(n)` / `chalk.hex(...)` as built at the map's level.
        Level::Ansi256 if map_level >= 2 => format!("\x1b[{};5;{}m", if bg { 48 } else { 38 }, entry.ansi256),
        Level::Ansi256 => sixteen(ansi256_to_ansi(entry.ansi256 as u32)),
        Level::Truecolor => {
            let (r, g, b) = entry.rgb;
            match map_level {
                3 => format!("\x1b[{};2;{r};{g};{b}m", if bg { 48 } else { 38 }),
                2 => {
                    format!("\x1b[{};5;{}m", if bg { 48 } else { 38 }, rgb_to_ansi256(crate::gradient::Rgb { r, g, b }))
                }
                _ => sixteen(ansi256_to_ansi(rgb_to_ansi256(crate::gradient::Rgb { r, g, b }))),
            }
        }
    }
}

/// `applyParensDim`: dim each `(...)` span.
pub fn apply_parens_dim(text: &str, bold: bool) -> String {
    let reset = if bold { "\x1b[22;1m" } else { "\x1b[22m" };
    let mut out = String::with_capacity(text.len() + 16);
    let b = text.as_bytes();
    let mut i = 0;
    let mut last = 0;
    while i < b.len() {
        if b[i] == b'(' {
            // find the matching ')' with no '(' in between
            let mut j = i + 1;
            while j < b.len() && b[j] != b'(' && b[j] != b')' {
                j += 1;
            }
            if j < b.len() && b[j] == b')' {
                out.push_str(&text[last..i]);
                out.push_str("\x1b[2m");
                out.push_str(&text[i..=j]);
                out.push_str(reset);
                i = j + 1;
                last = i;
                continue;
            }
        }
        i += 1;
    }
    out.push_str(&text[last..]);
    out
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dim {
    No,
    Yes,
    Parens,
}

/// Upstream `applyColors`.
pub fn apply_colors(text: &str, fg: Option<&str>, bg: Option<&str>, bold: bool, level: Level, dim: Dim) -> String {
    let fg = fg.filter(|s| !s.is_empty());
    let bg = bg.filter(|s| !s.is_empty());
    let styled = if dim == Dim::Parens { apply_parens_dim(text, bold) } else { text.to_string() };
    if fg.is_none() && bg.is_none() && !bold && dim != Dim::Yes {
        return styled;
    }
    let mut prefix = String::new();
    let mut suffix = String::new();
    if bold {
        prefix.push_str("\x1b[1m");
    }
    if dim == Dim::Yes {
        prefix.push_str("\x1b[2m");
    }
    if bold || dim == Dim::Yes {
        suffix.insert_str(0, "\x1b[22m");
    }
    if let Some(bg) = bg {
        let code = color_ansi_code(Some(bg), level, true);
        if !code.is_empty() {
            prefix.push_str(&code);
            suffix.insert_str(0, "\x1b[49m");
        }
    }
    if let Some(fg) = fg {
        if let Some(stops) = parse_gradient_spec(Some(fg)) {
            if level != Level::Ansi16 {
                return format!("{prefix}{}\x1b[39m{suffix}", apply_gradient_to_text(&styled, &stops, level));
            }
        }
        let code = color_ansi_code(Some(fg), level, false);
        if !code.is_empty() {
            prefix.push_str(&code);
            suffix.insert_str(0, "\x1b[39m");
        }
    }
    format!("{prefix}{styled}{suffix}")
}

pub struct ThemeColors {
    pub fg: &'static [&'static str],
    pub bg: &'static [&'static str],
}

pub struct PowerlineTheme {
    pub name: &'static str,
    pub display: &'static str,
    pub description: &'static str,
    pub levels: Option<[ThemeColors; 3]>,
}

macro_rules! theme {
    ($key:literal, $name:literal, $desc:literal,
     [$($f1:literal),*], [$($b1:literal),*],
     [$($f2:literal),*], [$($b2:literal),*],
     [$($f3:literal),*], [$($b3:literal),*]) => {
        PowerlineTheme {
            name: $key, display: $name, description: $desc,
            levels: Some([
                ThemeColors { fg: &[$($f1),*], bg: &[$($b1),*] },
                ThemeColors { fg: &[$($f2),*], bg: &[$($b2),*] },
                ThemeColors { fg: &[$($f3),*], bg: &[$($b3),*] },
            ]),
        }
    };
}

pub static POWERLINE_THEMES: &[PowerlineTheme] = &[
    PowerlineTheme {
        name: "custom",
        display: "Custom",
        description: "Uses individual widget background colors",
        levels: None,
    },
    theme!(
        "nord",
        "Nord",
        "Arctic, north-bluish color palette",
        ["black", "brightWhite", "brightWhite", "black", "black"],
        ["bgBrightCyan", "bgBrightBlack", "bgBlue", "bgBrightYellow", "bgBrightGreen"],
        ["ansi256:16", "ansi256:254", "ansi256:231", "ansi256:231", "ansi256:16"],
        ["ansi256:73", "ansi256:239", "ansi256:25", "ansi256:96", "ansi256:152"],
        ["hex:2E3440", "hex:D8DEE9", "hex:FDF6E3", "hex:2E3440", "hex:2E3440"],
        ["hex:88C0D0", "hex:4C566A", "hex:5E81AC", "hex:B48EAD", "hex:A3BE8C"]
    ),
    theme!(
        "nord-aurora",
        "Nord Aurora",
        "Nord theme with aurora colors",
        ["brightWhite", "black", "black", "black", "black"],
        ["bgRed", "bgBrightYellow", "bgBrightBlue", "bgGreen", "bgBrightMagenta"],
        ["ansi256:231", "ansi256:16", "ansi256:231", "ansi256:16", "ansi256:16"],
        ["ansi256:131", "ansi256:220", "ansi256:68", "ansi256:108", "ansi256:176"],
        ["hex:ECEFF4", "hex:2E3440", "hex:FDF6E3", "hex:2E3440", "hex:2E3440"],
        ["hex:BF616A", "hex:EBCB8B", "hex:5E81AC", "hex:A3BE8C", "hex:B48EAD"]
    ),
    theme!(
        "monokai",
        "Monokai",
        "Dark background with vibrant colors",
        ["black", "brightWhite", "black", "white", "black"],
        ["bgBrightGreen", "bgBrightBlack", "bgBrightYellow", "bgMagenta", "bgBrightCyan"],
        ["ansi256:235", "ansi256:255", "ansi256:235", "ansi256:16", "ansi256:235"],
        ["ansi256:148", "ansi256:238", "ansi256:186", "ansi256:141", "ansi256:81"],
        ["hex:272822", "hex:F8F8F2", "hex:272822", "hex:272822", "hex:272822"],
        ["hex:A6E22E", "hex:49483E", "hex:E6DB74", "hex:AE81FF", "hex:66D9EF"]
    ),
    theme!(
        "solarized",
        "Solarized",
        "Precision colors for readability",
        ["brightWhite", "black", "brightWhite", "black", "black"],
        ["bgBlue", "bgBrightYellow", "bgBrightBlack", "bgCyan", "bgBrightWhite"],
        ["ansi256:231", "ansi256:234", "ansi256:254", "ansi256:16", "ansi256:234"],
        ["ansi256:33", "ansi256:136", "ansi256:240", "ansi256:37", "ansi256:254"],
        ["hex:073642", "hex:073642", "hex:FDF6E3", "hex:073642", "hex:073642"],
        ["hex:268BD2", "hex:B58900", "hex:586E75", "hex:2AA198", "hex:EEE8D5"]
    ),
    theme!(
        "minimal",
        "Minimal",
        "Clean monochrome theme",
        ["brightWhite", "black", "white", "black", "black"],
        ["bgBrightBlack", "bgBrightWhite", "bgBlack", "bgWhite", "bgBrightWhite"],
        ["ansi256:255", "ansi256:232", "ansi256:255", "ansi256:232", "ansi256:252"],
        ["ansi256:240", "ansi256:251", "ansi256:233", "ansi256:248", "ansi256:236"],
        ["hex:FFFFFF", "hex:1C1C1C", "hex:FFFFFF", "hex:1C1C1C", "hex:E4E4E4"],
        ["hex:585858", "hex:D0D0D0", "hex:1A1A1A", "hex:A8A8A8", "hex:303030"]
    ),
    theme!(
        "dracula",
        "Dracula",
        "Dark theme with purple accents",
        ["brightWhite", "black", "brightWhite", "black", "white"],
        ["bgMagenta", "bgBrightWhite", "bgRed", "bgBrightCyan", "bgBrightBlack"],
        ["ansi256:235", "ansi256:235", "ansi256:235", "ansi256:235", "ansi256:231"],
        ["ansi256:141", "ansi256:253", "ansi256:204", "ansi256:117", "ansi256:236"],
        ["hex:282A36", "hex:282A36", "hex:282A36", "hex:282A36", "hex:F8F8F2"],
        ["hex:BD93F9", "hex:F8F8F2", "hex:FF5555", "hex:8BE9FD", "hex:44475A"]
    ),
    theme!(
        "catppuccin",
        "Catppuccin",
        "Soothing pastel theme",
        ["black", "brightWhite", "black", "brightWhite", "black"],
        ["bgBrightMagenta", "bgBrightBlack", "bgBrightGreen", "bgBlue", "bgBrightYellow"],
        ["ansi256:235", "ansi256:255", "ansi256:235", "ansi256:235", "ansi256:235"],
        ["ansi256:176", "ansi256:238", "ansi256:150", "ansi256:210", "ansi256:111"],
        ["hex:1E1E2E", "hex:CDD6F4", "hex:1E1E2E", "hex:1E1E2E", "hex:CDD6F4"],
        ["hex:CBA6F7", "hex:45475A", "hex:A6E3A1", "hex:F38BA8", "hex:585B70"]
    ),
    theme!(
        "gruvbox",
        "Gruvbox",
        "Retro groove color scheme",
        ["brightWhite", "black", "black", "brightWhite", "black"],
        ["bgRed", "bgBrightYellow", "bgBrightWhite", "bgBlue", "bgBrightGreen"],
        ["ansi256:16", "ansi256:235", "ansi256:235", "ansi256:16", "ansi256:235"],
        ["ansi256:167", "ansi256:214", "ansi256:246", "ansi256:109", "ansi256:142"],
        ["hex:EBDBB2", "hex:282828", "hex:282828", "hex:FDF6E3", "hex:282828"],
        ["hex:CC241D", "hex:FABD2F", "hex:A89984", "hex:458588", "hex:98971A"]
    ),
    theme!(
        "onedark",
        "One Dark",
        "Atom-inspired dark theme",
        ["black", "brightWhite", "black", "brightWhite", "black"],
        ["bgBrightBlue", "bgBrightBlack", "bgBrightGreen", "bgRed", "bgBrightYellow"],
        ["ansi256:235", "ansi256:251", "ansi256:235", "ansi256:16", "ansi256:235"],
        ["ansi256:75", "ansi256:237", "ansi256:114", "ansi256:204", "ansi256:180"],
        ["hex:282C34", "hex:ABB2BF", "hex:282C34", "hex:282C34", "hex:282C34"],
        ["hex:61AFEF", "hex:3E4452", "hex:98C379", "hex:E06C75", "hex:E5C07B"]
    ),
    theme!(
        "tokyonight",
        "Tokyo Night",
        "Clean, dark theme inspired by Tokyo nightlife",
        ["brightWhite", "black", "brightWhite", "black", "black"],
        ["bgBlue", "bgBrightWhite", "bgMagenta", "bgBrightYellow", "bgBrightCyan"],
        ["ansi256:16", "ansi256:234", "ansi256:16", "ansi256:234", "ansi256:234"],
        ["ansi256:111", "ansi256:248", "ansi256:176", "ansi256:221", "ansi256:80"],
        ["hex:1A1B26", "hex:1A1B26", "hex:1A1B26", "hex:1A1B26", "hex:1A1B26"],
        ["hex:7AA2F7", "hex:D5D6DB", "hex:BB9AF7", "hex:E0AF68", "hex:7DCFFF"]
    ),
];

pub fn powerline_theme(name: &str) -> Option<&'static PowerlineTheme> {
    POWERLINE_THEMES.iter().find(|t| t.name == name)
}

/// ansi-styles `ansi256ToAnsi`: the 16-color foreground code nearest to an ansi256 code.
pub fn ansi256_to_ansi(code: u32) -> u32 {
    if code < 8 {
        return 30 + code;
    }
    if code < 16 {
        return 90 + (code - 8);
    }
    let (red, green, blue);
    if code >= 232 {
        red = (((code - 232) * 10 + 8) as f64) / 255.0;
        green = red;
        blue = red;
    } else {
        let c = code - 16;
        let remainder = c % 36;
        red = (c / 36) as f64 / 5.0;
        green = (remainder / 6) as f64 / 5.0;
        blue = (remainder % 6) as f64 / 5.0;
    }
    let value = red.max(green).max(blue) * 2.0;
    if value == 0.0 {
        return 30;
    }
    let round = |v: f64| crate::jsnum::js_round(v) as u32;
    let mut result = 30 + ((round(blue) << 2) | (round(green) << 1) | round(red));
    if value == 2.0 {
        result += 60;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes() {
        set_chalk_level(2);
        assert_eq!(color_ansi_code(Some("red"), Level::Ansi256, false), "\x1b[38;5;160m");
        assert_eq!(color_ansi_code(Some("bgRed"), Level::Ansi256, true), "\x1b[48;5;160m");
        assert_eq!(color_ansi_code(Some("hex:FF8800"), Level::Ansi256, false), "\x1b[38;2;255;136;0m");
        assert_eq!(bg_to_fg(Some("bgBrightRed")).as_deref(), Some("brightRed"));
        assert_eq!(apply_parens_dim("a (b) (c(d)) e", false), "a \x1b[2m(b)\x1b[22m (c\x1b[2m(d)\x1b[22m) e");
    }
}
