//! Widget group: misc — session/environment/custom widgets (upstream
//! `src/widgets/{OutputStyle,CurrentWorkingDir,SessionCost,SessionCostRate,
//! TerminalWidth,Version,CustomText,CustomSymbol,CustomCommand,Link,
//! ClaudeSessionId,SandboxStatus,FreeMemory,VimMode,VoiceStatus,
//! RemoteControlStatus}.ts`), plus the herdr widgets (not upstream).

mod custom_command;
mod cwd;
pub(crate) mod editor;
mod herdr;
mod link;
mod toggles;

use super::shared::{format_cost, is_flag, is_hidden, raw_or_labeled, render_magnitude, resolve_number_format};
use super::Widget;
use crate::context::RenderContext;
use crate::settings::{NumberFormat, WidgetItem};

/// The identity methods every widget repeats.
macro_rules! widget_info {
    ($color:expr, $name:expr, $desc:expr, $cat:expr) => {
        fn default_color(&self) -> &'static str {
            $color
        }
        fn display_name(&self) -> &'static str {
            $name
        }
        fn description(&self) -> &'static str {
            $desc
        }
        fn category(&self) -> &'static str {
            $cat
        }
    };
}
pub(crate) use widget_info;

// ------------------------------------------------------------- output-style

pub struct OutputStyle;

impl Widget for OutputStyle {
    widget_info!("cyan", "Output Style", "Shows the current Claude Code output style", "Core");
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        if ctx.is_preview {
            return Some(raw_or_labeled(item, "Style: ", "default"));
        }
        let name = ctx.data.output_style_name.as_deref().filter(|n| !n.is_empty())?;
        if name == "default" && is_hidden(item, "default-value", false) {
            return None;
        }
        Some(raw_or_labeled(item, "Style: ", name))
    }
}

// ------------------------------------------------------------- session-cost

pub struct SessionCost;

impl Widget for SessionCost {
    widget_info!("green", "Session Cost", "Shows the total session cost in USD", "Session");
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        let format = resolve_number_format("cost", item, ctx.settings);
        if ctx.is_preview {
            return Some(raw_or_labeled(item, "Cost: ", &format_cost(2.45, &format)));
        }
        let total = ctx.data.cost.as_ref()?.total_cost_usd?;
        // the zero state is tied to the baseline cent precision
        let rounds_to_zero_cents = (0.0..0.005).contains(&total);
        if rounds_to_zero_cents && is_hidden(item, "zero", false) {
            return None;
        }
        Some(raw_or_labeled(item, "Cost: ", &format_cost(total, &format)))
    }
}

// -------------------------------------------------------- session-cost-rate

pub struct SessionCostRate;

const HOUR_MS: f64 = 60.0 * 60.0 * 1000.0;
const MIN_RATE_DURATION_MS: f64 = 60.0 * 1000.0;

impl Widget for SessionCostRate {
    widget_info!(
        "green",
        "Session Cost Rate",
        "Shows the session cost per hour, over the time Claude spent working or the whole session",
        "Session"
    );
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        let format = resolve_number_format("cost", item, ctx.settings);
        if ctx.is_preview {
            return Some(raw_or_labeled(item, "Rate: ", &format!("{}/hr", format_cost(4.9, &format))));
        }
        let cost = ctx.data.cost.as_ref()?;
        let total = cost.total_cost_usd?;
        let duration = if is_flag(item, "clockTime") { cost.total_duration_ms } else { cost.total_api_duration_ms }?;
        if duration < MIN_RATE_DURATION_MS {
            return None;
        }
        let per_hour = total / (duration / HOUR_MS);
        Some(raw_or_labeled(item, "Rate: ", &format!("{}/hr", format_cost(per_hour, &format))))
    }
}

// ----------------------------------------------------------- terminal-width

pub struct TerminalWidth;

