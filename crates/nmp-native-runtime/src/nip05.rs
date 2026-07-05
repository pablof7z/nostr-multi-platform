//! NIP-05 reverse-lookup poll state (chirp#155) — mirrors `crate::ad`'s
//! per-URL `AdUrlState` map, but keyed by the dispatched `name@domain`
//! identifier rather than the resolved thing itself, because a NIP-05
//! identifier resolves to a PUBKEY the caller doesn't know yet (an AD
//! candidate resolves the same URL it was dispatched with).
//!
//! Without this, a search UI's "Looking up …" affordance for a NIP-05
//! identifier had no poll seam at all: `dispatch_search_intent` reported
//! `Nip05 { identifier }` and the Rust-side lookup genuinely ran (bounded by
//! `nmp-wellknown-http`'s 10s timeout) and landed its result via
//! `nmp_nip05::ResolveNip05Command`'s generic side effects
//! (`ActorCommand::Refs` keyed by pubkey / `ActorCommand::ShowErrorToken`, a
//! global diagnostic) — but nothing let the caller ask "did MY lookup finish,
//! and how" keyed by the identifier it showed a spinner for. The affordance
//! spun forever even once the lookup had genuinely terminated.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use nmp_core::actor::ActorCommand;
use nmp_nip05::Nip05LookupObserver;

use crate::NmpApp;

/// Poll-friendly outcome of one dispatched NIP-05 reverse lookup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Nip05LookupState {
    /// Never dispatched (or the app's `nip05` feature is off).
    NotAttempted,
    /// Dispatched; the `.well-known` fetch is in flight.
    Resolving,
    /// Resolved to `pubkey` (64-hex).
    Resolved { pubkey: String },
    /// Terminally failed. `reason` is human-readable (never the raw
    /// response body).
    Failed { reason: String },
}

/// identifier → [`Nip05LookupState`]. `Arc<Mutex<..>>` so the
/// `Nip05LookupObserver` callback (invoked off the actor thread, from the
/// command's spawned worker) can update it.
pub(crate) type Nip05StateMap = Arc<Mutex<HashMap<String, Nip05LookupState>>>;

/// Binds one dispatched identifier to the shared state map. `Nip05LookupObserver`
/// carries no identifier parameter (see its doc comment — one observer per
/// dispatch), so this is the closure-equivalent that records the outcome under
/// the right key.
struct Nip05StateReporter {
    identifier: String,
    states: Nip05StateMap,
}

impl Nip05LookupObserver for Nip05StateReporter {
    fn on_resolved(&self, pubkey: &str) {
        if let Ok(mut states) = self.states.lock() {
            states.insert(
                self.identifier.clone(),
                Nip05LookupState::Resolved {
                    pubkey: pubkey.to_string(),
                },
            );
        }
    }

    fn on_failed(&self, reason: &str) {
        if let Ok(mut states) = self.states.lock() {
            states.insert(
                self.identifier.clone(),
                Nip05LookupState::Failed {
                    reason: reason.to_string(),
                },
            );
        }
    }
}

impl NmpApp {
    /// Current [`Nip05LookupState`] for `identifier` — the read-door query a
    /// search UI polls after `dispatch_search_intent` reports `Nip05`.
    /// `NotAttempted` when the identifier has never been dispatched.
    #[must_use]
    pub fn nip05_lookup_state(&self, identifier: &str) -> Nip05LookupState {
        self.nip05_states
            .lock()
            .ok()
            .and_then(|m| m.get(identifier).cloned())
            .unwrap_or(Nip05LookupState::NotAttempted)
    }

    /// Dispatch the reverse lookup for `identifier` (`name`/`domain` already
    /// shape-validated by the classifier — see `crate::intent`). Records
    /// `Resolving` immediately so `nip05_lookup_state` never answers
    /// `NotAttempted` for an identifier that was just dispatched: the search
    /// view can swap to its "Looking up …" affordance the instant this
    /// returns, before any frame tick.
    pub(crate) fn dispatch_nip05_lookup(&self, identifier: &str, name: String, domain: String) {
        if let Ok(mut states) = self.nip05_states.lock() {
            states.insert(identifier.to_string(), Nip05LookupState::Resolving);
        }
        let reporter = Nip05StateReporter {
            identifier: identifier.to_string(),
            states: Arc::clone(&self.nip05_states),
        };
        self.send_cmd(ActorCommand::Protocol(Box::new(
            nmp_nip05::ResolveNip05Command {
                name,
                domain,
                correlation_id: None,
                observer: Some(Arc::new(reporter)),
            },
        )));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_identifier_is_not_attempted() {
        let app = crate::new_app();
        assert_eq!(
            app.nip05_lookup_state("_@example.com"),
            Nip05LookupState::NotAttempted
        );
    }

    #[test]
    fn dispatch_records_resolving_immediately() {
        // Never a real network round-trip in this test — asserting only that
        // the state map leaves `NotAttempted` the instant dispatch returns,
        // exactly as `pollAdState`'s AD-candidate counterpart already relies
        // on for `ad_url_state`.
        let app = crate::new_app();
        app.dispatch_nip05_lookup(
            "_@nonexistent.invalid",
            "_".to_string(),
            "nonexistent.invalid".to_string(),
        );
        assert_eq!(
            app.nip05_lookup_state("_@nonexistent.invalid"),
            Nip05LookupState::Resolving
        );
    }

    // The "eventually terminates, never hangs" guarantee (the actual #155
    // regression) is proven at the `nmp-nip05` crate level via
    // `Nip05LookupObserver` + a blocking `recv_timeout` (never a sleep-poll
    // loop, per D8) — see
    // `nmp_nip05::tests::failed_lookup_notifies_observer_within_a_bounded_time`.
    // This module only needs to prove the map is wired to that observer.
}
