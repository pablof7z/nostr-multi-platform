//! ADR-0063 (#1671 Lane B) — profile-resolver + shared lifecycle/dedup/rev unit
//! tests for the kernel-owned `RefResolver` primitive.
//!
//! Event-resolver tests live in `tests_refs_event.rs` (Lane D merge target).

use super::nostr::NostrEvent;
use super::refs::{
    EventShape, ProfileShape, RefLiveness, RefNamespace, RefResolver, RefShape,
};
use super::refs::{EventNs, ProfileNs};
use super::*;
use crate::nip19::{encode_nevent, NeventData};
use crate::relay::DEFAULT_VISIBLE_LIMIT;

fn hex64(prefix: &str) -> String {
    format!("{prefix:0<64}").chars().take(64).collect()
}

fn nevent_uri(event_id: &str) -> String {
    let bech = encode_nevent(&NeventData {
        event_id: event_id.to_string(),
        relays: vec![],
        author: None,
        kind: Some(1),
    })
    .expect("encode_nevent");
    format!("nostr:{bech}")
}

fn inject_kind0(kernel: &mut Kernel, pubkey: &str, display_name: &str) {
    let content = serde_json::json!({
        "display_name": display_name,
        "picture": "https://example.com/a.png",
    })
    .to_string();
    kernel.inject_profile(NostrEvent {
        id: "0".repeat(64),
        pubkey: pubkey.to_string(),
        created_at: 1_700_000_000,
        kind: 0,
        tags: Vec::new(),
        content,
        sig: String::new(),
    });
}

fn profile_card(kernel: &mut Kernel, pk: &str, consumer: &str, liveness: RefLiveness) {
    kernel.resolve_ref(
        RefNamespace::Profile,
        pk.to_string(),
        consumer.to_string(),
        RefShape::Profile(ProfileShape::Card),
        liveness,
        false,
        Vec::new(),
    );
}

// ─── trait contract ──────────────────────────────────────────────────────────

#[test]
fn namespace_const_matches_marker() {
    assert_eq!(ProfileNs::NAMESPACE, RefNamespace::Profile);
    assert_eq!(EventNs::NAMESPACE, RefNamespace::Event);
}

#[test]
fn liveness_from_ffi_and_round_trips_through_profile_liveness() {
    assert_eq!(RefLiveness::from_ffi(0), RefLiveness::CacheOk);
    assert_eq!(RefLiveness::from_ffi(1), RefLiveness::Live);
    // Orthogonal conversions used by the scaffold delegators.
    assert_eq!(RefLiveness::from(ProfileLiveness::Live), RefLiveness::Live);
    assert_eq!(ProfileLiveness::from(RefLiveness::CacheOk), ProfileLiveness::CacheOk);
}

// ─── dedup + refcount-to-zero ────────────────────────────────────────────────

#[test]
fn resolve_ref_dedups_consumers_then_tears_down_on_last_release() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let alice = hex64("a11ce");

    profile_card(&mut kernel, &alice, "view-a", RefLiveness::CacheOk);
    profile_card(&mut kernel, &alice, "view-b", RefLiveness::CacheOk);

    assert_eq!(
        kernel.profile_claims.get(&alice).map(|s| s.len()),
        Some(2),
        "two consumers of one key share one refcounted slot"
    );
    assert!(
        kernel.profile_claim_interest_lifecycle_for_test(&alice).is_some(),
        "one deduped kind:0 interest is live"
    );

    // First release: slot survives.
    kernel.release_ref(RefNamespace::Profile, &alice, "view-a");
    assert_eq!(kernel.profile_claims.get(&alice).map(|s| s.len()), Some(1));
    assert!(kernel.profile_claim_interest_lifecycle_for_test(&alice).is_some());

    // Last release: refcount → zero, the slot + shape record disappear.
    kernel.release_ref(RefNamespace::Profile, &alice, "view-b");
    assert!(
        kernel.profile_claims.get(&alice).is_none(),
        "the last release removes the refcount entry"
    );
    assert_eq!(
        kernel.profile_claim_interest_lifecycle_for_test(&alice),
        None,
        "the deduped interest is dropped on the last release"
    );
    assert_eq!(
        kernel.ref_demanded_profile_shape(&alice),
        None,
        "the widest-shape record is dropped on full teardown (D5)"
    );
}

