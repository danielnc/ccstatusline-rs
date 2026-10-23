//! Port of upstream `src/utils/renderer.ts` (+ separator/theme index helpers).

use crate::ansi::{
    apply_line_gradient, apply_line_gradient_segment, get_visible_width, restore_background_after_resets,
    strip_sgr_codes, truncate_styled_text,
};
use crate::colors::{
    apply_colors, apply_parens_dim, bg_to_fg, chalk_gray, chalk_reset, color_ansi_code, powerline_theme, Dim, Level,
};
use crate::context::RenderContext;
use crate::gradient::{is_gradient_spec, parse_gradient_spec};
use crate::sanitize::sanitize_terminal_text;
use crate::settings::{Settings, WidgetItem};
use crate::widgets::{get_widget, widget_preserves_colors};

pub struct PreRendered<'a> {
    pub content: String,
    pub plain_length: usize,
    pub widget: &'a WidgetItem,
}

pub fn config_warning_badge(color_level: u8) -> String {
    apply_colors("⚠ invalid config", Some("red"), None, false, Level::from_color_level(color_level), Dim::No)
}

fn has_fg_override(s: &Settings) -> bool {
    s.override_foreground_color.as_deref().is_some_and(|c| !c.is_empty() && c != "none")
}

fn has_bg_override(s: &Settings) -> bool {
    s.override_background_color.as_deref().is_some_and(|c| !c.is_empty() && c != "none")
}

fn preserves_intrinsic_fg(item: &WidgetItem, s: &Settings) -> bool {
    widget_preserves_colors(item) && !has_fg_override(s)
}

fn padding_sides<'a>(padding: &'a str, side: &str) -> (&'a str, &'a str) {
    match side {
        "left" => (padding, ""),
        "right" => ("", padding),
        _ => (padding, padding),
    }
}

fn effective_terminal_width(detected: Option<usize>, s: &Settings, ctx: &RenderContext) -> Option<usize> {
    let detected = detected.filter(|w| *w > 0)?;
    let remaining = |r: usize| detected.saturating_sub(r);
    match s.flex_mode.as_str() {
        "full" => Some(remaining(6)),
        "full-minus-40" => Some(remaining(40)),
        "full-until-compact" => {
            if ctx.is_preview {
                return Some(remaining(6));
            }
            let pct = crate::widgets::shared::context_percentage(ctx);
            if pct >= s.compact_threshold {
                Some(remaining(40))
            } else {
                Some(remaining(6))
            }
        }
        _ => None,
    }
}

const FLEX_SENTINEL: &str = "\x01FLEX_SEP\x01";

fn is_sep_boundary(w: Option<&WidgetItem>) -> bool {
    w.is_some_and(|w| w.kind == "separator" || w.kind == "flex-separator")
}

struct PlElement<'a> {
    content: String,
    bg: Option<String>,
    fg: Option<String>,
    merges_with_next: bool,
    original_index: isize,
    widget: &'a WidgetItem,
}

