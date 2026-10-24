//! Upstream `ColorMenu.tsx` and `color-menu/mutations.ts`: one line's widgets with their foreground, background,
//! bold and dim, the hex, ansi256 and gradient entry modes, the separators toggle and the clear-all confirmation.

use super::confirm::{Answer, Confirm};
use crate::colors::{apply_colors, chalk_gray, chalk_wrap, color_names, is_background_color_name, Dim, Level};
use crate::jsobj;
use crate::tui::ink::keys::Input;
use crate::tui::ink::layout::*;
use crate::tui::screens::{Action, Ctx, Screen};
use crate::tui::text_cursor::should_insert_input;
use crate::widgets::get_widget;
use serde_json::Value;

// --- mutations.ts ------------------------------------------------------------------------------------------

fn id_of(w: &Value) -> Option<&str> {
    jsobj::get_str(w, "id")
}

fn kind(w: &Value) -> &str {
    jsobj::get_str(w, "type").unwrap_or("")
}

/// `updateWidgetById`.
pub fn update_widget_by_id(widgets: &[Value], widget_id: &str, updater: impl Fn(&Value) -> Value) -> Vec<Value> {
    widgets.iter().map(|w| if id_of(w) == Some(widget_id) { updater(w) } else { w.clone() }).collect()
}

/// `setWidgetColor`.
pub fn set_widget_color(widgets: &[Value], widget_id: &str, color: &str, editing_background: bool) -> Vec<Value> {
    let key = if editing_background { "backgroundColor" } else { "color" };
    update_widget_by_id(widgets, widget_id, |w| jsobj::with(w, key, Value::String(color.to_string())))
}

/// `toggleWidgetBold`.
pub fn toggle_widget_bold(widgets: &[Value], widget_id: &str) -> Vec<Value> {
    update_widget_by_id(widgets, widget_id, |w| {
        let bold = jsobj::get(w, "bold").is_some_and(crate::tui::truthy);
        jsobj::with(w, "bold", Value::Bool(!bold))
    })
}

/// `cycleWidgetDim`: off -> whole widget -> `(...)` spans only -> off.
pub fn cycle_widget_dim(widgets: &[Value], widget_id: &str) -> Vec<Value> {
    update_widget_by_id(widgets, widget_id, |w| match jsobj::get(w, "dim") {
        Some(Value::Bool(true)) => jsobj::with(w, "dim", Value::String("parens".into())),
        Some(Value::String(d)) if d == "parens" => jsobj::without(w, "dim"),
        _ => jsobj::with(w, "dim", Value::Bool(true)),
    })
}

const STYLING_KEYS: [&str; 4] = ["color", "backgroundColor", "bold", "dim"];

fn without_styling(w: &Value) -> Value {
    STYLING_KEYS.iter().fold(w.clone(), |w, k| jsobj::without(&w, k))
}

/// `resetWidgetStyling`.
pub fn reset_widget_styling(widgets: &[Value], widget_id: &str) -> Vec<Value> {
    update_widget_by_id(widgets, widget_id, without_styling)
}

/// `clearAllWidgetStyling`.
pub fn clear_all_widget_styling(widgets: &[Value]) -> Vec<Value> {
    widgets.iter().map(without_styling).collect()
}

/// `getDefaultForegroundColor`.
fn default_foreground_color(w: &Value) -> String {
    let k = kind(w);
    if k == "separator" || k == "flex-separator" {
        return "white".into();
    }
    get_widget(k).map(|i| i.default_color()).unwrap_or("white").to_string()
}

fn next_index(current: usize, len: usize, right: bool) -> usize {
    if right {
        (current + 1) % len
    } else if current == 0 {
        len - 1
    } else {
        current - 1
    }
}

/// `cycleWidgetColor`.
pub fn cycle_widget_color(
    widgets: &[Value],
    widget_id: &str,
    right: bool,
    editing_background: bool,
    colors: &[&str],
    background_colors: &[&str],
) -> Vec<Value> {
    update_widget_by_id(widgets, widget_id, |w| {
        if editing_background {
            if background_colors.is_empty() {
                return w.clone();
            }
            let current = jsobj::get_str(w, "backgroundColor").unwrap_or("");
            let index = background_colors.iter().position(|c| *c == current).unwrap_or(0);
            let next = background_colors[next_index(index, background_colors.len(), right)];
            return jsobj::with(
                w,
                "backgroundColor",
                if next.is_empty() { jsobj::undefined() } else { Value::String(next.into()) },
            );
        }
        if colors.is_empty() {
            return w.clone();
        }
        let default = default_foreground_color(w);
        let mut current = jsobj::get_str(w, "color").map(str::to_string).unwrap_or_else(|| default.clone());
        if current == "dim" {
            current = default;
        }
        let index = colors.iter().position(|c| *c == current).unwrap_or(0);
        let next = colors[next_index(index, colors.len(), right)];
        jsobj::with(w, "color", Value::String(next.into()))
    })
}

