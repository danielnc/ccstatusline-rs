//! Custom command runner (upstream `src/utils/custom-command.ts` and
//! `custom-command-capture.ts`).
//!
//! Upstream spawns a helper Node runtime that runs the command through
//! `/bin/sh -c` (detached: its own session), pipes the status JSON to its
//! stdin, captures at most 1 MiB of stdout (stderr ignored), enforces the
//! timeout, and answers with `{status:'ok',stdout}` or a failure marker. Here
//! the same contract is implemented directly with one poll loop, so a render
//! costs one `sh` process instead of a Node runtime plus `sh`.
//!
//! Results are cached like upstream when `customCommandCacheTtlSeconds > 0`:
//! in process (keyed with the cwd), and on disk per cwd at
//! `~/.cache/ccstatusline/custom-command-cache/cmd-<sha256(cwd)[..16]>.json`
//! when the session id is known.

use serde_json::{Map, Value};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::io::AsRawFd;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const MAX_TTL_SECONDS: f64 = 60.0;
const MAX_CACHED_OUTPUT_CHARS: usize = 16_384;
const MAX_STDOUT_BYTES: usize = 1024 * 1024;
/// Node's setTimeout clamps delays above 2^31-1 ms to 1 ms.
const TIMEOUT_MAX_MS: f64 = 2_147_483_647.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CmdResult {
    Ok(String),
    Failed(String),
}

impl CmdResult {
    fn to_json(&self) -> Value {
        let mut m = Map::new();
        match self {
            CmdResult::Ok(s) => {
                m.insert("status".into(), Value::String("ok".into()));
                m.insert("stdout".into(), Value::String(s.clone()));
            }
            CmdResult::Failed(s) => {
                m.insert("status".into(), Value::String("failed".into()));
                m.insert("marker".into(), Value::String(s.clone()));
            }
        }
        Value::Object(m)
    }

    fn from_json(v: &Value) -> Option<CmdResult> {
        match v.get("status")?.as_str()? {
            "ok" => Some(CmdResult::Ok(v.get("stdout")?.as_str()?.to_string())),
            "failed" => Some(CmdResult::Failed(v.get("marker")?.as_str()?.to_string())),
            _ => None,
        }
    }
}

pub struct Request<'a> {
    pub command: &'a str,
    /// JSON payload piped to the command on stdin.
    pub input: String,
    pub timeout_ms: f64,
    pub ttl_seconds: f64,
    pub session_id: Option<&'a str>,
    pub terminal_width: Option<usize>,
}

// ------------------------------------------------------------------ caching

struct Entry {
    result: CmdResult,
    created_at: f64,
}

static MEMORY: Mutex<Option<HashMap<String, Entry>>> = Mutex::new(None);

fn ttl_ms(ttl_seconds: f64) -> f64 {
    if !ttl_seconds.is_finite() {
        return 0.0;
    }
    MAX_TTL_SECONDS.min(ttl_seconds.max(0.0)) * 1000.0
}

fn entry_key(req: &Request) -> String {
    [
        req.command.to_string(),
        crate::jsnum::to_js_string(req.timeout_ms),
        req.session_id.unwrap_or("").to_string(),
        req.terminal_width.map(|w| w.to_string()).unwrap_or_default(),
    ]
    .join("\0")
}

fn is_fresh(created_at: f64, ttl_ms: f64, now: f64) -> bool {
    let age = now - created_at;
    age >= 0.0 && age <= ttl_ms
}

fn cache_path(cwd: &str) -> std::path::PathBuf {
    let hash = super::sha256::hex(cwd.as_bytes());
    crate::settings::home_dir()
        .join(".cache")
        .join("ccstatusline")
        .join("custom-command-cache")
        .join(format!("cmd-{}.json", &hash[..16]))
}

/// `isCacheEntry`
fn valid_entry(v: &Value) -> Option<(CmdResult, f64)> {
    let created = v.get("createdAt")?.as_f64()?;
    let result = v.get("result").filter(|r| r.is_object())?;
    Some((CmdResult::from_json(result)?, created))
}

/// `readPersistentCache`: (cwd, entries) with only well-formed entries kept.
fn read_persistent(path: &std::path::Path) -> Option<(String, Vec<(String, Value)>)> {
    let bytes = std::fs::read(path).ok()?;
    let v: Value = serde_json::from_str(&String::from_utf8_lossy(&bytes)).ok()?;
    if v.get("version").and_then(Value::as_f64) != Some(1.0) {
        return None;
    }
    let cwd = v.get("cwd")?.as_str()?.to_string();
    let entries: Vec<(String, Value)> = match v.get("entries")? {
        Value::Object(m) => m.iter().map(|(k, e)| (k.clone(), e.clone())).collect(),
        Value::Array(a) => a.iter().enumerate().map(|(i, e)| (i.to_string(), e.clone())).collect(),
        _ => return None,
    };
    Some((cwd, entries.into_iter().filter(|(_, e)| valid_entry(e).is_some()).collect()))
}

