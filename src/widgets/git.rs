//! Widget group: git, jj and worktree widgets (upstream `src/widgets/Git*.ts`,
//! `Jj*.ts` and `widgets/shared/{git-count-widget,git-remote-widget-base,
//! git-remote,git-status-widget,jj-widget-base}.ts`). Data comes from the
//! `crate::git` provider.

use serde_json::Value;

use super::shared::{build_ide_file_url, encode_git_ref_for_url_path, is_flag, is_hidden, osc8_link, raw_or_labeled};
use super::Widget;
use crate::context::RenderContext;
use crate::git::{self, jj, remote, review};
use crate::jsnum::to_js_string;
use crate::settings::WidgetItem;

#[derive(Clone, Copy, PartialEq)]
enum K {
    Branch,
    Changes,
    Insertions,
    Deletions,
    StagedFiles,
    UnstagedFiles,
    UntrackedFiles,
    CleanStatus,
    RootDir,
    Review,
    CiStatus,
    Worktree,
    Status,
    Staged,
    Unstaged,
    Untracked,
    AheadBehind,
    Conflicts,
    Sha,
    OriginOwner,
    OriginRepo,
    OriginOwnerRepo,
    UpstreamOwner,
    UpstreamRepo,
    UpstreamOwnerRepo,
    IsFork,
    JjBookmarks,
    JjWorkspace,
    JjRootDir,
    JjChanges,
    JjInsertions,
    JjDeletions,
    JjDescription,
    JjRevision,
    WorktreeMode,
    WorktreeName,
    WorktreeBranch,
    WorktreeOriginalBranch,
}

pub struct GitWidget {
    kind: K,
    color: &'static str,
    name: &'static str,
    desc: &'static str,
    category: &'static str,
    raw: bool,
}

impl Widget for GitWidget {
    fn default_color(&self) -> &'static str {
        self.color
    }
    fn display_name(&self) -> &'static str {
        self.name
    }
    fn description(&self) -> &'static str {
        self.desc
    }
    fn category(&self) -> &'static str {
        self.category
    }
    fn supports_raw_value(&self) -> bool {
        self.raw
    }
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        render(self.kind, item, ctx)
    }
}

const fn w(kind: K, color: &'static str, name: &'static str, desc: &'static str, raw: bool) -> GitWidget {
    GitWidget { kind, color, name, desc, category: "Git", raw }
}

const fn j(kind: K, color: &'static str, name: &'static str, desc: &'static str, raw: bool) -> GitWidget {
    GitWidget { kind, color, name, desc, category: "Jujutsu", raw }
}