// --- ColorMenu.tsx -----------------------------------------------------------------------------------------

/// `getAvailableColorsForUI` / `getAvailableBackgroundColorsForUI`: `(displayName, value)`, `Default` first.
fn color_options(background: bool) -> Vec<(String, &'static str)> {
    let mut out = vec![("Default".to_string(), "")];
    for name in color_names().filter(|n| is_background_color_name(n) == background) {
        out.push((display_name(name), name));
    }
    out
}

/// `COLOR_MAP`'s `displayName`: `brightRed` and `bgBrightRed` are both `Bright Red`.
fn display_name(name: &str) -> String {
    let base = name.strip_prefix("bg").unwrap_or(name);
    let mut out = String::new();
    for (i, c) in base.chars().enumerate() {
        if i == 0 {
            out.extend(c.to_uppercase());
        } else {
            if c.is_ascii_uppercase() {
                out.push(' ');
            }
            out.push(c);
        }
    }
    out
}

fn gradient_preset_names() -> impl Iterator<Item = &'static str> {
    crate::gradient::PRESETS.iter().map(|(n, _)| *n)
}

fn chalk_yellow(text: &str) -> String {
    chalk_wrap("\x1b[33m", "\x1b[39m", text)
}

fn chalk_green(text: &str) -> String {
    chalk_wrap("\x1b[32m", "\x1b[39m", text)
}

fn typed(w: &Value) -> crate::settings::WidgetItem {
    crate::settings::parse_widget_item(&jsobj::strip_undefined(w)).unwrap_or_default()
}

fn dim_of(w: &Value) -> Dim {
    match jsobj::get(w, "dim") {
        Some(Value::Bool(true)) => Dim::Yes,
        Some(Value::String(d)) if d == "parens" => Dim::Parens,
        _ => Dim::No,
    }
}

/// `getItemLabel`.
fn item_label(w: &Value) -> String {
    match kind(w) {
        "separator" => {
            let c = jsobj::get_str(w, "character").unwrap_or("|");
            format!("Separator: {}", if c == " " { "space" } else { c })
        }
        "flex-separator" => "Flex Separator".into(),
        k => match get_widget(k) {
            Some(i) => i.display_name().to_string(),
            None => format!("Unknown: {k}"),
        },
    }
}

