//! Upstream `ImportConfigDialog.tsx` and `ImportPreviewDialog.tsx`, with App.tsx's `handleImportFileChosen` and
//! `handleImportApply` and `validateImportFile`/`applyImport` from `src/utils/config.ts`.
//!
//! Upstream switches from the `importConfig` screen to `importPreview` once the file validates; here both are
//! this one hosted screen, and the switch mounts the preview afresh as React does.

use std::collections::HashSet;

use serde_json::{Map, Value};

use super::export_config::{expand_path, path_input_handle, path_input_view};
use super::list::{Entry, Event, Item, List, Props};
use crate::jsobj::{self, stringify};
use crate::settings::CURRENT_VERSION;
use crate::tui::ink::keys::Input;
use crate::tui::ink::layout::*;
use crate::tui::screens::{Action, Ctx, Screen};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Replace,
    Merge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ImportMode {
    Apply(Mode),
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConfirmChoice {
    Cancel,
    Confirm,
}

/// `{ status: 'valid', data, presentKeys }`.
struct ValidImport {
    data: Value,
    present_keys: Vec<String>,
}

// ---------------------------------------------------------------------------------------- validateImportFile

/// zod's `parsedType` with the English locale's names.
fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn invalid_type(expected: &str, v: &Value) -> String {
    format!("Invalid input: expected {expected}, received {}", type_name(v))
}

fn check_number(v: &Value, min: Option<f64>, max: Option<f64>) -> Option<String> {
    let Some(n) = v.as_f64() else { return Some(invalid_type("number", v)) };
    let fmt = crate::jsnum::to_js_string;
    if let Some(m) = min.filter(|m| n < *m) {
        return Some(format!("Too small: expected number to be >={}", fmt(m)));
    }
    if let Some(m) = max.filter(|m| n > *m) {
        return Some(format!("Too big: expected number to be <={}", fmt(m)));
    }
    None
}

fn check_enum(v: &Value, values: &[&str]) -> Option<String> {
    if v.as_str().is_some_and(|s| values.contains(&s)) {
        return None;
    }
    let joined: Vec<String> = values.iter().map(|s| format!("\"{s}\"")).collect();
    Some(format!("Invalid option: expected one of {}", joined.join("|")))
}

/// `result.error.issues[0].message` for `SettingsSchema.safeParse(v)`, which reports issues in the schema's key
/// order. Top-level fields get zod's own messages; a nested failure (inside a widget, `powerline`, ...) falls back
/// to the Rust parser's description.
fn zod_first_issue(v: &Value, fallback: &str) -> String {
    let Some(o) = v.as_object() else { return invalid_type("object", v) };
    let nested_ok = |key: &str, value: &Value| {
        let mut m = Map::new();
        m.insert(key.to_string(), value.clone());
        crate::settings::parse_settings(&Value::Object(m)).is_ok()
    };
    let fields: &[&str] = &[
        "version",
        "lines",
        "flexMode",
        "compactThreshold",
        "colorLevel",
        "defaultSeparator",
        "defaultPadding",
        "defaultPaddingSide",
        "inheritSeparatorColors",
        "overrideBackgroundColor",
        "overrideForegroundColor",
        "globalBold",
        "numberFormat",
        "gitCacheTtlSeconds",
        "terminalWidthCacheTtlSeconds",
        "customCommandCacheTtlSeconds",
        "minimalistMode",
        "powerline",
        "updatemessage",
        "installation",
    ];
    for &key in fields {
        let Some(value) = o.get(key) else { continue };
        let issue = match key {
            "version" => check_number(value, None, None),
            "lines" => match value.as_array() {
                None => Some(invalid_type("array", value)),
                Some(lines) => {
                    // Element issues come in index order.
                    for line in lines {
                        let Some(widgets) = line.as_array() else { return invalid_type("array", line) };
                        for w in widgets {
                            if !w.is_object() {
                                return invalid_type("object", w);
                            }
                            if !nested_ok(key, &Value::Array(vec![Value::Array(vec![w.clone()])])) {
                                return fallback.to_string();
                            }
                        }
                    }
                    lines.is_empty().then(|| "Too small: expected array to have >=1 items".into())
                }
            },
            "flexMode" => check_enum(value, &["full", "full-minus-40", "full-until-compact"]),
            "compactThreshold" => check_number(value, Some(1.0), Some(99.0)),
            "colorLevel" => {
                (!value.as_f64().is_some_and(|n| [0.0, 1.0, 2.0, 3.0].contains(&n))).then(|| "Invalid input".into())
            }
            "defaultSeparator" | "defaultPadding" | "overrideBackgroundColor" | "overrideForegroundColor" => {
                (!value.is_string()).then(|| invalid_type("string", value))
            }
            "defaultPaddingSide" => check_enum(value, &["both", "left", "right"]),
            "inheritSeparatorColors" | "globalBold" | "minimalistMode" => {
                (!value.is_boolean()).then(|| invalid_type("boolean", value))
            }
            "gitCacheTtlSeconds" | "customCommandCacheTtlSeconds" => check_number(value, Some(0.0), Some(60.0)),
            "terminalWidthCacheTtlSeconds" => check_number(value, Some(0.0), Some(300.0)),
            _ => {
                if !value.is_object() {
                    Some(invalid_type("object", value))
                } else if !nested_ok(key, value) {
                    return fallback.to_string();
                } else {
                    None
                }
            }
        };
        if let Some(message) = issue {
            return message;
        }
    }
    fallback.to_string()
}

/// `validateImportFile(filePath)`.
fn validate_import_file(file_path: &str) -> Result<ValidImport, String> {
    let expanded = expand_path(file_path);
    let raw = match std::fs::read(crate::settings::path_resolve(&expanded)) {
        Ok(bytes) if !expanded.is_empty() => String::from_utf8_lossy(&bytes).into_owned(),
        _ => return Err(format!("Cannot read file: {expanded}")),
    };
    let Ok(mut parsed) = serde_json::from_str::<Value>(&raw) else { return Err("File is not valid JSON".into()) };
    if let Some(version) = parsed.get("version").and_then(Value::as_f64) {
        if parsed.is_object() && version > CURRENT_VERSION {
            return Err(format!(
                "Config version {} is newer than supported version {}",
                crate::jsnum::to_js_string(version),
                crate::jsnum::to_js_string(CURRENT_VERSION)
            ));
        }
    }
    if crate::migrations::needs_migration(&parsed, CURRENT_VERSION) {
        parsed = crate::migrations::migrate_config(parsed, CURRENT_VERSION);
    }
    let settings = match crate::settings::parse_settings(&parsed) {
        Ok(s) => s,
        Err(e) => return Err(format!("Invalid config format: {}", zod_first_issue(&parsed, &e))),
    };
    let data = crate::settings::settings_json(&settings);
    let present_keys = parsed
        .as_object()
        .map(|o| o.keys().filter(|k| data.get(k.as_str()).is_some()).cloned().collect())
        .unwrap_or_default();
    Ok(ValidImport { data, present_keys })
}

const IMPORT_EXCLUDED_KEYS: &[&str] = &["installation", "version", "updatemessage", "exportedBy"];

/// `applyImport(current, imported, mode, presentKeys)`.
fn apply_import(current: &Value, imported: &Value, mode: Mode, present_keys: &[String]) -> Value {
    let merge_keys: HashSet<&str> = present_keys.iter().map(String::as_str).collect();
    let mut imported_clean = Map::new();
    if let Some(o) = imported.as_object() {
        for (k, v) in o {
            if !IMPORT_EXCLUDED_KEYS.contains(&k.as_str()) && (mode == Mode::Replace || merge_keys.contains(k.as_str()))
            {
                imported_clean.insert(k.clone(), v.clone());
            }
        }
    }
    match mode {
        Mode::Replace => {
            if let Some(installation) = jsobj::get(current, "installation") {
                imported_clean.insert("installation".into(), installation.clone());
            }
            match crate::settings::parse_settings(&Value::Object(imported_clean)) {
                Ok(s) => crate::settings::settings_json(&s),
                Err(_) => imported.clone(),
            }
        }
        Mode::Merge => jsobj::merge(current, &Value::Object(imported_clean)),
    }
}

// ---------------------------------------------------------------------------------------- ImportPreviewDialog

const EXCLUDED_KEYS: &[&str] = &["version", "installation", "updatemessage"];

/// `Object.keys(v)`, including keys holding `undefined`; an array's indices.
fn js_keys(v: &Value) -> Vec<String> {
    match v {
        Value::Object(o) => o.keys().cloned().collect(),
        Value::Array(a) => (0..a.len()).map(|i| i.to_string()).collect(),
        _ => Vec::new(),
    }
}

/// `o[key]`, `undefined` as `None`.
fn prop<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Object(_) => jsobj::get(v, key),
        Value::Array(a) => key.parse::<usize>().ok().and_then(|i| a.get(i)).filter(|e| !jsobj::is_undefined(e)),
        _ => None,
    }
}

