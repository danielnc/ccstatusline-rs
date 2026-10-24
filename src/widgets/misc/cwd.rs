//! current-working-dir (upstream `src/widgets/CurrentWorkingDir.tsx`).

use super::widget_info;
use crate::context::RenderContext;
use crate::settings::WidgetItem;
use crate::widgets::shared::{is_flag, raw_or_labeled};
use crate::widgets::Widget;

pub struct CurrentWorkingDir;

pub static CURRENT_WORKING_DIR: CurrentWorkingDir = CurrentWorkingDir;

/// `formatSymbolPrefix(item, defaultSymbol)`: the symbol plus its joining
/// space; an empty override collapses the space too.
pub fn format_symbol_prefix(item: &WidgetItem, default_symbol: &str) -> String {
    let symbol = item.character.as_deref().unwrap_or(default_symbol);
    if symbol.is_empty() {
        String::new()
    } else {
        format!("{symbol} ")
    }
}

fn is_sep(c: char) -> bool {
    c == '/' || c == '\\'
}

/// `path.split(/[\\/]+/).filter(part => part !== '')`
fn split_parts(path: &str) -> Vec<&str> {
    path.split(is_sep).filter(|p| !p.is_empty()).collect()
}

fn keep_last_segments(path: &str, segments: usize) -> String {
    let use_backslash = path.contains('\\') && !path.contains('/');
    let out_sep = if use_backslash { "\\" } else { "/" };
    let parts = split_parts(path);
    let prefix = if parts.first() == Some(&"~") { format!("~{out_sep}") } else { String::new() };
    let segs: &[&str] = if prefix.is_empty() { &parts } else { &parts[1..] };
    if segs.len() <= segments {
        return path.to_string();
    }
    format!("{prefix}...{out_sep}{}", segs[segs.len() - segments..].join(out_sep))
}

fn abbreviate_home_dir(path: &str) -> String {
    let home = crate::settings::home_dir().to_string_lossy().into_owned();
    if path == home {
        return "~".into();
    }
    if let Some(rest) = path.strip_prefix(home.as_str()) {
        // path[homeDir.length] must be a separator (undefined is not)
        if !rest.starts_with(is_sep) {
            return path.to_string();
        }
        return format!("~{rest}");
    }
    path.to_string()
}

fn abbreviate_path(path: &str) -> String {
    let use_backslash = path.contains('\\') && !path.contains('/');
    let sep = if use_backslash { "\\" } else { "/" };
    let normalized = abbreviate_home_dir(path);
    let parts = split_parts(&normalized);
    let last = parts.len().saturating_sub(1);
    let abbreviated: Vec<String> = parts
        .iter()
        .enumerate()
        .map(|(i, part)| {
            if i == 0 || i == last {
                return part.to_string();
            }
            // hidden directories keep the dot; `part.length > 1` counts UTF-16 units
            if part.starts_with('.') && part.encode_utf16().count() > 1 {
                return format!(".{}", crate::misc::second_utf16_unit(part).unwrap_or_default());
            }
            crate::misc::first_utf16_unit(part).unwrap_or_default()
        })
        .collect();
    let joined = abbreviated.join(sep);
    if normalized.starts_with('~') {
        joined
    } else if normalized.starts_with('/') {
        format!("{sep}{joined}")
    } else {
        joined
    }
}

impl Widget for CurrentWorkingDir {
    widget_info!("blue", "Current Working Dir", "Shows the current working directory", "Environment");
    fn label_prefix(&self, _item: &WidgetItem) -> Option<String> {
        Some("cwd: ".into())
    }
    fn editor_display(&self, item: &WidgetItem) -> crate::widgets::EditorDisplay {
        super::editor::cwd_display(item, self.display_name())
    }
    fn custom_keybinds(&self, _item: &WidgetItem) -> Vec<crate::widgets::Keybind> {
        super::editor::cwd_keybinds()
    }
    fn handle_editor_action(&self, action: &str, item: &serde_json::Value) -> Option<serde_json::Value> {
        super::editor::cwd_action(action, item)
    }
    fn render(&self, item: &WidgetItem, ctx: &RenderContext) -> Option<String> {
        // `metadata.segments ? parseInt(segments, 10) : undefined`, used when > 0
        let segments = item
            .meta("segments")
            .filter(|s| !s.is_empty())
            .and_then(crate::jsnum::parse_int)
            .filter(|n| *n > 0.0)
            .map(|n| n.min(usize::MAX as f64) as usize);
        let fish_style = is_flag(item, "fishStyle");
        let abbreviate_home = is_flag(item, "abbreviateHome");
        let symbol_prefix = format_symbol_prefix(item, "");

        if ctx.is_preview {
            let preview = if fish_style {
                "~/D/P/my-project".to_string()
            } else {
                let p = if abbreviate_home {
                    "~/Documents/Projects/my-project"
                } else {
                    "/Users/example/Documents/Projects/my-project"
                };
                match segments {
                    Some(n) => keep_last_segments(p, n),
                    None => p.to_string(),
                }
            };
            return Some(format!("{symbol_prefix}{}", raw_or_labeled(item, "cwd: ", &preview)));
        }

        let cwd = ctx.data.cwd.as_deref().filter(|c| !c.is_empty())?;
        let display = if fish_style {
            abbreviate_path(cwd)
        } else {
            let mut d = if abbreviate_home { abbreviate_home_dir(cwd) } else { cwd.to_string() };
            if let Some(n) = segments {
                d = keep_last_segments(&d, n);
            }
            d
        };
        Some(format!("{symbol_prefix}{}", raw_or_labeled(item, "cwd: ", &display)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments() {
        assert_eq!(keep_last_segments("/a/b/c/d", 2), ".../c/d");
        assert_eq!(keep_last_segments("~/a/b/c", 2), "~/.../b/c");
        assert_eq!(keep_last_segments("~/a", 1), "~/a");
        assert_eq!(keep_last_segments("C:\\x\\y\\z", 1), "...\\z");
        assert_eq!(keep_last_segments("/a/b", 5), "/a/b");
    }

    #[test]
    fn fish() {
        assert_eq!(abbreviate_path("/var/lib/.hidden/project"), "/var/l/.h/project");
        assert_eq!(abbreviate_path("/"), "/");
        assert_eq!(abbreviate_path("rel/x/y"), "rel/x/y");
        assert_eq!(abbreviate_path("/😀dir/x/end"), "/😀dir/x/end");
        assert_eq!(abbreviate_path("/a/😀dir/end"), "/a/\u{fffd}/end");
    }
}
