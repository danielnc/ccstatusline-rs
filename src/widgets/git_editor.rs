//! The editor half of the git and jj widgets (upstream `src/widgets/Git*.ts`, `Jj*.ts`): editor display, hideable
//! states, label prefixes, key bindings and editor actions. Widgets not listed here keep the trait defaults.

use super::editor::{self, NO_GIT, NO_JJ, NO_REMOTE, NO_UPSTREAM};
use super::git::K;
use super::{EditorDisplay, HideableState, Keybind};
use crate::jsobj;
use crate::settings::WidgetItem;
use serde_json::{Map, Value};

/// The types whose editor methods are ported (checked by `editor_meta_tests`).
pub(crate) const EDITOR_PORTED: &[&str] = &[
    "git-branch",
    "git-changes",
    "git-insertions",
    "git-deletions",
    "git-staged-files",
    "git-unstaged-files",
    "git-untracked-files",
    "git-clean-status",
    "git-root-dir",
    "git-review",
    "git-ci-status",
    "git-worktree",
    "git-status",
    "git-staged",
    "git-unstaged",
    "git-untracked",
    "git-ahead-behind",
    "git-conflicts",
    "git-sha",
    "git-origin-owner",
    "git-origin-repo",
    "git-origin-owner-repo",
    "git-upstream-owner",
    "git-upstream-repo",
    "git-upstream-owner-repo",
    "git-is-fork",
    "jj-bookmarks",
    "jj-workspace",
    "jj-root-dir",
    "jj-changes",
    "jj-insertions",
    "jj-deletions",
    "jj-description",
    "jj-revision",
    "worktree-mode",
    "worktree-name",
    "worktree-branch",
    "worktree-original-branch",
];

const LINK_KEY: &str = "linkToRepo";
const LEGACY_LINK_KEY: &str = "linkToGitHub";
const TOGGLE_LINK_ACTION: &str = "toggle-link";

fn link_enabled(item: &WidgetItem) -> bool {
    let meta = |k: &str| item.meta(k);
    meta(LINK_KEY) == Some("true") || (meta(LINK_KEY).is_none() && meta(LEGACY_LINK_KEY) == Some("true"))
}

