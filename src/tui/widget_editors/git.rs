//! The git, jj and worktree widgets' editors. Every one is shared: the max-width editor, the glyph editor
//! (`renderSymbolOverrideEditor`) or the symbol slots editor (`renderSymbolSlotsEditor`). git-review, git-ci-status
//! and the remote widgets have no `renderEditor` upstream.

use super::shared::{self, Slot};
use super::WidgetEditor;
use crate::widgets::editor::MAX_WIDTH_ACTION;
use serde_json::Value;

const INSERTIONS_SLOT: Slot = Slot { id: "symbolInsertions", label: "Insertions", default_symbol: "+" };
const DELETIONS_SLOT: Slot = Slot { id: "symbolDeletions", label: "Deletions", default_symbol: "-" };
const AHEAD_SLOT: Slot = Slot { id: "symbolAhead", label: "Ahead", default_symbol: "↑" };
const BEHIND_SLOT: Slot = Slot { id: "symbolBehind", label: "Behind", default_symbol: "↓" };
const CLEAN_SLOT: Slot = Slot { id: "symbolClean", label: "Clean", default_symbol: "✓" };
const DIRTY_SLOT: Slot = Slot { id: "symbolDirty", label: "Dirty", default_symbol: "✗" };
const CONFLICT_SLOT: Slot = Slot { id: "character", label: "Conflicts", default_symbol: "⚠" };
const CONFLICTS_SLOT: Slot = Slot { id: "symbolConflicts", label: "Conflicts", default_symbol: "!" };
const STAGED_SLOT: Slot = Slot { id: "symbolStaged", label: "Staged", default_symbol: "+" };
const UNSTAGED_SLOT: Slot = Slot { id: "symbolUnstaged", label: "Unstaged", default_symbol: "*" };
const UNTRACKED_SLOT: Slot = Slot { id: "symbolUntracked", label: "Untracked", default_symbol: "?" };

/// `renderEditor` for the group's types; None for a type without one (or git-root-dir's other actions).
pub fn open(kind: &str, action: &str, item: &Value) -> Option<Box<dyn WidgetEditor>> {
    let glyph = |default_symbol| Some(shared::symbol_override(item, default_symbol));
    let slots = |slots: &[Slot]| Some(shared::symbol_slots(item, slots.to_vec()));
    match kind {
        "git-branch" | "git-root-dir" if action == MAX_WIDTH_ACTION => {
            Some(Box::new(shared::MaxWidthEditor::new(item)))
        }
        "git-branch" => glyph("⎇"),
        "git-root-dir" => None,
        "git-is-fork" => glyph("⑂"),
        "git-worktree" => glyph("𖠰"),
        "worktree-mode" => glyph("⎇"),
        "git-staged" => glyph("+"),
        "git-unstaged" => glyph("*"),
        "git-untracked" => glyph("?"),
        "jj-bookmarks" => glyph("🔖"),
        "jj-workspace" => glyph("◆"),
        "jj-revision" => glyph("\u{f1fa}"),
        "git-changes" | "jj-changes" => slots(&[INSERTIONS_SLOT, DELETIONS_SLOT]),
        "git-insertions" | "jj-insertions" => slots(&[INSERTIONS_SLOT]),
        "git-deletions" | "jj-deletions" => slots(&[DELETIONS_SLOT]),
        "git-ahead-behind" => slots(&[AHEAD_SLOT, BEHIND_SLOT]),
        "git-clean-status" => slots(&[CLEAN_SLOT, DIRTY_SLOT]),
        "git-conflicts" => slots(&[CONFLICT_SLOT, CLEAN_SLOT]),
        "git-status" => slots(&[CONFLICTS_SLOT, STAGED_SLOT, UNSTAGED_SLOT, UNTRACKED_SLOT]),
        _ => None,
    }
}
