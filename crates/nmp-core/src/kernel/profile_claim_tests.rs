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
use crate::relay::{CONTENT_RELAY_URL, DEFAULT_VISIBLE_LIMIT, INDEXER_RELAY_URL};

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
        let _ = kernel.claim_profile(pk.clone(), format!("view-{i}"), false, false);
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
        let _ = kernel.claim_profile(hex64(&format!("{i}")), format!("view-{i}"), false, false);
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
        content_relay_reqs
            .iter()
            .map(|m| &m.relay_url)
            .collect::<Vec<_>>()
    );
}

/// Cold-start profile claims go to the indexer relay with all authors in one filter.
#[test]
fn cold_start_profile_claims_go_to_indexer_relay_only() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);

    let authors: Vec<String> = (0..5).map(|i| hex64(&format!("{i}"))).collect();
    for (i, pk) in authors.iter().enumerate() {
        let _ = kernel.claim_profile(pk.clone(), format!("view-{i}"), false, false);
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

/// Gap 2: after `claim_profile` fetched kind:0 against the indexer lane
/// (cold-start, no mailbox cached yet), the arrival of a kind:10002 for that
/// pubkey must re-queue the pubkey on `profile_requests.pending` so the next
/// `pending_profile_claim_requests` tick re-fetches kind:0 against the
/// freshly-known write set.
#[test]
fn kind10002_arrival_requeues_already_requested_profile() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let alice = hex64("a");

    // Step 1: cold-start claim — alice is queued in `pending`, then flushed
    // through `pending_profile_claim_requests`. After flush alice sits in
    // `requested` (the inflight set).
    let _ = kernel.claim_profile(alice.clone(), "view-0".to_string(), false, false);
    let _ = kernel.pending_profile_claim_requests();
    assert!(
        kernel
            .profile_requests_requested_for_test()
            .contains(&alice),
        "post-flush, alice must be in `requested`"
    );
    assert!(
        !kernel.profile_requests_pending_for_test().contains(&alice),
        "post-flush, alice must not still be in `pending`"
    );

    // Step 2: kind:10002 arrives for alice (production path: substrate parser
    // mutates the cache; this helper substitutes the same effect AND calls
    // `refresh_profile_after_mailbox` to mirror `on_mailbox_changed`).
    let outcome = kernel
        .inject_replaceable_event(
            "1111111111111111111111111111111111111111111111111111111111111111",
            &alice,
            1_000,
            10002,
            vec![vec![
                "r".to_string(),
                "wss://alice-write.example/".to_string(),
                "write".to_string(),
            ]],
            "wss://seed.relay/",
            1_000_000,
        )
        .expect("inject kind:10002 must succeed");
    assert!(matches!(
        outcome,
        crate::store::InsertOutcome::Inserted { .. } | crate::store::InsertOutcome::Replaced { .. }
    ));

    // Post-mailbox alice must be back in `pending` and out of `requested`.
    assert!(
        !kernel
            .profile_requests_requested_for_test()
            .contains(&alice),
        "after kind:10002, alice must leave `requested`"
    );
    assert!(
        kernel.profile_requests_pending_for_test().contains(&alice),
        "after kind:10002, alice must be re-queued in `pending`"
    );

    // Step 3: the next `pending_profile_claim_requests` tick must emit a
    // kind:0 REQ targeting alice's declared write relay (NOT the indexer).
    let msgs = kernel.pending_profile_claim_requests();
    let reqs: Vec<&OutboundMessage> = msgs
        .iter()
        .filter(|m| m.text.starts_with("[\"REQ\""))
        .collect();
    let relay_urls: Vec<&str> = reqs.iter().map(|m| m.relay_url.as_str()).collect();
    assert!(
        relay_urls
            .iter()
            .any(|u| *u == "wss://alice-write.example/"),
        "post-refresh kind:0 REQ must route to alice's NIP-65 write relay; got {relay_urls:?}"
    );
}

/// `refresh_profile_after_mailbox` is a no-op for a pubkey that was never
/// claimed — moving an unknown pubkey into `pending` would issue a fetch the
/// host never asked for.
#[test]
fn refresh_profile_after_mailbox_is_noop_for_unclaimed_pubkey() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let bob = hex64("b");

    kernel.refresh_profile_after_mailbox(&bob);

    assert!(
        !kernel.profile_requests_pending_for_test().contains(&bob),
        "unclaimed pubkey must not enter `pending`"
    );
    assert!(
        !kernel.profile_requests_requested_for_test().contains(&bob),
        "unclaimed pubkey must not enter `requested`"
    );
}