impl Widget for TerminalWidth {
    widget_info!("gray", "Terminal Width", "Shows current terminal width in columns", "Environment");
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        let width = ctx.terminal_width.or_else(crate::terminal::memoized_width);
        if ctx.is_preview {
            let w = width.map(|w| w.to_string()).unwrap_or_else(|| "??".into());
            return Some(raw_or_labeled(item, "Term: ", &w));
        }
        let w = width.filter(|w| *w > 0)?;
        Some(raw_or_labeled(item, "Term: ", &w.to_string()))
    }
}

// ------------------------------------------------------------------ version

pub struct Version;

impl Widget for Version {
    widget_info!("gray", "Version", "Shows Claude Code CLI version number", "Core");
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        if ctx.is_preview {
            return Some(if item.raw() { "1.0.0" } else { "v1.0.0" }.into());
        }
        let v = ctx.data.version.as_deref().filter(|v| !v.is_empty())?;
        Some(if item.raw() { v.to_string() } else { format!("v{v}") })
    }
}

// ------------------------------------------------------ custom text/symbol

pub struct CustomText;

impl Widget for CustomText {
    widget_info!("white", "Custom Text", "Displays user-defined custom text", "Custom");
    fn render(&self, item: &WidgetItem, _ctx: &RenderContext) -> Option<String> {
        Some(item.custom_text.clone().unwrap_or_default())
    }
    fn supports_raw_value(&self) -> bool {
        false
    }
}

pub struct CustomSymbol;

impl Widget for CustomSymbol {
    widget_info!("white", "Custom Symbol", "Displays a custom symbol or emoji (single character)", "Custom");
    fn render(&self, item: &WidgetItem, _ctx: &RenderContext) -> Option<String> {
        Some(item.custom_symbol.clone().unwrap_or_default())
    }
    fn supports_raw_value(&self) -> bool {
        false
    }
}

// -------------------------------------------------------- claude-session-id

pub struct ClaudeSessionId;

impl Widget for ClaudeSessionId {
    widget_info!(
        "cyan",
        "Claude Session ID",
        "Shows the current Claude Code session ID reported in status JSON",
        "Core"
    );
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        if ctx.is_preview {
            return Some(raw_or_labeled(item, "Session ID: ", "preview-session-id"));
        }
        let sid = ctx.data.session_id.as_deref().filter(|s| !s.is_empty())?;
        Some(raw_or_labeled(item, "Session ID: ", sid))
    }
}

// -------------------------------------------------------------- free-memory

pub struct FreeMemory;

/// `formatBytes(bytes, format)`
fn format_bytes(bytes: f64, f: &NumberFormat) -> String {
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const KB: f64 = 1024.0;
    if bytes >= GB {
        format!("{}G", render_magnitude(bytes / GB, f, 1))
    } else if bytes >= MB {
        format!("{}M", render_magnitude(bytes / MB, f, 0))
    } else if bytes >= KB {
        format!("{}K", render_magnitude(bytes / KB, f, 0))
    } else {
        format!("{}B", crate::jsnum::to_js_string(bytes))
    }
}

impl Widget for FreeMemory {
    widget_info!("cyan", "Memory Usage", "Shows system memory usage (used/total)", "Environment");
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        let format = resolve_number_format("memory", item, ctx.settings);
        if ctx.is_preview {
            let gb = 1024.0f64.powi(3);
            let v = format!("{}/{}", format_bytes(12.4 * gb, &format), format_bytes(16.0 * gb, &format));
            return Some(raw_or_labeled(item, "Mem: ", &v));
        }
        let m = crate::misc::memory::read();
        let v = format!("{}/{}", format_bytes(m.used, &format), format_bytes(m.total, &format));
        Some(raw_or_labeled(item, "Mem: ", &v))
    }
}

// ------------------------------------------------------------------ registry

static OUTPUT_STYLE: OutputStyle = OutputStyle;
static SESSION_COST: SessionCost = SessionCost;
static SESSION_COST_RATE: SessionCostRate = SessionCostRate;
static TERMINAL_WIDTH: TerminalWidth = TerminalWidth;
static VERSION: Version = Version;
static CUSTOM_TEXT: CustomText = CustomText;
static CUSTOM_SYMBOL: CustomSymbol = CustomSymbol;
static CLAUDE_SESSION_ID: ClaudeSessionId = ClaudeSessionId;
static FREE_MEMORY: FreeMemory = FreeMemory;

