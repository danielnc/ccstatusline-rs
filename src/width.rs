//! Display width, ported from string-width 7.2 (as bundled by upstream) plus
//! ccstatusline's own display-cluster rules in `src/utils/ansi.ts`.

use crate::unicode_tables as t;
use unicode_segmentation::UnicodeSegmentation;

fn in_table(table: &[(u32, u32)], cp: u32) -> bool {
    table
        .binary_search_by(|&(lo, hi)| {
            if cp < lo {
                std::cmp::Ordering::Greater
            } else if cp > hi {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

pub fn is_mark(cp: u32) -> bool {
    cp >= 0x300 && in_table(t::MARK, cp)
}
pub fn is_emoji_modifier(cp: u32) -> bool {
    in_table(t::EMOJI_MODIFIER, cp)
}
pub fn is_emoji_presentation(cp: u32) -> bool {
    in_table(t::EMOJI_PRESENTATION, cp)
}
pub fn is_extended_pictographic(cp: u32) -> bool {
    in_table(t::EXTENDED_PICTOGRAPHIC, cp)
}
pub fn is_variation_selector(cp: u32) -> bool {
    (0xfe00..=0xfe0f).contains(&cp) || (0xe0100..=0xe01ef).contains(&cp)
}
pub fn is_regional_indicator(cp: u32) -> bool {
    (0x1f1e6..=0x1f1ff).contains(&cp)
}

/// Whether emoji-regex (unanchored, as string-width uses it) matches somewhere
/// in the grapheme.
fn grapheme_has_emoji(g: &str) -> bool {
    let cps: Vec<u32> = g.chars().map(|c| c as u32).collect();
    for i in 0..cps.len() {
        let cp = cps[i];
        let next = cps.get(i + 1).copied();
        if in_table(t::EMOJI_SINGLE, cp) {
            return true;
        }
        if next == Some(0xfe0f) && in_table(t::EMOJI_WITH_VS16, cp) {
            return true;
        }
        if let Some(n) = next {
            if is_emoji_modifier(n) && in_table(t::EMOJI_WITH_MODIFIER, cp) {
                return true;
            }
        }
        if matches!(cp, 0x23 | 0x2a | 0x30..=0x39) {
            let mut j = i + 1;
            if cps.get(j) == Some(&0xfe0f) {
                j += 1;
            }
            if cps.get(j) == Some(&0x20e3) && (j == i + 2 || t::KEYCAP_WITHOUT_VS16) {
                return true;
            }
        }
        if is_regional_indicator(cp) {
            if let Some(n) = next {
                if is_regional_indicator(n) {
                    let idx = ((cp - 0x1f1e6) * 26 + (n - 0x1f1e6)) as u16;
                    if t::FLAG_PAIRS.binary_search(&idx).is_ok() {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn east_asian_width(cp: u32) -> usize {
    if in_table(t::WIDE, cp) {
        2
    } else {
        1
    }
}

/// string-width 7.2 `stringWidth(s)` with default options, for text that has
/// already had its escape sequences removed.
pub fn string_width(s: &str) -> usize {
    if s.is_empty() {
        return 0;
    }
    let mut width = 0;
    for g in s.graphemes(true) {
        let mut chars = g.chars();
        let cp = chars.next().unwrap() as u32;
        let single = chars.next().is_none();
        if cp <= 31 || (127..=159).contains(&cp) {
            continue;
        }
        if (8203..=8207).contains(&cp) || cp == 65279 {
            continue;
        }
        if (768..=879).contains(&cp)
            || (6832..=6911).contains(&cp)
            || (7616..=7679).contains(&cp)
            || (8400..=8447).contains(&cp)
            || (65056..=65071).contains(&cp)
        {
            continue;
        }
        if (65024..=65039).contains(&cp) {
            continue;
        }
        if single && in_table(t::DEFAULT_IGNORABLE, cp) {
            continue;
        }
        if grapheme_has_emoji(g) {
            width += 2;
            continue;
        }
        width += east_asian_width(cp);
    }
    width
}

fn is_zero_width_standalone_cluster(cluster: &str) -> bool {
    !cluster.is_empty()
        && cluster.chars().all(|c| {
            let cp = c as u32;
            cp == 0x200d || cp == 0x20e3 || is_variation_selector(cp) || is_mark(cp) || is_emoji_modifier(cp)
        })
}

fn should_treat_as_narrow_text_pictograph(cluster: &str, sw: usize) -> bool {
    if sw <= 1 {
        return false;
    }
    for c in cluster.chars() {
        let cp = c as u32;
        if cp == 0x200d
            || cp == 0x20e3
            || is_variation_selector(cp)
            || is_regional_indicator(cp)
            || is_emoji_presentation(cp)
            || is_emoji_modifier(cp)
        {
            return false;
        }
    }
    cluster.chars().any(|c| is_extended_pictographic(c as u32))
}

/// ccstatusline's `getClusterWidth`.
pub fn cluster_width(cluster: &str) -> usize {
    let b = cluster.as_bytes();
    if b.len() == 1 && (0x20..=0x7e).contains(&b[0]) {
        return 1;
    }
    if cluster.is_empty() || is_zero_width_standalone_cluster(cluster) {
        return 0;
    }
    let sw = string_width(cluster);
    if should_treat_as_narrow_text_pictograph(cluster, sw) {
        return 1;
    }
    sw
}

/// ccstatusline's `consumeDisplayCluster`: returns the byte end of the cluster
/// starting at `start`.
pub fn consume_display_cluster(text: &str, start: usize) -> Option<usize> {
    let mut it = text[start..].char_indices();
    let (_, first) = it.next()?;
    let mut index = start + first.len_utf8();
    let first_cp = first as u32;
    if is_regional_indicator(first_cp) {
        if let Some(n) = text[index..].chars().next() {
            if is_regional_indicator(n as u32) {
                index += n.len_utf8();
            }
        }
        return Some(index);
    }
    while index < text.len() {
        let next = text[index..].chars().next().unwrap();
        let ncp = next as u32;
        if is_variation_selector(ncp) || ncp == 0x20e3 || is_mark(ncp) || is_emoji_modifier(ncp) {
            index += next.len_utf8();
            continue;
        }
        if ncp == 0x200d {
            index += next.len_utf8();
            match text[index..].chars().next() {
                Some(j) => {
                    index += j.len_utf8();
                    continue;
                }
                None => break,
            }
        }
        break;
    }
    Some(index)
}

/// ccstatusline's `getTextDisplayWidth` for escape-free text.
pub fn text_display_width(text: &str) -> usize {
    let b = text.as_bytes();
    let mut width = 0;
    let mut index = 0;
    while index < b.len() {
        if (0x20..=0x7e).contains(&b[index]) {
            let next = index + 1;
            // next char below U+0300 (any ASCII byte or a 2-byte sequence below 0xCC 0x80)
            if next >= b.len() || b[next] < 0x80 || (b[next] < 0xcc && b[next] >= 0xc0) {
                width += 1;
                index = next;
                continue;
            }
        }
        match consume_display_cluster(text, index) {
            Some(end) => {
                width += cluster_width(&text[index..end]);
                index = end;
            }
            None => break,
        }
    }
    width
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths() {
        assert_eq!(string_width("abc"), 3);
        assert_eq!(string_width("日本"), 4);
        assert_eq!(string_width("😀"), 2);
        assert_eq!(text_display_width("a😀b"), 4);
        assert_eq!(text_display_width("e\u{301}"), 1);
        assert_eq!(text_display_width("\u{e0b0}"), 1);
        assert_eq!(text_display_width("█░"), 2);
    }
}
