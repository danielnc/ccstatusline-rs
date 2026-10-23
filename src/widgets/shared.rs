//! Helpers shared by many widgets: labels, hide states, metadata flags and
//! formats, number formatting, progress bars, context-window math, links.
//! Ports of upstream `src/widgets/shared/{raw-or-labeled,hideable,metadata,
//! format-options,progress-bar}.ts` and `src/utils/{number-format,
//! format-tokens,context-window,model-context,context-percentage,hyperlink}.ts`.

use crate::context::RenderContext;
use crate::jsnum::{js_round, to_fixed, to_js_string};
use crate::settings::{NumberFormat, Settings, WidgetItem};
use crate::status::{CurrentUsage, StatusJson};

// ------------------------------------------------------------ labels/hiding

pub fn label<'a>(item: &'a WidgetItem, default: &'a str) -> &'a str {
    item.meta("label").unwrap_or(default)
}

/// `formatRawOrLabeledValue`
pub fn raw_or_labeled(item: &WidgetItem, prefix: &str, value: &str) -> String {
    if item.raw() {
        value.to_string()
    } else {
        format!("{}{value}", label(item, prefix))
    }
}

pub fn parse_hide_states(v: Option<&str>) -> Vec<&str> {
    match v {
        None => vec![],
        Some(v) => v.split(',').map(crate::jsnum::js_trim).filter(|k| !k.is_empty()).collect(),
    }
}

/// `isHidden(item, key, defaultEnabled)`
pub fn is_hidden(item: &WidgetItem, key: &str, default_enabled: bool) -> bool {
    match item.meta("hide") {
        None => default_enabled,
        Some(v) => parse_hide_states(Some(v)).contains(&key),
    }
}

pub fn is_flag(item: &WidgetItem, key: &str) -> bool {
    item.meta(key) == Some("true")
}

/// `getFormat(item, { formats, defaultFormat })`
pub fn get_format<'a>(item: &WidgetItem, formats: &[&'a str], default: &'a str) -> &'a str {
    let f = item.meta("format").unwrap_or("");
    formats.iter().find(|x| **x == f).copied().unwrap_or(default)
}

/// `isNerdFontEnabled`: the widget's predicate AND metadata.nerdFont === 'true'.
pub fn nerd_font_enabled(item: &WidgetItem, can_use: bool) -> bool {
    can_use && is_flag(item, "nerdFont")
}

// --------------------------------------------------------- number formatting

pub fn effective_decimals(f: &NumberFormat, baseline: usize) -> usize {
    if f.style.as_deref() == Some("whole") {
        return 0;
    }
    f.decimals.map(|d| d as usize).unwrap_or(baseline)
}

fn trim_trailing_zeros(v: &str) -> String {
    if !v.contains('.') {
        return v.to_string();
    }
    // /\.?0+$/
    let t = v.trim_end_matches('0');
    let t = t.strip_suffix('.').unwrap_or(t);
    if t.len() == v.len() {
        v.to_string()
    } else {
        t.to_string()
    }
}

pub fn render_magnitude(v: f64, f: &NumberFormat, baseline: usize) -> String {
    let fixed = to_fixed(v, effective_decimals(f, baseline));
    if f.style.as_deref() == Some("compact") {
        trim_trailing_zeros(&fixed)
    } else {
        fixed
    }
}

pub fn format_percent(v: f64, f: &NumberFormat, baseline: usize) -> String {
    format!("{}%", render_magnitude(v, f, baseline))
}

pub fn format_cost(v: f64, f: &NumberFormat) -> String {
    format!("${}", render_magnitude(v, f, 2))
}

/// `resolveNumberFormat(kind, item, settings)`: kind is token|speed|percent|memory|cost.
pub fn resolve_number_format(kind: &str, item: &WidgetItem, s: &Settings) -> NumberFormat {
    let global = s.number_format.as_ref().and_then(|g| match kind {
        "token" => g.token.clone(),
        "speed" => g.speed.clone(),
        "percent" => g.percent.clone(),
        "memory" => g.memory.clone(),
        "cost" => g.cost.clone(),
        _ => None,
    });
    global.or_else(|| item.number_format.clone()).unwrap_or_default()
}

/// `formatTokens(count, format, decimals)`
pub fn format_tokens(count: f64, f: &NumberFormat, decimals: usize) -> String {
    let k_dec = effective_decimals(f, decimals);
    if count >= 1_000_000.0 - 500.0 / 10f64.powi(k_dec as i32) {
        return format!("{}M", render_magnitude(count / 1_000_000.0, f, 1));
    }
    if count >= 1000.0 {
        return format!("{}k", render_magnitude(count / 1000.0, f, decimals));
    }
    to_js_string(count)
}

// --------------------------------------------------------------- progress bar

