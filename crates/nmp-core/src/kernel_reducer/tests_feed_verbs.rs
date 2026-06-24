//! PR-3 correctness tests: B1 (idempotence gate) + B3 (E2E acceptance).
//!
//! Split from `tests.rs` to keep that file under the 500-LOC hard ceiling.
//!
//! B1 — Calling `set_active_account` twice with the same pubkey must be a
//!      no-op (idempotence gate) — it must not re-run the follow-feed
//!      reconcile / cache-serve teardown on an unchanged account.
//!
//! B3 — After `set_active_account` + active-follows declaration + kind:3
//!      ingest, `tick()` must emit a REQ whose filter carries both the followed
//!      author pubkeys AND the compiled acquisition kinds.

use super::*;
use nmp_network::role::RelayRole;
const RELAY: &str = "wss://relay.example";
const PK: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";

#[test]
fn set_active_account_twice_same_pubkey_is_a_noop() {
    // B1 — The idempotence gate: a redundant same-account call must
    // early-return `Vec::new()` and NOT re-run
    // `reconcile_follow_feed_after_identity_change`. (ADR-0057: the
    // `pre_kind3_buffer` this test previously guarded is deleted; the
    // surviving invariant is the no-op gate itself.)
    let mut r = KernelReducer::new();

    // First call: active_account was None → account changes → reconcile runs.
    let _ = r.set_active_account(PK.to_string());

    // Second call with the SAME pubkey — must be a no-op.
    let out = r.set_active_account(PK.to_string());
    assert!(
        out.is_empty(),
        "same-account set_active_account must return Vec::new() (idempotence gate)"
    );
    assert_eq!(
        r.kernel.active_account_pubkey(),
        Some(PK),
        "the active account must be unchanged after the redundant call"
    );
}

#[test]
fn kind3_ingest_followed_by_tick_emits_req_with_follows_and_kinds() {
    // B3 — E2E acceptance test: kind:3 arrives for the active viewer →
    // tick() must emit an active-follows REQ carrying both the followed author
    // pubkeys AND the compiled acquisition kinds.
    //
    // Negative proof (break-without-fix): if the active-follows declaration is
    // skipped, `follow_feed_kinds` is empty and no follow-feed interests are
    // registered -> tick() emits no REQ with follow authors.
    let viewer_keys = ::nostr::Keys::generate();
    let viewer_pk = viewer_keys.public_key().to_hex();
    let follow_pk = ::nostr::Keys::generate().public_key().to_hex();

    let mut r = KernelReducer::new();
    r.set_configured_relays(vec![(RELAY.to_string(), "both".to_string())]);
    let _ = r.handle_relay_connected(RelayRole::Content, RELAY, false);

    // Establish viewer identity and seed compiled acquisition kinds {1, 6}.
    let _ = r.set_active_account(viewer_pk.clone());
    let _ = r.declare_active_follows_feed([1u32, 6u32].into_iter().collect());

    // Seed the follow's NIP-65 relay list so the planner can resolve their
    // write relay and compile a REQ.
    r.kernel.seed_kind10002_for_test(&follow_pk, &[RELAY]);

    // Inject a kind:3 for the viewer whose follow set contains `follow_pk`.
    r.kernel
        .inject_replaceable_event(
            &"0".repeat(64),
            &viewer_pk,
            1_700_000_000,
            3,
            vec![vec!["p".to_string(), follow_pk.clone()]],
            RELAY,
            1_700_000_000_000,
        )
        .expect("inject kind:3 must succeed");

    // Drain the lifecycle trigger — the follow-feed REQ compiles here.
    let out = r.tick();

    let req_texts: Vec<&str> = out
        .iter()
        .filter(|m| m.text.contains("REQ"))
        .map(|m| m.text.as_str())
        .collect();

    assert!(
        !req_texts.is_empty(),
        "tick() after kind:3 ingest must emit at least one REQ; got empty outbound"
    );

    assert!(
        req_texts.iter().any(|t| t.contains(&follow_pk)),
        "follow-feed REQ must carry follow_pk in authors filter; got: {req_texts:?}"
    );

    assert!(
        req_texts
            .iter()
            .any(|t| t.contains("\"kinds\"") && t.contains('1') && t.contains('6')),
        "follow-feed REQ must carry compiled acquisition kinds 1 and 6; got: {req_texts:?}"
    );
}
