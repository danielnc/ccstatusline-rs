//! The chalk instances the editor's screens are styled with.
//!
//! Upstream's TUI has three chalks: Ink's own copy (used by `<Text>` and borders), gradient-string's copy (the
//! header gradient) and the app's (`chalk.level = settings.colorLevel`, used by the status line preview through
//! `crate::colors`). The first two take their level from supports-color at startup, so this module ports that
//! detection and chalk's string semantics with an explicit level.

/// Which bundled chalk: Ink and gradient-string carry chalk 5, the app itself chalk 6, whose supports-color
/// reads `FORCE_COLOR` differently.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flavor {
    Chalk5,
    Chalk6,
}

fn force_color_chalk6() -> Option<u8> {
    let v = std::env::var("FORCE_COLOR").ok()?;
    if v == "false" {
        return Some(0);
    }
    if v == "true" || v.is_empty() {
        return Some(1);
    }
    if !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()) {
        return Some(v.parse::<u64>().map_or(3, |n| n.min(3)) as u8);
    }
    None
}

/// supports-color's level for stdout, as chalk computes it at import time (no `--color` flags are ever passed).
pub fn supports_color_level(flavor: Flavor, stdout_is_tty: bool) -> u8 {
    let env = |k: &str| std::env::var(k).ok();
    let has = |k: &str| std::env::var_os(k).is_some();
    if flavor == Flavor::Chalk6 {
        match force_color_chalk6() {
            Some(0) => return 0,
            // A numeric FORCE_COLOR is the answer outright.
            Some(n)
                if std::env::var("FORCE_COLOR")
                    .is_ok_and(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit())) =>
            {
                return n
            }
            Some(n) => return detect(stdout_is_tty, Some(n as i64)),
            None => return detect(stdout_is_tty, None),
        }
    }
    let force = env("FORCE_COLOR").map(|v| {
        if v == "true" {
            1
        } else if v == "false" {
            0
        } else if v.is_empty() {
            1
        } else {
            // Math.min(parseInt(v), 3); NaN compares false, so Math.min returns NaN, treated as "no force".
            match crate::jsnum::parse_int(&v) {
                Some(n) => (n as i64).min(3),
                None => -1,
            }
        }
    });
    let force = match force {
        Some(-1) => None,
        f => f,
    };
    if force == Some(0) {
        return 0;
    }
    let _ = has;
    detect(stdout_is_tty, force)
}

/// The rest of `_supportsColor`, shared by both flavors.
fn detect(stdout_is_tty: bool, force: Option<i64>) -> u8 {
    let env = |k: &str| std::env::var(k).ok();
    let has = |k: &str| std::env::var_os(k).is_some();
    if has("TF_BUILD") && has("AGENT_NAME") {
        return 1;
    }
    if !stdout_is_tty && force.is_none() {
        return 0;
    }
    let min = force.unwrap_or(0).max(0) as u8;
    let term = env("TERM").unwrap_or_default();
    if term == "dumb" {
        return min;
    }
    if has("CI") {
        if ["GITHUB_ACTIONS", "GITEA_ACTIONS", "CIRCLECI"].iter().any(|k| has(k)) {
            return 3;
        }
        if ["TRAVIS", "APPVEYOR", "GITLAB_CI", "BUILDKITE", "DRONE"].iter().any(|k| has(k))
            || env("CI_NAME").as_deref() == Some("codeship")
        {
            return 1;
        }
        return min;
    }
    if let Some(v) = env("TEAMCITY_VERSION") {
        let re = regex::Regex::new(r"^(9\.(0*[1-9]\d*)\.|\d{2,}\.)").unwrap();
        return if re.is_match(&v) { 1 } else { 0 };
    }
    if env("COLORTERM").as_deref() == Some("truecolor") {
        return 3;
    }
    if matches!(term.as_str(), "xterm-kitty" | "xterm-ghostty" | "wezterm") {
        return 3;
    }
    if let Some(program) = env("TERM_PROGRAM") {
        let version = env("TERM_PROGRAM_VERSION")
            .and_then(|v| crate::jsnum::parse_int(v.split('.').next().unwrap_or("")))
            .map_or(-1, |n| n as i64);
        match program.as_str() {
            "iTerm.app" => return if version >= 3 { 3 } else { 2 },
            "Apple_Terminal" => return 2,
            _ => {}
        }
    }
    let lower = term.to_ascii_lowercase();
    if lower.ends_with("-256") || lower.ends_with("-256color") {
        return 2;
    }
    let basic = regex::Regex::new(r"(?i)^screen|^xterm|^vt100|^vt220|^rxvt|color|ansi|cygwin|linux").unwrap();
    if basic.is_match(&term) {
        return 1;
    }
    if has("COLORTERM") {
        return 1;
    }
    min
}

