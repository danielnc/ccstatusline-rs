//! ccstatusline-rs: a native drop-in replacement for ccstatusline.
#![allow(dead_code)]

mod ansi;
mod cachefile;
mod claude_config;
mod clock;
mod colors;
mod context;
mod git;
mod gradient;
mod hooks;
mod jsjson;
mod jsnum;
mod jsobj;
mod migrations;
mod misc;
mod render;
mod sanitize;
mod settings;
mod status;
mod terminal;
mod timefmt;
mod transcript;
mod tui;
mod unicode_tables;
mod usage;
mod widgets;
mod width;

use std::io::{IsTerminal, Read, Write};

/// The upstream release this binary is at parity with.
pub const UPSTREAM_VERSION: &str = "2.2.32";

/// Read piped stdin to EOF, like upstream, but never hang: upstream waits for
/// EOF forever when the writer keeps the pipe open (upstream #485), leaving
/// orphaned processes. Here, once a complete JSON value has arrived and the
/// pipe has been quiet for a moment, or after an overall deadline, we stop.
fn read_stdin() -> Option<String> {
    if std::io::stdin().is_terminal() {
        return None;
    }
    const QUIET_MS: i32 = 100;
    const DEADLINE_MS: u128 = 3000;
    let start = std::time::Instant::now();
    let mut buf = Vec::new();
    let mut chunk = [0u8; 65536];
    let mut stdin = std::io::stdin().lock();
    loop {
        let elapsed = start.elapsed().as_millis();
        if elapsed >= DEADLINE_MS {
            break;
        }
        let complete = !buf.is_empty() && serde_json::from_slice::<serde::de::IgnoredAny>(&buf).is_ok();
        let wait = if complete { QUIET_MS } else { (DEADLINE_MS - elapsed) as i32 };
        let mut pfd = libc::pollfd { fd: 0, events: libc::POLLIN, revents: 0 };
        let ready = unsafe { libc::poll(&mut pfd, 1, wait) };
        if ready < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            break;
        }
        if ready == 0 {
            if complete {
                break;
            }
            continue;
        }
        match stdin.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    Some(String::from_utf8_lossy(&buf).into_owned())
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();

    if widgets::git::handle_refresh_cli(&args) || usage::handle_refresh_cli(&args) {
        return;
    }

    if args.iter().any(|a| a == "--version") {
        println!("{UPSTREAM_VERSION}");
        std::process::exit(0);
    }

    let mut config_path: Option<String> = None;
    if let Some(idx) = args.iter().position(|a| a == "--config") {
        match args.get(idx + 1) {
            Some(p) if !p.starts_with("--") => {
                config_path = Some(p.clone());
                args.drain(idx..idx + 2);
            }
            _ => {
                eprintln!("--config requires a file path argument");
                std::process::exit(1);
            }
        }
    }
    let mut store = settings::ConfigStore::new(config_path.as_deref());

    if args.iter().any(|a| a == "--hook") {
        let input = read_stdin();
        hooks::handle_hook_input(input.as_deref());
        return;
    }

    if !std::io::stdin().is_terminal() {
        let input = read_stdin().unwrap_or_default();
        if crate::jsnum::js_trim(&input).is_empty() {
            eprintln!("No input received");
            std::process::exit(1);
        }
        let value: serde_json::Value = match serde_json::from_str(&input) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("Error parsing JSON: {e}");
                std::process::exit(1);
            }
        };
        let data = match status::parse_status(value) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("Invalid status JSON format: {e}");
                std::process::exit(1);
            }
        };
        render_multiple_lines(&data, &mut store);
    } else {
        let st = store.load();
        if st.updatemessage.is_some() {
            let mut st = st.clone();
            st.updatemessage = None;
            store.save(&st);
        }
        if tui::enabled() {
            tui::run(store);
            return;
        }
        eprintln!(
            "ccstatusline-rs: the interactive editor is not built yet; edit {} with `npx ccstatusline@latest` (same file).",
            store.path.display()
        );
    }
}

fn render_multiple_lines(data: &status::StatusJson, store: &mut settings::ConfigStore) {
    let st = store.load();
    let config_error = store.load_error;
    colors::set_chalk_level(st.color_level);

    let width = terminal::terminal_width(data.session_id.as_deref(), st.terminal_width_cache_ttl_seconds);
    let base = context::RenderContext::new(data, &st, width);

    let pre = render::pre_render_all(&st.lines, &base);
    let max_widths = render::max_widths_from_pre_rendered(&pre, &st);

    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());
    let mut sep_index = 0;
    let mut theme_index = 0;
    let mut cap_index = 0;
    let mut badge_done = false;
    for (i, line_items) in st.lines.iter().enumerate() {
        if line_items.is_empty() {
            continue;
        }
        let ctx = context::RenderContext {
            line_index: Some(i),
            global_separator_index: sep_index,
            global_powerline_theme_index: theme_index,
            global_powerline_start_cap_index: Some(cap_index),
            ..context::RenderContext::new(data, &st, width)
        };
        let pre_line = &pre[i];
        let mut line = render::render_status_line(line_items, &st, &ctx, pre_line, &max_widths);
        let visible = ansi::get_visible_text(&line);
        if !jsnum::js_trim(&visible).is_empty() {
            if config_error.is_some() && !badge_done {
                line = format!("{} | {line}", render::config_warning_badge(st.color_level));
                badge_done = true;
            }
            let output = sanitize::sanitize_terminal_text(&line).replace(' ', "\u{a0}");
            let _ = writeln!(out, "\x1b[0m{output}");
            sep_index += render::count_separator_slots(line_items, pre_line);
            if st.powerline.enabled {
                cap_index += render::count_powerline_start_cap_slots(line_items, pre_line);
            }
            if st.powerline.enabled && st.powerline.continue_theme_across_lines {
                theme_index += render::count_powerline_theme_slots(pre_line);
            }
        }
    }
    if config_error.is_some() && !badge_done {
        let _ = writeln!(out, "\x1b[0m{}", render::config_warning_badge(st.color_level).replace(' ', "\u{a0}"));
    }
    if let Some(um) = &st.updatemessage {
        let msg = um.message.clone().flatten();
        let rem = um.remaining.flatten();
        if let (Some(msg), Some(rem)) = (msg, rem) {
            if !jsnum::js_trim(&msg).is_empty() && rem > 0.0 {
                let _ = writeln!(out, "{}", sanitize::sanitize_terminal_text(&msg));
                let _ = out.flush();
                let mut next = st.clone();
                if rem - 1.0 <= 0.0 {
                    next.updatemessage = None;
                } else {
                    next.updatemessage =
                        Some(settings::UpdateMessage { message: um.message.clone(), remaining: Some(Some(rem - 1.0)) });
                }
                store.save(&next);
            }
        }
    }
    let _ = out.flush();
}