// ─── Live wins ───────────────────────────────────────────────────────────────

#[test]
fn live_wins_over_cacheok_regardless_of_order() {
    // CacheOk then Live → Tailing.
    let mut k1 = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let a = hex64("a1");
    profile_card(&mut k1, &a, "feed", RefLiveness::CacheOk);
    assert_eq!(
        k1.profile_claim_interest_lifecycle_for_test(&a),
        Some(false),
        "a lone CacheOk claim is OneShot"
    );
    profile_card(&mut k1, &a, "screen", RefLiveness::Live);
    assert_eq!(
        k1.profile_claim_interest_lifecycle_for_test(&a),
        Some(true),
        "a later Live claim upgrades the slot to Tailing (Live wins)"
    );

    // Live then CacheOk → stays Tailing.
    let mut k2 = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let b = hex64("b2");
    profile_card(&mut k2, &b, "screen", RefLiveness::Live);
    profile_card(&mut k2, &b, "feed", RefLiveness::CacheOk);
    assert_eq!(
        k2.profile_claim_interest_lifecycle_for_test(&b),
        Some(true),
        "a later CacheOk claim must NOT downgrade a Tailing slot"
    );
}

// ─── shape ⟂ liveness ────────────────────────────────────────────────────────

#[test]
fn shape_is_independent_of_liveness() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let pk = hex64("5ha9e");

    // Ref + CacheOk: narrow shape, OneShot.
    kernel.resolve_ref(
        RefNamespace::Profile,
        pk.clone(),
        "feed".into(),
        RefShape::Profile(ProfileShape::Ref),
        RefLiveness::CacheOk,
        false,
        Vec::new(),
    );
    assert_eq!(kernel.ref_demanded_profile_shape(&pk), Some(ProfileShape::Ref));
    assert_eq!(kernel.profile_claim_interest_lifecycle_for_test(&pk), Some(false));

    // Widen the shape to Card WITHOUT touching liveness (still CacheOk): shape
    // changes, liveness does not.
    kernel.resolve_ref(
        RefNamespace::Profile,
        pk.clone(),
        "detail".into(),
        RefShape::Profile(ProfileShape::Card),
        RefLiveness::CacheOk,
        false,
        Vec::new(),
    );
    assert_eq!(kernel.ref_demanded_profile_shape(&pk), Some(ProfileShape::Card));
    assert_eq!(
        kernel.profile_claim_interest_lifecycle_for_test(&pk),
        Some(false),
        "widening the shape must not change liveness"
    );

    // Raise liveness to Live with the NARROW shape: liveness changes, the
    // widest demanded shape is unaffected (monotonic widen — not narrowed).
    kernel.resolve_ref(
        RefNamespace::Profile,
        pk.clone(),
        "screen".into(),
        RefShape::Profile(ProfileShape::Ref),
        RefLiveness::Live,
        false,
        Vec::new(),
    );
    assert_eq!(
        kernel.ref_demanded_profile_shape(&pk),
        Some(ProfileShape::Card),
        "raising liveness must not narrow the widest demanded shape"
    );
    assert_eq!(
        kernel.profile_claim_interest_lifecycle_for_test(&pk),
        Some(true),
        "liveness moves orthogonally to shape"
    );
}

// ─── per-key rev: resolve + release sites ────────────────────────────────────

