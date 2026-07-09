//! Shared visible-limit/emit-hz clamp contract + runtime start/configure
//! (split out of `lib.rs` for file-size discipline).

use nmp_native_runtime::{NmpApp, DEFAULT_EMIT_HZ, DEFAULT_VISIBLE_LIMIT};

/// Clamp `visible_limit` identically for all UniFFI facades.
#[must_use]
pub fn clamp_visible(visible_limit: u32) -> usize {
    if visible_limit == 0 {
        DEFAULT_VISIBLE_LIMIT
    } else {
        visible_limit.clamp(1, 500) as usize
    }
}

/// Clamp `emit_hz` identically for all UniFFI facades.
#[must_use]
pub fn clamp_emit_hz(emit_hz: u32) -> u32 {
    if emit_hz == 0 {
        DEFAULT_EMIT_HZ
    } else {
        emit_hz.clamp(1, 12)
    }
}

/// Start a runtime through the shared UniFFI clamp contract.
pub fn start_runtime(app: &NmpApp, visible_limit: u32, emit_hz: u32) {
    app.start_runtime(clamp_visible(visible_limit), clamp_emit_hz(emit_hz));
}

/// Reconfigure a runtime through the shared UniFFI clamp contract.
pub fn configure_runtime(app: &NmpApp, visible_limit: u32, emit_hz: u32) {
    app.configure_runtime(clamp_visible(visible_limit), clamp_emit_hz(emit_hz));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_contract_matches_runtime_defaults() {
        assert_eq!(clamp_visible(0), DEFAULT_VISIBLE_LIMIT);
        assert_eq!(clamp_visible(999), 500);
        assert_eq!(clamp_visible(10), 10);

        assert_eq!(clamp_emit_hz(0), DEFAULT_EMIT_HZ);
        assert_eq!(clamp_emit_hz(99), 12);
        assert_eq!(clamp_emit_hz(4), 4);
    }
}
