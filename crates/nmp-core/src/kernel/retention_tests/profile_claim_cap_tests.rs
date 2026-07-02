//! T114b — `profile_claims[pk]` per-pubkey consumer-id cap. Pins the
//! drop-newest overflow semantics, the per-pubkey (not global) scoping, the
//! release→reclaim recovery path, and a 16× flood harness proving the set
//! never grows past `MAX_CLAIMS_PER_PUBKEY`.

use super::retention_fixtures_support::{deterministic_pubkey, resolve_profile_card};
use crate::kernel::{Kernel, RefNamespace, MAX_CLAIMS_PER_PUBKEY};
use crate::relay::DEFAULT_VISIBLE_LIMIT;

/// T114b core invariant: per-pubkey claim consumer-id set is bounded.
/// Pump 4× the cap of unique consumer_ids onto one pubkey. The set must
/// stabilise at `MAX_CLAIMS_PER_PUBKEY` and `claim_drops_total` must record
/// the overflow exactly (4×cap claims sent → cap retained → 3×cap drops).
#[test]
fn claim_profile_set_bounded_at_per_pubkey_cap() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let pk = deterministic_pubkey(0);

    let n = MAX_CLAIMS_PER_PUBKEY * 4;
    for i in 0..n {
        // Unique consumer_id per call — mirrors S2's mix (no matching release).
        resolve_profile_card(&mut kernel, &pk, format!("c{i}"));
    }

    assert_eq!(
        kernel.profile_claims_len_for_test(&pk),
        MAX_CLAIMS_PER_PUBKEY,
        "claim set must stabilise at cap"
    );
    assert_eq!(
        kernel.claim_drops_total_test(),
        (n - MAX_CLAIMS_PER_PUBKEY) as u64,
        "every overflow must be counted"
    );
}

/// T114b — D6 invariant: a dropped claim is a silent no-op, not an FFI error.
/// a profile `resolve_ref` returns `Vec<OutboundMessage>` for the actor's outbound
/// path; a dropped claim must produce an empty Vec, never a panic or partial
/// mutation that could later trip an assertion.
#[test]
fn dropped_claim_is_silent_noop() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let pk = deterministic_pubkey(0);

    // Fill to cap.
    for i in 0..MAX_CLAIMS_PER_PUBKEY {
        resolve_profile_card(&mut kernel, &pk, format!("c{i}"));
    }
    assert_eq!(kernel.claim_drops_total_test(), 0);

    // One past the cap.
    let overflow = resolve_profile_card(&mut kernel, &pk, "overflow-consumer");
    assert!(
        overflow.is_empty(),
        "dropped claim must return empty outbound"
    );
    assert_eq!(kernel.claim_drops_total_test(), 1);

    // Re-claiming an already-present consumer is NOT a drop — it's an
    // idempotent no-op handled by `BTreeSet::insert` returning false. The
    // cap check must skip when the consumer is already in the set.
    let dup = resolve_profile_card(&mut kernel, &pk, "c0");
    assert!(dup.is_empty());
    assert_eq!(
        kernel.claim_drops_total_test(),
        1,
        "duplicate claim of existing consumer must NOT count as drop"
    );
}

/// T114b — distinct pubkeys retain independent caps. Filling one pubkey's
/// set to cap must not steal capacity from another pubkey.
#[test]
fn claim_cap_is_per_pubkey_not_global() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let pk_a = deterministic_pubkey(1);
    let pk_b = deterministic_pubkey(2);

    // Saturate pk_a.
    for i in 0..(MAX_CLAIMS_PER_PUBKEY + 16) {
        resolve_profile_card(&mut kernel, &pk_a, format!("a{i}"));
    }
    assert_eq!(
        kernel.profile_claims_len_for_test(&pk_a),
        MAX_CLAIMS_PER_PUBKEY
    );
    assert_eq!(kernel.claim_drops_total_test(), 16);

    // pk_b is fresh — claims must succeed up to its own cap.
    for i in 0..32 {
        resolve_profile_card(&mut kernel, &pk_b, format!("b{i}"));
    }
    assert_eq!(kernel.profile_claims_len_for_test(&pk_b), 32);
    assert_eq!(
        kernel.claim_drops_total_test(),
        16,
        "filling pk_b must not bump the global drop counter beyond pk_a's overflow"
    );
}

/// T114b — release path is still effective after a drop episode. Once
/// existing consumers release, freed slots accept new claims again. This
/// pins the recovery semantic: drop-newest is not a permanent block.
#[test]
fn claim_recovers_after_release_post_drop() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let pk = deterministic_pubkey(3);

    // Fill + overflow.
    for i in 0..(MAX_CLAIMS_PER_PUBKEY + 1) {
        resolve_profile_card(&mut kernel, &pk, format!("c{i}"));
    }
    assert_eq!(kernel.claim_drops_total_test(), 1);

    // Release one existing consumer (c0..c1023 are in the set; the overflow
    // c1024 was dropped, so releasing c0 frees a slot).
    kernel.release_ref(RefNamespace::Profile, &pk, "c0");
    assert_eq!(
        kernel.profile_claims_len_for_test(&pk),
        MAX_CLAIMS_PER_PUBKEY - 1
    );

    // The previously-dropped consumer can now claim.
    resolve_profile_card(&mut kernel, &pk, "post-release-consumer");
    assert_eq!(
        kernel.profile_claims_len_for_test(&pk),
        MAX_CLAIMS_PER_PUBKEY
    );
    assert_eq!(
        kernel.claim_drops_total_test(),
        1,
        "post-release claim must NOT bump drops (slot was free)"
    );
}

/// T114b — allocation-bounded harness using the global allocator. Pumps
/// 16× MAX_CLAIMS_PER_PUBKEY claims (16k unique consumer_ids) onto one
/// pubkey and asserts that the bound is observed via the public counter +
/// the set size. This is a deterministic functional check for the bounded
/// retention invariant.
#[test]
fn claim_flood_does_not_grow_unbounded() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let pk = deterministic_pubkey(7);

    let flood_size = MAX_CLAIMS_PER_PUBKEY * 16;
    for i in 0..flood_size {
        resolve_profile_card(&mut kernel, &pk, format!("flood-{i:08}"));
    }

    // The set is at cap, no more.
    assert_eq!(
        kernel.profile_claims_len_for_test(&pk),
        MAX_CLAIMS_PER_PUBKEY,
        "16× flood must NOT grow past cap"
    );

    // Drops counter recorded every overflow.
    assert_eq!(
        kernel.claim_drops_total_test(),
        (flood_size - MAX_CLAIMS_PER_PUBKEY) as u64,
        "every overflow accounted for"
    );

    // Memory bound proof: if the BTreeSet were still growing per-dispatch
    // (a pre-fix regression), `len()` would be `flood_size` not the cap.
    // The set's heap footprint is therefore O(MAX_CLAIMS_PER_PUBKEY × avg
    // consumer_id size), independent of dispatch count — the D8 invariant.
}
