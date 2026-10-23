//! Checks every widget's editor behaviour against `testdata/editor-meta.json`, which `tools/gen-editor-meta.mjs`
//! records from upstream's own widget registry.

use super::editor as widget_editor;
use super::{get_widget, HideableState, Keybind};
use serde_json::Value;

const FIXTURE: &str = include_str!("testdata/editor-meta.json");

/// Widget types whose editor methods are ported; the per-item checks run for these. Add a type here once its
/// `editor_display`, `label_prefix`, `custom_keybinds` and `handle_editor_action` follow upstream's.
const PORTED: &[&str] = &["model", "context-length", "git-branch", "git-changes"];

fn fixture() -> Value {
    serde_json::from_str(FIXTURE).unwrap()
}

fn layout_entry(kind: &str) -> Option<(&'static str, &'static str, &'static str)> {
    match kind {
        "separator" => Some(("Separator", "Layout", "A separator character between status line widgets")),
        "flex-separator" => Some(("Flex Separator", "Layout", "Expands to fill available terminal width")),
        _ => None,
    }
}

#[test]
fn catalog_matches_upstream() {
    let f = fixture();
    let mut failures = Vec::new();
    for w in f["widgets"].as_array().unwrap() {
        let kind = w["type"].as_str().unwrap();
        let (name, category, description) = match layout_entry(kind) {
            Some(e) => e,
            None => match get_widget(kind) {
                Some(widget) => (widget.display_name(), widget.category(), widget.description()),
                None => {
                    failures.push(format!("{kind}: not in the registry"));
                    continue;
                }
            },
        };
        for (field, ours, theirs) in [
            ("displayName", name, &w["displayName"]),
            ("category", category, &w["category"]),
            ("description", description, &w["description"]),
        ] {
            if theirs.as_str() != Some(ours) {
                failures.push(format!("{kind}.{field}: {ours:?} vs upstream {theirs}"));
            }
        }
    }
    assert!(failures.is_empty(), "{} catalog mismatches:\n{}", failures.len(), failures.join("\n"));
}

fn keybinds_json(kbs: &[Keybind]) -> Value {
    Value::Array(kbs.iter().map(|k| serde_json::json!({"key": k.key, "label": k.label, "action": k.action})).collect())
}

fn states_json(states: &[HideableState]) -> Value {
    Value::Array(
        states
            .iter()
            .map(|s| {
                let mut o = serde_json::json!({"key": s.key, "label": s.label});
                if s.default_enabled {
                    o["defaultEnabled"] = Value::Bool(true);
                }
                o
            })
            .collect(),
    )
}

fn opt_str(v: Option<String>) -> Value {
    v.map(Value::String).unwrap_or(Value::Null)
}

