//! Tests for the M2 registry-backed profile-claim path.
//!
//! `claim_profile` registers a kind:0 `LogicalInterest` through the
//! `InterestRegistry`; the planner emits the wire REQ on the next
//! `drain_lifecycle_outbound`. These tests assert the migrated behaviour:
//!
//! * a claim for an author with a cached kind:10002 routes the kind:0 to that
//!   author's own write relays (warm outbox);
//! * a cold-start claim (uncached mailbox) reaches the indexer relay AND
//!   triggers the D3 kind:10002 probe; when the kind:10002 lands, the next
//!   recompile re-routes the kind:0 to the author's write relays
//!   (`Nip65Arrived`, replacing the deleted `refresh_profile_after_mailbox`);
//! * hundreds of single-author claims coalesce into ONE batched kind:0 REQ
//!   (`limit: None` author-union);
//! * `Live` claims register a Tailing kind:0 sub; mixed `CacheOk` + `Live`
//!   on one pubkey resolve to Tailing (Live wins);
//! * multi-consumer refcount keeps one deduped interest live until the last
//!   consumer releases;
//! * the F-TTL `force` re-verify of a cached profile still fires;
//! * the `claimed_profiles` projection (driven off `profile_claims`,
//!   unchanged) stays correct, including the warm-reclaim zero-REQ invariant.

use super::*;
use crate::kernel::ProfileLiveness;
use crate::relay::{DEFAULT_VISIBLE_LIMIT, INDEXER_RELAY_URL};

fn hex64(prefix: &str) -> String {
    format!("{prefix:0<64}").chars().take(64).collect()
}

/// Drain the planner and return only the REQ `OutboundMessage`s.
fn drain_reqs(kernel: &mut Kernel) -> Vec<OutboundMessage> {
    kernel
        .drain_lifecycle_outbound()
        .into_iter()
        .filter(|m| m.text.starts_with("[\"REQ\""))
        .collect()
}

/// Relay URLs of REQ frames whose filter targets `pubkey` with kinds == [0].
fn kind0_req_relays_for(reqs: &[OutboundMessage], pubkey: &str) -> Vec<String> {
    reqs.iter()
        .filter_map(|m| {
            let v: serde_json::Value = serde_json::from_str(&m.text).ok()?;
            let arr = v.as_array()?;
            if arr.first()?.as_str()? != "REQ" {
                return None;
            }
            let filter = arr.get(2)?;
            let kinds = filter.get("kinds")?.as_array()?;
            let is_kind0 = kinds.len() == 1 && kinds[0].as_u64() == Some(0);
            let authors = filter.get("authors")?.as_array()?;
            let has_author = authors.iter().any(|a| a.as_str() == Some(pubkey));
            (is_kind0 && has_author).then(|| m.relay_url.clone())
        })
        .collect()
}

/// True iff `reqs` contains a kind:10002 probe REQ whose authors include `pubkey`.
fn has_10002_probe_for(reqs: &[OutboundMessage], pubkey: &str) -> bool {
    reqs.iter().any(|m| {
        let v: serde_json::Value = serde_json::from_str(&m.text).unwrap_or(serde_json::Value::Null);
        let Some(filter) = v.get(2) else { return false };
        let is_10002 = filter
            .get("kinds")
            .and_then(|k| k.as_array())
            .map(|k| k.iter().any(|x| x.as_u64() == Some(10002)))
            .unwrap_or(false);
        let has_author = filter
            .get("authors")
            .and_then(|a| a.as_array())
            .map(|a| a.iter().any(|x| x.as_str() == Some(pubkey)))
            .unwrap_or(false);
        is_10002 && has_author
    })
}

// ─── (a) cached kind:10002 → kind:0 routes to author's own write relays ──────

#[test]
fn cached_nip65_profile_claim_routes_kind0_to_author_write_relays() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    kernel.relay_connected(RelayRole::Content);
    kernel.relay_connected(RelayRole::Indexer);

    let alice = hex64("a11ce");
    let alice_relay = "wss://alice-write.example";
    kernel.seed_mailbox_relay_list(&alice, vec![], vec![alice_relay.to_string()], vec![]);

    let _ = kernel.claim_profile(
        alice.clone(),
        "view-0".to_string(),
        true,
        false,
        ProfileLiveness::CacheOk,
    );

    let reqs = drain_reqs(&mut kernel);
    let relays = kind0_req_relays_for(&reqs, &alice);
    assert!(
        relays.iter().any(|u| u == alice_relay),
        "warm kind:0 claim must route to the author's NIP-65 write relay {alice_relay}; got {relays:?}"
    );
}

