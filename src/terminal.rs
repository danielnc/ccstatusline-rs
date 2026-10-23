//! Terminal width detection (upstream `src/utils/terminal.ts`,
//! `terminal-native.ts`, `terminal-width-cache.ts`), without subprocesses: the
//! ancestor walk that upstream does with `ps`/`stty` uses proc_pidinfo/sysctl
//! and TIOCGWINSZ directly.

use crate::cachefile;
use serde_json::{Map, Value};
use std::sync::OnceLock;

static WIDTH: OnceLock<Option<usize>> = OnceLock::new();

fn parse_positive_int(s: &str) -> Option<usize> {
    let v = crate::jsnum::parse_int(s)?;
    if v <= 0.0 {
        None
    } else {
        Some(v as usize)
    }
}

/// Memoized width for this render; the first call may consult the shared
/// "no TTY" cache keyed by session.
pub fn terminal_width(session_id: Option<&str>, ttl_seconds: f64) -> Option<usize> {
    *WIDTH.get_or_init(|| compute(session_id, ttl_seconds))
}

/// Width as already memoized (callers that pass no options upstream).
pub fn memoized_width() -> Option<usize> {
    *WIDTH.get_or_init(|| compute(None, 0.0))
}

fn compute(session_id: Option<&str>, ttl: f64) -> Option<usize> {
    if let Ok(raw) = std::env::var("CCSTATUSLINE_WIDTH") {
        if !raw.is_empty() {
            if let Some(w) = parse_positive_int(&raw) {
                return Some(w);
            }
        }
    }
    if let Some(sid) = session_id {
        if read_cached_no_tty(sid, ttl) {
            return None;
        }
    }
    let width = probe();
    if let Some(sid) = session_id {
        if ttl > 0.0 && width.is_none() {
            write_cached_no_tty(sid);
        }
    }
    width
}

fn cache_path() -> std::path::PathBuf {
    crate::settings::home_dir().join(".cache").join("ccstatusline").join("terminal-width.json")
}

fn read_cache() -> Map<String, Value> {
    let Ok(text) = std::fs::read_to_string(cache_path()) else { return Map::new() };
    let Ok(v) = serde_json::from_str::<Value>(&text) else { return Map::new() };
    if v.get("version").and_then(Value::as_f64) != Some(1.0) {
        return Map::new();
    }
    let Some(entries) = v.get("entries").and_then(Value::as_object) else { return Map::new() };
    entries
        .iter()
        .filter(|(_, e)| {
            let w = e.get("width");
            (w.is_some_and(|w| w.is_number() || w.is_null())) && e.get("createdAt").is_some_and(Value::is_number)
        })
        .map(|(k, e)| (k.clone(), e.clone()))
        .collect()
}

fn read_cached_no_tty(sid: &str, ttl: f64) -> bool {
    if ttl <= 0.0 {
        return false;
    }
    let entries = read_cache();
    let Some(e) = entries.get(sid) else { return false };
    let created = e.get("createdAt").and_then(Value::as_f64).unwrap_or(0.0);
    if crate::clock::now_ms() - created > ttl * 1000.0 {
        return false;
    }
    e.get("width").is_some_and(Value::is_null)
}

fn write_cached_no_tty(sid: &str) {
    let path = cache_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    cachefile::with_lock(&path, 2000.0, || {
        let now = crate::clock::now_ms();
        let mut entries: Map<String, Value> = read_cache()
            .into_iter()
            .filter(|(_, e)| now - e.get("createdAt").and_then(Value::as_f64).unwrap_or(0.0) <= 3_600_000.0)
            .collect();
        let mut e = Map::new();
        e.insert("width".into(), Value::Null);
        e.insert("createdAt".into(), crate::jsjson::num(now));
        entries.insert(sid.to_string(), Value::Object(e));
        let mut root = Map::new();
        root.insert("version".into(), Value::from(1));
        root.insert("entries".into(), Value::Object(entries));
        let tmp = path.with_file_name(format!("terminal-width.json.{}.tmp", std::process::id()));
        if std::fs::write(&tmp, crate::jsjson::stringify(&Value::Object(root))).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    });
}

fn probe() -> Option<usize> {
    if crate::clock::no_probe() {
        return None;
    }
    let mut pid = std::process::id() as i32;
    for _ in 0..8 {
        let Some(info) = proc_info(pid) else { break };
        let ppid = info.ppid;
        if ppid <= 0 {
            break;
        }
        pid = ppid;
        let Some(parent) = proc_info(pid) else { continue };
        let Some(tty) = parent.tty else { continue };
        if let Some(w) = width_of_device(&tty) {
            return Some(w);
        }
    }
    None
}

struct ProcInfo {
    ppid: i32,
    tty: Option<String>,
}

#[cfg(target_os = "macos")]
fn proc_info(pid: i32) -> Option<ProcInfo> {
    unsafe {
        let mut info: libc::proc_bsdinfo = std::mem::zeroed();
        let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
        let n = libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, &mut info as *mut _ as *mut libc::c_void, size);
        if n != size {
            return None;
        }
        let dev = info.e_tdev;
        let tty = if dev == u32::MAX || dev == 0 {
            None
        } else {
            let name = libc::devname(dev as libc::dev_t, libc::S_IFCHR);
            if name.is_null() {
                None
            } else {
                let s = std::ffi::CStr::from_ptr(name).to_string_lossy().into_owned();
                if s.is_empty() || s == "??" {
                    None
                } else {
                    Some(format!("/dev/{s}"))
                }
            }
        };
        Some(ProcInfo { ppid: info.pbi_ppid as i32, tty })
    }
}

#[cfg(target_os = "linux")]
fn proc_info(pid: i32) -> Option<ProcInfo> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let end = stat.rfind(')')?;
    let fields: Vec<&str> = stat[end + 1..].split_whitespace().collect();
    let ppid: i32 = fields.get(1)?.parse().ok()?;
    let mut tty = None;
    for fd in 0..3 {
        if let Ok(t) = std::fs::read_link(format!("/proc/{pid}/fd/{fd}")) {
            let t = t.to_string_lossy().into_owned();
            if t.starts_with("/dev/pts/") || t.starts_with("/dev/tty") {
                tty = Some(t);
                break;
            }
        }
    }
    Some(ProcInfo { ppid, tty })
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn proc_info(_pid: i32) -> Option<ProcInfo> {
    None
}

fn width_of_device(dev: &str) -> Option<usize> {
    let c = std::ffi::CString::new(dev).ok()?;
    unsafe {
        let fd = libc::open(c.as_ptr(), libc::O_RDONLY | libc::O_NOCTTY | libc::O_NONBLOCK);
        if fd < 0 {
            return None;
        }
        let mut ws: libc::winsize = std::mem::zeroed();
        let ok = libc::isatty(fd) == 1 && libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws) == 0;
        libc::close(fd);
        if ok && ws.ws_col > 0 {
            Some(ws.ws_col as usize)
        } else {
            None
        }
    }
}
