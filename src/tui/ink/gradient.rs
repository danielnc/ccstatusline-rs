//! gradient-string's `multiline` with tinygradient's RGB interpolation, as ink-gradient applies it to the
//! editor's title.

use super::chalk::Chalk;

/// gradient-string's `retro` preset.
pub const RETRO: &[&str] =
    &["#3f51b1", "#5a55ae", "#7b5fac", "#8f6aae", "#a86aa4", "#cc6b8e", "#f18271", "#f3a469", "#f7c978"];

#[derive(Clone, Copy, Debug)]
struct Rgb {
    r: f64,
    g: f64,
    b: f64,
}

impl Rgb {
    /// tinycolor's `toHex`: each channel rounded.
    fn to_hex(self) -> String {
        let c = |v: f64| crate::jsnum::js_round(v) as i64;
        format!("{:02x}{:02x}{:02x}", c(self.r), c(self.g), c(self.b))
    }

    /// tinycolor's `toRgb`.
    fn rounded(self) -> Rgb {
        let c = |v: f64| crate::jsnum::js_round(v);
        Rgb { r: c(self.r), g: c(self.g), b: c(self.b) }
    }
}

fn parse(hex: &str) -> Rgb {
    let (r, g, b) = super::chalk::hex_to_rgb(hex);
    Rgb { r: r as f64, g: g as f64, b: b as f64 }
}

/// tinycolor's constructor on interpolated channels (`bound01(n, 255) * 255`, and values under 1 rounded).
fn tinycolor(c: Rgb) -> Rgb {
    let bound = |n: f64| {
        let n = n.clamp(0.0, 255.0);
        let v = if (n - 255.0).abs() < 0.000001 { 1.0 } else { (n % 255.0) / 255.0 };
        let v = v * 255.0;
        if v < 1.0 {
            crate::jsnum::js_round(v)
        } else {
            v
        }
    };
    Rgb { r: bound(c.r), g: bound(c.g), b: bound(c.b) }
}

fn interpolate_rgb(a: Rgb, b: Rgb, steps: usize) -> Vec<Rgb> {
    let start = a.rounded();
    let end = b.rounded();
    let step = |s: f64, e: f64| if steps == 0 { 0.0 } else { (e - s) / steps as f64 };
    let (sr, sg, sb) = (step(start.r, end.r), step(start.g, end.g), step(start.b, end.b));
    let wrap = |v: f64| if v < 0.0 { v + 256.0 } else { v % 256.0 };
    let mut out = vec![a];
    for i in 1..steps {
        let i = i as f64;
        out.push(tinycolor(Rgb { r: wrap(sr * i + start.r), g: wrap(sg * i + start.g), b: wrap(sb * i + start.b) }));
    }
    out
}

fn substeps(stop_count: usize, steps: usize) -> Vec<usize> {
    let positions: Vec<f64> = (0..stop_count).map(|i| i as f64 / (stop_count - 1) as f64).collect();
    let mut sub: Vec<i64> = (1..stop_count)
        .map(|i| (crate::jsnum::js_round((steps as f64 - 1.0) * (positions[i] - positions[i - 1])) as i64).max(1))
        .collect();
    let mut total: i64 = 1 + sub.iter().sum::<i64>();
    while total != steps as i64 {
        if total < steps as i64 {
            let min = *sub.iter().min().unwrap();
            let idx = sub.iter().position(|&v| v == min).unwrap();
            sub[idx] += 1;
            total += 1;
        } else {
            let max = *sub.iter().max().unwrap();
            let idx = sub.iter().position(|&v| v == max).unwrap();
            sub[idx] -= 1;
            total -= 1;
        }
    }
    sub.into_iter().map(|v| v as usize).collect()
}

/// tinygradient's `rgb(steps)` for evenly spaced stops.
fn gradient_rgb(stops: &[&str], steps: usize) -> Vec<Rgb> {
    let colors: Vec<Rgb> = stops.iter().map(|h| parse(h)).collect();
    let sub = substeps(colors.len(), steps);
    let mut out = Vec::new();
    for i in 0..colors.len() - 1 {
        out.extend(interpolate_rgb(colors[i], colors[i + 1], sub[i]));
    }
    out.push(*colors.last().unwrap());
    out
}

/// gradient-string's `multiline(str)`.
pub fn multiline(chalk: &Chalk, stops: &[&str], text: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let max_len = lines.iter().map(|l| l.encode_utf16().count()).max().unwrap_or(0).max(stops.len());
    let colors = gradient_rgb(stops, max_len);
    lines
        .iter()
        .map(|line| {
            let mut it = colors.iter();
            line.chars()
                .map(|c| {
                    let hex = it.next().map_or_else(|| "#000".to_string(), |c| c.to_hex());
                    chalk.apply(&chalk.hex(&hex, false), &c.to_string())
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_matches_upstream_at_256_colors() {
        // Captured from upstream's TUI at color level 2.
        let s = multiline(&Chalk::new(2), RETRO, "CCStatusline Configuration");
        let mut o = super::super::output::Output::new(40, 1);
        o.write(0, 0, s);
        assert_eq!(
            o.get().0,
            "\x1b[38;5;67mCC\x1b[38;5;103mStatus\x1b[38;5;139mline C\x1b[38;5;175monfi\x1b[38;5;174mg\x1b[38;5;216murati\x1b[38;5;222mon\x1b[39m"
        );
    }
}