static WIDGETS: &[(&str, GitWidget)] = &[
    ("git-branch", w(K::Branch, "magenta", "Git Branch", "Shows the current git branch name", true)),
    ("git-changes", w(K::Changes, "yellow", "Git Changes", "Shows git changes count (+insertions, -deletions)", false)),
    ("git-insertions", w(K::Insertions, "green", "Git Insertions", "Shows git insertions count", false)),
    ("git-deletions", w(K::Deletions, "red", "Git Deletions", "Shows git deletions count", false)),
    ("git-staged-files", w(K::StagedFiles, "green", "Git Staged Files", "Shows count of staged files", true)),
    ("git-unstaged-files", w(K::UnstagedFiles, "yellow", "Git Unstaged Files", "Shows count of unstaged files", true)),
    ("git-untracked-files", w(K::UntrackedFiles, "red", "Git Untracked Files", "Shows count of untracked files", true)),
    (
        "git-clean-status",
        w(
            K::CleanStatus,
            "green",
            "Git Clean Status",
            "Shows ✓ when the working tree is clean and ✗ when it is dirty",
            true,
        ),
    ),
    ("git-root-dir", w(K::RootDir, "cyan", "Git Root Dir", "Shows the git repository root directory name", false)),
    (
        "git-review",
        w(
            K::Review,
            "cyan",
            "Git PR/MR",
            "Shows PR/MR info for the current branch (clickable link, status, title)",
            true,
        ),
    ),
    (
        "git-ci-status",
        w(
            K::CiStatus,
            "green",
            "Git CI Status",
            "Shows CI check status for the current branch's PR (GitHub only)",
            true,
        ),
    ),
    ("git-worktree", w(K::Worktree, "blue", "Git Worktree", "Shows the current git worktree name", true)),
    (
        "git-status",
        w(
            K::Status,
            "yellow",
            "Git Status",
            "Shows git status indicators: + staged, * unstaged, ? untracked, ! conflicts",
            false,
        ),
    ),
    ("git-staged", w(K::Staged, "green", "Git Staged", "Shows + when there are staged changes", true)),
    ("git-unstaged", w(K::Unstaged, "yellow", "Git Unstaged", "Shows * when there are unstaged changes", true)),
    ("git-untracked", w(K::Untracked, "red", "Git Untracked", "Shows ? when there are untracked files", true)),
    (
        "git-ahead-behind",
        w(K::AheadBehind, "cyan", "Git Ahead/Behind", "Shows commits ahead/behind upstream (↑2↓3)", true),
    ),
    ("git-conflicts", w(K::Conflicts, "red", "Git Conflicts", "Shows count of merge conflicts", true)),
    ("git-sha", w(K::Sha, "gray", "Git SHA", "Shows short commit hash (SHA)", false)),
    (
        "git-origin-owner",
        w(K::OriginOwner, "cyan", "Git Origin Owner", "Shows the origin remote owner/organization", false),
    ),
    ("git-origin-repo", w(K::OriginRepo, "cyan", "Git Origin Repo", "Shows the origin remote repository name", false)),
    (
        "git-origin-owner-repo",
        w(K::OriginOwnerRepo, "cyan", "Git Origin Owner/Repo", "Shows the origin remote as owner/repo", false),
    ),
    (
        "git-upstream-owner",
        w(K::UpstreamOwner, "magenta", "Git Upstream Owner", "Shows the upstream remote owner/organization", false),
    ),
    (
        "git-upstream-repo",
        w(K::UpstreamRepo, "magenta", "Git Upstream Repo", "Shows the upstream remote repository name", false),
    ),
    (
        "git-upstream-owner-repo",
        w(K::UpstreamOwnerRepo, "magenta", "Git Upstream Owner/Repo", "Shows the upstream remote as owner/repo", false),
    ),
    (
        "git-is-fork",
        w(K::IsFork, "yellow", "Git Is Fork", "Shows fork indicator when repo is a fork of upstream", true),
    ),
    ("jj-bookmarks", j(K::JjBookmarks, "magenta", "JJ Bookmarks", "Shows the current jujutsu bookmark(s)", true)),
    ("jj-workspace", j(K::JjWorkspace, "blue", "JJ Workspace", "Shows the current jujutsu workspace name", true)),
    ("jj-root-dir", j(K::JjRootDir, "cyan", "JJ Root Dir", "Shows the jujutsu repository root directory name", false)),
    (
        "jj-changes",
        j(K::JjChanges, "yellow", "JJ Changes", "Shows jujutsu changes count (+insertions, -deletions)", false),
    ),
    ("jj-insertions", j(K::JjInsertions, "green", "JJ Insertions", "Shows jujutsu insertions count", false)),
    ("jj-deletions", j(K::JjDeletions, "red", "JJ Deletions", "Shows jujutsu deletions count", false)),
    (
        "jj-description",
        j(K::JjDescription, "white", "JJ Description", "Shows the current jujutsu change description", false),
    ),
    ("jj-revision", j(K::JjRevision, "green", "JJ Revision", "Shows the current jujutsu change ID (short)", true)),
    (
        "worktree-mode",
        w(K::WorktreeMode, "yellow", "Git Worktree Mode", "Shows indicator when Claude Code is in worktree mode", true),
    ),
    ("worktree-name", w(K::WorktreeName, "yellow", "Git Worktree Name", "Name of the active worktree", false)),
    (
        "worktree-branch",
        w(K::WorktreeBranch, "yellow", "Git Worktree Branch", "Git branch name for the active worktree", false),
    ),
    (
        "worktree-original-branch",
        w(
            K::WorktreeOriginalBranch,
            "yellow",
            "Git Worktree Original Branch",
            "Git branch checked out before entering the worktree",
            false,
        ),
    ),
];

pub fn lookup(kind: &str) -> Option<&'static dyn Widget> {
    WIDGETS.iter().find(|(k, _)| *k == kind).map(|(_, w)| w as &dyn Widget)
}

/// Detached git-review cache refresh re-entry
/// (`--internal-refresh-git-review-cache`); true when the flag was handled.
pub fn handle_refresh_cli(args: &[String]) -> bool {
    review::handle_refresh_cli(args)
}

// ------------------------------------------------------------- helpers

/// `getSymbol(item, default)`: the character override, or the default.
fn symbol<'a>(item: &'a WidgetItem, default: &'a str) -> &'a str {
    item.character.as_deref().unwrap_or(default)
}

