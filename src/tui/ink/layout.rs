//! The element tree upstream's screens build (Ink's `<Box>` and `<Text>`), the part of yoga's flexbox that
//! those props reach, and Ink's `renderNodeToOutput`.
//!
//! Yoga is ported for what upstream uses: row and column directions, `flexShrink` (Ink's default 1) with
//! `flexGrow` 0, margins, `paddingLeft`, borders, point and percent widths, `gap`, stretch alignment, and yoga's
//! pixel-grid rounding (including its text-node rule). Like yoga it computes in `f32`.

use super::chalk::Chalk;
use super::output::Output;
use super::text::{measure_text, widest_line, wrap_text, TextWrap};

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Dir {
    #[default]
    Row,
    Column,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Width {
    #[default]
    Auto,
    Points(f32),
    Percent(f32),
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Edges {
    pub top: f32,
    pub bottom: f32,
    pub left: f32,
    pub right: f32,
}

#[derive(Clone, Debug, Default)]
pub struct Border {
    pub color: Option<String>,
    pub dim: bool,
}

#[derive(Clone, Debug, Default)]
pub struct BoxStyle {
    pub dir: Dir,
    pub margin: Edges,
    pub padding: Edges,
    pub width: Width,
    pub gap: f32,
    /// `borderStyle='round'` (the only border upstream draws).
    pub border: Option<Border>,
}

/// A `<Text>`'s own props, applied by its transform in Ink's order.
#[derive(Clone, Debug, Default)]
pub struct TextStyle {
    pub color: Option<String>,
    pub background: Option<String>,
    pub dim: bool,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub inverse: bool,
}

#[derive(Clone, Debug)]
pub enum Transform {
    Style(TextStyle),
    /// ink-gradient's `<Gradient>`: strip ANSI, then gradient-string's `multiline` with these stops.
    Gradient(&'static [&'static str]),
}

/// Text content: strings and nested `<Text>`s (Ink's virtual text nodes).
#[derive(Clone, Debug)]
pub enum Inline {
    Str(String),
    Nested(Transform, Vec<Inline>),
}

#[derive(Clone, Debug)]
pub enum El {
    Box(BoxStyle, Vec<El>),
    Text { style: TextStyle, wrap: TextWrap, content: Vec<Inline> },
}

/// Everything a transform needs: Ink's chalk and gradient-string's chalk (both at supports-color's level).
#[derive(Clone, Copy, Debug)]
pub struct Chalks {
    pub ink: Chalk,
    pub gradient: Chalk,
}

pub fn apply_style(ch: &Chalk, st: &TextStyle, mut s: String) -> String {
    if st.dim {
        s = ch.style_text("dim", &s);
    }
    if let Some(c) = &st.color {
        s = ch.colorize(&s, c, false);
    }
    if let Some(c) = &st.background {
        s = ch.colorize(&s, c, true);
    }
    for (on, name) in [
        (st.bold, "bold"),
        (st.italic, "italic"),
        (st.underline, "underline"),
        (st.strikethrough, "strikethrough"),
        (st.inverse, "inverse"),
    ] {
        if on {
            s = ch.style_text(name, &s);
        }
    }
    s
}

fn apply_transform(chalks: &Chalks, t: &Transform, s: String) -> String {
    match t {
        Transform::Style(st) => apply_style(&chalks.ink, st, s),
        Transform::Gradient(stops) => super::gradient::multiline(&chalks.gradient, stops, &super::text::strip_ansi(&s)),
    }
}

/// Ink's `squashTextNodes`: nested texts are transformed before they are joined.
pub fn squash(chalks: &Chalks, content: &[Inline]) -> String {
    let mut out = String::new();
    for c in content {
        match c {
            Inline::Str(s) => out.push_str(s),
            Inline::Nested(t, inner) => {
                let text = squash(chalks, inner);
                if !text.is_empty() {
                    out.push_str(&apply_transform(chalks, t, text));
                }
            }
        }
    }
    out
}

// --- Layout ------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Exactly,
    AtMost,
}

/// A laid-out node: position relative to its parent and size, before and after rounding.
#[derive(Clone, Debug, Default)]
struct Layout {
    left: f32,
    top: f32,
    width: f32,
    height: f32,
    children: Vec<Layout>,
}

