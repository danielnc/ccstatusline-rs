//! Settings schema (upstream `src/types/Settings.ts` + `Widget.ts`), zod-like
//! validation, and the loader/saver from `src/utils/config.ts`.

use crate::jsjson;
use indexmap::IndexMap;
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

pub const CURRENT_VERSION: f64 = 4.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DimSetting {
    Bool(bool),
    Parens,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeSetting {
    Bool(bool),
    NoPadding,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NumberFormat {
    pub style: Option<String>,
    pub decimals: Option<f64>,
}

#[derive(Clone, Debug, Default)]
pub struct WidgetItem {
    pub id: String,
    pub kind: String,
    pub color: Option<String>,
    pub background_color: Option<String>,
    pub bold: Option<bool>,
    pub dim: Option<DimSetting>,
    pub number_format: Option<NumberFormat>,
    pub character: Option<String>,
    pub raw_value: Option<bool>,
    pub custom_text: Option<String>,
    pub custom_symbol: Option<String>,
    pub command_path: Option<String>,
    pub max_width: Option<f64>,
    pub preserve_colors: Option<bool>,
    pub timeout: Option<f64>,
    pub merge: Option<MergeSetting>,
    pub exclude_from_auto_align: Option<bool>,
    pub metadata: Option<IndexMap<String, String>>,
}

impl WidgetItem {
    pub fn meta(&self, key: &str) -> Option<&str> {
        self.metadata.as_ref().and_then(|m| m.get(key)).map(|s| s.as_str())
    }
    pub fn raw(&self) -> bool {
        self.raw_value == Some(true)
    }
    pub fn is_bold(&self) -> bool {
        self.bold == Some(true)
    }
    /// JS truthiness of `item.merge`.
    pub fn merges(&self) -> bool {
        matches!(self.merge, Some(MergeSetting::Bool(true)) | Some(MergeSetting::NoPadding))
    }
    pub fn merge_no_padding(&self) -> bool {
        self.merge == Some(MergeSetting::NoPadding)
    }
    pub fn dim_value(&self) -> crate::colors::Dim {
        match self.dim {
            Some(DimSetting::Bool(true)) => crate::colors::Dim::Yes,
            Some(DimSetting::Parens) => crate::colors::Dim::Parens,
            _ => crate::colors::Dim::No,
        }
    }
    pub fn is_separator(&self) -> bool {
        self.kind == "separator" || self.kind == "flex-separator"
    }
}

#[derive(Clone, Debug)]
pub struct PowerlineConfig {
    pub enabled: bool,
    pub separators: Vec<String>,
    pub separator_invert_background: Vec<bool>,
    pub start_caps: Vec<String>,
    pub end_caps: Vec<String>,
    pub theme: Option<String>,
    pub auto_align: bool,
    pub continue_theme_across_lines: bool,
}

impl Default for PowerlineConfig {
    fn default() -> Self {
        PowerlineConfig {
            enabled: false,
            separators: vec!["\u{e0b0}".into()],
            separator_invert_background: vec![false],
            start_caps: vec![],
            end_caps: vec![],
            theme: None,
            auto_align: false,
            continue_theme_across_lines: false,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct GlobalNumberFormat {
    pub token: Option<NumberFormat>,
    pub speed: Option<NumberFormat>,
    pub percent: Option<NumberFormat>,
    pub memory: Option<NumberFormat>,
    pub cost: Option<NumberFormat>,
}

#[derive(Clone, Debug)]
pub struct UpdateMessage {
    pub message: Option<Option<String>>,
    pub remaining: Option<Option<f64>>,
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub version: f64,
    pub lines: Vec<Vec<WidgetItem>>,
    pub flex_mode: String,
    pub compact_threshold: f64,
    pub color_level: u8,
    pub default_separator: Option<String>,
    pub default_padding: Option<String>,
    pub default_padding_side: String,
    pub inherit_separator_colors: bool,
    pub override_background_color: Option<String>,
    pub override_foreground_color: Option<String>,
    pub global_bold: bool,
    pub number_format: Option<GlobalNumberFormat>,
    pub git_cache_ttl_seconds: f64,
    pub terminal_width_cache_ttl_seconds: f64,
    pub custom_command_cache_ttl_seconds: f64,
    pub minimalist_mode: bool,
    pub powerline: PowerlineConfig,
    pub updatemessage: Option<UpdateMessage>,
    /// Validated installation metadata, kept as JSON (only the TUI uses it).
    pub installation: Option<Value>,
}

fn default_lines() -> Vec<Vec<WidgetItem>> {
    let w = |id: &str, kind: &str, color: Option<&str>| WidgetItem {
        id: id.into(),
        kind: kind.into(),
        color: color.map(String::from),
        ..Default::default()
    };
    vec![
        vec![
            w("1", "model", Some("cyan")),
            w("2", "separator", None),
            w("3", "context-length", Some("brightBlack")),
            w("4", "separator", None),
            w("5", "git-branch", Some("magenta")),
            w("6", "separator", None),
            w("7", "git-changes", Some("yellow")),
        ],
        vec![],
        vec![],
    ]
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            version: CURRENT_VERSION,
            lines: default_lines(),
            flex_mode: "full".into(),
            compact_threshold: 60.0,
            color_level: 2,
            default_separator: None,
            default_padding: None,
            default_padding_side: "both".into(),
            inherit_separator_colors: false,
            override_background_color: None,
            override_foreground_color: None,
            global_bold: false,
            number_format: None,
            git_cache_ttl_seconds: 5.0,
            terminal_width_cache_ttl_seconds: 5.0,
            custom_command_cache_ttl_seconds: 0.0,
            minimalist_mode: false,
            powerline: PowerlineConfig::default(),
            updatemessage: None,
            installation: None,
        }
    }
}

// ---------------------------------------------------------------- validation

type R<T> = Result<T, String>;

fn opt<'a>(o: &'a Map<String, Value>, key: &str) -> Option<&'a Value> {
    o.get(key)
}

fn get_str(o: &Map<String, Value>, key: &str) -> R<Option<String>> {
    match opt(o, key) {
        None => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(format!("{key}: expected string")),
    }
}

fn get_bool(o: &Map<String, Value>, key: &str) -> R<Option<bool>> {
    match opt(o, key) {
        None => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(format!("{key}: expected boolean")),
    }
}

fn get_num(o: &Map<String, Value>, key: &str) -> R<Option<f64>> {
    match opt(o, key) {
        None => Ok(None),
        Some(Value::Number(n)) => Ok(n.as_f64()),
        Some(_) => Err(format!("{key}: expected number")),
    }
}

fn get_num_range(o: &Map<String, Value>, key: &str, min: f64, max: f64) -> R<Option<f64>> {
    let v = get_num(o, key)?;
    if let Some(n) = v {
        if n < min || n > max {
            return Err(format!("{key}: out of range"));
        }
    }
    Ok(v)
}

fn parse_number_format(v: &Value) -> R<NumberFormat> {
    let o = v.as_object().ok_or("numberFormat: expected object")?;
    let style = get_str(o, "style")?;
    if let Some(s) = &style {
        if !matches!(s.as_str(), "precise" | "compact" | "whole") {
            return Err("numberFormat.style: invalid".into());
        }
    }
    let decimals = get_num(o, "decimals")?;
    if let Some(d) = decimals {
        if !crate::jsnum::is_integer(d) || !(0.0..=6.0).contains(&d) {
            return Err("numberFormat.decimals: invalid".into());
        }
    }
    Ok(NumberFormat { style, decimals })
}

pub fn parse_widget_item(v: &Value) -> R<WidgetItem> {
    let o = v.as_object().ok_or("item: expected object")?;
    let id = get_str(o, "id")?.ok_or("id: required")?;
    let kind = get_str(o, "type")?.ok_or("type: required")?;
    let dim = match opt(o, "dim") {
        None => None,
        Some(Value::Bool(b)) => Some(DimSetting::Bool(*b)),
        Some(Value::String(s)) if s == "parens" => Some(DimSetting::Parens),
        Some(_) => return Err("dim: invalid".into()),
    };
    let merge = match opt(o, "merge") {
        None => None,
        Some(Value::Bool(b)) => Some(MergeSetting::Bool(*b)),
        Some(Value::String(s)) if s == "no-padding" => Some(MergeSetting::NoPadding),
        Some(_) => return Err("merge: invalid".into()),
    };
    let number_format = match opt(o, "numberFormat") {
        None => None,
        Some(v) => Some(parse_number_format(v)?),
    };
    let metadata = match opt(o, "metadata") {
        None => None,
        Some(Value::Object(m)) => {
            let mut out = IndexMap::new();
            for (k, v) in m {
                match v {
                    Value::String(s) => {
                        out.insert(k.clone(), s.clone());
                    }
                    _ => return Err("metadata: expected string values".into()),
                }
            }
            Some(out)
        }
        Some(_) => return Err("metadata: expected object".into()),
    };
    Ok(WidgetItem {
        id,
        kind,
        color: get_str(o, "color")?,
        background_color: get_str(o, "backgroundColor")?,
        bold: get_bool(o, "bold")?,
        dim,
        number_format,
        character: get_str(o, "character")?,
        raw_value: get_bool(o, "rawValue")?,
        custom_text: get_str(o, "customText")?,
        custom_symbol: get_str(o, "customSymbol")?,
        command_path: get_str(o, "commandPath")?,
        max_width: get_num(o, "maxWidth")?,
        preserve_colors: get_bool(o, "preserveColors")?,
        timeout: get_num(o, "timeout")?,
        merge,
        exclude_from_auto_align: get_bool(o, "excludeFromAutoAlign")?,
        metadata,
    })
}

fn parse_lines(v: &Value) -> R<Vec<Vec<WidgetItem>>> {
    let arr = v.as_array().ok_or("lines: expected array")?;
    arr.iter()
        .map(|line| {
            line.as_array()
                .ok_or_else(|| "lines[]: expected array".to_string())?
                .iter()
                .map(parse_widget_item)
                .collect::<R<Vec<_>>>()
        })
        .collect()
}

fn parse_str_array(v: &Value, key: &str) -> R<Vec<String>> {
    v.as_array()
        .ok_or(format!("{key}: expected array"))?
        .iter()
        .map(|x| x.as_str().map(String::from).ok_or(format!("{key}[]: expected string")))
        .collect()
}

fn parse_powerline(v: &Value) -> R<PowerlineConfig> {
    let o = v.as_object().ok_or("powerline: expected object")?;
    let d = PowerlineConfig::default();
    Ok(PowerlineConfig {
        enabled: get_bool(o, "enabled")?.unwrap_or(d.enabled),
        separators: match opt(o, "separators") {
            None => d.separators,
            Some(v) => parse_str_array(v, "separators")?,
        },
        separator_invert_background: match opt(o, "separatorInvertBackground") {
            None => d.separator_invert_background,
            Some(v) => v
                .as_array()
                .ok_or("separatorInvertBackground: expected array")?
                .iter()
                .map(|x| x.as_bool().ok_or_else(|| "separatorInvertBackground[]: expected boolean".to_string()))
                .collect::<R<Vec<_>>>()?,
        },
        start_caps: match opt(o, "startCaps") {
            None => vec![],
            Some(v) => parse_str_array(v, "startCaps")?,
        },
        end_caps: match opt(o, "endCaps") {
            None => vec![],
            Some(v) => parse_str_array(v, "endCaps")?,
        },
        theme: get_str(o, "theme")?,
        auto_align: get_bool(o, "autoAlign")?.unwrap_or(false),
        continue_theme_across_lines: get_bool(o, "continueThemeAcrossLines")?.unwrap_or(false),
    })
}

fn parse_installation(v: &Value) -> R<Value> {
    let o = v.as_object().ok_or("installation: expected object")?;
    let method = get_str(o, "method")?.ok_or("installation.method: required")?;
    let pm = get_str(o, "packageManager")?;
    let mut out = Map::new();
    out.insert("method".into(), Value::String(method.clone()));
    match method.as_str() {
        "auto-update" => {
            let pm = pm.ok_or("packageManager: required")?;
            if !matches!(pm.as_str(), "npm" | "bun") {
                return Err("packageManager: invalid".into());
            }
            out.insert("packageManager".into(), Value::String(pm));
        }
        "pinned" => {
            if let Some(v) = get_str(o, "installedVersion")? {
                out.insert("installedVersion".into(), Value::String(v));
            }
        }
        "self-managed" | "unknown" => {
            let pm = pm.unwrap_or_else(|| "unknown".into());
            if !matches!(pm.as_str(), "npm" | "bun" | "unknown") {
                return Err("packageManager: invalid".into());
            }
            out.insert("packageManager".into(), Value::String(pm));
        }
        _ => return Err("installation.method: invalid".into()),
    }
    Ok(Value::Object(out))
}

fn parse_global_number_format(v: &Value) -> R<GlobalNumberFormat> {
    let o = v.as_object().ok_or("numberFormat: expected object")?;
    let f = |k: &str| -> R<Option<NumberFormat>> { opt(o, k).map(parse_number_format).transpose() };
    Ok(GlobalNumberFormat {
        token: f("token")?,
        speed: f("speed")?,
        percent: f("percent")?,
        memory: f("memory")?,
        cost: f("cost")?,
    })
}

/// `SettingsSchema.safeParse(value)`.
pub fn parse_settings(v: &Value) -> R<Settings> {
    let o = v.as_object().ok_or("settings: expected object")?;
    let d = Settings::default();
    let lines = match opt(o, "lines") {
        None => d.lines,
        Some(v) => {
            let l = parse_lines(v)?;
            if l.is_empty() {
                return Err("lines: too small".into());
            }
            l
        }
    };
    let flex_mode = get_str(o, "flexMode")?.unwrap_or(d.flex_mode);
    if !matches!(flex_mode.as_str(), "full" | "full-minus-40" | "full-until-compact") {
        return Err("flexMode: invalid".into());
    }
    let color_level = match get_num(o, "colorLevel")? {
        None => d.color_level,
        Some(n) if n == 0.0 || n == 1.0 || n == 2.0 || n == 3.0 => n as u8,
        Some(_) => return Err("colorLevel: invalid".into()),
    };
    let side = get_str(o, "defaultPaddingSide")?.unwrap_or(d.default_padding_side);
    if !matches!(side.as_str(), "both" | "left" | "right") {
        return Err("defaultPaddingSide: invalid".into());
    }
    let updatemessage = match opt(o, "updatemessage") {
        None => None,
        Some(Value::Object(m)) => Some(UpdateMessage {
            message: match m.get("message") {
                None => None,
                Some(Value::Null) => Some(None),
                Some(Value::String(s)) => Some(Some(s.clone())),
                Some(_) => return Err("updatemessage.message: invalid".into()),
            },
            remaining: match m.get("remaining") {
                None => None,
                Some(Value::Null) => Some(None),
                Some(Value::Number(n)) => Some(n.as_f64()),
                Some(_) => return Err("updatemessage.remaining: invalid".into()),
            },
        }),
        Some(_) => return Err("updatemessage: expected object".into()),
    };
    Ok(Settings {
        version: get_num(o, "version")?.unwrap_or(CURRENT_VERSION),
        lines,
        flex_mode,
        compact_threshold: get_num_range(o, "compactThreshold", 1.0, 99.0)?.unwrap_or(d.compact_threshold),
        color_level,
        default_separator: get_str(o, "defaultSeparator")?,
        default_padding: get_str(o, "defaultPadding")?,
        default_padding_side: side,
        inherit_separator_colors: get_bool(o, "inheritSeparatorColors")?.unwrap_or(false),
        override_background_color: get_str(o, "overrideBackgroundColor")?,
        override_foreground_color: get_str(o, "overrideForegroundColor")?,
        global_bold: get_bool(o, "globalBold")?.unwrap_or(false),
        number_format: opt(o, "numberFormat").map(parse_global_number_format).transpose()?,
        git_cache_ttl_seconds: get_num_range(o, "gitCacheTtlSeconds", 0.0, 60.0)?.unwrap_or(d.git_cache_ttl_seconds),
        terminal_width_cache_ttl_seconds: get_num_range(o, "terminalWidthCacheTtlSeconds", 0.0, 300.0)?
            .unwrap_or(d.terminal_width_cache_ttl_seconds),
        custom_command_cache_ttl_seconds: get_num_range(o, "customCommandCacheTtlSeconds", 0.0, 60.0)?
            .unwrap_or(d.custom_command_cache_ttl_seconds),
        minimalist_mode: get_bool(o, "minimalistMode")?.unwrap_or(false),
        powerline: match opt(o, "powerline") {
            None => d.powerline,
            Some(v) => parse_powerline(v)?,
        },
        updatemessage,
        installation: opt(o, "installation").map(parse_installation).transpose()?,
    })
}

/// `SettingsSchema_v1.safeParse(value)` (only validity matters).
fn validate_v1(v: &Value) -> R<()> {
    let o = v.as_object().ok_or("settings: expected object")?;
    if let Some(l) = opt(o, "lines") {
        parse_lines(l)?;
    }
    if let Some(f) = get_str(o, "flexMode")? {
        if !matches!(f.as_str(), "full" | "full-minus-40" | "full-until-compact") {
            return Err("flexMode: invalid".into());
        }
    }
    get_num(o, "compactThreshold")?;
    if let Some(n) = get_num(o, "colorLevel")? {
        if !(n == 0.0 || n == 1.0 || n == 2.0 || n == 3.0) {
            return Err("colorLevel: invalid".into());
        }
    }
    get_str(o, "defaultSeparator")?;
    get_str(o, "defaultPadding")?;
    get_bool(o, "inheritSeparatorColors")?;
    get_str(o, "overrideBackgroundColor")?;
    get_str(o, "overrideForegroundColor")?;
    get_bool(o, "globalBold")?;
    Ok(())
}

// ------------------------------------------------------------- serialization

fn nf_json(nf: &NumberFormat) -> Value {
    let mut m = Map::new();
    if let Some(s) = &nf.style {
        m.insert("style".into(), Value::String(s.clone()));
    }
    if let Some(d) = nf.decimals {
        m.insert("decimals".into(), jsjson::num(d));
    }
    Value::Object(m)
}

pub fn item_json(it: &WidgetItem) -> Value {
    let mut m = Map::new();
    let s = |v: &Option<String>| v.as_ref().map(|s| Value::String(s.clone()));
    m.insert("id".into(), Value::String(it.id.clone()));
    m.insert("type".into(), Value::String(it.kind.clone()));
    let mut put = |k: &str, v: Option<Value>| {
        if let Some(v) = v {
            m.insert(k.into(), v);
        }
    };
    put("color", s(&it.color));
    put("backgroundColor", s(&it.background_color));
    put("bold", it.bold.map(Value::Bool));
    put(
        "dim",
        it.dim.map(|d| match d {
            DimSetting::Bool(b) => Value::Bool(b),
            DimSetting::Parens => Value::String("parens".into()),
        }),
    );
    put("numberFormat", it.number_format.as_ref().map(nf_json));
    put("character", s(&it.character));
    put("rawValue", it.raw_value.map(Value::Bool));
    put("customText", s(&it.custom_text));
    put("customSymbol", s(&it.custom_symbol));
    put("commandPath", s(&it.command_path));
    put("maxWidth", it.max_width.map(jsjson::num));
    put("preserveColors", it.preserve_colors.map(Value::Bool));
    put("timeout", it.timeout.map(jsjson::num));
    put(
        "merge",
        it.merge.map(|d| match d {
            MergeSetting::Bool(b) => Value::Bool(b),
            MergeSetting::NoPadding => Value::String("no-padding".into()),
        }),
    );
    put("excludeFromAutoAlign", it.exclude_from_auto_align.map(Value::Bool));
    put(
        "metadata",
        it.metadata
            .as_ref()
            .map(|md| Value::Object(md.iter().map(|(k, v)| (k.clone(), Value::String(v.clone()))).collect())),
    );
    Value::Object(m)
}

/// The zod output object for these settings, keys in schema order.
pub fn settings_json(st: &Settings) -> Value {
    let mut m = Map::new();
    m.insert("version".into(), jsjson::num(st.version));
    m.insert(
        "lines".into(),
        Value::Array(st.lines.iter().map(|l| Value::Array(l.iter().map(item_json).collect())).collect()),
    );
    m.insert("flexMode".into(), Value::String(st.flex_mode.clone()));
    m.insert("compactThreshold".into(), jsjson::num(st.compact_threshold));
    m.insert("colorLevel".into(), jsjson::num(st.color_level as f64));
    if let Some(s) = &st.default_separator {
        m.insert("defaultSeparator".into(), Value::String(s.clone()));
    }
    if let Some(s) = &st.default_padding {
        m.insert("defaultPadding".into(), Value::String(s.clone()));
    }
    m.insert("defaultPaddingSide".into(), Value::String(st.default_padding_side.clone()));
    m.insert("inheritSeparatorColors".into(), Value::Bool(st.inherit_separator_colors));
    if let Some(s) = &st.override_background_color {
        m.insert("overrideBackgroundColor".into(), Value::String(s.clone()));
    }
    if let Some(s) = &st.override_foreground_color {
        m.insert("overrideForegroundColor".into(), Value::String(s.clone()));
    }
    m.insert("globalBold".into(), Value::Bool(st.global_bold));
    if let Some(g) = &st.number_format {
        let mut gm = Map::new();
        for (k, v) in [
            ("token", &g.token),
            ("speed", &g.speed),
            ("percent", &g.percent),
            ("memory", &g.memory),
            ("cost", &g.cost),
        ] {
            if let Some(v) = v {
                gm.insert(k.into(), nf_json(v));
            }
        }
        m.insert("numberFormat".into(), Value::Object(gm));
    }
    m.insert("gitCacheTtlSeconds".into(), jsjson::num(st.git_cache_ttl_seconds));
    m.insert("terminalWidthCacheTtlSeconds".into(), jsjson::num(st.terminal_width_cache_ttl_seconds));
    m.insert("customCommandCacheTtlSeconds".into(), jsjson::num(st.custom_command_cache_ttl_seconds));
    m.insert("minimalistMode".into(), Value::Bool(st.minimalist_mode));
    let p = &st.powerline;
    let mut pm = Map::new();
    pm.insert("enabled".into(), Value::Bool(p.enabled));
    pm.insert("separators".into(), Value::Array(p.separators.iter().cloned().map(Value::String).collect()));
    pm.insert(
        "separatorInvertBackground".into(),
        Value::Array(p.separator_invert_background.iter().map(|b| Value::Bool(*b)).collect()),
    );
    pm.insert("startCaps".into(), Value::Array(p.start_caps.iter().cloned().map(Value::String).collect()));
    pm.insert("endCaps".into(), Value::Array(p.end_caps.iter().cloned().map(Value::String).collect()));
    if let Some(t) = &p.theme {
        pm.insert("theme".into(), Value::String(t.clone()));
    }
    pm.insert("autoAlign".into(), Value::Bool(p.auto_align));
    pm.insert("continueThemeAcrossLines".into(), Value::Bool(p.continue_theme_across_lines));
    m.insert("powerline".into(), Value::Object(pm));
    if let Some(u) = &st.updatemessage {
        let mut um = Map::new();
        if let Some(msg) = &u.message {
            um.insert("message".into(), msg.as_ref().map(|s| Value::String(s.clone())).unwrap_or(Value::Null));
        }
        if let Some(rem) = &u.remaining {
            um.insert("remaining".into(), rem.map(jsjson::num).unwrap_or(Value::Null));
        }
        m.insert("updatemessage".into(), Value::Object(um));
    }
    if let Some(i) = &st.installation {
        m.insert("installation".into(), i.clone());
    }
    Value::Object(m)
}

// ------------------------------------------------------------------- loading

pub fn home_dir() -> PathBuf {
    match std::env::var_os("HOME") {
        // os.homedir(): a set HOME wins even when empty
        Some(h) => PathBuf::from(h),
        _ => {
            // os.homedir() falls back to the passwd entry
            unsafe {
                let pw = libc::getpwuid(libc::getuid());
                if !pw.is_null() && !(*pw).pw_dir.is_null() {
                    let s = std::ffi::CStr::from_ptr((*pw).pw_dir).to_string_lossy().into_owned();
                    return PathBuf::from(s);
                }
            }
            PathBuf::from("/")
        }
    }
}

pub fn default_settings_path() -> PathBuf {
    home_dir().join(".config").join("ccstatusline").join("settings.json")
}

/// `path.resolve(p)`: absolute, normalized, no symlink resolution.
pub fn path_resolve(p: &str) -> PathBuf {
    let base = if Path::new(p).is_absolute() {
        PathBuf::new()
    } else {
        std::env::var_os("PWD")
            .map(PathBuf::from)
            .filter(|d| d.is_absolute() && same_dir(d))
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_default()
    };
    let joined = base.join(p);
    let mut out = PathBuf::from("/");
    for comp in joined.components() {
        match comp {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::Normal(c) => out.push(c),
            _ => {}
        }
    }
    out
}

fn same_dir(d: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(d), std::fs::metadata(".")) {
        (Ok(a), Ok(b)) => a.ino() == b.ino() && a.dev() == b.dev(),
        _ => false,
    }
}