/// `{...item, metadata: {...rest, [key]: value}}` with `keys` destructured out of the metadata first, or the rest
/// alone (`undefined` when empty) without a value.
fn replace_metadata_keys(item: &Value, keys: &[&str], value: Option<(&str, &str)>) -> Value {
    let mut rest: Map<String, Value> = jsobj::get(item, "metadata")
        .and_then(Value::as_object)
        .map(|m| m.iter().filter(|(k, _)| !keys.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();
    if let Some((k, v)) = value {
        rest.insert(k.into(), Value::String(v.into()));
    }
    jsobj::with(item, "metadata", if rest.is_empty() { jsobj::undefined() } else { Value::Object(rest) })
}

/// GitBranch's `toggleLink`: both link keys dropped from metadata, the canonical one re-added when turning on.
fn toggle_link(item: &Value, typed: &WidgetItem) -> Value {
    let next_enabled = !link_enabled(typed);
    replace_metadata_keys(item, &[LINK_KEY, LEGACY_LINK_KEY], next_enabled.then_some((LINK_KEY, "true")))
}

// --- Git Root Dir (`GitRootDir.ts`) ----------------------------------------------------------------------------

const IDE_LINK_KEY: &str = "linkToIDE";
const LEGACY_CURSOR_LINK_KEY: &str = "linkToCursor";
const IDE_LINK_MODES: &[&str] = &["vscode", "cursor"];

fn ide_link_label(mode: &str) -> &'static str {
    match mode {
        "vscode" => "link-vscode",
        _ => "link-cursor",
    }
}

/// `cycleIdeLinkMode`: none → vscode → cursor → none, dropping the legacy cursor flag.
fn cycle_ide_link_mode(item: &Value, typed: &WidgetItem) -> Value {
    let current = super::git::ide_link_mode(typed).and_then(|m| IDE_LINK_MODES.iter().position(|x| *x == m));
    let next = match current {
        Some(i) if i == IDE_LINK_MODES.len() - 1 => None,
        Some(i) => IDE_LINK_MODES.get(i + 1).copied(),
        None => IDE_LINK_MODES.first().copied(),
    };
    replace_metadata_keys(item, &[IDE_LINK_KEY, LEGACY_CURSOR_LINK_KEY], next.map(|m| (IDE_LINK_KEY, m)))
}

// --- Git Conflicts (`GitConflicts.ts`) -------------------------------------------------------------------------

const ZERO_DISPLAYS: &[&str] = &["count", "clean"];
const DEFAULT_ZERO_DISPLAY: &str = "count";
const ZERO_DISPLAY_METADATA_KEY: &str = "zeroDisplay";
const CYCLE_ZERO_DISPLAY_ACTION: &str = "cycle-zero-display";

fn zero_display(item: &WidgetItem) -> &str {
    match item.meta(ZERO_DISPLAY_METADATA_KEY) {
        Some(v) if ZERO_DISPLAYS.contains(&v) => v,
        _ => DEFAULT_ZERO_DISPLAY,
    }
}

/// `cycleZeroDisplay`: the default is stored as the absence of the key.
fn cycle_zero_display(item: &Value, typed: &WidgetItem) -> Value {
    let current = zero_display(typed);
    let index = ZERO_DISPLAYS.iter().position(|d| *d == current).unwrap_or(0);
    let next = ZERO_DISPLAYS[(index + 1) % ZERO_DISPLAYS.len()];
    if next == DEFAULT_ZERO_DISPLAY {
        return jsobj::remove_metadata_keys(item, &[ZERO_DISPLAY_METADATA_KEY]);
    }
    jsobj::set_metadata(item, ZERO_DISPLAY_METADATA_KEY, next)
}

// --- Git Origin/Upstream remotes (`shared/git-remote.ts`, `GitOriginOwnerRepo.ts`) -----------------------------

const OWNER_ONLY_WHEN_FORK_KEY: &str = "ownerOnlyWhenFork";
const TOGGLE_OWNER_ONLY_ACTION: &str = "toggle-owner-only";

fn is_remote(kind: K) -> bool {
    matches!(
        kind,
        K::OriginOwner | K::OriginRepo | K::OriginOwnerRepo | K::UpstreamOwner | K::UpstreamRepo | K::UpstreamOwnerRepo
    )
}

pub fn display(kind: K, item: &WidgetItem, name: &str) -> EditorDisplay {
    let mut modifiers = Vec::new();
    match kind {
        K::Branch => {
            if link_enabled(item) {
                modifiers.push("repo link".to_string());
            }
            if let Some(m) = editor::max_width_modifier(item) {
                modifiers.push(m);
            }
        }
        K::RootDir => {
            if let Some(mode) = super::git::ide_link_mode(item) {
                modifiers.push(ide_link_label(mode).to_string());
            }
            if let Some(m) = editor::max_width_modifier(item) {
                modifiers.push(m);
            }
        }
        K::Conflicts => {
            let modifier = (zero_display(item) == "clean").then(|| "(clean when zero)".to_string());
            return EditorDisplay { text: name.into(), modifier };
        }
        _ if is_remote(kind) => {
            if item.meta(LINK_KEY) == Some("true") {
                modifiers.push("link".to_string());
            }
            if kind == K::OriginOwnerRepo && item.meta(OWNER_ONLY_WHEN_FORK_KEY) == Some("true") {
                modifiers.push("owner only when fork".to_string());
            }
        }
        _ => return EditorDisplay::plain(name),
    }
    EditorDisplay { text: name.into(), modifier: editor::make_modifier_text(&modifiers) }
}

pub fn hideable_states(kind: K) -> &'static [HideableState] {
    // A literal rather than a `const fn` call, so `&[...]` is promoted to `'static`.
    macro_rules! zero {
        ($label:literal) => {
            HideableState { key: "zero", label: $label, default_enabled: false }
        };
    }
    const NO_PR: HideableState =
        HideableState { key: "no-data", label: "when there is no PR/MR", default_enabled: false };
    const NO_CHECKS: HideableState =
        HideableState { key: "no-data", label: "when there is no check data", default_enabled: false };
    const STATUS: HideableState = HideableState { key: "status", label: "status segment", default_enabled: false };
    const TITLE: HideableState = HideableState { key: "title", label: "title segment", default_enabled: false };
    const NOT_DIVERGED: HideableState =
        HideableState { key: "zero", label: "when not diverged (↑0↓0)", default_enabled: true };
    const NOT_FORK: HideableState =
        HideableState { key: "not-fork", label: "when repo is not a fork", default_enabled: false };
    match kind {
        K::Changes => &[NO_GIT, zero!("when there are no changes")],
        K::Insertions => &[NO_GIT, zero!("when the insertion count is zero")],
        K::Deletions => &[NO_GIT, zero!("when the deletion count is zero")],
        K::StagedFiles => &[NO_GIT, zero!("when the staged file count is zero")],
        K::UnstagedFiles => &[NO_GIT, zero!("when the unstaged file count is zero")],
        K::UntrackedFiles => &[NO_GIT, zero!("when the untracked file count is zero")],
        K::Conflicts => &[NO_GIT, zero!("when there are no conflicts")],
        K::Branch
        | K::CleanStatus
        | K::RootDir
        | K::Worktree
        | K::Status
        | K::Staged
        | K::Unstaged
        | K::Untracked
        | K::Sha => &[NO_GIT],
        K::Review => &[NO_GIT, NO_PR, STATUS, TITLE],
        K::CiStatus => &[NO_GIT, NO_CHECKS],
        K::AheadBehind => &[NO_GIT, NO_UPSTREAM, NOT_DIVERGED],
        K::OriginOwner | K::OriginRepo | K::OriginOwnerRepo => &[NO_REMOTE],
        K::UpstreamOwner | K::UpstreamRepo | K::UpstreamOwnerRepo => &[NO_UPSTREAM],
        K::IsFork => &[NOT_FORK],
        K::JjBookmarks
        | K::JjWorkspace
        | K::JjRootDir
        | K::JjChanges
        | K::JjInsertions
        | K::JjDeletions
        | K::JjDescription
        | K::JjRevision => &[NO_JJ],
        K::WorktreeMode | K::WorktreeName | K::WorktreeBranch | K::WorktreeOriginalBranch => &[],
    }
}

