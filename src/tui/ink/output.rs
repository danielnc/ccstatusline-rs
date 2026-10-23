//! Ink's `Output`: a grid of styled characters that text is written into at (x, y), then serialized line by
//! line. Styles are tracked the way @alcalzone/ansi-tokenize does (one code per close code, so bold and dim
//! replace each other), and cell widths follow the tokenizer (`is_fullwidth_code_point` or an astral code
//! point), not string-width. Both quirks decide what the terminal ends up showing.

use super::text::{ansi_close_code, is_fullwidth_code_point};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Code {
    code: String,
    end_code: String,
}

#[derive(Clone, Debug)]
struct Cell {
    value: String,
    styles: Vec<Code>,
}

fn blank() -> Cell {
    Cell { value: " ".into(), styles: Vec::new() }
}

const RESET: &str = "\x1b[0m";
const LINK_PREFIX: &str = "\x1b]8;;";
const LINK_END: &str = "\x1b]8;;\x07";

fn is_end_code(code: &str) -> bool {
    // endCodesSet: the close codes of every ansi-styles entry.
    matches!(
        code,
        "\x1b[0m"
            | "\x1b[22m"
            | "\x1b[23m"
            | "\x1b[24m"
            | "\x1b[55m"
            | "\x1b[27m"
            | "\x1b[28m"
            | "\x1b[29m"
            | "\x1b[39m"
            | "\x1b[49m"
    )
}

fn end_code_for(code: &str) -> String {
    if is_end_code(code) {
        return code.to_string();
    }
    if code.starts_with(LINK_PREFIX) {
        return LINK_END.to_string();
    }
    let mut body = &code[2..];
    let owned;
    if body.contains(';') {
        owned = format!("{}0", &body[..1]);
        body = &owned;
    }
    match crate::jsnum::parse_int(body).and_then(|n| ansi_close_code(n as u32)) {
        Some(close) => format!("\x1b[{close}m"),
        None => RESET.to_string(),
    }
}

fn parse_link_code(s: &str) -> Option<&str> {
    if !s.starts_with(LINK_PREFIX) {
        return None;
    }
    let end = s[LINK_PREFIX.len()..].find('\x07')? + LINK_PREFIX.len();
    Some(&s[..=end])
}

fn parse_ansi_code(s: &str) -> Option<&str> {
    // string.slice(offset, offset + 19), then from the first digit to the next "m" (or the window's end).
    let window_end = s.char_indices().nth(19).map_or(s.len(), |(i, _)| i);
    let window = &s[..window_end];
    let start = window.find(|c: char| c.is_ascii_digit())?;
    let end = window[start..].find('m').map_or(window.len(), |i| start + i + 1);
    Some(&window[..end])
}

enum Token {
    Ansi(Code),
    Char { value: String, full_width: bool },
}

fn tokenize(s: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let c = s[i..].chars().next().unwrap();
        if c == '\x1b' || c == '\u{9b}' {
            if let Some(code) = parse_link_code(&s[i..]).or_else(|| parse_ansi_code(&s[i..])) {
                out.push(Token::Ansi(Code { code: code.to_string(), end_code: end_code_for(code) }));
                i += code.len();
                continue;
            }
        }
        out.push(Token::Char { value: c.to_string(), full_width: is_fullwidth_code_point(c as u32) });
        i += c.len_utf8();
    }
    out
}

fn reduce(codes: &mut Vec<Code>, code: Code) {
    if code.code == RESET {
        codes.clear();
    } else if is_end_code(&code.code) {
        codes.retain(|c| c.end_code != code.code);
    } else {
        codes.retain(|c| c.end_code != code.end_code);
        codes.push(code);
    }
}

fn styled_chars(s: &str) -> Vec<Cell> {
    let mut codes: Vec<Code> = Vec::new();
    let mut out = Vec::new();
    for t in tokenize(s) {
        match t {
            Token::Ansi(code) => reduce(&mut codes, code),
            Token::Char { value, full_width } => {
                // The tokenizer's own flag; the grid also treats any multi-unit (astral) value as wide.
                let _ = full_width;
                out.push(Cell { value, styles: codes.clone() });
            }
        }
    }
    out
}

fn undo(codes: &[Code]) -> Vec<String> {
    let mut reduced: Vec<Code> = Vec::new();
    for c in codes {
        reduce(&mut reduced, c.clone());
    }
    reduced.iter().rev().map(|c| c.end_code.clone()).collect()
}

