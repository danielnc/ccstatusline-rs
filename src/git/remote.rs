//! Remote URL parsing and remote lookups (upstream `src/utils/git-remote.ts`).

use super::{run_git, run_git_args};
use crate::context::RenderContext;
use crate::jsnum::js_trim;

#[derive(Clone, Debug, PartialEq)]
pub struct ParsedRemote {
    pub host: String,
    pub owner: String,
    pub repo: String,
}

#[derive(Clone, Debug)]
pub struct RemoteInfo {
    pub name: String,
    pub url: String,
    pub host: String,
    pub owner: String,
    pub repo: String,
}

fn is_line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// `^([^:]+):(.+?)(?:\.git)?\/?$` on `r`: host and path.
fn ssh_host_path(r: &str) -> Option<(&str, &str)> {
    let colon = r.find(':')?;
    let host = &r[..colon];
    let rest = &r[colon + 1..];
    if host.is_empty() || rest.is_empty() || rest.contains(is_line_terminator) {
        return None;
    }
    // The lazy path stops at the earliest point where the optional suffixes
    // can end the match: the longest of ".git/", ".git", "/", "" that still
    // leaves at least one character.
    for suffix in [".git/", ".git", "/", ""] {
        if let Some(p) = rest.strip_suffix(suffix) {
            if !p.is_empty() {
                return Some((host, p));
            }
        }
    }
    None
}

/// `/^(?:[^@]+@)?([^:]+):(.+?)(?:\.git)?\/?$/`
fn ssh_match(s: &str) -> Option<(&str, &str)> {
    if let Some(at) = s.find('@') {
        if at > 0 {
            if let Some(m) = ssh_host_path(&s[at + 1..]) {
                return Some(m);
            }
        }
    }
    ssh_host_path(s)
}

fn split_owner_repo(path: &str) -> Option<(String, String)> {
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let (repo, owner) = segs.split_last()?;
    let owner = owner.join("/");
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some((owner, repo.to_string()))
}

/// `parseRemoteUrl(url)`
pub fn parse_remote_url(url: &str) -> Option<ParsedRemote> {
    let trimmed = js_trim(url);
    if trimmed.is_empty() {
        return None;
    }
    if !trimmed.contains("://") {
        if let Some((host, path)) = ssh_match(trimmed) {
            let (owner, repo) = split_owner_repo(path)?;
            return Some(ParsedRemote { host: host.to_string(), owner, repo });
        }
    }
    let u = super::url::parse(trimmed)?;
    if !matches!(u.protocol.as_str(), "http:" | "https:" | "ssh:" | "git:") {
        return None;
    }
    let joined = u.pathname.split('/').filter(|s| !s.is_empty()).collect::<Vec<_>>().join("/");
    let pathname = joined.strip_suffix(".git").unwrap_or(&joined);
    let (owner, repo) = split_owner_repo(pathname)?;
    let host = if u.protocol == "http:" || u.protocol == "https:" { u.host } else { u.hostname };
    Some(ParsedRemote { host, owner, repo })
}

/// `getRemoteInfo(remoteName, context)`
pub fn get_remote_info(name: &str, ctx: &RenderContext) -> Option<RemoteInfo> {
    let key = format!("remote get-url -- {name}");
    let url = run_git_args(ctx, &["remote", "get-url", "--", name], Some(&key))?;
    let p = parse_remote_url(&url)?;
    Some(RemoteInfo { name: name.to_string(), url, host: p.host, owner: p.owner, repo: p.repo })
}

pub fn list_remotes(ctx: &RenderContext) -> Vec<String> {
    match run_git(ctx, "remote") {
        Some(o) => o.split('\n').filter(|s| !s.is_empty()).map(String::from).collect(),
        None => Vec::new(),
    }
}

fn tracking_remote_name(ctx: &RenderContext) -> Option<String> {
    let upstream_ref = run_git(ctx, "rev-parse --abbrev-ref --symbolic-full-name @{upstream}")?;
    let mut remotes = list_remotes(ctx);
    // stable sort by length, longest first (UTF-16 length)
    remotes.sort_by_key(|r| std::cmp::Reverse(r.encode_utf16().count()));
    remotes.into_iter().find(|r| upstream_ref == *r || upstream_ref.starts_with(&format!("{r}/")))
}

/// `getUpstreamRemoteInfo(context)`
pub fn get_upstream_remote_info(ctx: &RenderContext) -> Option<RemoteInfo> {
    if let Some(u) = get_remote_info("upstream", ctx) {
        return Some(u);
    }
    let name = tracking_remote_name(ctx)?;
    get_remote_info(&name, ctx)
}

/// `getForkStatus(context).isFork`
pub fn is_fork(ctx: &RenderContext) -> bool {
    let origin = get_remote_info("origin", ctx);
    let upstream = get_remote_info("upstream", ctx);
    let peer = upstream.or_else(|| get_remote_info("fork", ctx));
    match (origin, peer) {
        (Some(o), Some(p)) => o.owner != p.owner || o.repo != p.repo,
        _ => false,
    }
}

pub fn repo_web_url(r: &RemoteInfo) -> String {
    format!("https://{}/{}/{}", r.host, r.owner, r.repo)
}

pub fn branch_web_url(r: &RemoteInfo, encoded_branch: &str) -> String {
    format!("{}/tree/{encoded_branch}", repo_web_url(r))
}

#[cfg(test)]
mod tests {
    use super::parse_remote_url;

    fn p(u: &str) -> Option<(String, String, String)> {
        parse_remote_url(u).map(|r| (r.host, r.owner, r.repo))
    }

    #[test]
    fn formats() {
        let s = |a: &str, b: &str, c: &str| Some((a.to_string(), b.to_string(), c.to_string()));
        assert_eq!(p("git@github.com:owner/repo.git"), s("github.com", "owner", "repo"));
        assert_eq!(p("https://github.com/owner/repo.git"), s("github.com", "owner", "repo"));
        assert_eq!(p("ssh://git@github.com/owner/repo.git"), s("github.com", "owner", "repo"));
        assert_eq!(p("git://github.com/owner/repo"), s("github.com", "owner", "repo"));
        assert_eq!(p("git@gitlab.com:group/sub/repo.git/"), s("gitlab.com", "group/sub", "repo"));
        assert_eq!(p("https://github.com//owner//repo.git//"), s("github.com", "owner", "repo"));
        assert_eq!(p("a@b@c:o/r"), s("b@c", "o", "r"));
        assert_eq!(p("https:/github.com/a/b"), s("https", "github.com/a", "b"));
        assert_eq!(p("/local/path"), None);
        assert_eq!(p("file:///x/y"), None);
        assert_eq!(p("host:repo"), None);
        assert_eq!(p("x:.git"), None);
    }
}
