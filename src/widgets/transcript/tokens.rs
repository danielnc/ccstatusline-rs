//! Tokens Input/Output/Cached/Total (upstream `TokensInput.ts` …,
//! `widgets/shared/token-count-widget.ts`).

use crate::context::RenderContext;
use crate::settings::WidgetItem;
use crate::widgets::shared::{context_window_metrics, format_tokens, is_hidden, raw_or_labeled, resolve_number_format};
use crate::widgets::Widget;

pub struct TokenCount {
    label: &'static str,
    preview: f64,
    color: &'static str,
    name: &'static str,
    description: &'static str,
    count: fn(&RenderContext) -> Option<f64>,
}

impl Widget for TokenCount {
    fn default_color(&self) -> &'static str {
        self.color
    }
    fn display_name(&self) -> &'static str {
        self.name
    }
    fn description(&self) -> &'static str {
        self.description
    }
    fn category(&self) -> &'static str {
        "Tokens"
    }
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        let format = resolve_number_format("token", item, ctx.settings);
        if ctx.is_preview {
            return Some(raw_or_labeled(item, self.label, &format_tokens(self.preview, &format, 1)));
        }
        let tokens = (self.count)(ctx)?;
        if tokens == 0.0 && is_hidden(item, "zero", false) {
            return None;
        }
        Some(raw_or_labeled(item, self.label, &format_tokens(tokens, &format, 1)))
    }
}

fn metrics(ctx: &RenderContext) -> Option<crate::transcript::TokenMetrics> {
    crate::transcript::token_metrics(ctx)
}

pub static INPUT: TokenCount = TokenCount {
    label: "In: ",
    preview: 15200.0,
    color: "blue",
    name: "Tokens Input",
    description: "Shows input token count for the current session",
    count: |ctx| metrics(ctx).map(|m| m.input_tokens).or_else(|| context_window_metrics(ctx.data).total_input_tokens),
};

pub static OUTPUT: TokenCount = TokenCount {
    label: "Out: ",
    preview: 3400.0,
    color: "white",
    name: "Tokens Output",
    description: "Shows output token count for the current session",
    count: |ctx| metrics(ctx).map(|m| m.output_tokens).or_else(|| context_window_metrics(ctx.data).total_output_tokens),
};

pub static CACHED: TokenCount = TokenCount {
    label: "Cached: ",
    preview: 12000.0,
    color: "cyan",
    name: "Tokens Cached",
    description: "Shows cached token count for the current session",
    count: |ctx| metrics(ctx).map(|m| m.cached_tokens),
};

pub static TOTAL: TokenCount = TokenCount {
    label: "Total: ",
    preview: 30600.0,
    color: "cyan",
    name: "Tokens Total",
    description: "Shows total token count (input + output + cache) for the current session",
    count: |ctx| metrics(ctx).map(|m| m.total_tokens),
};
