//! Format-cycling status widgets: sandbox-status, voice-status,
//! remote-control-status and vim-mode (upstream `src/widgets/{SandboxStatus,
//! VoiceStatus,RemoteControlStatus,VimMode}.ts`).

use super::widget_info;
use crate::context::RenderContext;
use crate::jsobj;
use crate::misc::claude_layers;
use crate::settings::WidgetItem;
use crate::widgets::editor::{self, format_keybinds, handle_format_action, FormatOptions};
use crate::widgets::shared::{get_format, nerd_font_enabled};
use crate::widgets::{EditorDisplay, Keybind, Widget};
use serde_json::Value;

// Upstream's `canUseNerdFont` predicates read only the resolved format and `rawValue`. Each widget keeps one
// predicate over those two, used by render and the editor display, and wraps it for `FormatOptions`.

fn raw_value(item: &Value) -> bool {
    jsobj::get_bool(item, "rawValue") == Some(true)
}

/// The resolved format and `canUseNerdFont` for a typed item.
fn format_state(item: &WidgetItem, options: &FormatOptions, can_use: fn(&str, bool) -> bool) -> (&'static str, bool) {
    let format = get_format(item, options.formats, options.default_format);
    (format, can_use(format, item.raw()))
}

/// `getFormatModifierText`, e.g. "(icon, nerd font)".
fn format_modifier_text(item: &WidgetItem, options: &FormatOptions, can_use: fn(&str, bool) -> bool) -> String {
    let (format, can_use) = format_state(item, options, can_use);
    if nerd_font_enabled(item, can_use) {
        format!("({format}, nerd font)")
    } else {
        format!("({format})")
    }
}

fn format_keybinds_for(item: &WidgetItem, options: &FormatOptions, can_use: fn(&str, bool) -> bool) -> Vec<Keybind> {
    format_keybinds(format_state(item, options, can_use).1)
}

const STATE_DOT_OFF: &str = "○";
const STATE_DOT_ON: &str = "◉";

// ----------------------------------------------------------- sandbox-status

pub struct SandboxStatus;
pub static SANDBOX_STATUS: SandboxStatus = SandboxStatus;

const SANDBOX_FORMATS: &[&str] = &["glyph", "text", "word"];

fn sandbox_can_use_nerd_font(format: &str, _raw: bool) -> bool {
    format == "glyph"
}

fn sandbox_options_can_use(item: &Value) -> bool {
    sandbox_can_use_nerd_font(editor::get_format(item, &SANDBOX_OPTIONS), raw_value(item))
}

const SANDBOX_OPTIONS: FormatOptions =
    FormatOptions { formats: SANDBOX_FORMATS, default_format: "glyph", can_use_nerd_font: sandbox_options_can_use };

fn sandbox_text(enabled: bool, format: &str, nerd: bool, raw: bool) -> String {
    let state = if enabled { "ON" } else { "OFF" };
    let glyph = match (nerd, enabled) {
        (true, true) => "\u{f023}",
        (true, false) => "\u{f09c}",
        (false, true) => "●",
        (false, false) => "○",
    };
    match format {
        "glyph" => {
            if raw {
                glyph.to_string()
            } else {
                format!("SB: {glyph}")
            }
        }
        "text" => {
            if raw {
                state.to_string()
            } else {
                format!("SB: {state}")
            }
        }
        _ => {
            if raw {
                state.to_string()
            } else {
                format!("Sandbox: {state}")
            }
        }
    }
}

impl Widget for SandboxStatus {
    widget_info!(
        "green",
        "Sandbox Status",
        "Shows whether Claude Code bash sandbox mode is enabled\nBest effort: may not reflect active sandboxing when managed or CLI settings override it, or when sandbox initialization fails.",
        "Core"
    );
    fn editor_display(&self, item: &WidgetItem) -> EditorDisplay {
        EditorDisplay {
            text: self.display_name().into(),
            modifier: Some(format_modifier_text(item, &SANDBOX_OPTIONS, sandbox_can_use_nerd_font)),
        }
    }
    fn custom_keybinds(&self, item: &WidgetItem) -> Vec<Keybind> {
        format_keybinds_for(item, &SANDBOX_OPTIONS, sandbox_can_use_nerd_font)
    }
    fn handle_editor_action(&self, action: &str, item: &Value) -> Option<Value> {
        handle_format_action(action, item, &SANDBOX_OPTIONS)
    }
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        let format = get_format(item, SANDBOX_FORMATS, "glyph");
        let nerd = nerd_font_enabled(item, sandbox_can_use_nerd_font(format, item.raw()));
        if ctx.is_preview {
            return Some(sandbox_text(true, format, nerd, item.raw()));
        }
        let enabled = claude_layers::layered_enabled(claude_layers::resolve_config_cwd(ctx), "sandbox")?;
        Some(sandbox_text(enabled, format, nerd, item.raw()))
    }
}

