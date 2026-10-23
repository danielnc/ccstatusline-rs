//! custom-command (upstream `src/widgets/CustomCommand.tsx`); the runner and
//! its cache live in `crate::misc::custom_command`.

use super::widget_info;
use crate::context::RenderContext;
use crate::misc::custom_command::{run, CmdResult, Request};
use crate::settings::WidgetItem;
use crate::status::coerce_number;
use crate::widgets::Widget;
use serde_json::{Map, Value};

pub struct CustomCommand;

pub static CUSTOM_COMMAND: CustomCommand = CustomCommand;

/// `s.substring(0, n)` in UTF-16 units (a split pair becomes U+FFFD).
fn utf16_prefix(s: &str, n: usize) -> String {
    let mut units = 0;
    let mut out = String::new();
    for c in s.chars() {
        let w = c.len_utf16();
        if units + w > n {
            if units < n {
                out.push('\u{fffd}');
            }
            break;
        }
        units += w;
        out.push(c);
    }
    out
}

/// `applyMaxWidth(text, maxWidth)` for maxWidth > 0. Widths are integers, so
/// a fractional limit acts like its floor (`'.'.repeat(2.5)` is "..").
fn apply_max_width(text: &str, max_width: f64) -> String {
    let w = max_width.floor().min(usize::MAX as f64) as usize;
    if w == 0 {
        // maxWidth in (0, 1): only zero-width text fits
        return if crate::ansi::get_visible_width(text) == 0 { text.to_string() } else { String::new() };
    }
    crate::ansi::truncate_styled_text(text, w, true)
}

impl Widget for CustomCommand {
    widget_info!("white", "Custom Command", "Executes a custom shell command and displays output", "Custom");
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        let command = item.command_path.as_deref().filter(|c| !c.is_empty());
        if ctx.is_preview {
            return Some(match command {
                Some(c) => {
                    let more = if c.encode_utf16().count() > 20 { "..." } else { "" };
                    format!("[cmd: {}{more}]", utf16_prefix(c, 20))
                }
                None => "[No command]".into(),
            });
        }
        let command = command?;
        let mut payload = status_payload(&ctx.data.raw);
        if let (Some(w), Value::Object(m)) = (ctx.terminal_width, &mut payload) {
            m.insert("terminal_width".into(), Value::from(w as u64));
        }
        let result = run(&Request {
            command,
            input: crate::jsjson::stringify(&payload),
            timeout_ms: item.timeout.unwrap_or(1000.0),
            ttl_seconds: ctx.settings.custom_command_cache_ttl_seconds,
            session_id: ctx.data.session_id.as_deref(),
            terminal_width: ctx.terminal_width,
        });
        let mut output = match result {
            CmdResult::Failed(marker) => return Some(marker),
            CmdResult::Ok(s) => s,
        };
        if item.preserve_colors != Some(true) {
            output = crate::ansi::get_visible_text(&output);
        }
        let truncated = match item.max_width.filter(|w| *w > 0.0) {
            Some(w) => apply_max_width(&output, w),
            None => output.clone(),
        };
        // a cut can drop the command's own trailing reset; close open styling
        output = if truncated != output && crate::ansi::strip_sgr_codes(&truncated) != truncated {
            format!("{truncated}\x1b[0m")
        } else {
            truncated
        };
        (!output.is_empty()).then_some(output)
    }
    fn preserves_rendered_colors(&self, item: &WidgetItem) -> bool {
        item.preserve_colors == Some(true)
    }
    fn supports_raw_value(&self) -> bool {
        false
    }
    fn supports_colors(&self, item: &WidgetItem) -> bool {
        item.preserve_colors != Some(true)
    }
}

// ---------------------------------------------------- the zod-parsed status

/// One field of a zod object shape.
enum F {
    Str,
    Num,
    Obj(&'static [(&'static str, F)]),
    /// `model`: string or `{id, display_name}`
    Model,
    /// `current_usage`: number or the usage object
    Usage,
}

