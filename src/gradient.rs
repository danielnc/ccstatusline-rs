//! Port of upstream `src/utils/gradient.ts`.

use crate::colors::Level;
use crate::jsnum::js_round;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

const PREFIX: &str = "gradient:";

pub const PRESETS: &[(&str, &[&str])] = &[
    ("atlas", &["#feac5e", "#c779d0", "#4bc0c8"]),
    ("cristal", &["#bdfff3", "#4ac29a"]),
    ("teen", &["#77a1d3", "#79cbca", "#e684ae"]),
    ("mind", &["#473b7b", "#3584a7", "#30d2be"]),
    ("morning", &["#ff5f6d", "#ffc371"]),
    ("vice", &["#5ee7df", "#b490ca"]),
    ("passion", &["#f43b47", "#453a94"]),
    ("fruit", &["#ff4e50", "#f9d423"]),
    ("instagram", &["#833ab4", "#fd1d1d", "#fcb045"]),
    ("retro", &["#3f51b1", "#5a55ae", "#7b5fac", "#8f6aae", "#a86aa4", "#cc6b8e", "#f18271", "#f3a469", "#f7c978"]),
    ("summer", &["#fdbb2d", "#22c1c3"]),
    ("rainbow", &["#ff0000", "#ffff00", "#00ff00", "#00ffff", "#0000ff", "#ff00ff", "#ff0000"]),
    ("pastel", &["#aee9d8", "#cdeeb0", "#f6f0a8", "#f7c8a8", "#f3aecb", "#c3b6f0", "#aee9d8"]),
];

pub fn is_gradient_spec(v: Option<&str>) -> bool {
    v.is_some_and(|s| s.starts_with(PREFIX))
}

fn hex_to_rgb(hex: &str) -> Option<Rgb> {
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(Rgb {
        r: u8::from_str_radix(&hex[0..2], 16).ok()?,
        g: u8::from_str_radix(&hex[2..4], 16).ok()?,
        b: u8::from_str_radix(&hex[4..6], 16).ok()?,
    })
}

fn resolve_stop(stop: &str) -> Option<Rgb> {
    if let Some(h) = stop.strip_prefix("hex:") {
        return hex_to_rgb(h);
    }
    if let Some(h) = stop.strip_prefix('#') {
        return hex_to_rgb(h);
    }
    hex_to_rgb(stop)
}

pub fn parse_gradient_spec(v: Option<&str>) -> Option<Vec<Rgb>> {
    let body = v?.strip_prefix(PREFIX)?;
    let body = crate::jsnum::js_trim(body);
    if body.is_empty() {
        return None;
    }
    let lower = body.to_lowercase();
    let raw: Vec<String> = match PRESETS.iter().find(|(n, _)| *n == lower) {
        Some((_, stops)) => stops.iter().map(|s| s.to_string()).collect(),
        None => body.split(if body.contains(',') { ',' } else { '-' }).map(|s| s.to_string()).collect(),
    };
    let stops: Vec<Rgb> =
        raw.iter().map(|s| crate::jsnum::js_trim(s)).filter(|s| !s.is_empty()).filter_map(resolve_stop).collect();
    if stops.len() >= 2 {
        Some(stops)
    } else {
        None
    }
}

