//! Tests for the generic `claim_event` / `release_event` kernel primitive
//! and the `claimed_events` snapshot projection (F-CR-06 / ADR-0034).
//!
//! These tests stay scoped to `nmp-core`: no relay traffic, no actor wiring,
//! no FFI. Each test drives `Kernel::claim_event` / `release_event` directly
//! and asserts on either the `event_claims` refcount state, the
//! `discovery_in_flight()` OneshotApi counter, or the snapshot's
//! `projections.claimed_events` map.
//!
//! Test-support paths:
//! - `inject_replaceable_event` covers kinds 0/3/10002 (the only kinds with
//!   kernel-side ingest arms) but is NOT suitable for kind:30023 / kind:1 —
//!   for those the store would accept the insert but `self.events` would not
//!   be populated. We use `ingest_pre_verified_event` directly here so the
//!   read-cache `claim_event` consults is up to date.

use super::*;
use crate::nip19::{encode_naddr, encode_nevent, NaddrData, NeventData};
use crate::relay::{RelayRole, DEFAULT_VISIBLE_LIMIT};
use crate::store::{RawEvent, VerifiedEvent};

const TEST_AUTHOR_HEX: &str = "abababababababababababababababababababababababababababababababab";
const TEST_D_TAG: &str = "kind-dispatch";

/// Helper: build a 64-hex event id from a single-char prefix (rest zeros).
fn hex64(prefix: &str) -> String {
    let mut s = prefix.to_string();
    while s.len() < 64 {
        s.push('0');
    }
    s.chars().take(64).collect()
}

/// Helper: build an `nostr:nevent…` URI for an event with no relay hints.
fn nevent_uri(event_id: &str, kind: Option<u32>, author: Option<&str>) -> String {
    let bech = encode_nevent(&NeventData {
        event_id: event_id.to_string(),
        relays: vec![],
        author: author.map(str::to_string),
        kind,
    })
    .expect("encode_nevent");
    format!("nostr:{bech}")
}

/// Helper: build an `nostr:naddr…` URI for a kind:30023 article.
fn naddr_uri(kind: u32, author: &str, d_tag: &str) -> String {
    let bech = encode_naddr(&NaddrData {
        identifier: d_tag.to_string(),
        pubkey: author.to_string(),
        kind,
        relays: vec![],
    })
    .expect("encode_naddr");
    format!("nostr:{bech}")
}

/// Helper: inject a kind:30023 article event with `(author, d_tag)` into the
/// kernel's read-cache. Bypasses signature verification and the replaceable-
/// event dispatch arms (kind:30023 has no kernel-side ingest arm, so
/// `inject_replaceable_event` would store but not populate `events`).
fn inject_article(
    kernel: &mut Kernel,
    id: &str,
    author: &str,
    kind: u32,
    d_tag: &str,
    title: &str,
) {
    let raw = RawEvent {
        id: id.to_string(),
        pubkey: author.to_string(),
        created_at: 1_700_000_000,
        kind,
        tags: vec![
            vec!["d".to_string(), d_tag.to_string()],
            vec!["title".to_string(), title.to_string()],
        ],
        content: format!("body of {title}"),
        sig: "a".repeat(128),
    };
    kernel.ingest_pre_verified_event(
        RelayRole::Content,
        "test-event-claim",
        VerifiedEvent::from_raw_unchecked(raw),
    );
}

/// Helper: inject a bare kind:1 note (event-id-form `primary_id`).
fn inject_note(kernel: &mut Kernel, id: &str, author: &str, content: &str) {
    let raw = RawEvent {
        id: id.to_string(),
        pubkey: author.to_string(),
        created_at: 1_700_000_000,
        kind: 1,
        tags: vec![],
        content: content.to_string(),
        sig: "a".repeat(128),
    };
    kernel.ingest_pre_verified_event(
        RelayRole::Content,
        "test-event-claim",
        VerifiedEvent::from_raw_unchecked(raw),
    );
}

