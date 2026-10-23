//! Upstream `widgets/shared/text-cursor.ts`: a single-line text input whose cursor sits on grapheme boundaries.
//! Indices are UTF-16 offsets, as in JavaScript.

use super::ink::keys::Key;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextCursor {
    pub text: String,
    /// UTF-16 offset.
    pub cursor: usize,
}

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// Byte index of a UTF-16 offset (which always falls on a grapheme boundary here).
fn byte_at(s: &str, utf16: usize) -> usize {
    let mut units = 0;
    for (i, c) in s.char_indices() {
        if units >= utf16 {
            return i;
        }
        units += c.len_utf16();
    }
    s.len()
}

fn boundaries(text: &str) -> Vec<usize> {
    let mut b = vec![0];
    for g in text.graphemes(true) {
        b.push(b.last().unwrap() + utf16_len(g));
    }
    b
}

/// `shouldInsertInput`.
pub fn should_insert_input(input: &str, key: &Key) -> bool {
    if input.is_empty() || key.ctrl || key.meta || key.tab {
        return false;
    }
    !input.chars().any(|c| (c as u32) <= 0x1f || c as u32 == 0x7f)
}

impl TextCursor {
    pub fn new(text: &str) -> Self {
        TextCursor { text: text.to_string(), cursor: utf16_len(text) }
    }

    /// `applyTextCursorInput`: the new state, or None when the key does not edit text.
    pub fn apply(&self, input: &str, key: &Key) -> Option<TextCursor> {
        let text = &self.text;
        let cursor = self.cursor;
        let b = boundaries(text);
        let index = b.iter().position(|&x| x >= cursor);
        let previous = index.and_then(|i| i.checked_sub(1)).and_then(|i| b.get(i)).copied();
        let next = index.and_then(|i| b.get(i + 1)).copied();
        let total = utf16_len(text);
        if key.ctrl && key.left_arrow {
            return Some(TextCursor { text: text.clone(), cursor: 0 });
        }
        if key.ctrl && key.right_arrow {
            return Some(TextCursor { text: text.clone(), cursor: total });
        }
        if key.left_arrow {
            return Some(previous.map_or_else(|| self.clone(), |p| TextCursor { text: text.clone(), cursor: p }));
        }
        if key.right_arrow {
            return Some(next.map_or_else(|| self.clone(), |n| TextCursor { text: text.clone(), cursor: n }));
        }
        if key.backspace {
            return Some(match previous {
                None => self.clone(),
                Some(p) => TextCursor {
                    text: format!("{}{}", &text[..byte_at(text, p)], &text[byte_at(text, cursor)..]),
                    cursor: p,
                },
            });
        }
        if key.delete {
            return Some(match next {
                None => self.clone(),
                Some(n) => TextCursor {
                    text: format!("{}{}", &text[..byte_at(text, cursor)], &text[byte_at(text, n)..]),
                    cursor,
                },
            });
        }
        if should_insert_input(input, key) {
            let at = byte_at(text, cursor);
            return Some(TextCursor {
                text: format!("{}{input}{}", &text[..at], &text[at..]),
                cursor: cursor + utf16_len(input),
            });
        }
        None
    }

    /// `handleInput`: applies the key and reports whether it was consumed.
    pub fn handle(&mut self, input: &str, key: &Key) -> bool {
        match self.apply(input, key) {
            Some(next) => {
                *self = next;
                true
            }
            None => false,
        }
    }

    /// `renderTextWithCursor`: the grapheme under the cursor in inverse video, or a trailing block at the end.
    pub fn display(&self) -> String {
        let mut offset = 0;
        let mut out = String::new();
        for g in self.text.graphemes(true) {
            if offset == self.cursor {
                out.push_str(&format!("\x1b[7m{g}\x1b[27m"));
            } else {
                out.push_str(g);
            }
            offset += utf16_len(g);
        }
        if self.cursor == utf16_len(&self.text) {
            out.push_str("\x1b[7m \x1b[27m");
        }
        out
    }
}
