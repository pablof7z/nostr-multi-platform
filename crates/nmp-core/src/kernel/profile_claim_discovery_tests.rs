//! Discovery / kind:10002-probe / indexer-reconnect tests for the M2
//! registry-backed profile-claim path.
//!
//! Split out of `profile_claim_tests.rs` (file-size gate, 500 LOC hard
//! ceiling). These exercise the implicit-discovery half of `claim_profile`:
//! a cold-start (uncached mailbox) claim must reach the indexer AND emit the D3
//! kind:10002 probe; a genuine indexer reconnect (down → up) re-arms the probe
//! set; a redundant/startup connect must NOT (the #1436 regression). The
//! REQ-inspection helpers are shared via `profile_claim_test_support`.

use super::profile_claim_test_support::{
    drain_reqs, has_10002_probe_for, hex64, kind0_req_relays_for,
};
use super::*;
use crate::kernel::refs::{ProfileShape, RefLiveness, RefNamespace, RefShape};
use crate::relay::{DEFAULT_VISIBLE_LIMIT, INDEXER_RELAY_URL};

// ─── (b) cold-start claim → indexer + D3 probe; 10002 arrival → re-route ─────

#[test]
fn cold_start_claim_reaches_indexer_and_probes_then_reroutes_on_nip65() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    kernel.relay_connected(RelayRole::Content);
    kernel.relay_connected(RelayRole::Indexer);

    let stranger = hex64("57a"); // not in any follow set, no cached mailbox

    let _ = kernel.resolve_ref(
            RefNamespace::Profile,
            stranger.clone(),
            "view-0".to_string(),
            RefShape::Profile(ProfileShape::Card),
            RefLiveness::CacheOk.into(),
            false,
            Vec::new(),
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
    let probe_reaches_indexer = reqs
        .iter()
        .any(|m| m.relay_url == INDEXER_RELAY_URL && m.text.contains("10002"));
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
    let _ = kernel.resolve_ref(
            RefNamespace::Profile,
            stranger.clone(),
            "view-0".to_string(),
            RefShape::Profile(ProfileShape::Card),
            RefLiveness::CacheOk.into(),
            false,
            Vec::new(),
        );
    let _ = kernel.drain_lifecycle_outbound();
    assert!(
        kernel.probed_mailboxes_for_test().contains(&stranger),
        "stranger must be marked probed after the first drain"
    );

    // The indexer socket goes DOWN (the author's 10002 never arrived — empty
    // EOSE, or the indexer dropped) and then comes back. A *genuine* reconnect
    // (down → up) must clear the probed set so the next recompile re-probes the
    // still-uncached stranger. The down-state is what distinguishes this from a
    // redundant/startup connect (see `redundant_indexer_connect_does_not_churn`).
    kernel.relay_closed(RelayRole::Indexer, INDEXER_RELAY_URL);
    kernel.relay_connected_url(RelayRole::Indexer, INDEXER_RELAY_URL);
    assert!(
        !kernel.probed_mailboxes_for_test().contains(&stranger),
        "genuine indexer reconnect (down → up) must clear the probed mark (retry-on-miss)"
    );

    let reqs = drain_reqs(&mut kernel);
    assert!(
        has_10002_probe_for(&reqs, &stranger),
        "after a genuine indexer reconnect the still-uncached stranger must be re-probed"
    );
}

// ─── (b''') B3: a single indexer flap among live siblings must NOT re-arm ─────

/// B3 (Workstream B acquisition-one-door): the mailbox-probe re-arm is gated on
/// a *full-lane* outage recovery, not a per-socket reconnect. With two indexers
/// connected, one flapping (close → reconnect) while the other stays live must
/// NOT clear the probed set — the lane never went down. This is the
/// per-reconnect churn the old per-socket `indexer_socket_was_down` gate caused
/// (and the reverted naive "clear on every reconnect" approach); the lane epoch
/// fixes it. A subsequent FULL outage (both down) then recovery DOES re-arm.
#[test]
fn single_indexer_flap_among_live_siblings_does_not_rearm() {
    const INDEXER_A: &str = "wss://indexer-a.example";
    const INDEXER_B: &str = "wss://indexer-b.example";

    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    kernel.relay_connected_url(RelayRole::Content, crate::relay::FALLBACK_CONTENT_RELAY);
    kernel.relay_connected_url(RelayRole::Indexer, INDEXER_A);
    kernel.relay_connected_url(RelayRole::Indexer, INDEXER_B);

    let stranger = hex64("5ed");
    let _ = kernel.resolve_ref(
            RefNamespace::Profile,
            stranger.clone(),
            "view-0".to_string(),
            RefShape::Profile(ProfileShape::Card),
            RefLiveness::CacheOk.into(),
            false,
            Vec::new(),
        );
    let _ = kernel.drain_lifecycle_outbound();
    assert!(
        kernel.probed_mailboxes_for_test().contains(&stranger),
        "stranger must be marked probed after the first drain"
    );

    // Indexer A flaps (closes then reconnects) while B stays connected the whole
    // time. The lane never fully went down → no re-arm, no churn.
    kernel.relay_closed(RelayRole::Indexer, INDEXER_A);
    assert!(
        kernel.probed_mailboxes_for_test().contains(&stranger),
        "closing one of two indexers must NOT clear the probed set (lane still up via B)"
    );
    kernel.relay_connected_url(RelayRole::Indexer, INDEXER_A);
    assert!(
        kernel.probed_mailboxes_for_test().contains(&stranger),
        "a sibling-still-live indexer reconnect must NOT re-arm the probed set (B3 anti-churn)"
    );
    let after = kernel.drain_lifecycle_outbound();
    assert!(
        !after.iter().any(|m| m.text.contains("10002")),
        "a sibling-still-live reconnect must NOT re-emit a kind:10002 probe; got {after:?}"
    );

    // Now a GENUINE full outage: BOTH indexers down, then one recovers. The lane
    // went down → up, so the recovery re-arms and re-probes the still-uncached
    // stranger.
    kernel.relay_closed(RelayRole::Indexer, INDEXER_A);
    kernel.relay_closed(RelayRole::Indexer, INDEXER_B);
    kernel.relay_connected_url(RelayRole::Indexer, INDEXER_A);
    assert!(
        !kernel.probed_mailboxes_for_test().contains(&stranger),
        "a full-lane outage recovery (both down → one up) MUST re-arm the probed set"
    );
    let reqs = drain_reqs(&mut kernel);
    assert!(
        has_10002_probe_for(&reqs, &stranger),
        "after a full-lane outage recovery the still-uncached stranger must be re-probed"
    );
}

