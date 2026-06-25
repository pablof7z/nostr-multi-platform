//! Pure `jittered_backoff` unit tests (T116c / G12), split out of
//! `relay_worker::tests` for file-size ownership. No socket or worker
//! setup — these exercise the deterministic per-URL backoff jitter
//! function directly.

use std::time::Duration;

use crate::relay_protocol::jittered_backoff;

/// Same URL → same jitter offset (deterministic per URL).
#[test]
fn t116c_jitter_is_deterministic_per_url() {
    let base = Duration::from_secs(3);
    let url = "wss://relay.example.com";
    let a = jittered_backoff(base, url);
    let b = jittered_backoff(base, url);
    assert_eq!(
        a, b,
        "jittered_backoff must return the same value for the same URL"
    );
}

/// Two different URLs → different jitter offsets (spread across relays).
#[test]
fn t116c_jitter_differs_across_urls() {
    let base = Duration::from_secs(3);
    let url_a = "wss://relay-a.example.com";
    let url_b = "wss://relay-b.example.com";
    let a = jittered_backoff(base, url_a);
    let b = jittered_backoff(base, url_b);
    assert_ne!(
        a, b,
        "jittered_backoff should produce different offsets for different URLs"
    );
}

/// Jitter is bounded: result is in [base, base + 5s] for any URL.
#[test]
fn t116c_jitter_bounded_within_5s() {
    let base = Duration::from_secs(3);
    let urls = [
        "wss://relay.damus.io",
        "wss://purplepag.es",
        "wss://nos.lol",
        "wss://relay.snort.social",
        "wss://relay.primal.net",
        "wss://nostr.wine",
        "wss://relay.current.fyi",
        "wss://relay.nostrbuild.io",
        "wss://nostr.mom",
        "wss://relay.nostr.bg",
    ];
    let max_jitter = Duration::from_millis(5000);
    for url in urls {
        let result = jittered_backoff(base, url);
        assert!(
            result >= base,
            "jitter must not reduce backoff below base for {url}: got {result:?}"
        );
        assert!(
            result <= base + max_jitter,
            "jitter must not exceed base + 5s for {url}: got {result:?}"
        );
    }
}