const MODEL_OBJ: &[(&str, F)] = &[("id", F::Str), ("display_name", F::Str)];
const USAGE_OBJ: &[(&str, F)] = &[
    ("input_tokens", F::Num),
    ("output_tokens", F::Num),
    ("cache_creation_input_tokens", F::Num),
    ("cache_read_input_tokens", F::Num),
];
const PERIOD: &[(&str, F)] = &[("used_percentage", F::Num), ("resets_at", F::Num)];
const SHAPE: &[(&str, F)] = &[
    ("hook_event_name", F::Str),
    ("session_id", F::Str),
    ("transcript_path", F::Str),
    ("cwd", F::Str),
    ("model", F::Model),
    ("workspace", F::Obj(&[("current_dir", F::Str), ("project_dir", F::Str)])),
    ("version", F::Str),
    ("output_style", F::Obj(&[("name", F::Str)])),
    ("effort", F::Obj(&[("level", F::Str)])),
    (
        "cost",
        F::Obj(&[
            ("total_cost_usd", F::Num),
            ("total_duration_ms", F::Num),
            ("total_api_duration_ms", F::Num),
            ("total_lines_added", F::Num),
            ("total_lines_removed", F::Num),
        ]),
    ),
    (
        "context_window",
        F::Obj(&[
            ("context_window_size", F::Num),
            ("total_input_tokens", F::Num),
            ("total_output_tokens", F::Num),
            ("current_usage", F::Usage),
            ("used_percentage", F::Num),
            ("remaining_percentage", F::Num),
        ]),
    ),
    ("vim", F::Obj(&[("mode", F::Str)])),
    (
        "worktree",
        F::Obj(&[
            ("name", F::Str),
            ("path", F::Str),
            ("branch", F::Str),
            ("original_cwd", F::Str),
            ("original_branch", F::Str),
        ]),
    ),
    (
        "rate_limits",
        F::Obj(&[
            ("five_hour", F::Obj(PERIOD)),
            ("seven_day", F::Obj(PERIOD)),
            ("seven_day_sonnet", F::Obj(PERIOD)),
            ("seven_day_opus", F::Obj(PERIOD)),
        ]),
    ),
];

/// A value already validated by `status::parse_status`, as zod outputs it:
/// numbers coerced, unknown keys of nested objects stripped.
fn shape_value(v: &Value, f: &F) -> Value {
    match (f, v) {
        (_, Value::Null) => Value::Null,
        (F::Num, v) => coerce_number(v).map(crate::jsjson::num).unwrap_or_else(|| v.clone()),
        (F::Obj(fields), Value::Object(o)) => shape_object(o, fields, false),
        (F::Model, Value::Object(o)) => shape_object(o, MODEL_OBJ, false),
        (F::Usage, Value::Object(o)) => shape_object(o, USAGE_OBJ, false),
        (F::Usage, v) => coerce_number(v).map(crate::jsjson::num).unwrap_or_else(|| v.clone()),
        (_, v) => v.clone(),
    }
}

/// Shape keys first (in shape order, when present), then — for the loose top
/// level only — the remaining input keys in input order.
fn shape_object(o: &Map<String, Value>, fields: &[(&str, F)], loose: bool) -> Value {
    let mut out = Map::new();
    for (k, f) in fields {
        if let Some(v) = o.get(*k) {
            out.insert(k.to_string(), shape_value(v, f));
        }
    }
    if loose {
        for (k, v) in o {
            if !fields.iter().any(|(f, _)| f == k) {
                out.insert(k.clone(), v.clone());
            }
        }
    }
    Value::Object(out)
}

pub fn status_payload(raw: &Value) -> Value {
    match raw {
        Value::Object(o) => shape_object(o, SHAPE, true),
        v => v.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_matches_zod_output_order() {
        let raw: Value = serde_json::from_str(
            r#"{"zzz":1.50,"cost":{"extra":1,"total_lines_removed":"7","total_cost_usd":" 1.5 "},"model":{"display_name":"X","id":"y","foo":2},"session_id":"s","aaa":{"n":[1.0,2e3,"x"]},"workspace":null,"vim":{"mode":"NORMAL","x":1},"rate_limits":{"seven_day":{"resets_at":"5"},"five_hour":{}},"context_window":{"current_usage":"12"}}"#,
        )
        .unwrap();
        let mut p = status_payload(&raw);
        p.as_object_mut().unwrap().insert("terminal_width".into(), Value::from(3000));
        assert_eq!(
            crate::jsjson::stringify(&p),
            r#"{"session_id":"s","model":{"id":"y","display_name":"X"},"workspace":null,"cost":{"total_cost_usd":1.5,"total_lines_removed":7},"context_window":{"current_usage":12},"vim":{"mode":"NORMAL"},"rate_limits":{"five_hour":{},"seven_day":{"resets_at":5}},"zzz":1.5,"aaa":{"n":[1,2000,"x"]},"terminal_width":3000}"#
        );
    }

    #[test]
    fn preview_prefix() {
        assert_eq!(utf16_prefix("abcdef", 3), "abc");
        assert_eq!(utf16_prefix("ab😀", 3), "ab\u{fffd}");
    }
}
