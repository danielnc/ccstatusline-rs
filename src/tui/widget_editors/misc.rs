//! The misc widgets' editors: current-working-dir's segments (`CurrentWorkingDir.tsx`), custom text and symbol
//! (`CustomText.tsx`, `CustomSymbol.tsx`), custom command's command and timeout (`CustomCommand.tsx`) and link's
//! URL and text (`Link.tsx`). Custom command's width editor is the shared max-width one, which it duplicates.

use super::{shared, WidgetEditor};
use crate::jsnum::js_trim;
use crate::jsobj;
use crate::tui::components::hide_label::Done;
use crate::tui::ink::keys::{Input, Key};
use crate::tui::ink::layout::*;
use crate::tui::text_cursor::{should_insert_input, TextCursor};
use crate::widgets::editor::SYMBOL_OVERRIDE_ACTION;
use serde_json::{Map, Value};
use unicode_segmentation::UnicodeSegmentation;

pub fn open(kind: &str, action: &str, item: &Value) -> Option<Box<dyn WidgetEditor>> {
    Some(match kind {
        "current-working-dir" if action == SYMBOL_OVERRIDE_ACTION => shared::symbol_override(item, ""),
        "current-working-dir" if action == "edit-segments" => Box::new(DigitsEditor::segments(item)),
        "current-working-dir" => Box::new(UnknownMode),
        "custom-text" => Box::new(CustomTextEditor::new(item)),
        "custom-symbol" => Box::new(CustomSymbolEditor::new(item)),
        "custom-command" => match action {
            "edit-width" => Box::new(shared::MaxWidthEditor::new(item)),
            "edit-timeout" => Box::new(DigitsEditor::timeout(item)),
            _ => Box::new(CommandEditor::new(item)),
        },
        "link" => Box::new(LinkEditor::new(item, action == "edit-url")),
        _ => return None,
    })
}

/// `<Text>Unknown editor mode</Text>`, whose input handler does nothing for the mode.
struct UnknownMode;

impl WidgetEditor for UnknownMode {
    fn handle(&mut self, _ev: &Input) -> Done {
        Done::Pending
    }

    fn view(&mut self) -> El {
        plain("Unknown editor mode")
    }
}

/// The gray block upstream draws as the cursor.
fn cursor_style() -> TextStyle {
    TextStyle { background: Some("gray".into()), color: Some("black".into()), ..Default::default() }
}

// --- Digit fields: cwd segments, custom-command timeout --------------------------------------------------------

/// A digits-only field saved on Enter as `save(item, parsed)`, where `parsed` is the input's `parseInt` when it is
/// a number above zero.
struct DigitsEditor {
    widget: Value,
    input: String,
    prompt: &'static str,
    save: fn(&Value, Option<f64>) -> Value,
}

impl DigitsEditor {
    fn segments(widget: &Value) -> Self {
        DigitsEditor {
            widget: widget.clone(),
            input: jsobj::metadata_str(widget, "segments").unwrap_or("").to_string(),
            prompt: "Enter number of segments to display (blank for full path): ",
            save: save_segments,
        }
    }

    fn timeout(widget: &Value) -> Self {
        // `widget.timeout?.toString() ?? '1000'`
        let input = jsobj::get(widget, "timeout")
            .and_then(Value::as_f64)
            .map_or_else(|| "1000".to_string(), crate::jsnum::to_js_string);
        DigitsEditor {
            widget: widget.clone(),
            input,
            prompt: "Enter timeout in milliseconds (default 1000): ",
            save: |w, timeout| match timeout {
                Some(t) => jsobj::with(w, "timeout", crate::jsjson::num(t)),
                None => jsobj::without(w, "timeout"),
            },
        }
    }
}

/// `{...widget, metadata: {...widget.metadata, segments: String(n)}}`, or segments dropped (and the metadata with
/// it when nothing else is left).
fn save_segments(widget: &Value, segments: Option<f64>) -> Value {
    if let Some(n) = segments {
        return jsobj::set_metadata(widget, "segments", &crate::jsnum::to_js_string(n));
    }
    let rest = jsobj::get(widget, "metadata").map(|m| jsobj::without(m, "segments"));
    let rest = rest.filter(|m| m.as_object().is_some_and(|o| !o.is_empty()));
    jsobj::with(widget, "metadata", rest.unwrap_or_else(jsobj::undefined))
}

