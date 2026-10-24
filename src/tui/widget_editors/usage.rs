//! The usage group's editors: the reset timers' locale and timezone pickers (upstream
//! `widgets/shared/{locale-editor,timezone-editor}.tsx`, with the options from `utils/locales.ts` and
//! `utils/timezones.ts`).

use super::shared::{fuzzy_filter_options, SearchOption, SearchableOptionEditor};
use super::WidgetEditor;
use crate::jsobj;
use crate::tui::intl::{self, DEFAULT_RESET_LOCALE};
use serde_json::Value;

pub const LOCALE_EDITOR_ACTION: &str = "edit-locale";
pub const TIMEZONE_EDITOR_ACTION: &str = "edit-timezone";

/// `renderEditor` of `BlockResetTimer` and `WeeklyResetTimer`.
pub fn open(kind: &str, action: &str, item: &Value) -> Option<Box<dyn WidgetEditor>> {
    match (kind, action) {
        ("reset-timer" | "weekly-reset-timer", LOCALE_EDITOR_ACTION) => Some(Box::new(locale_editor(item))),
        ("reset-timer" | "weekly-reset-timer", TIMEZONE_EDITOR_ACTION) => Some(Box::new(timezone_editor(item))),
        _ => None,
    }
}

/// `getUsageTimezone` / `getUsageLocale`: a non-empty string.
fn non_empty_metadata<'a>(item: &'a Value, key: &str) -> Option<&'a str> {
    jsobj::metadata_str(item, key).filter(|v| !v.is_empty())
}

fn sort_index(i: usize) -> String {
    format!("{:0>4}", i + 2)
}

// --- Locales ---------------------------------------------------------------------------------------------------

const COMMON_LOCALES: &[(&str, &str, &str)] = &[
    ("en-US", "English (United States)", "english united states usa us america"),
    ("en-CA", "English (Canada)", "english canada canadian ca"),
    ("en-GB", "English (United Kingdom)", "english united kingdom great britain uk gb"),
    ("fr-FR", "French (France)", "french france francais"),
    ("fr-CA", "French (Canada)", "french canada canadian quebec francais"),
    ("de-DE", "German (Germany)", "german germany deutsch deutschland"),
    ("es-ES", "Spanish (Spain)", "spanish spain espanol espana"),
    ("es-MX", "Spanish (Mexico)", "spanish mexico mexican espanol"),
    ("ja-JP", "Japanese (Japan)", "japanese japan nihongo"),
    ("ko-KR", "Korean (South Korea)", "korean korea hangul"),
    ("zh-CN", "Chinese (China, Simplified)", "chinese china simplified mandarin"),
    ("zh-TW", "Chinese (Taiwan, Traditional)", "chinese taiwan traditional mandarin"),
    ("pt-BR", "Portuguese (Brazil)", "portuguese brazil brasil portugues"),
    ("it-IT", "Italian (Italy)", "italian italy italiano"),
    ("nl-NL", "Dutch (Netherlands)", "dutch netherlands nederland nederlands"),
    ("sv-SE", "Swedish (Sweden)", "swedish sweden svenska"),
    ("pl-PL", "Polish (Poland)", "polish poland polski"),
    ("ru-RU", "Russian (Russia)", "russian russia russkiy"),
    ("hi-IN", "Hindi (India)", "hindi india devanagari"),
    ("ar-SA", "Arabic (Saudi Arabia)", "arabic saudi arabia"),
];

fn option(
    value: &str,
    display_name: String,
    description: &str,
    search_text: String,
    sort_text: String,
) -> SearchOption {
    SearchOption { value: value.into(), display_name, description: description.into(), search_text, sort_text }
}

/// `appendUniqueLocale`.
fn append_unique(options: &mut Vec<SearchOption>, option: SearchOption) {
    if !options.iter().any(|o| o.value == option.value) {
        options.push(option);
    }
}

/// `getLocaleOptions`.
fn locale_options(current_locale: Option<&str>) -> Vec<SearchOption> {
    let mut options = vec![option(
        DEFAULT_RESET_LOCALE,
        DEFAULT_RESET_LOCALE.into(),
        "Default reset timestamp locale",
        "default english united states usa us america reset timestamp".into(),
        format!("0000 {DEFAULT_RESET_LOCALE}"),
    )];
    let system_locale = intl::system_locale();
    if let Some(system) = system_locale.as_deref().filter(|l| *l != DEFAULT_RESET_LOCALE) {
        append_unique(
            &mut options,
            option(
                system,
                format!("System ({system})"),
                "Current system locale",
                format!("system current local machine runtime {system}"),
                format!("0001 {system}"),
            ),
        );
    }
    let configured = current_locale.and_then(intl::canonicalize_locale);
    if let Some(configured) =
        configured.as_deref().filter(|c| *c != DEFAULT_RESET_LOCALE && Some(*c) != system_locale.as_deref())
    {
        append_unique(
            &mut options,
            option(
                configured,
                configured.into(),
                "Configured locale",
                format!("configured current {configured}"),
                format!("0002 {configured}"),
            ),
        );
    }
    for (i, (value, description, search_text)) in COMMON_LOCALES.iter().enumerate() {
        append_unique(
            &mut options,
            option(value, (*value).into(), description, (*search_text).into(), format!("{} {value}", sort_index(i))),
        );
    }
    options
}