pub struct ConfigStore {
    pub path: PathBuf,
    pub load_error: Option<&'static str>,
}

fn now_ms() -> f64 {
    crate::clock::now_ms()
}

impl ConfigStore {
    pub fn new(path: Option<&str>) -> Self {
        ConfigStore { path: path.map(path_resolve).unwrap_or_else(default_settings_path), load_error: None }
    }

    pub fn is_custom_path(&self) -> bool {
        self.path != default_settings_path()
    }

    fn write_json(&self, v: &Value) -> std::io::Result<()> {
        let dir = self.path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("/"));
        std::fs::create_dir_all(&dir)?;
        let (target, temp_dir) = match std::fs::symlink_metadata(&self.path) {
            Ok(md) if md.file_type().is_symlink() => {
                let t = std::fs::canonicalize(&self.path)
                    .or_else(|_| std::fs::read_link(&self.path).map(|l| self.path.parent().unwrap().join(l)))?;
                let td = t.parent().map(Path::to_path_buf).unwrap_or_default();
                (t, td)
            }
            Ok(_) => (self.path.clone(), dir.clone()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (self.path.clone(), dir.clone()),
            Err(e) => return Err(e),
        };
        let base = target.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let tmp = temp_dir.join(format!("{base}.{}.{}.tmp", std::process::id(), now_ms() as u64));
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mode = std::fs::metadata(&target).map(|m| m.permissions().mode() & 0o777).unwrap_or(0o666);
        let res = (|| {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(mode).open(&tmp)?;
            f.write_all(jsjson::stringify_pretty(v).as_bytes())?;
            drop(f);
            std::fs::rename(&tmp, &target)
        })();
        if res.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        res
    }