impl WidgetEditor for DigitsEditor {
    fn handle(&mut self, ev: &Input) -> Done {
        let k = &ev.key;
        if k.return_ {
            let parsed = crate::jsnum::parse_int(&self.input).filter(|n| *n > 0.0);
            return Done::Complete((self.save)(&self.widget, parsed));
        } else if k.escape {
            return Done::Cancel;
        } else if k.backspace {
            self.input.pop();
        } else if should_insert_input(&ev.input, k) && ev.input.chars().any(|c| c.is_ascii_digit()) {
            self.input.push_str(&ev.input);
        }
        Done::Pending
    }

    fn view(&mut self) -> El {
        col(vec![
            row(vec![plain(self.prompt), plain(self.input.clone()), text(cursor_style(), vec![s(" ")])]),
            text(TextStyle::dim(), vec![s("Press Enter to save, ESC to cancel")]),
        ])
    }
}

// --- custom-text / custom-symbol -------------------------------------------------------------------------------

struct CustomTextEditor {
    widget: Value,
    cursor: TextCursor,
}

impl CustomTextEditor {
    fn new(widget: &Value) -> Self {
        let text = jsobj::get_str(widget, "customText").unwrap_or("");
        CustomTextEditor { widget: widget.clone(), cursor: TextCursor::new(text) }
    }
}

impl WidgetEditor for CustomTextEditor {
    fn handle(&mut self, ev: &Input) -> Done {
        if ev.key.return_ {
            return Done::Complete(jsobj::with(&self.widget, "customText", Value::String(self.cursor.text.clone())));
        } else if ev.key.escape {
            return Done::Cancel;
        }
        self.cursor.handle(&ev.input, &ev.key);
        Done::Pending
    }

    fn view(&mut self) -> El {
        col(vec![
            plain(format!("Enter custom text: {}", self.cursor.display())),
            text(TextStyle::dim(), vec![s("←→ move cursor, Ctrl+←→ jump to start/end, Enter save, ESC cancel")]),
        ])
    }
}

struct CustomSymbolEditor {
    widget: Value,
    symbol: String,
}

impl CustomSymbolEditor {
    fn new(widget: &Value) -> Self {
        let symbol = jsobj::get_str(widget, "customSymbol").unwrap_or("").to_string();
        CustomSymbolEditor { widget: widget.clone(), symbol }
    }
}

impl WidgetEditor for CustomSymbolEditor {
    fn handle(&mut self, ev: &Input) -> Done {
        let k = &ev.key;
        if k.return_ {
            return Done::Complete(jsobj::with(&self.widget, "customSymbol", Value::String(self.symbol.clone())));
        } else if k.escape {
            return Done::Cancel;
        } else if k.backspace || k.delete {
            self.symbol.clear();
        } else if should_insert_input(&ev.input, k) {
            // only the first grapheme, so a multi-code-point emoji stays whole
            self.symbol = ev.input.graphemes(true).next().unwrap_or("").to_string();
        }
        Done::Pending
    }

    fn view(&mut self) -> El {
        let symbol = if self.symbol.is_empty() {
            nested(TextStyle { inverse: true, dim: true, ..Default::default() }, vec![s("(empty)")])
        } else {
            nested(TextStyle { inverse: true, ..Default::default() }, vec![s(self.symbol.clone())])
        };
        col(vec![
            text(TextStyle::default(), vec![s("Enter custom symbol: "), symbol]),
            text(TextStyle::dim(), vec![s("Type any character or emoji, Backspace clear, Enter save, ESC cancel")]),
        ])
    }
}

// --- custom-command / link: UTF-16 cursor fields ---------------------------------------------------------------

/// The plain string-and-index input custom command and link use: the cursor moves by UTF-16 units, as
/// `slice` and `[i]` do, not by graphemes.
struct Utf16Field {
    units: Vec<u16>,
    cursor: usize,
}

impl Utf16Field {
    fn new(text: &str) -> Self {
        let units: Vec<u16> = text.encode_utf16().collect();
        Utf16Field { cursor: units.len(), units }
    }

    fn text(&self) -> String {
        String::from_utf16_lossy(&self.units)
    }

    /// The arrows, Backspace, Delete and typing; Enter and ESC are the caller's.
    fn handle(&mut self, input: &str, k: &Key) {
        let len = self.units.len();
        if k.left_arrow {
            self.cursor = self.cursor.saturating_sub(1);
        } else if k.right_arrow {
            self.cursor = (self.cursor + 1).min(len);
        } else if k.backspace {
            if self.cursor > 0 {
                self.units.remove(self.cursor - 1);
                self.cursor -= 1;
            }
        } else if k.delete {
            if self.cursor < len {
                self.units.remove(self.cursor);
            }
        } else if should_insert_input(input, k) {
            let typed: Vec<u16> = input.encode_utf16().collect();
            self.units.splice(self.cursor..self.cursor, typed.iter().copied());
            self.cursor += typed.len();
        }
    }