pub fn timer_progress_bar(percent: f64, width: usize, cursor_percent: Option<f64>) -> String {
    let clamped = percent.clamp(0.0, 100.0);
    let filled = js_round((clamped / 100.0) * width as f64) as isize;
    let cursor: isize = match cursor_percent {
        Some(c) => (((c.clamp(0.0, 100.0)) / 100.0 * width as f64).floor() as isize).min(width as isize - 1),
        None => -1,
    };
    let mut bar = String::new();
    for i in 0..width as isize {
        if i == cursor {
            bar.push('│');
        } else if i < filled {
            bar.push('█');
        } else {
            bar.push('░');
        }
    }
    bar
}

// ------------------------------------------------------------- context window

#[derive(Clone, Debug, Default)]
pub struct ContextWindowMetrics {
    pub window_size: Option<f64>,
    pub used_tokens: Option<f64>,
    pub context_length_tokens: Option<f64>,
    pub used_percentage: Option<f64>,
    pub remaining_percentage: Option<f64>,
    pub total_input_tokens: Option<f64>,
    pub total_output_tokens: Option<f64>,
    pub cached_tokens: Option<f64>,
    pub total_tokens: Option<f64>,
}

fn non_neg(v: Option<f64>) -> Option<f64> {
    v.filter(|x| x.is_finite()).map(|x| x.max(0.0))
}

#[derive(Clone, Copy, Debug, Default)]
pub struct UsageTokens {
    pub input: f64,
    pub output: f64,
    pub creation: f64,
    pub read: f64,
}

pub fn parse_usage_tokens(u: &crate::status::CurrentUsageObj) -> UsageTokens {
    UsageTokens {
        input: non_neg(u.input_tokens).unwrap_or(0.0),
        output: non_neg(u.output_tokens).unwrap_or(0.0),
        creation: non_neg(u.cache_creation_input_tokens).unwrap_or(0.0),
        read: non_neg(u.cache_read_input_tokens).unwrap_or(0.0),
    }
}

pub fn context_window_metrics(data: &StatusJson) -> ContextWindowMetrics {
    let Some(cw) = &data.context_window else { return ContextWindowMetrics::default() };
    let raw_size = non_neg(cw.context_window_size);
    let window_size = raw_size.filter(|s| *s > 0.0);
    let total_input = non_neg(cw.total_input_tokens);
    let total_output = non_neg(cw.total_output_tokens);
    let (mut cu_total, mut ctx_len, mut cached) = (None, None, None);
    match &cw.current_usage {
        Some(CurrentUsage::Num(n)) => {
            cu_total = non_neg(Some(*n));
            ctx_len = cu_total;
        }
        Some(CurrentUsage::Obj(o)) => {
            let t = parse_usage_tokens(o);
            cu_total = Some(t.input + t.output + t.creation + t.read);
            ctx_len = Some(t.input + t.creation + t.read);
            cached = Some(t.creation + t.read);
        }
        None => {}
    }
    let raw_used = non_neg(cw.used_percentage);
    let raw_remaining = non_neg(cw.remaining_percentage);
    let used_from_pct = match (raw_used, window_size) {
        (Some(p), Some(w)) => Some((p / 100.0) * w),
        _ => None,
    };
    let used_tokens = cu_total.or(used_from_pct);
    let clamp = |v: f64| v.clamp(0.0, 100.0);
    let used_percentage = match raw_used {
        Some(p) => Some(clamp(p)),
        None => match (used_tokens, window_size) {
            (Some(u), Some(w)) if w > 0.0 => Some(clamp((u / w) * 100.0)),
            _ => None,
        },
    };
    let remaining_percentage = match raw_remaining {
        Some(r) => Some(clamp(r)),
        None => used_percentage.map(|u| 100.0 - u),
    };
    let total_tokens = cu_total.or(match (total_input, total_output) {
        (Some(i), Some(o)) => Some(i + o),
        _ => None,
    });
    ContextWindowMetrics {
        window_size,
        used_tokens,
        context_length_tokens: ctx_len.or(used_tokens),
        used_percentage,
        remaining_percentage,
        total_input_tokens: total_input,
        total_output_tokens: total_output,
        cached_tokens: cached,
        total_tokens,
    }
}

pub struct ContextConfig {
    pub max_tokens: f64,
    pub usable_tokens: f64,
}

fn fallback_window_size() -> f64 {
    if let Ok(raw) = std::env::var("CCSTATUSLINE_CONTEXT_SIZE_FALLBACK") {
        if !raw.is_empty() {
            if let Some(v) = crate::jsnum::parse_int(&raw) {
                if v > 0.0 {
                    return v;
                }
            }
        }
    }
    200000.0
}