/// `formatSymbolPrefix`: the symbol and a space; nothing for an empty one.
fn symbol_prefix(item: &WidgetItem, default: &str) -> String {
    let s = symbol(item, default);
    if s.is_empty() {
        String::new()
    } else {
        format!("{s} ")
    }
}

/// `getSlotSymbol` for a metadata slot.
fn slot<'a>(item: &'a WidgetItem, id: &str, default: &'a str) -> &'a str {
    item.meta(id).unwrap_or(default)
}

fn max_width_set(item: &WidgetItem) -> bool {
    // `item.maxWidth` truthy
    item.max_width.is_some_and(|m| m != 0.0 && !m.is_nan())
}

/// `applyMaxWidth(text, item.maxWidth)`
fn apply_max_width(text: &str, item: &WidgetItem) -> String {
    match item.max_width {
        Some(m) if m > 0.0 => crate::ansi::truncate_styled_text(text, m.floor().min(usize::MAX as f64) as usize, true),
        _ => text.to_string(),
    }
}

fn no_git(item: &WidgetItem, text: &str) -> Option<String> {
    if is_hidden(item, "no-git", false) {
        None
    } else {
        Some(text.to_string())
    }
}

/// `getRootDirName(rootDir)` (Git Root Dir and JJ Root Dir)
fn root_dir_name(root: &str) -> String {
    let trimmed = root.trim_end_matches(['/', '\\']);
    let normalized = if trimmed.is_empty() { root } else { trimmed };
    match normalized.split(['/', '\\']).rfind(|s| !s.is_empty()) {
        Some(last) => last.to_string(),
        None => normalized.to_string(),
    }
}

pub fn branch_link_enabled(item: &WidgetItem) -> bool {
    is_flag(item, "linkToRepo") || (item.meta("linkToRepo").is_none() && is_flag(item, "linkToGitHub"))
}

fn ide_link_mode(item: &WidgetItem) -> Option<&'static str> {
    match item.meta("linkToIDE") {
        Some("vscode") => return Some("vscode"),
        Some("cursor") => return Some("cursor"),
        _ => {}
    }
    is_flag(item, "linkToCursor").then_some("cursor")
}

fn num(n: f64) -> String {
    to_js_string(n)
}

// ------------------------------------------------------------- render

fn render(k: K, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
    match k {
        K::Branch => branch(item, ctx),
        K::Changes | K::Insertions | K::Deletions => line_counts(k, item, ctx),
        K::StagedFiles | K::UnstagedFiles | K::UntrackedFiles => file_counts(k, item, ctx),
        K::CleanStatus | K::Status | K::Staged | K::Unstaged | K::Untracked | K::Sha => status_widget(k, item, ctx),
        K::RootDir => root_dir(item, ctx),
        K::Review => review_widget(item, ctx),
        K::CiStatus => ci_status(item, ctx),
        K::Worktree => worktree(item, ctx),
        K::AheadBehind => ahead_behind(item, ctx),
        K::Conflicts => conflicts(item, ctx),
        K::OriginOwner
        | K::OriginRepo
        | K::OriginOwnerRepo
        | K::UpstreamOwner
        | K::UpstreamRepo
        | K::UpstreamOwnerRepo => remote_widget(k, item, ctx),
        K::IsFork => is_fork(item, ctx),
        K::JjBookmarks
        | K::JjWorkspace
        | K::JjRootDir
        | K::JjChanges
        | K::JjInsertions
        | K::JjDeletions
        | K::JjDescription
        | K::JjRevision => jj_widget(k, item, ctx),
        K::WorktreeMode => worktree_mode(item, ctx),
        K::WorktreeName => {
            if ctx.is_preview {
                return Some("my-feature".into());
            }
            ctx.data.worktree.as_ref()?.name.clone()
        }
        K::WorktreeBranch => {
            if ctx.is_preview {
                return Some("wt-my-feature".into());
            }
            ctx.data.worktree.as_ref()?.branch.clone()
        }
        K::WorktreeOriginalBranch => {
            if ctx.is_preview {
                return Some("main".into());
            }
            ctx.data.worktree.as_ref()?.original_branch.clone()
        }
    }
}

