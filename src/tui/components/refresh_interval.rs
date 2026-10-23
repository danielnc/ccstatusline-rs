//! Upstream `RefreshIntervalMenu.tsx`: the Configure Status Line screen. Claude Code's refresh interval and the
//! git, custom command and terminal width cache TTLs, each edited as a digits-only input.

use serde_json::Value;

use super::list::{Entry, Event, Item, List, Props};
use crate::jsnum::to_js_string;
use crate::tui::ink::keys::Input;
use crate::tui::ink::layout::*;
use crate::tui::screens::{Action, Ctx, Screen};
use crate::tui::text_cursor::should_insert_input;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TtlField {
    GitCacheTtl,
    CustomCommandCacheTtl,
    TerminalWidthCacheTtl,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConfigureValue {
    RefreshInterval,
    Ttl(TtlField),
}

/// `String(value)` for the interval read from Claude's settings (JSON of any type).
fn js_string(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => crate::jsjson::number(n),
        Value::String(s) => s.clone(),
        Value::Array(a) => a
            .iter()
            .map(|e| if e.is_null() { String::new() } else { js_string(e) })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".into(),
    }
}

/// `getRefreshInputValue`.
fn refresh_input_value(interval: Option<&Value>) -> String {
    interval.map(js_string).unwrap_or_default()
}

/// `getRefreshIntervalSublabel`.
fn refresh_interval_sublabel(interval: Option<&Value>, supported: bool) -> String {
    if !supported {
        return "(requires Claude Code >=2.1.97)".into();
    }
    match interval {
        None => "(not set)".into(),
        Some(v) => format!("({}s)", js_string(v)),
    }
}

/// `getGitCacheTtlSublabel`.
fn git_cache_ttl_sublabel(ttl: f64) -> String {
    if ttl == 0.0 {
        "(mtime only)".into()
    } else {
        format!("({}s)", to_js_string(ttl))
    }
}

/// `getCacheTtlSublabel`.
fn cache_ttl_sublabel(ttl: f64) -> String {
    if ttl == 0.0 {
        "(disabled)".into()
    } else {
        format!("({}s)", to_js_string(ttl))
    }
}

/// `buildConfigureStatusLineItems`.
fn build_items(interval: Option<&Value>, supported: bool, ctx: &Ctx) -> Vec<Item<ConfigureValue>> {
    let t = ctx.typed;
    vec![
        Item::Entry(
            Entry::new("🔄 Refresh Interval", ConfigureValue::RefreshInterval)
                .sublabel(Some(refresh_interval_sublabel(interval, supported)))
                .disabled(!supported)
                .description(if supported {
                    "How often Claude Code refreshes the status line by re-running the command. Enter value in seconds (1-60), or leave empty to remove."
                } else {
                    "This setting requires Claude Code version 2.1.97 or later. Please update Claude Code to use this feature."
                }),
        ),
        Item::Entry(
            Entry::new("🧮 Git Cache TTL", ConfigureValue::Ttl(TtlField::GitCacheTtl))
                .sublabel(Some(git_cache_ttl_sublabel(t.git_cache_ttl_seconds)))
                .description("How long git widget subprocess output can be reused while .git/HEAD and .git/index are unchanged. Enter 0-60 seconds;\n0 disables age-based expiry, so cached output is reused until those git metadata mtimes change."),
        ),
        Item::Entry(
            Entry::new("🔧 Custom Command Cache TTL", ConfigureValue::Ttl(TtlField::CustomCommandCacheTtl))
                .sublabel(Some(cache_ttl_sublabel(t.custom_command_cache_ttl_seconds)))
                .description("How long custom command output is reused before the command runs again. Enter 0-60 seconds;\n0 disables caching, so every status line render spawns the command."),
        ),
        Item::Entry(
            Entry::new("🖥️  Terminal Width Cache TTL", ConfigureValue::Ttl(TtlField::TerminalWidthCacheTtl))
                .sublabel(Some(cache_ttl_sublabel(t.terminal_width_cache_ttl_seconds)))
                .description("How long a cached \"no TTY detected\" result is trusted before re-probing the terminal width. Enter 0-300 seconds;\n0 disables the cache (always re-probes). A detected width is never cached across renders, only this no-TTY result is."),
        ),
    ]
}

