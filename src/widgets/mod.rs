//! Widget trait and registry (upstream `src/types/Widget.ts`,
//! `src/utils/widgets.ts`, `src/utils/widget-manifest.ts`).
//!
//! Widgets are grouped by data source; each group module exposes
//! `lookup(kind)` and the registry chains them.

pub mod catalog;
pub mod core;
pub mod editor;
#[cfg(test)]
mod editor_meta_tests;
pub mod extra;
pub mod git;
mod git_editor;
pub mod misc;
pub mod shared;
pub mod transcript;
pub mod usage;
mod usage_editor;

use crate::context::RenderContext;
use crate::settings::WidgetItem;

/// `getEditorDisplay`'s result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditorDisplay {
    pub text: String,
    pub modifier: Option<String>,
}

impl EditorDisplay {
    pub fn plain(text: impl Into<String>) -> Self {
        EditorDisplay { text: text.into(), modifier: None }
    }
}

/// A condition under which a widget can hide (`HideableState`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HideableState {
    pub key: &'static str,
    pub label: &'static str,
    pub default_enabled: bool,
}

/// An editor key binding (`CustomKeybind`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keybind {
    pub key: String,
    pub label: String,
    pub action: String,
}

impl Keybind {
    pub fn new(key: &str, label: &str, action: &str) -> Self {
        Keybind { key: key.into(), label: label.into(), action: action.into() }
    }
}

pub trait Widget: Sync {
    fn default_color(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn category(&self) -> &'static str;
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String>;
    /// The rendered text carries its own foreground colors.
    fn preserves_rendered_colors(&self, _item: &WidgetItem) -> bool {
        false
    }
    fn supports_raw_value(&self) -> bool {
        true
    }
    fn supports_colors(&self, _item: &WidgetItem) -> bool {
        true
    }
    /// Claude Code hooks this widget needs: (event, matcher).
    fn hooks(&self) -> &'static [(&'static str, Option<&'static str>)] {
        &[]
    }

    // --- The config editor (upstream's optional Widget methods) --------------------------------------------

    /// `getEditorDisplay(item)`.
    fn editor_display(&self, _item: &WidgetItem) -> EditorDisplay {
        EditorDisplay::plain(self.display_name())
    }
    /// `supportsNumberFormat()`.
    fn supports_number_format(&self) -> bool {
        false
    }
    /// `getHideableStates()`.
    fn hideable_states(&self) -> &'static [HideableState] {
        &[]
    }
    /// `getLabelPrefix(item)`, for widgets that declare one.
    fn label_prefix(&self, _item: &WidgetItem) -> Option<String> {
        None
    }
    /// `getCustomKeybinds(item)`.
    fn custom_keybinds(&self, _item: &WidgetItem) -> Vec<Keybind> {
        Vec::new()
    }
    /// `handleEditorAction(action, item)` on the item's JSON object (edited with `crate::jsobj`, so key order
    /// matches upstream's); None is upstream's `null` (or a widget without the method).
    fn handle_editor_action(&self, _action: &str, _item: &serde_json::Value) -> Option<serde_json::Value> {
        None
    }
}

/// Every upstream widget type, in manifest order.
pub const UPSTREAM_TYPES: &[&str] = &[
    "model",
    "output-style",
    "git-branch",
    "git-changes",
    "git-insertions",
    "git-deletions",
    "git-staged-files",
    "git-unstaged-files",
    "git-untracked-files",
    "git-clean-status",
    "git-root-dir",
    "git-review",
    "git-ci-status",
    "git-worktree",
    "git-status",
    "git-staged",
    "git-unstaged",
    "git-untracked",
    "git-ahead-behind",
    "git-conflicts",
    "git-sha",
    "git-origin-owner",
    "git-origin-repo",
    "git-origin-owner-repo",
    "git-upstream-owner",
    "git-upstream-repo",
    "git-upstream-owner-repo",
    "git-is-fork",
    "jj-bookmarks",
    "jj-workspace",
    "jj-root-dir",
    "jj-changes",
    "jj-insertions",
    "jj-deletions",
    "jj-description",
    "jj-revision",
    "current-working-dir",
    "tokens-input",
    "tokens-output",
    "tokens-cached",
    "tokens-total",
    "cache-hit-rate",
    "cache-read",
    "cache-write",
    "input-speed",
    "output-speed",
    "total-speed",
    "context-length",
    "context-window",
    "context-percentage",
    "context-percentage-usable",
    "session-clock",
    "session-cost",
    "session-cost-rate",
    "block-timer",
    "terminal-width",
    "version",
    "custom-text",
    "custom-symbol",
    "custom-command",
    "link",
    "claude-session-id",
    "claude-account-email",
    "claude-status",
    "sandbox-status",
    "session-name",
    "free-memory",
    "session-usage",
    "weekly-usage",
    "extra-usage-utilization",
    "extra-usage-remaining",
    "extra-usage-used",
    "extra-usage-daily-budget",
    "weekly-sonnet-usage",
    "weekly-opus-usage",
    "fable-weekly-usage",
    "reset-timer",
    "weekly-reset-timer",
    "context-bar",
    "skills",
    "thinking-effort",
    "vim-mode",
    "voice-status",
    "remote-control-status",
    "worktree-mode",
    "worktree-name",
    "worktree-branch",
    "worktree-original-branch",
    "compaction-counter",
    "cache-timer",
];

pub fn resolve_legacy_type(kind: &str) -> &str {
    if kind == "git-pr" {
        "git-review"
    } else {
        kind
    }
}

pub fn get_widget(kind: &str) -> Option<&'static dyn Widget> {
    let kind = resolve_legacy_type(kind);
    core::lookup(kind)
        .or_else(|| git::lookup(kind))
        .or_else(|| transcript::lookup(kind))
        .or_else(|| usage::lookup(kind))
        .or_else(|| misc::lookup(kind))
        .or_else(|| extra::lookup(kind))
}

pub fn widget_preserves_colors(item: &WidgetItem) -> bool {
    get_widget(&item.kind).is_some_and(|w| w.preserves_rendered_colors(item))
}
