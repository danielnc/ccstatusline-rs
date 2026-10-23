//! Data providers for the misc widget group: custom commands (upstream
//! `src/utils/custom-command{,-capture}.ts`), system memory, Claude Code's
//! layered settings and session manifests (`src/utils/claude-settings.ts`), a
//! WHATWG URL subset for the link widget, and the herdr socket client.

pub mod claude_layers;
pub mod custom_command;
pub mod herdr;
pub mod memory;
pub mod sha256;
pub mod url;

/// The first UTF-16 code unit of `s` (`s[0]` in JS). A lone surrogate cannot
/// live in a Rust string; Node writes it to stdout as U+FFFD, so that is what
/// we return for an astral first character.
pub fn first_utf16_unit(s: &str) -> Option<String> {
    let c = s.chars().next()?;
    Some(if (c as u32) > 0xffff { "\u{fffd}".to_string() } else { c.to_string() })
}

/// The second UTF-16 code unit of `s` (`s[1]`), with the same U+FFFD rule.
pub fn second_utf16_unit(s: &str) -> Option<String> {
    let mut it = s.chars();
    let c0 = it.next()?;
    if (c0 as u32) > 0xffff {
        // s[1] is the low surrogate of the first character
        return Some("\u{fffd}".to_string());
    }
    first_utf16_unit(it.as_str())
}

/// Node `path.join` for POSIX strings (join with "/" then `path.normalize`).
pub fn posix_join(parts: &[&str]) -> String {
    let joined = parts.iter().filter(|p| !p.is_empty()).copied().collect::<Vec<_>>().join("/");
    if joined.is_empty() {
        return ".".into();
    }
    posix_normalize(&joined)
}

/// Node `path.posix.normalize`.
pub fn posix_normalize(p: &str) -> String {
    if p.is_empty() {
        return ".".into();
    }
    let absolute = p.starts_with('/');
    let trailing = p.ends_with('/');
    let mut out: Vec<&str> = Vec::new();
    for seg in p.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                if out.last().is_some_and(|l| *l != "..") {
                    out.pop();
                } else if !absolute {
                    out.push("..");
                }
            }
            s => out.push(s),
        }
    }
    let mut s = out.join("/");
    if s.is_empty() && !absolute {
        s = ".".into();
    }
    if !s.is_empty() && trailing {
        s.push('/');
    }
    if absolute {
        format!("/{s}")
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_units() {
        assert_eq!(first_utf16_unit("abc").as_deref(), Some("a"));
        assert_eq!(first_utf16_unit("😀x").as_deref(), Some("\u{fffd}"));
        assert_eq!(first_utf16_unit(""), None);
        assert_eq!(second_utf16_unit(".😀").as_deref(), Some("\u{fffd}"));
        assert_eq!(second_utf16_unit(".é").as_deref(), Some("é"));
        assert_eq!(second_utf16_unit("."), None);
    }

    #[test]
    fn normalize() {
        assert_eq!(posix_join(&["/a/b/", ".claude", "x.json"]), "/a/b/.claude/x.json");
        assert_eq!(posix_join(&["/a/../b", ".claude"]), "/b/.claude");
        assert_eq!(posix_join(&["rel", ".claude"]), "rel/.claude");
        assert_eq!(posix_join(&["../x", ".claude"]), "../x/.claude");
        assert_eq!(posix_join(&["/..", ".claude"]), "/.claude");
        assert_eq!(posix_join(&["//a//b", "c"]), "/a/b/c");
    }
}
