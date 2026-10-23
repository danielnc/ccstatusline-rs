//! Status JSON from Claude Code (upstream `src/types/StatusJSON.ts`), validated
//! with the same rules as the zod schema.

use serde_json::{Map, Value};

type R<T> = Result<T, String>;

#[derive(Clone, Debug, Default)]
pub enum Model {
    #[default]
    None,
    Str(String),
    Obj {
        id: Option<String>,
        display_name: Option<String>,
    },
}

#[derive(Clone, Debug, Default)]
pub struct CurrentUsageObj {
    pub input_tokens: Option<f64>,
    pub output_tokens: Option<f64>,
    pub cache_creation_input_tokens: Option<f64>,
    pub cache_read_input_tokens: Option<f64>,
}

#[derive(Clone, Debug)]
pub enum CurrentUsage {
    Num(f64),
    Obj(CurrentUsageObj),
}

#[derive(Clone, Debug, Default)]
pub struct ContextWindow {
    pub context_window_size: Option<f64>,
    pub total_input_tokens: Option<f64>,
    pub total_output_tokens: Option<f64>,
    pub current_usage: Option<CurrentUsage>,
    pub used_percentage: Option<f64>,
    pub remaining_percentage: Option<f64>,
}

#[derive(Clone, Debug, Default)]
pub struct Cost {
    pub total_cost_usd: Option<f64>,
    pub total_duration_ms: Option<f64>,
    pub total_api_duration_ms: Option<f64>,
    pub total_lines_added: Option<f64>,
    pub total_lines_removed: Option<f64>,
}

#[derive(Clone, Debug, Default)]
pub struct Worktree {
    pub name: Option<String>,
    pub path: Option<String>,
    pub branch: Option<String>,
    pub original_cwd: Option<String>,
    pub original_branch: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct RateLimitPeriod {
    pub used_percentage: Option<f64>,
    /// Unix epoch seconds
    pub resets_at: Option<f64>,
}

#[derive(Clone, Debug, Default)]
pub struct RateLimits {
    pub five_hour: Option<RateLimitPeriod>,
    pub seven_day: Option<RateLimitPeriod>,
    pub seven_day_sonnet: Option<RateLimitPeriod>,
    pub seven_day_opus: Option<RateLimitPeriod>,
}

#[derive(Clone, Debug, Default)]
pub struct Workspace {
    pub current_dir: Option<String>,
    pub project_dir: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct StatusJson {
    pub hook_event_name: Option<String>,
    pub session_id: Option<String>,
    pub transcript_path: Option<String>,
    pub cwd: Option<String>,
    pub model: Model,
    pub workspace: Option<Workspace>,
    pub version: Option<String>,
    pub output_style_name: Option<String>,
    pub output_style_present: bool,
    /// `effort` present (object) — and its level (None when null/missing)
    pub effort: Option<Option<String>>,
    /// whether `effort` object has a `level` key at all
    pub effort_has_level: bool,
    pub cost: Option<Cost>,
    pub context_window: Option<ContextWindow>,
    pub vim_mode: Option<String>,
    pub vim_present: bool,
    pub worktree: Option<Worktree>,
    pub rate_limits: Option<RateLimits>,
    /// The whole payload, for fields outside the schema (looseObject).
    pub raw: Value,
}

fn obj<'a>(v: &'a Value, what: &str) -> R<&'a Map<String, Value>> {
    v.as_object().ok_or_else(|| format!("{what}: expected object"))
}

fn ostr(o: &Map<String, Value>, k: &str) -> R<Option<String>> {
    match o.get(k) {
        None => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(format!("{k}: expected string")),
    }
}

fn ostr_nullable(o: &Map<String, Value>, k: &str) -> R<Option<String>> {
    match o.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(format!("{k}: expected string")),
    }
}

/// zod `CoercedNumberSchema`: numbers, or strings that `Number()` turns into a
/// finite number.
pub fn coerce_number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64().filter(|f| f.is_finite()),
        Value::String(s) => {
            let t = crate::jsnum::js_trim(s);
            if t.is_empty() {
                return None;
            }
            crate::jsnum::number_from_str(t).filter(|f| f.is_finite())
        }
        _ => None,
    }
}

fn onum(o: &Map<String, Value>, k: &str) -> R<Option<f64>> {
    match o.get(k) {
        None => Ok(None),
        Some(v) => coerce_number(v).map(Some).ok_or_else(|| format!("{k}: expected number")),
    }
}

fn onum_nullable(o: &Map<String, Value>, k: &str) -> R<Option<f64>> {
    match o.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => coerce_number(v).map(Some).ok_or_else(|| format!("{k}: expected number")),
    }
}

/// Some(None) means present-but-null; None means absent.
fn nested<'a>(o: &'a Map<String, Value>, k: &str, nullable: bool) -> R<Option<&'a Map<String, Value>>> {
    match o.get(k) {
        None => Ok(None),
        Some(Value::Null) if nullable => Ok(None),
        Some(v) => obj(v, k).map(Some),
    }
}

