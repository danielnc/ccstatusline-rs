//! `execFileSync` equivalent with a timeout, plus a non-blocking variant so
//! several git commands can run at once.

use std::io;
use std::process::{Command, Output, Stdio};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

pub struct Cmd<'a> {
    pub prog: &'a str,
    pub args: Vec<String>,
    pub cwd: Option<&'a str>,
    pub env: Vec<(String, String)>,
    pub timeout_ms: u64,
    /// stdio[2] = 'pipe' (kept for the error text) instead of 'ignore'
    pub capture_stderr: bool,
}

/// What execFileSync throws: `status` is the exit code (None for spawn
/// errors, timeouts and signals); `message` follows node's wording.
#[derive(Debug)]
pub struct Failure {
    pub status: Option<i32>,
    pub message: String,
    pub stderr: String,
}

pub struct Pending {
    rx: Receiver<io::Result<Output>>,
    pid: u32,
    deadline: Instant,
    prog: String,
    cmdline: String,
}

impl<'a> Cmd<'a> {
    pub fn new(prog: &'a str, args: Vec<String>, cwd: Option<&'a str>, timeout_ms: u64) -> Self {
        Cmd { prog, args, cwd, env: Vec::new(), timeout_ms, capture_stderr: false }
    }

    pub fn spawn(&self) -> Result<Pending, Failure> {
        let mut c = Command::new(self.prog);
        c.args(&self.args).stdin(Stdio::null()).stdout(Stdio::piped());
        c.stderr(if self.capture_stderr { Stdio::piped() } else { Stdio::null() });
        if let Some(d) = self.cwd {
            c.current_dir(d);
        }
        for (k, v) in &self.env {
            c.env(k, v);
        }
        let child = c.spawn().map_err(|e| Failure {
            status: None,
            message: format!("spawnSync {} {}", self.prog, io_code(&e)),
            stderr: String::new(),
        })?;
        let pid = child.id();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(child.wait_with_output());
        });
        Ok(Pending {
            rx,
            pid,
            deadline: Instant::now() + Duration::from_millis(self.timeout_ms),
            prog: self.prog.to_string(),
            cmdline: std::iter::once(self.prog.to_string())
                .chain(self.args.iter().cloned())
                .collect::<Vec<_>>()
                .join(" "),
        })
    }

    pub fn run(&self) -> Result<String, Failure> {
        self.spawn()?.wait()
    }
}

impl Pending {
    /// stdout decoded as UTF-8 (lossy), or the failure execFileSync throws.
    pub fn wait(self) -> Result<String, Failure> {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        let out = match self.rx.recv_timeout(remaining) {
            Ok(Ok(o)) => o,
            Ok(Err(e)) => {
                return Err(Failure {
                    status: None,
                    message: format!("spawnSync {} {}", self.prog, io_code(&e)),
                    stderr: String::new(),
                })
            }
            Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => {
                unsafe {
                    libc::kill(self.pid as libc::pid_t, libc::SIGTERM);
                }
                return Err(Failure {
                    status: None,
                    message: format!("spawnSync {} ETIMEDOUT", self.prog),
                    stderr: String::new(),
                });
            }
        };
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        if out.status.success() {
            return Ok(stdout);
        }
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        let mut message = format!("Command failed: {}", self.cmdline);
        if !stderr.is_empty() {
            message.push('\n');
            message.push_str(&stderr);
        }
        Err(Failure { status: out.status.code(), message, stderr })
    }
}

fn io_code(e: &io::Error) -> &'static str {
    match e.kind() {
        io::ErrorKind::NotFound => "ENOENT",
        io::ErrorKind::PermissionDenied => "EACCES",
        _ => "EIO",
    }
}

/// `spawn(prog, args, { detached: true, stdio: 'ignore' }).unref()`
pub fn spawn_detached(prog: &std::path::Path, args: &[String]) -> io::Result<()> {
    use std::os::unix::process::CommandExt;
    let mut c = Command::new(prog);
    c.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    unsafe {
        c.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    c.spawn().map(|_| ())
}
