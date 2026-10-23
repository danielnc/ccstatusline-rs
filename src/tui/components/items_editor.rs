//! Upstream `ItemsEditor.tsx` and `items-editor/input-handlers.ts`: one line's widgets, with move mode, the
//! two-level widget picker, the clear-line confirmation, and the hide-state and label editors.

use super::confirm::{Answer, Confirm};
use super::hide_label::{Done, HideStatesEditor, LabelEditor};
use super::line_selector::move_item;
use crate::jsobj;
use crate::settings::Settings;
use crate::tui::ink::keys::Input;
use crate::tui::ink::layout::*;
use crate::widgets::catalog::{self, Entry};
use crate::widgets::editor as we;
use crate::widgets::{get_widget, Keybind, Widget};
use serde_json::Value;

const SEPARATOR_CHARS: [&str; 4] = ["|", "-", ",", " "];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PickerAction {
    Change,
    Add,
    Insert,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Level {
    Category,
    Widget,
}

#[derive(Clone, Debug)]
struct Picker {
    action: PickerAction,
    level: Level,
    selected_category: Option<String>,
    category_query: String,
    widget_query: String,
    selected_type: Option<String>,
}

/// The overlay replacing the list: a widget's own editor, or one of the shared ones.
enum Overlay {
    Hide(Value, HideStatesEditor),
    Label(Value, String, LabelEditor),
    /// A widget's `renderEditor`, opened for this action; ported with the widget's group.
    Custom(Value, String),
}

pub struct ItemsEditor {
    selected: usize,
    move_mode: bool,
    picker: Option<Picker>,
    overlay: Option<Overlay>,
    clear_confirm: Option<Confirm>,
}

/// What the editor asks the App to do.
pub enum Outcome {
    None,
    Update(Vec<Value>),
    Back,
}

fn kind(w: &Value) -> &str {
    jsobj::get_str(w, "type").unwrap_or("")
}

fn is_separator_kind(k: &str) -> bool {
    k == "separator" || k == "flex-separator"
}

fn typed(w: &Value) -> crate::settings::WidgetItem {
    crate::settings::parse_widget_item(&jsobj::strip_undefined(w)).unwrap_or_default()
}

fn impl_of(w: &Value) -> Option<&'static dyn Widget> {
    let k = kind(w);
    if is_separator_kind(k) {
        return None;
    }
    get_widget(k)
}

/// `getCustomKeybindsForWidget`.
fn keybinds_for(widget: &dyn Widget, item: &Value) -> Vec<Keybind> {
    let t = typed(item);
    let mut kbs = widget.custom_keybinds(&t);
    if widget.supports_number_format() {
        kbs.push(we::number_format_keybind());
    }
    if !widget.hideable_states().is_empty() {
        kbs.push(we::hide_keybind());
    }
    if widget.label_prefix(&t).is_some() && !t.raw() {
        kbs.push(we::label_keybind());
    }
    kbs
}

/// `getWidgetDisplay` for items without a widget implementation.
fn widget_display(w: &Value) -> String {
    match kind(w) {
        "separator" => {
            let c = jsobj::get_str(w, "character").unwrap_or("|");
            format!("Separator {}", if c == " " { "(space)" } else { c })
        }
        "flex-separator" => "Flex Separator".into(),
        k => match get_widget(k) {
            Some(impl_) => {
                let d = impl_.editor_display(&typed(w));
                match d.modifier {
                    Some(m) => format!("{} {m}", d.text),
                    None => d.text,
                }
            }
            None => format!("Unknown: {k}"),
        },
    }
}

fn merge_of(w: &Value) -> Option<&Value> {
    jsobj::get(w, "merge")
}

fn is_merged_into_previous(widgets: &[Value], index: usize) -> bool {
    index > 0 && widgets.get(index - 1).and_then(merge_of).is_some_and(crate::tui::truthy)
}

/// `getUniqueBackgroundColor`: a random background unlike its neighbours, with a non-custom powerline theme.
fn unique_background(settings: &Settings, widgets: &[Value], insert_index: usize) -> Option<String> {
    if !settings.powerline.enabled || settings.powerline.theme.as_deref() == Some("custom") {
        return None;
    }
    let colors: Vec<&str> = crate::colors::color_names()
        .filter(|n| crate::colors::is_background_color_name(n) && *n != "bgBlack")
        .collect();
    let prev =
        insert_index.checked_sub(1).and_then(|i| widgets.get(i)).and_then(|w| jsobj::get_str(w, "backgroundColor"));
    let next = widgets.get(insert_index).and_then(|w| jsobj::get_str(w, "backgroundColor"));
    let available: Vec<&str> = colors.iter().copied().filter(|c| Some(*c) != prev && Some(*c) != next).collect();
    if !available.is_empty() {
        let i = (crate::tui::random::math_random() * available.len() as f64).floor() as usize;
        return Some(available[i].to_string());
    }
    colors.iter().find(|c| Some(**c) != prev).or(colors.first()).map(|c| c.to_string())
}