/// Known-NIP-65 authors: profile claims use their declared write relays, NOT the indexer.
#[test]
fn known_nip65_profile_claims_use_declared_write_relays() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    kernel.relay_connected(RelayRole::Content);
    kernel.relay_connected(RelayRole::Indexer);

    let alice = hex64("alice");
    let alice_relay = "wss://alice-write.example/";

    kernel.seed_mailbox_relay_list(&alice, vec![], vec![alice_relay.to_string()], vec![]);

    let msgs = kernel.claim_profile(alice.clone(), "view-0".to_string(), true, false);
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

// ─── Tier-0 reliability gates: name→pubkey→name flicker invariants ───────────
//
// Defect: `ChirpAvatar.onDisappear` releases a profile claim; `.task(id:)`
// re-claims on return. During 1–2 ticks (≤500ms at 4Hz) the profile is absent
// from `claimed_profiles`. The kernel's `profile_card_for()` reads from the
// **resident store** — so for a cached author, zero new REQ is needed. The
// warm-reclaim gap is a Swift-lifecycle issue, but these Rust tests establish
// the kernel invariant that must hold: a re-claim of a resident kind:0
// repopulates `claimed_profiles` on the very next tick with zero new relay REQ.
//
// `claimed_profiles` (built in `update/projections.rs`, iterating
// `self.profile_claims.keys()`) emits a card for EVERY currently-claimed
// pubkey. So "absent from `claimed_profiles`" means the pubkey's `profile_claims`
// key is gone — i.e. every consumer released. "Present" with no resident kind:0
// is still a placeholder card (key present, `display_name` null); "present" with
// a resident kind:0 carries the real `display_name`.

/// Drive one kernel tick (`make_update`) and parse the emitted JSON snapshot —
/// the exact bytes that cross the C-ABI. Mirrors `state_projection_tests::snapshot`.
fn tick_snapshot(kernel: &mut Kernel) -> serde_json::Value {
    let json = kernel.make_update_json_for_test(true);
    serde_json::from_str(&json).expect("kernel snapshot must be valid JSON")
}

/// Ingest a kind:0 (profile metadata) for `pubkey` carrying `display_name`.
/// Lands in the resident `profiles` store keyed by pubkey, exactly like the
/// production substrate kind:0 path — see
/// `state_projection_tests::profile_metadata_appears_in_snapshot_after_kind0_ingest`.
fn ingest_kind0_name(kernel: &mut Kernel, pubkey: &str, display_name: &str) {
    let event = nostr::NostrEvent {
        id: hex64("c0"),
        pubkey: pubkey.to_string(),
        created_at: 1_700_000_000,
        kind: 0,
        tags: vec![],
        content: format!(r#"{{"display_name":"{display_name}"}}"#),
        sig: String::new(),
    };
    kernel.ingest_profile(event);
}

/// Flagship Tier-0 gate. A re-claim for a *resident* kind:0 repopulates
/// `claimed_profiles` on the very next tick with **zero** new relay REQ — the
/// warm-reclaim path issues no round-trip because the card is served from the
/// resident store. This is the invariant the Swift flicker defect must not break.
#[test]
fn warm_reclaim_reemits_profile_next_tick_with_no_req() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let pubkey = hex64("a");
    let consumer_a = "view-A".to_string();

    // 1. Consumer A claims a profile on P.
    let _ = kernel.claim_profile(pubkey.clone(), consumer_a.clone(), false, false);
    // 2. A kind:0 for P arrives (name "Alice") — now resident.
    ingest_kind0_name(&mut kernel, &pubkey, "Alice");

    // 3. Next tick: P is present with the real name.
    let snap = tick_snapshot(&mut kernel);
    assert_eq!(
        snap["projections"]["claimed_profiles"][&pubkey]["display_name"].as_str(),
        Some("Alice"),
        "after kind:0 ingest the claimed profile must carry the resident name"
    );

    // 4. Release the sole claim — P drops out of `profile_claims`.
    let _ = kernel.release_profile(&pubkey, &consumer_a);

    // 5. Next tick: P is absent from `claimed_profiles` (no claim held).
    let snap = tick_snapshot(&mut kernel);
    assert!(
        snap["projections"]["claimed_profiles"]
            .get(&pubkey)
            .is_none(),
        "with no claim held, P must be absent from claimed_profiles"
    );

    // 6. Re-claim P. Because the kind:0 is still resident, the claim call itself
    //    short-circuits (profile.rs `self.profiles.contains_key` early return)
    //    and emits NO outbound REQ.
    let reclaim_msgs = kernel.claim_profile(pubkey.clone(), consumer_a.clone(), false, false);
    assert!(
        req_texts(&reclaim_msgs).is_empty(),
        "warm re-claim of a resident profile must emit zero REQ; got {:#?}",
        req_texts(&reclaim_msgs)
    );

    // 7. The pending-flush tick must also emit zero REQ mentioning P — P is not
    //    in `pending` after a resident re-claim, so no round-trip is queued.
    let flush_msgs = kernel.pending_profile_claim_requests();
    let p_reqs: Vec<&str> = req_texts(&flush_msgs)
        .into_iter()
        .filter(|t| t.contains(pubkey.as_str()))
        .collect();
    assert!(
        p_reqs.is_empty(),
        "no outbound REQ may mention P after a resident re-claim; got {p_reqs:#?}"
    );

    // 8. Very next tick: P is repopulated from cache with the resident name — no
    //    flicker, no round-trip.
    let snap = tick_snapshot(&mut kernel);
    assert_eq!(
        snap["projections"]["claimed_profiles"][&pubkey]["display_name"].as_str(),
        Some("Alice"),
        "warm re-claim must repopulate the resident name on the very next tick"
    );
}

