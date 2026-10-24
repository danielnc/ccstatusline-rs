//! Upstream `PowerlineSetup.tsx`, `PowerlineThemeSelector.tsx` and `PowerlineSeparatorEditor.tsx`, plus the font
//! check and install they show (`utils/powerline.ts`, in [`fonts`]).

use serde_json::Value;

use super::confirm::{Answer, Confirm};
use super::list::{Entry, Event, Item, List, Props};
use crate::jsobj;
use crate::settings::PowerlineConfig;
use crate::tui::ink::keys::Input;
use crate::tui::ink::layout::*;
use crate::tui::screens::{Action, Ctx, Screen};
use crate::tui::text_cursor::should_insert_input;

/// `getPlainInput`.
fn plain_input(ev: &Input) -> &str {
    if ev.key.ctrl || ev.key.meta {
        ""
    } else {
        ev.input.as_str()
    }
}

fn is(shortcut: &str, c: &str) -> bool {
    shortcut == c || shortcut == c.to_uppercase()
}

const SEPARATOR_PRESETS: [(&str, &str, &str); 4] = [
    ("\u{E0B0}", "Triangle Right", "E0B0"),
    ("\u{E0B2}", "Triangle Left", "E0B2"),
    ("\u{E0B4}", "Round Right", "E0B4"),
    ("\u{E0B6}", "Round Left", "E0B6"),
];
const START_CAP_PRESETS: [(&str, &str, &str); 4] = [
    ("\u{E0B2}", "Triangle", "E0B2"),
    ("\u{E0B6}", "Round", "E0B6"),
    ("\u{E0BA}", "Lower Triangle", "E0BA"),
    ("\u{E0BE}", "Diagonal", "E0BE"),
];
const END_CAP_PRESETS: [(&str, &str, &str); 4] = [
    ("\u{E0B0}", "Triangle", "E0B0"),
    ("\u{E0B4}", "Round", "E0B4"),
    ("\u{E0B8}", "Lower Triangle", "E0B8"),
    ("\u{E0BC}", "Diagonal", "E0BC"),
];

// ------------------------------------------------------------------------------------------- PowerlineSetup

const POWERLINE_MENU_LABEL_WIDTH: usize = 11;

/// `getDefaultPowerlineTheme()`.
const DEFAULT_POWERLINE_THEME: &str = "nord-aurora";

fn format_powerline_menu_label(label: &str) -> String {
    format!("{label:<POWERLINE_MENU_LABEL_WIDTH$}")
}

/// `getSeparatorDisplay`.
pub fn separator_display(pl: &PowerlineConfig) -> String {
    let seps = &pl.separators;
    if seps.len() > 1 {
        return "multiple".into();
    }
    let sep = seps.first().map(String::as_str).unwrap_or("\u{E0B0}");
    match SEPARATOR_PRESETS.iter().find(|p| p.0 == sep) {
        Some((ch, name, _)) => format!("{ch} - {name}"),
        None => format!("{sep} - Custom"),
    }
}

/// `getCapDisplay`.
pub fn cap_display(pl: &PowerlineConfig, start: bool) -> String {
    let caps = if start { &pl.start_caps } else { &pl.end_caps };
    if caps.is_empty() {
        return "none".into();
    }
    if caps.len() > 1 {
        return "multiple".into();
    }
    let cap = caps[0].as_str();
    if cap.is_empty() {
        return "none".into();
    }
    let presets = if start { &START_CAP_PRESETS } else { &END_CAP_PRESETS };
    match presets.iter().find(|p| p.0 == cap) {
        Some((ch, name, _)) => format!("{ch} - {name}"),
        None => format!("{cap} - Custom"),
    }
}

/// `getThemeDisplay`: `theme.charAt(0).toUpperCase() + theme.slice(1)`.
pub fn theme_display(pl: &PowerlineConfig) -> String {
    let theme = pl.theme.as_deref().unwrap_or("");
    if theme.is_empty() || theme == "custom" {
        return "Custom".into();
    }
    let mut chars = theme.chars();
    let first = chars.next().unwrap_or_default();
    // charAt(0) of an astral character is half a surrogate pair, which toUpperCase leaves alone.
    let head: String = if first.len_utf16() == 1 { first.to_uppercase().collect() } else { first.to_string() };
    format!("{head}{}", chars.as_str())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MenuValue {
    Separator,
    StartCap,
    EndCap,
    Themes,
}

fn menu_items(pl: &PowerlineConfig) -> Vec<Item<MenuValue>> {
    let disabled = !pl.enabled;
    let entry = |label: &str, sublabel: String, value: MenuValue, description: &str| {
        Item::Entry(
            Entry::new(format_powerline_menu_label(label), value)
                .sublabel(Some(format!("({sublabel})")))
                .disabled(disabled)
                .description(description),
        )
    };
    vec![
        entry(
            "Separator",
            separator_display(pl),
            MenuValue::Separator,
            "Choose the glyph used between powerline segments.",
        ),
        entry(
            "Start Cap",
            cap_display(pl, true),
            MenuValue::StartCap,
            "Configure the cap glyph that appears at the start of each powerline line.",
        ),
        entry(
            "End Cap",
            cap_display(pl, false),
            MenuValue::EndCap,
            "Configure the cap glyph that appears at the end of each powerline line.",
        ),
        entry(
            "Themes",
            theme_display(pl),
            MenuValue::Themes,
            "Preview built-in powerline themes or copy a theme into custom widget colors.",
        ),
    ]
}

fn menu_props(items: &[Item<MenuValue>], initial: usize) -> Props<'_, MenuValue> {
    let mut p = Props::new(items);
    p.margin_top = 1.0;
    p.initial_selection = initial;
    p.show_back_button = true;
    p
}

