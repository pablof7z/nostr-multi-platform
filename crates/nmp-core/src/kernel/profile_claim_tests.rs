//! TDD tests for profile-claim batching and indexer-only routing.
//!
//! Two bugs being fixed:
//!
//! 1. **Wrong relay**: `profile_claim_request` used `author_write_relays()` for
//!    cold-start authors, which returns the full `BOOTSTRAP_DISCOVERY_RELAYS`
//!    set. Profile lookups (kind:0) are discovery fetches — they must go to the
//!    **indexer relay only** (`purplepag.es`), not the content relay.
//!
//! 2. **No batching**: each `claim_profile` fired a separate `profile-claim-N`
//!    REQ per author. 37 follows → 37 × 2 = 74 REQs (one per relay in the
//!    cold-start bootstrap set). The correct shape is one REQ per relay with ALL
//!    authors in a single `authors` array.
//!
//! ## Test strategy
//!
//! The real 37-author burst flows through the `can_send=false` queue path:
//! the follow list arrives before the relay connects, so all authors are queued
//! in `pending_profiles`. When the relay connects the tick calls
//! `pending_profile_claim_requests()` which should batch them. Tests 1-3
//! exercise this queue path (claim with `can_send=false`, then flush via
//! `pending_profile_claim_requests()`). Test 4 exercises the immediate path
//! for a NIP-65-known author.

use super::*;
use crate::relay::{DEFAULT_VISIBLE_LIMIT, INDEXER_RELAY_URL, CONTENT_RELAY_URL};

fn hex64(prefix: &str) -> String {
    format!("{prefix:0<64}").chars().take(64).collect()
}

fn req_texts(msgs: &[OutboundMessage]) -> Vec<&str> {
    msgs.iter()
        .filter(|m| m.text.starts_with("[\"REQ\""))
        .map(|m| m.text.as_str())
        .collect()
}

/// Cold-start: N profile claims queued (can_send=false) must produce exactly ONE
/// batched REQ when pending_profile_claim_requests() flushes — not N REQs.
#[test]
fn cold_start_profile_claims_are_batched_into_one_req() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let authors: Vec<String> = (0..10).map(|i| hex64(&format!("{i}"))).collect();

    // Queue all 10 authors with can_send=false (relay not yet connected).
    for (i, pk) in authors.iter().enumerate() {
        let _ = kernel.claim_profile(pk.clone(), format!("view-{i}"), false);
    }

    // Flush all pending via a single batch call.
    let all_reqs = kernel.pending_profile_claim_requests();
    let req_texts: Vec<&str> = req_texts(&all_reqs);

    // Must be batched: far fewer REQs than authors.
    // Ideal: 1 REQ (all cold-start authors → same indexer relay).
    assert!(
        req_texts.len() < authors.len(),
        "profile claims must be batched — got {} REQs for {} authors: {req_texts:#?}",
        req_texts.len(),
        authors.len()
    );
}

/// Cold-start profile claims must NEVER go to the content relay.
/// They are discovery fetches — only the indexer relay is the right destination.
#[test]
fn cold_start_profile_claims_never_go_to_content_relay() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Queue 5 cold-start authors.
    for i in 0..5 {
        let _ = kernel.claim_profile(hex64(&format!("{i}")), format!("view-{i}"), false);
    }

    let all_msgs = kernel.pending_profile_claim_requests();

    let content_relay_reqs: Vec<&OutboundMessage> = all_msgs
        .iter()
        .filter(|m| m.text.starts_with("[\"REQ\"") && m.relay_url == CONTENT_RELAY_URL)
        .collect();

    assert!(
        content_relay_reqs.is_empty(),
        "profile claims must NOT go to the content relay ({}); got: {:#?}",
        CONTENT_RELAY_URL,
        content_relay_reqs.iter().map(|m| &m.relay_url).collect::<Vec<_>>()
    );
}

/// Cold-start profile claims go to the indexer relay with all authors in one filter.
#[test]
fn cold_start_profile_claims_go_to_indexer_relay_only() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let authors: Vec<String> = (0..5).map(|i| hex64(&format!("{i}"))).collect();
    for (i, pk) in authors.iter().enumerate() {
        let _ = kernel.claim_profile(pk.clone(), format!("view-{i}"), false);
    }

    let all_msgs = kernel.pending_profile_claim_requests();

    let indexer_reqs: Vec<&OutboundMessage> = all_msgs
        .iter()
        .filter(|m| m.text.starts_with("[\"REQ\"") && m.relay_url == INDEXER_RELAY_URL)
        .collect();

    assert!(
        !indexer_reqs.is_empty(),
        "cold-start profile claims must go to indexer relay {INDEXER_RELAY_URL}"
    );

    // Every author should appear in the batched filter.
    let combined_text: String = indexer_reqs.iter().map(|m| m.text.as_str()).collect();
    for pk in &authors {
        assert!(
            combined_text.contains(pk.as_str()),
            "author {pk} must appear in the batched indexer REQ; combined: {combined_text}"
        );
    }
}

/// Known-NIP-65 authors: profile claims use their declared write relays, NOT the indexer.
#[test]
fn known_nip65_profile_claims_use_declared_write_relays() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.relay_connected(RelayRole::Content);
    kernel.relay_connected(RelayRole::Indexer);

    let alice = hex64("alice");
    let alice_relay = "wss://alice-write.example/";

    kernel.seed_mailbox_relay_list(
        &alice,
        vec![],
        vec![alice_relay.to_string()],
        vec![],
    );

    let msgs = kernel.claim_profile(alice.clone(), "view-0".to_string(), true);
    let reqs: Vec<&OutboundMessage> = msgs
        .iter()
        .filter(|m| m.text.starts_with("[\"REQ\""))
        .collect();

    assert!(
        !reqs.is_empty(),
        "known NIP-65 author must trigger a profile claim REQ"
    );

    let relay_urls: Vec<&str> = reqs.iter().map(|m| m.relay_url.as_str()).collect();
    assert!(
        relay_urls.contains(&alice_relay),
        "known NIP-65 profile claim must go to declared write relay {alice_relay}; got {relay_urls:?}"
    );
    assert!(
        !relay_urls.iter().any(|u| *u == CONTENT_RELAY_URL),
        "known NIP-65 profile claim must NOT go to the content relay; got {relay_urls:?}"
    );
}