// ─── (b) cold-start claim → indexer + D3 probe; 10002 arrival → re-route ─────

#[test]
fn cold_start_claim_reaches_indexer_and_probes_then_reroutes_on_nip65() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    kernel.relay_connected(RelayRole::Content);
    kernel.relay_connected(RelayRole::Indexer);

    let stranger = hex64("57a"); // not in any follow set, no cached mailbox

    let _ = kernel.claim_profile(
        stranger.clone(),
        "view-0".to_string(),
        true,
        false,
        ProfileLiveness::CacheOk,
    );

    let reqs = drain_reqs(&mut kernel);

    // At cold start (uncached mailbox) the kind:0 routes to the app/content
    // relay fallback (best-effort immediate fetch — owner decision #1: app
    // relays SHOULD receive kind:0). It must reach SOME relay (never blank).
    let kind0_relays = kind0_req_relays_for(&reqs, &stranger);
    assert!(
        !kind0_relays.is_empty(),
        "cold-start kind:0 claim must reach an app/content fallback relay; got none"
    );

    // D3: a kind:10002 discovery probe for the stranger is emitted to the
    // indexer (the acquisition half that makes Lane 1 reachable).
    assert!(
        kernel.probed_mailboxes_for_test().contains(&stranger),
        "cold-start claim must mark the stranger probed (D3 kind:10002 probe)"
    );
    assert!(
        has_10002_probe_for(&reqs, &stranger),
        "cold-start claim must emit a kind:10002 probe REQ for the stranger"
    );
    let probe_reaches_indexer = reqs.iter().any(|m| {
        m.relay_url == INDEXER_RELAY_URL && m.text.contains("10002")
    });
    assert!(
        probe_reaches_indexer,
        "the kind:10002 probe must reach the indexer relay {INDEXER_RELAY_URL}"
    );

    // The stranger publishes a kind:10002 → mailbox cache updates, Nip65Arrived
    // fires, and the next recompile re-routes the kind:0 to their write relay.
    let stranger_relay = "wss://stranger-write.example";
    let _ = kernel
        .inject_replaceable_event(
            &hex64("e0e0"),
            &stranger,
            1_000,
            10002,
            vec![vec![
                "r".to_string(),
                stranger_relay.to_string(),
                "write".to_string(),
            ]],
            "wss://seed.relay/",
            1_000_000,
        )
        .expect("inject kind:10002 must succeed");

    let reqs2 = drain_reqs(&mut kernel);
    let relays2 = kind0_req_relays_for(&reqs2, &stranger);
    assert!(
        relays2.iter().any(|u| u == stranger_relay),
        "after the stranger's kind:10002 lands, the kind:0 must re-route to their \
         write relay {stranger_relay}; got {relays2:?}"
    );
}

// ─── (b') retry-on-miss: indexer reconnect re-probes a still-uncached author ─

#[test]
fn indexer_reconnect_reprobes_uncached_author() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    kernel.relay_connected_url(RelayRole::Content, crate::relay::FALLBACK_CONTENT_RELAY);
    kernel.relay_connected_url(RelayRole::Indexer, INDEXER_RELAY_URL);

    let stranger = hex64("9a9");
    let _ = kernel.claim_profile(
        stranger.clone(),
        "view-0".to_string(),
        true,
        false,
        ProfileLiveness::CacheOk,
    );
    let _ = kernel.drain_lifecycle_outbound();
    assert!(
        kernel.probed_mailboxes_for_test().contains(&stranger),
        "stranger must be marked probed after the first drain"
    );

    // Indexer socket reconnects (the author's 10002 never arrived — empty EOSE,
    // or the indexer was down). The probed set must be cleared so the next
    // recompile re-probes the still-uncached stranger.
    kernel.relay_connected_url(RelayRole::Indexer, INDEXER_RELAY_URL);
    assert!(
        !kernel.probed_mailboxes_for_test().contains(&stranger),
        "indexer reconnect must clear the probed mark (retry-on-miss)"
    );

    let reqs = drain_reqs(&mut kernel);
    assert!(
        has_10002_probe_for(&reqs, &stranger),
        "after indexer reconnect the still-uncached stranger must be re-probed"
    );
}