fn diff(from: &[Code], to: &[Code]) -> String {
    let undo_part: Vec<Code> = from.iter().filter(|c| !to.iter().any(|t| t.end_code == c.end_code)).cloned().collect();
    let mut s: String = undo(&undo_part).concat();
    for c in to {
        if !from.iter().any(|f| f.code == c.code) {
            s.push_str(&c.code);
        }
    }
    s
}

fn styled_chars_to_string(chars: &[&Cell]) -> String {
    let mut s = String::new();
    for (i, c) in chars.iter().enumerate() {
        if i == 0 {
            for code in &c.styles {
                s.push_str(&code.code);
            }
        } else {
            s.push_str(&diff(&chars[i - 1].styles, &c.styles));
        }
        s.push_str(&c.value);
        if i == chars.len() - 1 {
            s.push_str(&diff(&c.styles, &[]));
        }
    }
    s
}

fn is_wide(cell: &Cell) -> bool {
    let mut chars = cell.value.chars();
    let first = chars.next();
    // `character.fullWidth || character.value.length > 1` (UTF-16 length)
    first.is_some_and(|c| is_fullwidth_code_point(c as u32) || c.len_utf16() > 1)
}

pub struct Output {
    width: usize,
    height: usize,
    writes: Vec<(i64, i64, String)>,
}

impl Output {
    pub fn new(width: usize, height: usize) -> Self {
        Output { width, height, writes: Vec::new() }
    }

    pub fn write(&mut self, x: i64, y: i64, text: String) {
        if !text.is_empty() {
            self.writes.push((x, y, text));
        }
    }

    /// The serialized frame (lines joined by "\n", each trimmed at the end) and its height.
    pub fn get(&self) -> (String, usize) {
        let mut grid: Vec<Vec<Option<Cell>>> =
            (0..self.height).map(|_| (0..self.width).map(|_| Some(blank())).collect()).collect();
        for (x, y, text) in &self.writes {
            for (offset_y, line) in text.split('\n').enumerate() {
                let row_index = *y + offset_y as i64;
                if row_index < 0 || row_index as usize >= grid.len() {
                    continue;
                }
                let row = &mut grid[row_index as usize];
                let mut offset_x = *x;
                for cell in styled_chars(line) {
                    let wide = is_wide(&cell);
                    let styles = cell.styles.clone();
                    set(row, offset_x, cell);
                    if wide {
                        set(row, offset_x + 1, Cell { value: String::new(), styles });
                    }
                    offset_x += if wide { 2 } else { 1 };
                }
            }
        }
        let lines: Vec<String> = grid
            .iter()
            .map(|row| {
                let cells: Vec<&Cell> = row.iter().flatten().collect();
                trim_end_js(&styled_chars_to_string(&cells)).to_string()
            })
            .collect();
        (lines.join("\n"), self.height)
    }
}

/// Assigning past the row's end grows the JavaScript array (with holes, which `filter` drops).
fn set(row: &mut Vec<Option<Cell>>, x: i64, cell: Cell) {
    if x < 0 {
        return;
    }
    let x = x as usize;
    if x >= row.len() {
        row.resize_with(x + 1, || None);
    }
    row[x] = Some(cell);
}

fn trim_end_js(s: &str) -> &str {
    s.trim_end_matches(|c: char| crate::jsnum::js_trim(c.encode_utf8(&mut [0; 4])).is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_and_trims() {
        let mut o = Output::new(10, 2);
        o.write(2, 0, "ab".into());
        o.write(0, 1, "\x1b[1mx\x1b[22m".into());
        assert_eq!(o.get().0, "  ab\n\x1b[1mx\x1b[22m");
    }

    #[test]
    fn dim_replaces_bold() {
        // b is tracked as dim only, so bold is never closed before it: the terminal shows b bold and dim.
        let mut o = Output::new(5, 1);
        o.write(0, 0, "\x1b[1ma\x1b[2mb\x1b[22m\x1b[1mc\x1b[22m".into());
        assert_eq!(o.get().0, "\x1b[1ma\x1b[2mb\x1b[1mc\x1b[22m");
    }

    #[test]
    fn astral_chars_take_two_cells() {
        let mut o = Output::new(6, 1);
        o.write(0, 0, "📝x".into());
        o.write(3, 0, "y".into());
        assert_eq!(o.get().0, "📝xy");
    }
}