/// `Number.parseInt(value, 10)`, NaN as `None`.
fn parse_int(value: &str) -> Option<f64> {
    crate::jsnum::parse_int(value)
}

/// `validateRefreshIntervalInput`.
fn validate_refresh_interval_input(value: &str) -> Option<String> {
    if value.is_empty() {
        return None;
    }
    let Some(parsed) = parse_int(value) else { return Some("Please enter a valid number".into()) };
    if parsed < 1.0 {
        return Some(format!("Minimum interval is 1s (you entered {}s)", to_js_string(parsed)));
    }
    if parsed > 60.0 {
        return Some(format!("Maximum interval is 60s (you entered {}s)", to_js_string(parsed)));
    }
    None
}

/// `validateTtlInput`.
fn validate_ttl_input(value: &str, label: &str, maximum: f64) -> Option<String> {
    let parsed = parse_int(value);
    let Some(parsed) = parsed.filter(|_| !value.is_empty()) else {
        return Some("Please enter a valid number".into());
    };
    if parsed < 0.0 {
        return Some(format!("Minimum {label} is 0s (you entered {}s)", to_js_string(parsed)));
    }
    if parsed > maximum {
        return Some(format!(
            "Maximum {label} is {}s (you entered {}s)",
            to_js_string(maximum),
            to_js_string(parsed)
        ));
    }
    None
}

/// One entry of upstream's `ttlFields`.
struct TtlFieldConfig {
    current_value: f64,
    max_input_length: usize,
    prompt: &'static str,
    helper_text: &'static str,
    hint: &'static str,
    label: &'static str,
    maximum: f64,
    settings_key: &'static str,
    flash: &'static str,
}

fn ttl_field(field: TtlField, ctx: &Ctx) -> TtlFieldConfig {
    let t = ctx.typed;
    match field {
        TtlField::GitCacheTtl => TtlFieldConfig {
            current_value: t.git_cache_ttl_seconds,
            max_input_length: 2,
            prompt: "Enter Git cache TTL in seconds (0-60):",
            helper_text: "This affects how quickly git widgets notice unstaged and untracked working-tree changes.",
            hint: "0 disables age-based expiry; cache validity uses .git/HEAD and .git/index mtimes only.",
            label: "Git cache TTL",
            maximum: 60.0,
            settings_key: "gitCacheTtlSeconds",
            flash: "✓ Git cache TTL updated",
        },
        TtlField::CustomCommandCacheTtl => TtlFieldConfig {
            current_value: t.custom_command_cache_ttl_seconds,
            max_input_length: 2,
            prompt: "Enter custom command cache TTL in seconds (0-60):",
            helper_text:
                "This affects how quickly custom command widgets show new output, and how often they spawn a shell.",
            hint: "0 disables caching; every status line render spawns the command again.",
            label: "custom command cache TTL",
            maximum: 60.0,
            settings_key: "customCommandCacheTtlSeconds",
            flash: "✓ Custom command cache TTL updated",
        },
        TtlField::TerminalWidthCacheTtl => TtlFieldConfig {
            current_value: t.terminal_width_cache_ttl_seconds,
            max_input_length: 3,
            prompt: "Enter Terminal Width cache TTL in seconds (0-300):",
            helper_text: "Controls how long a \"no TTY detected\" result is cached. A detected width is always re-probed on the next render so resizes take effect immediately.",
            hint: "0 disables the cache (always re-probes).",
            label: "Terminal Width cache TTL",
            maximum: 300.0,
            settings_key: "terminalWidthCacheTtlSeconds",
            flash: "✓ Terminal Width cache TTL updated",
        },
    }
}

/// `value.length` in UTF-16 units (the input only ever holds ASCII digits, but an initial value may not).
fn len16(s: &str) -> usize {
    s.encode_utf16().count()
}

/// `value.slice(0, -1)`.
fn drop_last(s: &str) -> String {
    let mut out = s.to_string();
    out.pop();
    out
}

/// `/\d/.test(input)`.
fn has_digit(input: &str) -> bool {
    input.chars().any(|c| c.is_ascii_digit())
}