/// `JSON.stringify(a) === JSON.stringify(b)`, where `undefined` stringifies to `undefined`.
fn same(a: Option<&Value>, b: Option<&Value>) -> bool {
    a.map(stringify) == b.map(stringify)
}

fn union_keys(a: Option<&Value>, b: Option<&Value>) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    for k in a.map(js_keys).unwrap_or_default().into_iter().chain(b.map(js_keys).unwrap_or_default()) {
        if !keys.contains(&k) {
            keys.push(k);
        }
    }
    keys
}

/// `getImportPreviewKeys`.
fn preview_keys(current: &Value, imported: &Value) -> Vec<String> {
    union_keys(Some(current), Some(imported)).into_iter().filter(|k| !EXCLUDED_KEYS.contains(&k.as_str())).collect()
}

/// `escapeControlCharacters`.
fn escape_control_characters(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        let u = c as u32;
        if u <= 0x1f || (0x7f..=0x9f).contains(&u) {
            out.push_str(&format!("\\u{u:04x}"));
        } else {
            out.push(c);
        }
    }
    out
}

/// `formatScalar`.
fn format_scalar(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => "none".into(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => crate::jsjson::number(n),
        Some(Value::String(s)) => {
            let e = escape_control_characters(s);
            if e.is_empty() {
                "(empty)".into()
            } else {
                e
            }
        }
        Some(other) => escape_control_characters(&stringify(other)),
    }
}