fn box_padding_border(st: &BoxStyle) -> (f32, f32, f32, f32) {
    let b = if st.border.is_some() { 1.0 } else { 0.0 };
    (st.padding.left + b, st.padding.right + b, st.padding.top + b, st.padding.bottom + b)
}

fn margins(el: &El) -> Edges {
    match el {
        El::Box(st, _) => st.margin,
        El::Text { .. } => Edges::default(),
    }
}

fn fixed_width(el: &El, owner_width: f32) -> Option<f32> {
    match el {
        El::Box(st, _) => match st.width {
            Width::Auto => None,
            Width::Points(p) => Some(p),
            Width::Percent(p) => Some(owner_width * p / 100.0),
        },
        El::Text { .. } => None,
    }
}

struct Ctx<'a> {
    chalks: &'a Chalks,
}

fn measure_text_node(ctx: &Ctx, content: &[Inline], wrap: TextWrap, width: f32) -> (f32, f32) {
    let text = squash(ctx.chalks, content);
    let (w, h) = measure_text(&text);
    let width64 = width as f64;
    if (w as f64) <= width64 {
        return (w as f32, h as f32);
    }
    if w >= 1 && width64 > 0.0 && width64 < 1.0 {
        return (w as f32, h as f32);
    }
    let (w, h) = measure_text(&wrap_text(&text, width64, wrap));
    (w as f32, h as f32)
}

