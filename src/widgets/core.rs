//! Core widgets owned by the lead (reference implementations for the widget
//! groups): model.

use super::shared::raw_or_labeled;
use super::Widget;
use crate::context::RenderContext;
use crate::settings::WidgetItem;
use crate::status::Model;

/// The types whose editor methods are ported (checked by `editor_meta_tests`).
pub(crate) const EDITOR_PORTED: &[&str] = &["model"];

pub struct ModelWidget;

impl Widget for ModelWidget {
    fn default_color(&self) -> &'static str {
        "cyan"
    }
    fn display_name(&self) -> &'static str {
        "Model"
    }
    fn description(&self) -> &'static str {
        "Displays the Claude model name (e.g., Claude 3.5 Sonnet)"
    }
    fn category(&self) -> &'static str {
        "Core"
    }
    fn label_prefix(&self, _item: &WidgetItem) -> Option<String> {
        Some("Model: ".into())
    }
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        if ctx.is_preview {
            return Some(raw_or_labeled(item, "Model: ", "Claude"));
        }
        let name = match &ctx.data.model {
            Model::Str(s) => Some(s.as_str()),
            Model::Obj { id, display_name } => display_name.as_deref().or(id.as_deref()),
            Model::None => None,
        }?;
        if name.is_empty() {
            return None;
        }
        // .replace(/\s*\(.*\)$/, '')
        let short = strip_paren_suffix(name);
        Some(raw_or_labeled(item, "Model: ", &short))
    }
}

/// `s.replace(/\s*\(.*\)$/, '')`: drop the first "(" that has a ")" at the very
/// end of the string (`.` excludes line terminators), with preceding whitespace.
fn strip_paren_suffix(s: &str) -> String {
    if !s.ends_with(')') {
        return s.to_string();
    }
    for (i, c) in s.char_indices() {
        if c == '(' {
            let inner = &s[i + 1..s.len() - 1];
            if inner.contains(['\n', '\r', '\u{2028}', '\u{2029}']) {
                continue;
            }
            let head = s[..i].trim_end_matches(crate::gradient::is_js_space);
            return head.to_string();
        }
    }
    s.to_string()
}

static MODEL: ModelWidget = ModelWidget;

pub fn lookup(kind: &str) -> Option<&'static dyn Widget> {
    match kind {
        "model" => Some(&MODEL),
        _ => None,
    }
}