// ─── (d) avatar feed: N CacheOk claims coalesce into ONE batched kind:0 REQ ──

#[test]
fn many_cache_ok_claims_coalesce_into_one_batched_kind0_req() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    kernel.relay_connected(RelayRole::Content);
    kernel.relay_connected(RelayRole::Indexer);

    let authors: Vec<String> = (0..12).map(|i| hex64(&format!("a{i}"))).collect();
    for (i, pk) in authors.iter().enumerate() {
        let _ = kernel.claim_profile(
            pk.clone(),
            format!("avatar-{i}"),
            false,
            false,
            ProfileLiveness::CacheOk,
        );
    }

    let reqs = drain_reqs(&mut kernel);

    // Count distinct kind:0 REQ frames PER RELAY. With `limit: None` the merge
    // lattice unions same-shape authors, so the dozen single-author claims
    // collapse onto ONE kind:0 REQ per relay carrying all authors — not 12.
    let mut kind0_frames_by_relay: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    for m in &reqs {
        let v: serde_json::Value = serde_json::from_str(&m.text).unwrap_or(serde_json::Value::Null);
        let is_kind0 = v
            .get(2)
            .and_then(|f| f.get("kinds"))
            .and_then(|k| k.as_array())
            .map(|k| k.len() == 1 && k[0].as_u64() == Some(0))
            .unwrap_or(false);
        if is_kind0 {
            *kind0_frames_by_relay.entry(m.relay_url.clone()).or_default() += 1;
        }
    }
    assert!(
        !kind0_frames_by_relay.is_empty(),
        "a kind:0 REQ must be emitted for the avatar feed"
    );
    for (relay, count) in &kind0_frames_by_relay {
        assert_eq!(
            *count, 1,
            "a dozen kind:0 claims must coalesce into ONE batched REQ per relay \
             (no storm); relay {relay} got {count} frames"
        );
    }
    // The single batched frame carries every claimed author.
    let batched = reqs
        .iter()
        .find(|m| {
            let v: serde_json::Value =
                serde_json::from_str(&m.text).unwrap_or(serde_json::Value::Null);
            v.get(2)
                .and_then(|f| f.get("kinds"))
                .and_then(|k| k.as_array())
                .map(|k| k.len() == 1 && k[0].as_u64() == Some(0))
                .unwrap_or(false)
        })
        .expect("a kind:0 frame exists");
    for pk in &authors {
        assert!(
            batched.text.contains(pk.as_str()),
            "the batched kind:0 REQ must contain author {pk}"
        );
    }
}

// ─── (c-liveness) Live registers Tailing; mixed CacheOk + Live → Tailing ─────

#[test]
fn live_claim_registers_tailing_interest() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let alice = hex64("11e");
    let _ = kernel.claim_profile(
        alice.clone(),
        "profile-view".to_string(),
        false,
        false,
        ProfileLiveness::Live,
    );
    assert_eq!(
        kernel.profile_claim_interest_lifecycle_for_test(&alice),
        Some(true),
        "a Live claim must register a Tailing kind:0 interest"
    );
}

#[test]
fn cache_ok_claim_registers_oneshot_interest() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let alice = hex64("0a0");
    let _ = kernel.claim_profile(
        alice.clone(),
        "avatar".to_string(),
        false,
        false,
        ProfileLiveness::CacheOk,
    );
    assert_eq!(
        kernel.profile_claim_interest_lifecycle_for_test(&alice),
        Some(false),
        "a CacheOk claim must register a OneShot kind:0 interest"
    );
}

