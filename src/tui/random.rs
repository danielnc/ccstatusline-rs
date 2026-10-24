//! `Math.random()` for the editor (new widgets' GUIDs, the powerline background picked for a new widget). Under the
//! parity harness `CCSL_TEST_RANDOM_SEED` makes it the same mulberry32 sequence the preload gives upstream.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::OnceLock;

static STATE: AtomicU32 = AtomicU32::new(0);
static SEEDED: OnceLock<()> = OnceLock::new();

fn seed() {
    SEEDED.get_or_init(|| {
        let s = std::env::var("CCSL_TEST_RANDOM_SEED").ok().and_then(|v| v.trim().parse::<u32>().ok()).unwrap_or_else(
            || {
                let t = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0);
                (t as u32) ^ ((t >> 32) as u32) ^ std::process::id().rotate_left(16)
            },
        );
        STATE.store(s, Ordering::Relaxed);
    });
}

/// One mulberry32 step from a state already advanced by the increment.
fn mulberry32(state: u32) -> f64 {
    let mut t = state;
    t = (t ^ (t >> 15)).wrapping_mul(t | 1);
    t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
    ((t ^ (t >> 14)) as f64) / 4_294_967_296.0
}

/// mulberry32, as `parity/preload.mjs` installs it: a float in [0, 1).
pub fn math_random() -> f64 {
    seed();
    mulberry32(STATE.fetch_add(0x6d2b_79f5, Ordering::Relaxed).wrapping_add(0x6d2b_79f5))
}

/// Upstream `generateGuid()`.
pub fn generate_guid() -> String {
    "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx"
        .chars()
        .map(|c| match c {
            'x' | 'y' => {
                let r = (math_random() * 16.0) as u32;
                let v = if c == 'x' { r } else { (r & 0x3) | 0x8 };
                std::char::from_digit(v, 16).unwrap()
            }
            other => other,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn matches_the_preloads_sequence() {
        // From the preload's Math.random with seed 1.
        let mut state: u32 = 1;
        let mut next = || {
            state = state.wrapping_add(0x6d2b_79f5);
            super::mulberry32(state)
        };
        assert_eq!(next(), 0.6270739405881613);
        assert_eq!(next(), 0.002735721180215478);
        assert_eq!(next(), 0.5274470399599522);
    }
}