fn parse_context_window_size(id: &str) -> Option<f64> {
    use std::sync::OnceLock;
    static DELIM: OnceLock<regex::Regex> = OnceLock::new();
    static PLAIN: OnceLock<regex::Regex> = OnceLock::new();
    let delim = DELIM.get_or_init(|| {
        regex::Regex::new(r"(?i)(?:\(|\[)\s*([0-9]+(?:[,_][0-9]+)*(?:\.[0-9]+)?)\s*([km])\s*(?:\)|\])").unwrap()
    });
    let plain = PLAIN.get_or_init(|| {
        regex::Regex::new(
            r"(?i)(?-u:\b)([0-9]+(?:[,_][0-9]+)*(?:\.[0-9]+)?)\s*([km])(?:\s*(?:token\s*)?context)?(?-u:\b)",
        )
        .unwrap()
    });
    let to_size = |v: &str, unit: &str| -> Option<f64> {
        let parsed = crate::jsnum::parse_float(&v.replace([',', '_'], ""))?;
        if parsed.is_finite() && parsed > 0.0 {
            Some(js_round(parsed * if unit.eq_ignore_ascii_case("m") { 1_000_000.0 } else { 1000.0 }))
        } else {
            None
        }
    };
    if let Some(c) = delim.captures(id) {
        if let Some(v) = to_size(&c[1], &c[2]) {
            return Some(v);
        }
    }
    let c = plain.captures(id)?;
    to_size(&c[1], &c[2])
}

/// `getModelContextIdentifier(model)`
pub fn model_context_identifier(data: &StatusJson) -> Option<String> {
    use crate::status::Model;
    match &data.model {
        Model::Str(s) => {
            let t = crate::jsnum::js_trim(s);
            (!t.is_empty()).then(|| t.to_string())
        }
        Model::None => None,
        Model::Obj { id, display_name } => {
            let id = id.as_deref().map(crate::jsnum::js_trim);
            let dn = display_name.as_deref().map(crate::jsnum::js_trim);
            match (id.filter(|s| !s.is_empty()), dn.filter(|s| !s.is_empty())) {
                (Some(i), Some(d)) => Some(format!("{i} {d}")),
                // `id ?? displayName`: an empty (trimmed) id is not nullish
                _ => id.or(dn).map(String::from),
            }
        }
    }
}

pub fn context_config(model_identifier: Option<&str>, window_size: Option<f64>) -> ContextConfig {
    let mk = |s: f64| ContextConfig { max_tokens: s, usable_tokens: (s * 0.8).floor() };
    if let Some(w) = window_size.filter(|w| w.is_finite() && *w > 0.0) {
        return mk(w);
    }
    let fallback = fallback_window_size();
    let Some(id) = model_identifier.filter(|s| !s.is_empty()) else { return mk(fallback) };
    match parse_context_window_size(id) {
        Some(v) => mk(v),
        None => mk(fallback),
    }
}

pub struct ContextPercentage {
    pub used_percentage: f64,
    pub window_size: Option<f64>,
}

pub fn context_percentage_metrics(ctx: &RenderContext) -> Option<ContextPercentage> {
    let m = context_window_metrics(ctx.data);
    let id = model_context_identifier(ctx.data);
    let cfg = context_config(id.as_deref(), m.window_size);
    if let Some(u) = m.used_percentage {
        return Some(ContextPercentage { used_percentage: u, window_size: Some(cfg.max_tokens) });
    }
    let tm = crate::transcript::token_metrics(ctx)?;
    Some(ContextPercentage {
        used_percentage: js_min(100.0, tm.context_length / cfg.max_tokens * 100.0),
        window_size: Some(cfg.max_tokens),
    })
}

pub fn context_percentage(ctx: &RenderContext) -> f64 {
    context_percentage_metrics(ctx).map(|m| m.used_percentage).unwrap_or(0.0)
}

// -------------------------------------------------------------------- links

pub fn osc8_link(url: &str, text: &str) -> String {
    if !crate::sanitize::is_safe_hyperlink_url(url) {
        return text.to_string();
    }
    format!("\x1b]8;;{url}\x1b\\{text}\x1b]8;;\x1b\\")
}

/// JS `encodeURIComponent`.
pub fn encode_uri_component(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub fn encode_git_ref_for_url_path(r: &str) -> String {
    r.split('/').map(encode_uri_component).collect::<Vec<_>>().join("/")
}

fn encode_file_path_for_uri(p: &str) -> String {
    p.replace('\\', "/").split('/').map(encode_uri_component).collect::<Vec<_>>().join("/")
}

/// `buildIdeFileUrl(path, mode)` (mode: vscode|cursor)
pub fn build_ide_file_url(path: &str, mode: &str) -> String {
    let p = path.replace('\\', "/");
    if let Some(rest) = p.strip_prefix("//") {
        let (host, tail) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, ""),
        };
        if !host.is_empty() {
            let enc = encode_file_path_for_uri(if tail.is_empty() { "/" } else { tail });
            return format!("{mode}://file//{host}{enc}");
        }
    }
    let b = p.as_bytes();
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b.len() == 2 || b[2] == b'/') {
        let tail = if b.len() == 2 { "/" } else { &p[2..] };
        return format!("{mode}://file/{}{}", &p[..2], encode_file_path_for_uri(tail));
    }
    format!("{mode}://file{}", encode_file_path_for_uri(&p))
}

/// `Math.min(a, b)`: NaN if either is NaN (f64::min drops NaN).
pub fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.min(b)
    }
}