/// `!!value` for an optional string setting.
fn set(v: &Option<String>) -> bool {
    v.as_deref().is_some_and(|s| !s.is_empty())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GradientStep {
    Start,
    End,
}

/// ink-select-input's state. ColorMenu keys it on `${showSeparators}-${highlightedItemId}`, so it mounts afresh
/// (at the highlighted item) whenever either changes.
struct Select {
    key: (bool, Option<String>),
    values: Vec<String>,
    index: usize,
}

pub struct ColorMenu {
    show_separators: bool,
    hex_input_mode: bool,
    hex_input: String,
    ansi256_input_mode: bool,
    ansi256_input: String,
    clear_confirm: Option<Confirm>,
    gradient_mode: bool,
    gradient_index: usize,
    gradient_custom_step: Option<GradientStep>,
    gradient_start_hex: String,
    gradient_hex_input: String,
    highlighted: Option<String>,
    editing_background: bool,
    /// The SelectInput, while the last render mounted it.
    select: Option<Select>,
}

impl ColorMenu {
    pub fn new(ctx: &Ctx) -> Self {
        let mut menu = ColorMenu {
            show_separators: false,
            hex_input_mode: false,
            hex_input: String::new(),
            ansi256_input_mode: false,
            ansi256_input: String::new(),
            clear_confirm: None,
            gradient_mode: false,
            gradient_index: 0,
            gradient_custom_step: None,
            gradient_start_hex: String::new(),
            gradient_hex_input: String::new(),
            highlighted: None,
            editing_background: false,
            select: None,
        };
        let widgets = ctx.line_widgets();
        menu.highlighted = menu.colorable(&widgets).first().and_then(|w| id_of(w)).map(str::to_string);
        menu
    }

    /// `colorableWidgets`.
    fn colorable<'a>(&self, widgets: &'a [Value]) -> Vec<&'a Value> {
        widgets
            .iter()
            .filter(|w| {
                if kind(w) == "separator" {
                    return self.show_separators;
                }
                get_widget(kind(w)).map(|i| i.supports_colors(&typed(w))).unwrap_or(true)
            })
            .collect()
    }

    /// The menu items' values: the colorable widgets' ids, then `back`.
    fn menu_values(&self, widgets: &[Value]) -> Vec<String> {
        let mut values: Vec<String> =
            self.colorable(widgets).iter().map(|w| id_of(w).unwrap_or("").to_string()).collect();
        values.push("back".into());
        values
    }

    /// SelectInput's own `useInput` (it is a child, so it sees each key first). No `onSelect` is passed, so only
    /// the highlight moves.
    fn handle_select(&mut self, ev: &Input, widgets: &[Value]) {
        let values = self.menu_values(widgets);
        let Some(sel) = self.select.as_mut() else { return };
        let n = values.len();
        let mut next = None;
        if ev.input == "k" || ev.key.up_arrow {
            next = Some(if sel.index == 0 { n - 1 } else { sel.index - 1 });
        }
        if ev.input == "j" || ev.key.down_arrow {
            next = Some(if sel.index == n - 1 { 0 } else { sel.index + 1 });
        }
        if let Some(i) = next {
            sel.index = i;
            self.highlighted = values.get(i).cloned();
        }
    }

    fn handle_hex(&mut self, ev: &Input, widgets: &[Value], colorable: &[&Value], hl: Option<&str>) -> Vec<Action> {
        let k = &ev.key;
        if k.up_arrow || k.down_arrow {
            return Vec::new();
        }
        let mut out = Vec::new();
        if k.escape {
            self.hex_input_mode = false;
            self.hex_input.clear();
        } else if k.return_ {
            if self.hex_input.len() == 6 {
                let color = format!("hex:{}", self.hex_input);
                if let Some(w) = colorable.iter().find(|w| id_of(w) == hl) {
                    out.push(Action::SetLineWidgets(set_widget_color(
                        widgets,
                        id_of(w).unwrap_or(""),
                        &color,
                        self.editing_background,
                    )));
                }
                self.hex_input_mode = false;
                self.hex_input.clear();
            }
        } else if k.backspace || k.delete {
            self.hex_input.pop();
        } else if should_insert_input(&ev.input, k) && self.hex_input.len() < 6 {
            if let Some(c) = single_hex_digit(&ev.input) {
                self.hex_input.push(c);
            }
        }
        out
    }

    fn handle_ansi256(&mut self, ev: &Input, widgets: &[Value], colorable: &[&Value], hl: Option<&str>) -> Vec<Action> {
        let k = &ev.key;
        if k.up_arrow || k.down_arrow {
            return Vec::new();
        }
        let mut out = Vec::new();
        if k.escape {
            self.ansi256_input_mode = false;
            self.ansi256_input.clear();
        } else if k.return_ {
            if let Ok(code) = self.ansi256_input.parse::<u32>() {
                if code <= 255 {
                    if let Some(w) = colorable.iter().find(|w| id_of(w) == hl) {
                        out.push(Action::SetLineWidgets(set_widget_color(
                            widgets,
                            id_of(w).unwrap_or(""),
                            &format!("ansi256:{code}"),
                            self.editing_background,
                        )));
                        self.ansi256_input_mode = false;
                        self.ansi256_input.clear();
                    }
                }
            }
        } else if k.backspace || k.delete {
            self.ansi256_input.pop();
        } else if should_insert_input(&ev.input, k)
            && self.ansi256_input.len() < 3
            && ev.input.len() == 1
            && ev.input.as_bytes()[0].is_ascii_digit()
        {
            let next = format!("{}{}", self.ansi256_input, ev.input);
            if next.parse::<u32>().is_ok_and(|c| c <= 255) {
                self.ansi256_input = next;
            }
        }
        out
    }

    fn exit_gradient(&mut self) {
        self.gradient_mode = false;
        self.gradient_custom_step = None;
        self.gradient_start_hex.clear();
        self.gradient_hex_input.clear();
    }

    fn handle_gradient(
        &mut self,
        ev: &Input,
        widgets: &[Value],
        colorable: &[&Value],
        hl: Option<&str>,
    ) -> Vec<Action> {
        let k = &ev.key;
        let mut out = Vec::new();
        let mut apply = |menu: &mut ColorMenu, value: String| {
            if let Some(w) = colorable.iter().find(|w| id_of(w) == hl) {
                out.push(Action::SetLineWidgets(set_widget_color(widgets, id_of(w).unwrap_or(""), &value, false)));
            }
            menu.exit_gradient();
        };
        if let Some(step) = self.gradient_custom_step {
            if k.escape {
                self.gradient_custom_step = None;
                self.gradient_hex_input.clear();
            } else if k.return_ {
                if self.gradient_hex_input.len() == 6 {
                    if step == GradientStep::Start {
                        self.gradient_start_hex = std::mem::take(&mut self.gradient_hex_input);
                        self.gradient_custom_step = Some(GradientStep::End);
                    } else {
                        let value = format!("gradient:{}-{}", self.gradient_start_hex, self.gradient_hex_input);
                        apply(self, value);
                    }
                }
            } else if k.backspace || k.delete {
                self.gradient_hex_input.pop();
            } else if should_insert_input(&ev.input, k) && self.gradient_hex_input.len() < 6 {
                if let Some(c) = single_hex_digit(&ev.input) {
                    self.gradient_hex_input.push(c);
                }
            }
            return out;
        }
        let names: Vec<&str> = gradient_preset_names().collect();
        let total = names.len() + 1;
        if k.escape {
            self.exit_gradient();
        } else if k.up_arrow {
            self.gradient_index = (self.gradient_index + total - 1) % total;
        } else if k.down_arrow {
            self.gradient_index = (self.gradient_index + 1) % total;
        } else if k.return_ {
            if self.gradient_index < names.len() {
                apply(self, format!("gradient:{}", names[self.gradient_index]));
            } else {
                self.gradient_start_hex.clear();
                self.gradient_hex_input.clear();
                self.gradient_custom_step = Some(GradientStep::Start);
            }
        }
        out
    }

    fn view_gradient(&self, selected: Option<&Value>, level: Level) -> El {
        let widget_name = selected.map(item_label).unwrap_or_default();
        let suffix = if widget_name.is_empty() { String::new() } else { format!(" - {widget_name}") };
        if let Some(step) = self.gradient_custom_step {
            let mut body = vec![plain(if step == GradientStep::Start {
                "Enter START hex color (without #):"
            } else {
                "Enter END hex color (without #):"
            })];
            if step == GradientStep::End {
                body.push(text(TextStyle::dim(), vec![s("Start: #"), s(self.gradient_start_hex.clone())]));
            }
            body.push(hex_field(&self.gradient_hex_input));
            body.push(plain(" "));
            body.push(text(TextStyle::dim(), vec![s("Press Enter when done, ESC to go back")]));
            return col(vec![
                text(TextStyle::bold(), vec![s("Custom Gradient"), s(suffix)]),
                boxed(BoxStyle::column().margin_top(1.0), body),
            ]);
        }
        let names: Vec<&str> = gradient_preset_names().collect();
        let mut rows: Vec<El> = names
            .iter()
            .enumerate()
            .map(|(idx, name)| {
                let sel = idx == self.gradient_index;
                plain_parts(vec![
                    (if sel { "▶ " } else { "  " }).into(),
                    apply_colors(name, Some(&format!("gradient:{name}")), None, sel, level, Dim::No),
                ])
            })
            .collect();
        rows.push(plain_parts(vec![
            (if self.gradient_index == names.len() { "▶ " } else { "  " }).into(),
            "Custom (enter two hex stops)".into(),
        ]));
        col(vec![
            text(TextStyle::bold(), vec![s("Select Gradient"), s(suffix)]),
            boxed(
                BoxStyle::default().margin_top(1.0),
                vec![text(TextStyle::dim(), vec![s("↑↓ to select, Enter to apply, ESC to cancel")])],
            ),
            boxed(BoxStyle::column().margin_top(1.0), rows),
        ])
    }
}