// ------------------------------------------- voice-status / remote-control

const ICON_FORMATS_VOICE: &[&str] = &["icon", "icon-text", "text", "word"];
const ICON_FORMATS_REMOTE: &[&str] = &["icon", "icon-text", "text", "word", "label-check", "label-mark"];

/// Shared by voice and remote control: `canUseNerdFont`.
fn icon_can_use_nerd_font(format: &str, raw: bool) -> bool {
    format == "icon" || (format == "icon-text" && !raw)
}

fn voice_options_can_use(item: &Value) -> bool {
    icon_can_use_nerd_font(editor::get_format(item, &VOICE_OPTIONS), raw_value(item))
}

fn remote_options_can_use(item: &Value) -> bool {
    icon_can_use_nerd_font(editor::get_format(item, &REMOTE_OPTIONS), raw_value(item))
}

const VOICE_OPTIONS: FormatOptions =
    FormatOptions { formats: ICON_FORMATS_VOICE, default_format: "icon", can_use_nerd_font: voice_options_can_use };
const REMOTE_OPTIONS: FormatOptions =
    FormatOptions { formats: ICON_FORMATS_REMOTE, default_format: "icon", can_use_nerd_font: remote_options_can_use };

struct Icons {
    emoji: &'static str,
    nerd_on: &'static str,
    nerd_off: &'static str,
    word: &'static str,
}

const VOICE_ICONS: Icons = Icons { emoji: "🎤", nerd_on: "\u{f130}", nerd_off: "\u{f131}", word: "voice" };
const REMOTE_ICONS: Icons = Icons { emoji: "📡", nerd_on: "\u{f1eb}", nerd_off: "\u{f6ac}", word: "remote" };

fn icon_status_text(icons: &Icons, enabled: bool, format: &str, nerd: bool, raw: bool) -> String {
    let state = if enabled { "on" } else { "off" };
    let dot = if enabled { STATE_DOT_ON } else { STATE_DOT_OFF };
    let icon = if nerd {
        if enabled {
            icons.nerd_on
        } else {
            icons.nerd_off
        }
    } else {
        icons.emoji
    };
    let word = icons.word;
    match format {
        "icon" => {
            if nerd {
                icon.to_string()
            } else if raw {
                dot.to_string()
            } else {
                format!("{icon} {dot}")
            }
        }
        "icon-text" => {
            if raw {
                state.to_string()
            } else {
                format!("{icon} {state}")
            }
        }
        "text" => state.to_string(),
        "word" => {
            if raw {
                state.to_string()
            } else {
                format!("{word} {state}")
            }
        }
        "label-check" => {
            let m = if enabled { "✅" } else { "❌" };
            if raw {
                m.to_string()
            } else {
                format!("{word} {m}")
            }
        }
        _ => {
            let m = if enabled { "✓" } else { "✗" };
            if raw {
                m.to_string()
            } else {
                format!("{word} {m}")
            }
        }
    }
}

pub struct VoiceStatus;
pub static VOICE_STATUS: VoiceStatus = VoiceStatus;

impl Widget for VoiceStatus {
    widget_info!("magenta", "Voice Status", "Shows whether Claude Code voice input is enabled", "Core");
    fn editor_display(&self, item: &WidgetItem) -> EditorDisplay {
        EditorDisplay {
            text: self.display_name().into(),
            modifier: Some(format_modifier_text(item, &VOICE_OPTIONS, icon_can_use_nerd_font)),
        }
    }
    fn custom_keybinds(&self, item: &WidgetItem) -> Vec<Keybind> {
        format_keybinds_for(item, &VOICE_OPTIONS, icon_can_use_nerd_font)
    }
    fn handle_editor_action(&self, action: &str, item: &Value) -> Option<Value> {
        handle_format_action(action, item, &VOICE_OPTIONS)
    }
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        let format = get_format(item, ICON_FORMATS_VOICE, "icon");
        let nerd = nerd_font_enabled(item, icon_can_use_nerd_font(format, item.raw()));
        if ctx.is_preview {
            return Some(icon_status_text(&VOICE_ICONS, true, format, nerd, item.raw()));
        }
        let enabled = claude_layers::layered_enabled(claude_layers::resolve_config_cwd(ctx), "voice")?;
        Some(icon_status_text(&VOICE_ICONS, enabled, format, nerd, item.raw()))
    }
}

pub struct RemoteControlStatus;
pub static REMOTE_CONTROL_STATUS: RemoteControlStatus = RemoteControlStatus;