/// `filterLocaleOptions`: a typed locale the list lacks comes first as `Use <locale>`.
fn filter_locale_options(options: &[SearchOption], query: &str) -> Vec<SearchOption> {
    let matches = fuzzy_filter_options(options, query);
    let canonical_query = match intl::canonicalize_locale(query) {
        Some(q) if !options.iter().any(|o| o.value == q) => q,
        _ => return matches,
    };
    let custom = option(
        &canonical_query,
        format!("Use {canonical_query}"),
        "Custom locale",
        format!("custom typed {canonical_query}"),
        format!("0000 {canonical_query}"),
    );
    std::iter::once(custom).chain(matches).collect()
}

/// `setUsageLocale`: the default locale (or one that does not canonicalize) removes the key.
fn set_usage_locale(item: &Value, locale: &str) -> Value {
    match intl::canonicalize_locale(locale) {
        Some(canonical) if canonical != DEFAULT_RESET_LOCALE => jsobj::set_metadata(item, "locale", &canonical),
        _ => jsobj::remove_metadata_keys(item, &["locale"]),
    }
}

/// `UsageLocaleEditor`.
fn locale_editor(widget: &Value) -> SearchableOptionEditor {
    let current = non_empty_metadata(widget, "locale");
    let initial = match current {
        Some(c) => intl::canonicalize_locale(c),
        None => Some(DEFAULT_RESET_LOCALE.to_string()),
    };
    let item = widget.clone();
    SearchableOptionEditor::new(
        "Locale",
        current.unwrap_or(DEFAULT_RESET_LOCALE).to_string(),
        initial.as_deref(),
        locale_options(current),
        filter_locale_options,
        "No locales match the search.",
        Box::new(move |locale| set_usage_locale(&item, locale)),
    )
}

// --- Timezones -------------------------------------------------------------------------------------------------

/// `createIanaTimezoneOption`.
fn iana_option(timezone: &str, index: usize) -> SearchOption {
    option(
        timezone,
        timezone.into(),
        "IANA timezone",
        timezone.replace(['/', '_', '-'], " "),
        format!("{} {timezone}", sort_index(index)),
    )
}

/// `getTimezoneOptions`.
fn timezone_options(current: Option<&str>) -> Vec<SearchOption> {
    let local = intl::local_timezone();
    let suffix = local.as_deref().map(|l| format!(" ({l})")).unwrap_or_default();
    let mut options = vec![
        option(
            "UTC",
            "UTC".into(),
            "Default UTC reset timestamp",
            "utc default coordinated universal time z".into(),
            "0000 UTC".into(),
        ),
        option(
            "local",
            format!("Local{suffix}"),
            "Use this system timezone",
            ["local", "system", "machine", "browser", local.as_deref().unwrap_or("")].join(" "),
            format!("0001 Local{suffix}"),
        ),
    ];
    options.extend(
        intl::supported_time_zones().iter().filter(|tz| **tz != "UTC").enumerate().map(|(i, tz)| iana_option(tz, i)),
    );
    let Some(current) = current else { return options };
    let has_current = current != "UTC" && current != "local" && options.iter().any(|o| o.value == current);
    if has_current || current == "UTC" || current == "local" || !intl::is_valid_timezone(current) {
        return options;
    }
    let configured = SearchOption { description: "Configured timezone".into(), ..iana_option(current, 0) };
    options.insert(2, configured);
    options
}

/// `setUsageTimezone`: UTC, the default, removes the key.
fn set_usage_timezone(item: &Value, timezone: &str) -> Value {
    if timezone == "UTC" {
        return jsobj::remove_metadata_keys(item, &["timezone"]);
    }
    jsobj::set_metadata(item, "timezone", timezone)
}

/// `UsageTimezoneEditor`.
fn timezone_editor(widget: &Value) -> SearchableOptionEditor {
    let current = non_empty_metadata(widget, "timezone");
    let item = widget.clone();
    SearchableOptionEditor::new(
        "Timezone",
        current.unwrap_or("UTC").to_string(),
        Some(current.unwrap_or("UTC")),
        timezone_options(current),
        fuzzy_filter_options,
        "No timezones match the search.",
        Box::new(move |timezone| set_usage_timezone(&item, timezone)),
    )
}