fn write_persistent(path: &std::path::Path, cwd: &str, entries: Vec<(String, Value)>) {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    let mut root = Map::new();
    root.insert("version".into(), Value::from(1));
    root.insert("cwd".into(), Value::String(cwd.to_string()));
    root.insert("entries".into(), Value::Object(entries.into_iter().collect()));
    let text = crate::jsjson::stringify(&Value::Object(root));
    let Some(dir) = path.parent() else { return };
    if std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir).is_err() {
        return;
    }
    let tmp = std::path::PathBuf::from(format!(
        "{}.{}.{}.tmp",
        path.to_string_lossy(),
        std::process::id(),
        crate::jsnum::to_js_string(crate::clock::now_ms())
    ));
    let res = (|| {
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
        f.write_all(text.as_bytes())?;
        drop(f);
        std::fs::rename(&tmp, path)
    })();
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

fn read_persistent_entry(cwd: &str, key: &str, ttl: f64, now: f64) -> Option<(CmdResult, f64)> {
    let (file_cwd, entries) = read_persistent(&cache_path(cwd))?;
    if file_cwd != cwd {
        return None;
    }
    let (_, e) = entries.iter().find(|(k, _)| k == key)?;
    let (result, created) = valid_entry(e)?;
    is_fresh(created, ttl, now).then_some((result, created))
}

fn write_persistent_entry(cwd: &str, key: &str, result: &CmdResult, created_at: f64) {
    let path = cache_path(cwd);
    let mut entries: Vec<(String, Value)> = match read_persistent(&path) {
        Some((c, entries)) if c == cwd => entries
            .into_iter()
            .filter(|(_, e)| {
                valid_entry(e).is_some_and(|(_, created)| is_fresh(created, MAX_TTL_SECONDS * 1000.0, created_at))
            })
            .collect(),
        _ => Vec::new(),
    };
    let mut e = Map::new();
    e.insert("result".into(), result.to_json());
    e.insert("createdAt".into(), crate::jsjson::num(created_at));
    // `entries[key] = entry`: an existing key keeps its position
    match entries.iter_mut().find(|(k, _)| k == key) {
        Some(slot) => slot.1 = Value::Object(e),
        None => entries.push((key.to_string(), Value::Object(e))),
    }
    write_persistent(&path, cwd, entries);
}

/// `runCustomCommand(request)`
pub fn run(req: &Request) -> CmdResult {
    let ttl = ttl_ms(req.ttl_seconds);
    if ttl == 0.0 {
        return execute(req);
    }
    let Ok(cwd) = std::env::current_dir() else { return execute(req) };
    let cwd = cwd.to_string_lossy().into_owned();
    let key = entry_key(req);
    let mem_key = format!("{key}\0{cwd}");
    let shared = req.session_id.is_some_and(|s| !s.is_empty());
    let now = crate::clock::now_ms();

    if let Some(e) = MEMORY
        .lock()
        .ok()
        .and_then(|m| m.as_ref().and_then(|m| m.get(&mem_key)).map(|e| (e.result.clone(), e.created_at)))
    {
        if is_fresh(e.1, ttl, now) {
            return e.0;
        }
    }
    if shared {
        if let Some((result, created_at)) = read_persistent_entry(&cwd, &key, ttl, now) {
            if let Ok(mut m) = MEMORY.lock() {
                m.get_or_insert_with(HashMap::new).insert(mem_key, Entry { result: result.clone(), created_at });
            }
            return result;
        }
    }
    let result = execute(req);
    let created_at = crate::clock::now_ms();
    if let Ok(mut m) = MEMORY.lock() {
        m.get_or_insert_with(HashMap::new).insert(mem_key, Entry { result: result.clone(), created_at });
    }
    if shared {
        write_persistent_entry(&cwd, &key, &result, created_at);
    }
    result
}

// ---------------------------------------------------------------- execution