    /// Upstream `loadSettings`.
    pub fn load(&mut self) -> Settings {
        self.load_error = None;
        if !self.path.exists() {
            let d = Settings::default();
            match self.write_json(&settings_json(&d)) {
                Ok(()) => eprintln!("Default settings written to {}", self.path.display()),
                Err(e) => eprintln!("Failed to write default settings: {e}"),
            }
            return d;
        }
        let content = match std::fs::read(&self.path) {
            Ok(c) => String::from_utf8_lossy(&c).into_owned(),
            Err(e) => {
                eprintln!("Error loading settings, using defaults: {e}");
                self.load_error = Some("settings.json could not be read");
                return Settings::default();
            }
        };
        let mut raw: Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(_) => {
                eprintln!("Failed to parse settings.json, using defaults (file left unchanged)");
                self.load_error = Some("settings.json is not valid JSON");
                return Settings::default();
            }
        };
        let has_version = raw.as_object().is_some_and(|o| o.contains_key("version"));
        let mut migrated = false;
        if !has_version {
            if let Err(e) = validate_v1(&raw) {
                eprintln!("Invalid v1 settings format, using defaults (file left unchanged): {e}");
                self.load_error = Some("settings.json is not in a valid format");
                return Settings::default();
            }
            raw = crate::migrations::migrate_config(raw, CURRENT_VERSION);
            migrated = true;
        } else if crate::migrations::needs_migration(&raw, CURRENT_VERSION) {
            raw = crate::migrations::migrate_config(raw, CURRENT_VERSION);
            migrated = true;
        }
        let mut st = match parse_settings(&raw) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Failed to parse settings, using defaults (file left unchanged): {e}");
                self.load_error = Some("settings.json is not in a valid format");
                return Settings::default();
            }
        };
        if migrated {
            if let Err(e) = self.write_json(&raw) {
                eprintln!("Error loading settings, using defaults: {e}");
                self.load_error = Some("settings.json could not be read");
                return Settings::default();
            }
        }
        for line in &mut st.lines {
            for item in line {
                if item.kind == "git-pr" {
                    item.kind = "git-review".into();
                }
            }
        }
        st
    }

    /// Upstream `saveSettings` (the file write; widget hook sync is separate).
    pub fn save(&self, st: &Settings) {
        let mut st = st.clone();
        st.version = CURRENT_VERSION;
        if let Err(e) = self.write_json(&settings_json(&st)) {
            eprintln!("{e}");
            return;
        }
        crate::hooks::sync_widget_hooks(&st);
    }
}