fn branch(item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
    let link = branch_link_enabled(item);
    let prefix = symbol_prefix(item, "⎇");
    if ctx.is_preview {
        let sample = if max_width_set(item) { "feature/long-branch-name" } else { "main" };
        let text = apply_max_width(&if item.raw() { sample.to_string() } else { format!("{prefix}{sample}") }, item);
        return Some(if link {
            osc8_link(&format!("https://github.com/owner/repo/tree/{sample}"), &text)
        } else {
            text
        });
    }
    if !git::is_inside_work_tree(ctx) {
        return no_git(item, &format!("{prefix}no git"));
    }
    let branch = git::symbolic_ref_head(ctx).map(|r| match r.strip_prefix("refs/heads/") {
        Some(b) => b.to_string(),
        None => r,
    });
    let r = match &branch {
        Some(b) => Some(b.clone()),
        None => git::short_sha(ctx),
    };
    let r = match r {
        Some(r) if !r.is_empty() => r,
        _ => return no_git(item, &format!("{prefix}no git")),
    };
    let value = branch.unwrap_or_else(|| format!("({r})"));
    let display = apply_max_width(&if item.raw() { value } else { format!("{prefix}{value}") }, item);
    if link {
        if let Some(origin) = remote::get_remote_info("origin", ctx) {
            return Some(osc8_link(&remote::branch_web_url(&origin, &encode_git_ref_for_url_path(&r)), &display));
        }
    }
    Some(display)
}

/// GitCountWidgetBase: Changes, Insertions, Deletions
fn line_counts(k: K, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
    let format = |(ins, del): (f64, f64)| -> String {
        let (si, sd) = (slot(item, "symbolInsertions", "+"), slot(item, "symbolDeletions", "-"));
        match k {
            K::Changes => format!("({si}{},{sd}{})", num(ins), num(del)),
            K::Insertions => format!("{si}{}", num(ins)),
            _ => format!("{sd}{}", num(del)),
        }
    };
    if ctx.is_preview {
        return Some(format((42.0, 10.0)));
    }
    if !git::is_inside_work_tree(ctx) {
        return no_git(item, "(no git)");
    }
    let counts = git::change_counts(ctx);
    let zero = match k {
        K::Changes => counts.0 == 0.0 && counts.1 == 0.0,
        K::Insertions => counts.0 == 0.0,
        _ => counts.1 == 0.0,
    };
    if zero && is_hidden(item, "zero", false) {
        return None;
    }
    Some(format(counts))
}

/// GitFileCountWidget: Staged, Unstaged, Untracked Files
fn file_counts(k: K, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
    let (label, preview) = match k {
        K::StagedFiles => ("S:", 3),
        K::UnstagedFiles => ("M:", 2),
        _ => ("?:", 1),
    };
    if ctx.is_preview {
        return Some(raw_or_labeled(item, label, &preview.to_string()));
    }
    if !git::is_inside_work_tree(ctx) {
        return no_git(item, "(no git)");
    }
    let (s, u, t) = git::file_status_counts(ctx);
    let count = match k {
        K::StagedFiles => s,
        K::UnstagedFiles => u,
        _ => t,
    };
    if count == 0 && is_hidden(item, "zero", false) {
        return None;
    }
    Some(raw_or_labeled(item, label, &count.to_string()))
}

/// GitStatusWidgetBase: Clean Status, Status, Staged, Unstaged, Untracked, SHA
fn status_widget(k: K, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
    let flag_symbol = match k {
        K::Staged => "+",
        K::Unstaged => "*",
        _ => "?",
    };
    let format_flag = || if item.raw() { "true".to_string() } else { symbol(item, flag_symbol).to_string() };
    let format_status = |st: git::GitStatus| {
        let mut s = String::new();
        if st.conflicts {
            s.push_str(slot(item, "symbolConflicts", "!"));
        }
        if st.staged {
            s.push_str(slot(item, "symbolStaged", "+"));
        }
        if st.unstaged {
            s.push_str(slot(item, "symbolUnstaged", "*"));
        }
        if st.untracked {
            s.push_str(slot(item, "symbolUntracked", "?"));
        }
        s
    };
    if ctx.is_preview {
        return Some(match k {
            K::CleanStatus => {
                if item.raw() {
                    "clean".into()
                } else {
                    slot(item, "symbolClean", "✓").into()
                }
            }
            K::Status => {
                format_status(git::GitStatus { staged: true, unstaged: true, untracked: false, conflicts: false })
            }
            K::Sha => "a1b2c3d".into(),
            _ => format_flag(),
        });
    }
    if !git::is_inside_work_tree(ctx) {
        return no_git(item, "(no git)");
    }
    match k {
        K::CleanStatus => {
            let st = git::git_status(ctx);
            let clean = !st.staged && !st.unstaged && !st.untracked && !st.conflicts;
            Some(match (item.raw(), clean) {
                (true, true) => "clean".into(),
                (true, false) => "dirty".into(),
                (false, true) => slot(item, "symbolClean", "✓").into(),
                (false, false) => slot(item, "symbolDirty", "✗").into(),
            })
        }
        K::Status => {
            let st = git::git_status(ctx);
            if !st.staged && !st.unstaged && !st.untracked && !st.conflicts {
                return None;
            }
            Some(format_status(st))
        }
        K::Sha => match git::short_sha(ctx) {
            Some(s) => Some(s),
            None => no_git(item, "(no commit)"),
        },
        _ => {
            let st = git::git_status(ctx);
            let on = match k {
                K::Staged => st.staged,
                K::Unstaged => st.unstaged,
                _ => st.untracked,
            };
            on.then(format_flag)
        }
    }
}

