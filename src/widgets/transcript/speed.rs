//! Input / Output / Total Speed (upstream `InputSpeed.ts` …,
//! `widgets/shared/speed-widget.tsx`, `utils/speed-{metrics,window}.ts`).

use crate::context::RenderContext;
use crate::jsnum::parse_int;
use crate::settings::{NumberFormat, WidgetItem};
use crate::transcript::state::SpeedMetrics;
use crate::widgets::shared::{effective_decimals, is_hidden, raw_or_labeled, render_magnitude, resolve_number_format};
use crate::widgets::Widget;

#[derive(Clone, Copy)]
enum Kind {
    Input,
    Output,
    Total,
}

pub struct Speed {
    kind: Kind,
    label: &'static str,
    name: &'static str,
    description: &'static str,
    session_preview: f64,
    windowed_preview: f64,
}

/// `getWidgetSpeedWindowSeconds`
fn window_seconds(item: &WidgetItem) -> f64 {
    let Some(v) = item.meta("windowSeconds").filter(|v| !v.is_empty()) else { return 0.0 };
    match parse_int(v) {
        Some(p) if p.is_finite() => p.trunc().clamp(0.0, 120.0),
        _ => 0.0,
    }
}

/// `formatSpeed`
fn format_speed(tps: Option<f64>, f: &NumberFormat) -> String {
    let Some(v) = tps else { return "—".into() };
    if v >= 1000.0 - 0.5 / 10f64.powi(effective_decimals(f, 1) as i32) {
        return format!("{}k t/s", render_magnitude(v / 1000.0, f, 1));
    }
    format!("{} t/s", render_magnitude(v, f, 1))
}

impl Speed {
    fn speed(&self, m: &SpeedMetrics) -> Option<f64> {
        match self.kind {
            Kind::Input => m.input_speed(),
            Kind::Output => m.output_speed(),
            Kind::Total => m.total_speed(),
        }
    }
}

impl Widget for Speed {
    fn default_color(&self) -> &'static str {
        "cyan"
    }
    fn display_name(&self) -> &'static str {
        self.name
    }
    fn description(&self) -> &'static str {
        self.description
    }
    fn category(&self) -> &'static str {
        "Token Speed"
    }
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        let format = resolve_number_format("speed", item, ctx.settings);
        let window = window_seconds(item);
        if ctx.is_preview {
            let v = if window > 0.0 { self.windowed_preview } else { self.session_preview };
            return Some(raw_or_labeled(item, self.label, &format_speed(Some(v), &format)));
        }
        let metrics = if window > 0.0 {
            crate::transcript::speed_windowed(ctx, window)
        } else {
            crate::transcript::speed_session(ctx)
        }?;
        let speed = self.speed(&metrics);
        if speed.is_none() && is_hidden(item, "no-data", false) {
            return None;
        }
        Some(raw_or_labeled(item, self.label, &format_speed(speed, &format)))
    }
}

pub static INPUT: Speed = Speed {
    kind: Kind::Input,
    label: "In: ",
    name: "Input Speed",
    description: "Shows session-average input token speed (tokens/sec). Optional window: 0-120 seconds (0 = full-session average).",
    session_preview: 85.2,
    windowed_preview: 31.5,
};

pub static OUTPUT: Speed = Speed {
    kind: Kind::Output,
    label: "Out: ",
    name: "Output Speed",
    description: "Shows session-average output token speed (tokens/sec). Optional window: 0-120 seconds (0 = full-session average).",
    session_preview: 42.5,
    windowed_preview: 26.8,
};

pub static TOTAL: Speed = Speed {
    kind: Kind::Total,
    label: "Total: ",
    name: "Total Speed",
    description: "Shows session-average total token speed (tokens/sec). Optional window: 0-120 seconds (0 = full-session average).",
    session_preview: 127.7,
    windowed_preview: 58.3,
};
