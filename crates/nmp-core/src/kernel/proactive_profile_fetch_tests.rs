//! F-CR-00 deterministic proof: removing the proactive kind:0 fetch on kind:1 ingest.
//!
//! ## What these tests prove
//!
//! `timeline.rs` previously called `request_profile_for_rendered_note` on every
//! kind:1 ingest, unconditionally queueing a kind:0 REQ for the author regardless
//! of any component claim. The F-CR-00 capstone removes that call so the kernel
//! fetches kind:0 ONLY in response to a profile `resolve_ref` from a component.
//!
//! Two invariants are pinned here:
//!
//! 1. `kind1_ingest_does_not_queue_profile_fetch` — ingesting a kind:1 for an
//!    author does NOT move the author into `profile_requests.pending` or
//!    `profile_requests.requested`. The proactive fetch is gone.
//!
//! 2. `resolve_profile_after_ingest_queues_fetch` — after the same ingest,
//!    resolving the author's profile (`resolve_ref`, can_send=true) DOES emit a
//!    kind:0 REQ. The resolve-driven path fully replaces the proactive path.
//!
//! Together these two tests are the deterministic before/after proof required
//! by the F-CR-00 capstone task (timeline.rs:172 removal).
//!
//! ## Test strategy
//!
//! Both tests drive a real Schnorr-signed kind:1 event through
//! `ingest_timeline_event` with a `diag-firehose-*` sub_id — the same trick
//! the timeline perf harness uses — so the event is accepted by
//! `should_store_event` without needing to pre-populate `timeline_authors`.
//! This keeps the fixture minimal while exercising the real ingest code path
//! (including `request_profile_for_rendered_note` before removal and its
//! absence after removal).
//!
//! `Kernel::relay_connected` is called for both roles so that the profile
//! `resolve_ref` with `can_send=true` finds an open relay and emits REQs synchronously
//! (the `can_send=false` queue path is already exercised by `profile_claim_tests`).

use super::nostr::NostrEvent;
use super::*;
use crate::relay::{DEFAULT_VISIBLE_LIMIT, INDEXER_RELAY_URL};
use nmp_network::role::RelayRole;

/// Build a real Schnorr-signed kind:1 event.
///
/// Uses `nostr::EventBuilder::text_note` so the fixture survives
/// `VerifiedEvent::try_from_raw`'s full secp256k1 sig verification.
fn signed_kind1(keys: &::nostr::Keys, content: &str, ts: u64) -> NostrEvent {
    let nostr_event = ::nostr::EventBuilder::text_note(content)
        .custom_created_at(::nostr::Timestamp::from(ts))
        .sign_with_keys(keys)
        .expect("sign_with_keys cannot fail with a generated keypair");
    NostrEvent {
        id: nostr_event.id.to_hex(),
        pubkey: nostr_event.pubkey.to_hex(),
        created_at: nostr_event.created_at.as_secs(),
        kind: nostr_event.kind.as_u16() as u32,
        tags: nostr_event
            .tags
            .iter()
            .map(|t| t.as_slice().to_vec())
            .collect(),
        content: nostr_event.content.clone(),
        sig: nostr_event.sig.to_string(),
    }
}

