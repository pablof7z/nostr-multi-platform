//! App-default search-relay configuration and the `effective_search_relays`
//! read helper consumed by the higher-order NIP-50 search crate.
//!
//! # Design
//!
//! NIP-50 search needs a relay list at runtime. The preference order is:
//!
//! 1. **User-published kind:10007 list** — populated by
//!    [`SearchRelayListProjection`] from the live subscription registered by
//!    [`crate::runtimes::register_search_relay_runtime`]. Authoritative: the
//!    user explicitly chose these relays.
//! 2. **App default** — a small list the APP (not NMP, not this crate)
//!    supplies at build time via [`SearchDefaults`]. If the user has no
//!    kind:10007 list published (new account, unconfigured), the default is
//!    used so searches don't fail-closed.
//!
//! # No operator policy in protocol crates
//!
//! The built-in default relay (`wss://search.nos.lol`) lives here in
//! `nmp-defaults`, which is a composition/operator-policy crate (step 10 of
//! `docs/architecture/crate-boundaries.md`). It is NEVER in `nmp-core` or any
//! NIP protocol crate. Apps that want a different default override it with
//! [`SearchDefaults::with_default_relays`] before calling
//! [`crate::register_defaults`].
//!
//! # Usage
//!
//! ```rust,ignore
//! // 1. Configure at build time (optional — NMP ships a sensible built-in).
//! let search_defaults = SearchDefaults::default(); // uses wss://search.nos.lol
//! // or: SearchDefaults::with_default_relays(vec!["wss://my-search.example".to_string()])
//!
//! // 2. Register the runtime and keep the projection handle.
//! let search_projection = register_search_relay_runtime(&mut app);
//!
//! // 3. At query time, resolve the effective relay list.
//! let relays = effective_search_relays(&search_projection, &search_defaults);
//! ```

use std::sync::Arc;

use nmp_nip51::SearchRelayListProjection;

/// NMP's built-in fallback search relay, used when the app supplies no
/// override and the active account has no published kind:10007 list.
///
/// `wss://search.nos.lol` is the canonical NIP-50 search relay operated by
/// nos.social. Apps that prefer a different relay (or wish to supply none at
/// all) set [`SearchDefaults::default_relays`] before calling
/// `register_defaults`. This constant is intentionally in `nmp-defaults` (the
/// composition/operator-policy crate), not in any NIP protocol crate.
pub const NMP_BUILTIN_SEARCH_RELAY: &str = "wss://search.nos.lol";

/// App-overridable default search-relay configuration.
///
/// An app calls [`SearchDefaults::with_default_relays`] to replace the NMP
/// built-in with its own list. The list is used when the active account has no
/// published kind:10007 relay list (new user, unconfigured account, etc.).
///
/// The default-constructed value uses [`NMP_BUILTIN_SEARCH_RELAY`].
#[derive(Clone, Debug)]
pub struct SearchDefaults {
    /// Relay URLs to use when the user has no kind:10007 list.
    pub default_relays: Vec<String>,
}

impl Default for SearchDefaults {
    fn default() -> Self {
        Self {
            default_relays: vec![NMP_BUILTIN_SEARCH_RELAY.to_string()],
        }
    }
}

impl SearchDefaults {
    /// Construct a [`SearchDefaults`] with an app-supplied relay list.
    ///
    /// Pass canonical `wss://` URLs. The list replaces the NMP built-in
    /// entirely — if you want to extend the built-in, start with
    /// `SearchDefaults::default().default_relays` and append to it.
    #[must_use]
    pub fn with_default_relays(relays: Vec<String>) -> Self {
        Self {
            default_relays: relays,
        }
    }
}