#[test]
fn ported_widgets_match_upstream() {
    let f = fixture();
    let mut failures = Vec::new();
    let mut checked = 0;
    for w in f["widgets"].as_array().unwrap() {
        let kind = w["type"].as_str().unwrap();
        if !PORTED.contains(&kind) {
            continue;
        }
        let widget = get_widget(kind).unwrap();
        let mut check = |what: String, ours: Value, theirs: &Value| {
            if &ours != theirs {
                failures.push(format!("{kind} {what}:\n  ours     {ours}\n  upstream {theirs}"));
            }
        };
        check("supportsRawValue".into(), Value::Bool(widget.supports_raw_value()), &w["supportsRawValue"]);
        check("supportsNumberFormat".into(), Value::Bool(widget.supports_number_format()), &w["supportsNumberFormat"]);
        check("hideableStates".into(), states_json(widget.hideable_states()), &w["hideableStates"]);
        for entry in w["items"].as_array().unwrap() {
            checked += 1;
            let item = &entry["item"];
            let at = crate::jsobj::stringify(item);
            let typed = crate::settings::parse_widget_item(item).unwrap();
            let display = widget.editor_display(&typed);
            let mut d = serde_json::json!({"displayText": display.text});
            if let Some(m) = display.modifier {
                d["modifierText"] = Value::String(m);
            }
            check(format!("display at {at}"), d, &entry["display"]);
            check(
                format!("supportsColors at {at}"),
                Value::Bool(widget.supports_colors(&typed)),
                &entry["supportsColors"],
            );
            check(format!("labelPrefix at {at}"), opt_str(widget.label_prefix(&typed)), &entry["labelPrefix"]);
            let kbs = widget.custom_keybinds(&typed);
            check(format!("keybinds at {at}"), keybinds_json(&kbs), &entry["keybinds"]);
            for (action, expected) in entry["actions"].as_object().unwrap() {
                let ours = widget.handle_editor_action(action, item).map(|v| crate::jsobj::strip_undefined(&v));
                let ours_s = ours.as_ref().map(crate::jsobj::stringify);
                let theirs_s = (!expected.is_null()).then(|| crate::jsjson::stringify(expected));
                check(
                    format!("action {action} at {at}"),
                    ours_s.map(Value::String).unwrap_or(Value::Null),
                    &theirs_s.map(Value::String).unwrap_or(Value::Null),
                );
            }
            check(
                format!("hideModifier at {at}"),
                opt_str(widget_editor::hide_modifier_text(item, widget.hideable_states())),
                &entry["hideModifier"],
            );
            check(
                format!("labelModifier at {at}"),
                opt_str(widget_editor::label_modifier_text(item)),
                &entry["labelModifier"],
            );
            check(
                format!("numberFormatModifier at {at}"),
                opt_str(widget_editor::number_format_modifier_text(item)),
                &entry["numberFormatModifier"],
            );
        }
    }
    assert!(checked > 0);
    assert!(failures.is_empty(), "{} mismatches over {checked} items:\n{}", failures.len(), failures.join("\n"));
}

#[test]
fn picker_search_matches_upstream() {
    let f = fixture();
    let catalog = super::catalog::catalog(false, false);
    let types: Vec<&str> = catalog.iter().map(|e| e.kind.as_str()).collect();
    let theirs: Vec<&str> = f["catalog"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
    assert_eq!(types, theirs, "catalog order");
    let with_powerline: Vec<String> = super::catalog::catalog(true, false).into_iter().map(|e| e.kind).collect();
    let theirs_pl: Vec<String> =
        f["catalogWithPowerline"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
    assert_eq!(with_powerline, theirs_pl, "catalog with powerline");
    let cats = super::catalog::categories(&catalog);
    let theirs_cats: Vec<String> =
        f["categories"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
    assert_eq!(cats, theirs_cats, "categories");

    let mut failures = Vec::new();
    for s in f["search"].as_array().unwrap() {
        let (category, query) = (s["category"].as_str().unwrap(), s["query"].as_str().unwrap());
        let ours: Vec<&str> =
            super::catalog::filter(&catalog, category, query).iter().map(|e| e.kind.as_str()).collect();
        let theirs: Vec<&str> = s["types"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        if ours != theirs {
            failures.push(format!("filter({category:?}, {query:?}):\n  ours     {ours:?}\n  upstream {theirs:?}"));
        }
    }
    for s in f["segments"].as_array().unwrap() {
        let (text, query) = (s["text"].as_str().unwrap(), s["query"].as_str().unwrap());
        let ours: Vec<Value> = super::catalog::match_segments(text, query)
            .into_iter()
            .map(|(t, m)| serde_json::json!({"text": t, "matched": m}))
            .collect();
        if Value::Array(ours.clone()) != s["segments"] {
            failures.push(format!(
                "segments({text:?}, {query:?}):\n  ours     {}\n  upstream {}",
                Value::Array(ours),
                s["segments"]
            ));
        }
    }
    assert!(failures.is_empty(), "{} search mismatches:\n{}", failures.len(), failures.join("\n"));
}