pub fn lookup(kind: &str) -> Option<&'static dyn Widget> {
    Some(match kind {
        "output-style" => &OUTPUT_STYLE,
        "current-working-dir" => &cwd::CURRENT_WORKING_DIR,
        "session-cost" => &SESSION_COST,
        "session-cost-rate" => &SESSION_COST_RATE,
        "terminal-width" => &TERMINAL_WIDTH,
        "version" => &VERSION,
        "custom-text" => &CUSTOM_TEXT,
        "custom-symbol" => &CUSTOM_SYMBOL,
        "custom-command" => &custom_command::CUSTOM_COMMAND,
        "link" => &link::LINK,
        "claude-session-id" => &CLAUDE_SESSION_ID,
        "sandbox-status" => &toggles::SANDBOX_STATUS,
        "free-memory" => &FREE_MEMORY,
        "vim-mode" => &toggles::VIM_MODE,
        "voice-status" => &toggles::VOICE_STATUS,
        "remote-control-status" => &toggles::REMOTE_CONTROL_STATUS,
        // not upstream
        "herdr-workspace" => &herdr::HERDR_WORKSPACE,
        "herdr-tab" => &herdr::HERDR_TAB,
        "herdr-pane" => &herdr::HERDR_PANE,
        _ => return None,
    })
}

#[cfg(test)]
pub(crate) mod test_util {
    use crate::settings::{Settings, WidgetItem};
    use crate::status::StatusJson;

    pub fn item(kind: &str) -> WidgetItem {
        WidgetItem { id: "1".into(), kind: kind.into(), ..Default::default() }
    }

    pub fn with_meta(mut item: WidgetItem, pairs: &[(&str, &str)]) -> WidgetItem {
        let m = item.metadata.get_or_insert_with(Default::default);
        for (k, v) in pairs {
            m.insert(k.to_string(), v.to_string());
        }
        item
    }

    pub fn status(json: &str) -> StatusJson {
        crate::status::parse_status(serde_json::from_str(json).unwrap()).unwrap()
    }

    pub fn settings() -> Settings {
        Settings::default()
    }
}

#[cfg(test)]
mod tests {
    use super::test_util::*;
    use super::*;

    fn render(kind: &str, item: &WidgetItem, json: &str) -> Option<String> {
        let data = status(json);
        let s = settings();
        let ctx = RenderContext::new(&data, &s, None);
        lookup(kind).unwrap().render(item, &ctx)
    }

    #[test]
    fn cost_rate() {
        let j = r#"{"cost":{"total_cost_usd":1.5,"total_api_duration_ms":1800000,"total_duration_ms":59999}}"#;
        assert_eq!(render("session-cost-rate", &item("session-cost-rate"), j).as_deref(), Some("Rate: $3.00/hr"));
        let clock = with_meta(item("session-cost-rate"), &[("clockTime", "true")]);
        assert_eq!(render("session-cost-rate", &clock, j), None);
    }

    #[test]
    fn cost_zero_hidden() {
        let it = with_meta(item("session-cost"), &[("hide", "zero")]);
        assert_eq!(render("session-cost", &it, r#"{"cost":{"total_cost_usd":0.004}}"#), None);
        assert_eq!(render("session-cost", &it, r#"{"cost":{"total_cost_usd":0.005}}"#).as_deref(), Some("Cost: $0.01"));
    }

    #[test]
    fn bytes() {
        let f = NumberFormat::default();
        assert_eq!(format_bytes(512.0, &f), "512B");
        assert_eq!(format_bytes(f64::NAN, &f), "NaNB");
        assert_eq!(format_bytes(1536.0, &f), "2K");
        assert_eq!(format_bytes(5683281920.0, &f), "5.3G");
    }
}
