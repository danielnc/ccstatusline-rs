//! The editors several widgets share: max width (`widgets/shared/max-width.tsx`), glyph overrides
//! (`widgets/shared/symbol-override.tsx`) and the searchable option list (`widgets/shared/searchable-option-editor.tsx`).

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

// --- searchable-option-editor.tsx ------------------------------------------------------------------------------

const MAX_VISIBLE_OPTIONS: usize = 10;

/// A `SearchableOption`, with the search and sort texts its filter reads.
#[derive(Clone, Debug)]
pub struct SearchOption {
    pub value: String,
    pub display_name: String,
    pub description: String,
    pub search_text: String,
    pub sort_text: String,
}

/// `filterOptions`: the options matching a query, best first.
pub type FilterOptions = fn(&[SearchOption], &str) -> Vec<SearchOption>;

/// The fuzzy filter the locale and timezone options share (`filterFuzzySearchRecords` over
/// `displayName description value searchText`).
pub fn fuzzy_filter_options(options: &[SearchOption], query: &str) -> Vec<SearchOption> {
    let search_texts: Vec<String> =
        options.iter().map(|o| format!("{} {} {} {}", o.display_name, o.description, o.value, o.search_text)).collect();
    let records: Vec<crate::widgets::catalog::Record> = options
        .iter()
        .zip(&search_texts)
        .map(|(o, search_text)| crate::widgets::catalog::Record {
            name: &o.display_name,
            kind: &o.value,
            description: &o.description,
            search_text,
            sort_text: &o.sort_text,
            secondary_sort_text: &o.value,
        })
        .collect();
    crate::widgets::catalog::filter_records(&records, query).into_iter().map(|i| options[i].clone()).collect()
}

/// `SearchableOptionEditor`: type to search, Up/Down to select, Enter to save, ESC to cancel.
pub struct SearchableOptionEditor {
    title: &'static str,
    current_label: String,
    options: Vec<SearchOption>,
    filter_options: FilterOptions,
    empty_message: &'static str,
    on_select: Box<dyn Fn(&str) -> Value>,
    query: String,
    selected_index: usize,
}

impl SearchableOptionEditor {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        title: &'static str,
        current_label: String,
        initial_value: Option<&str>,
        options: Vec<SearchOption>,
        filter_options: FilterOptions,
        empty_message: &'static str,
        on_select: Box<dyn Fn(&str) -> Value>,
    ) -> Self {
        // getInitialSelectedIndex
        let selected_index = options.iter().position(|o| Some(o.value.as_str()) == initial_value).unwrap_or(0);
        SearchableOptionEditor {
            title,
            current_label,
            options,
            filter_options,
            empty_message,
            on_select,
            query: String::new(),
            selected_index,
        }
    }

    fn filtered(&self) -> Vec<SearchOption> {
        (self.filter_options)(&self.options, &self.query)
    }
}

fn clamped_index(selected: usize, len: usize) -> usize {
    if len == 0 {
        0
    } else {
        selected.min(len - 1)
    }
}

/// `getVisibleRange`.
fn visible_range(selected: usize, total: usize) -> (usize, usize) {
    if total <= MAX_VISIBLE_OPTIONS {
        return (0, total);
    }
    let half = MAX_VISIBLE_OPTIONS / 2;
    let start = selected.saturating_sub(half).min(total - MAX_VISIBLE_OPTIONS);
    (start, start + MAX_VISIBLE_OPTIONS)
}

impl WidgetEditor for SearchableOptionEditor {
    fn handle(&mut self, ev: &Input) -> Done {
        let k = &ev.key;
        let filtered = self.filtered();
        if k.return_ {
            if let Some(option) = filtered.get(clamped_index(self.selected_index, filtered.len())) {
                return Done::Complete((self.on_select)(&option.value));
            }
            return Done::Pending;
        }
        if k.escape {
            return Done::Cancel;
        }
        if k.up_arrow || k.down_arrow {
            let n = filtered.len();
            if n > 0 {
                let current = self.selected_index.min(n - 1);
                self.selected_index = if k.down_arrow {
                    if current + 1 > n - 1 {
                        0
                    } else {
                        current + 1
                    }
                } else if current == 0 {
                    n - 1
                } else {
                    current - 1
                };
            }
            return Done::Pending;
        }
        if k.backspace || k.delete {
            self.query.pop();
            self.selected_index = 0;
            return Done::Pending;
        }
        if should_insert_input(&ev.input, k) {
            self.query.push_str(&ev.input);
            self.selected_index = 0;
        }
        Done::Pending
    }

    fn view(&mut self) -> El {
        let filtered = self.filtered();
        let selected = clamped_index(self.selected_index, filtered.len());
        let (start, end) = visible_range(selected, filtered.len());
        let list = if filtered.is_empty() {
            vec![text(TextStyle::dim(), vec![s(self.empty_message)])]
        } else {
            filtered[start..end]
                .iter()
                .enumerate()
                .map(|(visible_index, option)| {
                    let is_selected = start + visible_index == selected;
                    let mut children = vec![boxed(
                        BoxStyle::default().width(Width::Points(3.0)),
                        vec![text(
                            TextStyle::default().with_color(is_selected.then_some("green")),
                            vec![s(if is_selected { "> " } else { "  " })],
                        )],
                    )];
                    for (segment, matched) in crate::widgets::catalog::match_segments(&option.display_name, &self.query)
                    {
                        // getSegmentColor: the selected row is all green; other rows highlight the matched letters.
                        let color = if is_selected { Some("green") } else { matched.then_some("yellowBright") };
                        children.push(text(
                            TextStyle::default().with_color(color).with_bold(is_selected || matched),
                            vec![s(segment)],
                        ));
                    }
                    children.push(text(TextStyle::dim(), vec![s(format!(" - {}", option.description))]));
                    row(children)
                })
                .collect()
        };
        let mut out = vec![
            row(vec![
                text(TextStyle::bold(), vec![s(self.title)]),
                text(TextStyle::dim(), vec![s(format!(" Current: {}", self.current_label))]),
            ]),
            row(vec![
                text(TextStyle::dim(), vec![s("Search: ")]),
                text(
                    TextStyle::color("cyan"),
                    vec![s(if self.query.is_empty() { "(none)".to_string() } else { self.query.clone() })],
                ),
            ]),
            text(TextStyle::dim(), vec![s("Type to search, Up/Down select, Enter save, ESC cancel")]),
            boxed(BoxStyle::column().margin_top(1.0), list),
        ];
        if filtered.len() > MAX_VISIBLE_OPTIONS {
            out.push(boxed(
                BoxStyle::default().margin_top(1.0),
                vec![text(TextStyle::dim(), vec![s(format!("Showing {}-{} of {}", start + 1, end, filtered.len()))])],
            ));
        }
        col(out)
    }
}
