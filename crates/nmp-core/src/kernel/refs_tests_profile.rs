//! ADR-0063 (#1671 Lane B) — profile-resolver + shared lifecycle/dedup/rev unit
//! tests for the kernel-owned `RefResolver` primitive.
//!
//! Event-resolver tests live in `refs_tests_event.rs` (Lane D merge target).

use super::refs::{
    EventShape, ProfileShape, RefLiveness, RefNamespace, RefResolver, RefShape,
};
use super::refs::{EventNs, ProfileNs};
use super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

fn hex64(prefix: &str) -> String {
    format!("{prefix:0<64}").chars().take(64).collect()
}

fn inject_kind0(kernel: &mut Kernel, pubkey: &str, display_name: &str) {
    kernel.seed_profile_view_for_test(
        pubkey,
        crate::substrate::ProfileView {
            event_id: "0".repeat(64),
            created_at: 1_700_000_000,
            display: display_name.to_string(),
            raw_display_name: Some(display_name.to_string()),
            picture_url: Some("https://example.com/a.png".to_string()),
            ..Default::default()
        },
    );
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

    // A NON-last release (c1) bumps the SURVIVING row (site 2 — the row narrows /
    // re-asserts to the remaining consumers).
    kernel.release_ref(RefNamespace::Profile, &pk, "c1");
    let r3 = kernel.ref_row_rev(RefNamespace::Profile, &pk);
    assert!(r3 > r2, "a non-last release advances the surviving per-key rev (site 2)");

    // The LAST release tears the row down: `clear_profile_row` bumps to the final
    // post-clear rev and immediately removes the entry (BLOCKING 2), so the row now
    // reads 0 (gone) rather than retaining a stale rev forever.
    kernel.release_ref(RefNamespace::Profile, &pk, "c2");
    assert_eq!(
        kernel.ref_row_rev(RefNamespace::Profile, &pk),
        0,
        "the last release clears the per-key rev entry (row gone — BLOCKING 2)"
    );
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

// ─── warm re-resolve is a pure no-op (BLOCKING 3 regression guard) ──────────
//
// A warm identical re-resolve (same pubkey/consumer/shape/liveness, force=false)
// must NOT bump `changed_since_emit` or the per-key rev.  Without this gate
// a `claim → snapshot → render → claim` loop would busy-emit on every UI frame.
//
// Symmetrically, releasing an already-absent consumer must not bump the dirty
// flag — a spurious release of a never-claimed (or already-released) consumer
// is a structural no-op (ADR-0063 BLOCKING 3 / BLOCKING 2(a)).
//
// Reconciles the intent from preserved stash `stash-preserve/20260615-7-profile-claim-loop`
// (branch `pr1436-investigate`) against the current ADR-0063 `resolve_ref` /
// `release_ref` API (the stash's `claim_profile` / `release_profile` names and
// `profile_claim_projection_tests.rs` file no longer exist).

#[test]
fn warm_reresolve_and_noop_release_emit_no_change() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let pk = hex64("w4rmk3y");

    // ── Step 1: cold first resolve — must dirty the kernel and advance the rev.
    profile_card(&mut kernel, &pk, "view-x", RefLiveness::CacheOk);
    assert!(
        kernel.changed_since_emit,
        "the first (cold) resolve must set changed_since_emit"
    );
    let r1 = kernel.ref_row_rev(RefNamespace::Profile, &pk);
    assert!(r1 > 0, "the first resolve must advance the per-key rev above zero");

    // ── Step 2: simulate a snapshot emission — clears the dirty flag.
    let _ = kernel.make_update(true);
    assert!(
        !kernel.changed_since_emit,
        "make_update must clear changed_since_emit (post-emit baseline)"
    );

    // ── Step 3: warm re-resolve — identical (key, consumer, shape, liveness).
    // `inserted=false`, `shape_widened=false`, `liveness_upgraded=false` →
    // `mutated=false` → dirty flag and per-key rev must stay unchanged.
    profile_card(&mut kernel, &pk, "view-x", RefLiveness::CacheOk);
    assert!(
        !kernel.changed_since_emit,
        "a warm identical re-resolve must NOT set changed_since_emit (loop breaker)"
    );
    assert_eq!(
        kernel.ref_row_rev(RefNamespace::Profile, &pk),
        r1,
        "a warm identical re-resolve must NOT advance the per-key rev"
    );

    // ── Step 4: real release of the lone consumer — must dirty the kernel.
    kernel.release_ref(RefNamespace::Profile, &pk, "view-x");
    assert!(
        kernel.changed_since_emit,
        "releasing the only consumer must set changed_since_emit"
    );
    let _ = kernel.make_update(true);
    assert!(
        !kernel.changed_since_emit,
        "make_update must clear changed_since_emit after the release"
    );

    // ── Step 5: spurious release of an already-absent consumer — no-op.
    // `actually_removed=false`, `shape_narrowed=false`, `liveness_downgraded=false`
    // → `mutated=false` → dirty flag must remain false.
    kernel.release_ref(RefNamespace::Profile, &pk, "view-x");
    assert!(
        !kernel.changed_since_emit,
        "a spurious release of an absent consumer must NOT set changed_since_emit (BLOCKING 2(a))"
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