impl ItemsEditor {
    pub fn new() -> Self {
        ItemsEditor { selected: 0, move_mode: false, picker: None, overlay: None, clear_confirm: None }
    }

    fn catalog(settings: &Settings) -> (Vec<Entry>, Vec<String>) {
        let cat = catalog::catalog(
            settings.powerline.enabled,
            settings.default_separator.as_deref().is_some_and(|s| !s.is_empty()),
        );
        let mut cats = vec!["All".to_string()];
        cats.extend(catalog::categories(&cat));
        (cat, cats)
    }

    /// `normalizePickerState`.
    fn normalize(p: Picker, cat: &[Entry], cats: &[String]) -> Picker {
        let selected_category = match &p.selected_category {
            Some(c) if cats.contains(c) => Some(c.clone()),
            _ => cats.first().cloned(),
        };
        let top = p.level == Level::Category && !crate::jsnum::js_trim(&p.category_query).is_empty();
        let effective_category =
            if top { "All".to_string() } else { selected_category.clone().unwrap_or_else(|| "All".into()) };
        let query = if top { &p.category_query } else { &p.widget_query };
        let filtered = catalog::filter(cat, &effective_category, query);
        let has_selected = p.selected_type.as_ref().is_some_and(|t| filtered.iter().any(|e| &e.kind == t));
        Picker {
            selected_type: if has_selected {
                p.selected_type.clone()
            } else {
                filtered.first().map(|e| e.kind.clone())
            },
            selected_category,
            ..p
        }
    }

    fn open_picker(&mut self, action: PickerAction, widgets: &[Value], settings: &Settings) {
        let (cat, cats) = Self::catalog(settings);
        if cat.is_empty() {
            return;
        }
        let selected_type =
            if action == PickerAction::Change { widgets.get(self.selected).map(|w| kind(w).to_string()) } else { None };
        self.picker = Some(Self::normalize(
            Picker {
                action,
                level: Level::Category,
                selected_category: Some("All".into()),
                category_query: String::new(),
                widget_query: String::new(),
                selected_type,
            },
            &cat,
            &cats,
        ));
    }

    fn apply_picker(&mut self, selected_type: &str, widgets: &[Value], settings: &Settings) -> Outcome {
        let Some(p) = self.picker.take() else { return Outcome::None };
        match p.action {
            PickerAction::Change => {
                let Some(current) = widgets.get(self.selected) else { return Outcome::None };
                let mut next = widgets.to_vec();
                next[self.selected] = if kind(current) == selected_type {
                    current.clone()
                } else {
                    jsobj::with(&we::clear_label(current), "type", Value::String(selected_type.into()))
                };
                Outcome::Update(next)
            }
            PickerAction::Add | PickerAction::Insert => {
                let insert_index = if p.action == PickerAction::Add {
                    if widgets.is_empty() {
                        0
                    } else {
                        self.selected + 1
                    }
                } else {
                    self.selected
                };
                let background = unique_background(settings, widgets, insert_index);
                let mut item = jsobj::object(vec![
                    ("id", Value::String(crate::tui::random::generate_guid())),
                    ("type", Value::String(selected_type.into())),
                ]);
                if let Some(bg) = background {
                    item = jsobj::with(&item, "backgroundColor", Value::String(bg));
                }
                let mut next = widgets.to_vec();
                next.insert(insert_index.min(next.len()), item);
                self.selected = insert_index;
                Outcome::Update(next)
            }
        }
    }

