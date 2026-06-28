//! Test-only observation registry for claim-expansion subscription routing.
//!
//! A pair of thread-local tables that let claim-expansion tests record which
//! `sub_id` was registered for which author and which `(sub_id, relay_url)`
//! match has been observed, so assertions can verify the kernel routed an
//! expanded claim onto the expected relay. Pure test scaffolding — never
//! compiled into production (the parent module is gated on
//! `cfg(any(test, feature = "test-support"))`).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use crate::relay::CanonicalRelayUrl;

thread_local! {
    static CLAIM_EXPANSION_SUBS: RefCell<BTreeMap<String, String>> =
        RefCell::new(BTreeMap::new());
    static CLAIM_EXPANSION_MATCHES: RefCell<BTreeSet<(String, String)>> =
        RefCell::new(BTreeSet::new());
}

pub(crate) fn register_claim_expansion_sub(sub_id: &str, author: &str) {
    CLAIM_EXPANSION_SUBS.with(|m| {
        m.borrow_mut()
            .insert(sub_id.to_string(), author.to_string());
    });
}

pub(crate) fn get_claim_expansion_author(sub_id: &str) -> Option<String> {
    CLAIM_EXPANSION_SUBS.with(|m| m.borrow().get(sub_id).cloned())
}

pub(crate) fn mark_claim_expansion_match_seen(sub_id: &str, relay_url: &str) {
    CLAIM_EXPANSION_MATCHES.with(|m| {
        m.borrow_mut().insert((
            sub_id.to_string(),
            CanonicalRelayUrl::parse_or_raw(relay_url).into_string(),
        ));
    });
}

pub(crate) fn take_claim_expansion_match_seen(sub_id: &str, relay_url: &str) -> bool {
    CLAIM_EXPANSION_MATCHES.with(|m| {
        m.borrow_mut().remove(&(
            sub_id.to_string(),
            CanonicalRelayUrl::parse_or_raw(relay_url).into_string(),
        ))
    })
}

pub(crate) fn clear_claim_expansion_subs() {
    CLAIM_EXPANSION_SUBS.with(|m| m.borrow_mut().clear());
    CLAIM_EXPANSION_MATCHES.with(|m| m.borrow_mut().clear());
}