fn rate_period(o: &Map<String, Value>, k: &str, nullable: bool) -> R<Option<RateLimitPeriod>> {
    Ok(match nested(o, k, nullable)? {
        None => None,
        Some(p) => Some(RateLimitPeriod {
            used_percentage: onum_nullable(p, "used_percentage")?,
            resets_at: onum_nullable(p, "resets_at")?,
        }),
    })
}

pub fn parse_status(v: Value) -> R<StatusJson> {
    let o = obj(&v, "status")?;
    let model = match o.get("model") {
        None => Model::None,
        Some(Value::String(s)) => Model::Str(s.clone()),
        Some(Value::Object(m)) => Model::Obj { id: ostr(m, "id")?, display_name: ostr(m, "display_name")? },
        Some(_) => return Err("model: invalid".into()),
    };
    let workspace = match nested(o, "workspace", true)? {
        None => None,
        Some(w) => Some(Workspace { current_dir: ostr(w, "current_dir")?, project_dir: ostr(w, "project_dir")? }),
    };
    let (output_style_name, output_style_present) = match nested(o, "output_style", true)? {
        None => (None, false),
        Some(s) => (ostr(s, "name")?, true),
    };
    let (effort, effort_has_level) = match nested(o, "effort", true)? {
        None => (None, false),
        Some(e) => (Some(ostr_nullable(e, "level")?), e.contains_key("level")),
    };
    let cost = match nested(o, "cost", true)? {
        None => None,
        Some(c) => Some(Cost {
            total_cost_usd: onum(c, "total_cost_usd")?,
            total_duration_ms: onum(c, "total_duration_ms")?,
            total_api_duration_ms: onum(c, "total_api_duration_ms")?,
            total_lines_added: onum(c, "total_lines_added")?,
            total_lines_removed: onum(c, "total_lines_removed")?,
        }),
    };
    let context_window = match nested(o, "context_window", true)? {
        None => None,
        Some(c) => Some(ContextWindow {
            context_window_size: onum_nullable(c, "context_window_size")?,
            total_input_tokens: onum_nullable(c, "total_input_tokens")?,
            total_output_tokens: onum_nullable(c, "total_output_tokens")?,
            current_usage: match c.get("current_usage") {
                None | Some(Value::Null) => None,
                Some(Value::Object(u)) => Some(CurrentUsage::Obj(CurrentUsageObj {
                    input_tokens: onum(u, "input_tokens")?,
                    output_tokens: onum(u, "output_tokens")?,
                    cache_creation_input_tokens: onum(u, "cache_creation_input_tokens")?,
                    cache_read_input_tokens: onum(u, "cache_read_input_tokens")?,
                })),
                Some(v) => Some(CurrentUsage::Num(coerce_number(v).ok_or("current_usage: invalid")?)),
            },
            used_percentage: onum_nullable(c, "used_percentage")?,
            remaining_percentage: onum_nullable(c, "remaining_percentage")?,
        }),
    };
    let (vim_mode, vim_present) = match nested(o, "vim", true)? {
        None => (None, false),
        Some(m) => (ostr(m, "mode")?, true),
    };
    let worktree = match nested(o, "worktree", true)? {
        None => None,
        Some(w) => Some(Worktree {
            name: ostr(w, "name")?,
            path: ostr(w, "path")?,
            branch: ostr(w, "branch")?,
            original_cwd: ostr(w, "original_cwd")?,
            original_branch: ostr(w, "original_branch")?,
        }),
    };
    let rate_limits = match nested(o, "rate_limits", true)? {
        None => None,
        Some(r) => Some(RateLimits {
            five_hour: rate_period(r, "five_hour", false)?,
            seven_day: rate_period(r, "seven_day", false)?,
            seven_day_sonnet: rate_period(r, "seven_day_sonnet", true)?,
            seven_day_opus: rate_period(r, "seven_day_opus", true)?,
        }),
    };
    let s = StatusJson {
        hook_event_name: ostr(o, "hook_event_name")?,
        session_id: ostr(o, "session_id")?,
        transcript_path: ostr(o, "transcript_path")?,
        cwd: ostr(o, "cwd")?,
        model,
        workspace,
        version: ostr_nullable(o, "version")?,
        output_style_name,
        output_style_present,
        effort,
        effort_has_level,
        cost,
        context_window,
        vim_mode,
        vim_present,
        worktree,
        rate_limits,
        raw: Value::Null,
    };
    Ok(StatusJson { raw: v, ..s })
}

impl StatusJson {
    pub fn model_id(&self) -> Option<&str> {
        match &self.model {
            Model::Obj { id, .. } => id.as_deref(),
            _ => None,
        }
    }
    pub fn model_display_name(&self) -> Option<&str> {
        match &self.model {
            Model::Obj { display_name, .. } => display_name.as_deref(),
            _ => None,
        }
    }
    pub fn current_dir(&self) -> Option<&str> {
        self.workspace.as_ref().and_then(|w| w.current_dir.as_deref())
    }
    /// A loose (non-schema) top-level field.
    pub fn extra(&self, key: &str) -> Option<&Value> {
        self.raw.get(key)
    }
}
