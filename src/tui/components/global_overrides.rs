//! Upstream `GlobalOverridesMenu.tsx`.

use serde_json::{Map, Value};

use super::confirm::{Answer, Confirm};
use crate::colors::{apply_colors, chalk_wrap, Dim, Level};
use crate::jsobj;
use crate::tui::ink::keys::Input;
use crate::tui::ink::layout::*;
use crate::tui::screens::{Action, Ctx, Screen};
use crate::tui::text_cursor::should_insert_input;

/// `NUMBER_KINDS`.
const NUMBER_KINDS: [&str; 5] = ["token", "speed", "percent", "memory", "cost"];
/// `NUMBER_FORMAT_KIND_WIDTH`: the longest kind.
const NUMBER_FORMAT_KIND_WIDTH: usize = 7;
/// `DefaultPaddingSideSchema.options`.
const PADDING_SIDES: [&str; 3] = ["both", "left", "right"];

fn gradient_preset_names() -> impl Iterator<Item = &'static str> {
    crate::gradient::PRESETS.iter().map(|(name, _)| *name)
}

/// `getNextNumberStyle`: precise (unset) → compact → whole → precise.
fn next_number_style(current: Option<&str>) -> Option<&'static str> {
    match current.filter(|s| *s != "precise") {
        None => Some("compact"),
        Some("compact") => Some("whole"),
        _ => None,
    }
}

/// `cycleGlobalNumberStyle`.
fn cycle_global_number_style(settings: &Value, kind: &str) -> Value {
    let global = jsobj::get(settings, "numberFormat");
    let kind_format_src = global.and_then(|g| jsobj::get(g, kind));
    let current = kind_format_src.and_then(|f| jsobj::get_str(f, "style"));
    let next_style = next_number_style(current);

    let mut kind_format = kind_format_src.and_then(Value::as_object).cloned().unwrap_or_default();
    match next_style {
        None => {
            kind_format.shift_remove("style");
        }
        Some(style) => {
            kind_format.insert("style".into(), Value::String(style.into()));
        }
    }

    let mut next_global: Map<String, Value> = global.and_then(Value::as_object).cloned().unwrap_or_default();
    next_global.shift_remove(kind);
    if !kind_format.is_empty() {
        next_global.insert(kind.into(), Value::Object(kind_format));
    }

    jsobj::with(
        settings,
        "numberFormat",
        if next_global.is_empty() { jsobj::undefined() } else { Value::Object(next_global) },
    )
}