pub struct RefreshIntervalMenu {
    /// `currentInterval`: the App's refresh interval, fixed while this screen is shown (it only changes on leaving).
    current_interval: Option<Value>,
    supports_refresh_interval: bool,
    editing_refresh_interval: bool,
    editing_ttl_field: Option<TtlField>,
    refresh_input: String,
    ttl_input: String,
    validation_error: Option<String>,
    list: List,
}

impl RefreshIntervalMenu {
    pub fn new(ctx: &Ctx, current_interval: Option<Value>, supports_refresh_interval: bool) -> Self {
        RefreshIntervalMenu {
            refresh_input: refresh_input_value(current_interval.as_ref()),
            ttl_input: to_js_string(ctx.typed.git_cache_ttl_seconds),
            current_interval,
            supports_refresh_interval,
            editing_refresh_interval: false,
            editing_ttl_field: None,
            validation_error: None,
            list: List::new(0),
        }
    }

    fn props<'a>(items: &'a [Item<ConfigureValue>]) -> Props<'a, ConfigureValue> {
        let mut p = Props::new(items);
        p.margin_top = 1.0;
        p.show_back_button = true;
        p
    }

    fn handle_refresh_edit(&mut self, ev: &Input) -> Vec<Action> {
        let key = &ev.key;
        if key.return_ {
            if self.refresh_input.is_empty() {
                self.editing_refresh_interval = false;
                self.validation_error = None;
                return vec![Action::SetRefreshInterval(None)];
            }
            match validate_refresh_interval_input(&self.refresh_input) {
                Some(error) => self.validation_error = Some(error),
                None => {
                    let value = parse_int(&self.refresh_input).unwrap_or(f64::NAN);
                    self.editing_refresh_interval = false;
                    self.validation_error = None;
                    return vec![Action::SetRefreshInterval(Some(value))];
                }
            }
        } else if key.escape {
            self.refresh_input = refresh_input_value(self.current_interval.as_ref());
            self.editing_refresh_interval = false;
            self.validation_error = None;
            self.list = List::new(0);
        } else if key.backspace {
            self.refresh_input = drop_last(&self.refresh_input);
            self.validation_error = None;
        } else if key.delete {
            // No cursor position in simple input
        } else if should_insert_input(&ev.input, key) && has_digit(&ev.input) {
            let new_value = format!("{}{}", self.refresh_input, ev.input);
            if len16(&new_value) <= 2 {
                self.refresh_input = new_value;
                self.validation_error = None;
            }
        }
        Vec::new()
    }

    fn handle_ttl_edit(&mut self, field: TtlField, ev: &Input, ctx: &Ctx) -> Vec<Action> {
        let cfg = ttl_field(field, ctx);
        let key = &ev.key;
        if key.return_ {
            match validate_ttl_input(&self.ttl_input, cfg.label, cfg.maximum) {
                Some(error) => self.validation_error = Some(error),
                None => {
                    let value = parse_int(&self.ttl_input).unwrap_or(f64::NAN);
                    self.editing_ttl_field = None;
                    self.validation_error = None;
                    let next = crate::jsobj::with(ctx.settings, cfg.settings_key, crate::jsjson::num(value));
                    return vec![Action::SetSettings(next), Action::Flash(cfg.flash.into(), "green"), Action::Back];
                }
            }
        } else if key.escape {
            self.ttl_input = to_js_string(cfg.current_value);
            self.editing_ttl_field = None;
            self.validation_error = None;
            self.list = List::new(0);
        } else if key.backspace {
            self.ttl_input = drop_last(&self.ttl_input);
            self.validation_error = None;
        } else if key.delete {
            // No cursor position in simple input
        } else if should_insert_input(&ev.input, key) && has_digit(&ev.input) {
            let new_value = format!("{}{}", self.ttl_input, ev.input);
            if len16(&new_value) <= cfg.max_input_length {
                self.ttl_input = new_value;
                self.validation_error = None;
            }
        }
        Vec::new()
    }
}