    pub fn handle(&mut self, ev: &Input, widgets: &[Value], settings: &Settings) -> Outcome {
        // The overlays' own handlers (the items editor's returns early while one is open).
        if let Some(overlay) = &mut self.overlay {
            let done = match overlay {
                Overlay::Hide(w, ed) => {
                    let states = impl_of(w).map(|i| i.hideable_states()).unwrap_or(&[]);
                    ed.handle(ev, w, states)
                }
                Overlay::Label(w, _, ed) => ed.handle(ev, w),
                Overlay::Custom(..) => {
                    if ev.key.escape {
                        Done::Cancel
                    } else {
                        Done::Pending
                    }
                }
            };
            return match done {
                Done::Pending => Outcome::None,
                Done::Cancel => {
                    self.overlay = None;
                    Outcome::None
                }
                Done::Complete(updated) => {
                    self.overlay = None;
                    let mut next = widgets.to_vec();
                    if self.selected < next.len() {
                        next[self.selected] = updated;
                    }
                    Outcome::Update(next)
                }
            };
        }
        if let Some(confirm) = &mut self.clear_confirm {
            return match confirm.handle(ev, false) {
                Answer::Yes => {
                    self.selected = 0;
                    self.clear_confirm = None;
                    Outcome::Update(Vec::new())
                }
                Answer::No => {
                    self.clear_confirm = None;
                    Outcome::None
                }
                Answer::None => Outcome::None,
            };
        }
        if self.picker.is_some() {
            return self.handle_picker(ev, widgets, settings);
        }
        if self.move_mode {
            let n = widgets.len();
            if ev.key.up_arrow && n > 1 {
                let target = if self.selected == 0 { n - 1 } else { self.selected - 1 };
                let next = move_item(widgets, self.selected, target);
                self.selected = target;
                return Outcome::Update(next);
            } else if ev.key.down_arrow && n > 1 {
                let target = if self.selected + 1 > n - 1 { 0 } else { self.selected + 1 };
                let next = move_item(widgets, self.selected, target);
                self.selected = target;
                return Outcome::Update(next);
            } else if ev.key.escape || ev.key.return_ {
                self.move_mode = false;
            }
            return Outcome::None;
        }
        self.handle_normal(ev, widgets, settings)
    }

    fn handle_picker(&mut self, ev: &Input, widgets: &[Value], settings: &Settings) -> Outcome {
        let (cat, cats) = Self::catalog(settings);
        let p = self.picker.clone().unwrap();
        let selected_category = match &p.selected_category {
            Some(c) if cats.contains(c) => Some(c.clone()),
            _ => cats.first().cloned(),
        };
        let top = p.level == Level::Category && !crate::jsnum::js_trim(&p.category_query).is_empty();
        let top_entries: Vec<&Entry> = if top { catalog::filter(&cat, "All", &p.category_query) } else { Vec::new() };
        let top_selected =
            top_entries.iter().find(|e| Some(&e.kind) == p.selected_type.as_ref()).or(top_entries.first()).copied();
        let filtered = catalog::filter(&cat, selected_category.as_deref().unwrap_or("All"), &p.widget_query);
        let selected_entry =
            filtered.iter().find(|e| Some(&e.kind) == p.selected_type.as_ref()).or(filtered.first()).copied();
        let k = &ev.key;
        let typing = !ev.input.is_empty() && !k.ctrl && !k.meta && !k.tab;
        let set = |me: &mut Self, next: Picker| me.picker = Some(Self::normalize(next, &cat, &cats));
        let step = |len: usize, current: Option<usize>, down: bool| {
            let i = current.unwrap_or(0);
            if down {
                if i + 1 > len - 1 {
                    0
                } else {
                    i + 1
                }
            } else if i == 0 {
                len - 1
            } else {
                i - 1
            }
        };
        if p.level == Level::Category {
            if k.escape {
                if !p.category_query.is_empty() {
                    set(self, Picker { category_query: String::new(), ..p });
                } else {
                    self.picker = None;
                }
            } else if k.return_ {
                if top {
                    if let Some(e) = top_selected {
                        let t = e.kind.clone();
                        return self.apply_picker(&t, widgets, settings);
                    }
                } else if let Some(c) = selected_category {
                    set(self, Picker { level: Level::Widget, selected_category: Some(c), ..p });
                }
            } else if k.up_arrow || k.down_arrow {
                if top {
                    if top_entries.is_empty() {
                        return Outcome::None;
                    }
                    let current = top_entries.iter().position(|e| Some(&e.kind) == p.selected_type.as_ref());
                    let next = top_entries[step(top_entries.len(), current, k.down_arrow)].kind.clone();
                    set(self, Picker { selected_type: Some(next), ..p });
                } else {
                    if cats.is_empty() {
                        return Outcome::None;
                    }
                    let current = cats.iter().position(|c| Some(c) == selected_category.as_ref());
                    let next = cats[step(cats.len(), current, k.down_arrow)].clone();
                    set(self, Picker { selected_category: Some(next), ..p });
                }
            } else if k.backspace || k.delete {
                let mut q = p.category_query.clone();
                pop_utf16(&mut q);
                set(self, Picker { category_query: q, selected_type: None, ..p });
            } else if typing {
                let q = format!("{}{}", p.category_query, ev.input);
                set(self, Picker { category_query: q, selected_type: None, ..p });
            }
        } else if k.escape {
            if !p.widget_query.is_empty() {
                set(self, Picker { widget_query: String::new(), ..p });
            } else {
                set(self, Picker { level: Level::Category, ..p });
            }
        } else if k.return_ {
            if let Some(e) = selected_entry {
                let t = e.kind.clone();
                return self.apply_picker(&t, widgets, settings);
            }
        } else if k.up_arrow || k.down_arrow {
            if filtered.is_empty() {
                return Outcome::None;
            }
            let current = filtered.iter().position(|e| Some(&e.kind) == p.selected_type.as_ref());
            let next = filtered[step(filtered.len(), current, k.down_arrow)].kind.clone();
            set(self, Picker { selected_type: Some(next), ..p });
        } else if k.backspace || k.delete {
            let mut q = p.widget_query.clone();
            pop_utf16(&mut q);
            set(self, Picker { widget_query: q, selected_type: None, ..p });
        } else if typing {
            let q = format!("{}{}", p.widget_query, ev.input);
            set(self, Picker { widget_query: q, selected_type: None, ..p });
        }
        Outcome::None
    }

