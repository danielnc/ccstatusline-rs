//! The interactive config editor (upstream `src/tui/`), run when stdin is a terminal.

mod app;
mod claude;
mod components;
mod ink;
mod install;
mod intl;
mod intl_tables;
mod open_url;
pub(crate) mod random;
pub mod screens;
mod text_cursor;
pub mod widget_editors;

/// JavaScript truthiness of a JSON value (`undefined` properties are falsy).
pub fn truthy(v: &serde_json::Value) -> bool {
    use serde_json::Value;
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Value::String(s) => !s.is_empty() && !crate::jsobj::is_undefined(v),
        Value::Array(_) | Value::Object(_) => true,
    }
}

use ink::layout::{plain, render_frame, Chalks};
use ink::term::{self, Read};

/// `runTUI()`: clear the screen, then render the App inline until it exits.
pub fn run(mut store: crate::settings::ConfigStore) {
    use std::io::{IsTerminal, Write};
    {
        let mut out = std::io::stdout().lock();
        let _ = out.write_all(b"\x1b[2J\x1b[H");
        let _ = out.flush();
    }
    let tty = std::io::stdout().is_terminal();
    let level = ink::chalk::supports_color_level(ink::chalk::Flavor::Chalk5, tty);
    let chalks = Chalks { ink: ink::chalk::Chalk::new(level), gradient: ink::chalk::Chalk::new(level) };
    // The app's own chalk, at import: COLOR_MAP's entries are built at this level and never rebuilt.
    crate::colors::set_color_map_level(ink::chalk::supports_color_level(ink::chalk::Flavor::Chalk6, tty));
    let (mut columns, mut rows) = term::size().unwrap_or((80, usize::MAX));
    let mut writer = term::Writer::default();

    // The first frame, before the App's effects have loaded anything.
    let (out, height) = render_frame(&chalks, &plain("Loading settings..."), columns);
    writer.render(&out, height, rows);
    let mut raw = term::RawMode::enable();

    store.captured = Some(Vec::new());
    let mut app = app::App::new(store, columns);
    for message in app.take_messages() {
        // Ink's patched console: clear the frame, print, redraw.
        writer.write_through(&format!("{message}\n"));
    }

    loop {
        app.tick();
        let frame = app.view();
        let (out, height) = render_frame(&chalks, &frame, columns);
        writer.render(&out, height, rows);
        if app.exit == app::Exit::AfterRender {
            break;
        }
        let timeout = app.next_timer().map(|d| d.as_millis().min(i32::MAX as u128) as i32 + 1);
        match term::read_chunk(timeout) {
            Read::Chunk(s) => {
                app.handle(&ink::keys::to_input(&s));
                for message in app.take_messages() {
                    writer.write_through(&format!("{message}\n"));
                }
                if app.exit == app::Exit::Now {
                    break;
                }
            }
            Read::Timeout => {}
            Read::Interrupted => {
                if term::take_resized() {
                    if let Some((c, r)) = term::size() {
                        columns = c;
                        rows = r;
                        app.terminal_width = c;
                    }
                }
            }
            Read::Eof => break,
        }
    }
    writer.done();
    raw.disable();
}
