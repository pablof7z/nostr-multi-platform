use std::collections::BTreeSet;

use nmp_core::planner::{
    InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest,
};

// Kind constants sourced from the canonical registry in nmp-core::kinds
// (which re-exports from nmp-kinds, the zero-dep Layer-0 crate — V-57 P2).
// Public re-exports preserve downstream visibility for crate-internal users
// (score.rs, runtime.rs, lib.rs) that previously saw these as pub const here.
// KIND_MUTE_LIST (10000) is not yet in the registry; defer to a follow-up.
/// NIP-01 profile metadata (canonical source: nmp-kinds::KIND_PROFILE_METADATA).
pub const KIND_PROFILE: u32 = nmp_core::kinds::KIND_PROFILE_METADATA;
/// NIP-02 contact list (canonical source: nmp-kinds::KIND_CONTACT_LIST).
pub use nmp_core::kinds::KIND_CONTACT_LIST;
/// NIP-51 mute list.
pub const KIND_MUTE_LIST: u32 = 10_000;
/// NIP-65 relay list (canonical source: nmp-kinds::KIND_RELAY_LIST).
pub use nmp_core::kinds::KIND_RELAY_LIST;

/// Replaceable kinds fetched for followed authors during WOT bootstrap.
pub const WOT_BOOTSTRAP_KINDS: [u32; 4] = [
    KIND_PROFILE,
    KIND_CONTACT_LIST,
    KIND_MUTE_LIST,
    KIND_RELAY_LIST,
];

/// Stable single-slot id for the active account's WOT bootstrap fetch.
#[must_use]
pub fn active_follow_graph_interest_id() -> InterestId {
    InterestId(nmp_core::stable_hash::stable_hash64(
        "wot.follow_graph.active",
    ))
}

/// Build the one-shot replaceable-kind fetch used to seed local WOT state.
///
/// The shape is intentionally exact: explicit `authors`, explicit replaceable
/// `kinds`, no `limit`, no tags. That lets the generic NIP-77 runtime reconcile
/// the set by author-kind product instead of sending a blind large REQ.
#[must_use]
pub fn follow_graph_interest<I, S>(authors: I) -> Option<LogicalInterest>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let authors = authors
        .into_iter()
        .map(Into::into)
        .filter(|author| is_hex_pubkey(author))
        .collect::<BTreeSet<_>>();
    if authors.is_empty() {
        return None;
    }

    Some(LogicalInterest {
        id: active_follow_graph_interest_id(),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors,
            kinds: WOT_BOOTSTRAP_KINDS.into_iter().collect(),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::OneShot,
        // WoT bootstrap fetches contacts for known authors via NIP-65; the
        // mailbox is expected to be cached by the time WoT runs, so no
        // bootstrap-indexer fallback opt-in.
        is_indexer_discovery: false,
    })
}

pub(crate) fn is_hex_pubkey(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn author(n: u16) -> String {
        format!("{n:064x}")
    }

    #[test]
    fn builds_exact_one_shot_replaceable_interest() {
        let interest = follow_graph_interest([author(2), "bad".to_string(), author(1)]).unwrap();

        assert_eq!(interest.id, active_follow_graph_interest_id());
        assert!(matches!(interest.lifecycle, InterestLifecycle::OneShot));
        assert!(matches!(interest.scope, InterestScope::Global));
        assert_eq!(interest.shape.limit, None);
        assert_eq!(
            interest.shape.authors.into_iter().collect::<Vec<_>>(),
            vec![author(1), author(2)]
        );
        assert_eq!(
            interest.shape.kinds.into_iter().collect::<Vec<_>>(),
            WOT_BOOTSTRAP_KINDS
        );
    }

    #[test]
    fn empty_or_invalid_author_set_returns_none() {
        assert!(follow_graph_interest(["not-a-pubkey"]).is_none());
    }

    #[test]
    fn large_wot_bootstrap_interest_opens_nip77() {
        use nmp_core::planner::InterestLifecycle;
        use nmp_core::substrate::{ReqFrameContext, ReqFrameInterceptor};
        use nmp_core::{Kernel, RelayRole};

        let interest = follow_graph_interest((0..1_052).map(author)).unwrap();
        let filter_json = serde_json::json!({
            "authors": interest.shape.authors.iter().cloned().collect::<Vec<_>>(),
            "kinds": interest.shape.kinds.iter().copied().collect::<Vec<_>>(),
        })
        .to_string();
        let ctx = ReqFrameContext {
            role: RelayRole::Indexer,
            relay_url: "wss://relay.example".to_string(),
            sub_id: "wot-bootstrap".to_string(),
            filter_json,
            interest_id: interest.id,
            lifecycle: InterestLifecycle::OneShot,
        };
        let runtime = nmp_nip77::NegentropySyncRuntime::new(Default::default());
        let mut kernel = Kernel::testing_new(50);

        let out = runtime
            .intercept_req(&mut kernel, &ctx)
            .expect("large WOT bootstrap must use NIP-77");

        assert_eq!(out.len(), 1);
        assert!(out[0].text().starts_with(r#"["NEG-OPEN","wot-bootstrap","#));
    }
}