fn srgb_to_linear(c: u8) -> f64 {
    let n = c as f64 / 255.0;
    if n <= 0.04045 {
        n / 12.92
    } else {
        ((n + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f64) -> u8 {
    let v = if c <= 0.0031308 { 12.92 * c } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    js_round(v.clamp(0.0, 1.0) * 255.0) as u8
}

struct Lab {
    l: f64,
    a: f64,
    b: f64,
}

fn to_oklab(c: Rgb) -> Lab {
    let (lr, lg, lb) = (srgb_to_linear(c.r), srgb_to_linear(c.g), srgb_to_linear(c.b));
    let l = 0.4122214708 * lr + 0.5363325363 * lg + 0.0514459929 * lb;
    let m = 0.2119034982 * lr + 0.6806995451 * lg + 0.1073969566 * lb;
    let s = 0.0883024619 * lr + 0.2817188376 * lg + 0.6299787005 * lb;
    let (l, m, s) = (l.cbrt(), m.cbrt(), s.cbrt());
    Lab {
        l: 0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        a: 1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        b: 0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    }
}

fn from_oklab(lab: Lab) -> Rgb {
    let l = lab.l + 0.3963377774 * lab.a + 0.2158037573 * lab.b;
    let m = lab.l - 0.1055613458 * lab.a - 0.0638541728 * lab.b;
    let s = lab.l - 0.0894841775 * lab.a - 1.2914855480 * lab.b;
    let (l, m, s) = (l.powf(3.0), m.powf(3.0), s.powf(3.0));
    Rgb {
        r: linear_to_srgb(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s),
        g: linear_to_srgb(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s),
        b: linear_to_srgb(-0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s),
    }
}

pub fn sample_gradient(stops: &[Rgb], t: f64) -> Rgb {
    let Some(&first) = stops.first() else { return Rgb { r: 0, g: 0, b: 0 } };
    if stops.len() == 1 {
        return first;
    }
    let clamped = t.clamp(0.0, 1.0);
    let scaled = clamped * (stops.len() - 1) as f64;
    let lower_index = ((stops.len() - 2) as f64).min(scaled.floor()) as usize;
    let fraction = scaled - lower_index as f64;
    let a = to_oklab(stops[lower_index]);
    let b = to_oklab(stops[lower_index + 1]);
    from_oklab(Lab {
        l: a.l + (b.l - a.l) * fraction,
        a: a.a + (b.a - a.a) * fraction,
        b: a.b + (b.b - a.b) * fraction,
    })
}

pub fn rgb_to_ansi256(c: Rgb) -> u32 {
    if c.r == c.g && c.g == c.b {
        if c.r < 8 {
            return 16;
        }
        if c.r > 248 {
            return 231;
        }
        return js_round(((c.r as f64 - 8.0) / 247.0) * 24.0) as u32 + 232;
    }
    let q = |v: u8| js_round((v as f64 / 255.0) * 5.0) as u32;
    16 + 36 * q(c.r) + 6 * q(c.g) + q(c.b)
}

pub fn gradient_code_at(stops: &[Rgb], t: f64, level: Level) -> String {
    let c = sample_gradient(stops, t);
    if level == Level::Truecolor {
        format!("\x1b[38;2;{};{};{}m", c.r, c.g, c.b)
    } else {
        format!("\x1b[38;5;{}m", rgb_to_ansi256(c))
    }
}

/// JS `/\s/` for a single code point.
pub fn is_js_space(c: char) -> bool {
    matches!(
        c,
        '\u{9}'..='\u{d}' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}'
            | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'
    )
}

/// Per-code-point gradient over one widget's text (no trailing reset).
pub fn apply_gradient_to_text(text: &str, stops: &[Rgb], level: Level) -> String {
    if level == Level::Ansi16 || text.is_empty() {
        return text.to_string();
    }
    let mut visible = 0usize;
    let mut i = 0;
    while i < text.len() {
        if let Some(e) = crate::ansi::parse_escape(text, i) {
            i = e.next;
            continue;
        }
        let c = text[i..].chars().next().unwrap();
        if !is_js_space(c) {
            visible += 1;
        }
        i += c.len_utf8();
    }
    if visible == 0 {
        return text.to_string();
    }
    let denom = (visible.saturating_sub(1)).max(1) as f64;
    let mut out = String::new();
    let mut idx = 0usize;
    let mut i = 0;
    while i < text.len() {
        if let Some(e) = crate::ansi::parse_escape(text, i) {
            out.push_str(e.seq(text));
            i = e.next;
            continue;
        }
        let c = text[i..].chars().next().unwrap();
        if is_js_space(c) {
            out.push(c);
        } else {
            out.push_str(&gradient_code_at(stops, idx as f64 / denom, level));
            out.push(c);
            idx += 1;
        }
        i += c.len_utf8();
    }
    out
}