/// `GitFileCountWidget.getLabelPrefix`.
pub fn label_prefix(kind: K) -> Option<String> {
    match kind {
        K::StagedFiles => Some("S:".into()),
        K::UnstagedFiles => Some("M:".into()),
        K::UntrackedFiles => Some("?:".into()),
        _ => None,
    }
}

pub fn keybinds(kind: K, _item: &WidgetItem) -> Vec<Keybind> {
    let link_to_repo = || Keybind::new("l", "(l)ink to repo", TOGGLE_LINK_ACTION);
    match kind {
        K::Branch => vec![link_to_repo(), editor::max_width_keybind(), editor::symbol_keybind()],
        K::RootDir => vec![Keybind::new("l", "(l)ink to IDE", TOGGLE_LINK_ACTION), editor::max_width_keybind()],
        K::Conflicts => {
            vec![Keybind::new("z", "(z)ero conflicts display", CYCLE_ZERO_DISPLAY_ACTION), editor::symbol_keybind()]
        }
        K::OriginOwnerRepo => {
            vec![link_to_repo(), Keybind::new("o", "(o)wner only when fork", TOGGLE_OWNER_ONLY_ACTION)]
        }
        _ if is_remote(kind) => vec![link_to_repo()],
        K::Changes
        | K::Insertions
        | K::Deletions
        | K::CleanStatus
        | K::Worktree
        | K::Status
        | K::Staged
        | K::Unstaged
        | K::Untracked
        | K::AheadBehind
        | K::IsFork
        | K::JjBookmarks
        | K::JjWorkspace
        | K::JjChanges
        | K::JjInsertions
        | K::JjDeletions
        | K::JjRevision
        | K::WorktreeMode => vec![editor::symbol_keybind()],
        _ => Vec::new(),
    }
}

pub fn handle_action(kind: K, action: &str, item: &Value) -> Option<Value> {
    let typed = crate::settings::parse_widget_item(&jsobj::strip_undefined(item)).ok()?;
    match (kind, action) {
        (K::Branch, TOGGLE_LINK_ACTION) => Some(toggle_link(item, &typed)),
        (K::RootDir, TOGGLE_LINK_ACTION) => Some(cycle_ide_link_mode(item, &typed)),
        (K::Conflicts, CYCLE_ZERO_DISPLAY_ACTION) => Some(cycle_zero_display(item, &typed)),
        (K::OriginOwnerRepo, TOGGLE_OWNER_ONLY_ACTION) => {
            Some(jsobj::toggle_metadata_flag(item, OWNER_ONLY_WHEN_FORK_KEY))
        }
        (_, TOGGLE_LINK_ACTION) if is_remote(kind) => Some(jsobj::toggle_metadata_flag(item, LINK_KEY)),
        _ => None,
    }
}
