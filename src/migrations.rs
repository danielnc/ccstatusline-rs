//! Port of upstream `src/utils/migrations.ts` (v1 → v2 → v3 → v4), operating on
//! raw JSON so key order matches what upstream writes back.

use serde_json::{Map, Value};

fn guid() -> String {
    // UUID v4 from the OS random source (upstream uses Math.random).
    let mut bytes = [0u8; 16];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        use std::io::Read;
        let _ = f.read_exact(&mut bytes);
    }
    let mut out = String::with_capacity(36);
    let template = "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx";
    let mut i = 0;
    for c in template.chars() {
        match c {
            'x' | 'y' => {
                let r = (bytes[i / 2] >> ((i % 2) * 4)) & 0xf;
                i += 1;
                let v = if c == 'x' { r } else { (r & 0x3) | 0x8 };
                out.push(std::char::from_digit(v as u32, 16).unwrap());
            }
            c => out.push(c),
        }
    }
    out
}

type FieldCheck = fn(&Value) -> bool;

const V1_FIELDS: &[(&str, FieldCheck)] = &[
    ("flexMode", Value::is_string),
    ("compactThreshold", Value::is_number),
    ("colorLevel", Value::is_number),
    ("defaultSeparator", Value::is_string),
    ("defaultPadding", Value::is_string),
    ("inheritSeparatorColors", Value::is_boolean),
    ("overrideBackgroundColor", Value::is_string),
    ("overrideForegroundColor", Value::is_string),
    ("globalBold", Value::is_boolean),
];

fn to_widget_line(line: &[Value], strip_separators: bool) -> Vec<Value> {
    line.iter()
        .filter(|item| {
            !(strip_separators
                && item.as_object().is_some_and(|o| o.get("type").and_then(Value::as_str) == Some("separator")))
        })
        .filter_map(|item| {
            let o = item.as_object()?;
            let t = o.get("type")?.as_str()?.to_string();
            let mut m = o.clone();
            m.insert("id".into(), Value::String(guid()));
            m.insert("type".into(), Value::String(t));
            Some(Value::Object(m))
        })
        .collect()
}

fn migrate_v1(data: &Map<String, Value>) -> Map<String, Value> {
    let mut out = Map::new();
    if let Some(Value::Array(lines)) = data.get("lines") {
        let strip = match data.get("defaultSeparator") {
            Some(Value::String(s)) => !s.is_empty(),
            Some(Value::Null) | None => false,
            Some(Value::Bool(b)) => *b,
            Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
            Some(_) => true,
        };
        let processed: Vec<Value> =
            lines.iter().filter_map(|l| l.as_array().map(|a| Value::Array(to_widget_line(a, strip)))).collect();
        out.insert("lines".into(), Value::Array(processed));
    }
    for (key, valid) in V1_FIELDS {
        if let Some(v) = data.get(*key) {
            if valid(v) {
                out.insert((*key).into(), v.clone());
            }
        }
    }
    out.insert("version".into(), Value::from(2));
    let mut um = Map::new();
    um.insert("message".into(), Value::String("ccstatusline updated to v2.0.0, launch tui to use new settings".into()));
    um.insert("remaining".into(), Value::from(12));
    out.insert("updatemessage".into(), Value::Object(um));
    out
}

fn migrate_v2(data: &Map<String, Value>) -> Map<String, Value> {
    let mut out = data.clone();
    out.insert("version".into(), Value::from(3));
    let mut um = Map::new();
    um.insert("message".into(), Value::String("ccstatusline updated to v2.0.2, 5hr block timer widget added".into()));
    um.insert("remaining".into(), Value::from(12));
    out.insert("updatemessage".into(), Value::Object(um));
    out
}

struct HideRule {
    legacy: &'static [(&'static str, &'static [&'static str])],
    order: &'static [&'static str],
    defaults: &'static [&'static str],
}

