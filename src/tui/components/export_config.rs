//! Upstream `ExportConfigDialog.tsx`, with App.tsx's `handleExportConfig` and `exportConfig` from
//! `src/utils/config.ts`: a path input, then the settings written there with `exportedBy`.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::tui::ink::keys::Input;
use crate::tui::ink::layout::*;
use crate::tui::screens::{Action, Ctx, Screen};
use crate::tui::text_cursor::should_insert_input;

/// `expandPath`: `~` and `~/...` under the home directory, anything else as typed.
pub fn expand_path(file_path: &str) -> String {
    if file_path.starts_with("~/") || file_path == "~" {
        let home = crate::settings::home_dir();
        return node_path_join(&home.to_string_lossy(), file_path.get(2..).unwrap_or(""));
    }
    file_path.to_string()
}

/// `path.join(a, b)`: joined with `/`, then normalized (`.`/`..` segments, repeated slashes; a trailing slash kept).
fn node_path_join(a: &str, b: &str) -> String {
    let joined = match (a.is_empty(), b.is_empty()) {
        (true, true) => return ".".into(),
        (true, false) => b.to_string(),
        (false, true) => a.to_string(),
        (false, false) => format!("{a}/{b}"),
    };
    let absolute = joined.starts_with('/');
    let trailing = joined.ends_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for seg in joined.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|p| *p != "..") {
                    parts.pop();
                } else if !absolute {
                    parts.push("..");
                }
            }
            seg => parts.push(seg),
        }
    }
    let mut out = parts.join("/");
    if absolute {
        out.insert(0, '/');
    }
    if out.is_empty() {
        out = ".".into();
    }
    if trailing && !out.ends_with('/') {
        out.push('/');
    }
    out
}

/// Node's fs error message: `CODE: description, syscall 'path'`.
pub fn node_fs_error(err: &std::io::Error, syscall: &str, path: &str) -> String {
    let (code, description) = match err.raw_os_error() {
        Some(libc::ENOENT) => ("ENOENT", "no such file or directory"),
        Some(libc::EACCES) => ("EACCES", "permission denied"),
        Some(libc::EPERM) => ("EPERM", "operation not permitted"),
        Some(libc::ENOTDIR) => ("ENOTDIR", "not a directory"),
        Some(libc::EISDIR) => ("EISDIR", "illegal operation on a directory"),
        Some(libc::EEXIST) => ("EEXIST", "file already exists"),
        Some(libc::EROFS) => ("EROFS", "read-only file system"),
        Some(libc::ENOSPC) => ("ENOSPC", "no space left on device"),
        Some(libc::ENAMETOOLONG) => ("ENAMETOOLONG", "name too long"),
        Some(libc::ELOOP) => ("ELOOP", "too many symbolic links encountered"),
        _ => ("EIO", "i/o error"),
    };
    format!("{code}: {description}, {syscall} '{path}'")
}

/// `path.dirname(p)` (posix).
fn node_dirname(p: &str) -> String {
    let b = p.as_bytes();
    if b.is_empty() {
        return ".".into();
    }
    let has_root = b[0] == b'/';
    let mut end = None;
    let mut matched_slash = true;
    for i in (1..b.len()).rev() {
        if b[i] == b'/' {
            if !matched_slash {
                end = Some(i);
                break;
            }
        } else {
            matched_slash = false;
        }
    }
    match end {
        None => (if has_root { "/" } else { "." }).into(),
        Some(1) if has_root => "//".into(),
        Some(e) => p[..e].to_string(),
    }
}

/// `fs.mkdir(dir, { recursive: true })`: the error names the first path that could not be created.
fn mkdir_recursive(dir: &str) -> Result<(), String> {
    let resolved = crate::settings::path_resolve(dir);
    match std::fs::metadata(&resolved) {
        Ok(md) if md.is_dir() => return Ok(()),
        Ok(_) => {
            return Err(node_fs_error(&std::io::Error::from_raw_os_error(libc::EEXIST), "mkdir", dir));
        }
        Err(_) => {}
    }
    // The deepest missing ancestor is created first; a failure names the path given (as Node's mkdirp does).
    let mut missing: Vec<PathBuf> = Vec::new();
    let mut cur: &Path = &resolved;
    loop {
        match std::fs::symlink_metadata(cur) {
            Ok(_) => break,
            Err(_) => {
                missing.push(cur.to_path_buf());
                match cur.parent() {
                    Some(p) => cur = p,
                    None => break,
                }
            }
        }
    }
    for p in missing.iter().rev() {
        if let Err(e) = std::fs::create_dir(p) {
            if e.kind() == std::io::ErrorKind::AlreadyExists && p.is_dir() {
                continue;
            }
            return Err(node_fs_error(&e, "mkdir", dir));
        }
    }
    Ok(())
}

