//! Upstream `TerminalOptionsMenu.tsx`, with the slice of `utils/color-sanitize.ts` it calls.
//!
//! Upstream also sets `chalk.level` to the new color level; the preview here takes its chalk level from
//! `settings.colorLevel` on every render, so the settings update is enough.

use serde_json::Value;

use super::confirm::{Answer, Confirm};
use super::list::{Entry, Event, Item, List, Props};
use crate::jsobj;
use crate::tui::ink::keys::Input;
use crate::tui::ink::layout::*;
use crate::tui::screens::{Action, Ctx, Screen};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Choice {
    Width,
    ColorLevel,
}

/// `getNextColorLevel`.
pub fn next_color_level(level: u8) -> u8 {
    (level + 1) % 4
}

/// `shouldWarnOnColorLevelChange`.
pub fn should_warn_on_color_level_change(current: u8, next: u8, has_custom_colors: bool) -> bool {
    has_custom_colors && ((current == 2 && next != 2) || (current == 3 && next != 3))
}

/// `getColorLevelLabel`.
pub fn color_level_label(level: u8) -> &'static str {
    match level {
        0 => "No Color",
        1 => "Basic",
        3 => "Truecolor",
        _ => "256 Color (default)",
    }
}

fn build_items(color_level: u8) -> Vec<Item<Choice>> {
    vec![
        Item::Entry(
            Entry::new("◱ Terminal Width", Choice::Width)
                .description("Configure how the status line uses available terminal width and when it should compact."),
        ),
        Item::Entry(
            Entry::new("▓ Color Level", Choice::ColorLevel)
                .sublabel(Some(format!("({})", color_level_label(color_level))))
                .description(
                    [
                        "Color level affects how colors are rendered:",
                        "• Truecolor: Full 24-bit RGB colors (16.7M colors)",
                        "• 256 Color: Extended color palette (256 colors)",
                        "• Basic: Standard 16-color terminal palette",
                        "• No Color: Disables all color output",
                    ]
                    .join("\n"),
                ),
        ),
    ]
}

fn props(items: &[Item<Choice>]) -> Props<'_, Choice> {
    let mut p = Props::new(items);
    p.margin_top = 1.0;
    p.show_back_button = true;
    p
}

/// `isCustomColor`.
fn is_custom_color(v: Option<&str>) -> bool {
    v.is_some_and(|v| !v.is_empty() && (v.starts_with("ansi256:") || v.starts_with("hex:")))
}

/// `isIncompatibleForLevel`.
fn is_incompatible_for_level(v: Option<&str>, next: u8) -> bool {
    if !is_custom_color(v) {
        return false;
    }
    let v = v.unwrap_or_default();
    match next {
        2 => v.starts_with("hex:"),
        3 => v.starts_with("ansi256:"),
        _ => true,
    }
}

fn lines(settings: &Value) -> Vec<Value> {
    jsobj::get(settings, "lines").and_then(Value::as_array).cloned().unwrap_or_default()
}

fn widgets(line: &Value) -> &[Value] {
    line.as_array().map(Vec::as_slice).unwrap_or_default()
}

/// `hasCustomWidgetColors`.
pub fn has_custom_widget_colors(lines: &[Value]) -> bool {
    lines.iter().any(|line| {
        widgets(line).iter().any(|w| {
            is_custom_color(jsobj::get_str(w, "color")) || is_custom_color(jsobj::get_str(w, "backgroundColor"))
        })
    })
}

/// `sanitizeLinesForColorLevel`.
pub fn sanitize_lines_for_color_level(lines: &[Value], next: u8) -> Vec<Value> {
    lines
        .iter()
        .map(|line| {
            Value::Array(
                widgets(line)
                    .iter()
                    .map(|widget| {
                        let mut next_widget = widget.clone();
                        if is_incompatible_for_level(jsobj::get_str(widget, "color"), next) {
                            // `resetWidgetForegroundToDefault`.
                            let kind = jsobj::get_str(widget, "type").unwrap_or_default();
                            if kind != "separator" && kind != "flex-separator" {
                                if let Some(w) = crate::widgets::get_widget(kind) {
                                    next_widget =
                                        jsobj::with(&next_widget, "color", Value::String(w.default_color().into()));
                                }
                            }
                        }
                        if is_incompatible_for_level(jsobj::get_str(widget, "backgroundColor"), next) {
                            next_widget = jsobj::with(&next_widget, "backgroundColor", jsobj::undefined());
                        }
                        next_widget
                    })
                    .collect(),
            )
        })
        .collect()
}

/// `{...settings, lines: cleanedLines, colorLevel: level}`.
fn apply_color_level(settings: &Value, level: u8) -> Value {
    let cleaned = sanitize_lines_for_color_level(&lines(settings), level);
    let next = jsobj::with(settings, "lines", Value::Array(cleaned));
    jsobj::with(&next, "colorLevel", crate::jsjson::num(level as f64))
}

pub struct TerminalOptionsMenu {
    list: List,
    /// `showColorWarning` and `pendingColorLevel`, set together.
    pending_color_level: Option<u8>,
    show_color_warning: bool,
    confirm: Confirm,
}

