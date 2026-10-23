//! The editor half of the git and jj widgets (upstream `src/widgets/Git*.ts`, `Jj*.ts`): editor display, hideable
//! states, label prefixes, key bindings and editor actions. Widgets not listed here keep the trait defaults.

use super::editor::{self, NO_GIT};
use super::git::K;
use super::{EditorDisplay, HideableState, Keybind};
use crate::jsobj;
use crate::settings::WidgetItem;
use serde_json::{Map, Value};

/// The types whose editor methods are ported (checked by `editor_meta_tests`).
pub(crate) const EDITOR_PORTED: &[&str] = &["git-branch", "git-changes"];

const LINK_KEY: &str = "linkToRepo";
const LEGACY_LINK_KEY: &str = "linkToGitHub";
const TOGGLE_LINK_ACTION: &str = "toggle-link";

fn link_enabled(item: &WidgetItem) -> bool {
    let meta = |k: &str| item.meta(k);
    meta(LINK_KEY) == Some("true") || (meta(LINK_KEY).is_none() && meta(LEGACY_LINK_KEY) == Some("true"))
}

/// GitBranch's `toggleLink`: both link keys dropped from metadata, the canonical one re-added when turning on.
fn toggle_link(item: &Value, typed: &WidgetItem) -> Value {
    let next_enabled = !link_enabled(typed);
    let mut rest: Map<String, Value> = jsobj::get(item, "metadata")
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .filter(|(k, _)| *k != LINK_KEY && *k != LEGACY_LINK_KEY)
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        })
        .unwrap_or_default();
    if next_enabled {
        rest.insert(LINK_KEY.into(), Value::String("true".into()));
    }
    jsobj::with(item, "metadata", if rest.is_empty() { jsobj::undefined() } else { Value::Object(rest) })
}

pub fn display(kind: K, item: &WidgetItem, name: &str) -> EditorDisplay {
    match kind {
        K::Branch => {
            let mut modifiers = Vec::new();
            if link_enabled(item) {
                modifiers.push("repo link".to_string());
            }
            if let Some(m) = editor::max_width_modifier(item) {
                modifiers.push(m);
            }
            EditorDisplay { text: name.into(), modifier: editor::make_modifier_text(&modifiers) }
        }
        _ => EditorDisplay::plain(name),
    }
}

pub fn hideable_states(kind: K) -> &'static [HideableState] {
    const ZERO_CHANGES: HideableState =
        HideableState { key: "zero", label: "when there are no changes", default_enabled: false };
    match kind {
        K::Branch => &[NO_GIT],
        K::Changes => &[NO_GIT, ZERO_CHANGES],
        _ => &[],
    }
}

pub fn keybinds(kind: K, _item: &WidgetItem) -> Vec<Keybind> {
    match kind {
        K::Branch => vec![
            Keybind::new("l", "(l)ink to repo", TOGGLE_LINK_ACTION),
            editor::max_width_keybind(),
            editor::symbol_keybind(),
        ],
        K::Changes => vec![editor::symbol_keybind()],
        _ => Vec::new(),
    }
}

pub fn handle_action(kind: K, action: &str, item: &Value) -> Option<Value> {
    let typed = crate::settings::parse_widget_item(&jsobj::strip_undefined(item)).ok()?;
    match (kind, action) {
        (K::Branch, TOGGLE_LINK_ACTION) => Some(toggle_link(item, &typed)),
        _ => None,
    }
}