#[allow(clippy::too_many_arguments)]
fn render_powerline_line(
    widgets: &[WidgetItem],
    s: &Settings,
    ctx: &RenderContext,
    line_index: usize,
    global_separator_offset: usize,
    global_theme_offset: usize,
    pre: &[PreRendered],
    max_widths: &[usize],
) -> String {
    let cfg = &s.powerline;
    let separators = &cfg.separators;
    let invert = &cfg.separator_invert_background;
    let start_caps = &cfg.start_caps;
    let end_caps = &cfg.end_caps;
    let start_cap_index = ctx.global_powerline_start_cap_index.unwrap_or(ctx.line_index.unwrap_or(line_index));
    let get_start_cap = |off: usize| -> &str {
        if start_caps.is_empty() {
            ""
        } else {
            &start_caps[(start_cap_index + off) % start_caps.len()]
        }
    };
    let get_end_cap = |off: usize| -> &str {
        if end_caps.is_empty() {
            ""
        } else {
            &end_caps[(start_cap_index + off) % end_caps.len()]
        }
    };
    let level = Level::from_color_level(s.color_level);
    let theme_colors = match cfg.theme.as_deref() {
        Some(name) if !name.is_empty() && name != "custom" => {
            powerline_theme(name).and_then(|t| t.levels.as_ref()).map(|lv| {
                &lv[match level {
                    Level::Ansi16 => 0,
                    Level::Ansi256 => 1,
                    Level::Truecolor => 2,
                }]
            })
        }
        _ => None,
    };
    let fg_gradient = parse_gradient_spec(s.override_foreground_color.as_deref());

    let filtered: Vec<usize> = (0..widgets.len()).filter(|&i| !widgets[i].is_separator()).collect();
    if filtered.is_empty() {
        return String::new();
    }
    let detected = ctx.terminal_width.or_else(crate::terminal::memoized_width);
    let terminal_width = effective_terminal_width(detected, s, ctx);

    let content_at = |j: usize| pre.get(j).is_some_and(|p| !p.content.is_empty());
    let has_next_rendered_before_sep = |orig: usize| -> bool {
        for (j, w) in widgets.iter().enumerate().skip(orig + 1) {
            if is_sep_boundary(Some(w)) {
                return false;
            }
            if content_at(j) {
                return true;
            }
        }
        false
    };
    let can_merge_with_next = |orig: Option<usize>| -> bool {
        match orig {
            None => false,
            Some(o) => widgets[o].merges() && has_next_rendered_before_sep(o),
        }
    };
    let find_prev_rendered_before_sep = |orig: usize| -> Option<usize> {
        let mut j = orig;
        while j > 0 {
            j -= 1;
            if is_sep_boundary(Some(&widgets[j])) {
                return None;
            }
            if content_at(j) {
                return Some(j);
            }
        }
        None
    };

    let mut elements: Vec<PlElement> = Vec::new();
    let mut color_index = if cfg.continue_theme_across_lines { global_theme_offset } else { 0 };
    for &orig in &filtered {
        let widget = &widgets[orig];
        let Some(pr) = pre.get(orig) else { continue };
        if pr.content.is_empty() {
            continue;
        }
        let mut text = pr.content.clone();
        let default_color = get_widget(&widget.kind).map(|w| w.default_color()).unwrap_or("white");
        let padding = s.default_padding.as_deref().unwrap_or("");
        let (lead_pad, trail_pad) = padding_sides(padding, &s.default_padding_side);
        if has_fg_override(s) && widget_preserves_colors(widget) {
            text = strip_sgr_codes(&text);
        }
        let prev_rendered = find_prev_rendered_before_sep(orig);
        let prev_widget = prev_rendered.map(|p| &widgets[p]);
        let merges_with_next = can_merge_with_next(Some(orig));
        let omit_lead = prev_widget.is_some_and(|p| p.merge_no_padding()) && can_merge_with_next(prev_rendered);
        let omit_trail = widget.merge_no_padding() && merges_with_next;
        let padded =
            format!("{}{}{}", if omit_lead { "" } else { lead_pad }, text, if omit_trail { "" } else { trail_pad });

        let mut fg = Some(widget.color.clone().unwrap_or_else(|| default_color.to_string()));
        let mut bg = widget.background_color.clone();
        let skip_fg_theme = preserves_intrinsic_fg(widget, s);
        if let Some(tc) = theme_colors {
            if !skip_fg_theme {
                fg = Some(tc.fg[color_index % tc.fg.len()].to_string());
            }
            bg = Some(tc.bg[color_index % tc.bg.len()].to_string());
            if !merges_with_next {
                color_index += 1;
            }
        }
        if let Some(o) = s.override_foreground_color.as_deref() {
            if !o.is_empty() && o != "none" && !is_gradient_spec(Some(o)) {
                fg = Some(o.to_string());
            }
        }
        elements.push(PlElement {
            content: padded,
            bg: bg.filter(|b| !b.is_empty()),
            fg,
            merges_with_next,
            original_index: orig as isize,
            widget,
        });
    }
    if elements.is_empty() {
        return String::new();
    }

    let rendered_index_of = |orig: usize| elements.iter().position(|e| e.original_index == orig as isize);
    let mut flex_after: std::collections::HashMap<usize, usize> = Default::default();
    let mut start_cap_before: std::collections::HashMap<usize, usize> = Default::default();
    let mut segment_of: std::collections::HashMap<usize, usize> = Default::default();
    let mut leading_flex = 0;
    let mut total_flex = 0;
    let mut last_rendered: Option<usize> = None;
    let mut pending_flex = 0;
    let mut segment_offset = 0;
    let mut has_segment = false;
    for (i, w) in widgets.iter().enumerate() {
        if w.kind == "separator" {
            continue;
        }
        if w.kind == "flex-separator" {
            total_flex += 1;
            match last_rendered {
                None => leading_flex += 1,
                Some(l) => {
                    pending_flex += 1;
                    *flex_after.entry(l).or_default() += 1;
                }
            }
            continue;
        }
        if let Some(ri) = rendered_index_of(i) {
            if !has_segment {
                start_cap_before.insert(ri, segment_offset);
                pending_flex = 0;
                has_segment = true;
            } else if pending_flex > 0 {
                segment_offset += 1;
                start_cap_before.insert(ri, segment_offset);
                pending_flex = 0;
            }
            segment_of.insert(ri, segment_offset);
            last_rendered = Some(ri);
        }
    }

    if cfg.auto_align {
        let mut align_pos = 0;
        let mut i = 0;
        while i < elements.len() {
            if elements[i].widget.exclude_from_auto_align == Some(true) {
                break;
            }
            let prev_merged = i > 0 && elements[i - 1].merges_with_next;
            if !prev_merged {
                if let Some(&max) = max_widths.get(align_pos) {
                    let mut combined = get_visible_width(&elements[i].content);
                    let mut j = i;
                    while j + 1 < elements.len() && elements[j].merges_with_next {
                        j += 1;
                        combined += get_visible_width(&elements[j].content);
                    }
                    if max > combined {
                        let pad = " ".repeat(max - combined);
                        elements[j].content.push_str(&pad);
                    }
                    i = j;
                }
                align_pos += 1;
            }
            i += 1;
        }
    }

    let gradient_width = match (&fg_gradient, level) {
        (Some(_), l) if l != Level::Ansi16 => elements
            .iter()
            .filter(|e| !preserves_intrinsic_fg(e.widget, s))
            .map(|e| get_visible_width(&e.content))
            .sum(),
        _ => 0,
    };
    let mut gradient_column = 0;

    let mut result = String::new();
    if leading_flex > 0 {
        result.push_str(&FLEX_SENTINEL.repeat(leading_flex));
    }
    let mut local_sep = 0;
    for i in 0..elements.len() {
        let el = &elements[i];
        let next = elements.get(i + 1);
        let should_bold = s.global_bold || el.widget.is_bold();
        let should_dim = el.widget.dim_value() == Dim::Yes;
        let needs_separator =
            i + 1 < elements.len() && !separators.is_empty() && next.is_some() && !el.merges_with_next;
        let flex_count_after = flex_after.get(&i).copied().unwrap_or(0);
        let cur_segment = segment_of.get(&i).copied().unwrap_or(0);
        let is_last = i == elements.len() - 1;
        let has_end_cap_after = !get_end_cap(cur_segment).is_empty() && (flex_count_after > 0 || is_last);

        if let Some(&off) = start_cap_before.get(&i) {
            let cap = get_start_cap(off);
            if !cap.is_empty() {
                if let Some(bg) = &el.bg {
                    let fg = bg_to_fg(Some(bg));
                    result.push_str(&color_ansi_code(fg.as_deref(), level, false));
                    result.push_str(cap);
                    result.push_str("\x1b[39m");
                } else {
                    result.push_str(cap);
                }
            }
        }

        let mut wc = String::new();
        let preserve = preserves_intrinsic_fg(el.widget, s);
        if should_bold {
            wc.push_str("\x1b[1m");
        }
        if should_dim {
            wc.push_str("\x1b[2m");
        }
        let text_gradient = if !preserve && gradient_width > 1 { fg_gradient.as_ref() } else { None };
        let segment_content = match (&el.bg, preserve) {
            (Some(bg), true) => restore_background_after_resets(&el.content, &color_ansi_code(Some(bg), level, true)),
            _ => el.content.clone(),
        };
        let styled = if el.widget.dim_value() == Dim::Parens {
            apply_parens_dim(&segment_content, should_bold)
        } else {
            segment_content
        };
        if el.fg.is_some() && !preserve && text_gradient.is_none() {
            wc.push_str(&color_ansi_code(el.fg.as_deref(), level, false));
        }
        if let Some(bg) = &el.bg {
            wc.push_str(&color_ansi_code(Some(bg), level, true));
        }
        if let Some(stops) = text_gradient {
            let r = apply_line_gradient_segment(&styled, stops, level, gradient_column, gradient_width);
            wc.push_str(&r.text);
            gradient_column = r.next_column;
        } else {
            wc.push_str(&styled);
        }
        if preserve {
            wc.push_str("\x1b[0m");
        } else {
            wc.push_str("\x1b[49m\x1b[39m");
            let restore_bold = should_dim && should_bold && (needs_separator || has_end_cap_after);
            if restore_bold {
                wc.push_str("\x1b[22;1m");
            } else if should_dim || (should_bold && !needs_separator && !has_end_cap_after) {
                wc.push_str("\x1b[22m");
            }
        }
        result.push_str(&wc);

        if flex_count_after > 0 {
            let cap = get_end_cap(cur_segment);
            if !cap.is_empty() {
                if let Some(bg) = &el.bg {
                    let fg = bg_to_fg(Some(bg));
                    result.push_str(&color_ansi_code(fg.as_deref(), level, false));
                    result.push_str(cap);
                    result.push_str("\x1b[39m");
                } else {
                    result.push_str(cap);
                }
            }
            if should_bold {
                result.push_str("\x1b[22m");
            }
            result.push_str(&FLEX_SENTINEL.repeat(flex_count_after));
            continue;
        }

        if needs_separator {
            let next = next.unwrap();
            let gi = global_separator_offset + local_sep;
            let si = gi % separators.len();
            let sep = separators.get(si).map(String::as_str).unwrap_or("\u{e0b0}");
            let inv = invert.get(si).copied().unwrap_or(false);
            let same_bg = matches!((&el.bg, &next.bg), (Some(a), Some(b)) if a == b);
            let code = |c: Option<&str>, bg: bool| color_ansi_code(c, level, bg);
            let out = match (&el.bg, &next.bg) {
                (Some(cb), Some(nb)) => {
                    let (fgc, bgc) = if inv {
                        if same_bg {
                            (code(next.fg.as_deref(), false), code(Some(cb), true))
                        } else {
                            (code(bg_to_fg(Some(nb)).as_deref(), false), code(Some(cb), true))
                        }
                    } else if same_bg {
                        (code(el.fg.as_deref(), false), code(Some(nb), true))
                    } else {
                        (code(bg_to_fg(Some(cb)).as_deref(), false), code(Some(nb), true))
                    };
                    format!("{fgc}{bgc}{sep}\x1b[39m\x1b[49m")
                }
                (Some(cb), None) => format!("{}{sep}\x1b[39m", code(bg_to_fg(Some(cb)).as_deref(), false)),
                (None, Some(nb)) => format!("{}{sep}\x1b[39m", code(bg_to_fg(Some(nb)).as_deref(), false)),
                (None, None) => sep.to_string(),
            };
            result.push_str(&out);
            if should_bold || should_dim {
                result.push_str("\x1b[22m");
            }
            local_sep += 1;
        }
    }

    {
        let last_i = elements.len() - 1;
        let last = &elements[last_i];
        let last_bold = s.global_bold || last.widget.is_bold();
        let last_dim = last.widget.dim_value() == Dim::Yes;
        let last_seg = segment_of.get(&last_i).copied().unwrap_or(0);
        let has_flex_after = flex_after.get(&last_i).copied().unwrap_or(0) > 0;
        let cap = get_end_cap(last_seg);
        if !cap.is_empty() && !has_flex_after {
            if let Some(bg) = &last.bg {
                let fg = bg_to_fg(Some(bg));
                result.push_str(&color_ansi_code(fg.as_deref(), level, false));
                result.push_str(cap);
                result.push_str("\x1b[39m");
            } else {
                result.push_str(cap);
            }
            if last_bold || last_dim {
                result.push_str("\x1b[22m");
            }
        }
    }

    if total_flex > 0 {
        if let Some(tw) = terminal_width {
            let parts: Vec<&str> = result.split(FLEX_SENTINEL).collect();
            let content: usize = parts.iter().map(|p| get_visible_width(p)).sum();
            let flex_count = parts.len() - 1;
            let total_space = tw.saturating_sub(content);
            let per = total_space.checked_div(flex_count).unwrap_or(0);
            let extra = if flex_count > 0 { total_space % flex_count } else { 0 };
            let mut nr = parts[0].to_string();
            for (i, p) in parts.iter().enumerate().skip(1) {
                let size = per + if i - 1 < extra { 1 } else { 0 };
                nr.push_str(&" ".repeat(size));
                nr.push_str(p);
            }
            result = nr;
        } else {
            result = result.split(FLEX_SENTINEL).collect::<Vec<_>>().join(" ");
        }
    }

    result.push_str(&chalk_reset(""));

    if let Some(tw) = terminal_width {
        if get_visible_width(&result) > tw {
            result = truncate_styled_text(&result, tw, true);
        }
    }
    result
}

