//! Upstream `HideStatesEditor.tsx` and `LabelEditor.tsx`, the two overrides the items editor offers every widget
//! that declares hideable states or a label.

use crate::tui::ink::keys::Input;
use crate::tui::ink::layout::*;
use crate::tui::text_cursor::TextCursor;
use crate::widgets::editor as we;
use crate::widgets::HideableState;
use serde_json::Value;

pub enum Done {
    Pending,
    Complete(Value),
    Cancel,
}

pub struct HideStatesEditor {
    selected: usize,
    enabled: Vec<&'static str>,
}

impl HideStatesEditor {
    pub fn new(widget: &Value, states: &[HideableState]) -> Self {
        HideStatesEditor { selected: 0, enabled: we::enabled_hide_states(widget, states) }
    }

    pub fn handle(&mut self, ev: &Input, widget: &Value, states: &[HideableState]) -> Done {
        let shortcut = if ev.key.ctrl || ev.key.meta { "" } else { ev.input.as_str() };
        let n = states.len();
        if ev.key.return_ {
            return Done::Complete(we::set_enabled_hide_states(widget, states, &self.enabled));
        } else if ev.key.escape {
            return Done::Cancel;
        } else if ev.key.up_arrow && n > 0 {
            self.selected = if self.selected == 0 { n - 1 } else { self.selected - 1 };
        } else if ev.key.down_arrow && n > 0 {
            self.selected = if self.selected + 1 > n - 1 { 0 } else { self.selected + 1 };
        } else if shortcut == " " {
            if let Some(state) = states.get(self.selected) {
                if self.enabled.contains(&state.key) {
                    self.enabled.retain(|k| *k != state.key);
                } else {
                    self.enabled.push(state.key);
                }
            }
        }
        Done::Pending
    }

    pub fn view(&self, widget: &Value, states: &[HideableState]) -> El {
        let merged = crate::jsobj::get(widget, "merge").is_some_and(crate::tui::truthy);
        let rows: Vec<El> = states
            .iter()
            .enumerate()
            .map(|(i, state)| {
                let sel = i == self.selected;
                let color = sel.then_some("green");
                let mut cells = vec![
                    boxed(
                        BoxStyle::default().width(Width::Points(3.0)),
                        vec![text(TextStyle::default().with_color(color), vec![s(if sel { "▶ " } else { "  " })])],
                    ),
                    text(
                        TextStyle::default().with_color(color),
                        vec![s(format!(
                            "[{}] {}",
                            if self.enabled.contains(&state.key) { "x" } else { " " },
                            state.label
                        ))],
                    ),
                ];
                if state.key == we::MERGE_TARGET_HIDDEN.key && !merged {
                    cells.push(text(TextStyle::dim(), vec![s(" (requires merge)")]));
                }
                row(cells)
            })
            .collect();
        col(vec![
            text(TextStyle::bold(), vec![s("Hide")]),
            text(TextStyle::dim(), vec![s("↑↓ select, Space toggle, Enter save, ESC cancel")]),
            boxed(BoxStyle::column().margin_top(1.0), rows),
        ])
    }
}

pub struct LabelEditor {
    cursor: TextCursor,
}

impl LabelEditor {
    pub fn new(widget: &Value, default_label: &str) -> Self {
        LabelEditor { cursor: TextCursor::new(&we::get_label(widget, default_label)) }
    }

    pub fn handle(&mut self, ev: &Input, widget: &Value) -> Done {
        if ev.key.return_ {
            Done::Complete(we::set_label(widget, &self.cursor.text))
        } else if ev.key.escape {
            Done::Cancel
        } else if ev.key.tab {
            Done::Complete(we::clear_label(widget))
        } else {
            self.cursor.handle(&ev.input, &ev.key);
            Done::Pending
        }
    }

    pub fn view(&self, default_label: &str) -> El {
        let mut quoted = String::new();
        crate::jsjson::quote(default_label, &mut quoted);
        col(vec![
            text(TextStyle::bold(), vec![s("Label")]),
            text(
                TextStyle::dim(),
                vec![s("←→ move cursor, Ctrl+←→ jump to start/end, Tab reset to default, Enter save, ESC cancel")],
            ),
            boxed(
                BoxStyle::default().margin_top(1.0),
                vec![text(
                    TextStyle::default(),
                    vec![
                        s(format!("\"{}\"", self.cursor.display())),
                        nested(TextStyle::dim(), vec![s(format!(" (default: {quoted})"))]),
                    ],
                )],
            ),
        ])
    }
}