fn signal_name(sig: i32) -> String {
    let names: &[(i32, &str)] = &[
        (libc::SIGHUP, "SIGHUP"),
        (libc::SIGINT, "SIGINT"),
        (libc::SIGQUIT, "SIGQUIT"),
        (libc::SIGILL, "SIGILL"),
        (libc::SIGTRAP, "SIGTRAP"),
        (libc::SIGABRT, "SIGABRT"),
        (libc::SIGBUS, "SIGBUS"),
        (libc::SIGFPE, "SIGFPE"),
        (libc::SIGKILL, "SIGKILL"),
        (libc::SIGUSR1, "SIGUSR1"),
        (libc::SIGSEGV, "SIGSEGV"),
        (libc::SIGUSR2, "SIGUSR2"),
        (libc::SIGPIPE, "SIGPIPE"),
        (libc::SIGALRM, "SIGALRM"),
        (libc::SIGTERM, "SIGTERM"),
        (libc::SIGCHLD, "SIGCHLD"),
        (libc::SIGCONT, "SIGCONT"),
        (libc::SIGSTOP, "SIGSTOP"),
        (libc::SIGTSTP, "SIGTSTP"),
        (libc::SIGTTIN, "SIGTTIN"),
        (libc::SIGTTOU, "SIGTTOU"),
        (libc::SIGURG, "SIGURG"),
        (libc::SIGXCPU, "SIGXCPU"),
        (libc::SIGXFSZ, "SIGXFSZ"),
        (libc::SIGVTALRM, "SIGVTALRM"),
        (libc::SIGPROF, "SIGPROF"),
        (libc::SIGWINCH, "SIGWINCH"),
        (libc::SIGIO, "SIGIO"),
        (libc::SIGSYS, "SIGSYS"),
        #[cfg(target_os = "macos")]
        (libc::SIGEMT, "SIGEMT"),
        #[cfg(target_os = "macos")]
        (libc::SIGINFO, "SIGINFO"),
        #[cfg(target_os = "linux")]
        (libc::SIGSTKFLT, "SIGSTKFLT"),
        #[cfg(target_os = "linux")]
        (libc::SIGPWR, "SIGPWR"),
    ];
    names.iter().find(|(n, _)| *n == sig).map(|(_, s)| s.to_string()).unwrap_or_else(|| sig.to_string())
}

fn set_nonblocking(fd: i32) {
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags >= 0 {
            libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
        }
    }
}

fn kill_group(pid: u32) {
    unsafe {
        if libc::kill(-(pid as i32), libc::SIGKILL) != 0 {
            libc::kill(pid as i32, libc::SIGKILL);
        }
    }
}

fn exit_marker(status: std::process::ExitStatus) -> Option<String> {
    if let Some(sig) = status.signal() {
        return Some(format!("[Signal: {}]", signal_name(sig)));
    }
    match status.code() {
        Some(0) => None,
        Some(c) => Some(format!("[Exit: {c}]")),
        None => Some("[Error]".into()),
    }
}

/// `output.toString('utf8', 0, length).slice(0, maxChars).trim()`
fn finish_stdout(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut units = 0usize;
    let mut end = text.len();
    for (i, c) in text.char_indices() {
        let w = c.len_utf16();
        if units + w > MAX_CACHED_OUTPUT_CHARS {
            end = i;
            // A cut through a surrogate pair leaves a lone high surrogate,
            // which reaches the terminal as U+FFFD.
            let mut s = text[..end].to_string();
            if units < MAX_CACHED_OUTPUT_CHARS {
                s.push('\u{fffd}');
            }
            return crate::jsnum::js_trim(&s).to_string();
        }
        units += w;
    }
    crate::jsnum::js_trim(&text[..end]).to_string()
}

