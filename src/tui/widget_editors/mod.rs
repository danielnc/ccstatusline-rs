//! Widgets' own editors (upstream `renderEditor`), opened by the items editor for a key binding whose
//! `handleEditorAction` returns null. Shared editors live in `shared`; each widget group maps its types and actions
//! to editors in its own module.

pub mod git;
pub mod misc;
pub mod shared;
mod transcript;
pub mod usage;

use crate::tui::components::hide_label::Done;
use crate::tui::ink::keys::Input;
use crate::tui::ink::layout::El;
use serde_json::Value;

/// A widget editor: the component upstream's `renderEditor` returns, with `onComplete` / `onCancel` as [`Done`].
pub trait WidgetEditor {
    fn handle(&mut self, ev: &Input) -> Done;
    fn view(&mut self) -> El;
}

/// The editor `renderEditor({widget, action})` shows for this widget type and action, or None where upstream's
/// renders nothing (the items editor then shows a blank screen that ESC leaves).
pub fn open(kind: &str, action: &str, item: &Value) -> Option<Box<dyn WidgetEditor>> {
    match kind {
        "reset-timer" | "weekly-reset-timer" => usage::open(kind, action, item),
        _ => git::open(kind, action, item)
            .or_else(|| misc::open(kind, action, item))
            .or_else(|| transcript::open(kind, action, item)),
    }
}
