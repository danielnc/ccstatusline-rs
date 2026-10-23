//! The widget picker's catalog and search (upstream `getWidgetCatalog`, `filterWidgetCatalog` in
//! `utils/widgets.ts`, and `utils/fuzzy.ts`).

use super::{get_widget, UPSTREAM_TYPES};
use std::cmp::Ordering;

#[derive(Clone, Debug)]
pub struct Entry {
    pub kind: String,
    pub display_name: String,
    pub description: String,
    pub category: String,
    pub search_text: String,
}

fn entry(kind: &str, name: &str, description: &str, category: &str) -> Entry {
    Entry {
        kind: kind.into(),
        display_name: name.into(),
        description: description.into(),
        category: category.into(),
        search_text: format!("{name} {description} {kind}").to_lowercase(),
    }
}

/// `getWidgetCatalog(settings)`: the manifest's widgets, then the separator (only without powerline or a
/// default separator) and the flex separator.
pub fn catalog(powerline_enabled: bool, has_default_separator: bool) -> Vec<Entry> {
    let mut out: Vec<Entry> = UPSTREAM_TYPES
        .iter()
        .map(|kind| match get_widget(kind) {
            Some(w) => entry(kind, w.display_name(), w.description(), w.category()),
            None => entry(kind, kind, &format!("Unknown widget: {kind}"), "Other"),
        })
        .collect();
    if !powerline_enabled && !has_default_separator {
        out.push(entry("separator", "Separator", "A separator character between status line widgets", "Layout"));
    }
    out.push(entry("flex-separator", "Flex Separator", "Expands to fill available terminal width", "Layout"));
    out
}

/// `getWidgetCatalogCategories`: categories in first-seen order.
pub fn categories(catalog: &[Entry]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for e in catalog {
        if !out.contains(&e.category) {
            out.push(e.category.clone());
        }
    }
    out
}

// --- fuzzy.ts ------------------------------------------------------------------------------------------------
// Positions are UTF-16 indices, as in JavaScript; the catalog's texts are ASCII, so chars stand in for units.

fn is_word_start(text: &[char], position: usize) -> bool {
    position == 0 || matches!(text[position - 1], ' ' | '-' | '_' | '/')
}

fn subsequence_positions(text: &[char], query: &[char]) -> Option<Vec<usize>> {
    let mut qi = 0;
    let mut positions = Vec::new();
    for (ti, c) in text.iter().enumerate() {
        if qi >= query.len() {
            break;
        }
        if *c == query[qi] {
            positions.push(ti);
            qi += 1;
        }
    }
    (qi >= query.len()).then_some(positions)
}

fn initialism_match(text: &[char], query: &[char]) -> Option<(Vec<usize>, i64)> {
    let starts: Vec<usize> = (0..text.len()).filter(|&i| is_word_start(text, i)).collect();
    let initials: Vec<char> = starts.iter().map(|&p| text[p]).collect();
    let matched = subsequence_positions(&initials, query)?;
    let (&first, &last) = (matched.first()?, matched.last()?);
    let positions: Vec<usize> = matched.iter().map(|&i| starts[i]).collect();
    Some((positions, (last as i64 - first as i64) + first as i64))
}

fn fuzzy_score(text: &[char], query: &[char]) -> Option<i64> {
    let positions = subsequence_positions(text, query)?;
    let (&first, &last) = (positions.first()?, positions.last()?);
    let mut consecutive = 0i64;
    let mut word_starts = 0i64;
    let mut previous: Option<usize> = None;
    for &p in &positions {
        if previous.is_some_and(|q| p == q + 1) {
            consecutive += 5;
        }
        if is_word_start(text, p) {
            word_starts += 1;
        }
        previous = Some(p);
    }
    let full_initialism = if word_starts == query.len() as i64 { 40 } else { 0 };
    Some((last - first) as i64 + first as i64 - consecutive - word_starts * 20 - full_initialism)
}

/// A `FuzzySearchRecord`: the texts a fuzzy search scores and sorts on.
pub struct Record<'a> {
    pub name: &'a str,
    pub kind: &'a str,
    pub description: &'a str,
    pub search_text: &'a str,
    pub sort_text: &'a str,
    pub secondary_sort_text: &'a str,
}

fn score(e: &Record, query: &str) -> Option<i64> {
    if query.is_empty() {
        return Some(99);
    }
    const TIER: i64 = 1000;
    let q: Vec<char> = query.chars().collect();
    let name_s = e.name.to_lowercase();
    let name: Vec<char> = name_s.chars().collect();
    let description = e.description.to_lowercase();
    let kind = e.kind.to_lowercase();
    let search = e.search_text.to_lowercase();
    let initialism = initialism_match(&name, &q);
    if name_s.starts_with(query) {
        if let Some((_, s)) = &initialism {
            return Some(*s);
        }
        return Some(TIER);
    }
    if let Some((_, s)) = initialism {
        return Some(2 * TIER + s);
    }
    if name_s.contains(query) {
        return Some(3 * TIER);
    }
    if kind.contains(query) {
        return Some(4 * TIER);
    }
    if let Some(s) = fuzzy_score(&name, &q) {
        return Some(5 * TIER + s);
    }
    if description.contains(query) {
        return Some(6 * TIER);
    }
    if search.contains(query) {
        return Some(7 * TIER);
    }
    if let Some(s) = fuzzy_score(&kind.chars().collect::<Vec<_>>(), &q) {
        return Some(8 * TIER + s);
    }
    fuzzy_score(&search.chars().collect::<Vec<_>>(), &q).map(|s| 9 * TIER + s)
}

