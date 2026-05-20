//! Per-relay NIP-77 capability negotiation.
//!
//! A relay either supports the `NEG-OPEN` verb or it doesn't.  We discover
//! support by probing once on first connect; the result is cached so future
//! reconciliations skip the probe.  The cache is owned by the M4 substrate;
//! persistence is layered on top by [`crate::capability_domain::CapabilityDomain`].
//!
//! ## Probe state machine
//!
//! ```text
//!  ┌──────────┐  start_probe        ┌───────────┐
//!  │  Unknown │ ───────────────────▶│  Probing  │
//!  └──────────┘                     └──────┬────┘
//!                                          │
//!                          ┌── NEG-MSG ────┤── NEG-ERR ──┐
//!                          ▼                              ▼
//!                  ┌──────────────┐               ┌──────────────┐
//!                  │ Supported    │               │ Unsupported  │
//!                  └──────────────┘               └──────────────┘
//! ```
//!
//! `Supported` and `Unsupported` are terminal in normal operation.  A relay
//! that downgrades or revokes support requires an explicit reset (e.g. a
//! manual `RunSync` invocation) — same policy as `nostr-sdk`'s capability
//! tracking.

use std::collections::HashMap;
use std::sync::Mutex;

/// What we believe about a relay's NIP-77 support.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RelayCapabilities {
    pub supports_nip77: bool,
}

/// Probe lifecycle state for a single relay.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProbeState {
    Unknown,
    Probing,
    Supported,
    Unsupported,
}

/// What a probe transition produces — used by callers to drive next steps.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProbeOutcome {
    /// Probe was already complete; no further work.
    AlreadySettled(RelayCapabilities),
    /// Probe transitioned to a terminal state on this call.
    Settled(RelayCapabilities),
    /// Probe is in flight; caller must wait for the next relay frame.
    Pending,
}

/// In-memory cache of per-relay capabilities.
///
/// Implementations are free to back the cache with disk (see
/// [`crate::capability_domain::CapabilityDomain`]) — the trait is the
/// minimal substrate the M4 reconciler depends on.
pub trait CapabilityCache: Send + Sync {
    fn get(&self, relay_url: &str) -> Option<RelayCapabilities>;
    fn set(&self, relay_url: &str, caps: RelayCapabilities);
    fn state(&self, relay_url: &str) -> ProbeState;
    fn set_state(&self, relay_url: &str, state: ProbeState);
}

/// Lock-protected `HashMap` implementation of [`CapabilityCache`].  Adequate
/// for app-lifetime usage; persistence is layered on top by the domain
/// module.
pub struct InMemoryCapabilityCache {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    caps: HashMap<String, RelayCapabilities>,
    state: HashMap<String, ProbeState>,
}

impl InMemoryCapabilityCache {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
        }
    }
}

impl Default for InMemoryCapabilityCache {
    fn default() -> Self {
        Self::new()
    }
}

impl CapabilityCache for InMemoryCapabilityCache {
    fn get(&self, relay_url: &str) -> Option<RelayCapabilities> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .caps
            .get(relay_url)
            .copied()
    }

    fn set(&self, relay_url: &str, caps: RelayCapabilities) {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.caps.insert(relay_url.to_string(), caps);
        guard.state.insert(
            relay_url.to_string(),
            if caps.supports_nip77 {
                ProbeState::Supported
            } else {
                ProbeState::Unsupported
            },
        );
    }

    fn state(&self, relay_url: &str) -> ProbeState {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .state
            .get(relay_url)
            .copied()
            .unwrap_or(ProbeState::Unknown)
    }

    fn set_state(&self, relay_url: &str, state: ProbeState) {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .state
            .insert(relay_url.to_string(), state);
    }
}

/// Drives the probe state machine for one relay.
pub struct CapabilityProbe<'a> {
    relay_url: String,
    cache: &'a dyn CapabilityCache,
}

impl<'a> CapabilityProbe<'a> {
    pub fn new(relay_url: impl Into<String>, cache: &'a dyn CapabilityCache) -> Self {
        Self {
            relay_url: relay_url.into(),
            cache,
        }
    }

    /// Begin probing if not already settled.  Callers should issue a
    /// `NEG-OPEN` frame after this call returns [`ProbeOutcome::Pending`].
    pub fn begin(&self) -> ProbeOutcome {
        match self.cache.state(&self.relay_url) {
            ProbeState::Supported => ProbeOutcome::AlreadySettled(RelayCapabilities {
                supports_nip77: true,
            }),
            ProbeState::Unsupported => ProbeOutcome::AlreadySettled(RelayCapabilities {
                supports_nip77: false,
            }),
            ProbeState::Probing | ProbeState::Unknown => {
                self.cache.set_state(&self.relay_url, ProbeState::Probing);
                ProbeOutcome::Pending
            }
        }
    }

    /// Settle the probe given the relay's first response frame.  Pass
    /// `Some(true)` for `supported` after observing a `NEG-MSG`, `Some(false)`
    /// after a `NEG-ERR` with an "unsupported"/"command not recognised"-style
    /// reason, or `None` to leave the probe pending (e.g. the frame was for a
    /// different sub-id).
    pub fn settle(&self, supported: Option<bool>) -> ProbeOutcome {
        let Some(value) = supported else {
            return ProbeOutcome::Pending;
        };
        let caps = RelayCapabilities {
            supports_nip77: value,
        };
        self.cache.set(&self.relay_url, caps);
        ProbeOutcome::Settled(caps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_promotes_unknown_to_supported() {
        let cache = InMemoryCapabilityCache::new();
        let probe = CapabilityProbe::new("wss://r/", &cache);
        assert!(matches!(probe.begin(), ProbeOutcome::Pending));
        assert_eq!(cache.state("wss://r/"), ProbeState::Probing);
        let settled = probe.settle(Some(true));
        assert_eq!(
            settled,
            ProbeOutcome::Settled(RelayCapabilities {
                supports_nip77: true
            })
        );
        assert_eq!(cache.state("wss://r/"), ProbeState::Supported);
    }

    #[test]
    fn probe_promotes_unknown_to_unsupported_on_err() {
        let cache = InMemoryCapabilityCache::new();
        let probe = CapabilityProbe::new("wss://r/", &cache);
        let _ = probe.begin();
        let settled = probe.settle(Some(false));
        assert_eq!(
            settled,
            ProbeOutcome::Settled(RelayCapabilities {
                supports_nip77: false
            })
        );
        assert_eq!(cache.state("wss://r/"), ProbeState::Unsupported);
    }

    #[test]
    fn second_probe_is_idempotent_after_settle() {
        let cache = InMemoryCapabilityCache::new();
        let probe = CapabilityProbe::new("wss://r/", &cache);
        let _ = probe.begin();
        let _ = probe.settle(Some(true));
        let again = probe.begin();
        assert_eq!(
            again,
            ProbeOutcome::AlreadySettled(RelayCapabilities {
                supports_nip77: true
            })
        );
    }

    #[test]
    fn settle_with_none_keeps_probe_pending() {
        let cache = InMemoryCapabilityCache::new();
        let probe = CapabilityProbe::new("wss://r/", &cache);
        let _ = probe.begin();
        assert!(matches!(probe.settle(None), ProbeOutcome::Pending));
        assert_eq!(cache.state("wss://r/"), ProbeState::Probing);
    }
}