fn format_separator(sep: &str) -> String {
    match sep {
        "|" => " | ".into(),
        " " => " ".into(),
        "," => ", ".into(),
        "-" => " - ".into(),
        other => other.to_string(),
    }
}

fn is_spacing_separator(w: Option<&WidgetItem>, default_sep: Option<&str>) -> bool {
    w.is_some_and(|w| {
        w.kind == "separator" && crate::jsnum::js_trim(w.character.as_deref().or(default_sep).unwrap_or("|")).is_empty()
    })
}

pub fn count_powerline_start_cap_slots(widgets: &[WidgetItem], pre: &[PreRendered]) -> usize {
    let mut pending = false;
    let mut has = false;
    let mut count = 0;
    for (i, w) in widgets.iter().enumerate() {
        if w.kind == "separator" {
            continue;
        }
        if w.kind == "flex-separator" {
            if has {
                pending = true;
            }
            continue;
        }
        if !pre.get(i).is_some_and(|p| !p.content.is_empty()) {
            continue;
        }
        if !has {
            has = true;
            count = 1;
        } else if pending {
            count += 1;
            pending = false;
        }
    }
    count
}

pub fn count_separator_slots(widgets: &[WidgetItem], pre: &[PreRendered]) -> usize {
    let mut count = 0;
    let mut has_prev = false;
    let mut prev_merges = false;
    for (i, w) in widgets.iter().enumerate() {
        if w.kind == "separator" {
            if has_prev {
                prev_merges = false;
            }
            continue;
        }
        if w.kind == "flex-separator" {
            has_prev = false;
            prev_merges = false;
            continue;
        }
        if !pre.get(i).is_some_and(|p| !p.content.is_empty()) {
            continue;
        }
        if has_prev && !prev_merges {
            count += 1;
        }
        has_prev = true;
        prev_merges = w.merges();
    }
    count
}