/// `getCustomCommands`.
fn custom_commands(settings: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for line in jsobj::get(settings, "lines").and_then(Value::as_array).into_iter().flatten() {
        for widget in line.as_array().into_iter().flatten() {
            if jsobj::get_str(widget, "type") != Some("custom-command") {
                continue;
            }
            if let Some(path) = jsobj::get(widget, "commandPath").filter(|p| crate::tui::truthy(p)) {
                out.push(path.as_str().map(str::to_string).unwrap_or_else(|| stringify(path)));
            }
        }
    }
    out
}

/// `getImportedCommands`.
fn imported_commands(current: &Value, preview: &Value) -> Vec<String> {
    let existing: HashSet<String> = custom_commands(current).into_iter().collect();
    let mut out: Vec<String> = Vec::new();
    for c in custom_commands(preview) {
        if !existing.contains(&c) && !out.contains(&c) {
            out.push(c);
        }
    }
    out
}

struct DiffEntry {
    path: String,
    current: Option<Value>,
    imported: Option<Value>,
}

/// `diffObject`.
fn diff_object(current: &Value, imported: &Value, prefix: &str) -> Vec<DiffEntry> {
    let mut entries = Vec::new();
    for key in union_keys(Some(current), Some(imported)) {
        let path = if prefix.is_empty() { key.clone() } else { format!("{prefix}.{key}") };
        let a = prop(current, &key);
        let b = prop(imported, &key);
        if same(a, b) {
            continue;
        }
        match (a, b) {
            (Some(a @ Value::Object(_)), Some(b @ Value::Object(_))) if crate::tui::truthy(a) => {
                entries.extend(diff_object(a, b, &path))
            }
            _ => entries.push(DiffEntry { path, current: a.cloned(), imported: b.cloned() }),
        }
    }
    entries
}