impl Widget for RemoteControlStatus {
    widget_info!(
        "blue",
        "Remote Control Status",
        "Shows whether Claude Code remote control is attached to the current session",
        "Core"
    );
    fn editor_display(&self, item: &WidgetItem) -> EditorDisplay {
        EditorDisplay {
            text: self.display_name().into(),
            modifier: Some(format_modifier_text(item, &REMOTE_OPTIONS, icon_can_use_nerd_font)),
        }
    }
    fn custom_keybinds(&self, item: &WidgetItem) -> Vec<Keybind> {
        format_keybinds_for(item, &REMOTE_OPTIONS, icon_can_use_nerd_font)
    }
    fn handle_editor_action(&self, action: &str, item: &Value) -> Option<Value> {
        handle_format_action(action, item, &REMOTE_OPTIONS)
    }
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        let format = get_format(item, ICON_FORMATS_REMOTE, "icon");
        let nerd = nerd_font_enabled(item, icon_can_use_nerd_font(format, item.raw()));
        if ctx.is_preview {
            return Some(icon_status_text(&REMOTE_ICONS, true, format, nerd, item.raw()));
        }
        let enabled = claude_layers::remote_control_enabled(ctx.data.session_id.as_deref())?;
        Some(icon_status_text(&REMOTE_ICONS, enabled, format, nerd, item.raw()))
    }
}

// ----------------------------------------------------------------- vim-mode

pub struct VimMode;
pub static VIM_MODE: VimMode = VimMode;

const VIM_FORMATS: &[&str] = &["icon-dash-letter", "icon-letter", "icon", "letter", "word"];

fn vim_can_use_nerd_font(format: &str, _raw: bool) -> bool {
    matches!(format, "icon-dash-letter" | "icon-letter" | "icon")
}

fn vim_options_can_use(item: &Value) -> bool {
    vim_can_use_nerd_font(editor::get_format(item, &VIM_OPTIONS), raw_value(item))
}

const VIM_OPTIONS: FormatOptions =
    FormatOptions { formats: VIM_FORMATS, default_format: "icon-dash-letter", can_use_nerd_font: vim_options_can_use };

fn vim_text(mode: &str, format: &str, icon: &str) -> String {
    let letter = match mode {
        "NORMAL" => "N".to_string(),
        "INSERT" => "I".to_string(),
        // `mode[0] ?? mode`
        _ => crate::misc::first_utf16_unit(mode).unwrap_or_else(|| mode.to_string()),
    };
    match format {
        "icon-dash-letter" => format!("{icon}-{letter}"),
        "icon-letter" => format!("{icon} {letter}"),
        "icon" => icon.to_string(),
        "letter" => letter,
        _ => mode.to_string(),
    }
}

impl Widget for VimMode {
    widget_info!("green", "Vim Mode", "Displays current vim editor mode", "Core");
    fn editor_display(&self, item: &WidgetItem) -> EditorDisplay {
        EditorDisplay {
            text: self.display_name().into(),
            modifier: Some(format_modifier_text(item, &VIM_OPTIONS, vim_can_use_nerd_font)),
        }
    }
    fn custom_keybinds(&self, item: &WidgetItem) -> Vec<Keybind> {
        format_keybinds_for(item, &VIM_OPTIONS, vim_can_use_nerd_font)
    }
    fn handle_editor_action(&self, action: &str, item: &Value) -> Option<Value> {
        handle_format_action(action, item, &VIM_OPTIONS)
    }
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        let format = get_format(item, VIM_FORMATS, "icon-dash-letter");
        let icon = if nerd_font_enabled(item, vim_can_use_nerd_font(format, item.raw())) { "\u{e62b}" } else { "v" };
        if ctx.is_preview {
            return Some(vim_text("NORMAL", format, icon));
        }
        let mode = ctx.data.vim_mode.as_deref()?;
        Some(vim_text(mode, format, icon))
    }
    fn supports_raw_value(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_formats() {
        assert_eq!(icon_status_text(&VOICE_ICONS, false, "icon", false, false), "🎤 ○");
        assert_eq!(icon_status_text(&VOICE_ICONS, true, "icon", true, false), "\u{f130}");
        assert_eq!(icon_status_text(&REMOTE_ICONS, false, "label-mark", false, false), "remote ✗");
        assert_eq!(icon_status_text(&REMOTE_ICONS, true, "label-check", false, true), "✅");
    }

    #[test]
    fn vim_letters() {
        assert_eq!(vim_text("VISUAL", "icon-dash-letter", "v"), "v-V");
        assert_eq!(vim_text("", "letter", "v"), "");
        assert_eq!(vim_text("INSERT", "word", "v"), "INSERT");
    }
}
