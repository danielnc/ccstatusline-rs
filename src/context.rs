//! Render context (upstream `src/types/RenderContext.ts`).
//!
//! Upstream computes every data source (transcript analysis, usage fetch, …)
//! eagerly before rendering. Here each source is computed lazily, on first
//! use by a widget that needs it, and memoized for the process; see the
//! provider modules (`transcript`, `usage`, `git`).

use crate::settings::Settings;
use crate::status::StatusJson;

pub struct RenderContext<'a> {
    pub data: &'a StatusJson,
    pub settings: &'a Settings,
    /// `context.terminalWidth` (the detected width, before flex-mode reserve)
    pub terminal_width: Option<usize>,
    pub is_preview: bool,
    pub minimalist: bool,
    pub line_index: Option<usize>,
    pub global_separator_index: usize,
    pub global_powerline_theme_index: usize,
    pub global_powerline_start_cap_index: Option<usize>,
}

impl<'a> RenderContext<'a> {
    pub fn new(data: &'a StatusJson, settings: &'a Settings, terminal_width: Option<usize>) -> Self {
        RenderContext {
            data,
            settings,
            terminal_width,
            is_preview: false,
            minimalist: settings.minimalist_mode,
            line_index: None,
            global_separator_index: 0,
            global_powerline_theme_index: 0,
            global_powerline_start_cap_index: None,
        }
    }

    /// Whether any configured widget has this type.
    pub fn has_widget(&self, kind: &str) -> bool {
        self.settings.lines.iter().any(|l| l.iter().any(|w| w.kind == kind))
    }
}