    fn handle_normal(&mut self, ev: &Input, widgets: &[Value], settings: &Settings) -> Outcome {
        let shortcut = if ev.key.ctrl || ev.key.meta { "" } else { ev.input.as_str() };
        let n = widgets.len();
        let k = &ev.key;
        let current = widgets.get(self.selected);
        let can_exclude_align = current.is_some_and(|w| !is_separator_kind(kind(w)))
            && settings.powerline.enabled
            && settings.powerline.auto_align
            && !is_merged_into_previous(widgets, self.selected);
        if k.up_arrow && n > 0 {
            self.selected = if self.selected == 0 { n - 1 } else { self.selected - 1 };
        } else if k.down_arrow && n > 0 {
            self.selected = if self.selected + 1 > n - 1 { 0 } else { self.selected + 1 };
        } else if (k.left_arrow || k.right_arrow) && n > 0 {
            self.open_picker(PickerAction::Change, widgets, settings);
        } else if k.return_ && n > 0 {
            self.move_mode = true;
        } else if shortcut == "a" {
            self.open_picker(PickerAction::Add, widgets, settings);
        } else if shortcut == "i" {
            self.open_picker(PickerAction::Insert, widgets, settings);
        } else if shortcut == "d" && n > 0 {
            let next: Vec<Value> =
                widgets.iter().enumerate().filter(|(i, _)| *i != self.selected).map(|(_, w)| w.clone()).collect();
            if self.selected >= next.len() && self.selected > 0 {
                self.selected -= 1;
            }
            return Outcome::Update(next);
        } else if shortcut == "k" && n > 0 {
            let Some(source) = current else { return Outcome::None };
            let insert_index = self.selected + 1;
            let background = unique_background(settings, widgets, insert_index);
            let mut clone = jsobj::with(source, "id", Value::String(crate::tui::random::generate_guid()));
            if let Some(meta) = jsobj::get(source, "metadata").filter(|m| crate::tui::truthy(m)) {
                clone = jsobj::with(&clone, "metadata", meta.clone());
            }
            if let Some(bg) = background {
                clone = jsobj::with(&clone, "backgroundColor", Value::String(bg));
            }
            let mut next = widgets.to_vec();
            next.insert(insert_index, clone);
            self.selected = insert_index;
            return Outcome::Update(next);
        } else if shortcut == "c" {
            if n > 0 {
                self.clear_confirm = Some(Confirm::default());
            }
        } else if shortcut == " " && n > 0 {
            if let Some(w) = current.filter(|w| kind(w) == "separator") {
                let c = jsobj::get_str(w, "character").unwrap_or("|");
                let i = SEPARATOR_CHARS.iter().position(|x| *x == c).map_or(-1, |i| i as i64);
                let next_char = SEPARATOR_CHARS[((i + 1) as usize) % SEPARATOR_CHARS.len()];
                let mut next = widgets.to_vec();
                next[self.selected] = jsobj::with(w, "character", Value::String(next_char.into()));
                return Outcome::Update(next);
            }
        } else if shortcut == "r" && n > 0 {
            if let Some(w) = current.filter(|w| !is_separator_kind(kind(w))) {
                if !impl_of(w).is_some_and(|i| i.supports_raw_value()) {
                    return Outcome::None;
                }
                let raw = jsobj::get(w, "rawValue").is_some_and(crate::tui::truthy);
                let mut next = widgets.to_vec();
                next[self.selected] = jsobj::with(w, "rawValue", Value::Bool(!raw));
                return Outcome::Update(next);
            }
        } else if shortcut == "m" && n > 0 {
            let is_last = self.selected == n - 1;
            if let Some(w) = current {
                let merge = merge_of(w);
                if (!is_last || merge.is_some_and(crate::tui::truthy)) && !is_separator_kind(kind(w)) {
                    let next_state = match merge {
                        None => Some(Value::Bool(true)),
                        Some(Value::Bool(true)) if !is_last => Some(Value::String("no-padding".into())),
                        _ => None,
                    };
                    let mut next = widgets.to_vec();
                    next[self.selected] = match next_state {
                        None => jsobj::without(w, "merge"),
                        Some(v) => jsobj::with(w, "merge", v),
                    };
                    return Outcome::Update(next);
                }
            }
        } else if shortcut == "x" && n > 0 {
            if let Some(w) = current.filter(|w| can_exclude_align && !is_separator_kind(kind(w))) {
                let mut next = widgets.to_vec();
                next[self.selected] = if jsobj::get(w, "excludeFromAutoAlign").is_some_and(crate::tui::truthy) {
                    jsobj::without(w, "excludeFromAutoAlign")
                } else {
                    jsobj::with(w, "excludeFromAutoAlign", Value::Bool(true))
                };
                return Outcome::Update(next);
            }
        } else if k.escape {
            return Outcome::Back;
        } else if n > 0 {
            let Some(w) = current.filter(|w| !is_separator_kind(kind(w))) else { return Outcome::None };
            let Some(widget) = impl_of(w) else { return Outcome::None };
            let kbs = keybinds_for(widget, w);
            let Some(kb) = kbs.iter().find(|kb| kb.key == shortcut) else { return Outcome::None };
            if kb.action == we::EDIT_HIDE_STATES_ACTION {
                self.overlay = Some(Overlay::Hide(w.clone(), HideStatesEditor::new(w, widget.hideable_states())));
                return Outcome::None;
            }
            if kb.action == we::EDIT_LABEL_ACTION {
                let default_label = widget.label_prefix(&typed(w)).unwrap_or_default();
                self.overlay =
                    Some(Overlay::Label(w.clone(), default_label.clone(), LabelEditor::new(w, &default_label)));
                return Outcome::None;
            }
            if kb.action == we::CYCLE_NUMBER_STYLE_ACTION {
                let mut next = widgets.to_vec();
                next[self.selected] = we::cycle_number_style(w);
                return Outcome::Update(next);
            }
            match widget.handle_editor_action(&kb.action, w) {
                Some(updated) => {
                    let mut next = widgets.to_vec();
                    next[self.selected] = updated;
                    return Outcome::Update(next);
                }
                None => self.overlay = Some(Overlay::Custom(w.clone(), kb.action.clone())),
            }
        }
        Outcome::None
    }