/// `input.toUpperCase()` when it is a single hex digit.
fn single_hex_digit(input: &str) -> Option<char> {
    let upper = input.to_uppercase();
    let mut chars = upper.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_hexdigit() => Some(c),
        _ => None,
    }
}

fn plain_parts(parts: Vec<String>) -> El {
    text(TextStyle::default(), parts.into_iter().map(s).collect())
}

/// `#{input}<Text dimColor>{'_' padding to 6}</Text>`.
fn hex_field(input: &str) -> El {
    text(
        TextStyle::default(),
        vec![s("#"), s(input), nested(TextStyle::dim(), vec![s("_".repeat(6usize.saturating_sub(input.len())))])],
    )
}

impl Screen for ColorMenu {
    fn handle(&mut self, ev: &Input, ctx: &Ctx) -> Vec<Action> {
        let widgets = ctx.line_widgets();
        let settings = ctx.typed;
        // The state this key's handlers see is the last render's.
        let hl = self.highlighted.clone();
        let hl = hl.as_deref();
        if let Some(confirm) = &mut self.clear_confirm {
            return match confirm.handle(ev, false) {
                Answer::Yes => {
                    self.clear_confirm = None;
                    vec![Action::SetLineWidgets(clear_all_widget_styling(&widgets))]
                }
                Answer::No => {
                    self.clear_confirm = None;
                    Vec::new()
                }
                Answer::None => Vec::new(),
            };
        }
        self.handle_select(ev, &widgets);

        let colorable = self.colorable(&widgets);
        if colorable.is_empty() {
            return vec![Action::Back];
        }
        if self.hex_input_mode {
            return self.handle_hex(ev, &widgets, &colorable, hl);
        }
        if self.ansi256_input_mode {
            return self.handle_ansi256(ev, &widgets, &colorable, hl);
        }
        if self.gradient_mode {
            return self.handle_gradient(ev, &widgets, &colorable, hl);
        }

        let input = ev.input.as_str();
        if input.len() == 1 && input.as_bytes()[0].is_ascii_digit() {
            return Vec::new();
        }
        let k = &ev.key;
        let shortcut = if k.ctrl || k.meta { "" } else { input };
        let on_item = hl.is_some_and(|h| h != "back");
        let selected = colorable.iter().find(|w| id_of(w) == hl).copied();
        let selected_id = selected.and_then(id_of).unwrap_or("");
        let mut out = Vec::new();
        if k.escape {
            if self.editing_background {
                self.editing_background = false;
            } else {
                out.push(Action::Back);
            }
        } else if k.return_ {
            // Back is chosen here rather than through SelectInput's onSelect, which number keys fire too.
            if hl == Some("back") {
                out.push(Action::Back);
            }
        } else if shortcut == "h" || shortcut == "H" {
            if on_item && settings.color_level == 3 {
                self.hex_input_mode = true;
                self.hex_input.clear();
            }
        } else if shortcut == "a" || shortcut == "A" {
            if on_item && settings.color_level == 2 {
                self.ansi256_input_mode = true;
                self.ansi256_input.clear();
            }
        } else if shortcut == "g" || shortcut == "G" {
            if on_item && !self.editing_background && settings.color_level >= 2 {
                self.gradient_mode = true;
                self.gradient_index = 0;
                self.gradient_custom_step = None;
                self.gradient_start_hex.clear();
                self.gradient_hex_input.clear();
            }
        } else if shortcut == "s" || shortcut == "S" {
            if !settings.powerline.enabled && !set(&settings.default_separator) {
                let was_showing = self.show_separators;
                self.show_separators = !was_showing;
                // Hiding the highlighted separator takes it off the list, so the highlight moves to the first
                // widget instead.
                let highlighted = colorable.iter().find(|w| id_of(w) == hl);
                if was_showing && highlighted.is_some_and(|w| kind(w) == "separator") {
                    self.highlighted =
                        colorable.iter().find(|w| kind(w) != "separator").and_then(|w| id_of(w)).map(str::to_string);
                }
            }
        } else if shortcut == "f" || shortcut == "F" {
            self.editing_background = !self.editing_background;
        } else if shortcut == "b" || shortcut == "B" {
            if on_item && selected.is_some() {
                out.push(Action::SetLineWidgets(toggle_widget_bold(&widgets, selected_id)));
            }
        } else if shortcut == "d" || shortcut == "D" {
            if on_item && selected.is_some() {
                out.push(Action::SetLineWidgets(cycle_widget_dim(&widgets, selected_id)));
            }
        } else if shortcut == "r" || shortcut == "R" {
            if on_item && selected.is_some() {
                out.push(Action::SetLineWidgets(reset_widget_styling(&widgets, selected_id)));
            }
        } else if shortcut == "c" || shortcut == "C" {
            self.clear_confirm = Some(Confirm::default());
        } else if (k.left_arrow || k.right_arrow) && on_item && selected.is_some() {
            let colors: Vec<&str> = color_options(false).into_iter().map(|(_, v)| v).collect();
            let bg_colors: Vec<&str> = color_options(true).into_iter().map(|(_, v)| v).collect();
            out.push(Action::SetLineWidgets(cycle_widget_color(
                &widgets,
                selected_id,
                k.right_arrow,
                self.editing_background,
                &colors,
                &bg_colors,
            )));
        }
        out
    }