    /// `{before}<Text backgroundColor='gray' color='black'>{value[cursor] ?? ' '}</Text>{after}`.
    fn inlines(&self) -> Vec<Inline> {
        let at = self.units.get(self.cursor).map_or_else(|| " ".to_string(), |u| String::from_utf16_lossy(&[*u]));
        let after = self.units.get(self.cursor + 1..).unwrap_or(&[]);
        vec![
            s(String::from_utf16_lossy(&self.units[..self.cursor])),
            nested(cursor_style(), vec![s(at)]),
            s(String::from_utf16_lossy(after)),
        ]
    }
}

struct CommandEditor {
    widget: Value,
    command: Utf16Field,
}

impl CommandEditor {
    fn new(widget: &Value) -> Self {
        let command = Utf16Field::new(jsobj::get_str(widget, "commandPath").unwrap_or(""));
        CommandEditor { widget: widget.clone(), command }
    }
}

impl WidgetEditor for CommandEditor {
    fn handle(&mut self, ev: &Input) -> Done {
        if ev.key.return_ {
            return Done::Complete(jsobj::with(&self.widget, "commandPath", Value::String(self.command.text())));
        } else if ev.key.escape {
            return Done::Cancel;
        }
        self.command.handle(&ev.input, &ev.key);
        Done::Pending
    }

    fn view(&mut self) -> El {
        let mut line = vec![s("Enter command path: ")];
        line.extend(self.command.inlines());
        col(vec![
            text(TextStyle::default(), line),
            text(TextStyle::dim(), vec![s("←→ move cursor, Enter save, ESC cancel")]),
        ])
    }
}

struct LinkEditor {
    widget: Value,
    url_mode: bool,
    url: Utf16Field,
    text: Utf16Field,
}

impl LinkEditor {
    fn new(widget: &Value, url_mode: bool) -> Self {
        LinkEditor {
            widget: widget.clone(),
            url_mode,
            url: Utf16Field::new(jsobj::metadata_str(widget, "url").unwrap_or("")),
            text: Utf16Field::new(jsobj::metadata_str(widget, "text").unwrap_or("")),
        }
    }
}

/// `buildMetadata`: the trimmed URL and text set (or deleted when blank), and no metadata key when none is left.
fn build_link_metadata(widget: &Value, url: &str, text: &str) -> Value {
    let mut metadata = jsobj::get(widget, "metadata").cloned().unwrap_or_else(|| Value::Object(Map::new()));
    for (key, value) in [("url", js_trim(url)), ("text", js_trim(text))] {
        metadata = if value.is_empty() {
            jsobj::without(&metadata, key)
        } else {
            jsobj::with(&metadata, key, Value::String(value.to_string()))
        };
    }
    if metadata.as_object().is_none_or(Map::is_empty) {
        return jsobj::without(widget, "metadata");
    }
    jsobj::with(widget, "metadata", metadata)
}

impl WidgetEditor for LinkEditor {
    fn handle(&mut self, ev: &Input) -> Done {
        if ev.key.return_ {
            return Done::Complete(build_link_metadata(&self.widget, &self.url.text(), &self.text.text()));
        } else if ev.key.escape {
            return Done::Cancel;
        }
        let active = if self.url_mode { &mut self.url } else { &mut self.text };
        active.handle(&ev.input, &ev.key);
        Done::Pending
    }

    fn view(&mut self) -> El {
        let url = self.url.text();
        let url_trimmed = js_trim(&url);
        let (prompt, active) = if self.url_mode {
            ("Enter URL (http/https): ", &self.url)
        } else {
            ("Enter link text (blank uses URL): ", &self.text)
        };
        let mut line = vec![s(prompt)];
        line.extend(active.inlines());
        let current = if self.url_mode {
            let text = self.text.text();
            let shown = Some(js_trim(&text)).filter(|t| !t.is_empty()).unwrap_or("(uses URL)").to_string();
            format!("Current text: {shown}")
        } else {
            format!("Current URL: {}", Some(url_trimmed).filter(|u| !u.is_empty()).unwrap_or("(none)"))
        };
        let mut rows = vec![text(TextStyle::default(), line), text(TextStyle::dim(), vec![s(current)])];
        if self.url_mode && !url_trimmed.is_empty() && crate::misc::url::parse_http_url(url_trimmed).is_none() {
            rows.push(text(TextStyle::color("yellow"), vec![s("URL must begin with http:// or https://")]));
        }
        rows.push(text(TextStyle::dim(), vec![s("←→ move cursor, Enter save, ESC cancel")]));
        col(rows)
    }
}
