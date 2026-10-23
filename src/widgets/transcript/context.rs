//! Context Length / Window / % / % (usable) (upstream `ContextLength.ts`,
//! `ContextWindow.ts`, `ContextPercentage.ts`, `ContextPercentageUsable.ts`,
//! `widgets/shared/context-{inverse,slider}.ts`).

use crate::context::RenderContext;
use crate::jsnum::js_round;
use crate::settings::{NumberFormat, WidgetItem};
use crate::widgets::shared::{
    context_config, context_percentage_metrics, context_window_metrics, format_percent, format_tokens, is_flag,
    model_context_identifier, raw_or_labeled, resolve_number_format,
};
use crate::widgets::{EditorDisplay, Keybind, Widget};
use serde_json::Value;

/// `Math.min(100, v)` (NaN stays NaN)
fn min100(v: f64) -> f64 {
    if v.is_nan() {
        v
    } else {
        v.min(100.0)
    }
}

pub struct Length;

impl Widget for Length {
    fn default_color(&self) -> &'static str {
        "brightBlack"
    }
    fn display_name(&self) -> &'static str {
        "Context Length"
    }
    fn description(&self) -> &'static str {
        "Shows the current context window size in tokens"
    }
    fn category(&self) -> &'static str {
        "Context"
    }
    fn label_prefix(&self, _item: &WidgetItem) -> Option<String> {
        Some("Ctx: ".into())
    }
    fn supports_number_format(&self) -> bool {
        true
    }
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        const LABEL: &str = "Ctx: ";
        let format = resolve_number_format("token", item, ctx.settings);
        if ctx.is_preview {
            return Some(raw_or_labeled(item, LABEL, &format_tokens(18600.0, &format, 1)));
        }
        if let Some(len) = context_window_metrics(ctx.data).context_length_tokens {
            return Some(raw_or_labeled(item, LABEL, &format_tokens(len, &format, 1)));
        }
        let m = crate::transcript::token_metrics(ctx)?;
        Some(raw_or_labeled(item, LABEL, &format_tokens(m.context_length, &format, 1)))
    }
}

const WINDOW_LABEL: &str = "Win: ";

pub struct Window;

impl Widget for Window {
    fn default_color(&self) -> &'static str {
        "brightBlack"
    }
    fn display_name(&self) -> &'static str {
        "Context Window"
    }
    fn description(&self) -> &'static str {
        "Shows the total context window size for the current model"
    }
    fn category(&self) -> &'static str {
        "Context"
    }
    fn label_prefix(&self, _item: &WidgetItem) -> Option<String> {
        Some(WINDOW_LABEL.into())
    }
    fn supports_number_format(&self) -> bool {
        true
    }
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        const LABEL: &str = WINDOW_LABEL;
        let format = resolve_number_format("token", item, ctx.settings);
        if ctx.is_preview {
            return Some(raw_or_labeled(item, LABEL, &format_tokens(200000.0, &format, 1)));
        }
        let total = context_window_metrics(ctx.data)
            .window_size
            .unwrap_or_else(|| context_config(model_context_identifier(ctx.data).as_deref(), None).max_tokens);
        if total <= 0.0 {
            return None;
        }
        Some(raw_or_labeled(item, LABEL, &format_tokens(total, &format, 1)))
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Slider {
    None,
    Full,
    Only,
}

fn slider_mode(item: &WidgetItem) -> Slider {
    match item.meta("display") {
        Some("slider") => Slider::Full,
        Some("slider-only") => Slider::Only,
        _ => Slider::None,
    }
}

/// `makeSliderBar(percent)` (width 10)
fn slider_bar(percent: f64) -> String {
    // NaN stays NaN (an all-empty bar), as with Math.max/Math.min
    let clamped = percent.clamp(0.0, 100.0);
    let filled = js_round((clamped / 100.0) * 10.0);
    (0..10).map(|i| if (i as f64) < filled { '▓' } else { '░' }).collect()
}

/// `formatRawOrLabeledValue(item, label, renderContextSlider(...) ?? formatPercent(...))`
fn format_context_percentage(item: &WidgetItem, label: &str, pct: f64, format: &NumberFormat) -> String {
    let value = match slider_mode(item) {
        Slider::None => format_percent(pct, format, 1),
        Slider::Full => format!("{} {}", slider_bar(pct), format_percent(pct, format, 1)),
        Slider::Only => slider_bar(pct),
    };
    raw_or_labeled(item, label, &value)
}

pub struct Percentage {
    usable: bool,
}

impl Percentage {
    fn label(&self, inverse: bool) -> &'static str {
        match (self.usable, inverse) {
            (false, false) => "Ctx Used: ",
            (false, true) => "Ctx Left: ",
            (true, false) => "Ctx(u) Used: ",
            (true, true) => "Ctx(u) Left: ",
        }
    }
}

impl Widget for Percentage {
    fn default_color(&self) -> &'static str {
        if self.usable {
            "green"
        } else {
            "blue"
        }
    }
    fn display_name(&self) -> &'static str {
        if self.usable {
            "Context % (usable)"
        } else {
            "Context %"
        }
    }
    fn description(&self) -> &'static str {
        if self.usable {
            "Shows percentage of usable context window used or remaining (80% of max before auto-compact)"
        } else {
            "Shows percentage of context window used or remaining"
        }
    }
    fn category(&self) -> &'static str {
        "Context"
    }
    fn label_prefix(&self, item: &WidgetItem) -> Option<String> {
        Some(self.label(is_flag(item, "inverse")).into())
    }
    fn editor_display(&self, item: &WidgetItem) -> EditorDisplay {
        super::editor::context_percentage_display(self.display_name(), item)
    }
    fn custom_keybinds(&self, _item: &WidgetItem) -> Vec<Keybind> {
        super::editor::context_percentage_keybinds()
    }
    fn handle_editor_action(&self, action: &str, item: &Value) -> Option<Value> {
        super::editor::context_percentage_action(action, item)
    }
    fn supports_number_format(&self) -> bool {
        true
    }
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        let inverse = is_flag(item, "inverse");
        let label = self.label(inverse);
        let format = resolve_number_format("percent", item, ctx.settings);
        let show = |used: f64| {
            let pct = if inverse { 100.0 - used } else { used };
            Some(format_context_percentage(item, label, pct, &format))
        };
        if ctx.is_preview {
            let used = if self.usable { 11.6 } else { 9.3 };
            let pct = if inverse {
                if self.usable {
                    88.4
                } else {
                    90.7
                }
            } else {
                used
            };
            return Some(format_context_percentage(item, label, pct, &format));
        }
        if !self.usable {
            let m = context_percentage_metrics(ctx)?;
            return show(m.used_percentage);
        }
        let metrics = context_window_metrics(ctx.data);
        let cfg = context_config(model_context_identifier(ctx.data).as_deref(), metrics.window_size);
        if let Some(len) = metrics.context_length_tokens {
            return show(min100((len / cfg.usable_tokens) * 100.0));
        }
        let tm = crate::transcript::token_metrics(ctx)?;
        show(min100((tm.context_length / cfg.usable_tokens) * 100.0))
    }
}

pub static LENGTH: Length = Length;
pub static WINDOW: Window = Window;
pub static PERCENTAGE: Percentage = Percentage { usable: false };
pub static PERCENTAGE_USABLE: Percentage = Percentage { usable: true };