pub fn count_powerline_theme_slots(pre: &[PreRendered]) -> usize {
    let mut prev: Option<&WidgetItem> = None;
    let mut next = 0;
    for p in pre {
        let w = p.widget;
        if w.is_separator() {
            prev = None;
            continue;
        }
        if p.content.is_empty() {
            continue;
        }
        if !prev.is_some_and(|pw| pw.merges()) {
            next += 1;
        }
        prev = Some(w);
    }
    next
}

const DECORATIVE: &[&str] = &["custom-text", "custom-symbol"];

fn apply_merge_target_hiding(line: &mut [PreRendered]) {
    let mut chain_start = 0;
    for i in 0..=line.len() {
        if i < line.len() && !line[i].widget.is_separator() {
            continue;
        }
        hide_segment(&mut line[chain_start..i]);
        chain_start = i + 1;
    }
}

fn hide_segment(seg: &mut [PreRendered]) {
    let mut chain_start = 0;
    for i in 0..seg.len() {
        let links = seg[i].widget.merges() && i < seg.len() - 1;
        if links {
            continue;
        }
        let range = chain_start..i + 1;
        chain_start = i + 1;
        if range.len() < 2 {
            continue;
        }
        for pos in range.clone() {
            let el = &seg[pos];
            if !DECORATIVE.contains(&el.widget.kind.as_str())
                || !crate::widgets::shared::is_hidden(el.widget, "merge-target-hidden", false)
            {
                continue;
            }
            let target = (pos + 1..range.end)
                .find(|&c| !DECORATIVE.contains(&seg[c].widget.kind.as_str()))
                .or_else(|| (range.start..pos).rev().find(|&c| !DECORATIVE.contains(&seg[c].widget.kind.as_str())));
            if let Some(t) = target {
                if seg[t].content.is_empty() {
                    seg[pos].content.clear();
                    seg[pos].plain_length = 0;
                }
            }
        }
    }
}

