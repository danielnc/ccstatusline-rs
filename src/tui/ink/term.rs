//! The terminal side of Ink: raw mode as libuv sets it, the window size, resize notifications, reading key
//! chunks, and Ink's frame writer (log-update plus Ink's own clear-and-redraw rule for tall frames).

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

static RESIZED: AtomicBool = AtomicBool::new(false);

extern "C" fn on_winch(_: libc::c_int) {
    RESIZED.store(true, Ordering::Relaxed);
}

pub struct RawMode {
    saved: Option<libc::termios>,
}

impl RawMode {
    /// libuv's `UV_TTY_MODE_RAW` on stdin.
    pub fn enable() -> RawMode {
        unsafe {
            let mut t: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(0, &mut t) != 0 {
                return RawMode { saved: None };
            }
            let saved = t;
            t.c_iflag &= !(libc::BRKINT | libc::ICRNL | libc::INPCK | libc::ISTRIP | libc::IXON);
            t.c_oflag |= libc::ONLCR;
            t.c_cflag |= libc::CS8;
            t.c_lflag &= !(libc::ECHO | libc::ICANON | libc::IEXTEN | libc::ISIG);
            t.c_cc[libc::VMIN] = 1;
            t.c_cc[libc::VTIME] = 0;
            libc::tcsetattr(0, libc::TCSADRAIN, &t);
            libc::signal(libc::SIGWINCH, on_winch as *const () as libc::sighandler_t);
            RawMode { saved: Some(saved) }
        }
    }

    pub fn disable(&mut self) {
        if let Some(t) = self.saved.take() {
            unsafe {
                libc::tcsetattr(0, libc::TCSADRAIN, &t);
            }
        }
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        self.disable();
    }
}

/// `process.stdout.columns` / `rows`, or None when stdout is not a terminal.
pub fn size() -> Option<(usize, usize)> {
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        if libc::ioctl(1, libc::TIOCGWINSZ, &mut ws) == 0 && ws.ws_col > 0 {
            Some((ws.ws_col as usize, ws.ws_row as usize))
        } else {
            None
        }
    }
}

pub fn take_resized() -> bool {
    RESIZED.swap(false, Ordering::Relaxed)
}

pub enum Read {
    Chunk(String),
    Timeout,
    Interrupted,
    Eof,
}

/// Waits up to `timeout_ms` (None: forever) for stdin and returns what one read gives, like a Node `readable`
/// chunk.
pub fn read_chunk(timeout_ms: Option<i32>) -> Read {
    read_chunk_from(0, timeout_ms)
}

/// A signal (SIGWINCH) interrupting the wait is `Interrupted`; any other error ends input, as Node ends the stdin
/// stream, rather than failing again on every loop (EIO, EBADF).
fn read_error() -> Read {
    match std::io::Error::last_os_error().raw_os_error() {
        Some(libc::EINTR | libc::EAGAIN) => Read::Interrupted,
        _ => Read::Eof,
    }
}

fn read_chunk_from(fd: libc::c_int, timeout_ms: Option<i32>) -> Read {
    let mut pfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
    let ready = unsafe { libc::poll(&mut pfd, 1, timeout_ms.unwrap_or(-1)) };
    if ready < 0 {
        return read_error();
    }
    if ready == 0 {
        return Read::Timeout;
    }
    let mut buf = [0u8; 65536];
    let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
    if n < 0 {
        return read_error();
    }
    if n == 0 {
        return Read::Eof;
    }
    Read::Chunk(String::from_utf8_lossy(&buf[..n as usize]).into_owned())
}

fn erase_lines(count: usize) -> String {
    let mut s = String::new();
    for i in 0..count {
        s.push_str("\x1b[2K");
        if i < count - 1 {
            s.push_str("\x1b[1A");
        }
    }
    if count > 0 {
        s.push_str("\x1b[G");
    }
    s
}

const CLEAR_TERMINAL: &str = "\x1b[2J\x1b[3J\x1b[H";

/// Ink's `onRender` and log-update, for an app without `<Static>` output.
#[derive(Default)]
pub struct Writer {
    previous_line_count: usize,
    previous_output: String,
    last_output: String,
    last_output_height: usize,
    cursor_hidden: bool,
}

impl Writer {
    pub fn render(&mut self, output: &str, output_height: usize, rows: usize) {
        let mut out = std::io::stdout().lock();
        if self.last_output_height >= rows {
            let _ = writeln!(out, "{CLEAR_TERMINAL}{output}");
            self.last_output = output.to_string();
            self.last_output_height = output_height;
            // log.sync(output)
            self.previous_output = format!("{output}\n");
            self.previous_line_count = self.previous_output.split('\n').count();
            let _ = out.flush();
            return;
        }
        if output != self.last_output {
            if !self.cursor_hidden {
                let _ = write!(out, "\x1b[?25l");
                self.cursor_hidden = true;
            }
            let frame = format!("{output}\n");
            if frame != self.previous_output {
                let _ = write!(out, "{}{frame}", erase_lines(self.previous_line_count));
                self.previous_line_count = frame.split('\n').count();
                self.previous_output = frame;
            }
        }
        self.last_output = output.to_string();
        self.last_output_height = output_height;
        let _ = out.flush();
    }

    /// Ink's `writeToStderr` (console output while the app runs): clear the frame, print, draw it again.
    pub fn write_through(&mut self, data: &str) {
        let mut out = std::io::stdout().lock();
        let _ = write!(out, "{}{data}", erase_lines(self.previous_line_count));
        self.previous_output.clear();
        self.previous_line_count = 0;
        let last = self.last_output.clone();
        let frame = format!("{last}\n");
        let _ = write!(out, "{frame}");
        self.previous_line_count = frame.split('\n').count();
        self.previous_output = frame;
        let _ = out.flush();
    }

    /// `log.done()` and the App's unmount: show the cursor again.
    pub fn done(&mut self) {
        let mut out = std::io::stdout().lock();
        let _ = write!(out, "\x1b[?25h");
        let _ = out.flush();
        self.previous_output.clear();
        self.previous_line_count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failing_read_ends_input() {
        // Far above the open-file limit, so never open: poll reports it ready (POLLNVAL) and read fails with EBADF.
        assert!(matches!(read_chunk_from(1_000_000, Some(0)), Read::Eof));
    }
}
