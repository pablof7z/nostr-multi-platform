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
//!    kind:10007 list published (new account, unconfigured), this explicit
//!    app/operator list is used.
//! 3. **Empty** — if neither source exists, relay search is cache-only. Shared
//!    NMP crates never pick a public operator on the app's behalf.
//!
//! # No operator policy in protocol crates
//!
//! `nmp-defaults` is a shared composition crate, not a leaf app. It wires the
//! search-relay seam but does not own a relay URL. Apps that want a default set
//! pass one with [`SearchDefaults::with_default_relays`] before calling
//! [`crate::register_defaults_with`]; apps that want no relay default use
//! [`SearchDefaults::default`].
//!
//! # Usage
//!
//! ```rust,ignore
//! // 1. Configure at build time.
//! let search_defaults = SearchDefaults::with_default_relays(vec![
//!     "wss://my-search.example".to_string(),
//! ]);
//! // Or use SearchDefaults::default() to declare no app-level search relay.
//!
//! // 2. Register the runtime and keep the projection handle.
//! let search_projection = register_search_relay_runtime_with(&mut app, search_defaults.clone());
//!
//! // 3. At query time, resolve the effective relay list.
//! let relays = effective_search_relays(&search_projection, &search_defaults);
//! ```

use std::sync::Arc;

use nmp_nip51::SearchRelayListProjection;

/// App-overridable default search-relay configuration.
///
/// An app calls [`SearchDefaults::with_default_relays`] to declare its own
/// fallback list. The list is used when the active account has no published
/// kind:10007 relay list (new user, unconfigured account, etc.).
///
/// The default-constructed value is empty: no shared NMP crate owns an
/// operator relay URL.
#[derive(Clone, Debug)]
pub struct SearchDefaults {
    /// Relay URLs to use when the user has no kind:10007 list.
    pub default_relays: Vec<String>,
}

impl Default for SearchDefaults {
    fn default() -> Self {
        Self {
            default_relays: Vec::new(),
        }
    }
}

impl SearchDefaults {
    /// Construct a [`SearchDefaults`] with an app-supplied relay list.
    ///
    /// Pass canonical `wss://` URLs. This is app/operator policy; shared NMP
    /// crates do not append or replace it with a built-in relay.
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
/// 3. Empty list when neither source exists
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
/// call, `snapshot()` returns an empty list, so the app default falls back. No
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
    use nmp_core::substrate::EventId;
    use nmp_core::substrate::KernelEvent;
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

        let defaults =
            SearchDefaults::with_default_relays(vec!["wss://default.example".to_string()]);
        let relays = effective_search_relays(&proj, &defaults);

        assert_eq!(relays, vec!["wss://user-relay.example".to_string()]);
    }

    #[test]
    fn default_search_defaults_are_empty_when_user_has_no_list() {
        let proj = make_projection(Some(ALICE));
        // No kind:10007 event ingested.

        let defaults = SearchDefaults::default();
        let relays = effective_search_relays(&proj, &defaults);

        assert!(
            relays.is_empty(),
            "shared defaults must not supply a public search relay"
        );
    }

    #[test]
    fn app_default_is_respected_when_user_has_no_list() {
        let proj = make_projection(Some(ALICE));

        let defaults =
            SearchDefaults::with_default_relays(vec!["wss://app-search.example".to_string()]);
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

        let defaults = SearchDefaults::with_default_relays(vec![
            "wss://bob-default-search.example".to_string(),
        ]);
        let relays = effective_search_relays(&proj, &defaults);

        // Alice's relay must NOT bleed through to Bob's effective list.
        assert_eq!(
            relays,
            vec!["wss://bob-default-search.example".to_string()],
            "after account switch, Alice's relay must not appear in Bob's effective list"
        );
    }
}