/// `getColorDisplayName`: the COLOR_MAP entry's display name, else the name itself.
fn color_display_name(name: &str) -> String {
    if !crate::colors::color_names().any(|n| n == name) {
        return name.to_string();
    }
    let base = if crate::colors::is_background_color_name(name) { &name[2..] } else { name };
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

/// `getChalkColor(name, 'ansi16', background)?.(text)`, with the app's chalk at its current level.
fn chalk_color(name: &str, background: bool, text: &str) -> Option<String> {
    let level = crate::colors::chalk_level();
    let (fg_open, bg_open) = (38, 48);
    let wrap = |code256: u32, bg: bool| {
        let close = if bg { "\x1b[49m" } else { "\x1b[39m" };
        let open = if level >= 2 {
            format!("\x1b[{};5;{code256}m", if bg { bg_open } else { fg_open })
        } else {
            format!("\x1b[{}m", crate::colors::ansi256_to_ansi(code256) + if bg { 10 } else { 0 })
        };
        chalk_wrap(&open, close, text)
    };
    if let Some(spec) = name.strip_prefix("ansi256:") {
        let code = crate::jsnum::parse_int(spec).filter(|c| (0.0..=255.0).contains(c))?;
        return Some(wrap(code as u32, background));
    }
    if let Some(hex) = name.strip_prefix("hex:") {
        if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or_default();
        let (r, g, b) = (byte(0), byte(2), byte(4));
        if level >= 3 {
            let close = if background { "\x1b[49m" } else { "\x1b[39m" };
            let open = format!("\x1b[{};2;{r};{g};{b}m", if background { bg_open } else { fg_open });
            return Some(chalk_wrap(&open, close, text));
        }
        return Some(wrap(crate::gradient::rgb_to_ansi256(crate::gradient::Rgb { r, g, b }), background));
    }
    if !crate::colors::color_names().any(|n| n == name) {
        return None;
    }
    // The entry's `ansi16` chalk: its own kind decides foreground or background.
    let bg = crate::colors::is_background_color_name(name);
    let open = crate::colors::color_ansi_code(Some(name), Level::Ansi16, bg);
    Some(chalk_wrap(&open, if bg { "\x1b[49m" } else { "\x1b[39m" }, text))
}

fn quoted_or(v: Option<&str>, empty: &str) -> String {
    match v.filter(|v| !v.is_empty()) {
        Some(v) => format!("\"{v}\""),
        None => empty.to_string(),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CustomStep {
    Start,
    End,
}

pub struct GlobalOverridesMenu {
    editing_padding: bool,
    editing_separator: bool,
    confirming_separator: bool,
    padding_input: String,
    separator_input: String,
    inherit_colors: bool,
    global_bold: bool,
    minimalist_mode: bool,
    number_format_mode: bool,
    number_format_kind_index: usize,
    gradient_mode: bool,
    gradient_index: usize,
    gradient_custom_step: Option<CustomStep>,
    gradient_start_hex: String,
    gradient_hex_input: String,
    confirm: Confirm,
}

fn pop_utf16(s: &str) -> String {
    let mut units: Vec<u16> = s.encode_utf16().collect();
    units.pop();
    String::from_utf16_lossy(&units)
}

fn lines(settings: &Value) -> Vec<Value> {
    jsobj::get(settings, "lines").and_then(Value::as_array).cloned().unwrap_or_default()
}

/// `settings.lines.map(line => line.filter(item => item.type !== 'separator'))`.
fn without_separators(settings: &Value) -> Value {
    Value::Array(
        lines(settings)
            .iter()
            .map(|line| {
                Value::Array(
                    line.as_array()
                        .map(Vec::as_slice)
                        .unwrap_or_default()
                        .iter()
                        .filter(|item| jsobj::get_str(item, "type") != Some("separator"))
                        .cloned()
                        .collect(),
                )
            })
            .collect(),
    )
}

impl GlobalOverridesMenu {
    pub fn new(ctx: &Ctx) -> Self {
        let t = ctx.typed;
        GlobalOverridesMenu {
            editing_padding: false,
            editing_separator: false,
            confirming_separator: false,
            padding_input: t.default_padding.clone().unwrap_or_default(),
            separator_input: t.default_separator.clone().unwrap_or_default(),
            inherit_colors: t.inherit_separator_colors,
            global_bold: t.global_bold,
            minimalist_mode: t.minimalist_mode,
            number_format_mode: false,
            number_format_kind_index: 0,
            gradient_mode: false,
            gradient_index: 0,
            gradient_custom_step: None,
            gradient_start_hex: String::new(),
            gradient_hex_input: String::new(),
            confirm: Confirm::default(),
        }
    }

    fn exit_gradient(&mut self) {
        self.gradient_mode = false;
        self.gradient_custom_step = None;
        self.gradient_start_hex.clear();
        self.gradient_hex_input.clear();
    }

    fn apply_gradient_value(&mut self, ctx: &Ctx, value: String) -> Vec<Action> {
        self.exit_gradient();
        vec![Action::SetSettings(jsobj::with(ctx.settings, "overrideForegroundColor", Value::String(value)))]
    }

    fn handle_gradient(&mut self, ev: &Input, ctx: &Ctx) -> Vec<Action> {
        let key = &ev.key;
        if let Some(step) = self.gradient_custom_step {
            if key.escape {
                self.gradient_custom_step = None;
                self.gradient_hex_input.clear();
            } else if key.return_ {
                if self.gradient_hex_input.len() == 6 {
                    if step == CustomStep::Start {
                        self.gradient_start_hex = std::mem::take(&mut self.gradient_hex_input);
                        self.gradient_custom_step = Some(CustomStep::End);
                    } else {
                        let value = format!("gradient:{}-{}", self.gradient_start_hex, self.gradient_hex_input);
                        return self.apply_gradient_value(ctx, value);
                    }
                }
            } else if key.backspace || key.delete {
                self.gradient_hex_input = pop_utf16(&self.gradient_hex_input);
            } else if should_insert_input(&ev.input, key) && self.gradient_hex_input.len() < 6 {
                let upper = ev.input.to_uppercase();
                if upper.len() == 1 && upper.bytes().all(|b| b.is_ascii_hexdigit()) {
                    self.gradient_hex_input.push_str(&upper);
                }
            }
            return Vec::new();
        }

        let presets = gradient_preset_names().count();
        let total = presets + 1;
        if key.escape {
            self.exit_gradient();
        } else if key.up_arrow {
            self.gradient_index = (self.gradient_index + total - 1) % total;
        } else if key.down_arrow {
            self.gradient_index = (self.gradient_index + 1) % total;
        } else if key.return_ {
            if let Some(name) = gradient_preset_names().nth(self.gradient_index) {
                return self.apply_gradient_value(ctx, format!("gradient:{name}"));
            }
            self.gradient_start_hex.clear();
            self.gradient_hex_input.clear();
            self.gradient_custom_step = Some(CustomStep::Start);
        }
        Vec::new()
    }

    fn handle_menu(&mut self, ev: &Input, ctx: &Ctx) -> Vec<Action> {
        let settings = ctx.settings;
        let shortcut = if ev.key.ctrl || ev.key.meta { "" } else { ev.input.as_str() };
        let powerline = ctx.typed.powerline.enabled;
        let set = |key: &str, value: Value| vec![Action::SetSettings(jsobj::with(settings, key, value))];
        let color_list = |bg: bool| -> Vec<&'static str> {
            std::iter::once("none")
                .chain(crate::colors::color_names().filter(|n| crate::colors::is_background_color_name(n) == bg))
                .collect()
        };
        let cycle_color = |key: &str, bg: bool| {
            let colors = color_list(bg);
            let current = jsobj::get_str(settings, key).unwrap_or("none");
            let next_index = colors.iter().position(|c| *c == current).map_or(0, |i| (i + 1) % colors.len());
            let next = colors[next_index];
            set(key, if next == "none" { jsobj::undefined() } else { Value::String(next.into()) })
        };
        if ev.key.escape {
            return vec![Action::Back];
        }
        match shortcut {
            "p" | "P" => self.editing_padding = true,
            "s" | "S" if !powerline => self.editing_separator = true,
            "i" | "I" if !powerline => {
                self.inherit_colors = !self.inherit_colors;
                return set("inheritSeparatorColors", Value::Bool(self.inherit_colors));
            }
            "b" | "B" if !powerline => return cycle_color("overrideBackgroundColor", true),
            "c" | "C" if !powerline => return set("overrideBackgroundColor", jsobj::undefined()),
            "o" | "O" => {
                self.global_bold = !self.global_bold;
                return set("globalBold", Value::Bool(self.global_bold));
            }
            "m" | "M" => {
                self.minimalist_mode = !self.minimalist_mode;
                return set("minimalistMode", Value::Bool(self.minimalist_mode));
            }
            "n" | "N" => {
                self.number_format_mode = true;
                self.number_format_kind_index = 0;
            }
            "f" | "F" => return cycle_color("overrideForegroundColor", false),
            "g" | "G" => {
                self.gradient_mode = true;
                self.gradient_index = 0;
                self.gradient_custom_step = None;
                self.gradient_start_hex.clear();
                self.gradient_hex_input.clear();
            }
            "x" | "X" => return set("overrideForegroundColor", jsobj::undefined()),
            "d" | "D" => {
                let current = jsobj::get_str(settings, "defaultPaddingSide");
                let index = PADDING_SIDES.iter().position(|s| Some(*s) == current);
                let next = PADDING_SIDES[index.map_or(0, |i| (i + 1) % PADDING_SIDES.len())];
                return set("defaultPaddingSide", Value::String(next.into()));
            }
            _ => {}
        }
        Vec::new()
    }

    fn number_format_view(&self, ctx: &Ctx) -> El {
        let global = jsobj::get(ctx.settings, "numberFormat");
        let rows: Vec<El> = NUMBER_KINDS
            .iter()
            .enumerate()
            .map(|(idx, kind)| {
                let style = global
                    .and_then(|g| jsobj::get(g, kind))
                    .and_then(|f| jsobj::get_str(f, "style"))
                    .unwrap_or("precise (default)");
                let sel = idx == self.number_format_kind_index;
                text(
                    TextStyle::default().with_color(sel.then_some("cyan")),
                    vec![s(format!("{}{kind:>NUMBER_FORMAT_KIND_WIDTH$}: {style}", if sel { "▶ " } else { "  " }))],
                )
            })
            .collect();
        col(vec![
            text(TextStyle::bold(), vec![s("Global Number Formatting")]),
            boxed(
                BoxStyle::default().margin_top(1.0),
                vec![text(
                    TextStyle::dim(),
                    vec![s("↑↓ to select a number type, ←→ to cycle its style, ESC to go back")],
                )],
            ),
            boxed(BoxStyle::column().margin_top(1.0), rows),
            boxed(
                BoxStyle::column().margin_top(1.0),
                vec![
                    text(
                        TextStyle::dim(),
                        vec![s("precise = keep trailing zeros (1.0M), compact = trim them (1M / 1.1M), whole = no \
                                decimals (1M).")],
                    ),
                    text(
                        TextStyle::dim(),
                        vec![s("A global style forces that type across every widget. Decimal places are set \
                                per-widget or in settings.json.")],
                    ),
                ],
            ),
        ])
    }

    fn gradient_view(&self, ctx: &Ctx) -> El {
        let level = Level::from_color_level(ctx.typed.color_level);
        if let Some(step) = self.gradient_custom_step {
            let mut rows = vec![plain(if step == CustomStep::Start {
                "Enter START hex color (without #):"
            } else {
                "Enter END hex color (without #):"
            })];
            if step == CustomStep::End {
                rows.push(text(TextStyle::dim(), vec![s(format!("Start: #{}", self.gradient_start_hex))]));
            }
            let missing = 6usize.saturating_sub(self.gradient_hex_input.len());
            rows.push(text(
                TextStyle::default(),
                vec![
                    s(format!("#{}", self.gradient_hex_input)),
                    nested(TextStyle::dim(), vec![s("_".repeat(missing))]),
                ],
            ));
            rows.push(plain(" "));
            rows.push(text(TextStyle::dim(), vec![s("Press Enter when done, ESC to go back")]));
            return col(vec![
                text(TextStyle::bold(), vec![s("Custom Gradient - Override FG Color")]),
                boxed(BoxStyle::column().margin_top(1.0), rows),
            ]);
        }
        let presets = gradient_preset_names().count();
        let mut rows: Vec<El> = gradient_preset_names()
            .enumerate()
            .map(|(idx, name)| {
                let sel = idx == self.gradient_index;
                plain(format!(
                    "{}{}",
                    if sel { "▶ " } else { "  " },
                    apply_colors(name, Some(&format!("gradient:{name}")), None, sel, level, Dim::No)
                ))
            })
            .collect();
        rows.push(plain(format!(
            "{}Custom (enter two hex stops)",
            if self.gradient_index == presets { "▶ " } else { "  " }
        )));
        col(vec![
            text(TextStyle::bold(), vec![s("Select Gradient - Override FG Color")]),
            boxed(
                BoxStyle::default().margin_top(1.0),
                vec![text(TextStyle::dim(), vec![s("↑↓ to select, Enter to apply, ESC to cancel")])],
            ),
            boxed(BoxStyle::column().margin_top(1.0), rows),
        ])
    }

    fn fg_override_display(ctx: &Ctx) -> El {
        let fg = jsobj::get_str(ctx.settings, "overrideForegroundColor").unwrap_or("none");
        if fg == "none" {
            return text(TextStyle::color("gray"), vec![s("(none)")]);
        }
        if let Some(body) = fg.strip_prefix("gradient:") {
            let lower = body.to_lowercase();
            let display = if gradient_preset_names().any(|n| n == lower) {
                format!("Gradient: {lower}")
            } else {
                format!("Gradient: {body}")
            };
            let level = Level::from_color_level(ctx.typed.color_level);
            return plain(apply_colors(&display, Some(fg), None, false, level, Dim::No));
        }
        let name = color_display_name(fg);
        plain(chalk_color(fg, false, &name).unwrap_or(name))
    }

    fn bg_override_display(ctx: &Ctx) -> El {
        let bg = jsobj::get_str(ctx.settings, "overrideBackgroundColor").unwrap_or("none");
        if bg == "none" {
            return text(TextStyle::color("gray"), vec![s("(none)")]);
        }
        let name = color_display_name(bg);
        plain(chalk_color(bg, true, &format!(" {name} ")).unwrap_or(name))
    }

    fn menu_view(&mut self, ctx: &Ctx) -> Vec<El> {
        let settings = ctx.settings;
        let powerline = ctx.typed.powerline.enabled;
        let dim = |t: &str| text(TextStyle::dim(), vec![s(t)]);
        let cyan = |t: String| text(TextStyle::color("cyan"), vec![s(t)]);
        let toggle = |on: bool| {
            text(
                TextStyle::color(if on { "green" } else { "red" }),
                vec![s(if on { "✓ Enabled" } else { "✗ Disabled" })],
            )
        };
        let disabled = || dim("[disabled - Powerline active]");

        if self.editing_padding {
            return vec![col(vec![
                row(vec![
                    plain("Enter default padding (applied per the Padding Side setting): "),
                    cyan(quoted_or(Some(&self.padding_input), "(empty)")),
                ]),
                dim("Press Enter to save, ESC to cancel"),
            ])];
        }
        if self.editing_separator {
            return vec![col(vec![
                row(vec![
                    plain("Enter default separator (placed between widgets): "),
                    cyan(quoted_or(Some(&self.separator_input), "(empty - no separator will be added)")),
                ]),
                dim("Press Enter to save, ESC to cancel"),
            ])];
        }
        if self.confirming_separator {
            return vec![col(vec![
                boxed(
                    BoxStyle::default().margin_bottom(1.0),
                    vec![text(
                        TextStyle::color("yellow"),
                        vec![s("⚠ Warning: Setting a default separator will remove all existing manual separators \
                                from your status lines.")],
                    )],
                ),
                row(vec![plain("New default separator: "), cyan(quoted_or(Some(&self.separator_input), "(empty)"))]),
                boxed(BoxStyle::default().margin_top(1.0), vec![plain("Do you want to continue? ")]),
                boxed(BoxStyle::default().margin_top(1.0), vec![self.confirm.view(None, true, false)]),
            ])];
        }

        let padding_side = match jsobj::get_str(settings, "defaultPaddingSide") {
            Some("left") => "Left only",
            Some("right") => "Right only",
            _ => "Both",
        };
        let mut out = vec![
            row(vec![plain("      Global Bold: "), toggle(self.global_bold), dim(" - Press (o) to toggle")]),
            row(vec![plain("  Minimalist Mode: "), toggle(self.minimalist_mode), dim(" - Press (m) to toggle")]),
            row(vec![
                plain("Number Formatting: "),
                cyan(
                    if jsobj::get(settings, "numberFormat").is_some_and(crate::tui::truthy) {
                        "customized"
                    } else {
                        "(defaults)"
                    }
                    .into(),
                ),
                dim(" - Press (n) to configure per-type"),
            ]),
            row(vec![
                plain("  Default Padding: "),
                cyan(quoted_or(jsobj::get_str(settings, "defaultPadding"), "(none)")),
                dim(" - Press (p) to edit"),
            ]),
            row(vec![plain("     Padding Side: "), cyan(padding_side.into()), dim(" - Press (d) to cycle")]),
            row(vec![
                plain("Override FG Color: "),
                Self::fg_override_display(ctx),
                dim(" - (f) cycle, (g) gradient, (x) clear"),
            ]),
            row(if powerline {
                vec![plain("Override BG Color: "), disabled()]
            } else {
                vec![plain("Override BG Color: "), Self::bg_override_display(ctx), dim(" - (b) cycle, (c) clear")]
            }),
            row(if powerline {
                vec![plain("   Inherit Colors: "), disabled()]
            } else {
                vec![plain("   Inherit Colors: "), toggle(self.inherit_colors), dim(" - Press (i) to toggle")]
            }),
            row(if powerline {
                vec![plain("Default Separator: "), disabled()]
            } else {
                vec![
                    plain("Default Separator: "),
                    cyan(quoted_or(jsobj::get_str(settings, "defaultSeparator"), "(none)")),
                    dim(" - Press (s) to edit"),
                ]
            }),
            boxed(BoxStyle::default().margin_top(2.0), vec![dim("Press ESC to go back")]),
        ];
        out.push(boxed(
            BoxStyle::column().margin_top(1.0),
            vec![
                dim("Note: These settings are applied during rendering and don't add widgets to your widget list."),
                dim("• Padding Side: Choose whether default padding applies to both sides, left only, or right only"),
                dim("• Inherit colors: Separators will use colors from the preceding widget"),
                dim("• Global Bold: Makes all text bold regardless of individual settings"),
                dim("• Minimalist Mode: Strips decorative prefixes and labels from widgets"),
                dim("• Override colors: All widgets will use these colors instead of their configured colors"),
            ],
        ));
        out
    }
}

impl Screen for GlobalOverridesMenu {
    fn handle(&mut self, ev: &Input, ctx: &Ctx) -> Vec<Action> {
        let settings = ctx.settings;
        let key = &ev.key;
        if self.confirming_separator {
            // The inline ConfirmDialog's handlers; the menu's own handler skips keys while it shows.
            return match self.confirm.handle(ev, false) {
                Answer::Yes => {
                    self.confirming_separator = false;
                    let next = jsobj::with(settings, "defaultSeparator", Value::String(self.separator_input.clone()));
                    vec![Action::SetSettings(jsobj::with(&next, "lines", without_separators(settings)))]
                }
                Answer::No => {
                    self.separator_input = ctx.typed.default_separator.clone().unwrap_or_default();
                    self.confirming_separator = false;
                    Vec::new()
                }
                Answer::None => Vec::new(),
            };
        }
        if self.editing_padding {
            if key.return_ {
                self.editing_padding = false;
                return vec![Action::SetSettings(jsobj::with(
                    settings,
                    "defaultPadding",
                    Value::String(self.padding_input.clone()),
                ))];
            } else if key.escape {
                self.padding_input = ctx.typed.default_padding.clone().unwrap_or_default();
                self.editing_padding = false;
            } else if key.backspace {
                self.padding_input = pop_utf16(&self.padding_input);
            } else if key.delete {
                // For simple text inputs without cursor, forward delete does nothing
            } else if should_insert_input(&ev.input, key) {
                self.padding_input.push_str(&ev.input);
            }
            return Vec::new();
        }
        if self.editing_separator {
            if key.return_ {
                let has_manual_separators = lines(settings).iter().any(|line| {
                    line.as_array()
                        .is_some_and(|items| items.iter().any(|i| jsobj::get_str(i, "type") == Some("separator")))
                });
                self.editing_separator = false;
                if !self.separator_input.is_empty() && has_manual_separators {
                    self.confirming_separator = true;
                    self.confirm = Confirm::default();
                    return Vec::new();
                }
                // Only remove manual separators if we're setting a non-empty default
                let next = if self.separator_input.is_empty() {
                    jsobj::with(settings, "defaultSeparator", jsobj::undefined())
                } else {
                    let next = jsobj::with(settings, "defaultSeparator", Value::String(self.separator_input.clone()));
                    jsobj::with(&next, "lines", without_separators(settings))
                };
                return vec![Action::SetSettings(next)];
            } else if key.escape {
                self.separator_input = ctx.typed.default_separator.clone().unwrap_or_default();
                self.editing_separator = false;
            } else if key.backspace {
                self.separator_input = pop_utf16(&self.separator_input);
            } else if key.delete {
                // For simple text inputs without cursor, forward delete does nothing
            } else if should_insert_input(&ev.input, key) {
                self.separator_input.push_str(&ev.input);
            }
            return Vec::new();
        }
        if self.gradient_mode {
            return self.handle_gradient(ev, ctx);
        }
        if self.number_format_mode {
            let n = NUMBER_KINDS.len();
            if key.escape {
                self.number_format_mode = false;
            } else if key.up_arrow {
                self.number_format_kind_index = (self.number_format_kind_index + n - 1) % n;
            } else if key.down_arrow {
                self.number_format_kind_index = (self.number_format_kind_index + 1) % n;
            } else if key.left_arrow || key.right_arrow {
                if let Some(kind) = NUMBER_KINDS.get(self.number_format_kind_index) {
                    return vec![Action::SetSettings(cycle_global_number_style(settings, kind))];
                }
            }
            return Vec::new();
        }
        self.handle_menu(ev, ctx)
    }

    fn view(&mut self, ctx: &Ctx) -> El {
        if self.number_format_mode {
            return self.number_format_view(ctx);
        }
        if self.gradient_mode {
            return self.gradient_view(ctx);
        }
        let mut children = vec![
            text(TextStyle::bold(), vec![s("Global Overrides")]),
            text(TextStyle::dim(), vec![s("Configure automatic padding and separators between widgets")]),
        ];
        if ctx.typed.powerline.enabled {
            children.push(boxed(
                BoxStyle::default().margin_top(1.0),
                vec![text(
                    TextStyle::color("yellow"),
                    vec![s("⚠ Some options are disabled while Powerline mode is active")],
                )],
            ));
        }
        children.push(boxed(BoxStyle::default().margin_top(1.0), Vec::new()));
        children.extend(self.menu_view(ctx));
        col(children)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn cycles_a_number_kind_through_its_styles_and_drops_empty_formats() {
        let settings = json!({"lines": [], "numberFormat": {"cost": {"decimals": 2}}, "globalBold": false});
        let compact = cycle_global_number_style(&settings, "token");
        assert_eq!(
            jsobj::stringify(&compact),
            r#"{"lines":[],"numberFormat":{"cost":{"decimals":2},"token":{"style":"compact"}},"globalBold":false}"#
        );
        let whole = cycle_global_number_style(&compact, "token");
        assert_eq!(jsobj::get(&whole, "numberFormat").unwrap()["token"]["style"], "whole");
        let back = cycle_global_number_style(&whole, "token");
        assert_eq!(
            jsobj::stringify(&back),
            r#"{"lines":[],"numberFormat":{"cost":{"decimals":2}},"globalBold":false}"#
        );
        let cleared = cycle_global_number_style(&cycle_global_number_style(&json!({"a": 1}), "cost"), "cost");
        let cleared = cycle_global_number_style(&cleared, "cost");
        assert_eq!(jsobj::stringify(&cleared), r#"{"a":1}"#);
    }

    #[test]
    fn names_colors_as_the_color_map_does() {
        assert_eq!(color_display_name("brightBlack"), "Bright Black");
        assert_eq!(color_display_name("bgBrightMagenta"), "Bright Magenta");
        assert_eq!(color_display_name("bgRed"), "Red");
        assert_eq!(color_display_name("hex:FF0000"), "hex:FF0000");
    }
}