/// `buildEnabledPowerlineSettings`.
fn build_enabled_powerline_settings(settings: &Value, remove_manual_separators: bool) -> Value {
    let pl = jsobj::get(settings, "powerline").cloned().unwrap_or(Value::Object(Default::default()));
    let theme = jsobj::get_str(&pl, "theme").unwrap_or("");
    let theme =
        if theme.is_empty() || theme == "custom" { DEFAULT_POWERLINE_THEME.to_string() } else { theme.to_string() };
    let mut next_pl = jsobj::with(&pl, "enabled", Value::Bool(true));
    next_pl = jsobj::with(&next_pl, "theme", Value::String(theme));
    // Separators are initialized by schema defaults, preserve existing values.
    for key in ["separators", "separatorInvertBackground"] {
        next_pl = jsobj::with(&next_pl, key, jsobj::get(&pl, key).cloned().unwrap_or_else(jsobj::undefined));
    }
    let lines = jsobj::get(settings, "lines").cloned().unwrap_or(Value::Array(Vec::new()));
    let lines = if remove_manual_separators {
        Value::Array(
            lines
                .as_array()
                .map(|ls| {
                    ls.iter()
                        .map(|line| {
                            Value::Array(
                                line.as_array()
                                    .map(|ws| ws.iter().filter(|w| !is_type(w, "separator")).cloned().collect())
                                    .unwrap_or_default(),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default(),
        )
    } else {
        lines
    };
    let mut next = jsobj::with(settings, "powerline", next_pl);
    next = jsobj::with(&next, "defaultPadding", Value::String(" ".into()));
    jsobj::with(&next, "lines", lines)
}

fn is_type(widget: &Value, t: &str) -> bool {
    jsobj::get_str(widget, "type") == Some(t)
}

/// `{...settings, powerline: {...powerlineConfig, [key]: value}}`.
fn with_powerline(settings: &Value, key: &str, value: Value) -> Value {
    let pl = jsobj::get(settings, "powerline").cloned().unwrap_or(Value::Object(Default::default()));
    jsobj::with(settings, "powerline", jsobj::with(&pl, key, value))
}

enum Sub {
    Menu,
    Editor(SeparatorEditor),
    Themes(ThemeSelector),
}

pub struct PowerlineSetup {
    screen: Sub,
    selected_menu_item: usize,
    confirming_enable: bool,
    confirming_font_install: bool,
    list: List,
    confirm: Confirm,
}

impl PowerlineSetup {
    pub fn new(_ctx: &Ctx) -> Self {
        PowerlineSetup {
            screen: Sub::Menu,
            selected_menu_item: 0,
            confirming_enable: false,
            confirming_font_install: false,
            list: List::new(0),
            confirm: Confirm::default(),
        }
    }

    /// The list's reset effect, then its `onSelectionChange` effect (`setSelectedMenuItem`).
    fn sync_menu(&mut self, items: &[Item<MenuValue>]) {
        self.list.sync(&menu_props(items, self.selected_menu_item));
        let selectable = items.iter().filter(|i| matches!(i, Item::Entry(e) if !e.disabled)).count() + 1;
        if self.list.selected < selectable {
            self.selected_menu_item = self.list.selected;
            self.list.sync(&menu_props(items, self.selected_menu_item));
        }
    }

    fn back_to_menu(&mut self) {
        self.screen = Sub::Menu;
        self.list = List::new(self.selected_menu_item);
    }

    fn menu_shown(&self, ctx: &Ctx) -> bool {
        matches!(self.screen, Sub::Menu)
            && !self.confirming_font_install
            && !self.confirming_enable
            && !ctx.installing_fonts
            && ctx.font_install_message.is_none()
    }
}

fn has_manual_separator_items(ctx: &Ctx) -> bool {
    jsobj::get(ctx.settings, "lines").and_then(Value::as_array).is_some_and(|lines| {
        lines.iter().any(|line| line.as_array().is_some_and(|ws| ws.iter().any(|w| is_type(w, "separator"))))
    })
}

impl Screen for PowerlineSetup {
    fn handle(&mut self, ev: &Input, ctx: &Ctx) -> Vec<Action> {
        let pl = &ctx.typed.powerline;
        let mut actions = Vec::new();
        // Children's handlers first, each with the state of the last render.
        let menu_shown = self.menu_shown(ctx);
        let confirming_font_install = self.confirming_font_install;
        let confirming_enable = self.confirming_enable;
        let screen_was_menu = matches!(self.screen, Sub::Menu);
        let mut back = false;
        match &mut self.screen {
            Sub::Editor(editor) => {
                let (a, b) = editor.handle(ev, ctx);
                actions.extend(a);
                back = b;
            }
            Sub::Themes(selector) => {
                let (a, b) = selector.handle(ev, ctx);
                actions.extend(a);
                back = b;
            }
            Sub::Menu => {}
        }
        if back {
            self.back_to_menu();
        }
        if screen_was_menu && (confirming_font_install || confirming_enable) && !ctx.installing_fonts {
            if ctx.font_install_message.is_none() {
                match self.confirm.handle(ev, false) {
                    Answer::Yes => {
                        if confirming_font_install {
                            self.confirming_font_install = false;
                            actions.push(Action::InstallFonts);
                        } else {
                            actions.push(Action::SetSettings(build_enabled_powerline_settings(ctx.settings, true)));
                            self.confirming_enable = false;
                        }
                    }
                    Answer::No => {
                        self.confirming_font_install = false;
                        self.confirming_enable = false;
                    }
                    Answer::None => {}
                }
            }
        } else if menu_shown {
            let items = menu_items(pl);
            self.sync_menu(&items);
            match self.list.handle(&menu_props(&items, self.selected_menu_item), ev) {
                Event::Selected(value, _) => {
                    self.screen = match value {
                        MenuValue::Separator => Sub::Editor(SeparatorEditor::new(Mode::Separator)),
                        MenuValue::StartCap => Sub::Editor(SeparatorEditor::new(Mode::StartCap)),
                        MenuValue::EndCap => Sub::Editor(SeparatorEditor::new(Mode::EndCap)),
                        MenuValue::Themes => Sub::Themes(ThemeSelector::new(ctx)),
                    };
                }
                Event::Back => actions.push(Action::Back),
                Event::None => self.sync_menu(&items),
            }
        }

        // PowerlineSetup's own handler.
        let shortcut = plain_input(ev);
        if ctx.font_install_message.is_some() || ctx.installing_fonts {
            if ctx.font_install_message.is_some() && !ev.key.escape {
                actions.push(Action::ClearFontMessage);
            }
            return actions;
        }
        if confirming_font_install || confirming_enable {
            return actions;
        }
        if screen_was_menu {
            if ev.key.escape {
                actions.push(Action::Back);
            } else if is(shortcut, "t") {
                if !pl.enabled {
                    if has_manual_separator_items(ctx) {
                        self.confirming_enable = true;
                        self.confirm = Confirm::default();
                    } else {
                        actions.push(Action::SetSettings(build_enabled_powerline_settings(ctx.settings, false)));
                    }
                } else {
                    actions.push(Action::SetSettings(with_powerline(ctx.settings, "enabled", Value::Bool(false))));
                }
            } else if is(shortcut, "i") {
                self.confirming_font_install = true;
                self.confirm = Confirm::default();
            } else if is(shortcut, "a") && pl.enabled {
                actions.push(Action::SetSettings(with_powerline(
                    ctx.settings,
                    "autoAlign",
                    Value::Bool(!pl.auto_align),
                )));
            } else if is(shortcut, "c") && pl.enabled {
                actions.push(Action::SetSettings(with_powerline(
                    ctx.settings,
                    "continueThemeAcrossLines",
                    Value::Bool(!pl.continue_theme_across_lines),
                )));
            }
        }
        actions
    }

    fn view(&mut self, ctx: &Ctx) -> El {
        match &mut self.screen {
            Sub::Editor(editor) => return editor.view(ctx),
            Sub::Themes(selector) => return selector.view(ctx),
            Sub::Menu => {}
        }
        let pl = &ctx.typed.powerline;
        let installing = ctx.installing_fonts;
        let message = ctx.font_install_message;
        let mut children = Vec::new();
        if !self.confirming_font_install && !installing && message.is_none() {
            let has_global_fg_override =
                ctx.typed.override_foreground_color.as_deref().is_some_and(|c| !c.is_empty() && c != "none");
            let mut header = vec![text(TextStyle::bold(), vec![s("Powerline Setup")])];
            if has_global_fg_override {
                header.push(text(
                    TextStyle::color("yellow").with_dim(true),
                    vec![s(".  "), s("⚠ Global override for FG active")],
                ));
            }
            children.push(row(header));
        }

        if self.confirming_font_install {
            children.push(col(vec![
                boxed(
                    BoxStyle::default().margin_bottom(1.0),
                    vec![text(TextStyle::color("cyan").with_bold(true), vec![s("Font Installation")])],
                ),
                boxed(BoxStyle::column().margin_bottom(1.0), font_install_steps()),
                boxed(
                    BoxStyle::default().margin_bottom(1.0),
                    vec![
                        text(TextStyle::color("yellow").with_bold(true), vec![s("Requirements: ")]),
                        text(TextStyle::dim(), vec![s("Git installed, Internet connection, Write permissions")]),
                    ],
                ),
                boxed(
                    BoxStyle::column().margin_bottom(1.0),
                    vec![
                        text(TextStyle::color("green").with_bold(true), vec![s("After install:")]),
                        text(TextStyle::dim(), vec![s("• Restart terminal")]),
                        text(TextStyle::dim(), vec![s("• Select a Powerline font")]),
                        text(TextStyle::dim(), vec![s("  (e.g. \"Meslo LG S for Powerline\")")]),
                    ],
                ),
                boxed(BoxStyle::default().margin_top(1.0), vec![plain("Proceed? ")]),
                boxed(BoxStyle::default().margin_top(1.0), vec![self.confirm.view(None, true, false)]),
            ]));
        } else if self.confirming_enable {
            let has = has_manual_separator_items(ctx);
            let mut inner = Vec::new();
            if has {
                inner.push(row(vec![text(
                    TextStyle::color("yellow"),
                    vec![s(
                        "⚠ Warning: Enabling Powerline mode will remove all existing manual separators from your status lines.",
                    )],
                )]));
                inner.push(boxed(
                    BoxStyle::default().margin_bottom(1.0),
                    vec![text(
                        TextStyle::dim(),
                        vec![s(
                            "Powerline mode uses its own separator system and is incompatible with manual separators.",
                        )],
                    )],
                ));
            }
            inner.push(boxed(
                BoxStyle::default().margin_top(if has { 1.0 } else { 0.0 }),
                vec![plain("Do you want to continue? ")],
            ));
            inner.push(boxed(BoxStyle::default().margin_top(1.0), vec![self.confirm.view(None, true, false)]));
            children.push(boxed(BoxStyle::column().margin_top(1.0), inner));
        } else if installing {
            children.push(row(vec![text(
                TextStyle::color("yellow"),
                vec![s("Installing Powerline fonts... This may take a moment.")],
            )]));
        } else if let Some(message) = message {
            children.push(col(vec![
                text(TextStyle::color(if message.contains("success") { "green" } else { "red" }), vec![s(message)]),
                boxed(
                    BoxStyle::default().margin_top(1.0),
                    vec![text(TextStyle::dim(), vec![s("Press any key to continue...")])],
                ),
            ]));
        } else {
            let status = if ctx.powerline_fonts_installed {
                vec![
                    nested(TextStyle::color("green"), vec![s("✓ Installed")]),
                    nested(TextStyle::dim(), vec![s(" - Ensure fonts are active in your terminal")]),
                ]
            } else {
                vec![
                    nested(TextStyle::color("yellow"), vec![s("✗ Not Installed")]),
                    nested(TextStyle::dim(), vec![s(" - Press (i) to install Powerline fonts")]),
                ]
            };
            let mut font_line = vec![s("    Font Status: ")];
            font_line.extend(status);
            children.push(col(vec![text(TextStyle::default(), font_line)]));
            children.push(toggle_row(" Powerline Mode: ", pl.enabled, " - Press (t) to toggle"));
            if pl.enabled {
                children.push(toggle_row("  Align Widgets: ", pl.auto_align, " - Press (a) to toggle"));
                children.push(toggle_row(
                    " Continue Theme: ",
                    pl.continue_theme_across_lines,
                    " - Press (c) to toggle",
                ));
                children.push(boxed(
                    BoxStyle::column().margin_top(1.0),
                    vec![
                        text(TextStyle::dim(), vec![s("Powerline mode uses its own separator system")]),
                        text(
                            TextStyle::dim(),
                            vec![s("Continue Theme keeps the Powerline color sequence running across lines")],
                        ),
                    ],
                ));
            } else {
                children.push(boxed(
                    BoxStyle::default().margin_top(1.0),
                    vec![text(
                        TextStyle::dim(),
                        vec![s("Enable Powerline mode to configure separators, caps, and themes.")],
                    )],
                ));
            }
            let items = menu_items(pl);
            self.sync_menu(&items);
            children.push(self.list.view(&menu_props(&items, self.selected_menu_item)));
        }
        col(children)
    }
}

fn toggle_row(label: &str, on: bool, hint: &str) -> El {
    row(vec![
        plain(label),
        text(
            TextStyle::color(if on { "green" } else { "red" }),
            vec![s(if on { "✓ Enabled  " } else { "✗ Disabled " })],
        ),
        text(TextStyle::dim(), vec![s(hint)]),
    ])
}

/// The "What will happen" list, per `os.platform()`.
fn font_install_steps() -> Vec<El> {
    let dim = |t: &str| text(TextStyle::dim(), vec![s(t)]);
    let mut out = vec![
        text(TextStyle::bold(), vec![s("What will happen:")]),
        text(
            TextStyle::default(),
            vec![
                nested(TextStyle::dim(), vec![s("• Clone fonts from ")]),
                nested(TextStyle::color("blue"), vec![s("https://github.com/powerline/fonts")]),
            ],
        ),
    ];
    match fonts::platform() {
        "darwin" => {
            out.push(dim("• Run install.sh script which will:"));
            out.push(dim("  - Copy all .ttf/.otf files to ~/Library/Fonts"));
            out.push(dim("  - Register fonts with macOS"));
        }
        "linux" => {
            out.push(dim("• Run install.sh script which will:"));
            out.push(dim("  - Copy all .ttf/.otf files to ~/.local/share/fonts"));
            out.push(dim("  - Run fc-cache to update font cache"));
        }
        "win32" => {
            out.push(dim("• Copy Powerline .ttf/.otf files to:"));
            out.push(dim("  AppData\\Local\\Microsoft\\Windows\\Fonts"));
        }
        _ => {}
    }
    out.push(dim("• Clean up temporary files"));
    out
}

// ----------------------------------------------------------------------------------- PowerlineThemeSelector

/// `buildPowerlineThemeItems`.
fn theme_items(original: &str) -> Vec<Item<&'static str>> {
    crate::colors::POWERLINE_THEMES
        .iter()
        .map(|t| {
            Item::Entry(
                Entry::new(t.display, t.name)
                    .sublabel((t.name == original).then(|| "(original)".to_string()))
                    .description(t.description),
            )
        })
        .collect()
}

fn theme_names() -> Vec<&'static str> {
    crate::colors::POWERLINE_THEMES.iter().map(|t| t.name).collect()
}

/// `assignPowerlineThemeSlots(widgets, startSlot)`: each widget's theme color slot. A widget merged into the
/// previous one shares its slot; separators get none.
fn assign_powerline_theme_slots(widgets: &[Value], start_slot: usize) -> (Vec<Option<usize>>, usize) {
    let mut previous_visible: Option<&Value> = None;
    let mut next_slot = start_slot;
    let slots = widgets
        .iter()
        .map(|w| {
            if is_type(w, "separator") || is_type(w, "flex-separator") {
                previous_visible = None;
                return None;
            }
            if !previous_visible.and_then(|p| jsobj::get(p, "merge")).is_some_and(crate::tui::truthy) {
                next_slot += 1;
            }
            previous_visible = Some(w);
            Some(next_slot - 1)
        })
        .collect();
    (slots, next_slot)
}

/// `applyCustomPowerlineTheme`: the theme's colors copied into the widgets, and the theme set to custom.
pub fn apply_custom_powerline_theme(settings: &Value, color_level: u8, theme_name: &str) -> Option<Value> {
    let theme = crate::colors::powerline_theme(theme_name)?;
    if theme_name == "custom" {
        return None;
    }
    let level = match color_level {
        0 | 1 => 0,
        3 => 2,
        _ => 1,
    };
    let colors = &theme.levels.as_ref()?[level];
    let continue_theme = jsobj::get(settings, "powerline")
        .and_then(|p| jsobj::get(p, "continueThemeAcrossLines"))
        .is_some_and(crate::tui::truthy);
    let mut next_line_slot = 0;
    let lines: Vec<Value> = jsobj::get(settings, "lines")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|line| {
            let widgets = line.as_array().cloned().unwrap_or_default();
            let (slots, next) = assign_powerline_theme_slots(&widgets, if continue_theme { next_line_slot } else { 0 });
            next_line_slot = next;
            Value::Array(
                widgets
                    .iter()
                    .zip(slots)
                    .map(|(w, slot)| match slot {
                        None => w.clone(),
                        Some(slot) => {
                            let fg = colors.fg[slot % colors.fg.len()];
                            let bg = colors.bg[slot % colors.bg.len()];
                            let w = jsobj::with(w, "color", Value::String(fg.into()));
                            jsobj::with(&w, "backgroundColor", Value::String(bg.into()))
                        }
                    })
                    .collect(),
            )
        })
        .collect();
    let next = with_powerline(settings, "theme", Value::String("custom".into()));
    Some(jsobj::with(&next, "lines", Value::Array(lines)))
}

struct ThemeSelector {
    themes: Vec<&'static str>,
    selected_index: usize,
    show_customize_confirm: bool,
    original_theme: String,
    original_settings: Value,
    list: List,
    confirm: Confirm,
}

impl ThemeSelector {
    fn new(ctx: &Ctx) -> Self {
        let themes = theme_names();
        let current = ctx.typed.powerline.theme.clone().unwrap_or_else(|| "custom".into());
        let selected_index = themes.iter().position(|t| *t == current).unwrap_or(0);
        ThemeSelector {
            themes,
            selected_index,
            show_customize_confirm: false,
            original_theme: current,
            original_settings: ctx.settings.clone(),
            list: List::new(selected_index),
            confirm: Confirm::default(),
        }
    }

    fn selected_theme(&self) -> Option<&'static str> {
        self.themes.get(self.selected_index).copied()
    }

    /// Returns the actions and whether the selector called `onBack`.
    fn handle(&mut self, ev: &Input, ctx: &Ctx) -> (Vec<Action>, bool) {
        let mut actions = Vec::new();
        if self.show_customize_confirm {
            match self.confirm.handle(ev, false) {
                Answer::Yes => {
                    if let Some(name) = self.selected_theme() {
                        if let Some(updated) = apply_custom_powerline_theme(ctx.settings, ctx.typed.color_level, name) {
                            actions.push(Action::SetSettings(updated));
                        }
                    }
                    self.show_customize_confirm = false;
                    return (actions, true);
                }
                Answer::No => {
                    self.show_customize_confirm = false;
                    self.list = List::new(self.selected_index);
                }
                Answer::None => {}
            }
            return (actions, false);
        }
        let items = theme_items(&self.original_theme);
        let p = list_props(&items, self.selected_index);
        self.list.sync(&p);
        let mut back = false;
        match self.list.handle(&p, ev) {
            Event::Selected(..) => back = true,
            _ => {
                // `onSelectionChange` sets selectedIndex; its effect previews the theme.
                if self.list.selected != self.selected_index && self.list.selected < self.themes.len() {
                    self.selected_index = self.list.selected;
                    self.list.sync(&list_props(&items, self.selected_index));
                    if let Some(name) = self.selected_theme() {
                        actions.push(Action::SetSettings(with_powerline(
                            ctx.settings,
                            "theme",
                            Value::String(name.into()),
                        )));
                    }
                }
            }
        }
        let shortcut = plain_input(ev);
        if ev.key.escape {
            actions.push(Action::SetSettings(self.original_settings.clone()));
            back = true;
        } else if is(shortcut, "c") && self.selected_theme().is_some_and(|t| t != "custom") {
            self.show_customize_confirm = true;
            self.confirm = Confirm::default();
        }
        (actions, back)
    }

    fn view(&mut self, ctx: &Ctx) -> El {
        let selected = self.selected_theme();
        let customizable = selected.is_some_and(|t| t != "custom");
        if self.show_customize_confirm {
            return col(vec![
                text(TextStyle::color("yellow").with_bold(true), vec![s("⚠ Confirm Customization")]),
                boxed(
                    BoxStyle::column().margin_top(1.0),
                    vec![
                        plain("This will copy the current theme colors to your widgets"),
                        plain("and switch to Custom theme mode."),
                        text(TextStyle::color("red"), vec![s("This will overwrite any existing custom colors!")]),
                    ],
                ),
                boxed(BoxStyle::default().margin_top(2.0), vec![plain("Continue?")]),
                boxed(BoxStyle::default().margin_top(1.0), vec![self.confirm.view(None, true, false)]),
            ]);
        }
        let items = theme_items(&self.original_theme);
        let p = list_props(&items, self.selected_index);
        self.list.sync(&p);
        let mut children = vec![
            text(
                TextStyle::bold(),
                vec![
                    s("Powerline Theme Selection  |  "),
                    nested(TextStyle::dim(), vec![s(format!("Original: {}", self.original_theme))]),
                ],
            ),
            row(vec![text(
                TextStyle::dim(),
                vec![s(format!(
                    "↑↓ navigate, Enter apply{}, ESC cancel",
                    if customizable { ", (c)ustomize theme" } else { "" }
                ))],
            )]),
            self.list.view(&p),
        ];
        if customizable {
            children.push(boxed(
                BoxStyle::default().margin_top(1.0),
                vec![text(TextStyle::dim(), vec![s("Press (c) to customize this theme - copies colors to widgets")])],
            ));
        }
        if ctx.typed.color_level == 1 {
            children.push(boxed(
                BoxStyle::default().margin_top(1.0),
                vec![text(
                    TextStyle::color("yellow"),
                    vec![s(
                        "⚠ 16 color mode themes have a very limited palette, we recommend switching color level in Terminal Options",
                    )],
                )],
            ));
        }
        col(children)
    }
}

fn list_props<'a>(items: &'a [Item<&'static str>], initial: usize) -> Props<'a, &'static str> {
    let mut p = Props::new(items);
    p.margin_top = 1.0;
    p.initial_selection = initial;
    p
}

// --------------------------------------------------------------------------------- PowerlineSeparatorEditor

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Separator,
    StartCap,
    EndCap,
}