    pub fn view(&mut self, widgets: &[Value], line_number: usize, settings: &Settings) -> El {
        match &self.overlay {
            Some(Overlay::Hide(w, ed)) => {
                let states = impl_of(w).map(|i| i.hideable_states()).unwrap_or(&[]);
                return ed.view(w, states);
            }
            Some(Overlay::Label(_, default_label, ed)) => return ed.view(default_label),
            Some(Overlay::Custom(..)) => return col(Vec::new()),
            None => {}
        }
        if let Some(confirm) = &mut self.clear_confirm {
            return col(vec![
                text(TextStyle::bold().with_color(Some("yellow")), vec![s("⚠ Confirm Clear Line")]),
                boxed(
                    BoxStyle::column().margin_top(1.0),
                    vec![
                        plain_parts(vec![
                            "This will remove all widgets from Line".into(),
                            " ".into(),
                            line_number.to_string(),
                            ".".into(),
                        ]),
                        text(TextStyle::color("red"), vec![s("This action cannot be undone!")]),
                    ],
                ),
                boxed(BoxStyle::default().margin_top(2.0), vec![plain("Continue?")]),
                boxed(BoxStyle::default().margin_top(1.0), vec![confirm.view(None, true, false)]),
            ]);
        }

        let current = widgets.get(self.selected);
        let is_sep = current.is_some_and(|w| kind(w) == "separator");
        let is_flex = current.is_some_and(|w| kind(w) == "flex-separator");
        let mut can_toggle_raw = false;
        let mut custom: Vec<Keybind> = Vec::new();
        if let Some(w) = current.filter(|_| !is_sep && !is_flex) {
            if let Some(widget) = impl_of(w) {
                can_toggle_raw = widget.supports_raw_value();
                custom = keybinds_for(widget, w);
            }
        }
        let can_merge = current.is_some_and(|w| {
            (self.selected + 1 < widgets.len() || merge_of(w).is_some_and(crate::tui::truthy)) && !is_sep && !is_flex
        });
        let can_exclude_align = current.is_some()
            && !is_sep
            && !is_flex
            && settings.powerline.enabled
            && settings.powerline.auto_align
            && !is_merged_into_previous(widgets, self.selected);
        let has_widgets = !widgets.is_empty();

        let mut help = if has_widgets {
            "↑↓ select, ←→ open type picker".to_string()
        } else {
            "(a)dd via picker, (i)nsert via picker".to_string()
        };
        if is_sep {
            help.push_str(", Space edit separator");
        }
        if has_widgets {
            help.push_str(", Enter to move, (a)dd via picker, (i)nsert via picker, (k) clone, (d)elete, (c)lear line");
        }
        if can_toggle_raw {
            help.push_str(", (r)aw value");
        }
        if can_merge {
            help.push_str(", (m)erge");
        }
        if can_exclude_align {
            help.push_str(", e(x)clude align");
        }
        help.push_str(", ESC back");
        let custom_text = custom.iter().map(|k| k.label.as_str()).collect::<Vec<_>>().join(", ");

        let picker = self.picker.clone();
        let action_label = match picker.as_ref().map(|p| p.action) {
            Some(PickerAction::Add) => "Add Widget",
            Some(PickerAction::Insert) => "Insert Widget",
            _ => "Change Widget Type",
        };

        let mut header =
            vec![text(TextStyle::bold(), vec![s("Edit Line"), s(" "), s(line_number.to_string()), s(" ")])];
        if self.move_mode {
            header.push(text(TextStyle::color("blue"), vec![s("[MOVE MODE]")]));
        }
        if picker.is_some() {
            header.push(text(TextStyle::color("cyan"), vec![s(format!("[{}]", action_label.to_uppercase()))]));
        }
        let default_sep = settings.default_separator.as_deref().is_some_and(|s| !s.is_empty());
        if settings.powerline.enabled || default_sep {
            let msg = if settings.powerline.enabled {
                "Powerline mode active: manual separators disabled"
            } else {
                "Default separator active: manual separators disabled"
            };
            header.push(boxed(
                BoxStyle::default().margin_left(2.0),
                vec![text(TextStyle::color("yellow"), vec![s("⚠"), s(" "), s(msg)])],
            ));
        }
        let mut children = vec![row(header)];

        let (cat, cats) = Self::catalog(settings);
        if self.move_mode {
            children.push(boxed(
                BoxStyle::column().margin_bottom(1.0),
                vec![text(TextStyle::dim(), vec![s("↑↓ to move widget, ESC or Enter to exit move mode")])],
            ));
        } else if let Some(p) = &picker {
            let mut rows = Vec::new();
            if p.level == Level::Category {
                if !crate::jsnum::js_trim(&p.category_query).is_empty() {
                    rows.push(text(TextStyle::dim(), vec![s("↑↓ select widget match, Enter apply, ESC clear/cancel")]));
                } else {
                    rows.push(text(
                        TextStyle::dim(),
                        vec![s("↑↓ select category, type to search all widgets, Enter continue, ESC cancel")],
                    ));
                }
                rows.push(row(vec![
                    text(TextStyle::dim(), vec![s("Search: ")]),
                    text(
                        TextStyle::color("cyan"),
                        vec![s(if p.category_query.is_empty() { "(none)" } else { &p.category_query })],
                    ),
                ]));
            } else {
                let sel_cat = match &p.selected_category {
                    Some(c) if cats.contains(c) => c.clone(),
                    _ => cats.first().cloned().unwrap_or_else(|| "(none)".into()),
                };
                rows.push(text(
                    TextStyle::dim(),
                    vec![s("↑↓ select widget, type to search widgets, Enter apply, ESC back")],
                ));
                rows.push(row(vec![
                    text(TextStyle::dim(), vec![s("Category:"), s(" "), s(sel_cat), s(" "), s("| Search:"), s(" ")]),
                    text(
                        TextStyle::color("cyan"),
                        vec![s(if p.widget_query.is_empty() { "(none)" } else { &p.widget_query })],
                    ),
                ]));
            }
            children.push(col(rows));
        } else {
            children.push(col(vec![
                text(TextStyle::dim(), vec![s(help)]),
                text(TextStyle::dim(), vec![s(if custom_text.is_empty() { " ".to_string() } else { custom_text })]),
            ]));
        }

        let has_flex = widgets.iter().any(|w| kind(w) == "flex-separator");
        if has_flex && crate::terminal::terminal_width(None, 0.0).is_none() {
            children.push(boxed(
                BoxStyle::default().margin_top(1.0),
                vec![
                    text(
                        TextStyle::color("yellow"),
                        vec![s("⚠ Note: Terminal width detection is currently unavailable in your environment.")],
                    ),
                    text(
                        TextStyle::dim(),
                        vec![s("  Flex separators will act as normal separators until width detection is available.")],
                    ),
                ],
            ));
        }

        if let Some(p) = &picker {
            children.push(boxed(BoxStyle::column().margin_top(1.0), picker_rows(p, &cat, &cats)));
        } else {
            let mut rows = Vec::new();
            if widgets.is_empty() {
                rows.push(text(TextStyle::dim(), vec![s("No widgets. Press 'a' to add one.")]));
            } else {
                for (index, w) in widgets.iter().enumerate() {
                    rows.push(self.widget_row(widgets, index, w, settings));
                }
                if let Some(w) = current {
                    let description = match kind(w) {
                        "separator" => "A separator character between status line widgets".to_string(),
                        "flex-separator" => "Expands to fill available terminal width".to_string(),
                        k => get_widget(k)
                            .map_or_else(|| "Unknown widget type".to_string(), |i| i.description().to_string()),
                    };
                    rows.push(boxed(
                        BoxStyle::default().margin_top(1.0).padding_left(2.0),
                        vec![text(TextStyle::dim(), vec![s(description)])],
                    ));
                }
            }
            children.push(boxed(BoxStyle::column().margin_top(1.0), rows));
        }
        col(children)
    }