/// Return the effective search relay list for the active account.
///
/// Preference order:
/// 1. User's kind:10007 snapshot (non-empty → authoritative)
/// 2. App default from [`SearchDefaults::default_relays`]
///
/// A higher-order NIP-50 search crate calls this at subscription-open time to
/// determine which relays to open a `{"search": "..."}` REQ on. The caller
/// should re-evaluate this on every snapshot tick (account switch, or the
/// user's kind:10007 arriving for the first time) rather than caching the
/// result indefinitely.
///
/// # Account-switch safety
///
/// [`SearchRelayListProjection::snapshot`] gates on the live `active_pubkey`
/// slot — if the account changed between the last kind:10007 ingest and this
/// call, `snapshot()` returns an empty list, so the default falls back. No
/// stale relay data from a prior account is ever returned.
#[must_use]
pub fn effective_search_relays(
    projection: &Arc<SearchRelayListProjection>,
    defaults: &SearchDefaults,
) -> Vec<String> {
    let user_list = projection.snapshot().relays;
    if !user_list.is_empty() {
        user_list
    } else {
        defaults.default_relays.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::substrate::KernelEvent;
    use nmp_core::substrate::EventId;
    use nmp_core::KernelEventObserver;
    use std::sync::Mutex;

    // kind:10007 — NIP-51 search relays (numeric literal; nmp_kinds is not a
    // direct dep of nmp-defaults, and importing it here just for a test
    // constant would be needless churn).
    const KIND_SEARCH_RELAYS: u32 = 10_007;

    const ALICE: &str = "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee";

    fn make_projection(active: Option<&str>) -> Arc<SearchRelayListProjection> {
        Arc::new(SearchRelayListProjection::new(Arc::new(Mutex::new(
            active.map(str::to_string),
        ))))
    }

    fn relay_event(author: &str, relays: &[&str]) -> KernelEvent {
        KernelEvent {
            id: EventId::from(
                "0000000000000000000000000000000000000000000000000000000000000001".to_string(),
            ),
            author: author.to_string(),
            kind: KIND_SEARCH_RELAYS,
            created_at: 100,
            tags: relays
                .iter()
                .map(|url| vec!["relay".to_string(), url.to_string()])
                .collect(),
            content: String::new(),
            relay_provenance: Vec::new(),
        }
    }

    #[test]
    fn user_list_takes_priority_over_default() {
        let proj = make_projection(Some(ALICE));
        proj.on_kernel_event(&relay_event(ALICE, &["wss://user-relay.example"]));

        let defaults = SearchDefaults::with_default_relays(vec!["wss://default.example".to_string()]);
        let relays = effective_search_relays(&proj, &defaults);

        assert_eq!(relays, vec!["wss://user-relay.example".to_string()]);
    }

    #[test]
    fn fallback_to_default_when_user_has_no_list() {
        let proj = make_projection(Some(ALICE));
        // No kind:10007 event ingested.

        let defaults = SearchDefaults::default();
        let relays = effective_search_relays(&proj, &defaults);

        assert_eq!(relays, vec![NMP_BUILTIN_SEARCH_RELAY.to_string()]);
    }

    #[test]
    fn custom_app_default_is_respected() {
        let proj = make_projection(Some(ALICE));

        let defaults = SearchDefaults::with_default_relays(vec![
            "wss://app-search.example".to_string(),
        ]);
        let relays = effective_search_relays(&proj, &defaults);

        assert_eq!(relays, vec!["wss://app-search.example".to_string()]);
    }

    #[test]
    fn account_switch_falls_back_to_default() {
        let slot = Arc::new(Mutex::new(Some(ALICE.to_string())));
        let proj = Arc::new(SearchRelayListProjection::new(Arc::clone(&slot)));
        proj.on_kernel_event(&relay_event(ALICE, &["wss://alice-search.example"]));

        // Simulate account switch: no kind:10007 for new account yet.
        const BOB: &str = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";
        *slot.lock().expect("slot") = Some(BOB.to_string());

        let defaults = SearchDefaults::default();
        let relays = effective_search_relays(&proj, &defaults);

        // Alice's relay must NOT bleed through to Bob's effective list.
        assert_eq!(
            relays,
            vec![NMP_BUILTIN_SEARCH_RELAY.to_string()],
            "after account switch, Alice's relay must not appear in Bob's effective list"
        );
    }
}