/// Lays out `el` given the space its parent offers. `owner_width` resolves percentages.
fn layout(ctx: &Ctx, el: &El, avail: f32, mode: Mode, owner_width: f32) -> Layout {
    match el {
        El::Text { content, wrap, .. } => {
            let (mw, mh) = measure_text_node(ctx, content, *wrap, avail);
            Layout { width: if mode == Mode::Exactly { avail } else { mw }, height: mh, ..Default::default() }
        }
        El::Box(st, children) => {
            let (pl, pr, pt, pb) = box_padding_border(st);
            let (width, mode) = match fixed_width(el, owner_width) {
                Some(w) if mode != Mode::Exactly => {
                    (w.min(if mode == Mode::AtMost { f32::INFINITY } else { w }), Mode::Exactly)
                }
                _ => (avail, mode),
            };
            let width = width.max(pl + pr);
            let inner = (width - pl - pr).max(0.0);
            let mut kids: Vec<Layout> = Vec::with_capacity(children.len());
            let (content_w, content_h);
            match st.dir {
                Dir::Column => {
                    let mut y = pt;
                    let mut max_w: f32 = 0.0;
                    for (i, c) in children.iter().enumerate() {
                        let m = margins(c);
                        let fixed = fixed_width(c, inner);
                        let (cw, cmode) = match fixed {
                            Some(w) => (w, Mode::Exactly),
                            None if mode == Mode::Exactly => ((inner - m.left - m.right).max(0.0), Mode::Exactly),
                            None => ((inner - m.left - m.right).max(0.0), Mode::AtMost),
                        };
                        let mut l = layout(ctx, c, cw, cmode, inner);
                        if i > 0 {
                            y += st.gap;
                        }
                        y += m.top;
                        l.top = y;
                        l.left = pl + m.left;
                        y += l.height + m.bottom;
                        max_w = max_w.max(l.width + m.left + m.right);
                        kids.push(l);
                    }
                    content_h = y - pt;
                    content_w = max_w;
                    let final_w = if mode == Mode::Exactly { width } else { content_w + pl + pr };
                    // Stretch: auto-width children of a column measured "at most" take the final inner width.
                    if mode != Mode::Exactly {
                        let final_inner = (final_w - pl - pr).max(0.0);
                        for (c, l) in children.iter().zip(kids.iter_mut()) {
                            if fixed_width(c, final_inner).is_none() {
                                let m = margins(c);
                                let target = (final_inner - m.left - m.right).max(0.0);
                                if l.width != target {
                                    let (top, left) = (l.top, l.left);
                                    *l = layout(ctx, c, target, Mode::Exactly, final_inner);
                                    l.top = top;
                                    l.left = left;
                                }
                            }
                        }
                        // Heights can change after re-layout at a new width; restack.
                        let mut y = pt;
                        for (i, (c, l)) in children.iter().zip(kids.iter_mut()).enumerate() {
                            let m = margins(c);
                            if i > 0 {
                                y += st.gap;
                            }
                            y += m.top;
                            l.top = y;
                            y += l.height + m.bottom;
                        }
                        let h = y - pt;
                        return Layout { width: final_w, height: h + pt + pb, children: kids, ..Default::default() };
                    }
                    Layout { width: final_w, height: content_h + pt + pb, children: kids, ..Default::default() }
                }
                Dir::Row => {
                    let n = children.len();
                    let mut basis: Vec<f32> = Vec::with_capacity(n);
                    for c in children {
                        let b = match fixed_width(c, inner) {
                            Some(w) => w,
                            None => {
                                layout(
                                    ctx,
                                    c,
                                    (inner - margins(c).left - margins(c).right).max(0.0),
                                    Mode::AtMost,
                                    inner,
                                )
                                .width
                            }
                        };
                        basis.push(b);
                    }
                    let gaps = st.gap * n.saturating_sub(1) as f32;
                    let consumed: f32 =
                        children.iter().zip(&basis).map(|(c, b)| b + margins(c).left + margins(c).right).sum::<f32>()
                            + gaps;
                    // Measured "at most", with flexGrow 0 throughout, yoga takes the consumed size as the available
                    // size: nothing shrinks, and the box is as wide as its content (the parent shrinks it later).
                    let free = if mode == Mode::Exactly { inner - consumed } else { 0.0 };
                    // yoga's minimum for a flex item: its padding and border.
                    let min_of = |c: &El| match c {
                        El::Box(cs, _) => {
                            let (a, b, _, _) = box_padding_border(cs);
                            a + b
                        }
                        _ => 0.0,
                    };
                    let mut sizes = basis.clone();
                    if free < 0.0 {
                        // Ink's default flexShrink is 1 for Box and Text, so each scaled factor is -basis.
                        // distributeFreeSpaceFirstPass: items whose share would cross their minimum are frozen there,
                        // and their factor leaves the total.
                        let mut total: f32 = -basis.iter().sum::<f32>();
                        let mut remaining = free;
                        let mut delta = 0.0;
                        for (c, b) in children.iter().zip(&basis) {
                            let factor = -b;
                            if factor != 0.0 && total != 0.0 {
                                let base = b + remaining / total * factor;
                                let bound = base.max(min_of(c));
                                if base != bound {
                                    delta += bound - b;
                                    total -= factor;
                                }
                            }
                        }
                        remaining -= delta;
                        // distributeFreeSpaceSecondPass.
                        for ((c, b), s) in children.iter().zip(&basis).zip(sizes.iter_mut()) {
                            let factor = -b;
                            if factor != 0.0 {
                                let size = if total == 0.0 { b + factor } else { b + remaining / total * factor };
                                *s = size.max(min_of(c));
                            }
                        }
                    }
                    let mut x = pl;
                    let mut max_h: f32 = 0.0;
                    for (i, (c, size)) in children.iter().zip(&sizes).enumerate() {
                        let m = margins(c);
                        let size = size.max(min_of(c));
                        let mut l = layout(ctx, c, size, Mode::Exactly, inner);
                        if i > 0 {
                            x += st.gap;
                        }
                        x += m.left;
                        l.left = x;
                        l.top = pt + m.top;
                        x += l.width + m.right;
                        max_h = max_h.max(l.height + m.top + m.bottom);
                        kids.push(l);
                    }
                    content_w = x - pl;
                    content_h = max_h;
                    // Stretch on the cross axis: children take the row's height.
                    for (c, l) in children.iter().zip(kids.iter_mut()) {
                        let m = margins(c);
                        l.height = (content_h - m.top - m.bottom).max(l.height.min(content_h));
                    }
                    let final_w = if mode == Mode::Exactly { width } else { content_w + pl + pr };
                    Layout { width: final_w, height: content_h + pt + pb, children: kids, ..Default::default() }
                }
            }
        }
    }
}

fn inexact_eq(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.0001
}

