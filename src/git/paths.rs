//! node `path` (POSIX) helpers on strings: resolve, join, dirname.

/// Lexical normalization of an absolute path: no `.`, `..`, empty segments or
/// trailing slash.
fn normalize_abs(p: &str) -> String {
    let mut segs: Vec<&str> = Vec::new();
    for s in p.split('/') {
        match s {
            "" | "." => {}
            ".." => {
                segs.pop();
            }
            s => segs.push(s),
        }
    }
    format!("/{}", segs.join("/"))
}

pub fn process_cwd() -> String {
    std::env::current_dir().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| "/".into())
}

/// `path.resolve(p)`
pub fn resolve(p: &str) -> String {
    if p.starts_with('/') {
        normalize_abs(p)
    } else {
        normalize_abs(&format!("{}/{p}", process_cwd()))
    }
}

/// `path.resolve(base, p)`
pub fn resolve_from(base: &str, p: &str) -> String {
    if p.starts_with('/') {
        normalize_abs(p)
    } else {
        resolve(&format!("{base}/{p}"))
    }
}

/// `path.join(dir, name)` for an absolute dir
pub fn join(dir: &str, name: &str) -> String {
    normalize_abs(&format!("{dir}/{name}"))
}

/// `path.dirname(p)` for a normalized absolute path
pub fn dirname(p: &str) -> String {
    match p.rfind('/') {
        Some(0) | None => "/".into(),
        Some(i) => p[..i].to_string(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn resolve() {
        assert_eq!(super::resolve("/a/b/../c/./"), "/a/c");
        assert_eq!(super::resolve("/.."), "/");
        assert_eq!(super::resolve_from("/a/b", "../x"), "/a/x");
        assert_eq!(super::dirname("/a"), "/");
        assert_eq!(super::join("/a", ".git"), "/a/.git");
    }
}
