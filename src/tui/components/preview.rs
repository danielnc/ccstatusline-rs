//! Upstream `StatusLinePreview.tsx`: the boxed header and every non-empty line rendered in preview mode.

use crate::settings::Settings;
use crate::tui::ink::layout::*;
use crate::tui::ink::text::TextWrap;

pub struct Rendered {
    pub lines: Vec<String>,
    pub any_truncated: bool,
}

/// The preview lines, as `renderStatusLineWithInfo` produces them for each configured line.
pub fn render_lines(st: &Settings, terminal_width: usize) -> Rendered {
    crate::colors::set_chalk_level(st.color_level);
    let data = crate::status::StatusJson::default();
    let base = crate::context::RenderContext {
        is_preview: true,
        ..crate::context::RenderContext::new(&data, st, Some(terminal_width))
    };
    let pre = crate::render::pre_render_all(&st.lines, &base);
    let max_widths = crate::render::max_widths_from_pre_rendered(&pre, st);
    let (mut sep_index, mut theme_index, mut cap_index) = (0, 0, 0);
    let mut lines = Vec::new();
    let mut truncated = false;
    for (i, items) in st.lines.iter().enumerate() {
        if items.is_empty() {
            continue;
        }
        let ctx = crate::context::RenderContext {
            is_preview: true,
            line_index: Some(i),
            global_separator_index: sep_index,
            global_powerline_theme_index: theme_index,
            global_powerline_start_cap_index: Some(cap_index),
            ..crate::context::RenderContext::new(&data, st, Some(terminal_width))
        };
        let pre_line = &pre[i];
        let line = crate::render::render_status_line(items, st, &ctx, pre_line, &max_widths);
        if crate::ansi::get_visible_text(&line).contains("...") {
            truncated = true;
        }
        lines.push(line);
        sep_index += crate::render::count_separator_slots(items, pre_line);
        if st.powerline.enabled {
            cap_index += crate::render::count_powerline_start_cap_slots(items, pre_line);
        }
        if st.powerline.enabled && st.powerline.continue_theme_across_lines {
            theme_index += crate::render::count_powerline_theme_slots(pre_line);
        }
    }
    Rendered { lines, any_truncated: truncated }
}

const INDENT: &str = "  ";

/// `preparePreviewLineForTerminal`.
fn prepare(line: &str, terminal_width: usize) -> String {
    let printable = crate::ansi::strip_osc_codes(line);
    let available = terminal_width.saturating_sub(INDENT.len());
    crate::ansi::truncate_styled_text(&printable, available, true)
}

pub fn view(rendered: &Rendered, terminal_width: usize) -> El {
    let header = BoxStyle {
        border: Some(Border { color: Some("gray".into()), dim: true }),
        width: Width::Percent(100.0),
        ..BoxStyle::default().padding_left(1.0)
    };
    let mut children = vec![boxed(
        header,
        vec![text(
            TextStyle::default(),
            vec![s(">"), nested(TextStyle::dim(), vec![s(" Preview  (ctrl+s to save configuration at any time)")])],
        )],
    )];
    for line in &rendered.lines {
        // `{chalk.reset('')}` is always the empty string.
        children.push(El::Text {
            style: TextStyle::default(),
            wrap: TextWrap::Truncate,
            content: vec![s(INDENT), s(prepare(line, terminal_width))],
        });
    }
    col(children)
}
