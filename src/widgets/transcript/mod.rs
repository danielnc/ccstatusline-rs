//! Widget group: transcript — token counts, cache, speed, context, session
//! and skills widgets. Their data comes from `crate::transcript` (computed
//! lazily, only when one of them needs it) and the status JSON.

mod cache;
mod context;
mod session;
mod speed;
mod tokens;

use super::Widget;

pub fn lookup(kind: &str) -> Option<&'static dyn Widget> {
    Some(match kind {
        "tokens-input" => &tokens::INPUT,
        "tokens-output" => &tokens::OUTPUT,
        "tokens-cached" => &tokens::CACHED,
        "tokens-total" => &tokens::TOTAL,
        "cache-hit-rate" => &cache::HIT_RATE,
        "cache-read" => &cache::READ,
        "cache-write" => &cache::WRITE,
        "cache-timer" => &cache::TIMER,
        "input-speed" => &speed::INPUT,
        "output-speed" => &speed::OUTPUT,
        "total-speed" => &speed::TOTAL,
        "context-length" => &context::LENGTH,
        "context-window" => &context::WINDOW,
        "context-percentage" => &context::PERCENTAGE,
        "context-percentage-usable" => &context::PERCENTAGE_USABLE,
        "session-clock" => &session::CLOCK,
        "compaction-counter" => &session::COMPACTION,
        "thinking-effort" => &session::THINKING,
        "session-name" => &session::NAME,
        "skills" => &session::SKILLS,
        _ => return None,
    })
}