impl TerminalOptionsMenu {
    pub fn new(_ctx: &Ctx) -> Self {
        TerminalOptionsMenu {
            list: List::new(0),
            pending_color_level: None,
            show_color_warning: false,
            confirm: Confirm::default(),
        }
    }

    fn handle_select(&mut self, value: Choice, ctx: &Ctx) -> Vec<Action> {
        if value == Choice::Width {
            return vec![Action::OpenTerminalWidth];
        }
        let has_custom_colors = has_custom_widget_colors(&lines(ctx.settings));
        let current = ctx.typed.color_level;
        let next = next_color_level(current);
        if should_warn_on_color_level_change(current, next, has_custom_colors) {
            self.show_color_warning = true;
            self.pending_color_level = Some(next);
            self.confirm = Confirm::default();
            return Vec::new();
        }
        vec![Action::SetSettings(apply_color_level(ctx.settings, next))]
    }

    fn close_warning(&mut self) {
        self.show_color_warning = false;
        self.pending_color_level = None;
        // The List unmounts behind the warning, so it comes back at its initial selection.
        self.list = List::new(0);
    }
}

impl Screen for TerminalOptionsMenu {
    fn handle(&mut self, ev: &Input, ctx: &Ctx) -> Vec<Action> {
        if self.show_color_warning {
            // Only the ConfirmDialog acts; the menu's own handler skips ESC while the warning shows.
            return match self.confirm.handle(ev, false) {
                Answer::Yes => {
                    let actions = match self.pending_color_level {
                        Some(level) => vec![Action::SetSettings(apply_color_level(ctx.settings, level))],
                        None => Vec::new(),
                    };
                    self.close_warning();
                    actions
                }
                Answer::No => {
                    self.close_warning();
                    Vec::new()
                }
                Answer::None => Vec::new(),
            };
        }
        let items = build_items(ctx.typed.color_level);
        let p = props(&items);
        self.list.sync(&p);
        let mut actions = match self.list.handle(&p, ev) {
            Event::Selected(value, _) => self.handle_select(value, ctx),
            Event::Back => vec![Action::Back],
            Event::None => Vec::new(),
        };
        if ev.key.escape {
            actions.push(Action::Back);
        }
        actions
    }

    fn view(&mut self, ctx: &Ctx) -> El {
        let mut children = vec![text(TextStyle::bold(), vec![s("Terminal Options")])];
        if self.show_color_warning {
            children.push(boxed(
                BoxStyle::column().margin_top(1.0),
                vec![
                    text(TextStyle::color("yellow"), vec![s("⚠ Warning: Custom colors detected!")]),
                    plain("Switching color modes will reset custom ansi256 or hex colors to defaults."),
                    boxed(BoxStyle::default().margin_top(1.0), vec![self.confirm.view(None, true, false)]),
                ],
            ));
        } else {
            let items = build_items(ctx.typed.color_level);
            let p = props(&items);
            self.list.sync(&p);
            children.push(text(
                TextStyle::color("white"),
                vec![s("Configure terminal-specific settings for optimal display")],
            ));
            children.push(self.list.view(&p));
        }
        col(children)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn cycles_color_levels_in_order() {
        assert_eq!([0, 1, 2, 3].map(next_color_level), [1, 2, 3, 0]);
    }

    #[test]
    fn warns_only_when_custom_colors_would_be_lost() {
        assert!(should_warn_on_color_level_change(2, 3, true));
        assert!(should_warn_on_color_level_change(3, 0, true));
        assert!(!should_warn_on_color_level_change(2, 3, false));
        assert!(!should_warn_on_color_level_change(1, 2, true));
        assert!(!should_warn_on_color_level_change(0, 1, true));
    }

    #[test]
    fn labels_each_color_level() {
        assert_eq!([0, 1, 2, 3].map(color_level_label), ["No Color", "Basic", "256 Color (default)", "Truecolor"]);
    }

    #[test]
    fn sanitizes_custom_colors_for_the_next_level() {
        let lines = vec![json!([
            {"id": "1", "type": "model", "color": "hex:FF0000", "backgroundColor": "ansi256:12"},
            {"id": "2", "type": "separator", "color": "ansi256:3"},
            {"id": "3", "type": "model", "color": "red"}
        ])];
        assert!(has_custom_widget_colors(&lines));
        let at256 = sanitize_lines_for_color_level(&lines, 2);
        let w = &at256[0][0];
        assert_eq!(jsobj::get_str(w, "color"), Some("cyan"));
        assert_eq!(jsobj::get_str(w, "backgroundColor"), Some("ansi256:12"));
        let basic = sanitize_lines_for_color_level(&lines, 1);
        assert_eq!(jsobj::get(&basic[0][0], "backgroundColor"), None);
        assert_eq!(jsobj::get_str(&basic[0][1], "color"), Some("ansi256:3"));
        assert_eq!(jsobj::get_str(&basic[0][2], "color"), Some("red"));
    }
}