/// `diffLines`.
fn diff_lines(current: &Value, imported: &Value) -> Vec<DiffEntry> {
    let empty = Vec::new();
    let cur_lines = current.as_array().unwrap_or(&empty);
    let imp_lines = imported.as_array().unwrap_or(&empty);
    let mut entries = Vec::new();
    for li in 0..cur_lines.len().max(imp_lines.len()) {
        let cur_line = cur_lines.get(li).and_then(Value::as_array).unwrap_or(&empty);
        let imp_line = imp_lines.get(li).and_then(Value::as_array).unwrap_or(&empty);
        for wi in 0..cur_line.len().max(imp_line.len()) {
            let cur_widget = cur_line.get(wi).filter(|w| crate::tui::truthy(w));
            let imp_widget = imp_line.get(wi).filter(|w| crate::tui::truthy(w));
            if same(cur_widget, imp_widget) {
                continue;
            }
            let Some(cur_widget) = cur_widget else {
                let added = imp_widget.and_then(|w| jsobj::get(w, "type")).map(format_type);
                entries.push(DiffEntry {
                    path: format!("line {} +{}", li + 1, added.unwrap_or_else(|| "unknown".into())),
                    current: None,
                    imported: Some(Value::String("[added]".into())),
                });
                continue;
            };
            let cur_type = jsobj::get(cur_widget, "type").map(format_type).unwrap_or_else(|| "undefined".into());
            let Some(imp_widget) = imp_widget else {
                entries.push(DiffEntry {
                    path: format!("line {} -{cur_type}", li + 1),
                    current: Some(Value::String("[removed]".into())),
                    imported: None,
                });
                continue;
            };
            let label = format!("{cur_type} (line {})", li + 1);
            for key in union_keys(Some(cur_widget), Some(imp_widget)) {
                if key == "id" {
                    continue;
                }
                let a = prop(cur_widget, &key);
                let b = prop(imp_widget, &key);
                if !same(a, b) {
                    entries.push(DiffEntry {
                        path: format!("{label} {key}"),
                        current: a.cloned(),
                        imported: b.cloned(),
                    });
                }
            }
        }
    }
    entries
}

/// A widget's `type` in a template string.
fn format_type(t: &Value) -> String {
    match t {
        Value::String(s) => s.clone(),
        other => format_scalar(Some(other)),
    }
}

fn diff_row(path: &str, current: Option<&Value>, imported: Option<&Value>) -> Vec<El> {
    vec![
        plain(path),
        text(TextStyle::color("red"), vec![s(format_scalar(current))]),
        plain(" → "),
        text(TextStyle::color("green"), vec![s(format_scalar(imported))]),
    ]
}

fn entry_rows(key: &str, entries: &[DiffEntry]) -> El {
    let mut children = vec![plain(format!("  {key}:"))];
    for e in entries {
        children.push(boxed(
            BoxStyle::default().margin_left(4.0),
            diff_row(&format!("{}: ", escape_control_characters(&e.path)), e.current.as_ref(), e.imported.as_ref()),
        ));
    }
    boxed(BoxStyle::column(), children)
}

fn commands_box(commands: &[String]) -> El {
    boxed(
        BoxStyle::column().margin_left(4.0),
        commands.iter().map(|c| plain(format!("$ {}", escape_control_characters(c)))).collect(),
    )
}

struct Preview {
    validation: ValidImport,
    preview_mode: Mode,
    /// The mode waiting for the user to confirm the shell commands it adds.
    pending_mode: Option<Mode>,
    list: List,
    confirm_list: List,
}

fn preview_items() -> Vec<Item<ImportMode>> {
    vec![
        Item::Entry(
            Entry::new("Replace All", ImportMode::Apply(Mode::Replace))
                .description("Overwrite all settings with the imported config"),
        ),
        Item::Entry(
            Entry::new("Merge", ImportMode::Apply(Mode::Merge))
                .description("Overlay imported settings on top of current settings"),
        ),
        Item::Separator,
        Item::Entry(Entry::new("Cancel", ImportMode::Cancel)),
    ]
}

fn confirm_items() -> Vec<Item<ConfirmChoice>> {
    vec![
        Item::Entry(
            Entry::new("Cancel", ConfirmChoice::Cancel).description("Go back to the preview without importing"),
        ),
        Item::Entry(
            Entry::new("Apply and run these commands", ConfirmChoice::Confirm)
                .description("Import the settings, including the commands above"),
        ),
    ]
}

impl Preview {
    fn new(validation: ValidImport) -> Self {
        Preview {
            validation,
            preview_mode: Mode::Replace,
            pending_mode: None,
            list: List::new(0),
            confirm_list: List::new(0),
        }
    }

