//! Tier-0 `claimed_profiles` projection invariants for the registry-backed
//! profile-claim path.
//!
//! The `claimed_profiles` snapshot projection is driven off `profile_claims`
//! (the `HashMap<pubkey, BTreeSet<consumer_id>>` refcount), which the M2
//! registry migration deliberately RETAINS as the projection source-of-truth.
//! These tests pin that the projection still reflects claim/release lifecycle
//! exactly, and the flagship warm-reclaim zero-REQ invariant (a re-claim of a
//! resident kind:0 serves from the store with no new network REQ).

use super::*;
use crate::kernel::ProfileLiveness;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

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

fn tick_snapshot(kernel: &mut Kernel) -> serde_json::Value {
    let json = kernel.make_update_json_for_test(true);
    serde_json::from_str(&json).expect("kernel snapshot must be valid JSON")
}

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

/// Flagship Tier-0 gate: a re-claim of a resident kind:0 repopulates
/// `claimed_profiles` with NO new kind:0 REQ for that pubkey.
#[test]
fn warm_reclaim_reemits_profile_next_tick_with_no_req() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let pubkey = hex64("a");
    let consumer_a = "view-A".to_string();

    let _ = kernel.claim_profile(
        pubkey.clone(),
        consumer_a.clone(),
        false,
        false,
        ProfileLiveness::CacheOk,
    );
    ingest_kind0_name(&mut kernel, &pubkey, "Alice");

    let snap = tick_snapshot(&mut kernel);
    assert_eq!(
        snap["projections"]["claimed_profiles"][&pubkey]["display_name"].as_str(),
        Some("Alice"),
        "after kind:0 ingest the claimed profile must carry the resident name"
    );

    let _ = kernel.release_profile(&pubkey, &consumer_a);
    let snap = tick_snapshot(&mut kernel);
    assert!(
        snap["projections"]["claimed_profiles"]
            .get(&pubkey)
            .is_none(),
        "with no claim held, P must be absent from claimed_profiles"
    );

    // Drain any CLOSE diff so the next drain reflects only the re-claim.
    let _ = kernel.drain_lifecycle_outbound();

    // Re-claim a resident profile (CacheOk). No kind:0 REQ for P should be
    // emitted — the resident store serves the card.
    let _ = kernel.claim_profile(
        pubkey.clone(),
        consumer_a.clone(),
        false,
        false,
        ProfileLiveness::CacheOk,
    );
    let reqs = drain_reqs(&mut kernel);
    let p_reqs = kind0_req_relays_for(&reqs, &pubkey);
    assert!(
        p_reqs.is_empty(),
        "warm re-claim of a resident profile must emit zero kind:0 REQ for P; got {p_reqs:?}"
    );

    let snap = tick_snapshot(&mut kernel);
    assert_eq!(
        snap["projections"]["claimed_profiles"][&pubkey]["display_name"].as_str(),
        Some("Alice"),
        "warm re-claim must repopulate the resident name on the next tick"
    );
}

#[test]
fn claimed_profiles_present_iff_claim_held() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let pubkey = hex64("b");
    let consumer_a = "view-A".to_string();

    let snap = tick_snapshot(&mut kernel);
    assert!(
        snap["projections"]["claimed_profiles"]
            .get(&pubkey)
            .is_none(),
        "with no claim, P must be absent from claimed_profiles"
    );

    let _ = kernel.claim_profile(
        pubkey.clone(),
        consumer_a.clone(),
        false,
        false,
        ProfileLiveness::CacheOk,
    );
    let snap = tick_snapshot(&mut kernel);
    assert!(
        snap["projections"]["claimed_profiles"]
            .get(&pubkey)
            .is_some(),
        "while a claim is held, P must be present in claimed_profiles"
    );

    let _ = kernel.release_profile(&pubkey, &consumer_a);
    let snap = tick_snapshot(&mut kernel);
    assert!(
        snap["projections"]["claimed_profiles"]
            .get(&pubkey)
            .is_none(),
        "after the last release, P must be absent from claimed_profiles"
    );
}

#[test]
fn multi_consumer_release_does_not_drop_resident_profile() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let pubkey = hex64("c");
    let consumer_a = "view-A".to_string();
    let consumer_b = "view-B".to_string();

    let _ = kernel.claim_profile(
        pubkey.clone(),
        consumer_a.clone(),
        false,
        false,
        ProfileLiveness::CacheOk,
    );
    let _ = kernel.claim_profile(
        pubkey.clone(),
        consumer_b.clone(),
        false,
        false,
        ProfileLiveness::CacheOk,
    );
    ingest_kind0_name(&mut kernel, &pubkey, "Carol");

    let snap = tick_snapshot(&mut kernel);
    assert_eq!(
        snap["projections"]["claimed_profiles"][&pubkey]["display_name"].as_str(),
        Some("Carol"),
        "with both consumers holding, P must carry the resident name"
    );

    let _ = kernel.release_profile(&pubkey, &consumer_a);
    let snap = tick_snapshot(&mut kernel);
    assert_eq!(
        snap["projections"]["claimed_profiles"][&pubkey]["display_name"].as_str(),
        Some("Carol"),
        "a single consumer release must NOT drop a still-claimed resident profile"
    );

    let _ = kernel.release_profile(&pubkey, &consumer_b);
    let snap = tick_snapshot(&mut kernel);
    assert!(
        snap["projections"]["claimed_profiles"]
            .get(&pubkey)
            .is_none(),
        "after the final consumer releases, P must be absent from claimed_profiles"
    );
}
