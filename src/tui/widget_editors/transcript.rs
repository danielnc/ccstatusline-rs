//! The transcript group's editors: the speed widgets' window (shared), Skills' list limit (`Skills.tsx`), and the
//! glyph slots of Cache Timer (`CacheTimer.ts`) and Compaction Counter (`CompactionCounter.ts`).

use super::shared::{self, Slot};
use super::WidgetEditor;
use crate::jsobj;
use crate::tui::components::hide_label::Done;
use crate::tui::ink::keys::Input;
use crate::tui::ink::layout::*;
use crate::tui::text_cursor::should_insert_input;
use serde_json::Value;

const EDIT_LIST_LIMIT_ACTION: &str = "edit-list-limit";

/// The editor for a transcript widget type and action; None for types outside the group or actions without one.
pub fn open(kind: &str, action: &str, item: &Value) -> Option<Box<dyn WidgetEditor>> {
    match (kind, action) {
        ("input-speed" | "output-speed" | "total-speed", "edit-window") => {
            Some(Box::new(shared::SpeedWindowEditor::new(item)))
        }
        ("skills", EDIT_LIST_LIMIT_ACTION) => Some(Box::new(SkillsEditor::new(item))),
        ("cache-timer", _) => Some(shared::symbol_slots(
            item,
            vec![
                Slot { id: "symbolHot", label: "Working", default_symbol: "🔥" },
                Slot { id: "symbolFresh", label: "Fresh", default_symbol: "🟢" },
                Slot { id: "symbolDraining", label: "Draining", default_symbol: "🟡" },
                Slot { id: "symbolUrgent", label: "Urgent", default_symbol: "🔴" },
                Slot { id: "symbolCold", label: "Cold", default_symbol: "❄️" },
            ],
        )),
        ("compaction-counter", _) => Some(shared::symbol_slots(
            item,
            vec![Slot { id: "symbolReclaimed", label: "Reclaimed", default_symbol: "↓" }],
        )),
        _ => None,
    }
}

/// `parseListLimit`.
fn parse_list_limit(item: &Value) -> f64 {
    match crate::jsnum::parse_int(jsobj::metadata_str(item, "listLimit").unwrap_or("0")) {
        Some(p) if p >= 0.0 => p,
        _ => 0.0,
    }
}

/// `setListLimit`: no limit is stored as no key.
fn set_list_limit(item: &Value, limit: f64) -> Value {
    if limit <= 0.0 {
        return jsobj::remove_metadata_keys(item, &["listLimit"]);
    }
    jsobj::set_metadata(item, "listLimit", &crate::jsnum::to_js_string(limit))
}

/// `SkillsEditor`.
struct SkillsEditor {
    widget: Value,
    input: String,
}

impl SkillsEditor {
    fn new(widget: &Value) -> Self {
        SkillsEditor { widget: widget.clone(), input: crate::jsnum::to_js_string(parse_list_limit(widget)) }
    }
}

impl WidgetEditor for SkillsEditor {
    fn handle(&mut self, ev: &Input) -> Done {
        let k = &ev.key;
        if k.return_ {
            let limit = match crate::jsnum::parse_int(&self.input) {
                Some(p) if p >= 0.0 => p,
                _ => 0.0,
            };
            return Done::Complete(set_list_limit(&self.widget, limit));
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
                plain("Enter max skills to show (0 for unlimited): "),
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