fn root_dir(item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
    let mode = ide_link_mode(item);
    if ctx.is_preview {
        let name = apply_max_width(if max_width_set(item) { "my-long-repository-name" } else { "my-repo" }, item);
        return Some(match mode {
            Some(m) => osc8_link(&build_ide_file_url("/Users/example/my-repo", m), &name),
            None => name,
        });
    }
    if !git::is_inside_work_tree(ctx) {
        return no_git(item, "no git");
    }
    let Some(root) = git::run_git(ctx, git::K_TOPLEVEL) else { return no_git(item, "no git") };
    let name = apply_max_width(&root_dir_name(&root), item);
    Some(match mode {
        Some(m) => osc8_link(&build_ide_file_url(&root, m), &name),
        None => name,
    })
}

fn worktree(item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
    let prefix = symbol_prefix(item, "𖠰");
    let fmt = |v: &str| if item.raw() { v.to_string() } else { format!("{prefix}{v}") };
    if ctx.is_preview {
        return Some(fmt("main"));
    }
    if !git::is_inside_work_tree(ctx) {
        return no_git(item, &format!("{prefix}no git"));
    }
    let output = git::run_git(ctx, git::K_WORKTREE_DIRS).unwrap_or_default();
    let dirs: Vec<String> = output.split('\n').map(|d| crate::jsnum::js_trim(d).replace('\\', "/")).collect();
    let git_dir = dirs.first().map(String::as_str).unwrap_or("");
    let name = if git_dir.is_empty() {
        None
    } else {
        // `${commonDir}/worktrees/` with commonDir possibly undefined
        let common = dirs.get(1).map(String::as_str);
        let linked = format!("{}/worktrees/", common.unwrap_or("undefined"));
        match common {
            Some(c) if !c.is_empty() && git_dir.starts_with(&linked) => {
                let wt = &git_dir[linked.len()..];
                (!wt.is_empty()).then(|| wt.to_string())
            }
            _ => Some("main".to_string()),
        }
    };
    match name {
        Some(n) => Some(fmt(&n)),
        None => no_git(item, &format!("{prefix}no git")),
    }
}

fn ahead_behind(item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
    let (a, b) = (slot(item, "symbolAhead", "↑"), slot(item, "symbolBehind", "↓"));
    if ctx.is_preview {
        return Some(if item.raw() { "2,3".into() } else { format!("{a}2{b}3") });
    }
    if !git::is_inside_work_tree(ctx) {
        return no_git(item, "(no git)");
    }
    let Some((ahead, behind)) = git::ahead_behind(ctx) else {
        return if is_hidden(item, "no-upstream", false) { None } else { Some("(no upstream)".into()) };
    };
    if ahead == 0.0 && behind == 0.0 {
        if is_hidden(item, "zero", true) {
            return None;
        }
        return Some(if item.raw() { "0,0".into() } else { format!("{a}0{b}0") });
    }
    if item.raw() {
        return Some(format!("{},{}", num(ahead), num(behind)));
    }
    let mut s = String::new();
    if ahead > 0.0 {
        s.push_str(&format!("{a}{}", num(ahead)));
    }
    if behind > 0.0 {
        s.push_str(&format!("{b}{}", num(behind)));
    }
    Some(s)
}

fn conflicts(item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
    let sym = symbol(item, "⚠");
    if ctx.is_preview {
        return Some(if item.raw() { "2".into() } else { format!("{sym}2") });
    }
    if !git::is_inside_work_tree(ctx) {
        return no_git(item, "(no git)");
    }
    let count = git::conflict_count(ctx);
    if count == 0 {
        if is_hidden(item, "zero", false) {
            return None;
        }
        if item.meta("zeroDisplay") == Some("clean") && !item.raw() {
            return Some(slot(item, "symbolClean", "✓").to_string());
        }
    }
    Some(if item.raw() { count.to_string() } else { format!("{sym}{count}") })
}