const NO_GIT: HideRule = HideRule { legacy: &[("hideNoGit", &["no-git"])], order: &["no-git"], defaults: &[] };
const NO_GIT_ZERO: HideRule =
    HideRule { legacy: &[("hideNoGit", &["no-git"])], order: &["no-git", "zero"], defaults: &[] };
const NO_GIT_NO_DATA: HideRule =
    HideRule { legacy: &[("hideNoGit", &["no-git"])], order: &["no-git", "no-data"], defaults: &[] };
const NO_JJ: HideRule = HideRule { legacy: &[("hideNoJj", &["no-jj"])], order: &["no-jj"], defaults: &[] };
const NO_REMOTE: HideRule =
    HideRule { legacy: &[("hideNoRemote", &["no-remote"])], order: &["no-remote"], defaults: &[] };
const NO_UPSTREAM: HideRule =
    HideRule { legacy: &[("hideNoRemote", &["no-upstream"])], order: &["no-upstream"], defaults: &[] };
const GIT_REVIEW: HideRule = HideRule {
    legacy: &[("hideNoGit", &["no-git", "no-data"]), ("hideStatus", &["status"]), ("hideTitle", &["title"])],
    order: &["no-git", "no-data", "status", "title"],
    defaults: &[],
};
const EXTRA_USAGE: HideRule =
    HideRule { legacy: &[("hideIfDisabled", &["disabled"])], order: &["disabled", "no-data"], defaults: &[] };
const CACHE: HideRule = HideRule { legacy: &[("hideWhenEmpty", &["empty"])], order: &["empty"], defaults: &[] };
const AHEAD_BEHIND: HideRule = HideRule {
    legacy: &[("hideNoGit", &["no-git", "no-upstream"])],
    order: &["no-git", "no-upstream", "zero"],
    defaults: &["zero"],
};
const NOT_FORK: HideRule =
    HideRule { legacy: &[("hideWhenNotFork", &["not-fork"])], order: &["not-fork"], defaults: &[] };
const ZERO: HideRule = HideRule { legacy: &[("hideZero", &["zero"])], order: &["zero"], defaults: &[] };
const EMPTY: HideRule = HideRule { legacy: &[("hideWhenEmpty", &["empty"])], order: &["empty"], defaults: &[] };

fn rule_for(kind: &str) -> Option<&'static HideRule> {
    Some(match kind {
        "git-branch" | "git-clean-status" | "git-root-dir" | "git-worktree" | "git-status" | "git-staged"
        | "git-unstaged" | "git-untracked" | "git-sha" => &NO_GIT,
        "git-changes"
        | "git-insertions"
        | "git-deletions"
        | "git-staged-files"
        | "git-unstaged-files"
        | "git-untracked-files"
        | "git-conflicts" => &NO_GIT_ZERO,
        "git-ci-status" => &NO_GIT_NO_DATA,
        "git-ahead-behind" => &AHEAD_BEHIND,
        "git-review" | "git-pr" => &GIT_REVIEW,
        "git-origin-owner" | "git-origin-repo" | "git-origin-owner-repo" => &NO_REMOTE,
        "git-upstream-owner" | "git-upstream-repo" | "git-upstream-owner-repo" => &NO_UPSTREAM,
        "git-is-fork" => &NOT_FORK,
        "jj-bookmarks" | "jj-workspace" | "jj-root-dir" | "jj-changes" | "jj-insertions" | "jj-deletions"
        | "jj-description" | "jj-revision" => &NO_JJ,
        "compaction-counter" => &ZERO,
        "skills" => &EMPTY,
        "cache-read" | "cache-write" | "cache-hit-rate" | "cache-timer" => &CACHE,
        "extra-usage-utilization" | "extra-usage-remaining" | "extra-usage-used" => &EXTRA_USAGE,
        _ => return None,
    })
}

const LEGACY_KEYS: &[&str] = &[
    "hideNoGit",
    "hideNoJj",
    "hideNoRemote",
    "hideZero",
    "hideWhenEmpty",
    "hideIfDisabled",
    "hideStatus",
    "hideTitle",
    "hideWhenNotFork",
];

