//! Keeping git from running what a repository's own config asks for
//! (upstream `src/utils/git-hardening.ts`).

use super::paths::resolve;

pub const HARDENING_ARGS: [&str; 4] = ["-c", "core.fsmonitor=", "-c", "safe.bareRepository=explicit"];

pub struct FilterEntry {
    pub scope: String,
    pub name: String,
    pub key: String,
    pub value: String,
}

/// `git config -z --get-regexp` output: `<key>\n<value>\0` per entry, each
/// preceded by `<scope>\0` with --show-scope.
pub fn parse_filter_config(output: &str, scoped: bool) -> Vec<FilterEntry> {
    let fields: Vec<&str> = output.split('\0').collect();
    let step = if scoped { 2 } else { 1 };
    let mut entries = Vec::new();
    let mut i = 0;
    while i + step - 1 < fields.len() {
        let scope = if scoped { fields[i] } else { "local" };
        let kv = fields[i + step - 1];
        i += step;
        if scope.is_empty() || kv.is_empty() {
            continue;
        }
        let (full_key, value) = match kv.find('\n') {
            Some(n) => (&kv[..n], &kv[n + 1..]),
            None => (kv, ""),
        };
        let Some(last_dot) = full_key.rfind('.') else { continue };
        if !full_key.starts_with("filter.") || last_dot <= "filter.".len() {
            continue;
        }
        entries.push(FilterEntry {
            scope: scope.to_string(),
            name: full_key["filter.".len()..last_dot].to_string(),
            key: full_key[last_dot + 1..].to_string(),
            value: value.to_string(),
        });
    }
    entries
}

fn is_absolute_path(c: &str) -> bool {
    let b = c.as_bytes();
    c.starts_with('/')
        || (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/'))
}

/// `isInside(candidate, root)` via `path.relative`
fn is_inside(candidate: &str, root: &str) -> bool {
    let c = resolve(candidate);
    let r = resolve(root);
    if c == r {
        return true;
    }
    let prefix = if r == "/" { "/".to_string() } else { format!("{r}/") };
    match c.strip_prefix(&prefix) {
        Some(rel) => !rel.starts_with(".."),
        None => false,
    }
}

fn git_crypt_regex() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        regex::Regex::new(
            r#"^(?:"([A-Za-z0-9_./\\:+@ -]+)"|([A-Za-z0-9_./\\:+@-]+)) (clean|smudge)(?: --key-name [A-Za-z0-9_-]+)?$"#,
        )
        .unwrap()
    })
}

fn is_git_crypt_command(key: &str, value: &str, repo_root: &str) -> bool {
    let Some(m) = git_crypt_regex().captures(value) else { return false };
    let Some(program) = m.get(1).or_else(|| m.get(2)).map(|x| x.as_str()) else { return false };
    if m.get(3).map(|x| x.as_str()) != Some(key) {
        return false;
    }
    let name = program.rsplit(['\\', '/']).next().unwrap_or("");
    (name == "git-crypt" || name == "git-crypt.exe") && is_absolute_path(program) && !is_inside(program, repo_root)
}

fn is_known_filter_command(key: &str, value: &str, repo_root: &str) -> bool {
    let lfs = match key {
        "clean" => Some("git-lfs clean -- %f"),
        "smudge" => Some("git-lfs smudge -- %f"),
        "process" => Some("git-lfs filter-process"),
        _ => None,
    };
    lfs == Some(value) || is_git_crypt_command(key, value, repo_root)
}

/// `getFilterOverrideArgs(entries, repoRoot)`; None when a driver name holds `=`.
pub fn filter_override_args(entries: &[FilterEntry], repo_root: &str) -> Option<Vec<String>> {
    let mut names: Vec<&str> = Vec::new();
    for e in entries {
        if !matches!(e.scope.as_str(), "local" | "worktree")
            || !matches!(e.key.as_str(), "clean" | "smudge" | "process")
        {
            continue;
        }
        if is_known_filter_command(&e.key, &e.value, repo_root) || names.contains(&e.name.as_str()) {
            continue;
        }
        names.push(&e.name);
    }
    if names.iter().any(|n| n.contains('=')) {
        return None;
    }
    Some(
        names
            .iter()
            .flat_map(|n| {
                [
                    "-c".to_string(),
                    format!("filter.{n}.clean="),
                    "-c".to_string(),
                    format!("filter.{n}.smudge="),
                    "-c".to_string(),
                    format!("filter.{n}.process="),
                    "-c".to_string(),
                    format!("filter.{n}.required=false"),
                ]
            })
            .collect(),
    )
}

/// `withGitHardeningEnv(process.env)`: the GIT_CONFIG_* entries to add.
pub fn hardening_env() -> Vec<(String, String)> {
    let existing = std::env::var("GIT_CONFIG_COUNT").ok();
    let start = match existing.as_deref().map(crate::jsnum::parse_int) {
        None => 0.0,
        Some(None) => 0.0,
        Some(Some(n)) if n < 0.0 => 0.0,
        Some(Some(n)) => n,
    };
    let mut out = Vec::new();
    for (offset, (k, v)) in [("core.fsmonitor", ""), ("safe.bareRepository", "explicit")].iter().enumerate() {
        let idx = crate::jsnum::to_js_string(start + offset as f64);
        out.push((format!("GIT_CONFIG_KEY_{idx}"), k.to_string()));
        out.push((format!("GIT_CONFIG_VALUE_{idx}"), v.to_string()));
    }
    out.push(("GIT_CONFIG_COUNT".to_string(), crate::jsnum::to_js_string(start + 2.0)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overrides() {
        let out = "local\0filter.lfs.clean\ngit-lfs clean -- %f\0local\0filter.evil.smudge\nsh -c x\0global\0filter.g.clean\nx\0";
        let e = parse_filter_config(out, true);
        assert_eq!(e.len(), 3);
        let args = filter_override_args(&e, "/repo").unwrap();
        assert_eq!(args[1], "filter.evil.clean=");
        assert_eq!(args.len(), 8);
        assert!(is_git_crypt_command("smudge", "\"/usr/bin/git-crypt\" smudge", "/repo"));
        assert!(!is_git_crypt_command("smudge", "/repo/bin/git-crypt smudge", "/repo"));
    }
}