/// `exportConfig(settings, filePath)`: `{...settings, exportedBy}` as `JSON.stringify(_, null, 2)`.
fn export_config(settings: &Value, file_path: &str) -> Result<(), String> {
    let expanded = expand_path(file_path);
    let data = crate::jsobj::with(settings, "exportedBy", Value::String(crate::UPSTREAM_VERSION.into()));
    mkdir_recursive(&node_dirname(&expanded))?;
    let target = crate::settings::path_resolve(&expanded);
    if target.is_dir() {
        return Err(node_fs_error(&std::io::Error::from_raw_os_error(libc::EISDIR), "open", &expanded));
    }
    if target.exists() {
        // `writeFile` opens the file for writing; where that fails, so must the export.
        std::fs::OpenOptions::new().write(true).open(&target).map_err(|e| node_fs_error(&e, "open", &expanded))?;
    }
    crate::settings::write_json_atomic(&target, &crate::jsobj::strip_undefined(&data))
        .map_err(|e| node_fs_error(&e, "open", &expanded))
}

pub struct ExportConfigDialog {
    input_value: String,
}

impl ExportConfigDialog {
    pub fn new(_ctx: &Ctx) -> Self {
        // DEFAULT_EXPORT_PATH
        let home = crate::settings::home_dir();
        ExportConfigDialog { input_value: node_path_join(&home.to_string_lossy(), "ccstatusline-config.json") }
    }
}

/// The path input both dialogs share: `Path: {value}` and an inverse block cursor.
pub fn path_input_view(title: &str, prompt: &str, value: &str) -> El {
    boxed(
        BoxStyle::column(),
        vec![
            text(TextStyle::bold(), vec![s(title)]),
            text(TextStyle::dim(), vec![s(prompt)]),
            boxed(
                BoxStyle::default().margin_top(1.0),
                vec![
                    plain("Path: "),
                    plain(value),
                    text(TextStyle { inverse: true, ..Default::default() }, vec![s(" ")]),
                ],
            ),
            boxed(
                BoxStyle::default().margin_top(1.0),
                vec![text(TextStyle::dim(), vec![s("Enter to confirm, Escape to cancel")])],
            ),
        ],
    )
}

/// The input's keys: `Some(true)` for Enter, `Some(false)` for Escape; other keys edit `value`.
pub fn path_input_handle(value: &mut String, ev: &Input) -> Option<bool> {
    let key = &ev.key;
    if key.return_ {
        return Some(true);
    }
    if key.escape {
        return Some(false);
    }
    if key.backspace {
        value.pop();
    } else if should_insert_input(&ev.input, key) {
        value.push_str(&ev.input);
    }
    None
}

impl Screen for ExportConfigDialog {
    fn handle(&mut self, ev: &Input, ctx: &Ctx) -> Vec<Action> {
        match path_input_handle(&mut self.input_value, ev) {
            Some(true) => match export_config(ctx.settings, &self.input_value) {
                Ok(()) => {
                    vec![Action::Flash(format!("Config exported to {}", self.input_value), "green"), Action::Back]
                }
                Err(message) => vec![Action::FlowNotice { title: "Export Failed".into(), message, color: "red" }],
            },
            Some(false) => vec![Action::Back],
            None => Vec::new(),
        }
    }

    fn view(&mut self, _ctx: &Ctx) -> El {
        path_input_view("Export Config", "Enter the file path to export your configuration to:", &self.input_value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_and_dirnames_like_node() {
        assert_eq!(node_path_join("/h", ""), "/h");
        assert_eq!(node_path_join("/h", "a/../b.json"), "/h/b.json");
        assert_eq!(node_path_join("/h/", "x/"), "/h/x/");
        assert_eq!(node_dirname("/h/a.json"), "/h");
        assert_eq!(node_dirname("/a.json"), "/");
        assert_eq!(node_dirname("a.json"), ".");
        assert_eq!(node_dirname("a/b/"), "a");
    }
}