fn migrate_item_hide_flags(item: &Value) -> Value {
    let Some(o) = item.as_object() else { return item.clone() };
    let Some(kind) = o.get("type").and_then(Value::as_str) else { return item.clone() };
    let Some(metadata) = o.get("metadata").and_then(Value::as_object) else { return item.clone() };
    let Some(rule) = rule_for(kind) else { return item.clone() };
    let present: Vec<&str> = LEGACY_KEYS.iter().copied().filter(|k| metadata.contains_key(*k)).collect();
    let legacy_zero = kind == "git-conflicts" && metadata.get("zeroDisplay").and_then(Value::as_str) == Some("hidden");
    if present.is_empty() && !legacy_zero {
        return item.clone();
    }
    let written = metadata.get("hide").and_then(Value::as_str);
    let mut enabled: Vec<String> = Vec::new();
    let add = |s: &str, enabled: &mut Vec<String>| {
        if !enabled.iter().any(|e| e == s) {
            enabled.push(s.to_string());
        }
    };
    match written {
        None => rule.defaults.iter().for_each(|d| add(d, &mut enabled)),
        Some(w) => {
            for k in w.split(',') {
                let t = crate::jsnum::js_trim(k);
                if !t.is_empty() {
                    add(t, &mut enabled);
                }
            }
        }
    }
    for key in &present {
        if metadata.get(*key).and_then(Value::as_str) == Some("true") {
            if let Some((_, states)) = rule.legacy.iter().find(|(k, _)| k == key) {
                states.iter().for_each(|s| add(s, &mut enabled));
            }
        }
    }
    if legacy_zero {
        add("zero", &mut enabled);
    }
    let mut next: Map<String, Value> = metadata
        .iter()
        .filter(|(k, _)| {
            !LEGACY_KEYS.contains(&k.as_str()) && k.as_str() != "hide" && !(legacy_zero && k.as_str() == "zeroDisplay")
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let ordered: Vec<&str> = rule.order.iter().copied().filter(|s| enabled.iter().any(|e| e == s)).collect();
    let matches_defaults = ordered.len() == rule.defaults.len() && ordered.iter().all(|s| rule.defaults.contains(s));
    if !matches_defaults {
        next.insert("hide".into(), Value::String(ordered.join(",")));
    }
    let mut out = o.clone();
    if !next.is_empty() {
        out.insert("metadata".into(), Value::Object(next));
    } else {
        out.shift_remove("metadata");
    }
    Value::Object(out)
}

fn migrate_v3(data: &Map<String, Value>) -> Map<String, Value> {
    let mut out = data.clone();
    if let Some(Value::Array(lines)) = data.get("lines") {
        let migrated: Vec<Value> = lines
            .iter()
            .map(|l| match l {
                Value::Array(items) => Value::Array(items.iter().map(migrate_item_hide_flags).collect()),
                other => other.clone(),
            })
            .collect();
        out.insert("lines".into(), Value::Array(migrated));
    }
    out.insert("version".into(), Value::from(4));
    out
}

pub fn detect_version(data: &Value) -> f64 {
    match data.as_object().and_then(|o| o.get("version")) {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(1.0),
        _ => 1.0,
    }
}

pub fn migrate_config(data: Value, target: f64) -> Value {
    let Value::Object(map) = data else { return data };
    let mut current = detect_version(&Value::Object(map.clone()));
    let mut migrated = map;
    while current < target {
        let step: fn(&Map<String, Value>) -> Map<String, Value> = if current == 1.0 {
            migrate_v1
        } else if current == 2.0 {
            migrate_v2
        } else if current == 3.0 {
            migrate_v3
        } else {
            break;
        };
        migrated = step(&migrated);
        current += 1.0;
    }
    Value::Object(migrated)
}

pub fn needs_migration(data: &Value, target: f64) -> bool {
    if let Some(o) = data.as_object() {
        if let Some(v) = o.get("version") {
            if !v.is_number() {
                return false;
            }
        }
    }
    detect_version(data) < target
}
