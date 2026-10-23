//! The contract between the App and the screens it hosts below the preview.
//!
//! A screen gets the App's state in a [`Ctx`] (upstream's props) and answers a keypress with [`Action`]s (upstream's
//! callbacks: `onUpdate`, `onBack`, ...), which the App applies in order. Each screen is mounted afresh when the App
//! switches to it, as a React component is.

use super::ink::keys::Input;
use super::ink::layout::El;
use crate::settings::Settings;
use serde_json::Value;

/// What a screen reads from the App.
pub struct Ctx<'a> {
    /// The settings object as upstream holds it (edit with `crate::jsobj`, so key order matches).
    pub settings: &'a Value,
    /// The same settings, typed.
    pub typed: &'a Settings,
    /// `process.stdout.columns`.
    pub terminal_width: usize,
    /// The line the colors screen edits.
    pub selected_line: usize,
}

impl Ctx<'_> {
    /// `settings.lines[selectedLine] ?? []`.
    pub fn line_widgets(&self) -> Vec<Value> {
        crate::jsobj::get(self.settings, "lines")
            .and_then(Value::as_array)
            .and_then(|lines| lines.get(self.selected_line))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    }
}

/// What a screen asks the App to do.
#[derive(Clone, Debug)]
pub enum Action {
    /// `onUpdate(settings)`: replace the settings object.
    SetSettings(Value),
    /// `onUpdate(widgets)` from the colors screen: replace the selected line's widgets.
    SetLineWidgets(Vec<Value>),
    /// `onBack()`.
    Back,
    /// TerminalOptionsMenu's `onBack('width')`.
    OpenTerminalWidth,
    /// `setFlashMessage({text, color})`.
    Flash(String, &'static str),
    /// RefreshIntervalMenu's `onUpdate(interval)`: write Claude's `statusLine.refreshInterval`, back to the menu.
    SetRefreshInterval(Option<f64>),
    /// `setFlowNotice({title, message, color, continueScreen: 'main'})` and `setScreen('flowNotice')`.
    FlowNotice { title: String, message: String, color: &'static str },
}

/// A screen hosted by the App.
pub trait Screen {
    fn handle(&mut self, ev: &Input, ctx: &Ctx) -> Vec<Action>;
    fn view(&mut self, ctx: &Ctx) -> El;
}

/// A screen whose port has not landed yet: blank, ESC goes back.
pub struct Placeholder;

impl Screen for Placeholder {
    fn handle(&mut self, ev: &Input, _ctx: &Ctx) -> Vec<Action> {
        if ev.key.escape {
            vec![Action::Back]
        } else {
            Vec::new()
        }
    }

    fn view(&mut self, _ctx: &Ctx) -> El {
        super::ink::layout::col(Vec::new())
    }
}