fn remote_widget(k: K, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
    let link = is_flag(item, "linkToRepo");
    let owner_only = is_flag(item, "ownerOnlyWhenFork");
    if ctx.is_preview {
        let (text, url) = match k {
            K::OriginOwner => ("owner", "https://github.com/owner/repo"),
            K::OriginRepo => ("repo", "https://github.com/owner/repo"),
            K::OriginOwnerRepo => (if owner_only { "owner" } else { "owner/repo" }, "https://github.com/owner/repo"),
            K::UpstreamOwner => ("upstream-owner", "https://github.com/upstream-owner/repo"),
            K::UpstreamRepo => ("upstream-repo", "https://github.com/upstream-owner/upstream-repo"),
            _ => ("upstream-owner/upstream-repo", "https://github.com/upstream-owner/upstream-repo"),
        };
        return Some(if link { osc8_link(url, text) } else { text.to_string() });
    }
    let origin = matches!(k, K::OriginOwner | K::OriginRepo | K::OriginOwnerRepo);
    let found = if origin { remote::get_remote_info("origin", ctx) } else { remote::get_upstream_remote_info(ctx) };
    let Some(r) = found else {
        let (key, placeholder) = if origin { ("no-remote", "no remote") } else { ("no-upstream", "no upstream") };
        return if is_hidden(item, key, false) { None } else { Some(placeholder.into()) };
    };
    let text = match k {
        K::OriginOwner | K::UpstreamOwner => r.owner.clone(),
        K::OriginRepo | K::UpstreamRepo => r.repo.clone(),
        K::OriginOwnerRepo if owner_only && remote::is_fork(ctx) => r.owner.clone(),
        _ => format!("{}/{}", r.owner, r.repo),
    };
    Some(if link { osc8_link(&remote::repo_web_url(&r), &text) } else { text })
}

fn is_fork(item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
    let sym = symbol(item, "⑂");
    if ctx.is_preview || remote::is_fork(ctx) {
        return Some(if item.raw() { "true".into() } else { sym.to_string() });
    }
    if is_hidden(item, "not-fork", false) {
        return None;
    }
    Some(if item.raw() { "false".into() } else { "isFork: false".into() })
}

fn worktree_mode(item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
    let in_worktree = ctx.is_preview || ctx.data.worktree.is_some();
    if item.raw() {
        return Some(if in_worktree { "true" } else { "false" }.into());
    }
    if !in_worktree {
        return None;
    }
    let s = symbol(item, "⎇");
    (!s.is_empty()).then(|| s.to_string())
}

// ------------------------------------------------------------ git-review

/// What a template literal makes of a JS value (`undefined` for a missing field).
fn js_str(v: Option<&Value>) -> String {
    match v {
        None => "undefined".into(),
        Some(Value::Null) => "null".into(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => to_js_string(n.as_f64().unwrap_or(f64::NAN)),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) => {
            a.iter().map(|x| if x.is_null() { String::new() } else { js_str(Some(x)) }).collect::<Vec<_>>().join(",")
        }
        Some(Value::Object(_)) => "[object Object]".into(),
    }
}

fn js_to_number(v: Option<&Value>) -> f64 {
    match v {
        None => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(s)) => {
            let t = crate::jsnum::js_trim(s);
            if t.is_empty() {
                0.0
            } else {
                crate::jsnum::number_from_str(t).unwrap_or(f64::NAN)
            }
        }
        Some(Value::Array(_)) => js_to_number(Some(&Value::String(js_str(v)))),
        Some(Value::Object(_)) => f64::NAN,
    }
}

fn js_truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// Reading `.length` of undefined/null throws in upstream, which ends the
/// process (unhandled rejection): nothing on stdout, exit code 1.
fn js_type_error() -> ! {
    eprintln!("TypeError: Cannot read properties of undefined (reading 'length')");
    std::process::exit(1)
}

fn pr_noun(pr: Option<&Value>, ctx: &RenderContext) -> &'static str {
    let provider = pr.and_then(|p| p.get("provider"));
    match provider {
        Some(Value::String(s)) if s == "glab" => return "MR",
        Some(Value::String(s)) if s == "gh" => return "PR",
        _ => {}
    }
    match pr {
        Some(p) => {
            let url = p.get("url").and_then(Value::as_str).unwrap_or("").to_lowercase();
            if url.contains("/-/merge_requests/") || url.contains("gitlab") {
                return "MR";
            }
        }
        None => {
            if remote::get_remote_info("origin", ctx).is_some_and(|o| o.host.to_lowercase().contains("gitlab")) {
                return "MR";
            }
        }
    }
    "PR"
}