/// `captureCustomCommand` + `executeCommand`.
fn execute(req: &Request) -> CmdResult {
    let mut cmd = Command::new("/bin/sh");
    cmd.arg("-c").arg(req.command).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    // detached: true -> setsid(), so the whole tree can be killed as a group
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return CmdResult::Failed(
                match e.raw_os_error() {
                    Some(libc::ENOENT) => "[Cmd not found]",
                    Some(libc::EACCES) => "[Permission denied]",
                    _ => "[Error]",
                }
                .into(),
            )
        }
    };
    let pid = child.id();
    let deadline = if req.timeout_ms > 0.0 {
        let ms = if req.timeout_ms > TIMEOUT_MAX_MS { 1.0 } else { req.timeout_ms };
        Some(Instant::now() + Duration::from_micros((ms * 1000.0) as u64))
    } else {
        None
    };

    let mut stdin = child.stdin.take();
    let mut stdout = child.stdout.take();
    if let Some(s) = &stdin {
        set_nonblocking(s.as_raw_fd());
    }
    if let Some(s) = &stdout {
        set_nonblocking(s.as_raw_fd());
    }
    let input = req.input.as_bytes();
    let mut written = 0usize;
    let mut output: Vec<u8> = Vec::new();
    let mut buf = vec![0u8; 64 * 1024];

    let fail = |child: &mut std::process::Child, marker: &str| {
        kill_group(pid);
        let _ = child.try_wait();
        CmdResult::Failed(marker.to_string())
    };

    // Phase 1: feed stdin and drain stdout until EOF ('close' needs stdout closed).
    while stdout.is_some() {
        let remaining = match deadline {
            Some(d) => {
                let now = Instant::now();
                if now >= d {
                    return on_timeout(&mut child, pid, &output);
                }
                (d - now).as_millis().min(i32::MAX as u128) as i32 + 1
            }
            None => -1,
        };
        let mut fds: Vec<libc::pollfd> = Vec::with_capacity(2);
        let out_fd = stdout.as_ref().unwrap().as_raw_fd();
        fds.push(libc::pollfd { fd: out_fd, events: libc::POLLIN, revents: 0 });
        if let Some(s) = &stdin {
            fds.push(libc::pollfd { fd: s.as_raw_fd(), events: libc::POLLOUT, revents: 0 });
        }
        let n = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, remaining) };
        if n < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return fail(&mut child, "[Error]");
        }
        if fds.len() > 1 && fds[1].revents != 0 {
            let s = stdin.as_mut().unwrap();
            match s.write(&input[written..]) {
                Ok(k) => {
                    written += k;
                    if written >= input.len() {
                        stdin = None; // end()
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::Interrupted => {
                }
                // Commands need not consume their stdin payload.
                Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => stdin = None,
                Err(_) => return fail(&mut child, "[Error]"),
            }
        }
        if fds[0].revents != 0 {
            match stdout.as_mut().unwrap().read(&mut buf) {
                Ok(0) => stdout = None,
                Ok(k) => {
                    if k > MAX_STDOUT_BYTES - output.len() {
                        return fail(&mut child, "[Error]");
                    }
                    output.extend_from_slice(&buf[..k]);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::Interrupted => {
                }
                Err(_) => return fail(&mut child, "[Error]"),
            }
        }
    }
    drop(stdin);

    // Phase 2: stdout closed; 'close' fires once the process has exited too.
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return match exit_marker(status) {
                    None => CmdResult::Ok(finish_stdout(&output)),
                    Some(m) => CmdResult::Failed(m),
                }
            }
            Ok(None) => {}
            Err(_) => return fail(&mut child, "[Error]"),
        }
        if deadline.is_some_and(|d| Instant::now() >= d) {
            return on_timeout(&mut child, pid, &output);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// The timer: an exited command keeps its exit outcome (stdout held open by
/// a background descendant); a running one is killed.
fn on_timeout(child: &mut std::process::Child, pid: u32, output: &[u8]) -> CmdResult {
    if let Ok(Some(status)) = child.try_wait() {
        return match exit_marker(status) {
            None => CmdResult::Ok(finish_stdout(output)),
            Some(m) => CmdResult::Failed(m),
        };
    }
    kill_group(pid);
    let _ = child.try_wait();
    CmdResult::Failed("[Timeout]".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(cmd: &str, timeout: f64) -> Request<'_> {
        Request {
            command: cmd,
            input: "{\"a\":1}".into(),
            timeout_ms: timeout,
            ttl_seconds: 0.0,
            session_id: None,
            terminal_width: None,
        }
    }

    #[test]
    fn runs_and_trims() {
        assert_eq!(execute(&req("printf '  hi  \\n'", 1000.0)), CmdResult::Ok("hi".into()));
        assert_eq!(execute(&req("cat", 1000.0)), CmdResult::Ok("{\"a\":1}".into()));
    }

    #[test]
    fn failure_markers() {
        assert_eq!(execute(&req("exit 3", 1000.0)), CmdResult::Failed("[Exit: 3]".into()));
        assert_eq!(execute(&req("kill -TERM $$", 1000.0)), CmdResult::Failed("[Signal: SIGTERM]".into()));
        assert_eq!(execute(&req("sleep 5", 100.0)), CmdResult::Failed("[Timeout]".into()));
    }

    #[test]
    fn background_child_holding_stdout() {
        // the shell exits at once but a descendant keeps stdout open
        let r = execute(&req("echo out; (sleep 3 &) ; exit 0", 200.0));
        assert_eq!(r, CmdResult::Ok("out".into()));
    }

    #[test]
    fn large_input_not_read() {
        let mut r = req("true", 2000.0);
        r.input = "x".repeat(1 << 20);
        assert_eq!(execute(&r), CmdResult::Ok(String::new()));
    }

    #[test]
    fn overflow_is_error() {
        assert_eq!(execute(&req("head -c 1048577 /dev/zero", 5000.0)), CmdResult::Failed("[Error]".into()));
        assert_eq!(execute(&req("head -c 1048576 /dev/zero | tr '\\0' a", 5000.0)), CmdResult::Ok("a".repeat(16_384)));
    }

    #[test]
    fn utf16_cut() {
        let s = format!("{}😀", "a".repeat(16_383));
        assert_eq!(finish_stdout(s.as_bytes()), format!("{}\u{fffd}", "a".repeat(16_383)));
    }
}
