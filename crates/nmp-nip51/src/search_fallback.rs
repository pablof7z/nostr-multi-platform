//! App-supplied search-relay fallback handling.
//!
//! NIP-51 owns the active account's kind:10007 search-relay facts. NIP-50 owns
//! search target resolution. This module bridges the two by reading the NIP-51
//! projection and applying an app-supplied NIP-50 fallback relay list when the
//! active account has no kind:10007 list.

use std::sync::Arc;

use nmp_nip50::SearchFallbackRelays;

use crate::SearchRelayListProjection;

/// Return the effective search relay list for the active account.
///
/// Preference order:
/// 1. User's kind:10007 snapshot (non-empty means authoritative)
/// 2. App-supplied fallback relays
/// 3. Empty list when neither source exists
///
/// [`SearchRelayListProjection::snapshot`] gates on the live active-pubkey
/// slot, so stale relay data from a prior account is never returned.
#[must_use]
pub fn effective_search_relays(
    projection: &Arc<SearchRelayListProjection>,
    fallback_relays: &SearchFallbackRelays,
) -> Vec<String> {
    let user_list = projection.snapshot().relays;
    if !user_list.is_empty() {
        user_list
    } else {
        fallback_relays.relays.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::substrate::{EventId, KernelEvent};
    use nmp_core::ObservedProjectionSink;
    use nmp_kinds::KIND_SEARCH_RELAYS;
    use std::sync::Mutex;

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
    fn user_list_takes_priority_over_fallback() {
        let proj = make_projection(Some(ALICE));
        proj.on_kernel_event(&relay_event(ALICE, &["wss://user-relay.example"]));

        let fallback = SearchFallbackRelays::new(vec!["wss://fallback.example".to_string()]);
        let relays = effective_search_relays(&proj, &fallback);

        assert_eq!(relays, vec!["wss://user-relay.example".to_string()]);
    }

    #[test]
    fn empty_fallback_means_no_framework_relay() {
        let proj = make_projection(Some(ALICE));
        let relays = effective_search_relays(&proj, &SearchFallbackRelays::default());

        assert!(
            relays.is_empty(),
            "shared crates must not supply a public search relay"
        );
    }

    #[test]
    fn app_fallback_is_respected_when_user_has_no_list() {
        let proj = make_projection(Some(ALICE));

        let fallback = SearchFallbackRelays::new(vec!["wss://app-search.example".to_string()]);
        let relays = effective_search_relays(&proj, &fallback);

        assert_eq!(relays, vec!["wss://app-search.example".to_string()]);
    }

    #[test]
    fn account_switch_falls_back_to_app_relays() {
        let slot = Arc::new(Mutex::new(Some(ALICE.to_string())));
        let proj = Arc::new(SearchRelayListProjection::new(Arc::clone(&slot)));
        proj.on_kernel_event(&relay_event(ALICE, &["wss://alice-search.example"]));

        const BOB: &str = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";
        *slot.lock().expect("slot") = Some(BOB.to_string());

        let fallback =
            SearchFallbackRelays::new(vec!["wss://bob-fallback-search.example".to_string()]);
        let relays = effective_search_relays(&proj, &fallback);

        assert_eq!(
            relays,
            vec!["wss://bob-fallback-search.example".to_string()],
            "after account switch, Alice's relay must not appear in Bob's effective list"
        );
    }
}