pub fn pre_render_all<'a>(lines: &'a [Vec<WidgetItem>], ctx: &RenderContext) -> Vec<Vec<PreRendered<'a>>> {
    lines
        .iter()
        .map(|line| {
            let mut out: Vec<PreRendered> = line
                .iter()
                .map(|w| {
                    if w.is_separator() {
                        return PreRendered { content: String::new(), plain_length: 0, widget: w };
                    }
                    let Some(imp) = get_widget(&w.kind) else {
                        return PreRendered { content: String::new(), plain_length: 0, widget: w };
                    };
                    let text = if ctx.minimalist {
                        let mut eff = w.clone();
                        eff.raw_value = Some(true);
                        imp.render(&eff, ctx)
                    } else {
                        imp.render(w, ctx)
                    };
                    let text = sanitize_terminal_text(&text.unwrap_or_default());
                    let plain = get_visible_width(&text);
                    PreRendered { content: text, plain_length: plain, widget: w }
                })
                .collect();
            apply_merge_target_hiding(&mut out);
            out
        })
        .collect()
}

pub fn max_widths_from_pre_rendered(lines: &[Vec<PreRendered>], s: &Settings) -> Vec<usize> {
    let mut max: Vec<usize> = Vec::new();
    let padding = s.default_padding.as_deref().unwrap_or("");
    let (l, t) = padding_sides(padding, &s.default_padding_side);
    // JS string length (UTF-16 units)
    let pair = l.encode_utf16().count() + t.encode_utf16().count();
    for line in lines {
        let has_next = |orig: usize| -> bool {
            for e in line.iter().skip(orig + 1) {
                if e.widget.is_separator() {
                    return false;
                }
                if !e.content.is_empty() {
                    return true;
                }
            }
            false
        };
        let rendered: Vec<(&PreRendered, bool)> = line
            .iter()
            .enumerate()
            .map(|(i, e)| (e, e.widget.merges() && has_next(i)))
            .filter(|(e, _)| !e.widget.is_separator() && !e.content.is_empty())
            .collect();
        let mut pos = 0;
        let mut i = 0;
        while i < rendered.len() {
            let (w, _) = rendered[i];
            if w.widget.exclude_from_auto_align == Some(true) {
                break;
            }
            let mut total = w.plain_length + pair;
            let mut j = i;
            while j + 1 < rendered.len() && rendered[j].1 {
                j += 1;
                let nw = rendered[j].0;
                if rendered[j - 1].0.widget.merge_no_padding() {
                    total += nw.plain_length;
                } else {
                    total += nw.plain_length + pair;
                }
            }
            if pos >= max.len() {
                max.push(total);
            } else {
                max[pos] = max[pos].max(total);
            }
            i = j + 1;
            pos += 1;
        }
    }
    max
}

