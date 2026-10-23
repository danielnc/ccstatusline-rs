//! Wall clock, overridable for the parity harness with `CCSL_TEST_NOW_MS`.

use std::sync::OnceLock;

static FIXED: OnceLock<Option<f64>> = OnceLock::new();

fn fixed() -> Option<f64> {
    *FIXED.get_or_init(|| std::env::var("CCSL_TEST_NOW_MS").ok().and_then(|v| v.trim().parse::<f64>().ok()))
}

/// `Date.now()`
pub fn now_ms() -> f64 {
    if let Some(f) = fixed() {
        return f;
    }
    let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    d.as_millis() as f64
}

/// True when the harness asked for no terminal-width probing.
pub fn no_probe() -> bool {
    std::env::var("CCSL_TEST_NO_PROBE").is_ok_and(|v| v == "1")
}