/// F-CR-00 invariant 1: ingesting a kind:1 MUST NOT queue a kind:0 fetch.
///
/// Before the removal of `request_profile_for_rendered_note` at timeline.rs:172
/// this test **fails** (the proactive fetch moves the author into
/// `profile_requests.pending`). After the removal it **passes** — the pending
/// and requested sets stay empty, proving the proactive path is gone.
#[test]
fn kind1_ingest_does_not_queue_profile_fetch() {
    let keys = ::nostr::Keys::generate();
    let event = signed_kind1(&keys, "hello world", 1_700_000_000);
    let author = event.pubkey.clone();

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // Connect relays so any queued profile request would be sent immediately
    // (can_send=true path). If anything were proactively queued the assertion
    // below would catch it in `requested` rather than just `pending`.
    kernel.relay_connected(RelayRole::Content);
    kernel.relay_connected(RelayRole::Indexer);

    // Ingest through the diag-firehose path (accepted by should_store_event
    // without needing timeline_authors pre-population — mirrors timeline_perf_tests).
    kernel.ingest_timeline_event(
        RelayRole::Content,
        "wss://test.relay",
        "diag-firehose-fcr00-test",
        event,
    );

    // No kind:0 claim interest must be registered for the author. The
    // proactive fetch at timeline.rs:172 would have registered/queued a kind:0
    // fetch; its removal leaves no claim interest (kind:0 is claim-driven only).
    assert!(
        !kernel.profile_claim_interest_registered_for_test(&author),
        "kind:1 ingest must NOT register a kind:0 claim interest for the author; \
         F-CR-00 proactive fetch was not removed"
    );
}

/// F-CR-00 invariant 2: a profile `resolve_ref` after ingest DOES fetch kind:0.
///
/// After the proactive fetch is removed the resolve-driven path must still
/// trigger a kind:0 fetch — otherwise author names would blank out. M2
/// migration: `resolve_ref` registers a kind:0 `LogicalInterest`; the planner
/// emits the REQ on the next `drain_lifecycle_outbound`. This test drives the
/// resolve, then the drain, and asserts a kind:0 REQ mentioning the author is
/// emitted to the cold-start indexer relay.
#[test]
fn resolve_profile_after_ingest_queues_fetch() {
    let keys = ::nostr::Keys::generate();
    let event = signed_kind1(&keys, "a note to trigger ingest", 1_700_000_001);
    let author = event.pubkey.clone();

    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    kernel.relay_connected(RelayRole::Content);
    kernel.relay_connected(RelayRole::Indexer);

    // Ingest the kind:1 first (no claim interest should be registered).
    kernel.ingest_timeline_event(
        RelayRole::Content,
        "wss://test.relay",
        "diag-firehose-fcr00-claim-test",
        event,
    );
    assert!(
        !kernel.profile_claim_interest_registered_for_test(&author),
        "ingest alone must not register a kind:0 claim interest"
    );

    // Now a component resolves the author (CacheOk → OneShot kind:0 fetch).
    let _ = kernel.resolve_ref(
            RefNamespace::Profile,
            author.clone(),
            "test-consumer-id".to_string(),
            RefShape::Profile(ProfileShape::Card),
            RefLiveness::CacheOk.into(),
            false,
            Vec::new(),
        );
    assert!(
        kernel.profile_claim_interest_registered_for_test(&author),
        "resolve_ref must register a kind:0 claim interest for the author"
    );

    // The planner emits the wire REQ on the next drain.
    let msgs = kernel.drain_lifecycle_outbound();
    let req_msgs: Vec<&OutboundMessage> = msgs
        .iter()
        .filter(|m| m.text.starts_with("[\"REQ\""))
        .collect();
    assert!(
        !req_msgs.is_empty(),
        "the planner must emit a kind:0 REQ after a claim; got no REQs"
    );

    // The cold-start (uncached mailbox) REQ reaches the indexer relay.
    let targets_indexer = req_msgs.iter().any(|m| m.relay_url == INDEXER_RELAY_URL);
    assert!(
        targets_indexer,
        "kind:0 claim REQ must reach the indexer relay ({INDEXER_RELAY_URL}) at cold start; \
         got relay URLs: {:?}",
        req_msgs.iter().map(|m| &m.relay_url).collect::<Vec<_>>()
    );

    // The REQ filter must mention the author's pubkey.
    let all_req_text: String = req_msgs.iter().map(|m| m.text.as_str()).collect();
    assert!(
        all_req_text.contains(&author),
        "kind:0 REQ must include the author's pubkey in the filter; req texts: {all_req_text}"
    );
}