/// Claim-lifecycle contract: a profile appears in `claimed_profiles` exactly
/// while a claim is held — present after claim, absent after the last release.
#[test]
fn claimed_profiles_present_iff_claim_held() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let pubkey = hex64("b");
    let consumer_a = "view-A".to_string();

    // 1. No claim → P absent.
    let snap = tick_snapshot(&mut kernel);
    assert!(
        snap["projections"]["claimed_profiles"]
            .get(&pubkey)
            .is_none(),
        "with no claim, P must be absent from claimed_profiles"
    );

    // 2. Claim P → present on the next tick (placeholder card; no kind:0 yet).
    let _ = kernel.claim_profile(pubkey.clone(), consumer_a.clone(), false, false);
    let snap = tick_snapshot(&mut kernel);
    assert!(
        snap["projections"]["claimed_profiles"]
            .get(&pubkey)
            .is_some(),
        "while a claim is held, P must be present in claimed_profiles"
    );

    // 3. Release P → absent on the next tick.
    let _ = kernel.release_profile(&pubkey, &consumer_a);
    let snap = tick_snapshot(&mut kernel);
    assert!(
        snap["projections"]["claimed_profiles"]
            .get(&pubkey)
            .is_none(),
        "after the last release, P must be absent from claimed_profiles"
    );
}

/// Multi-consumer reference-counting: when one view's `generatedConsumerID`
/// releases while another still holds the claim, the card must stay resident —
/// it only drops once the final consumer releases.
#[test]
fn multi_consumer_release_does_not_drop_resident_profile() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let pubkey = hex64("c");
    let consumer_a = "view-A".to_string();
    let consumer_b = "view-B".to_string();

    // 1. Two consumers claim P; a kind:0 arrives.
    let _ = kernel.claim_profile(pubkey.clone(), consumer_a.clone(), false, false);
    let _ = kernel.claim_profile(pubkey.clone(), consumer_b.clone(), false, false);
    ingest_kind0_name(&mut kernel, &pubkey, "Carol");

    // 2. Present with the resident name.
    let snap = tick_snapshot(&mut kernel);
    assert_eq!(
        snap["projections"]["claimed_profiles"][&pubkey]["display_name"].as_str(),
        Some("Carol"),
        "with both consumers holding, P must carry the resident name"
    );

    // 3. Consumer A releases — B still holds.
    let _ = kernel.release_profile(&pubkey, &consumer_a);

    // 4. Still present (B is a live consumer).
    let snap = tick_snapshot(&mut kernel);
    assert_eq!(
        snap["projections"]["claimed_profiles"][&pubkey]["display_name"].as_str(),
        Some("Carol"),
        "a single consumer release must NOT drop a still-claimed resident profile"
    );

    // 5. Consumer B releases — last claim gone.
    let _ = kernel.release_profile(&pubkey, &consumer_b);

    // 6. Now absent (no consumer holds the claim).
    let snap = tick_snapshot(&mut kernel);
    assert!(
        snap["projections"]["claimed_profiles"]
            .get(&pubkey)
            .is_none(),
        "after the final consumer releases, P must be absent from claimed_profiles"
    );
}