    fn view(&mut self, ctx: &Ctx) -> El {
        let widgets = ctx.line_widgets();
        let settings = ctx.typed;
        let line_suffix = format!(" - Line {}", ctx.selected_line + 1);
        let colorable = self.colorable(&widgets);
        if colorable.is_empty() {
            self.select = None;
            return col(vec![
                text(TextStyle::bold(), vec![s("Configure Colors"), s(line_suffix)]),
                boxed(
                    BoxStyle::default().margin_top(1.0),
                    vec![text(TextStyle::dim(), vec![s("No colorable widgets in the status line.")])],
                ),
                text(TextStyle::dim(), vec![s("Add a widget first to continue.")]),
                boxed(BoxStyle::default().margin_top(1.0), vec![plain("Press any key to go back...")]),
            ]);
        }

        let level = Level::from_color_level(settings.color_level);
        let color_opts = color_options(false);
        let bg_opts = color_options(true);
        let mut menu_items: Vec<(String, String)> = colorable
            .iter()
            .enumerate()
            .map(|(index, w)| {
                let label = format!("{}: {}", index + 1, item_label(w));
                let fg = jsobj::get_str(w, "color").map(str::to_string).unwrap_or_else(|| default_foreground_color(w));
                let bold = jsobj::get(w, "bold").is_some_and(crate::tui::truthy);
                let styled =
                    apply_colors(&label, Some(&fg), jsobj::get_str(w, "backgroundColor"), bold, level, dim_of(w));
                (styled, id_of(w).unwrap_or("").to_string())
            })
            .collect();
        menu_items.push(("← Back".into(), "back".into()));

        let hl = self.highlighted.as_deref();
        let selected =
            if hl.is_some_and(|h| h != "back") { colorable.iter().find(|w| id_of(w) == hl).copied() } else { None };
        let current_color = if self.editing_background {
            selected.and_then(|w| jsobj::get_str(w, "backgroundColor")).unwrap_or("").to_string()
        } else {
            match selected {
                Some(w) => {
                    jsobj::get_str(w, "color").map(str::to_string).unwrap_or_else(|| default_foreground_color(w))
                }
                None => "white".into(),
            }
        };
        let color_list = if self.editing_background { &bg_opts } else { &color_opts };
        let color_number = color_list.iter().position(|(_, v)| *v == current_color);

        let color_display = if current_color.is_empty() {
            chalk_gray(if self.editing_background { "(no background)" } else { "(default)" })
        } else {
            let display = if let Some(code) = current_color.strip_prefix("ansi256:") {
                format!("ANSI {code}")
            } else if let Some(hex) = current_color.strip_prefix("hex:") {
                format!("#{hex}")
            } else if let Some(body) = current_color.strip_prefix("gradient:").filter(|_| !self.editing_background) {
                let lower = body.to_lowercase();
                if gradient_preset_names().any(|n| n == lower) {
                    format!("Gradient: {lower}")
                } else {
                    format!("Gradient: {body}")
                }
            } else {
                let opts = if self.editing_background { &bg_opts } else { &color_opts };
                opts.iter().find(|(_, v)| *v == current_color).map(|(n, _)| n.clone()).unwrap_or(current_color.clone())
            };
            if self.editing_background {
                apply_colors(&format!(" {display} "), None, Some(&current_color), false, level, Dim::No)
            } else {
                apply_colors(&display, Some(&current_color), None, false, level, Dim::No)
            }
        };
        let style_indicators = [
            selected.is_some_and(|w| jsobj::get(w, "bold").is_some_and(crate::tui::truthy)).then_some("[BOLD]"),
            selected.is_some_and(|w| jsobj::get(w, "dim") == Some(&Value::Bool(true))).then_some("[DIM]"),
            selected.is_some_and(|w| jsobj::get_str(w, "dim") == Some("parens")).then_some("[DIM ()]"),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");

        if self.gradient_mode {
            self.select = None;
            return self.view_gradient(selected, level);
        }

        if let Some(confirm) = &mut self.clear_confirm {
            self.select = None;
            return col(vec![
                text(TextStyle::bold().with_color(Some("yellow")), vec![s("⚠ Confirm Clear All Colors")]),
                boxed(
                    BoxStyle::column().margin_top(1.0),
                    vec![
                        plain("This will reset all colors for all widgets to their defaults."),
                        text(TextStyle::color("red"), vec![s("This action cannot be undone!")]),
                    ],
                ),
                boxed(BoxStyle::default().margin_top(2.0), vec![plain("Continue?")]),
                boxed(BoxStyle::default().margin_top(1.0), vec![confirm.view(None, true, false)]),
            ]);
        }

        let fg_override = set(&settings.override_foreground_color);
        let bg_override = set(&settings.override_background_color) && !settings.powerline.enabled;
        let override_message = match (fg_override, bg_override) {
            (true, true) => Some("⚠ Global override for FG and BG active"),
            (true, false) => Some("⚠ Global override for FG active"),
            (false, true) => Some("⚠ Global override for BG active"),
            (false, false) => None,
        };

        let mut title = vec![s("Configure Colors"), s(line_suffix)];
        if self.editing_background {
            title.push(s(chalk_yellow(" [Background Mode]")));
        }
        let mut header = vec![text(TextStyle::bold(), title)];
        if let Some(m) = override_message {
            header.push(text(TextStyle::color("yellow").with_dim(true), vec![s(".  "), s(m)]));
        }
        let mut children = vec![row(header)];

        let fg_or_bg = if self.editing_background { "background" } else { "foreground" };
        let input_mode = self.hex_input_mode || self.ansi256_input_mode;
        if self.hex_input_mode {
            children.push(col(vec![
                plain("Enter 6-digit hex color code (without #):"),
                hex_field(&self.hex_input),
                plain(" "),
                text(TextStyle::dim(), vec![s("Press Enter when done, ESC to cancel")]),
            ]));
        } else if self.ansi256_input_mode {
            let pad = "_".repeat(3usize.saturating_sub(self.ansi256_input.len()));
            children.push(col(vec![
                plain("Enter ANSI 256 color code (0-255):"),
                text(TextStyle::default(), vec![s(self.ansi256_input.clone()), nested(TextStyle::dim(), vec![s(pad)])]),
                plain(" "),
                text(TextStyle::dim(), vec![s("Press Enter when done, ESC to cancel")]),
            ]));
        } else {
            let level_hint = match settings.color_level {
                3 => " (h)ex,",
                2 => " (a)nsi256,",
                _ => "",
            };
            let gradient_hint = if !self.editing_background && settings.color_level >= 2 { " (g)radient," } else { "" };
            children.push(text(
                TextStyle::dim(),
                vec![
                    s("↑↓ to select, ←→ to cycle"),
                    s(" "),
                    s(fg_or_bg),
                    s(", (f) to toggle bg/fg, (b)old, (d)im,"),
                    s(level_hint),
                    s(gradient_hint),
                    s(" "),
                    s("(r)eset, (c)lear all, ESC to go back"),
                ],
            ));
            if !settings.powerline.enabled && !set(&settings.default_separator) {
                children.push(text(
                    TextStyle::dim(),
                    vec![
                        s("(s)how separators:"),
                        s(if self.show_separators { chalk_green("ON") } else { chalk_gray("OFF") }),
                    ],
                ));
            }
            if selected.is_some() {
                let number = match color_number {
                    Some(i) => format!("{}/{}", i + 1, color_list.len()),
                    None => "custom".into(),
                };
                let mut parts = vec![
                    s("Current"),
                    s(" "),
                    s(fg_or_bg),
                    s(" "),
                    s("("),
                    s(number),
                    s("):"),
                    s(" "),
                    s(color_display),
                ];
                if !style_indicators.is_empty() {
                    parts.push(s(format!(" {style_indicators}")));
                }
                children.push(boxed(BoxStyle::default().margin_top(1.0), vec![text(TextStyle::default(), parts)]));
            } else {
                children.push(boxed(BoxStyle::default().margin_top(1.0), vec![plain(" ")]));
            }
        }

        let list = if input_mode {
            self.select = None;
            col(menu_items
                .iter()
                .map(|(label, value)| {
                    let sel = Some(value.as_str()) == hl;
                    text(
                        TextStyle::color(if sel { "cyan" } else { "white" }).with_bold(sel),
                        vec![s(if sel { "▶ " } else { "  " }), s(label.clone())],
                    )
                })
                .collect())
        } else {
            let values: Vec<String> = menu_items.iter().map(|(_, v)| v.clone()).collect();
            let key = (self.show_separators, self.highlighted.clone());
            match &mut self.select {
                Some(sel) if sel.key == key => {
                    // The effect on `items`: a different set of values starts over at the top.
                    if sel.values != values {
                        sel.index = 0;
                        sel.values = values;
                    }
                }
                _ => {
                    let index = values.iter().position(|v| Some(v.as_str()) == hl).unwrap_or(0);
                    self.select = Some(Select { key, values, index });
                }
            }
            let index = self.select.as_ref().map_or(0, |sel| sel.index);
            col(menu_items
                .iter()
                .enumerate()
                .map(|(i, (label, _))| {
                    let sel = i == index;
                    row(vec![plain(if sel { "▶" } else { "  " }), plain(format!(" {label}"))])
                })
                .collect())
        };
        children.push(boxed(BoxStyle::default().margin_top(1.0), vec![list]));
        children.push(boxed(
            BoxStyle::column().margin_top(1.0),
            vec![
                text(TextStyle::color("yellow"), vec![s("⚠ VSCode Users: ")]),
                text(TextStyle::dim(), vec![s(VSCODE_NOTE)]),
            ],
        ));
        col(children)
    }
}

const VSCODE_NOTE: &str = "If colors appear incorrect in the VSCode integrated terminal, the \"Terminal › Integrated: Minimum Contrast Ratio\" (`terminal.integrated.minimumContrastRatio`) setting is forcing a minimum contrast between foreground and background colors. You can adjust this setting to 1 to disable the contrast enforcement, or use a standalone terminal for accurate colors.";

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn arr(v: Value) -> Vec<Value> {
        v.as_array().cloned().unwrap()
    }

