//! JavaScript object semantics on `serde_json::Value`, for code that edits settings the way upstream's editor
//! does and must save the same bytes.
//!
//! Property order is insertion order (`preserve_order`). `{...o, k: v}` keeps an existing key where it is and
//! appends a new one; destructuring `const {k, ...rest} = o` removes the key. A property set to `undefined` keeps
//! its place but is skipped by `JSON.stringify`, so it is stored as a marker value: [`get`] treats it as absent,
//! and [`stringify`] and [`strip_undefined`] drop it.

use serde_json::{Map, Value};

const UNDEFINED: &str = "\u{0}undefined\u{0}";

pub fn undefined() -> Value {
    Value::String(UNDEFINED.to_string())
}

pub fn is_undefined(v: &Value) -> bool {
    v.as_str() == Some(UNDEFINED)
}

/// `o[key]`, with `undefined` properties reading as absent.
pub fn get<'a>(o: &'a Value, key: &str) -> Option<&'a Value> {
    o.get(key).filter(|v| !is_undefined(v))
}

pub fn get_str<'a>(o: &'a Value, key: &str) -> Option<&'a str> {
    get(o, key).and_then(Value::as_str)
}

pub fn get_bool(o: &Value, key: &str) -> Option<bool> {
    get(o, key).and_then(Value::as_bool)
}

/// `{...o, [key]: value}`.
pub fn with(o: &Value, key: &str, value: Value) -> Value {
    let mut out = o.clone();
    if let Some(m) = out.as_object_mut() {
        m.insert(key.to_string(), value);
    }
    out
}

/// `const { [key]: _, ...rest } = o; rest`.
pub fn without(o: &Value, key: &str) -> Value {
    let mut out = o.clone();
    if let Some(m) = out.as_object_mut() {
        m.shift_remove(key);
    }
    out
}

/// `{...a, ...b}`.
pub fn merge(a: &Value, b: &Value) -> Value {
    let mut out = a.clone();
    if let (Some(m), Some(bm)) = (out.as_object_mut(), b.as_object()) {
        for (k, v) in bm {
            m.insert(k.clone(), v.clone());
        }
    }
    out
}

pub fn object(pairs: Vec<(&str, Value)>) -> Value {
    let mut m = Map::new();
    for (k, v) in pairs {
        m.insert(k.to_string(), v);
    }
    Value::Object(m)
}

/// The value as `JSON.stringify` sees it: `undefined` properties gone, `undefined` array elements `null`.
pub fn strip_undefined(v: &Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(
            m.iter().filter(|(_, v)| !is_undefined(v)).map(|(k, v)| (k.clone(), strip_undefined(v))).collect(),
        ),
        Value::Array(a) => {
            Value::Array(a.iter().map(|v| if is_undefined(v) { Value::Null } else { strip_undefined(v) }).collect())
        }
        other => other.clone(),
    }
}

/// `JSON.stringify(v)`.
pub fn stringify(v: &Value) -> String {
    crate::jsjson::stringify(&strip_undefined(v))
}

/// `JSON.stringify(v, null, 2)`.
pub fn stringify_pretty(v: &Value) -> String {
    crate::jsjson::stringify_pretty(&strip_undefined(v))
}

// --- Widget item metadata (upstream `src/widgets/shared/metadata.ts`) ------------------------------------------

pub fn metadata_str<'a>(item: &'a Value, key: &str) -> Option<&'a str> {
    get(item, "metadata").and_then(|m| get_str(m, key))
}

/// `{...item, metadata: {...item.metadata, [key]: value}}`.
pub fn set_metadata(item: &Value, key: &str, value: &str) -> Value {
    let meta = get(item, "metadata").cloned().unwrap_or_else(|| Value::Object(Map::new()));
    with(item, "metadata", with(&meta, key, Value::String(value.to_string())))
}

/// `removeMetadataKeys`: the remaining metadata, or `metadata: undefined` when none is left.
pub fn remove_metadata_keys(item: &Value, keys: &[&str]) -> Value {
    let next: Map<String, Value> = get(item, "metadata")
        .and_then(Value::as_object)
        .map(|m| m.iter().filter(|(k, _)| !keys.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();
    with(item, "metadata", if next.is_empty() { undefined() } else { Value::Object(next) })
}

/// `isMetadataFlagEnabled`.
pub fn metadata_flag(item: &Value, key: &str) -> bool {
    metadata_str(item, key) == Some("true")
}

/// `toggleMetadataFlag`.
pub fn toggle_metadata_flag(item: &Value, key: &str) -> Value {
    set_metadata(item, key, if metadata_flag(item, key) { "false" } else { "true" })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn spreads_keep_positions() {
        let o = json!({"id": "1", "type": "x", "metadata": {"a": "1"}});
        let cleared = remove_metadata_keys(&o, &["a"]);
        assert_eq!(stringify(&cleared), r#"{"id":"1","type":"x"}"#);
        let again = set_metadata(&cleared, "b", "2");
        assert_eq!(stringify(&again), r#"{"id":"1","type":"x","metadata":{"b":"2"}}"#);
        let raw = with(&o, "rawValue", Value::Bool(true));
        assert_eq!(stringify(&without(&raw, "metadata")), r#"{"id":"1","type":"x","rawValue":true}"#);
    }
}

#[cfg(test)]
mod more_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn flags_merges_and_undefined_in_arrays() {
        let o = json!({"id": "1"});
        let on = toggle_metadata_flag(&o, "nerdFont");
        assert!(metadata_flag(&on, "nerdFont"));
        assert!(!metadata_flag(&toggle_metadata_flag(&on, "nerdFont"), "nerdFont"));
        let merged = merge(&o, &json!({"b": 1, "id": "2"}));
        assert_eq!(stringify(&merged), r#"{"id":"2","b":1}"#);
        assert_eq!(get_bool(&json!({"x": true}), "x"), Some(true));
        let built = object(vec![("a", Value::Array(vec![undefined(), json!(1)])), ("b", undefined())]);
        assert_eq!(stringify(&built), r#"{"a":[null,1]}"#);
        assert!(stringify_pretty(&built).contains("null"));
        assert!(get(&built, "b").is_none());
    }
}