struct Element<'a> {
    content: String,
    kind: &'a str,
    widget: Option<&'a WidgetItem>,
}

pub fn render_status_line(
    widgets: &[WidgetItem],
    s: &Settings,
    ctx: &RenderContext,
    pre: &[PreRendered],
    max_widths: &[usize],
) -> String {
    let level = Level::from_color_level(s.color_level);
    if s.powerline.enabled {
        return render_powerline_line(
            widgets,
            s,
            ctx,
            ctx.line_index.unwrap_or(0),
            ctx.global_separator_index,
            ctx.global_powerline_theme_index,
            pre,
            max_widths,
        );
    }
    let apply = |text: &str, fg: Option<&str>, bg: Option<&str>, bold: bool, dim: Dim| -> String {
        let mut fgc = fg.map(String::from);
        if let Some(o) = s.override_foreground_color.as_deref() {
            if !o.is_empty() && o != "none" {
                if !is_gradient_spec(Some(o)) {
                    fgc = Some(o.to_string());
                } else if level != Level::Ansi16 {
                    fgc = None;
                }
            }
        }
        let mut bgc = bg.map(String::from);
        if has_bg_override(s) {
            bgc = s.override_background_color.clone();
        }
        apply_colors(text, fgc.as_deref(), bgc.as_deref(), s.global_bold || bold, level, dim)
    };

    let detected = ctx.terminal_width.or_else(crate::terminal::memoized_width);
    let terminal_width = effective_terminal_width(detected, s, ctx);
    let default_sep_setting = s.default_separator.as_deref();

    let mut elements: Vec<Element> = Vec::new();
    let mut has_flex = false;
    for (i, w) in widgets.iter().enumerate() {
        if w.kind == "separator" {
            let mut content_before: Option<usize> = None;
            let mut replaces_spacing = false;
            let mut crossed_empty = false;
            let mut j = i;
            while j > 0 {
                j -= 1;
                let pw = &widgets[j];
                if pw.kind == "separator" {
                    if !is_spacing_separator(Some(pw), default_sep_setting) {
                        break;
                    }
                    replaces_spacing = true;
                    continue;
                }
                if pw.kind == "flex-separator" {
                    break;
                }
                if pre.get(j).is_some_and(|p| !p.content.is_empty()) {
                    if !pw.merges() || !crossed_empty {
                        content_before = Some(j);
                    }
                    break;
                }
                crossed_empty = true;
            }
            let Some(cb) = content_before else { continue };
            if replaces_spacing {
                while is_spacing_separator(elements.last().and_then(|e| e.widget), default_sep_setting) {
                    elements.pop();
                }
            }
            let sep_char = w.character.as_deref().or(default_sep_setting).unwrap_or("|");
            let formatted = format_separator(sep_char);
            let mut color = w.color.clone().unwrap_or_else(|| "gray".into());
            let mut bg = w.background_color.clone();
            let mut bold = w.is_bold();
            let mut dim = w.dim_value();
            if s.inherit_separator_colors
                && w.color.as_deref().is_none_or(str::is_empty)
                && w.background_color.as_deref().is_none_or(str::is_empty)
            {
                let pw = &widgets[cb];
                color =
                    pw.color.clone().filter(|c| !c.is_empty()).unwrap_or_else(|| {
                        get_widget(&pw.kind).map(|x| x.default_color()).unwrap_or("white").to_string()
                    });
                bg = pw.background_color.clone();
                bold = pw.is_bold();
                dim = pw.dim_value();
            }
            // `widget.color ?? 'gray'`: an explicit "" stays "" (no color)
            elements.push(Element {
                content: apply(&formatted, Some(&color), bg.as_deref(), bold, dim),
                kind: "separator",
                widget: Some(w),
            });
            continue;
        }
        if w.kind == "flex-separator" {
            elements.push(Element { content: String::new(), kind: "flex-separator", widget: Some(w) });
            has_flex = true;
            continue;
        }
        let Some(p) = pre.get(i) else { continue };
        if p.content.is_empty() {
            continue;
        }
        let default_color = get_widget(&w.kind).map(|x| x.default_color()).unwrap_or("white");
        if widget_preserves_colors(w) {
            let mut out = p.content.clone();
            if let Some(mw) = w.max_width {
                if mw > 0.0 && get_visible_width(&out) as f64 > mw {
                    out = truncate_styled_text(&out, js_width_arg(mw), false);
                }
            }
            if has_fg_override(s) {
                out = strip_sgr_codes(&out);
            }
            elements.push(Element {
                content: apply(&out, None, w.background_color.as_deref(), w.is_bold(), w.dim_value()),
                kind: &w.kind,
                widget: Some(w),
            });
        } else {
            let fg = w.color.clone().unwrap_or_else(|| default_color.to_string());
            elements.push(Element {
                content: apply(&p.content, Some(&fg), w.background_color.as_deref(), w.is_bold(), w.dim_value()),
                kind: &w.kind,
                widget: Some(w),
            });
        }
    }
    if elements.is_empty() {
        return String::new();
    }
    while elements.last().is_some_and(|e| e.kind == "separator") {
        elements.pop();
    }
    if terminal_width.is_none() {
        let mut i = elements.len();
        while i > 0 {
            i -= 1;
            if elements[i].kind != "separator" || !is_spacing_separator(elements[i].widget, default_sep_setting) {
                continue;
            }
            let prev_flex = i > 0 && elements[i - 1].kind == "flex-separator";
            let next_flex = elements.get(i + 1).is_some_and(|e| e.kind == "flex-separator");
            if prev_flex || next_flex {
                elements.remove(i);
            }
        }
    }

    let padding = s.default_padding.as_deref().unwrap_or("");
    let (lead, trail) = padding_sides(padding, &s.default_padding_side);
    let default_sep = match default_sep_setting {
        Some(d) if !d.is_empty() => format_separator(d),
        _ => String::new(),
    };
    let has_color_override = has_bg_override(s) || has_fg_override(s);
    let mut finals: Vec<Option<String>> = Vec::new();
    for (index, el) in elements.iter().enumerate() {
        let prev = if index > 0 { elements.get(index - 1) } else { None };
        let should_add_sep = !default_sep.is_empty()
            && index > 0
            && el.kind != "flex-separator"
            && prev.is_none_or(|p| p.kind != "flex-separator")
            && !prev.and_then(|p| p.widget).is_some_and(|w| w.merges());
        if should_add_sep {
            if s.inherit_separator_colors {
                if let Some(pw) = prev.and_then(|p| p.widget) {
                    let mut color = pw.color.clone().filter(|c| !c.is_empty());
                    if color.is_none() && !pw.is_separator() {
                        color = Some(get_widget(&pw.kind).map(|x| x.default_color()).unwrap_or("white").to_string());
                    }
                    finals.push(Some(apply(
                        &default_sep,
                        color.as_deref(),
                        pw.background_color.as_deref(),
                        pw.is_bold(),
                        pw.dim_value(),
                    )));
                } else {
                    finals.push(Some(default_sep.clone()));
                }
            } else if has_color_override {
                finals.push(Some(apply(&default_sep, None, None, false, Dim::No)));
            } else {
                finals.push(Some(default_sep.clone()));
            }
        }
        if el.kind == "flex-separator" {
            finals.push(None);
        } else if el.kind == "separator" {
            finals.push(Some(el.content.clone()));
        } else {
            let next = elements.get(index + 1);
            let omit_lead = prev.and_then(|p| p.widget).is_some_and(|w| w.merge_no_padding());
            let omit_trail = el.widget.is_some_and(|w| w.merge_no_padding())
                && next.is_some_and(|n| n.kind != "separator" && n.kind != "flex-separator");
            let elbg = el.widget.and_then(|w| w.background_color.as_deref()).filter(|b| !b.is_empty());
            if !padding.is_empty() && (elbg.is_some() || has_color_override) {
                let l =
                    if omit_lead || lead.is_empty() { String::new() } else { apply(lead, None, elbg, false, Dim::No) };
                let t = if omit_trail || trail.is_empty() {
                    String::new()
                } else {
                    apply(trail, None, elbg, false, Dim::No)
                };
                finals.push(Some(format!("{l}{}{t}", el.content)));
            } else if !padding.is_empty() {
                let l = if omit_lead || lead.is_empty() { String::new() } else { chalk_reset(lead) };
                let t = if omit_trail || trail.is_empty() { String::new() } else { chalk_reset(trail) };
                finals.push(Some(format!("{l}{}{t}", el.content)));
            } else {
                finals.push(Some(el.content.clone()));
            }
        }
    }

    let mut line = if let (true, Some(tw)) = (has_flex, terminal_width) {
        let mut parts: Vec<Vec<&str>> = vec![vec![]];
        for e in &finals {
            match e {
                None => parts.push(vec![]),
                Some(s) => parts.last_mut().unwrap().push(s),
            }
        }
        let total: usize = parts.iter().map(|p| get_visible_width(&p.concat())).sum();
        let flex_count = parts.len() - 1;
        let space = tw.saturating_sub(total);
        let per = space.checked_div(flex_count).unwrap_or(0);
        let extra = if flex_count > 0 { space % flex_count } else { 0 };
        let mut out = String::new();
        for (i, p) in parts.iter().enumerate() {
            out.push_str(&p.concat());
            if i < parts.len() - 1 {
                out.push_str(&" ".repeat(per + if i < extra { 1 } else { 0 }));
            }
        }
        out
    } else if has_flex {
        finals.iter().map(|e| e.clone().unwrap_or_else(|| chalk_gray(" | "))).collect::<String>()
    } else {
        finals.iter().flatten().map(String::as_str).collect::<String>()
    };

    let max = terminal_width.or(detected);
    if let Some(mw) = max {
        if (terminal_width.is_some() || mw > 0) && get_visible_width(&line) > mw {
            line = truncate_styled_text(&line, mw, true);
        }
    }
    if let Some(stops) = parse_gradient_spec(s.override_foreground_color.as_deref()) {
        line = apply_line_gradient(&line, &stops, level);
    }
    line
}

/// A JS number used as a column count in truncation (`maxWidth` may be fractional).
fn js_width_arg(v: f64) -> usize {
    // truncateStyledText compares widths against the raw number; widths are
    // integers, so `w + cw > target` with fractional target == floor semantics
    if v <= 0.0 {
        0
    } else {
        v.floor() as usize
    }
}
