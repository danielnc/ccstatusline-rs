//! The `Intl` facts the locale and timezone editors read (upstream `utils/locales.ts` and `utils/timezones.ts`),
//! from tables generated out of node's ICU data (`intl_tables.rs`, by `tools/gen-intl-tables.mjs`).
//!
//! Locale canonicalization covers language, script, region and variant subtags with ICU's language and region
//! aliases. Not covered: extension and private-use subtags (`-u-…`, `-x-…`) and the `posix` variant, which read as
//! invalid here.

use super::intl_tables::{DATE_LANGUAGES, LANGUAGE_ALIASES, REGION_ALIASES, TIME_ZONES};

pub const DEFAULT_RESET_LOCALE: &str = "en-US";

fn alias<'a>(table: &'a [(&str, &str)], key: &str) -> Option<&'a str> {
    table.binary_search_by(|(k, _)| (*k).cmp(key)).ok().map(|i| table[i].1)
}

fn is_alpha(s: &str, lens: std::ops::RangeInclusive<usize>) -> bool {
    lens.contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphabetic())
}

fn is_variant(s: &str) -> bool {
    let alnum = s.bytes().all(|b| b.is_ascii_alphanumeric());
    alnum && ((5..=8).contains(&s.len()) || (s.len() == 4 && s.as_bytes()[0].is_ascii_digit()))
}

fn title_case(s: &str) -> String {
    let lower = s.to_ascii_lowercase();
    lower[..1].to_ascii_uppercase() + &lower[1..]
}

/// `canonicalizeLocale`: `Intl.getCanonicalLocales(trimmed)[0]`, or null when that throws or no date format
/// supports it.
pub fn canonicalize_locale(locale: &str) -> Option<String> {
    let trimmed = crate::jsnum::js_trim(locale);
    if trimmed.is_empty() {
        return None;
    }
    let mut parts = trimmed.split('-').peekable();
    let first = parts.next()?;
    if !(is_alpha(first, 2..=3) || is_alpha(first, 5..=8)) {
        return None;
    }
    let mut language = first.to_ascii_lowercase();
    let mut script = parts.next_if(|p| is_alpha(p, 4..=4)).map(title_case);
    let mut region = parts
        .next_if(|p| is_alpha(p, 2..=2) || (p.len() == 3 && p.bytes().all(|b| b.is_ascii_digit())))
        .map(|p| p.to_ascii_uppercase());
    let mut variants: Vec<String> = Vec::new();
    for p in parts {
        if !is_variant(p) {
            return None;
        }
        let v = p.to_ascii_lowercase();
        if variants.contains(&v) || v == "posix" {
            return None;
        }
        variants.push(v);
    }
    variants.sort();
    if let Some(replacement) = alias(LANGUAGE_ALIASES, &language) {
        let mut subtags = replacement.split('-');
        language = subtags.next()?.to_string();
        for t in subtags {
            if t.len() == 4 {
                script = script.or_else(|| Some(t.to_string()));
            } else {
                region = region.or_else(|| Some(t.to_string()));
            }
        }
    }
    if let Some(r) = region.as_deref().and_then(|r| alias(REGION_ALIASES, r)) {
        region = Some(r.to_string());
    }
    DATE_LANGUAGES.binary_search(&language.as_str()).ok()?;
    let mut out = language;
    for t in script.into_iter().chain(region).chain(variants) {
        out.push('-');
        out.push_str(&t);
    }
    Some(out)
}

/// `getSystemLocale`: ICU's default locale (`LC_ALL`, `LC_MESSAGES`, then `LANG`), canonicalized.
pub fn system_locale() -> Option<String> {
    let var = ["LC_ALL", "LC_MESSAGES", "LANG"].iter().find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))?;
    let (base, modifier) = match var.split_once('@') {
        Some((b, m)) => (b, Some(m)),
        None => (var.as_str(), None),
    };
    let base = base.split('.').next().unwrap_or("");
    let tag = match base {
        "C" | "POSIX" | "en_US_POSIX" => DEFAULT_RESET_LOCALE.to_string(),
        _ => {
            let mut t = base.replace('_', "-");
            if let Some(m) = modifier {
                t = format!("{t}-{m}");
            }
            t
        }
    };
    canonicalize_locale(&tag)
}

/// `getLocalTimezone`.
pub fn local_timezone() -> Option<String> {
    crate::usage::tz::default_zone_name().filter(|z| !z.is_empty())
}

fn is_offset_zone(tz: &str) -> bool {
    let b = tz.as_bytes();
    let digits = |s: &[u8]| s.iter().all(u8::is_ascii_digit);
    matches!(b.first(), Some(b'+' | b'-'))
        && match b.len() {
            3 | 5 => digits(&b[1..]),
            6 => digits(&b[1..3]) && b[3] == b':' && digits(&b[4..]),
            _ => false,
        }
}

/// `isValidTimezone`: a zone `Intl.DateTimeFormat` accepts (IANA names in any case, their aliases, UTC offsets).
pub fn is_valid_timezone(tz: &str) -> bool {
    is_offset_zone(tz) || TIME_ZONES.iter().any(|z| z.eq_ignore_ascii_case(tz)) || crate::usage::tz::zone_exists(tz)
}

/// `Intl.supportedValuesOf('timeZone')`.
pub fn supported_time_zones() -> &'static [&'static str] {
    TIME_ZONES
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalizes_like_node() {
        for (input, expected) in [
            ("en", Some("en")),
            (" EN-us ", Some("en-US")),
            ("en_US", None),
            ("en-ZZ", Some("en-ZZ")),
            ("xx", None),
            ("e", None),
            ("us", None),
            ("ger", Some("de")),
            ("span", None),
            ("spanish", None),
            ("zh-hant", Some("zh-Hant")),
            ("sr-latn-rs", Some("sr-Latn-RS")),
            ("iw", Some("he")),
            ("sh", Some("sr-Latn")),
            ("sh-RS", Some("sr-Latn-RS")),
            ("de-1996-1901", Some("de-1901-1996")),
            ("de-DE-1901-1901", None),
            ("zh-yue", None),
            ("en-DD", Some("en-DE")),
            ("es-419", Some("es-419")),
            ("und", None),
        ] {
            assert_eq!(canonicalize_locale(input).as_deref(), expected, "{input}");
        }
    }

    #[test]
    fn offsets_and_zone_names() {
        assert!(is_offset_zone("+05:30"));
        assert!(is_offset_zone("-03"));
        assert!(!is_offset_zone("+5"));
        assert!(TIME_ZONES.iter().any(|z| z.eq_ignore_ascii_case("america/sao_paulo")));
    }
}