/// yoga's `roundValueToPixelGrid` at a point scale factor of 1.
fn round_to_pixel(value: f32, force_ceil: bool, force_floor: bool) -> f32 {
    let mut fractial = value % 1.0;
    if fractial < 0.0 {
        fractial += 1.0;
    }
    if inexact_eq(fractial, 0.0) {
        value - fractial
    } else if inexact_eq(fractial, 1.0) || force_ceil {
        value - fractial + 1.0
    } else if force_floor {
        value - fractial
    } else {
        value - fractial + if fractial > 0.5 || inexact_eq(fractial, 0.5) { 1.0 } else { 0.0 }
    }
}

/// A rounded layout, in the shape the renderer walks.
#[derive(Clone, Debug, Default)]
pub struct Rounded {
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
    pub children: Vec<Rounded>,
}

fn round_layout(el: &El, l: &Layout, abs_left: f32, abs_top: f32) -> Rounded {
    let text = matches!(el, El::Text { .. });
    let abs_node_left = abs_left + l.left;
    let abs_node_top = abs_top + l.top;
    let frac_w = !inexact_eq(l.width % 1.0, 0.0) && !inexact_eq(l.width % 1.0, 1.0);
    let frac_h = !inexact_eq(l.height % 1.0, 0.0) && !inexact_eq(l.height % 1.0, 1.0);
    let width = round_to_pixel(abs_node_left + l.width, text && frac_w, text && !frac_w)
        - round_to_pixel(abs_node_left, false, text);
    let height = round_to_pixel(abs_node_top + l.height, text && frac_h, text && !frac_h)
        - round_to_pixel(abs_node_top, false, text);
    let children = match el {
        El::Box(_, kids) => {
            kids.iter().zip(&l.children).map(|(c, cl)| round_layout(c, cl, abs_node_left, abs_node_top)).collect()
        }
        El::Text { .. } => Vec::new(),
    };
    Rounded {
        left: round_to_pixel(l.left, false, text),
        top: round_to_pixel(l.top, false, text),
        width,
        height,
        children,
    }
}

// --- Rendering ---------------------------------------------------------------------------------------------

/// cli-boxes `round`.
const ROUND: [&str; 6] = ["╭", "╮", "╰", "╯", "─", "│"];

fn render_border(chalks: &Chalks, st: &BoxStyle, x: i64, y: i64, r: &Rounded, out: &mut Output) {
    let Some(b) = &st.border else { return };
    let width = r.width as i64;
    let height = r.height as i64;
    let content_width = (width - 2).max(0) as usize;
    let color = |s: &str| match &b.color {
        Some(c) => chalks.ink.colorize(s, c, false),
        None => s.to_string(),
    };
    let dim = |s: String| if b.dim { chalks.ink.style_text("dim", &s) } else { s };
    let top = dim(color(&format!("{}{}{}", ROUND[0], ROUND[4].repeat(content_width), ROUND[1])));
    let vertical = (height - 2).max(0) as usize;
    let left = dim(format!("{}\n", color(ROUND[5])).repeat(vertical));
    let right = left.clone();
    let bottom = dim(color(&format!("{}{}{}", ROUND[2], ROUND[4].repeat(content_width), ROUND[3])));
    out.write(x, y, top);
    out.write(x, y + 1, left);
    out.write(x + width - 1, y + 1, right);
    out.write(x, y + height - 1, bottom);
}

fn render_node(chalks: &Chalks, el: &El, r: &Rounded, offset_x: f32, offset_y: f32, out: &mut Output) {
    let x = offset_x + r.left;
    let y = offset_y + r.top;
    match el {
        El::Text { style, wrap, content } => {
            let mut text = squash(chalks, content);
            if text.is_empty() {
                return;
            }
            let current = widest_line(&text) as f32;
            let max_width = r.width;
            if current > max_width {
                text = wrap_text(&text, max_width as f64, *wrap);
            }
            let styled: Vec<String> =
                text.split('\n').map(|line| apply_style(&chalks.ink, style, line.to_string())).collect();
            out.write(x as i64, y as i64, styled.join("\n"));
        }
        El::Box(st, kids) => {
            render_border(chalks, st, x as i64, y as i64, r, out);
            for (c, cr) in kids.iter().zip(&r.children) {
                render_node(chalks, c, cr, x, y, out);
            }
        }
    }
}