struct SeparatorEditor {
    mode: Mode,
    selected_index: usize,
    hex_input_mode: bool,
    /// UTF-16 code units, as the JS string holds them.
    hex_input: Vec<u16>,
    cursor_pos: usize,
}

impl SeparatorEditor {
    fn new(mode: Mode) -> Self {
        SeparatorEditor { mode, selected_index: 0, hex_input_mode: false, hex_input: Vec::new(), cursor_pos: 0 }
    }

    fn items<'a>(&self, pl: &'a PowerlineConfig) -> &'a [String] {
        match self.mode {
            Mode::Separator => &pl.separators,
            Mode::StartCap => &pl.start_caps,
            Mode::EndCap => &pl.end_caps,
        }
    }

    /// One entry per separator: the saved list can be shorter (its default is `[false]`).
    fn invert_bgs(&self, pl: &PowerlineConfig) -> Vec<bool> {
        if self.mode != Mode::Separator {
            return Vec::new();
        }
        (0..pl.separators.len()).map(|i| pl.separator_invert_background.get(i).copied().unwrap_or(false)).collect()
    }

    fn presets(&self) -> &'static [(&'static str, &'static str, &'static str); 4] {
        match self.mode {
            Mode::Separator => &SEPARATOR_PRESETS,
            Mode::StartCap => &START_CAP_PRESETS,
            Mode::EndCap => &END_CAP_PRESETS,
        }
    }

    fn display(&self, ch: &str, index: usize, invert_bgs: &[bool]) -> String {
        let invert = invert_bgs.get(index).copied().unwrap_or(false);
        if let Some((pch, name, _)) = self.presets().iter().find(|p| p.0 == ch) {
            let inversion = if self.mode == Mode::Separator && invert { " [Inverted]" } else { "" };
            return format!("{pch} - {name}{inversion}");
        }
        let code_point = ch.chars().next().map_or(0, |c| c as u32);
        format!("{ch} - Custom (U+{code_point:04X}){}", if invert { " [Inverted]" } else { "" })
    }

    /// `updateSeparators`.
    fn update(&self, ctx: &Ctx, new_seps: Vec<String>, new_invert_bgs: Option<Vec<bool>>) -> Action {
        let pl = jsobj::get(ctx.settings, "powerline").cloned().unwrap_or(Value::Object(Default::default()));
        let strings = |v: &[String]| Value::Array(v.iter().cloned().map(Value::String).collect());
        let next = match self.mode {
            Mode::Separator => {
                let invert_bgs = self.invert_bgs(&ctx.typed.powerline);
                let inv = new_invert_bgs.unwrap_or_else(|| {
                    (0..new_seps.len()).map(|i| invert_bgs.get(i).copied().unwrap_or(false)).collect()
                });
                let next = jsobj::with(&pl, "separators", strings(&new_seps));
                jsobj::with(
                    &next,
                    "separatorInvertBackground",
                    Value::Array(inv.into_iter().map(Value::Bool).collect()),
                )
            }
            Mode::StartCap => jsobj::with(&pl, "startCaps", strings(&new_seps)),
            Mode::EndCap => jsobj::with(&pl, "endCaps", strings(&new_seps)),
        };
        Action::SetSettings(jsobj::with(ctx.settings, "powerline", next))
    }

    fn reset_hex(&mut self) {
        self.hex_input_mode = false;
        self.hex_input.clear();
        self.cursor_pos = 0;
    }

    /// Returns the actions and whether the editor called `onBack`.
    fn handle(&mut self, ev: &Input, ctx: &Ctx) -> (Vec<Action>, bool) {
        let pl = &ctx.typed.powerline;
        let separators = self.items(pl).to_vec();
        let invert_bgs = self.invert_bgs(pl);
        let presets = self.presets();
        let shortcut = plain_input(ev);
        let k = &ev.key;
        let sel = self.selected_index;
        let is_sep = self.mode == Mode::Separator;
        let mut actions = Vec::new();
        if self.hex_input_mode {
            if k.escape {
                self.reset_hex();
            } else if k.return_ {
                if (4..=6).contains(&self.hex_input.len()) {
                    let code_point = parse_int_hex(&String::from_utf16_lossy(&self.hex_input));
                    // A lone surrogate (D800-DFFF) is a JS string but no Rust char; such input stays pending.
                    let ch =
                        code_point.filter(|&c| c >= 0.0 && c <= 0x10FFFF as f64).and_then(|c| char::from_u32(c as u32));
                    if let Some(ch) = ch {
                        let mut new_seps = separators.clone();
                        if separators.is_empty() {
                            new_seps.push(ch.to_string());
                        } else {
                            set_at(&mut new_seps, sel, ch.to_string(), String::new());
                        }
                        actions.push(self.update(ctx, new_seps, None));
                        self.reset_hex();
                    }
                }
            } else if k.backspace && self.cursor_pos > 0 {
                let at = self.cursor_pos.min(self.hex_input.len());
                if at > 0 {
                    self.hex_input.remove(at - 1);
                }
                self.cursor_pos -= 1;
            } else if should_insert_input(&ev.input, k)
                && ev.input.chars().any(|c| c.is_ascii_hexdigit())
                && self.hex_input.len() < 6
            {
                let at = self.cursor_pos.min(self.hex_input.len());
                let upper: Vec<u16> = ev.input.to_uppercase().encode_utf16().collect();
                self.hex_input.splice(at..at, upper);
                self.cursor_pos += 1;
            }
            return (actions, false);
        }

        let mut back = false;
        if k.escape {
            back = true;
        } else if k.up_arrow && !separators.is_empty() {
            self.selected_index = if sel == 0 { separators.len() - 1 } else { sel - 1 };
        } else if k.down_arrow && !separators.is_empty() {
            self.selected_index = if sel + 1 > separators.len() - 1 { 0 } else { sel + 1 };
        } else if (k.left_arrow || k.right_arrow) && !separators.is_empty() {
            // Cycle through preset separators
            let current = separators.get(sel).map(String::as_str).unwrap_or("\u{E0B0}");
            let new_index = match presets.iter().position(|p| p.0 == current) {
                Some(i) if k.right_arrow => (i + 1) % presets.len(),
                Some(i) => {
                    if i == 0 {
                        presets.len() - 1
                    } else {
                        i - 1
                    }
                }
                None if k.right_arrow => 0,
                None => presets.len() - 1,
            };
            let new_char = presets[new_index].0.to_string();
            let mut new_seps = separators.clone();
            set_at(&mut new_seps, sel, new_char.clone(), String::new());
            let mut new_inv = invert_bgs.clone();
            if is_sep {
                // Auto-set inversion for left-facing separators
                set_at(&mut new_inv, sel, new_char == "\u{E0B2}" || new_char == "\u{E0B6}", false);
            }
            actions.push(self.update(ctx, new_seps, is_sep.then_some(new_inv)));
        } else if is(shortcut, "a") || is(shortcut, "i") {
            let after = is(shortcut, "a");
            let mut new_seps = separators.clone();
            let mut new_inv = invert_bgs.clone();
            let default_char = presets[0].0;
            let is_left_facing = default_char == "\u{E0B2}" || default_char == "\u{E0B6}";
            if separators.is_empty() {
                new_seps.push(default_char.to_string());
                if is_sep {
                    new_inv.push(is_left_facing);
                }
                actions.push(self.update(ctx, new_seps, Some(new_inv)));
                self.selected_index = 0;
            } else {
                let at = if after { sel + 1 } else { sel };
                splice_at(&mut new_seps, at, default_char.to_string(), String::new());
                if is_sep {
                    splice_at(&mut new_inv, at, is_left_facing, false);
                }
                actions.push(self.update(ctx, new_seps, Some(new_inv)));
                if after {
                    self.selected_index = sel + 1;
                }
            }
        } else if is(shortcut, "d") && (!is_sep || separators.len() > 1) {
            // Delete current (min 1 for separator, no min for caps)
            let new_seps: Vec<String> =
                separators.iter().enumerate().filter(|(i, _)| *i != sel).map(|(_, s)| s.clone()).collect();
            let new_inv: Vec<bool> =
                invert_bgs.iter().enumerate().filter(|(i, _)| *i != sel).map(|(_, b)| *b).collect();
            let len = new_seps.len();
            actions.push(self.update(ctx, new_seps, Some(new_inv)));
            self.selected_index = sel.min(len.saturating_sub(1));
        } else if is(shortcut, "c") {
            if is_sep {
                // Reset to default right-facing separator with no inversion
                actions.push(self.update(ctx, vec!["\u{E0B0}".into()], Some(vec![false])));
            } else {
                actions.push(self.update(ctx, Vec::new(), None));
            }
            self.selected_index = 0;
        } else if is(shortcut, "h") {
            self.hex_input_mode = true;
            self.hex_input.clear();
            self.cursor_pos = 0;
        } else if is(shortcut, "t") && is_sep {
            let mut new_inv = invert_bgs.clone();
            let cur = new_inv.get(sel).copied().unwrap_or(false);
            set_at(&mut new_inv, sel, !cur, false);
            actions.push(self.update(ctx, separators, Some(new_inv)));
        }
        (actions, back)
    }

    fn view(&mut self, ctx: &Ctx) -> El {
        let pl = &ctx.typed.powerline;
        let separators = self.items(pl);
        let invert_bgs = self.invert_bgs(pl);
        let title = match self.mode {
            Mode::Separator => "Powerline Separator Configuration",
            Mode::StartCap => "Powerline Start Cap Configuration",
            Mode::EndCap => "Powerline End Cap Configuration",
        };
        let is_sep = self.mode == Mode::Separator;
        let can_delete = !is_sep || separators.len() > 1;
        let mut children = vec![text(TextStyle::bold(), vec![s(title)])];
        if self.hex_input_mode {
            let at = self.cursor_pos.min(self.hex_input.len());
            let before = String::from_utf16_lossy(&self.hex_input[..at]);
            let cursor =
                self.hex_input.get(self.cursor_pos).map_or("_".to_string(), |u| String::from_utf16_lossy(&[*u]));
            let after = String::from_utf16_lossy(self.hex_input.get(self.cursor_pos + 1..).unwrap_or(&[]));
            let mut line = vec![
                s("U+"),
                s(before),
                nested(
                    TextStyle { color: Some("black".into()), background: Some("gray".into()), ..Default::default() },
                    vec![s(cursor)],
                ),
                s(after),
            ];
            if self.hex_input.len() < 6 && self.hex_input.len() == self.cursor_pos {
                line.push(nested(TextStyle::dim(), vec![s("_".repeat(6 - self.hex_input.len() - 1))]));
            }
            children.push(boxed(
                BoxStyle::column().margin_top(2.0),
                vec![
                    plain(format!(
                        "Enter hex code (4-6 digits) for {}{}:",
                        if is_sep { "separator" } else { "cap" },
                        if separators.is_empty() { String::new() } else { format!(" {}", self.selected_index + 1) }
                    )),
                    text(TextStyle::default(), line),
                    text(
                        TextStyle::dim(),
                        vec![s(
                            "Enter 4-6 hex digits (0-9, A-F) for a Unicode code point, then press Enter. ESC to cancel.",
                        )],
                    ),
                    text(TextStyle::dim(), vec![s("Examples: E0B0 (powerline), 1F984 (🦄), 2764 (❤)")]),
                ],
            ));
        } else {
            children.push(row(vec![text(
                TextStyle::dim(),
                vec![s(format!(
                    "↑↓ select, ← → cycle, (a)dd, (i)nsert{}, (c)lear, (h)ex{}, ESC back",
                    if can_delete { ", (d)elete" } else { "" },
                    if is_sep { ", (t)oggle invert" } else { "" }
                ))],
            )]));
            let rows: Vec<El> = if separators.is_empty() {
                vec![text(TextStyle::dim(), vec![s("(none configured - press 'a' to add)")])]
            } else {
                separators
                    .iter()
                    .enumerate()
                    .map(|(index, sep)| {
                        let selected = index == self.selected_index;
                        row(vec![text(
                            TextStyle::default().with_color(selected.then_some("green")),
                            vec![
                                s(if selected { "▶  " } else { "   " }),
                                s(format!("{}: {}", index + 1, self.display(sep, index, &invert_bgs))),
                            ],
                        )])
                    })
                    .collect()
            };
            children.push(boxed(BoxStyle::column().margin_top(2.0), rows));
        }
        col(children)
    }
}

