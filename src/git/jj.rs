//! Jujutsu (upstream `src/utils/jj.ts`). Upstream runs jj for every call;
//! here identical calls within one render run once, and outside any `.jj`
//! workspace jj is not run at all (it would only fail).

use std::cell::RefCell;
use std::collections::HashMap;

use super::exec::Cmd;
use super::paths::{dirname, join, resolve};
use crate::context::RenderContext;

const JJ_TIMEOUT_MS: u64 = 5_000;

type MemoKey = (Option<String>, Vec<String>);

thread_local! {
    static MEMO: RefCell<HashMap<MemoKey, Option<String>>> = RefCell::new(HashMap::new());
    static NO_WORKSPACE: RefCell<HashMap<Option<String>, bool>> = RefCell::new(HashMap::new());
}

fn has_jj_ancestor(start: &str) -> bool {
    let mut current = start.to_string();
    loop {
        if std::fs::symlink_metadata(join(&current, ".jj")).is_ok() {
            return true;
        }
        let parent = dirname(&current);
        if parent == current {
            return false;
        }
        current = parent;
    }
}

/// jj looks for a `.jj` directory from its working directory upwards; with
/// none on either the logical or the physical path it can only fail.
fn surely_no_workspace(cwd: Option<&str>) -> bool {
    let key = cwd.map(String::from);
    if let Some(v) = NO_WORKSPACE.with(|m| m.borrow().get(&key).copied()) {
        return v;
    }
    let dir = cwd.map(resolve).unwrap_or_else(super::paths::process_cwd);
    let v = match std::fs::canonicalize(&dir) {
        Ok(real) => !has_jj_ancestor(&dir) && !has_jj_ancestor(&real.to_string_lossy()),
        // jj can't start in a missing directory either
        Err(_) => !has_jj_ancestor(&dir),
    };
    NO_WORKSPACE.with(|m| m.borrow_mut().insert(key, v));
    v
}

/// `runJjArgs(args, context, allowEmpty)`
pub fn run_jj_args(ctx: &RenderContext, args: &[&str], allow_empty: bool) -> Option<String> {
    let cwd = super::resolve_git_cwd(ctx.data);
    let raw = run_raw(cwd, args)?;
    let out = super::js_trim_end(&raw);
    (allow_empty || !out.is_empty()).then(|| out.to_string())
}

fn run_raw(cwd: Option<&str>, args: &[&str]) -> Option<String> {
    if surely_no_workspace(cwd) {
        return None;
    }
    let key = (cwd.map(String::from), args.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    if let Some(v) = MEMO.with(|m| m.borrow().get(&key).cloned()) {
        return v;
    }
    let cmd = Cmd::new("jj", key.1.clone(), cwd, JJ_TIMEOUT_MS);
    let out = cmd.run().ok();
    MEMO.with(|m| m.borrow_mut().insert(key, out.clone()));
    out
}

/// `isInsideJjRepo(context)`
pub fn is_inside_jj_repo(ctx: &RenderContext) -> bool {
    run_jj_args(ctx, &["root"], false).is_some()
}

/// `getJjChangeCounts(context)`
pub fn change_counts(ctx: &RenderContext) -> (f64, f64) {
    super::parse_diff_stat(&run_jj_args(ctx, &["diff", "--stat"], false).unwrap_or_default())
}