/// 1. A `claim_event` for an event already in the read-cache short-circuits
/// the OneshotApi registration — no discovery REQ is queued, the projection
/// emits the DTO immediately.
#[test]
fn claim_event_for_known_event_id_resolves_without_relay() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let id = hex64("e");
    inject_note(&mut kernel, &id, TEST_AUTHOR_HEX, "hello world");

    let uri = nevent_uri(&id, Some(1), Some(TEST_AUTHOR_HEX));
    let outbound = kernel.claim_event(uri, "view-0".to_string(), true);

    // No outbound frames — wire emission flows through the planner, and in
    // this case the kernel short-circuits before even registering interest.
    assert!(
        outbound.is_empty(),
        "claim_event must not emit OutboundMessages (D4 — planner emits)"
    );
    // No OneshotApi registration when the event is already cached.
    assert_eq!(
        kernel.discovery_in_flight(),
        0,
        "already-known event must not trigger a discovery oneshot"
    );
    assert!(
        !kernel.event_claim_is_requested_for_test(&id),
        "event_claim_requested must be empty for already-known event"
    );
    // Refcount recorded.
    assert_eq!(kernel.event_claims_len_for_test(&id), 1);

    // Snapshot carries the DTO.
    let snapshot = kernel.make_update_value_for_test(true);
    let entry = &snapshot["projections"]["claimed_events"][&id];
    assert!(entry.is_object(), "claimed_events[id] must be present");
    assert_eq!(entry["id"], id);
    assert_eq!(entry["kind"], 1);
    assert_eq!(entry["author_pubkey"], TEST_AUTHOR_HEX);
    assert_eq!(entry["content"], "hello world");
}

/// 2. A `claim_event` for an unknown event id registers a OneShot + Global
/// interest on the lifecycle registry (the OneshotApi `in_flight` counter
/// goes from 0 to 1) and records the `primary_id` in
/// `event_claim_requested` so a second claim is deduped.
#[test]
fn claim_event_emits_oneshot_request_via_lifecycle_registry() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let id = hex64("f");
    let uri = nevent_uri(&id, Some(1), None);

    assert_eq!(kernel.discovery_in_flight(), 0);
    let outbound = kernel.claim_event(uri.clone(), "view-0".to_string(), true);

    assert!(
        outbound.is_empty(),
        "claim_event returns Vec::new() (D4 — wire emission flows through planner)"
    );
    assert_eq!(
        kernel.discovery_in_flight(),
        1,
        "OneshotApi must register exactly one interest on cold-claim"
    );
    assert!(
        kernel.event_claim_is_requested_for_test(&id),
        "event_claim_requested must record the primary_id"
    );

    // Second claim from a different consumer must NOT register a new
    // interest — the `event_claim_requested` set dedupes.
    let _ = kernel.claim_event(uri, "view-1".to_string(), true);
    assert_eq!(
        kernel.discovery_in_flight(),
        1,
        "duplicate claim must not register a second OneshotApi interest"
    );
    assert_eq!(kernel.event_claims_len_for_test(&id), 2);
}

/// 3. An `naddr`-form URI resolves to the same article event when its
/// `(kind, author, d_tag)` triple matches a stored event, and the
/// projection key is the coordinate string `kind:pubkey:d_tag`.
#[test]
fn claim_event_naddr_matches_kind_pubkey_dtag_in_store() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let id = hex64("a1");
    inject_article(
        &mut kernel,
        &id,
        TEST_AUTHOR_HEX,
        30023,
        TEST_D_TAG,
        "ADR-0034",
    );

    let uri = naddr_uri(30023, TEST_AUTHOR_HEX, TEST_D_TAG);
    let coord_key = format!("30023:{TEST_AUTHOR_HEX}:{TEST_D_TAG}");

    let _ = kernel.claim_event(uri, "view-0".to_string(), true);

    // Already-resolved naddr → no fetch.
    assert_eq!(
        kernel.discovery_in_flight(),
        0,
        "already-resolved naddr must not trigger a discovery oneshot"
    );
    assert!(
        !kernel.event_claim_is_requested_for_test(&coord_key),
        "event_claim_requested must stay empty when the addressable triple is cached"
    );

    let snapshot = kernel.make_update_value_for_test(true);
    let entry = &snapshot["projections"]["claimed_events"][&coord_key];
    assert!(
        entry.is_object(),
        "claimed_events[{coord_key}] must be present after claim resolves"
    );
    assert_eq!(entry["primary_id"], coord_key);
    assert_eq!(entry["id"], id);
    assert_eq!(entry["kind"], 30023);
    assert_eq!(entry["author_pubkey"], TEST_AUTHOR_HEX);
}