    fn widget_row(&self, widgets: &[Value], index: usize, w: &Value, settings: &Settings) -> El {
        let selected = index == self.selected;
        let color = selected.then_some(if self.move_mode { "blue" } else { "green" });
        let impl_ = impl_of(w);
        let t = typed(w);
        let (display_text, modifier) = match impl_ {
            Some(i) => {
                let d = i.editor_display(&t);
                (d.text, d.modifier)
            }
            None => (widget_display(w), None),
        };
        let display_text = if display_text.is_empty() { widget_display(w) } else { display_text };
        let mut cells = vec![
            boxed(
                BoxStyle::default().width(Width::Points(3.0)),
                vec![text(
                    TextStyle::default().with_color(color),
                    vec![s(if selected {
                        if self.move_mode {
                            "◆ "
                        } else {
                            "▶ "
                        }
                    } else {
                        "  "
                    })],
                )],
            ),
            text(TextStyle::default().with_color(color), vec![s(format!("{}. {display_text}", index + 1))]),
        ];
        let dim_part = |cells: &mut Vec<El>, m: Option<String>| {
            if let Some(m) = m.filter(|m| !m.is_empty()) {
                cells.push(text(TextStyle::dim(), vec![s(" "), s(m)]));
            }
        };
        dim_part(&mut cells, modifier);
        if let Some(i) = impl_ {
            if i.supports_number_format() {
                dim_part(&mut cells, we::number_format_modifier_text(w));
            }
            dim_part(&mut cells, we::hide_modifier_text(w, i.hideable_states()));
            if i.label_prefix(&t).is_some() && !t.raw() {
                dim_part(&mut cells, we::label_modifier_text(w));
            }
            if i.supports_raw_value() && t.raw() {
                cells.push(text(TextStyle::dim(), vec![s(" (raw value)")]));
            }
        }
        match merge_of(w) {
            Some(Value::Bool(true)) => cells.push(text(TextStyle::dim(), vec![s(" (merged→)")])),
            Some(Value::String(m)) if m == "no-padding" => {
                cells.push(text(TextStyle::dim(), vec![s(" (merged-no-pad→)")]))
            }
            _ => {}
        }
        if jsobj::get(w, "excludeFromAutoAlign").is_some_and(crate::tui::truthy)
            && settings.powerline.enabled
            && settings.powerline.auto_align
            && !is_merged_into_previous(widgets, index)
        {
            cells.push(text(TextStyle::dim(), vec![s(" (no-align)")]));
        }
        row(cells)
    }
}