impl Screen for RefreshIntervalMenu {
    fn handle(&mut self, ev: &Input, ctx: &Ctx) -> Vec<Action> {
        if self.editing_refresh_interval {
            return self.handle_refresh_edit(ev);
        }
        if let Some(field) = self.editing_ttl_field {
            return self.handle_ttl_edit(field, ev, ctx);
        }
        // The List's handler runs first; the menu's own sees the state of the last render (not editing).
        let items = build_items(self.current_interval.as_ref(), self.supports_refresh_interval, ctx);
        let p = Self::props(&items);
        self.list.sync(&p);
        let mut actions = Vec::new();
        match self.list.handle(&p, ev) {
            Event::Back => actions.push(Action::Back),
            Event::Selected(ConfigureValue::RefreshInterval, _) => {
                self.refresh_input = refresh_input_value(self.current_interval.as_ref());
                self.editing_refresh_interval = true;
            }
            Event::Selected(ConfigureValue::Ttl(field), _) => {
                self.ttl_input = to_js_string(ttl_field(field, ctx).current_value);
                self.editing_ttl_field = Some(field);
            }
            Event::None => {}
        }
        if ev.key.escape && actions.is_empty() {
            actions.push(Action::Back);
        }
        actions
    }

    fn view(&mut self, ctx: &Ctx) -> El {
        let mut children = vec![
            text(TextStyle::bold(), vec![s("Configure Status Line")]),
            text(TextStyle::color("white"), vec![s("Configure Claude Code status line settings")]),
        ];
        if self.editing_refresh_interval {
            let suffix = if self.refresh_input.is_empty() { "" } else { "s" };
            children.push(boxed(
                BoxStyle::column().margin_top(1.0),
                vec![
                    plain(format!("Enter refresh interval in seconds (1-60): {}{suffix}", self.refresh_input)),
                    match &self.validation_error {
                        Some(e) => text(TextStyle::color("red"), vec![s(e)]),
                        None => text(
                            TextStyle::dim(),
                            vec![s("Press Enter to confirm, ESC to cancel. Leave empty to remove.")],
                        ),
                    },
                ],
            ));
        } else if let Some(field) = self.editing_ttl_field {
            let cfg = ttl_field(field, ctx);
            let suffix = if self.ttl_input.is_empty() { "" } else { "s" };
            children.push(boxed(
                BoxStyle::column().margin_top(1.0),
                vec![
                    plain(format!("{} {}{suffix}", cfg.prompt, self.ttl_input)),
                    plain(" "),
                    text(TextStyle::dim(), vec![s(cfg.helper_text)]),
                    match &self.validation_error {
                        Some(e) => text(TextStyle::color("red"), vec![s(e)]),
                        None => text(TextStyle::dim(), vec![s(cfg.hint)]),
                    },
                    text(TextStyle::dim(), vec![s("Press Enter to confirm, ESC to cancel.")]),
                ],
            ));
        } else {
            let items = build_items(self.current_interval.as_ref(), self.supports_refresh_interval, ctx);
            let p = Self::props(&items);
            self.list.sync(&p);
            children.push(self.list.view(&p));
        }
        boxed(BoxStyle::column(), children)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_like_upstream() {
        assert_eq!(validate_refresh_interval_input(""), None);
        assert_eq!(validate_refresh_interval_input("0").as_deref(), Some("Minimum interval is 1s (you entered 0s)"));
        assert_eq!(validate_refresh_interval_input("61").as_deref(), Some("Maximum interval is 60s (you entered 61s)"));
        assert_eq!(validate_refresh_interval_input("x").as_deref(), Some("Please enter a valid number"));
        assert_eq!(validate_ttl_input("", "Git cache TTL", 60.0).as_deref(), Some("Please enter a valid number"));
        assert_eq!(
            validate_ttl_input("301", "Terminal Width cache TTL", 300.0).as_deref(),
            Some("Maximum Terminal Width cache TTL is 300s (you entered 301s)")
        );
        assert_eq!(validate_ttl_input("0", "Git cache TTL", 60.0), None);
    }

    #[test]
    fn interval_strings() {
        assert_eq!(refresh_interval_sublabel(None, true), "(not set)");
        assert_eq!(refresh_interval_sublabel(Some(&serde_json::json!(5)), true), "(5s)");
        assert_eq!(refresh_interval_sublabel(Some(&serde_json::json!(5)), false), "(requires Claude Code >=2.1.97)");
        assert_eq!(refresh_input_value(Some(&serde_json::json!("7"))), "7");
    }
}