/// One chalk instance: a level and chalk's `applyStyle`.
#[derive(Clone, Copy, Debug)]
pub struct Chalk {
    pub level: u8,
}

/// A chalk style: its open and close codes.
#[derive(Clone, Debug)]
pub struct Style {
    pub open: String,
    pub close: String,
}

fn sgr(code: u32) -> String {
    format!("\x1b[{code}m")
}

impl Chalk {
    pub fn new(level: u8) -> Self {
        Chalk { level }
    }

    /// `applyStyle` for a single style (no parent chain: every call site here applies one style at a time).
    pub fn apply(&self, style: &Style, text: &str) -> String {
        if self.level == 0 || text.is_empty() {
            return text.to_string();
        }
        let mut s = text.to_string();
        if s.contains('\x1b') {
            s = string_replace_all(&s, &style.close, &style.open);
        }
        if let Some(lf) = s.find('\n') {
            s = encase_crlf(&s, &style.close, &style.open, lf);
        }
        format!("{}{}{}", style.open, s, style.close)
    }

    pub fn modifier(&self, name: &str) -> Option<Style> {
        let (open, close) = match name {
            "reset" => (0, 0),
            "bold" => (1, 22),
            "dim" => (2, 22),
            "italic" => (3, 23),
            "underline" => (4, 24),
            "overline" => (53, 55),
            "inverse" => (7, 27),
            "hidden" => (8, 28),
            "strikethrough" => (9, 29),
            _ => return None,
        };
        Some(Style { open: sgr(open), close: sgr(close) })
    }

    pub fn style_text(&self, name: &str, text: &str) -> String {
        match self.modifier(name) {
            Some(st) => self.apply(&st, text),
            None => text.to_string(),
        }
    }

    pub fn hex(&self, hex: &str, background: bool) -> Style {
        let (r, g, b) = hex_to_rgb(hex);
        self.rgb(r, g, b, background)
    }

    pub fn rgb(&self, r: u8, g: u8, b: u8, background: bool) -> Style {
        let open = match self.level {
            3 => format!("\x1b[{};2;{r};{g};{b}m", if background { 48 } else { 38 }),
            2 => format!("\x1b[{};5;{}m", if background { 48 } else { 38 }, rgb_to_ansi256(r, g, b)),
            _ => sgr(ansi256_to_ansi(rgb_to_ansi256(r, g, b)) + if background { 10 } else { 0 }),
        };
        Style { open, close: sgr(if background { 49 } else { 39 }) }
    }

    pub fn ansi256(&self, code: u32, background: bool) -> Style {
        let open = match self.level {
            2 | 3 => format!("\x1b[{};5;{code}m", if background { 48 } else { 38 }),
            _ => sgr(ansi256_to_ansi(code) + if background { 10 } else { 0 }),
        };
        Style { open, close: sgr(if background { 49 } else { 39 }) }
    }

    /// Ink's `colorize(str, color, type)`.
    pub fn colorize(&self, text: &str, color: &str, background: bool) -> String {
        if color.is_empty() {
            return text.to_string();
        }
        if let Some(st) = named_color(color, background) {
            return self.apply(&st, text);
        }
        if let Some(st) = self.modifier(color) {
            // `color in chalk` also matches modifiers (e.g. "bold"); chalk.bold / chalk.bgBold.
            return if background { text.to_string() } else { self.apply(&st, text) };
        }
        if color.starts_with('#') {
            return self.apply(&self.hex(color, background), text);
        }
        if color.starts_with("ansi256") {
            let re = regex::Regex::new(r"^ansi256\(\s?(\d+)\s?\)$").unwrap();
            return match re.captures(color) {
                Some(c) => self.apply(&self.ansi256(c[1].parse().unwrap_or(0), background), text),
                None => text.to_string(),
            };
        }
        if color.starts_with("rgb") {
            let re = regex::Regex::new(r"^rgb\(\s?(\d+),\s?(\d+),\s?(\d+)\s?\)$").unwrap();
            return match re.captures(color) {
                Some(c) => {
                    let n = |i: usize| c[i].parse::<u32>().unwrap_or(0).min(255) as u8;
                    self.apply(&self.rgb(n(1), n(2), n(3), background), text)
                }
                None => text.to_string(),
            };
        }
        text.to_string()
    }
}

