//! Upstream `TerminalWidthMenu.tsx`.

use serde_json::Value;

use super::list::{Entry, Event, Item, List, Props};
use crate::jsnum::to_js_string;
use crate::jsobj;
use crate::tui::ink::keys::Input;
use crate::tui::ink::layout::*;
use crate::tui::screens::{Action, Ctx, Screen};
use crate::tui::text_cursor::should_insert_input;

/// `TERMINAL_WIDTH_OPTIONS`.
pub const TERMINAL_WIDTH_OPTIONS: [&str; 3] = ["full", "full-minus-40", "full-until-compact"];

/// `getTerminalWidthSelectionIndex`.
pub fn selection_index(selected: &str) -> usize {
    TERMINAL_WIDTH_OPTIONS.iter().position(|o| *o == selected).unwrap_or(0)
}

/// `validateCompactThresholdInput`.
pub fn validate_compact_threshold_input(value: &str) -> Option<String> {
    match crate::jsnum::parse_int(value) {
        None => Some("Please enter a valid number".into()),
        Some(n) if !(1.0..=99.0).contains(&n) => {
            Some(format!("Value must be between 1 and 99 (you entered {})", to_js_string(n)))
        }
        Some(_) => None,
    }
}

/// `buildTerminalWidthItems`.
pub fn build_items(selected: &str, compact_threshold: f64) -> Vec<Item<&'static str>> {
    let t = to_js_string(compact_threshold);
    vec![
        Item::Entry(
            Entry::new("Full width always", "full")
                .sublabel((selected == "full").then(|| "(active)".into()))
                .description(
                    "Uses the full terminal width minus 4 characters for terminal padding. If the auto-compact \
                     message appears, it may cause the line to wrap.\n\nNOTE: If /ide integration is enabled, it is \
                     not recommended to use this mode.",
                ),
        ),
        Item::Entry(
            Entry::new("Full width minus 40", "full-minus-40")
                .sublabel(Some(if selected == "full-minus-40" { "(active)" } else { "(default)" }.into()))
                .description(
                    "Leaves a gap to the right of the status line to accommodate the auto-compact message. This \
                     prevents wrapping but may leave unused space. This limitation exists because we cannot detect \
                     when the message will appear.",
                ),
        ),
        Item::Entry(
            Entry::new("Full width until compact", "full-until-compact")
                .sublabel(Some(if selected == "full-until-compact" {
                    format!("(threshold {t}%, active)")
                } else {
                    format!("(threshold {t}%)")
                }))
                .description(format!(
                    "Dynamically adjusts width based on context usage. When context reaches {t}%, it switches to \
                     leaving space for the auto-compact message.\n\nNOTE: If /ide integration is enabled, it is not \
                     recommended to use this mode."
                )),
        ),
    ]
}

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

pub struct TerminalWidthMenu {
    selected_option: String,
    compact_threshold: f64,
    editing_threshold: bool,
    threshold_input: String,
    validation_error: Option<String>,
    list: List,
}

impl TerminalWidthMenu {
    pub fn new(ctx: &Ctx) -> Self {
        let selected_option = ctx.typed.flex_mode.clone();
        TerminalWidthMenu {
            list: List::new(selection_index(&selected_option)),
            selected_option,
            compact_threshold: ctx.typed.compact_threshold,
            editing_threshold: false,
            threshold_input: to_js_string(ctx.typed.compact_threshold),
            validation_error: None,
        }
    }

    /// `{...settings, flexMode, compactThreshold}`.
    fn updated(ctx: &Ctx, flex_mode: &str, threshold: f64) -> Value {
        let next = jsobj::with(ctx.settings, "flexMode", Value::String(flex_mode.into()));
        jsobj::with(&next, "compactThreshold", crate::jsjson::num(threshold))
    }

    fn stop_editing(&mut self) {
        self.editing_threshold = false;
        // The List was unmounted while editing, so it comes back at the selected option.
        self.list = List::new(selection_index(&self.selected_option));
    }