/// `arr[i] = v` (a write past the end pads with holes, which JSON writes as null; `hole` stands in).
fn set_at<T: Clone>(v: &mut Vec<T>, i: usize, value: T, hole: T) {
    if i >= v.len() {
        v.resize(i + 1, hole);
    }
    v[i] = value;
}

/// `arr.splice(i, 0, v)`.
fn splice_at<T>(v: &mut Vec<T>, i: usize, value: T, _hole: T) {
    let i = i.min(v.len());
    v.insert(i, value);
}

/// `Number.parseInt(s, 16)`; None means NaN.
fn parse_int_hex(s: &str) -> Option<f64> {
    let t = crate::jsnum::js_trim(s);
    let (neg, rest) = match t.chars().next() {
        Some('-') => (true, &t[1..]),
        Some('+') => (false, &t[1..]),
        _ => (false, t),
    };
    let rest = rest.strip_prefix("0x").or_else(|| rest.strip_prefix("0X")).unwrap_or(rest);
    let digits: String = rest.chars().take_while(char::is_ascii_hexdigit).collect();
    if digits.is_empty() {
        return None;
    }
    let v = digits.chars().fold(0f64, |acc, c| acc * 16.0 + c.to_digit(16).unwrap_or(0) as f64);
    Some(if neg { -v } else { v })
}