#[test]
fn per_key_rev_advances_on_resolve_and_release() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let pk = hex64("e7");

    assert_eq!(kernel.ref_row_rev(RefNamespace::Profile, &pk), 0);

    profile_card(&mut kernel, &pk, "c1", RefLiveness::CacheOk);
    let r1 = kernel.ref_row_rev(RefNamespace::Profile, &pk);
    assert!(r1 > 0, "resolve must advance the per-key rev (site 1)");

    profile_card(&mut kernel, &pk, "c2", RefLiveness::CacheOk);
    let r2 = kernel.ref_row_rev(RefNamespace::Profile, &pk);
    assert!(r2 > r1, "a second resolve advances it again");

    kernel.release_ref(RefNamespace::Profile, &pk, "c1");
    let r3 = kernel.ref_row_rev(RefNamespace::Profile, &pk);
    assert!(r3 > r2, "release must advance the per-key rev (site 2)");

    kernel.release_ref(RefNamespace::Profile, &pk, "c2");
    let r4 = kernel.ref_row_rev(RefNamespace::Profile, &pk);
    assert!(r4 > r3, "the last release advances it once more");
}

// ─── per-key rev: profile ingest site ────────────────────────────────────────

#[test]
fn per_key_rev_advances_on_kind0_ingest_only_for_claimed_authors() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let claimed = hex64("c1a1med");
    let stranger = hex64("57ra9er");

    profile_card(&mut kernel, &claimed, "view", RefLiveness::CacheOk);
    let before = kernel.ref_row_rev(RefNamespace::Profile, &claimed);

    // A kind:0 rewriting the CLAIMED author's row bumps ITS per-key rev (site 3).
    inject_kind0(&mut kernel, &claimed, "Claimed Author");
    let after = kernel.ref_row_rev(RefNamespace::Profile, &claimed);
    assert!(
        after > before,
        "a kind:0 for a claimed author advances its per-key rev (ingest site)"
    );

    // A kind:0 for an UNCLAIMED author creates no row rev — the per-key map stays
    // bounded to resolved refs.
    inject_kind0(&mut kernel, &stranger, "Stranger");
    assert_eq!(
        kernel.ref_row_rev(RefNamespace::Profile, &stranger),
        0,
        "an unclaimed author never enters the per-key rev map"
    );
}

// ─── HIGH 4: shape narrows on release of the widest consumer (profile) ────────

#[test]
fn profile_shape_narrows_when_widest_consumer_releases() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let pk = hex64("5ha9e2");

    // Card consumer + Ref consumer → the row carries the widest (Card).
    kernel.resolve_ref(
        RefNamespace::Profile,
        pk.clone(),
        "detail".into(),
        RefShape::Profile(ProfileShape::Card),
        RefLiveness::CacheOk,
        false,
        Vec::new(),
    );
    kernel.resolve_ref(
        RefNamespace::Profile,
        pk.clone(),
        "feed".into(),
        RefShape::Profile(ProfileShape::Ref),
        RefLiveness::CacheOk,
        false,
        Vec::new(),
    );
    assert_eq!(kernel.ref_demanded_profile_shape(&pk), Some(ProfileShape::Card));
    let rev_before = kernel.ref_row_rev(RefNamespace::Profile, &pk);

    // Release the Card (widest) consumer while the Ref consumer remains: the row
    // must NARROW to Ref and the per-key rev must bump so the wire carries it.
    kernel.release_ref(RefNamespace::Profile, &pk, "detail");
    assert_eq!(
        kernel.ref_demanded_profile_shape(&pk),
        Some(ProfileShape::Ref),
        "releasing the widest (Card) consumer narrows the row to Ref (HIGH 4)"
    );
    assert!(
        kernel.ref_row_rev(RefNamespace::Profile, &pk) > rev_before,
        "the narrowing bumps the per-key rev"
    );
}

// ─── BLOCKING 2: per-key rev map stays bounded ───────────────────────────────