fn review_display(item: &WidgetItem, pr: &Value, show_status: bool, show_title: bool, noun: &str) -> String {
    let number = js_str(pr.get("number"));
    let link_text = if item.raw() { format!("#{number}") } else { format!("{noun} #{number}") };
    let url = pr.get("url").and_then(Value::as_str).unwrap_or("");
    let mut parts = vec![osc8_link(url, &link_text)];
    if show_status {
        match review::status_label(pr.get("state"), pr.get("reviewDecision")) {
            None | Some(Value::Null) => js_type_error(),
            Some(Value::String(s)) => {
                if !s.is_empty() {
                    parts.push(s);
                }
            }
            Some(a @ Value::Array(_)) => {
                if !a.as_array().unwrap().is_empty() {
                    parts.push(js_str(Some(&a)));
                }
            }
            Some(_) => {}
        }
    }
    if show_title {
        match pr.get("title") {
            None | Some(Value::Null) => js_type_error(),
            Some(Value::String(t)) => {
                if !t.is_empty() {
                    parts.push(review::truncate_title(t));
                }
            }
            Some(_) => {}
        }
    }
    parts.join(" ")
}

fn review_cwd(ctx: &RenderContext) -> String {
    git::resolve_git_cwd(ctx.data).map(String::from).unwrap_or_else(git::paths::process_cwd)
}

fn review_widget(item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
    let show_status = !is_hidden(item, "status", false);
    let show_title = !is_hidden(item, "title", false);
    if ctx.is_preview {
        let preview: Value = serde_json::json!({
            "number": 42, "url": "https://github.com/owner/repo/pull/42", "title": "Example PR title",
            "state": "OPEN", "reviewDecision": ""
        });
        return Some(review_display(item, &preview, show_status, show_title, "PR"));
    }
    if !git::is_inside_work_tree(ctx) {
        return if is_hidden(item, "no-git", false) { None } else { Some(format!("(no {})", pr_noun(None, ctx))) };
    }
    let cwd = review_cwd(ctx);
    let git_dir = git::known_git_dir(ctx);
    let Some(pr) = review::cached_review_data(&cwd, ctx.has_widget("git-ci-status"), git_dir.as_deref()) else {
        return if is_hidden(item, "no-data", false) { None } else { Some(format!("(no {})", pr_noun(None, ctx))) };
    };
    Some(review_display(item, &pr, show_status, show_title, pr_noun(Some(&pr), ctx)))
}

fn ci_display(checks: &Value, raw: bool) -> String {
    if raw {
        return match checks.get("state") {
            None | Some(Value::Null) => String::new(),
            v => js_str(v),
        };
    }
    let mut parts = Vec::new();
    for (key, sym) in [("failing", "✗"), ("pending", "●"), ("success", "✓")] {
        let v = checks.get(key);
        if js_to_number(v) > 0.0 {
            parts.push(format!("{sym}{}", js_str(v)));
        }
    }
    if !parts.is_empty() {
        return parts.join(" ");
    }
    let state = js_str(checks.get("state"));
    let sym = match state.as_str() {
        "success" | "passing" => "✓",
        "failing" => "✗",
        "pending" => "●",
        _ => "undefined",
    };
    format!("{sym}0")
}

fn ci_status(item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
    let raw = item.raw();
    if ctx.is_preview {
        let preview = serde_json::json!({ "state": "failing", "failing": 1, "pending": 1, "success": 5 });
        return Some(ci_display(&preview, raw));
    }
    if !git::is_inside_work_tree(ctx) {
        return no_git(item, "(no git)");
    }
    let cwd = review_cwd(ctx);
    let git_dir = git::known_git_dir(ctx);
    let data = review::cached_review_data(&cwd, true, git_dir.as_deref());
    let checks = data.as_ref().and_then(|d| d.get("checks"));
    if !js_truthy(checks) {
        return if is_hidden(item, "no-data", false) { None } else { Some("-".into()) };
    }
    Some(ci_display(checks.unwrap(), raw))
}

// ------------------------------------------------------------------ jj

