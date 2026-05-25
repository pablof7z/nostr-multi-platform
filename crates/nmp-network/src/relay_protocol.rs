//! Wire-transport-agnostic relay protocol primitives.
//!
//! V-01 Stage 3 — splits the parts of the relay-worker FSM that are pure
//! (constants + helpers) out of the native-only [`crate::relay_worker`] module
//! so a non-native transport (today: `web_sys::WebSocket` in `nmp-wasm`) can
//! reuse the same backoff, keepalive thresholds, and error classification
//! without depending on `tungstenite`/`mio`/`rustls`.
//!
//! ## What lives here
//!
//! - [`RELAY_RECONNECT_DELAY_INITIAL`] / [`RELAY_RECONNECT_DELAY_MAX`] — the
//!   exponential-backoff bounds for mid-session reconnects.
//! - [`KEEPALIVE_IDLE_THRESHOLD`] / [`KEEPALIVE_PONG_TIMEOUT`] — the production
//!   knobs the native worker passes to [`crate::keepalive::KeepaliveState`].
//! - [`jittered_backoff`] — per-URL deterministic jitter that spreads
//!   simultaneous reconnects across a `[0, 5s]` window (T116c / G12).
//! - [`is_permanent_error`] — HTTP-level denial classifier (401/403/Forbidden).
//!
//! ## What does NOT live here
//!
//! The socket I/O loop itself (`mio` readiness, `tungstenite::write` /
//! `tungstenite::read`, thread spawning) stays in [`crate::relay_worker`]. The
//! `web_sys::WebSocket` driver in `nmp-wasm` is callback-driven and cannot
//! share that loop — only the data-plane primitives above. The native worker
//! re-exports these constants directly.

use std::time::Duration;

/// Initial mid-session reconnect delay. Doubled on each consecutive failure
/// up to [`RELAY_RECONNECT_DELAY_MAX`]; reset to this value on a successful
/// connect.
pub const RELAY_RECONNECT_DELAY_INITIAL: Duration = Duration::from_secs(3);

/// Upper bound on the exponential reconnect-delay growth.
pub const RELAY_RECONNECT_DELAY_MAX: Duration = Duration::from_secs(300);

/// T120b / G4 — emit a Ping after this much inbound silence.
pub const KEEPALIVE_IDLE_THRESHOLD: Duration = Duration::from_secs(30);

/// T120b / G4 — declare the socket dead if no inbound frame arrives within
/// this window after a Ping is emitted.
pub const KEEPALIVE_PONG_TIMEOUT: Duration = Duration::from_secs(30);

/// T116c / G12 — per-URL deterministic jitter to prevent thundering-herd
/// reconnects when many relays fail simultaneously (e.g. network partition
/// recovery). Uses a hash of the URL bytes to produce a spread that is:
///   - deterministic per URL (same URL always gets the same jitter offset),
///   - spread across all active relays (different URLs → different offsets),
///   - bounded to `[0, 5s]` so worst-case individual delay is `base + 5s`.
///
/// No shared state needed: each worker computes its own jitter independently.
#[must_use]
pub fn jittered_backoff(base: Duration, url: &str) -> Duration {
    let hash = url
        .bytes()
        .fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(u64::from(b)));
    let jitter_ms = hash % 5000; // 0–4999 ms spread
    base + Duration::from_millis(jitter_ms)
}

/// HTTP-level denial: the relay explicitly rejected the connection.
/// 401 and 403 are both permanent until the user changes credentials/policy.
///
/// Used both at connect time (HTTP handshake failure) and mid-session (a
/// relay tearing the socket down with a 401/403 after NIP-42 auth failure).
#[must_use]
pub fn is_permanent_error(error: &str) -> bool {
    error.contains("403") || error.contains("401") || error.contains("Forbidden")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jittered_backoff_is_deterministic_per_url() {
        let base = Duration::from_secs(3);
        let a = jittered_backoff(base, "wss://relay.example");
        let b = jittered_backoff(base, "wss://relay.example");
        assert_eq!(a, b, "same url must yield identical jitter");
    }

    #[test]
    fn jittered_backoff_spreads_across_distinct_urls() {
        let base = Duration::from_secs(3);
        let urls = [
            "wss://a.example",
            "wss://b.example",
            "wss://c.example",
            "wss://d.example",
        ];
        let offsets: Vec<u128> = urls
            .iter()
            .map(|u| (jittered_backoff(base, u) - base).as_millis())
            .collect();
        let distinct = offsets.iter().collect::<std::collections::HashSet<_>>();
        assert!(
            distinct.len() >= 2,
            "distinct urls must yield ≥2 distinct jitter offsets, got {offsets:?}",
        );
    }

    #[test]
    fn jittered_backoff_bounded_by_five_seconds() {
        let base = Duration::from_secs(3);
        for url in [
            "wss://r.example",
            "wss://very-long-relay-url.example/path",
            "",
        ] {
            let delay = jittered_backoff(base, url);
            assert!(
                delay >= base && delay <= base + Duration::from_millis(4999),
                "jittered_backoff({url:?}) = {delay:?} out of [base, base+5s)",
            );
        }
    }

    #[test]
    fn is_permanent_error_matches_documented_codes() {
        assert!(is_permanent_error("401 Unauthorized"));
        assert!(is_permanent_error("403 Forbidden"));
        assert!(is_permanent_error("Forbidden — bring NIP-42"));
        assert!(!is_permanent_error("502 Bad Gateway"));
        assert!(!is_permanent_error("connection reset by peer"));
    }
}
