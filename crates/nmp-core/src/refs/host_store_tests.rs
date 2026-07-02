//! ADR-0070 (#1671 Lane F) — `RefProfileStore` host-consumption tests.
//!
//! These assert the shell-facing contract the Rust shells rely on:
//! - a `kind:0` ingest (a `Changed` `refs.profile` row carrying a fresh KPRF
//!   card) updates `profile(pubkey)` (the rendered-profile-updates-via-refs gate);
//! - an absent key is Unchanged, not cleared;
//! - an explicit `Cleared` row drops the row (ref released / view closed);
//! - a garbage sidecar payload is a fail-closed no-op (prior cache retained).

use super::{RefEventStore, RefProfileStore};
use crate::kernel::public_typed_projections::{
    encode_claimed_events, encode_profile, ClaimedEventRow, ClaimedEventsModel, ProfileCardModel,
};
use crate::refs::{encode_ref_row_delta_batch, RefRow, RefRowDeltaBatch};

const ALICE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BOB: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const EVENT_ID: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

/// Build a KPRF row payload (what the kernel's `ref_row_source` emits per key).
fn card_payload(pubkey: &str, display_name: &str, picture_url: &str) -> Vec<u8> {
    encode_profile(&ProfileCardModel {
        pubkey: pubkey.to_string(),
        display_name: Some(display_name.to_string()),
        picture_url: Some(picture_url.to_string()),
        ..Default::default()
    })
}

/// Encode a `refs.profile` sidecar payload (an NRRD batch in the `"profile"`
/// namespace) — the bytes the shell pulls from the typed-projection sidecar.
fn sidecar(baseline: bool, rows: Vec<RefRow>) -> Vec<u8> {
    encode_ref_row_delta_batch(&RefRowDeltaBatch {
        namespace: "profile".to_string(),
        baseline,
        rows,
    })
}

fn event_payload(primary_id: &str, content: &str) -> Vec<u8> {
    encode_claimed_events(&ClaimedEventsModel {
        entries: vec![(
            primary_id.to_string(),
            ClaimedEventRow {
                primary_id: primary_id.to_string(),
                id: primary_id.to_string(),
                author_pubkey: ALICE.to_string(),
                kind: 1,
                created_at: 1_700_000_000,
                content: content.to_string(),
                tags: Vec::new(),
                content_tree_bytes: Vec::new(),
                signed_event_json: None,
                ..Default::default()
            },
        )],
    })
}

fn event_sidecar(baseline: bool, rows: Vec<RefRow>) -> Vec<u8> {
    encode_ref_row_delta_batch(&RefRowDeltaBatch {
        namespace: "event".to_string(),
        baseline,
        rows,
    })
}

#[test]
fn kind0_ingest_updates_rendered_profile_via_refs_profile() {
    let mut store = RefProfileStore::new();
    assert!(store.profile(ALICE).is_none(), "no ref yet");

    // First baseline: Alice resolves with her initial kind:0.
    let payload = sidecar(
        true,
        vec![RefRow::changed(
            ALICE,
            1,
            card_payload(ALICE, "Alice", "a.png"),
        )],
    );
    store.apply_sidecar(&payload, 1, 0);
    let card = store.profile(ALICE).expect("Alice resolved after baseline");
    assert_eq!(card.display_name.as_deref(), Some("Alice"));
    assert_eq!(card.picture_url.as_deref(), Some("a.png"));

    // A fresh kind:0 for Alice (newer rev) updates the rendered profile in place.
    let update = sidecar(
        false,
        vec![RefRow::changed(
            ALICE,
            2,
            card_payload(ALICE, "Alice v2", "a2.png"),
        )],
    );
    let outcome = store.apply_sidecar(&update, 1, 0);
    assert_eq!(outcome.changed_keys, vec![ALICE.to_string()]);
    let card = store.profile(ALICE).expect("Alice still resolved");
    assert_eq!(
        card.display_name.as_deref(),
        Some("Alice v2"),
        "rendered profile reflects the newer kind:0"
    );
    assert_eq!(card.picture_url.as_deref(), Some("a2.png"));
}

#[test]
fn absent_key_is_unchanged_not_cleared() {
    let mut store = RefProfileStore::new();
    store.apply_sidecar(
        &sidecar(
            true,
            vec![RefRow::changed(ALICE, 1, card_payload(ALICE, "Alice", ""))],
        ),
        1,
        0,
    );
    // An incremental frame about a DIFFERENT key must not drop Alice.
    store.apply_sidecar(
        &sidecar(
            false,
            vec![RefRow::changed(BOB, 1, card_payload(BOB, "Bob", ""))],
        ),
        1,
        0,
    );
    assert!(store.profile(ALICE).is_some(), "absent ⇒ unchanged");
    assert!(store.profile(BOB).is_some());
}