fn jj_widget(k: K, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
    let glyph = match k {
        K::JjBookmarks => Some("🔖"),
        K::JjWorkspace => Some("◆"),
        K::JjRevision => Some("\u{f1fa}"),
        _ => None,
    };
    let no_jj_text = match k {
        K::JjChanges | K::JjInsertions | K::JjDeletions => "(no jj)",
        _ => "no jj",
    };
    let placeholder = |text: &str| match glyph {
        Some(g) => format!("{}{text}", symbol_prefix(item, g)),
        None => text.to_string(),
    };
    let labeled = |v: &str| raw_or_labeled(item, &symbol_prefix(item, glyph.unwrap_or("")), v);
    let changes = |(ins, del): (f64, f64)| match k {
        K::JjChanges => format!(
            "({}{},{}{})",
            slot(item, "symbolInsertions", "+"),
            num(ins),
            slot(item, "symbolDeletions", "-"),
            num(del)
        ),
        K::JjInsertions => format!("{}{}", slot(item, "symbolInsertions", "+"), num(ins)),
        _ => format!("{}{}", slot(item, "symbolDeletions", "-"), num(del)),
    };
    if ctx.is_preview {
        return Some(match k {
            K::JjBookmarks => labeled("main"),
            K::JjWorkspace => labeled("default"),
            K::JjRevision => labeled("kkmpptxz"),
            K::JjRootDir => root_dir_name("my-repo"),
            K::JjDescription => "(no description)".into(),
            _ => changes((42.0, 10.0)),
        });
    }
    let hide = is_hidden(item, "no-jj", false);
    if !jj::is_inside_jj_repo(ctx) {
        return if hide { None } else { Some(placeholder(no_jj_text)) };
    }
    let value: Option<String> =
        match k {
            K::JjBookmarks => jj::run_jj_args(
                ctx,
                &["log", "--no-graph", "-r", "heads(::@ & bookmarks())", "--template", r#"bookmarks ++ "\n""#],
                false,
            )
            .and_then(|out| {
                let marks: Vec<&str> = git::js_split_ws(&out).into_iter().filter(|s| !s.is_empty()).collect();
                (!marks.is_empty()).then(|| labeled(&marks.join(", ")))
            }),
            K::JjWorkspace => jj::run_jj_args(
                ctx,
                &["workspace", "list", "--template", "if(target.current_working_copy(), name ++ \"\n\")"],
                false,
            )
            .and_then(|out| {
                out.split('\n')
                    .map(|l| crate::jsnum::js_trim(l.strip_suffix('\r').unwrap_or(l)))
                    .find(|l| !l.is_empty())
                    .map(labeled)
            }),
            K::JjRootDir => jj::run_jj_args(ctx, &["root"], false).map(|r| root_dir_name(&r)),
            K::JjDescription => {
                jj::run_jj_args(ctx, &["log", "--no-graph", "-r", "@", "-T", "description.first_line()"], true)
                    .map(|d| if d.is_empty() { "(no description)".into() } else { d })
            }
            K::JjRevision => {
                jj::run_jj_args(ctx, &["log", "--no-graph", "-r", "@", "-T", "change_id.shortest()"], false)
                    .map(|c| labeled(&c))
            }
            K::JjChanges => {
                let stat = jj::run_jj_args(ctx, &["diff", "--stat"], false);
                let (mut ins, mut del) = (0.0, 0.0);
                if let Some(stat) = stat {
                    let summary = stat.rsplit('\n').next().unwrap_or("");
                    if !summary.is_empty() {
                        ins = first_count(summary, " insertion");
                        del = first_count(summary, " deletion");
                    }
                }
                Some(changes((ins, del)))
            }
            _ => Some(changes(jj::change_counts(ctx))),
        };
    match value {
        Some(v) => Some(v),
        None => {
            if hide {
                None
            } else {
                Some(placeholder(if k == K::JjBookmarks { "(none)" } else { no_jj_text }))
            }
        }
    }
}

/// `/(\d+)<suffix>/` (suffix begins with a single space), or 0
fn first_count(s: &str, suffix: &str) -> f64 {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_digit() {
            let start = i;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            if s[i..].starts_with(suffix) {
                return crate::jsnum::parse_int(&s[start..i]).unwrap_or(0.0);
            }
        } else {
            i += 1;
        }
    }
    0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_names() {
        assert_eq!(root_dir_name("/a/b/repo"), "repo");
        assert_eq!(root_dir_name("/a/b/repo//"), "repo");
        assert_eq!(root_dir_name("/"), "/");
        assert_eq!(root_dir_name("C:\\x\\y"), "y");
    }

    #[test]
    fn counts() {
        assert_eq!(first_count("3 files changed, 12 insertions(+), 4 deletions(-)", " insertion"), 12.0);
        assert_eq!(first_count("nothing", " deletion"), 0.0);
    }
}