// ------------------------------------------------------------------------------------------------ fonts

/// Upstream `utils/powerline.ts`: whether Powerline fonts look installed, and installing them.
pub mod fonts {
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicBool, Ordering};

    /// Set once fonts were installed this session (for `DEBUG_FONT_INSTALL`).
    static INSTALLED_THIS_SESSION: AtomicBool = AtomicBool::new(false);

    const FONTS_REPO: &str = "https://github.com/powerline/fonts.git";

    /// `os.platform()`.
    pub fn platform() -> &'static str {
        match std::env::consts::OS {
            "macos" => "darwin",
            "windows" => "win32",
            other => other,
        }
    }

    /// Debug mode: pretend fonts aren't installed (unless we installed them this session).
    fn debug_not_installed() -> bool {
        std::env::var("DEBUG_FONT_INSTALL").is_ok_and(|v| v == "1") && !INSTALLED_THIS_SESSION.load(Ordering::Relaxed)
    }

    fn home() -> PathBuf {
        crate::settings::home_dir()
    }

    /// The `powerlineFontPatterns` regexes (`/a.*b/i`) as the substrings they require, in order.
    const PATTERNS: &[&[&str]] = &[
        &["powerline"],
        &["nerd font"],
        &["for powerline"],
        &["meslo", "lg"],
        &["source", "code", "pro", "powerline"],
        &["dejavu", "powerline"],
        &["ubuntu", "mono", "powerline"],
        &["cascadia", "code", "pl"],
        &["fira", "code", "nerd"],
    ];

    /// `pattern.test(file)` for any pattern. `/i` without `u` folds only ASCII letters, and `.` stops at line
    /// terminators.
    pub fn is_powerline_font_name(name: &str) -> bool {
        let lower = name.to_ascii_lowercase();
        lower.split(['\n', '\r', '\u{2028}', '\u{2029}']).any(|line| {
            PATTERNS.iter().any(|parts| {
                let mut pos = 0;
                parts.iter().all(|p| match line[pos..].find(p) {
                    Some(i) => {
                        pos += i + p.len();
                        true
                    }
                    None => false,
                })
            })
        })
    }

    /// `checkPowerlineFonts`: a font file named like a Powerline font in the platform's font directories.
    pub fn check() -> bool {
        if debug_not_installed() {
            return false;
        }
        let home = home();
        let font_paths: Vec<PathBuf> = match platform() {
            "darwin" => {
                vec![home.join("Library").join("Fonts"), "/Library/Fonts".into(), "/System/Library/Fonts".into()]
            }
            "linux" => vec![
                home.join(".local").join("share").join("fonts"),
                home.join(".fonts"),
                "/usr/share/fonts".into(),
                "/usr/local/share/fonts".into(),
            ],
            "win32" => vec![
                home.join("AppData").join("Local").join("Microsoft").join("Windows").join("Fonts"),
                "C:\\Windows\\Fonts".into(),
            ],
            _ => vec![],
        };
        font_paths
            .iter()
            .filter_map(|p| std::fs::read_dir(p).ok())
            .any(|entries| entries.flatten().any(|e| is_powerline_font_name(&e.file_name().to_string_lossy())))
    }

    /// `checkPowerlineFontsAsync` (blocking; the App runs it off the event loop): the quick check, then
    /// fontconfig's list on Linux and macOS.
    pub fn check_async() -> bool {
        if debug_not_installed() {
            return false;
        }
        if check() {
            return true;
        }
        if matches!(platform(), "linux" | "darwin") {
            let out = Command::new("/bin/sh")
                .args(["-c", "fc-list 2>/dev/null | grep -i powerline"])
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output();
            if let Ok(o) = out {
                if o.status.success() && !crate::jsnum::js_trim(&String::from_utf8_lossy(&o.stdout)).is_empty() {
                    return true;
                }
            }
        }
        false
    }

    /// Node's `code: description` for an fs error.
    fn errno(e: &std::io::Error) -> (&'static str, String) {
        let code = match e.raw_os_error() {
            Some(libc::ENOENT) => "ENOENT",
            Some(libc::EACCES) => "EACCES",
            Some(libc::EPERM) => "EPERM",
            Some(libc::EEXIST) => "EEXIST",
            Some(libc::ENOTDIR) => "ENOTDIR",
            Some(libc::EISDIR) => "EISDIR",
            Some(libc::EROFS) => "EROFS",
            Some(libc::ENOSPC) => "ENOSPC",
            Some(libc::ENOEXEC) => "ENOEXEC",
            _ => "UNKNOWN",
        };
        let desc = match code {
            "ENOENT" => "no such file or directory".to_string(),
            "EACCES" => "permission denied".to_string(),
            "EPERM" => "operation not permitted".to_string(),
            "EEXIST" => "file already exists".to_string(),
            "ENOTDIR" => "not a directory".to_string(),
            "EISDIR" => "illegal operation on a directory".to_string(),
            "EROFS" => "read-only file system".to_string(),
            "ENOSPC" => "no space left on device".to_string(),
            "ENOEXEC" => "exec format error".to_string(),
            _ => e.to_string(),
        };
        (code, desc)
    }

    fn fs_error(e: &std::io::Error, syscall: &str, path: &Path) -> String {
        let (code, desc) = errno(e);
        format!("{code}: {desc}, {syscall} '{}'", path.display())
    }

    /// `execFileSync(file, args, {stdio: 'pipe'})`, its error as Node words it.
    fn exec_file(file: &str, args: &[&str], cwd: Option<&Path>) -> Result<(), String> {
        let mut cmd = Command::new(file);
        cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        match cmd.output() {
            Err(e) => Err(format!("spawnSync {file} {}", errno(&e).0)),
            Ok(o) if o.status.success() => Ok(()),
            Ok(o) => {
                let mut msg = format!(
                    "Command failed: {}",
                    std::iter::once(file).chain(args.iter().copied()).collect::<Vec<_>>().join(" ")
                );
                if !o.stderr.is_empty() {
                    msg.push('\n');
                    msg.push_str(&String::from_utf8_lossy(&o.stderr));
                }
                Err(msg)
            }
        }
    }

    /// `os.tmpdir()` on POSIX.
    fn tmpdir() -> String {
        let dir = ["TMPDIR", "TMP", "TEMP"]
            .iter()
            .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
            .unwrap_or_else(|| "/tmp".into());
        if dir.len() > 1 && dir.ends_with('/') {
            dir[..dir.len() - 1].to_string()
        } else {
            dir
        }
    }

    /// `fs.mkdtempSync(prefix)`: a new directory only this account can use.
    fn mkdtemp(prefix: &str) -> Result<PathBuf, String> {
        let template = format!("{prefix}XXXXXX");
        let mut buf = std::ffi::CString::new(template.clone()).map_err(|e| e.to_string())?.into_bytes_with_nul();
        // SAFETY: buf is a NUL-terminated template mkdtemp rewrites in place.
        let p = unsafe { libc::mkdtemp(buf.as_mut_ptr().cast()) };
        if p.is_null() {
            return Err(fs_error(&std::io::Error::last_os_error(), "mkdtemp", Path::new(&template)));
        }
        buf.pop();
        Ok(PathBuf::from(String::from_utf8_lossy(&buf).into_owned()))
    }

    /// `installPowerlineFonts`: clone the Powerline fonts repository and run its installer.
    pub fn install() -> (bool, String) {
        let home = home();
        let platform = platform();
        let font_dir = match platform {
            "darwin" => home.join("Library").join("Fonts"),
            "linux" => home.join(".local").join("share").join("fonts"),
            "win32" => home.join("AppData").join("Local").join("Microsoft").join("Windows").join("Fonts"),
            _ => return (false, "Unsupported platform for font installation".into()),
        };
        match install_into(platform, &font_dir) {
            Ok(r) => r,
            Err(error_message) => {
                if error_message.contains("git") {
                    (false, "Git is required to install Powerline fonts. Please install Git and try again.".into())
                } else {
                    (
                        false,
                        format!(
                            "Failed to install Powerline fonts: {error_message}. You can manually install fonts from: https://github.com/powerline/fonts"
                        ),
                    )
                }
            }
        }
    }

    fn install_into(platform: &str, font_dir: &Path) -> Result<(bool, String), String> {
        if !font_dir.exists() {
            std::fs::create_dir_all(font_dir).map_err(|e| fs_error(&e, "mkdir", font_dir))?;
        }
        let temp_dir = mkdtemp(&format!("{}/ccstatusline-powerline-fonts-", tmpdir()))?;
        let result = install_from(platform, font_dir, &temp_dir);
        if temp_dir.exists() {
            let _ = std::fs::remove_dir_all(&temp_dir);
        }
        result
    }

    fn install_from(platform: &str, font_dir: &Path, temp_dir: &Path) -> Result<(bool, String), String> {
        let temp = temp_dir.to_string_lossy();
        exec_file("git", &["clone", "--depth=1", FONTS_REPO, &temp], None)?;
        if platform == "darwin" || platform == "linux" {
            let install_script = temp_dir.join("install.sh");
            if !install_script.exists() {
                return Err("Install script not found in Powerline fonts repository".into());
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&install_script, std::fs::Permissions::from_mode(0o755))
                    .map_err(|e| fs_error(&e, "chmod", &install_script))?;
            }
            exec_file(&install_script.to_string_lossy(), &[], Some(temp_dir))?;
            if platform == "linux" {
                // fc-cache might not be available
                let _ = Command::new("/bin/sh")
                    .args(["-c", "fc-cache -f -v"])
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .output();
            }
            if std::env::var("DEBUG_FONT_INSTALL").is_ok_and(|v| v == "1") {
                INSTALLED_THIS_SESSION.store(true, Ordering::Relaxed);
            }
            return Ok((
                true,
                "Powerline fonts installed successfully! Please restart your terminal and select a Powerline font (e.g., \"Source Code Pro for Powerline\", \"Meslo LG S for Powerline\", etc.)".into(),
            ));
        }
        // For Windows, copy the font files with "powerline" in their path.
        let mut font_files = Vec::new();
        find_font_files(temp_dir, &mut font_files)?;
        let installed = font_files
            .iter()
            .filter(|f| f.file_name().is_some_and(|n| std::fs::copy(f, font_dir.join(n)).is_ok()))
            .count();
        if installed > 0 {
            Ok((
                true,
                format!(
                    "Installed {installed} Powerline fonts. Please restart your terminal and select a Powerline font from your terminal settings."
                ),
            ))
        } else {
            Err("No fonts were installed".into())
        }
    }

    fn find_font_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
        let entries = std::fs::read_dir(dir).map_err(|e| fs_error(&e, "scandir", dir))?;
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let meta = std::fs::metadata(&path).map_err(|e| fs_error(&e, "stat", &path))?;
            if meta.is_dir() && !name.starts_with('.') {
                find_font_files(&path, out)?;
            } else if (name.ends_with(".ttf") || name.ends_with(".otf"))
                && path.to_string_lossy().to_lowercase().contains("powerline")
            {
                out.push(path);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pl() -> PowerlineConfig {
        PowerlineConfig::default()
    }

    #[test]
    fn formats_separator_cap_and_theme_display_values() {
        let mut p = pl();
        assert_eq!(separator_display(&p), "\u{E0B0} - Triangle Right");
        p.separators = vec!["\u{E0B6}".into()];
        assert_eq!(separator_display(&p), "\u{E0B6} - Round Left");
        p.separators = vec!["x".into()];
        assert_eq!(separator_display(&p), "x - Custom");
        p.separators = vec!["a".into(), "b".into()];
        assert_eq!(separator_display(&p), "multiple");
        p.separators = vec![];
        assert_eq!(separator_display(&p), "\u{E0B0} - Triangle Right");
        assert_eq!(cap_display(&p, true), "none");
        p.start_caps = vec!["\u{E0BE}".into()];
        assert_eq!(cap_display(&p, true), "\u{E0BE} - Diagonal");
        p.end_caps = vec!["".into()];
        assert_eq!(cap_display(&p, false), "none");
        p.end_caps = vec!["\u{E0BC}".into(), "\u{E0B0}".into()];
        assert_eq!(cap_display(&p, false), "multiple");
        assert_eq!(theme_display(&p), "Custom");
        p.theme = Some("nord-aurora".into());
        assert_eq!(theme_display(&p), "Nord-aurora");
    }

    #[test]
    fn menu_items_are_padded_and_disabled_with_powerline_off() {
        let items = menu_items(&pl());
        let Item::Entry(e) = &items[1] else { panic!() };
        assert_eq!(e.label, "Start Cap  ");
        assert_eq!(e.sublabel.as_deref(), Some("(none)"));
        assert!(e.disabled);
    }

    #[test]
    fn theme_slots_share_a_merged_group_and_skip_separators() {
        let ws: Vec<Value> = serde_json::from_str(
            r#"[{"type":"model","merge":true},{"type":"git"},{"type":"separator"},{"type":"a"},{"type":"b","merge":"no-padding"},{"type":"c"}]"#,
        )
        .unwrap();
        let (slots, next) = assign_powerline_theme_slots(&ws, 2);
        assert_eq!(slots, vec![Some(2), Some(2), None, Some(3), Some(4), Some(4)]);
        assert_eq!(next, 5);
    }

    #[test]
    fn custom_theme_copies_colors_and_continues_across_lines() {
        let settings: Value = serde_json::from_str(
            r#"{"lines":[[{"id":"1","type":"model"},{"id":"2","type":"git"}],[{"id":"3","type":"cwd"}]],"powerline":{"enabled":true,"theme":"nord","continueThemeAcrossLines":true}}"#,
        )
        .unwrap();
        let out = apply_custom_powerline_theme(&settings, 2, "nord").unwrap();
        let theme = crate::colors::powerline_theme("nord").unwrap();
        let c = &theme.levels.as_ref().unwrap()[1];
        assert_eq!(out["lines"][1][0]["backgroundColor"], c.bg[2]);
        assert_eq!(out["lines"][0][1]["color"], c.fg[1]);
        assert_eq!(out["powerline"]["theme"], "custom");
        assert!(apply_custom_powerline_theme(&settings, 2, "custom").is_none());
        assert!(apply_custom_powerline_theme(&settings, 2, "missing").is_none());
    }

    #[test]
    fn enabling_keeps_key_order_and_removes_manual_separators() {
        let settings: Value = serde_json::from_str(
            r#"{"lines":[[{"type":"model"},{"type":"separator"}]],"defaultPadding":"","powerline":{"enabled":false,"separators":["x"],"theme":"custom","autoAlign":false}}"#,
        )
        .unwrap();
        let out = build_enabled_powerline_settings(&settings, true);
        assert_eq!(
            crate::jsobj::stringify(&crate::jsobj::strip_undefined(&out)),
            format!(
                r#"{{"lines":[[{{"type":"model"}}]],"defaultPadding":" ","powerline":{{"enabled":true,"separators":["x"],"theme":"{}","autoAlign":false}}}}"#,
                DEFAULT_POWERLINE_THEME
            )
        );
    }

    #[test]
    fn font_name_patterns() {
        for yes in [
            "Meslo LG S Regular for Powerline.ttf",
            "FiraCodeNerdFont-Regular.ttf",
            "CascadiaCodePL.ttf",
            "x POWERLINE",
        ] {
            assert!(fonts::is_powerline_font_name(yes), "{yes}");
        }
        for no in ["Menlo.ttc", "lg meslo.ttf", "DejaVuSans.ttf", "fira\ncode nerd"] {
            assert!(!fonts::is_powerline_font_name(no), "{no}");
        }
    }

    #[test]
    fn parse_int_hex_matches_js() {
        assert_eq!(parse_int_hex("E0B0"), Some(57520.0));
        assert_eq!(parse_int_hex("0X1F"), Some(31.0));
        assert_eq!(parse_int_hex("ZZ"), None);
        assert_eq!(parse_int_hex("1FZ"), Some(31.0));
    }
}