    /// `getImportPreviewSettings`.
    fn preview_settings(&self, current: &Value, mode: Mode) -> Value {
        apply_import(current, &self.validation.data, mode, &self.validation.present_keys)
    }

    /// The List's `onSelectionChange` effect.
    fn selection_changed(&mut self, items: &[Item<ImportMode>]) {
        let p = Props::new(items);
        if let Some(ImportMode::Apply(mode)) = self.list.selected_value(&p) {
            self.preview_mode = *mode;
        }
    }

    /// App.tsx's `handleImportApply`.
    fn apply(&self, mode: Mode, ctx: &Ctx) -> Vec<Action> {
        let next = self.preview_settings(ctx.settings, mode);
        // applyTuiImport: chalk.level = nextSettings.colorLevel
        if let Some(level) = jsobj::get(&next, "colorLevel").and_then(Value::as_u64) {
            crate::colors::set_chalk_level(level as u8);
        }
        vec![
            Action::SetSettings(next),
            Action::Flash("Config imported — review and save".into(), "green"),
            Action::Back,
        ]
    }

    fn handle(&mut self, ev: &Input, ctx: &Ctx) -> Vec<Action> {
        if let Some(pending) = self.pending_mode {
            let items = confirm_items();
            let p = Props::new(&items);
            self.confirm_list.sync(&p);
            match self.confirm_list.handle(&p, ev) {
                Event::Selected(ConfirmChoice::Confirm, _) => return self.apply(pending, ctx),
                Event::Selected(ConfirmChoice::Cancel, _) | Event::Back => {
                    self.pending_mode = None;
                    // The preview's list mounts again, at its first item.
                    self.list = List::new(0);
                    let items = preview_items();
                    self.list.sync(&Props::new(&items));
                    self.selection_changed(&items);
                }
                Event::None => {}
            }
            return Vec::new();
        }
        let items = preview_items();
        let p = Props::new(&items);
        self.list.sync(&p);
        let event = self.list.handle(&p, ev);
        self.selection_changed(&items);
        match event {
            Event::Selected(ImportMode::Cancel, _) | Event::Back => vec![Action::Back],
            Event::Selected(ImportMode::Apply(mode), _) => {
                if !imported_commands(ctx.settings, &self.preview_settings(ctx.settings, mode)).is_empty() {
                    self.pending_mode = Some(mode);
                    self.confirm_list = List::new(0);
                    return Vec::new();
                }
                self.apply(mode, ctx)
            }
            Event::None => Vec::new(),
        }
    }

    fn view(&mut self, ctx: &Ctx) -> El {
        let current = ctx.settings;
        if let Some(pending) = self.pending_mode {
            let commands = imported_commands(current, &self.preview_settings(current, pending));
            let what = if commands.len() == 1 {
                "a shell command".to_string()
            } else {
                format!("{} shell commands", commands.len())
            };
            let items = confirm_items();
            let p = Props::new(&items);
            self.confirm_list.sync(&p);
            return boxed(
                BoxStyle::column(),
                vec![
                    text(TextStyle::bold(), vec![s("Import Preview")]),
                    text(
                        TextStyle::color("yellow"),
                        vec![s(format!(
                            "Apply this import? It adds {what} that will run on every status line render:"
                        ))],
                    ),
                    commands_box(&commands),
                    self.confirm_list.view(&p),
                ],
            );
        }

        let preview = self.preview_settings(current, self.preview_mode);
        let commands = imported_commands(current, &preview);
        let mut diff_rows: Vec<El> = Vec::new();
        for key in preview_keys(current, &preview) {
            let cur = jsobj::get(current, &key);
            let imp = jsobj::get(&preview, &key);
            if same(cur, imp) {
                diff_rows.push(row(vec![text(TextStyle::dim(), vec![s(format!("  {key}: {}", format_scalar(cur)))])]));
                continue;
            }
            if key == "lines" {
                let null = Value::Null;
                diff_rows.push(entry_rows(&key, &diff_lines(cur.unwrap_or(&null), imp.unwrap_or(&null))));
                continue;
            }
            if let (Some(c @ Value::Object(_)), Some(i @ (Value::Object(_) | Value::Array(_)))) = (cur, imp) {
                diff_rows.push(entry_rows(&key, &diff_object(c, i, &key)));
                continue;
            }
            diff_rows.push(row(diff_row(&format!("  {}: ", escape_control_characters(&key)), cur, imp)));
        }

        let mut children = vec![
            text(TextStyle::bold(), vec![s("Import Preview")]),
            text(TextStyle::dim(), vec![s("Changes that will be applied:")]),
            boxed(BoxStyle::column(), diff_rows),
        ];
        if !commands.is_empty() {
            children.push(boxed(
                BoxStyle::column().margin_top(1.0),
                vec![
                    text(
                        TextStyle::color("yellow"),
                        vec![s("This import adds shell commands that run on every status line render:")],
                    ),
                    commands_box(&commands),
                ],
            ));
        }
        let items = preview_items();
        let p = Props::new(&items);
        self.list.sync(&p);
        children.push(self.list.view(&p));
        boxed(BoxStyle::column(), children)
    }
}