/// The ansi-styles named colors (and their aliases) as foreground or background styles.
fn named_color(name: &str, background: bool) -> Option<Style> {
    let base = match name {
        "black" => 30,
        "red" => 31,
        "green" => 32,
        "yellow" => 33,
        "blue" => 34,
        "magenta" => 35,
        "cyan" => 36,
        "white" => 37,
        "blackBright" | "gray" | "grey" => 90,
        "redBright" => 91,
        "greenBright" => 92,
        "yellowBright" => 93,
        "blueBright" => 94,
        "magentaBright" => 95,
        "cyanBright" => 96,
        "whiteBright" => 97,
        _ => return None,
    };
    Some(if background {
        Style { open: sgr(base + 10), close: sgr(49) }
    } else {
        Style { open: sgr(base), close: sgr(39) }
    })
}

/// chalk's `stringReplaceAll`: every `substring` gets `replacer` appended after it.
fn string_replace_all(s: &str, substring: &str, replacer: &str) -> String {
    let Some(mut index) = s.find(substring) else {
        return s.to_string();
    };
    let mut end = 0;
    let mut out = String::new();
    loop {
        out.push_str(&s[end..index]);
        out.push_str(substring);
        out.push_str(replacer);
        end = index + substring.len();
        match s[end..].find(substring) {
            Some(i) => index = end + i,
            None => break,
        }
    }
    out.push_str(&s[end..]);
    out
}

fn encase_crlf(s: &str, prefix: &str, postfix: &str, mut index: usize) -> String {
    let mut end = 0;
    let mut out = String::new();
    loop {
        let got_cr = index > 0 && s.as_bytes()[index - 1] == b'\r';
        out.push_str(&s[end..if got_cr { index - 1 } else { index }]);
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

/// ansi-styles `hexToRgb`.
pub fn hex_to_rgb(hex: &str) -> (u8, u8, u8) {
    let re = regex::Regex::new(r"(?i)[a-f\d]{6}|[a-f\d]{3}").unwrap();
    let Some(m) = re.find(hex) else {
        return (0, 0, 0);
    };
    let mut s = m.as_str().to_string();
    if s.len() == 3 {
        s = s.chars().flat_map(|c| [c, c]).collect();
    }
    let v = u32::from_str_radix(&s, 16).unwrap_or(0);
    (((v >> 16) & 0xff) as u8, ((v >> 8) & 0xff) as u8, (v & 0xff) as u8)
}

/// ansi-styles `rgbToAnsi256`.
pub fn rgb_to_ansi256(r: u8, g: u8, b: u8) -> u32 {
    crate::gradient::rgb_to_ansi256(crate::gradient::Rgb { r, g, b })
}

pub use crate::colors::ansi256_to_ansi;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_close_codes_reopen() {
        let c = Chalk::new(2);
        let inner = c.style_text("dim", "x");
        let outer = c.colorize(&format!("{inner}y"), "green", false);
        assert_eq!(outer, "\x1b[32m\x1b[2mx\x1b[22my\x1b[39m");
        let bold = c.style_text("bold", &format!("a{inner}b"));
        assert_eq!(bold, "\x1b[1ma\x1b[2mx\x1b[22m\x1b[1mb\x1b[22m");
    }

    #[test]
    fn level_zero_is_plain() {
        assert_eq!(Chalk::new(0).colorize("x", "red", false), "x");
    }

    #[test]
    fn hex_levels() {
        assert_eq!(Chalk::new(2).hex("#3f51b1", false).open, "\x1b[38;5;67m");
        assert_eq!(Chalk::new(3).hex("#3f51b1", false).open, "\x1b[38;2;63;81;177m");
        assert_eq!(Chalk::new(1).hex("#3f51b1", false).open, "\x1b[34m");
    }
}