/// `filterFuzzySearchRecords(records, query)`: the indices of the matching records, best first.
pub fn filter_records(records: &[Record], query: &str) -> Vec<usize> {
    let q = crate::jsnum::js_trim(query).to_lowercase();
    let mut scored: Vec<(i64, usize)> =
        records.iter().enumerate().filter_map(|(i, r)| score(r, &q).map(|s| (s, i))).collect();
    // Array.prototype.sort is stable.
    scored.sort_by(|a, b| {
        let (ra, rb) = (&records[a.1], &records[b.1]);
        a.0.cmp(&b.0)
            .then_with(|| locale_compare(ra.sort_text, rb.sort_text))
            .then_with(|| locale_compare(ra.secondary_sort_text, rb.secondary_sort_text))
    });
    scored.into_iter().map(|(_, i)| i).collect()
}

/// `filterWidgetCatalog(catalog, category, query)`.
pub fn filter<'a>(catalog: &'a [Entry], category: &str, query: &str) -> Vec<&'a Entry> {
    let entries: Vec<&Entry> = catalog.iter().filter(|e| category == "All" || e.category == category).collect();
    let records: Vec<Record> = entries
        .iter()
        .map(|e| Record {
            name: &e.display_name,
            kind: &e.kind,
            description: &e.description,
            search_text: &e.search_text,
            sort_text: &e.display_name,
            secondary_sort_text: &e.kind,
        })
        .collect();
    filter_records(&records, query).into_iter().map(|i| entries[i]).collect()
}

/// `getMatchSegments(text, query)`: (text, matched) runs.
pub fn match_segments(text: &str, query: &str) -> Vec<(String, bool)> {
    let trimmed = crate::jsnum::js_trim(query);
    if trimmed.is_empty() {
        return vec![(text.to_string(), false)];
    }
    let q: Vec<char> = trimmed.to_lowercase().chars().collect();
    let chars: Vec<char> = text.chars().collect();
    let lower: Vec<char> = text.to_lowercase().chars().collect();
    let runs = |positions: &[usize]| {
        let mut segments: Vec<(String, bool)> = Vec::new();
        let mut current = String::new();
        let mut current_matched = positions.contains(&0);
        for (i, c) in chars.iter().enumerate() {
            let matched = positions.contains(&i);
            if matched != current_matched && !current.is_empty() {
                segments.push((std::mem::take(&mut current), current_matched));
                current.push(*c);
                current_matched = matched;
            } else {
                current.push(*c);
            }
        }
        if !current.is_empty() {
            segments.push((current, current_matched));
        }
        segments
    };
    if let Some((positions, _)) = initialism_match(&lower, &q) {
        return runs(&positions);
    }
    let lower_s: String = lower.iter().collect();
    let q_s: String = q.iter().collect();
    if let Some(byte_idx) = lower_s.find(&q_s) {
        let start = lower_s[..byte_idx].chars().count();
        let end = start + q.len();
        let mut segments = Vec::new();
        if start > 0 {
            segments.push((chars[..start].iter().collect(), false));
        }
        segments.push((chars[start..end.min(chars.len())].iter().collect(), true));
        if end < chars.len() {
            segments.push((chars[end..].iter().collect(), false));
        }
        return segments;
    }
    match subsequence_positions(&lower, &q) {
        Some(positions) => runs(&positions),
        None => vec![(text.to_string(), false)],
    }
}

// --- String.prototype.localeCompare (ICU root collation, for the ASCII the catalog holds) -----------------------

/// CLDR root order of ASCII punctuation and symbols (non-ignorable), before digits and letters.
const PUNCTUATION: &str = "_-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$";

fn primary(c: char) -> (u8, u32) {
    if c.is_whitespace() {
        return (0, c as u32);
    }
    if let Some(i) = PUNCTUATION.find(c) {
        return (1, i as u32);
    }
    if c.is_ascii_digit() {
        return (2, c as u32);
    }
    if c.is_alphabetic() {
        return (3, c.to_lowercase().next().unwrap_or(c) as u32);
    }
    (4, c as u32)
}

pub fn locale_compare(a: &str, b: &str) -> Ordering {
    let pa: Vec<(u8, u32)> = a.chars().map(primary).collect();
    let pb: Vec<(u8, u32)> = b.chars().map(primary).collect();
    pa.cmp(&pb).then_with(|| {
        // Tertiary: lowercase before uppercase, position by position.
        for (x, y) in a.chars().zip(b.chars()) {
            match (x.is_uppercase(), y.is_uppercase()) {
                (false, true) => return Ordering::Less,
                (true, false) => return Ordering::Greater,
                _ => {}
            }
        }
        Ordering::Equal
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_order() {
        assert_eq!(locale_compare("a", "B"), Ordering::Less);
        assert_eq!(locale_compare("git-branch", "git branch"), Ordering::Greater);
        assert_eq!(locale_compare("a", "A"), Ordering::Less);
    }
}