#[test]
fn mixed_liveness_resolves_to_tailing_live_wins() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let alice = hex64("a11");

    // A feed avatar claims CacheOk first.
    let _ = kernel.claim_profile(
        alice.clone(),
        "avatar".to_string(),
        false,
        false,
        ProfileLiveness::CacheOk,
    );
    assert_eq!(
        kernel.profile_claim_interest_lifecycle_for_test(&alice),
        Some(false),
        "first CacheOk claim → OneShot"
    );

    // The profile screen then claims Live for the same pubkey → upgrade.
    let _ = kernel.claim_profile(
        alice.clone(),
        "profile-view".to_string(),
        false,
        false,
        ProfileLiveness::Live,
    );
    assert_eq!(
        kernel.profile_claim_interest_lifecycle_for_test(&alice),
        Some(true),
        "a Live claim must upgrade the shared slot to Tailing (Live wins)"
    );

    // A later CacheOk claim must NOT downgrade it while a Live claim is held.
    let _ = kernel.claim_profile(
        alice.clone(),
        "avatar-2".to_string(),
        false,
        false,
        ProfileLiveness::CacheOk,
    );
    assert_eq!(
        kernel.profile_claim_interest_lifecycle_for_test(&alice),
        Some(true),
        "a later CacheOk claim must not downgrade a Tailing slot"
    );
}

// ─── (b-refcount) multi-consumer dedup: one interest, refcounted ─────────────

#[test]
fn multi_consumer_claim_dedups_to_one_interest_until_last_release() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let alice = hex64("c0c0");

    let _ = kernel.claim_profile(
        alice.clone(),
        "view-A".to_string(),
        false,
        false,
        ProfileLiveness::CacheOk,
    );
    let _ = kernel.claim_profile(
        alice.clone(),
        "view-B".to_string(),
        false,
        false,
        ProfileLiveness::CacheOk,
    );

    // Two consumers → exactly one kind:0 claim interest.
    let kind0_claim_count = kernel
        .lifecycle_mut()
        .registry()
        .iter_active()
        .iter()
        .filter(|i| {
            i.shape.kinds.len() == 1
                && i.shape.kinds.contains(&0)
                && i.shape.authors.contains(&alice)
        })
        .count();
    assert_eq!(
        kind0_claim_count, 1,
        "two consumers of one pubkey must dedup to ONE kind:0 interest"
    );

    // First consumer releases — interest stays (B still holds).
    let _ = kernel.release_profile(&alice, "view-A");
    assert!(
        kernel.profile_claim_interest_registered_for_test(&alice),
        "interest must survive while a second consumer holds it"
    );

    // Last consumer releases — interest is dropped.
    let _ = kernel.release_profile(&alice, "view-B");
    assert!(
        !kernel.profile_claim_interest_registered_for_test(&alice),
        "interest must be dropped once the last consumer releases"
    );
}

// ─── (c) F-TTL force re-verify of a cached profile still fires ───────────────

#[test]
fn force_reverify_of_cached_profile_enqueues_reverify() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let alice = hex64("f0f0");

    // Seed a resident kind:0 so the cached branch runs.
    let event = nostr::NostrEvent {
        id: hex64("c0"),
        pubkey: alice.clone(),
        created_at: 1_700_000_000,
        kind: 0,
        tags: vec![],
        content: r#"{"display_name":"Alice"}"#.to_string(),
        sig: String::new(),
    };
    kernel.ingest_profile(event);

    let before = kernel.pending_reverify_len();
    // force = true → unconditional F-TTL re-verify enqueue.
    let _ = kernel.claim_profile(
        alice.clone(),
        "profile-view".to_string(),
        false,
        true,
        ProfileLiveness::Live,
    );
    let after = kernel.pending_reverify_len();
    assert!(
        after > before,
        "force=true on a cached profile must enqueue an F-TTL re-verify"
    );
}

// ─── nprofile hint: kind:0 routes to embedded relay with no indexer 10002 ────

#[test]
fn nprofile_hint_routes_kind0_to_embedded_relay() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    kernel.relay_connected(RelayRole::Content);
    kernel.relay_connected(RelayRole::Indexer);

    let stranger = hex64("e3b");
    let hint_relay = "wss://hint.example";

    // Claim originating from an nprofile carrying a relay TLV — no cached
    // mailbox, no indexer kind:10002 ever arrives, but the embedded hint relay
    // must still receive the kind:0.
    let _ = kernel.claim_profile_with_hints(
        stranger.clone(),
        "view-0".to_string(),
        false,
        ProfileLiveness::CacheOk,
        vec![hint_relay.to_string()],
    );

    let reqs = drain_reqs(&mut kernel);
    let relays = kind0_req_relays_for(&reqs, &stranger);
    assert!(
        relays.iter().any(|u| u == hint_relay),
        "an nprofile-hinted claim must route the kind:0 to the embedded relay {hint_relay}; \
         got {relays:?}"
    );
}