/// 4. `release_event` removes the consumer from the per-`primary_id` set;
/// on the empty set the row is dropped along with the
/// `event_claim_requested` entry (so a re-claim can re-fetch).
#[test]
fn release_event_drops_consumer_and_removes_key_on_empty_set() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let id = hex64("b");
    let uri = nevent_uri(&id, Some(1), None);
    let _ = kernel.claim_event(uri.clone(), "view-0".to_string(), true);
    let _ = kernel.claim_event(uri.clone(), "view-1".to_string(), true);
    assert_eq!(kernel.event_claims_len_for_test(&id), 2);
    assert!(kernel.event_claim_is_requested_for_test(&id));

    // First release: row stays, requested-set entry stays.
    let _ = kernel.release_event(&uri, "view-0");
    assert_eq!(kernel.event_claims_len_for_test(&id), 1);
    assert!(
        kernel.event_claim_is_requested_for_test(&id),
        "event_claim_requested must persist while any consumer holds the claim"
    );

    // Second release: row gone, requested-set cleared.
    let _ = kernel.release_event(&uri, "view-1");
    assert_eq!(kernel.event_claims_len_for_test(&id), 0);
    assert!(
        !kernel.event_claim_is_requested_for_test(&id),
        "event_claim_requested must clear when the last consumer releases"
    );
}

/// 5. The `MAX_EVENT_CLAIMS_PER_KEY` cap bounds the consumer set; overflow
/// silently no-ops and increments `event_claim_drops_total`.
#[test]
fn claim_event_bounded_at_max_event_claims_per_key() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let id = hex64("c");
    let uri = nevent_uri(&id, Some(1), None);

    for i in 0..MAX_EVENT_CLAIMS_PER_KEY {
        let _ = kernel.claim_event(uri.clone(), format!("view-{i}"), true);
    }
    assert_eq!(
        kernel.event_claims_len_for_test(&id),
        MAX_EVENT_CLAIMS_PER_KEY
    );
    assert_eq!(kernel.event_claim_drops_total_for_test(), 0);

    // One past the cap: silently dropped.
    let _ = kernel.claim_event(uri.clone(), "view-overflow".to_string(), true);
    assert_eq!(
        kernel.event_claims_len_for_test(&id),
        MAX_EVENT_CLAIMS_PER_KEY,
        "claim_event must not grow the set past MAX_EVENT_CLAIMS_PER_KEY"
    );
    assert_eq!(
        kernel.event_claim_drops_total_for_test(),
        1,
        "overflow must increment event_claim_drops_total"
    );

    // An already-present consumer_id is idempotent and does NOT count as
    // a drop.
    let already_present = "view-0".to_string();
    let _ = kernel.claim_event(uri, already_present, true);
    assert_eq!(
        kernel.event_claim_drops_total_for_test(),
        1,
        "re-claim by existing consumer must not bump event_claim_drops_total"
    );
}

/// 6. Snapshot push semantics (D8): a claim registered BEFORE the event
/// arrives leaves `claimed_events` empty; once the event is ingested the
/// next snapshot tick surfaces the DTO under the `primary_id` key.
#[test]
fn claimed_events_projection_emits_dto_keyed_by_primary_id() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let id = hex64("d");
    let uri = nevent_uri(&id, Some(1), Some(TEST_AUTHOR_HEX));

    // Pre-arrival: the claim registers an interest but the projection has
    // no entry (the event is not yet in the read-cache).
    let _ = kernel.claim_event(uri, "view-0".to_string(), true);
    let snapshot = kernel.make_update_value_for_test(true);
    let entry = &snapshot["projections"]["claimed_events"][&id];
    assert!(
        entry.is_null(),
        "claimed_events[{id}] must be absent before the event arrives — got {entry:?}"
    );

    // Inject the event and re-emit; the DTO must appear under the same
    // key (the kernel's `primary_id` is the event-id hex).
    inject_note(&mut kernel, &id, TEST_AUTHOR_HEX, "post-arrival content");
    let snapshot = kernel.make_update_value_for_test(true);
    let entry = &snapshot["projections"]["claimed_events"][&id];
    assert!(
        entry.is_object(),
        "claimed_events[{id}] must surface after ingest — got {entry:?}"
    );
    assert_eq!(entry["primary_id"], id);
    assert_eq!(entry["content"], "post-arrival content");
    assert_eq!(entry["kind"], 1);
}