/// One Ink frame: lay out `root` under Ink's root node (a column `columns` wide), render it, and return the
/// serialized output and its height.
pub fn render_frame(chalks: &Chalks, root: &El, columns: usize) -> (String, usize) {
    let ctx = Ctx { chalks };
    let cols = columns as f32;
    // ink-root: yoga defaults (column, stretch), width set to the terminal's columns.
    let root_box = El::Box(BoxStyle { dir: Dir::Column, ..Default::default() }, vec![root.clone()]);
    let l = layout(&ctx, &root_box, cols, Mode::Exactly, cols);
    let r = round_layout(&root_box, &l, 0.0, 0.0);
    let mut out = Output::new(r.width.max(0.0) as usize, r.height.max(0.0) as usize);
    render_node(chalks, &root_box, &r, 0.0, 0.0, &mut out);
    out.get()
}

// --- Builders ----------------------------------------------------------------------------------------------

pub fn col(children: Vec<El>) -> El {
    El::Box(BoxStyle { dir: Dir::Column, ..Default::default() }, children)
}

pub fn row(children: Vec<El>) -> El {
    El::Box(BoxStyle::default(), children)
}

pub fn boxed(style: BoxStyle, children: Vec<El>) -> El {
    El::Box(style, children)
}

pub fn text(style: TextStyle, content: Vec<Inline>) -> El {
    El::Text { style, wrap: TextWrap::Wrap, content }
}

pub fn plain(s: impl Into<String>) -> El {
    El::Text { style: TextStyle::default(), wrap: TextWrap::Wrap, content: vec![Inline::Str(s.into())] }
}

pub fn s(v: impl Into<String>) -> Inline {
    Inline::Str(v.into())
}

pub fn nested(style: TextStyle, content: Vec<Inline>) -> Inline {
    Inline::Nested(Transform::Style(style), content)
}

impl TextStyle {
    pub fn dim() -> Self {
        TextStyle { dim: true, ..Default::default() }
    }
    pub fn bold() -> Self {
        TextStyle { bold: true, ..Default::default() }
    }
    pub fn color(c: &str) -> Self {
        TextStyle { color: Some(c.to_string()), ..Default::default() }
    }
    pub fn with_bold(mut self, on: bool) -> Self {
        self.bold = on;
        self
    }
    pub fn with_dim(mut self, on: bool) -> Self {
        self.dim = on;
        self
    }
    pub fn with_color(mut self, c: Option<&str>) -> Self {
        self.color = c.map(str::to_string);
        self
    }
}

impl BoxStyle {
    pub fn column() -> Self {
        BoxStyle { dir: Dir::Column, ..Default::default() }
    }
    pub fn margin_top(mut self, v: f32) -> Self {
        self.margin.top = v;
        self
    }
    pub fn margin_bottom(mut self, v: f32) -> Self {
        self.margin.bottom = v;
        self
    }
    pub fn margin_left(mut self, v: f32) -> Self {
        self.margin.left = v;
        self
    }
    pub fn padding_left(mut self, v: f32) -> Self {
        self.padding.left = v;
        self
    }
    pub fn width(mut self, w: Width) -> Self {
        self.width = w;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chalks() -> Chalks {
        Chalks { ink: Chalk::new(0), gradient: Chalk::new(0) }
    }

    #[test]
    fn column_with_margins() {
        let root = col(vec![plain("a"), boxed(BoxStyle::column().margin_top(1.0), vec![plain("b")])]);
        assert_eq!(render_frame(&chalks(), &root, 10).0, "a\n\nb");
    }

    #[test]
    fn row_places_texts_side_by_side_and_wraps_when_narrow() {
        let root = row(vec![plain("abc"), plain(" def")]);
        assert_eq!(render_frame(&chalks(), &root, 20).0, "abc def");
        let narrow = col(vec![plain("aaa bbb ccc")]);
        assert_eq!(render_frame(&chalks(), &narrow, 7).0, "aaa bbb\n ccc");
    }

    #[test]
    fn round_border_full_width() {
        let st = BoxStyle {
            border: Some(Border::default()),
            width: Width::Percent(100.0),
            ..BoxStyle::default().padding_left(1.0)
        };
        let root = col(vec![boxed(st, vec![plain("> x")])]);
        assert_eq!(render_frame(&chalks(), &root, 8).0, "╭──────╮\n│ > x  │\n╰──────╯");
    }
}
