//! App-default search-relay configuration.
//!
//! # Design
//!
//! NIP-50 search needs a relay list at runtime. The preference order is:
//!
//! 1. **User-published kind:10007 list** — populated by
//!    [`crate::SearchRelayListProjection`] from the live subscription registered by
//!    [`crate::register_search_relay_runtime`]. Authoritative: the user explicitly
//!    chose these relays.
//! 2. **App default** — a small list the APP (not NMP, not this crate) supplies at
//!    build time via [`SearchDefaults`]. If the user has no kind:10007 list
//!    published (new account, unconfigured), this explicit app/operator list is
//!    used.
//! 3. **Empty** — if neither source exists, relay search is cache-only. Shared NMP
//!    crates never pick a public operator on the app's behalf.
//!
//! # No operator policy in protocol crates
//!
//! This crate is a shared composition entry-point, not a leaf app. It wires the
//! search-relay seam but does not own a relay URL. Apps that want a default set
//! pass one with [`SearchDefaults::with_default_relays`] before calling
//! [`crate::register_search_relay_runtime_with`]; apps that want no relay default
//! use [`SearchDefaults::default`].

use std::sync::Arc;

use crate::SearchRelayListProjection;

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