#[test]
fn explicit_clear_drops_the_ref() {
    let mut store = RefProfileStore::new();
    store.apply_sidecar(
        &sidecar(
            true,
            vec![RefRow::changed(ALICE, 1, card_payload(ALICE, "Alice", ""))],
        ),
        1,
        0,
    );
    assert!(store.profile(ALICE).is_some());
    // The view closed → last consumer released → kernel emits an explicit Cleared.
    store.apply_sidecar(&sidecar(false, vec![RefRow::cleared(ALICE, 2)]), 1, 0);
    assert!(
        store.profile(ALICE).is_none(),
        "explicit Cleared drops the cached row"
    );
}

#[test]
fn garbage_sidecar_is_fail_closed_noop() {
    let mut store = RefProfileStore::new();
    store.apply_sidecar(
        &sidecar(
            true,
            vec![RefRow::changed(ALICE, 1, card_payload(ALICE, "Alice", ""))],
        ),
        1,
        0,
    );
    let outcome = store.apply_sidecar(b"not an NRRD batch", 1, 0);
    assert!(outcome.changed_keys.is_empty());
    assert!(
        store.profile(ALICE).is_some(),
        "garbage payload never empties the live cache"
    );
}

#[test]
fn profiles_map_materialises_all_live_rows() {
    let mut store = RefProfileStore::new();
    store.apply_sidecar(
        &sidecar(
            true,
            vec![
                RefRow::changed(ALICE, 1, card_payload(ALICE, "Alice", "")),
                RefRow::changed(BOB, 1, card_payload(BOB, "Bob", "")),
            ],
        ),
        1,
        0,
    );
    let map = store.profiles();
    assert_eq!(map.len(), 2);
    assert_eq!(map[ALICE].display_name.as_deref(), Some("Alice"));
    assert_eq!(map[BOB].display_name.as_deref(), Some("Bob"));
}

#[test]
fn profile_card_row_preserves_full_card_fields() {
    let mut store = RefProfileStore::new();
    let payload = encode_profile(&ProfileCardModel {
        pubkey: ALICE.to_string(),
        display_name: Some("Alice".to_string()),
        about: "full card about field".to_string(),
        website: Some("https://alice.example".to_string()),
        ..Default::default()
    });
    store.apply_sidecar(
        &sidecar(true, vec![RefRow::changed(ALICE, 1, payload)]),
        1,
        0,
    );

    let card = store.profile(ALICE).expect("profile card row decoded");
    assert_eq!(card.about, "full card about field");
    assert_eq!(card.website.as_deref(), Some("https://alice.example"));
}

#[test]
fn event_embed_row_updates_lookup() {
    let mut store = RefEventStore::new();
    store.apply_sidecar(
        &event_sidecar(
            true,
            vec![RefRow::changed(
                EVENT_ID,
                1,
                event_payload(EVENT_ID, "resolved event"),
            )],
        ),
        1,
        0,
    );

    let row = store.event(EVENT_ID).expect("event row decoded");
    assert_eq!(row.content, "resolved event");
    assert_eq!(store.events()[EVENT_ID].kind, 1);
}

#[test]
fn event_clear_drops_the_ref() {
    let mut store = RefEventStore::new();
    store.apply_sidecar(
        &event_sidecar(
            true,
            vec![RefRow::changed(
                EVENT_ID,
                3,
                event_payload(EVENT_ID, "live"),
            )],
        ),
        1,
        0,
    );
    assert!(store.event(EVENT_ID).is_some());

    store.apply_sidecar(
        &event_sidecar(false, vec![RefRow::cleared(EVENT_ID, 4)]),
        1,
        0,
    );
    assert!(store.event(EVENT_ID).is_none());
}

#[test]
fn stale_event_rev_does_not_replace_newer_row() {
    let mut store = RefEventStore::new();
    store.apply_sidecar(
        &event_sidecar(
            true,
            vec![RefRow::changed(
                EVENT_ID,
                5,
                event_payload(EVENT_ID, "newer"),
            )],
        ),
        1,
        0,
    );
    let outcome = store.apply_sidecar(
        &event_sidecar(
            false,
            vec![RefRow::changed(
                EVENT_ID,
                4,
                event_payload(EVENT_ID, "older"),
            )],
        ),
        1,
        0,
    );

    assert!(outcome.changed_keys.is_empty());
    assert_eq!(store.event(EVENT_ID).unwrap().content, "newer");
}

#[test]
fn malformed_event_row_payload_is_rejected_before_commit() {
    let mut store = RefEventStore::new();
    store.apply_sidecar(
        &event_sidecar(
            true,
            vec![RefRow::changed(
                EVENT_ID,
                1,
                event_payload(EVENT_ID, "live"),
            )],
        ),
        1,
        0,
    );

    let outcome = store.apply_sidecar(
        &event_sidecar(
            false,
            vec![RefRow::changed(EVENT_ID, 2, b"not KCEV".to_vec())],
        ),
        1,
        0,
    );

    assert!(outcome.decode_failed);
    assert_eq!(store.event(EVENT_ID).unwrap().content, "live");
}
