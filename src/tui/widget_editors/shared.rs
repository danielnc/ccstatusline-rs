//! The editors several widgets share: max width (`widgets/shared/max-width.tsx`) and glyph overrides
//! (`widgets/shared/symbol-override.tsx`).

use super::WidgetEditor;
use crate::jsobj;
use crate::tui::components::hide_label::Done;
use crate::tui::ink::keys::Input;
use crate::tui::ink::layout::*;
use crate::tui::text_cursor::should_insert_input;
use serde_json::Value;
use unicode_segmentation::UnicodeSegmentation;

/// `MaxWidthEditor`.
pub struct MaxWidthEditor {
    widget: Value,
    input: String,
}

impl MaxWidthEditor {
    pub fn new(widget: &Value) -> Self {
        let input =
            jsobj::get(widget, "maxWidth").and_then(Value::as_f64).map(crate::jsnum::to_js_string).unwrap_or_default();
        MaxWidthEditor { widget: widget.clone(), input }
    }
}

impl WidgetEditor for MaxWidthEditor {
    fn handle(&mut self, ev: &Input) -> Done {
        let k = &ev.key;
        if k.return_ {
            return match crate::jsnum::parse_int(&self.input) {
                Some(w) if w > 0.0 => Done::Complete(jsobj::with(&self.widget, "maxWidth", crate::jsjson::num(w))),
                _ => Done::Complete(jsobj::without(&self.widget, "maxWidth")),
            };
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
            row(vec![
                plain("Enter max width (blank for no limit): "),
                plain(self.input.clone()),
                text(
                    TextStyle { background: Some("gray".into()), color: Some("black".into()), ..Default::default() },
                    vec![s(" ")],
                ),
            ]),
            text(TextStyle::dim(), vec![s("Press Enter to save, ESC to cancel")]),
        ])
    }
}

/// One editable symbol (`SymbolSlot`): `character` stores on the item's `character` field, any other id in metadata.
#[derive(Clone, Debug)]
pub struct Slot {
    pub id: &'static str,
    pub label: &'static str,
    pub default_symbol: &'static str,
}

fn slot_symbol(item: &Value, slot: &Slot) -> String {
    if slot.id == "character" {
        return jsobj::get_str(item, "character").unwrap_or(slot.default_symbol).to_string();
    }
    jsobj::metadata_str(item, slot.id).unwrap_or(slot.default_symbol).to_string()
}

/// `setSlotSymbol`: overrides equal to the default are removed.
fn set_slot_symbol(item: &Value, slot: &Slot, value: &str) -> Value {
    if slot.id == "character" {
        if value == slot.default_symbol {
            return jsobj::without(item, "character");
        }
        return jsobj::with(item, "character", Value::String(value.into()));
    }
    if value == slot.default_symbol {
        return jsobj::remove_metadata_keys(item, &[slot.id]);
    }
    jsobj::set_metadata(item, slot.id, value)
}

/// `SymbolSlotsEditor`.
pub struct SymbolSlotsEditor {
    widget: Value,
    slots: Vec<Slot>,
    values: Vec<String>,
    selected: usize,
}

pub fn symbol_override(widget: &Value, default_symbol: &'static str) -> Box<dyn WidgetEditor> {
    symbol_slots(widget, vec![Slot { id: "character", label: "Glyph", default_symbol }])
}

pub fn symbol_slots(widget: &Value, slots: Vec<Slot>) -> Box<dyn WidgetEditor> {
    let values = slots.iter().map(|s| slot_symbol(widget, s)).collect();
    Box::new(SymbolSlotsEditor { widget: widget.clone(), slots, values, selected: 0 })
}

impl WidgetEditor for SymbolSlotsEditor {
    fn handle(&mut self, ev: &Input) -> Done {
        let k = &ev.key;
        let n = self.slots.len();
        if k.return_ {
            let mut item = self.widget.clone();
            for (slot, value) in self.slots.iter().zip(&self.values) {
                item = set_slot_symbol(&item, slot, value);
            }
            return Done::Complete(item);
        } else if k.escape {
            return Done::Cancel;
        } else if k.up_arrow && n > 1 {
            self.selected = if self.selected == 0 { n - 1 } else { self.selected - 1 };
        } else if k.down_arrow && n > 1 {
            self.selected = if self.selected + 1 > n - 1 { 0 } else { self.selected + 1 };
        } else if k.tab {
            self.values[self.selected] = self.slots[self.selected].default_symbol.to_string();
        } else if k.backspace || k.delete {
            self.values[self.selected].clear();
        } else if should_insert_input(&ev.input, k) {
            self.values[self.selected] = ev.input.graphemes(true).next().unwrap_or("").to_string();
        }
        Done::Pending
    }

    fn view(&mut self) -> El {
        let label_width = self.slots.iter().map(|s| crate::tui::ink::text::string_width(s.label)).max().unwrap_or(0);
        let rows: Vec<El> = self
            .slots
            .iter()
            .enumerate()
            .map(|(i, slot)| {
                let sel = i == self.selected;
                let color = sel.then_some("green");
                let value = &self.values[i];
                let padding = " ".repeat(label_width.saturating_sub(crate::tui::ink::text::string_width(slot.label)));
                row(vec![
                    boxed(
                        BoxStyle::default().width(Width::Points(4.0)),
                        vec![text(TextStyle::default().with_color(color), vec![s(if sel { "▶ " } else { "  " })])],
                    ),
                    text(TextStyle::default().with_color(color), vec![s(format!("{padding}{}: ", slot.label))]),
                    if value.is_empty() {
                        text(TextStyle { inverse: true, dim: true, ..Default::default() }, vec![s("(none)")])
                    } else {
                        text(TextStyle { inverse: true, ..Default::default() }, vec![s(value.clone())])
                    },
                    text(TextStyle::dim(), vec![s(format!(" (default: {})", slot.default_symbol))]),
                ])
            })
            .collect();
        col(vec![
            text(TextStyle::bold(), vec![s("Glyphs")]),
            text(
                TextStyle::dim(),
                vec![s(if self.slots.len() > 1 {
                    "↑↓ row, type to set, Tab default, Backspace none, Enter save, ESC cancel"
                } else {
                    "Type any character or emoji, Tab default, Backspace none, Enter save, ESC cancel"
                })],
            ),
            boxed(BoxStyle::column().margin_top(1.0), rows),
        ])
    }
}