pub struct ImportConfigDialog {
    input_value: String,
    preview: Option<Preview>,
}

impl ImportConfigDialog {
    pub fn new(_ctx: &Ctx) -> Self {
        ImportConfigDialog { input_value: String::new(), preview: None }
    }
}

impl Screen for ImportConfigDialog {
    fn handle(&mut self, ev: &Input, ctx: &Ctx) -> Vec<Action> {
        if let Some(preview) = self.preview.as_mut() {
            return preview.handle(ev, ctx);
        }
        match path_input_handle(&mut self.input_value, ev) {
            Some(true) => match validate_import_file(&self.input_value) {
                Err(message) => vec![Action::FlowNotice { title: "Import Failed".into(), message, color: "red" }],
                Ok(valid) => {
                    let mut preview = Preview::new(valid);
                    let items = preview_items();
                    preview.list.sync(&Props::new(&items));
                    preview.selection_changed(&items);
                    self.preview = Some(preview);
                    Vec::new()
                }
            },
            Some(false) => vec![Action::Back],
            None => Vec::new(),
        }
    }

    fn view(&mut self, ctx: &Ctx) -> El {
        match self.preview.as_mut() {
            Some(preview) => preview.view(ctx),
            None => {
                path_input_view("Import Config", "Enter the file path to import configuration from:", &self.input_value)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn zod_messages_for_top_level_fields() {
        assert_eq!(zod_first_issue(&json!(5), ""), "Invalid input: expected object, received number");
        assert_eq!(
            zod_first_issue(&json!({"flexMode": "wide"}), ""),
            "Invalid option: expected one of \"full\"|\"full-minus-40\"|\"full-until-compact\""
        );
        assert_eq!(zod_first_issue(&json!({"gitCacheTtlSeconds": 61}), ""), "Too big: expected number to be <=60");
        assert_eq!(zod_first_issue(&json!({"lines": []}), ""), "Too small: expected array to have >=1 items");
        assert_eq!(zod_first_issue(&json!({"colorLevel": 7}), ""), "Invalid input");
        assert_eq!(
            zod_first_issue(&json!({"globalBold": "yes", "compactThreshold": 0}), ""),
            "Too small: expected number to be >=1"
        );
    }

    #[test]
    fn escapes_and_formats() {
        assert_eq!(escape_control_characters("a\u{1b}[31mb\u{85}"), "a\\u001b[31mb\\u0085");
        assert_eq!(format_scalar(Some(&json!(""))), "(empty)");
        assert_eq!(format_scalar(None), "none");
        assert_eq!(format_scalar(Some(&json!([1, "x"]))), "[1,\"x\"]");
    }

    #[test]
    fn merge_overlays_only_present_keys() {
        let current =
            json!({"version": 4, "flexMode": "full", "globalBold": false, "installation": {"method": "pinned"}});
        let imported = json!({"version": 4, "flexMode": "full-minus-40", "globalBold": true});
        let merged = apply_import(&current, &imported, Mode::Merge, &["flexMode".into()]);
        assert_eq!(
            stringify(&merged),
            r#"{"version":4,"flexMode":"full-minus-40","globalBold":false,"installation":{"method":"pinned"}}"#
        );
    }
}