/// A spurious release of a never-claimed key must NOT create a permanent row-rev
/// entry — the map only ever holds keys that were actually claimed.
#[test]
fn spurious_release_of_never_claimed_key_creates_no_row() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let pk = hex64("9e7e6");
    kernel.release_ref(RefNamespace::Profile, &pk, "ghost");
    assert_eq!(kernel.ref_row_rev(RefNamespace::Profile, &pk), 0);
    assert!(
        kernel
            .projection_rev_tracker
            .source_versions
            .profile_row_revs
            .is_empty(),
        "a spurious profile release must not grow the rev map (BLOCKING 2 (a))"
    );

    let uri = nevent_uri(&hex64("9e7e7"));
    kernel.release_ref(RefNamespace::Event, &uri, "ghost");
    assert!(
        kernel
            .projection_rev_tracker
            .source_versions
            .event_row_revs
            .is_empty(),
        "a spurious event release must not grow the rev map (BLOCKING 2 (a))"
    );
}

/// resolve→release→reap churn of the same key keeps the per-key rev map bounded:
/// after Lane A emits each Cleared row and reaps, the map returns to empty.
#[test]
fn per_key_rev_map_stays_bounded_under_resolve_release_churn() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let pk = hex64("ba6c");
    for _ in 0..5 {
        profile_card(&mut kernel, &pk, "c", RefLiveness::CacheOk);
        // At most one live key while held.
        assert_eq!(
            kernel
                .projection_rev_tracker
                .source_versions
                .profile_row_revs
                .len(),
            1
        );
        kernel.release_ref(RefNamespace::Profile, &pk, "c");
        // Lane A emits the Cleared frame for this key, then reaps it.
        let reaped = kernel
            .projection_rev_tracker
            .source_versions
            .reap_cleared_profile_rows();
        assert_eq!(reaped, vec![pk.clone()], "the cleared key is reaped");
    }
    assert_eq!(
        kernel
            .projection_rev_tracker
            .source_versions
            .profile_row_revs
            .len(),
        0,
        "resolve→release→reap churn leaves the per-key rev map empty (BLOCKING 2 (b))"
    );
}

/// A key re-resolved BEFORE Lane A reaps it keeps its monotonic rev (the bump
/// cancels the pending clear), and the reap never drops a re-resolved live key.
/// This is the stale-low-rev guard coordinating with Lane A's rev consumption.
#[test]
fn re_resolve_before_reap_keeps_monotonic_rev_and_is_not_reaped() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let pk = hex64("ab12");
    profile_card(&mut kernel, &pk, "c", RefLiveness::CacheOk);
    kernel.release_ref(RefNamespace::Profile, &pk, "c"); // row Cleared; rev retained
    let rev_at_clear = kernel.ref_row_rev(RefNamespace::Profile, &pk);
    assert!(rev_at_clear > 0);

    // Re-resolve BEFORE the reap: cancels the pending clear, rev stays monotonic.
    profile_card(&mut kernel, &pk, "c2", RefLiveness::CacheOk);
    assert!(
        kernel.ref_row_rev(RefNamespace::Profile, &pk) > rev_at_clear,
        "a re-resolve advances the rev monotonically (no stale-low reuse)"
    );

    // The reap must leave the now-live key alone.
    let reaped = kernel
        .projection_rev_tracker
        .source_versions
        .reap_cleared_profile_rows();
    assert!(
        reaped.is_empty(),
        "a re-resolved key is no longer pending-clear, so reap leaves it"
    );
    assert!(
        kernel.ref_row_rev(RefNamespace::Profile, &pk) > 0,
        "the live re-resolved row keeps its rev (not reaped)"
    );
}

// ─── fail-closed on (namespace, shape) mismatch ──────────────────────────────

#[test]
fn namespace_shape_mismatch_is_a_silent_noop() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let pk = hex64("m15ma7ch");
    let out = kernel.resolve_ref(
        RefNamespace::Profile,
        pk.clone(),
        "v".into(),
        RefShape::Event(EventShape::Embed), // event shape under the profile namespace
        RefLiveness::CacheOk,
        false,
        Vec::new(),
    );
    assert!(out.is_empty());
    assert!(
        kernel.profile_claims.is_empty() && kernel.event_claims.is_empty(),
        "a shape/namespace mismatch must record no claim (D6 fail-closed)"
    );
}