    fn out(v: &[Value]) -> String {
        jsobj::stringify(&Value::Array(v.to_vec()))
    }

    #[test]
    fn update_widget_by_id_only_updates_the_matching_widget() {
        let widgets = arr(json!([{"id": "1", "type": "tokens-input", "color": "blue"},
                                 {"id": "2", "type": "tokens-output", "color": "white"}]));
        let updated = update_widget_by_id(&widgets, "1", |w| jsobj::with(w, "color", json!("red")));
        assert_eq!(
            out(&updated),
            r#"[{"id":"1","type":"tokens-input","color":"red"},{"id":"2","type":"tokens-output","color":"white"}]"#
        );
    }

    #[test]
    fn toggle_bold_flips_the_selected_widget_only() {
        let widgets = arr(json!([{"id": "1", "type": "tokens-input", "bold": true},
                                 {"id": "2", "type": "tokens-output", "bold": false}]));
        let updated = toggle_widget_bold(&widgets, "1");
        assert_eq!(updated[0]["bold"], json!(false));
        assert_eq!(updated[1]["bold"], json!(false));
        assert_eq!(toggle_widget_bold(&arr(json!([{"id": "1"}])), "1")[0]["bold"], json!(true));
    }

    #[test]
    fn cycle_dim_goes_off_whole_parens_off() {
        let widgets = arr(json!([{"id": "1", "type": "tokens-input"}, {"id": "2", "type": "tokens-output"}]));
        let whole = cycle_widget_dim(&widgets, "1");
        let parens = cycle_widget_dim(&whole, "1");
        let off = cycle_widget_dim(&parens, "1");
        assert_eq!(whole[0]["dim"], json!(true));
        assert_eq!(parens[0]["dim"], json!("parens"));
        assert_eq!(jsobj::stringify(&off[0]), r#"{"id":"1","type":"tokens-input"}"#);
        assert!(whole[1].get("dim").is_none());
    }

    #[test]
    fn reset_styling_keeps_number_format() {
        let widgets = arr(json!([
            {"id": "1", "type": "tokens-input", "color": "red", "backgroundColor": "blue", "bold": true,
             "dim": "parens", "numberFormat": {"style": "compact"}},
            {"id": "2", "type": "tokens-output", "color": "white", "bold": true}
        ]));
        let updated = reset_widget_styling(&widgets, "1");
        assert_eq!(
            out(&updated),
            r#"[{"id":"1","type":"tokens-input","numberFormat":{"style":"compact"}},{"id":"2","type":"tokens-output","color":"white","bold":true}]"#
        );
    }

    #[test]
    fn clear_all_strips_styling_from_every_widget() {
        let widgets = arr(json!([
            {"id": "1", "type": "tokens-input", "color": "red", "backgroundColor": "blue", "bold": true, "dim": true},
            {"id": "2", "type": "tokens-output", "color": "white", "bold": true, "dim": "parens",
             "numberFormat": {"style": "whole"}}
        ]));
        assert_eq!(
            out(&clear_all_widget_styling(&widgets)),
            r#"[{"id":"1","type":"tokens-input"},{"id":"2","type":"tokens-output","numberFormat":{"style":"whole"}}]"#
        );
    }

    #[test]
    fn cycles_background_and_maps_empty_to_undefined_in_place() {
        let widgets = arr(json!([{"id": "1", "type": "tokens-input", "backgroundColor": "bgRed", "bold": true}]));
        let colors = ["blue", "red"];
        let bgs = ["bgRed", ""];
        let right = cycle_widget_color(&widgets, "1", true, true, &colors, &bgs);
        assert!(jsobj::get(&right[0], "backgroundColor").is_none());
        let left = cycle_widget_color(&right, "1", false, true, &colors, &bgs);
        assert_eq!(out(&left), r#"[{"id":"1","type":"tokens-input","backgroundColor":"bgRed","bold":true}]"#);
    }

    #[test]
    fn cycles_foreground_from_default_and_treats_dim_as_default() {
        let colors = ["blue", "red"];
        let bgs = ["bgRed", ""];
        let from_default =
            cycle_widget_color(&arr(json!([{"id": "1", "type": "tokens-input"}])), "1", true, false, &colors, &bgs);
        let from_dim = cycle_widget_color(
            &arr(json!([{"id": "1", "type": "tokens-input", "color": "dim"}])),
            "1",
            true,
            false,
            &colors,
            &bgs,
        );
        assert_eq!(from_default[0]["color"], json!("red"));
        assert_eq!(from_dim[0]["color"], json!("red"));
    }

    #[test]
    fn color_option_names_match_the_color_map() {
        let fg = color_options(false);
        assert_eq!(fg.len(), 17);
        assert_eq!(fg[0], ("Default".to_string(), ""));
        assert_eq!(fg[9], ("Bright Black".to_string(), "brightBlack"));
        let bg = color_options(true);
        assert_eq!(bg[1], ("Black".to_string(), "bgBlack"));
        assert_eq!(bg[16], ("Bright White".to_string(), "bgBrightWhite"));
    }
}