    fn handle_editing(&mut self, ev: &Input, ctx: &Ctx) -> Vec<Action> {
        let key = &ev.key;
        if key.return_ {
            match validate_compact_threshold_input(&self.threshold_input) {
                Some(error) => self.validation_error = Some(error),
                None => {
                    let value = crate::jsnum::parse_int(&self.threshold_input).unwrap_or_default();
                    self.compact_threshold = value;
                    self.stop_editing();
                    self.validation_error = None;
                    return vec![Action::SetSettings(Self::updated(ctx, &self.selected_option, value))];
                }
            }
        } else if key.escape {
            self.threshold_input = to_js_string(self.compact_threshold);
            self.stop_editing();
            self.validation_error = None;
        } else if key.backspace {
            let mut units: Vec<u16> = self.threshold_input.encode_utf16().collect();
            units.pop();
            self.threshold_input = String::from_utf16_lossy(&units);
            self.validation_error = None;
        } else if key.delete {
            // For simple number inputs, forward delete does nothing since there's no cursor position
        } else if should_insert_input(&ev.input, key) && ev.input.bytes().any(|b| b.is_ascii_digit()) {
            let new_value = format!("{}{}", self.threshold_input, ev.input);
            if utf16_len(&new_value) <= 2 {
                self.threshold_input = new_value;
                self.validation_error = None;
            }
        }
        Vec::new()
    }
}

impl Screen for TerminalWidthMenu {
    fn handle(&mut self, ev: &Input, ctx: &Ctx) -> Vec<Action> {
        if self.editing_threshold {
            return self.handle_editing(ev, ctx);
        }
        let items = build_items(&self.selected_option, self.compact_threshold);
        let mut p = Props::new(&items);
        p.margin_top = 1.0;
        p.show_back_button = true;
        p.initial_selection = selection_index(&self.selected_option);
        self.list.sync(&p);
        let mut actions = match self.list.handle(&p, ev) {
            Event::Selected(value, _) => {
                self.selected_option = value.to_string();
                if value == "full-until-compact" {
                    self.editing_threshold = true;
                }
                vec![Action::SetSettings(Self::updated(ctx, value, self.compact_threshold))]
            }
            Event::Back => vec![Action::Back],
            Event::None => Vec::new(),
        };
        if ev.key.escape {
            actions.push(Action::Back);
        }
        actions
    }

    fn view(&mut self, _ctx: &Ctx) -> El {
        let mut children = vec![
            text(TextStyle::bold(), vec![s("Terminal Width")]),
            text(
                TextStyle::color("white"),
                vec![s("These settings affect where long lines are truncated, and where right-alignment occurs \
                        when using flex separators")],
            ),
            text(
                TextStyle::dim(),
                vec![s("Claude code does not currently provide an available width variable for the statusline and \
                        features like IDE integration, auto-compaction notices, etc all cause the statusline to wrap \
                        if we do not truncate it")],
            ),
        ];
        if self.editing_threshold {
            children.push(boxed(
                BoxStyle::column().margin_top(1.0),
                vec![
                    plain(format!("Enter compact threshold (1-99): {}%", self.threshold_input)),
                    match &self.validation_error {
                        Some(error) => text(TextStyle::color("red"), vec![s(error.clone())]),
                        None => text(TextStyle::dim(), vec![s("Press Enter to confirm, ESC to cancel")]),
                    },
                ],
            ));
        } else {
            let items = build_items(&self.selected_option, self.compact_threshold);
            let mut p = Props::new(&items);
            p.margin_top = 1.0;
            p.show_back_button = true;
            p.initial_selection = selection_index(&self.selected_option);
            self.list.sync(&p);
            children.push(self.list.view(&p));
        }
        col(children)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_compact_threshold_input() {
        assert_eq!(validate_compact_threshold_input("abc").as_deref(), Some("Please enter a valid number"));
        assert_eq!(validate_compact_threshold_input("").as_deref(), Some("Please enter a valid number"));
        assert_eq!(
            validate_compact_threshold_input("0").as_deref(),
            Some("Value must be between 1 and 99 (you entered 0)")
        );
        assert_eq!(
            validate_compact_threshold_input("100").as_deref(),
            Some("Value must be between 1 and 99 (you entered 100)")
        );
        assert_eq!(validate_compact_threshold_input("50"), None);
    }

    #[test]
    fn returns_the_current_option_index() {
        assert_eq!(selection_index("full"), 0);
        assert_eq!(selection_index("full-until-compact"), 2);
        assert_eq!(selection_index("bogus"), 0);
    }

    #[test]
    fn builds_items_with_active_and_threshold_sublabels() {
        let items = build_items("full-until-compact", 60.0);
        let subs: Vec<Option<String>> = items
            .iter()
            .map(|i| match i {
                Item::Entry(e) => e.sublabel.clone(),
                Item::Separator => None,
            })
            .collect();
        assert_eq!(subs, [None, Some("(default)".into()), Some("(threshold 60%, active)".into())]);
    }
}
