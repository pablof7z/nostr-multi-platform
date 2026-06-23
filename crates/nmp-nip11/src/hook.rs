//! The relay-connected hook: spawns a NIP-11 fetch when a relay connects and
//! posts the parsed document back into the actor loop.
//!
//! Installed on the [`nmp_core::substrate::RelayConnectedHookSlot`] (via
//! `NmpApp::add_relay_connected_hook`). The actor fans it on every
//! `PoolEvent::Opened`. The hook owns a per-URL TTL gate so a relay that
//! reconnects rapidly is not refetched on every connect — only after the
//! document goes stale.
//!
//! D8: `on_relay_connected` runs on the actor thread and only ever *spawns* a
//! worker; the blocking `ureq` GET happens on the new thread, which posts the
//! result back through the cloned [`CommandSender`] (the ADR-0050 §D3a waking
//! inbox handle) as [`ActorCommand::SetRelayInfo`].

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use nmp_core::substrate::RelayConnectedHook;
use nmp_core::{ActorCommand, CommandSender, RelayCommand};

use crate::fetch::fetch_relay_info_blocking;

/// Default freshness window for a fetched NIP-11 document. Matches the 5-minute
/// TTL the `nmp_core::util::TimeCached` doc-comment cites for relay info.
pub const NIP11_TTL: Duration = Duration::from_secs(300);

/// A [`RelayConnectedHook`] that fetches each relay's NIP-11 document on
/// connect, subject to a per-URL TTL, and posts it back via
/// [`ActorCommand::SetRelayInfo`].
#[derive(Debug)]
pub struct Nip11FetchHook {
    ttl: Duration,
    /// Per-URL last-fetch-START instant. Recorded the moment a worker is
    /// spawned (not when it finishes) so a burst of reconnects within the TTL
    /// spawns exactly one fetch. A failed fetch still occupies the slot until
    /// the TTL elapses, bounding retry pressure against a dead relay.
    last_fetch: Mutex<HashMap<String, Instant>>,
}

impl Default for Nip11FetchHook {
    fn default() -> Self {
        Self::with_ttl(NIP11_TTL)
    }
}

impl Nip11FetchHook {
    /// Construct with the default [`NIP11_TTL`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct with a custom TTL (tests pass a tiny value).
    #[must_use]
    pub fn with_ttl(ttl: Duration) -> Self {
        Self {
            ttl,
            last_fetch: Mutex::new(HashMap::new()),
        }
    }

    /// Return `true` and record `now` as the fetch-start instant when a fetch
    /// for `relay_url` should proceed (no fresh prior fetch). Returns `false`
    /// when a fetch within the TTL already happened — the caller skips. Pure
    /// w.r.t. the network; unit-testable with an injected `now`.
    fn should_fetch(&self, relay_url: &str, now: Instant) -> bool {
        let Ok(mut map) = self.last_fetch.lock() else {
            // A poisoned lock means a prior fetch-decision panicked; fail
            // closed (skip) rather than hammer the relay.
            return false;
        };
        match map.get(relay_url) {
            Some(prev) if now.saturating_duration_since(*prev) < self.ttl => false,
            _ => {
                map.insert(relay_url.to_string(), now);
                true
            }
        }
    }
}

impl RelayConnectedHook for Nip11FetchHook {
    fn on_relay_connected(
        &self,
        relay_url: &str,
        _is_reconnect: bool,
        command_sender: CommandSender,
    ) {
        if !self.should_fetch(relay_url, Instant::now()) {
            return;
        }
        let url = relay_url.to_string();
        // D8: spawn — never block the actor thread. The worker posts the
        // result back as `SetRelayInfo`; a failed fetch is silently dropped
        // (the relay simply has no document, and the TTL slot prevents a hot
        // retry loop).
        std::thread::spawn(move || {
            if let Ok(doc) = fetch_relay_info_blocking(&url) {
                if let Some(doc_json) = doc.to_json() {
                    let _ = command_sender.send(ActorCommand::Relay(RelayCommand::SetRelayInfo {
                        relay_url: url,
                        doc_json,
                    }));
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_connect_fetches_then_ttl_suppresses() {
        let hook = Nip11FetchHook::with_ttl(Duration::from_secs(300));
        let t0 = Instant::now();
        assert!(hook.should_fetch("wss://r", t0), "first connect fetches");
        // Within TTL: suppressed.
        assert!(!hook.should_fetch("wss://r", t0 + Duration::from_secs(60)));
        assert!(!hook.should_fetch("wss://r", t0 + Duration::from_secs(299)));
        // Past TTL: fetches again.
        assert!(hook.should_fetch("wss://r", t0 + Duration::from_secs(300)));
    }

    #[test]
    fn distinct_urls_are_tracked_independently() {
        let hook = Nip11FetchHook::with_ttl(Duration::from_secs(300));
        let t0 = Instant::now();
        assert!(hook.should_fetch("wss://a", t0));
        assert!(hook.should_fetch("wss://b", t0));
        assert!(!hook.should_fetch("wss://a", t0 + Duration::from_secs(1)));
    }
}