fn plain_parts(parts: Vec<String>) -> El {
    text(TextStyle::default(), parts.into_iter().map(s).collect())
}

fn pop_utf16(s: &mut String) {
    // `.slice(0, -1)` drops the last UTF-16 unit; a lone surrogate cannot be kept in a Rust string, so an astral
    // character goes whole.
    s.pop();
}

fn picker_rows(p: &Picker, cat: &[Entry], cats: &[String]) -> Vec<El> {
    let selected_category = match &p.selected_category {
        Some(c) if cats.contains(c) => Some(c.clone()),
        _ => cats.first().cloned(),
    };
    let entry_row = |index: usize, e: &Entry, selected: bool, query: &str, bold_selected: bool| {
        let color = selected.then_some("green");
        let mut cells = vec![
            boxed(
                BoxStyle::default().width(Width::Points(3.0)),
                vec![text(TextStyle::default().with_color(color), vec![s(if selected { "▶ " } else { "  " })])],
            ),
            text(TextStyle::default().with_color(color), vec![s(format!("{}. ", index + 1))]),
        ];
        for (seg, matched) in catalog::match_segments(&e.display_name, query) {
            let color = if selected {
                Some("green")
            } else if matched {
                Some("yellowBright")
            } else {
                None
            };
            let bold = if selected && bold_selected { true } else { matched };
            cells.push(text(TextStyle::default().with_color(color).with_bold(bold), vec![s(seg)]));
        }
        row(cells)
    };
    let mut rows = Vec::new();
    if p.level == Level::Category {
        if !crate::jsnum::js_trim(&p.category_query).is_empty() {
            let entries = catalog::filter(cat, "All", &p.category_query);
            if entries.is_empty() {
                rows.push(text(TextStyle::dim(), vec![s("No widgets match the search.")]));
            } else {
                let sel = entries
                    .iter()
                    .find(|e| Some(&e.kind) == p.selected_type.as_ref())
                    .or(entries.first())
                    .map(|e| e.kind.clone());
                for (i, e) in entries.iter().enumerate() {
                    rows.push(entry_row(i, e, Some(&e.kind) == sel.as_ref(), &p.category_query, true));
                }
                if let Some(e) = entries.iter().find(|e| Some(&e.kind) == sel.as_ref()) {
                    rows.push(boxed(
                        BoxStyle::default().margin_top(1.0).padding_left(2.0),
                        vec![text(TextStyle::dim(), vec![s(e.description.clone())])],
                    ));
                }
            }
        } else if cats.is_empty() {
            rows.push(text(TextStyle::dim(), vec![s("No categories available.")]));
        } else {
            for (i, c) in cats.iter().enumerate() {
                let selected = Some(c) == selected_category.as_ref();
                let color = selected.then_some("green");
                rows.push(row(vec![
                    boxed(
                        BoxStyle::default().width(Width::Points(3.0)),
                        vec![text(TextStyle::default().with_color(color), vec![s(if selected { "▶ " } else { "  " })])],
                    ),
                    text(TextStyle::default().with_color(color), vec![s(format!("{}. {c}", i + 1))]),
                ]));
            }
            if selected_category.as_deref() == Some("All") {
                rows.push(boxed(
                    BoxStyle::default().margin_top(1.0).padding_left(2.0),
                    vec![text(TextStyle::dim(), vec![s("Search across all widget categories.")])],
                ));
            }
        }
    } else {
        let entries = catalog::filter(cat, selected_category.as_deref().unwrap_or("All"), &p.widget_query);
        if entries.is_empty() {
            rows.push(text(TextStyle::dim(), vec![s("No widgets match the current category/search.")]));
        } else {
            let sel = entries
                .iter()
                .find(|e| Some(&e.kind) == p.selected_type.as_ref())
                .or(entries.first())
                .map(|e| e.kind.clone());
            for (i, e) in entries.iter().enumerate() {
                rows.push(entry_row(i, e, Some(&e.kind) == sel.as_ref(), &p.widget_query, false));
            }
            if let Some(e) = entries.iter().find(|e| Some(&e.kind) == sel.as_ref()) {
                rows.push(boxed(
                    BoxStyle::default().margin_top(1.0).padding_left(2.0),
                    vec![text(TextStyle::dim(), vec![s(e.description.clone())])],
                ));
            }
        }
    }
    rows
}