// ─── (b'') regression #1436: redundant/startup indexer connect must NOT churn ─

/// The PR that introduced the registry chokepoint cleared the probed set and
/// forced an `IndexerSetChanged` recompile on EVERY indexer socket-up event —
/// including the first/normal connect during startup and redundant duplicate
/// connects of an already-live socket. That broke the web feed: at initial
/// load the only relay (indexer role, uncached fixture authors) would clear the
/// probe set and re-plan the feed sub mid-load on every socket churn, starving
/// the single-threaded wasm UI so notes never painted.
///
/// A connect that is NOT preceded by a down-state (failed/closed) must be a
/// no-op for the discovery probe set: idempotent, no re-probe storm, no forced
/// recompile that could CLOSE+re-REQ the live content subscription.
#[test]
fn redundant_indexer_connect_does_not_churn() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    kernel.relay_connected_url(RelayRole::Content, crate::relay::FALLBACK_CONTENT_RELAY);
    kernel.relay_connected_url(RelayRole::Indexer, INDEXER_RELAY_URL);

    let stranger = hex64("b0b");
    let _ = kernel.resolve_ref(
            RefNamespace::Profile,
            stranger.clone(),
            "view-0".to_string(),
            RefShape::Profile(ProfileShape::Card),
            RefLiveness::CacheOk.into(),
            false,
            Vec::new(),
        );
    // First drain settles the plan and emits the kind:10002 probe once.
    let first = drain_reqs(&mut kernel);
    assert!(
        has_10002_probe_for(&first, &stranger),
        "first drain must emit the kind:10002 probe for the uncached stranger"
    );
    assert!(
        kernel.probed_mailboxes_for_test().contains(&stranger),
        "stranger must be marked probed after the first drain"
    );

    // Now a *redundant* indexer connect — the same already-live socket signals
    // "connected" again (startup churn / duplicate socket-up). NO `relay_closed`
    // or `relay_failed` preceded it, so it is NOT a genuine reconnect.
    kernel.relay_connected_url(RelayRole::Indexer, INDEXER_RELAY_URL);

    // (a) idempotent: the probed mark must SURVIVE — clearing it here is the bug.
    assert!(
        kernel.probed_mailboxes_for_test().contains(&stranger),
        "a redundant indexer connect must NOT clear the probed set (regression #1436)"
    );

    // (b) no forced recompile / no re-probe storm: the next drain settles to an
    // empty diff (no triggers were enqueued by the redundant connect). In
    // particular it must not re-emit the kind:10002 probe nor CLOSE any live sub.
    let after = kernel.drain_lifecycle_outbound();
    assert!(
        !after.iter().any(|m| m.text.contains("10002")),
        "a redundant indexer connect must NOT re-emit a kind:10002 probe; got {after:?}"
    );
    assert!(
        !after.iter().any(|m| m.text.starts_with("[\"CLOSE\"")),
        "a redundant indexer connect must NOT CLOSE any live subscription; got {after:?}"
    );

    // (c) repeated redundant connects stay idempotent (no oscillation).
    for _ in 0..3 {
        kernel.relay_connected_url(RelayRole::Indexer, INDEXER_RELAY_URL);
        let frames = kernel.drain_lifecycle_outbound();
        assert!(
            !frames.iter().any(|m| m.text.contains("10002")),
            "repeated redundant indexer connects must not oscillate into re-probes; got {frames:?}"
        );
    }
    assert!(
        kernel.probed_mailboxes_for_test().contains(&stranger),
        "probed mark must remain stable across repeated redundant connects"
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
        let _ = kernel.resolve_ref(
            RefNamespace::Profile,
            pk.clone(),
            format!("avatar-{i}"),
            RefShape::Profile(ProfileShape::Card),
            RefLiveness::CacheOk.into(),
            false,
            Vec::new(),
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
    let _ = kernel.resolve_ref(
        RefNamespace::Profile,
        stranger.clone(),
        "view-0".to_string(),
        RefShape::Profile(ProfileShape::Card),
        RefLiveness::CacheOk.into(),
        false,
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
