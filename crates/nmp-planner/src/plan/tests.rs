use super::*;
use crate::interest::InterestId;

/// Deterministic 64-char hex pubkey / event-id fixture from a single byte.
fn hex(byte: &str) -> String {
    byte.repeat(32)
}

/// Arbitrary host-declared timeline kind set for `timeline_for` fixtures.
/// Deliberately NOT `{1, 6}` — these hashing/dedup tests are kind-agnostic,
/// and using a non-social set keeps the V-68 "substrate carries no app
/// default" boundary visible at the call site.
fn tl_kinds() -> std::collections::BTreeSet<u32> {
    [30023u32].into_iter().collect()
}

// ── canonical_filter_hash ────────────────────────────────────────────────

#[test]
fn canonical_filter_hash_is_deterministic_for_identical_shapes() {
    // Two shapes built independently from the same field values must hash
    // identically — the §3.4 plan-id stability contract depends on this.
    let a = InterestShape::timeline_for([hex("aa"), hex("bb")].into_iter().collect(), tl_kinds());
    let b = InterestShape::timeline_for([hex("bb"), hex("aa")].into_iter().collect(), tl_kinds());
    // Same logical shape (BTreeSet sorts insertion order away).
    assert_eq!(a, b);
    // ...therefore the same canonical hash.
    assert_eq!(canonical_filter_hash(&a), canonical_filter_hash(&b));
    // And re-hashing the very same value is idempotent.
    assert_eq!(canonical_filter_hash(&a), canonical_filter_hash(&a));
}

#[test]
fn canonical_filter_hash_differs_for_distinct_shapes() {
    // Clearly distinct author sets → distinct hashes. Collision risk for
    // wholly different inputs is negligible for a 32-bit window.
    let aa = InterestShape::timeline_for([hex("aa")].into_iter().collect(), tl_kinds());
    let bb = InterestShape::timeline_for([hex("bb")].into_iter().collect(), tl_kinds());
    assert_ne!(canonical_filter_hash(&aa), canonical_filter_hash(&bb));

    // A different kind set on the same authors also changes the hash.
    let profile = InterestShape::profile_for(hex("aa"));
    assert_ne!(canonical_filter_hash(&aa), canonical_filter_hash(&profile));
}

#[test]
fn canonical_filter_hash_handles_empty_shape_without_panicking() {
    // The all-wildcard default shape must hash cleanly (no panic, no empty
    // string) — the compiler emits an empty plan for an empty interest set.
    let empty = InterestShape::default();
    let hash = canonical_filter_hash(&empty);
    assert!(!hash.is_empty());
    // The empty shape is stable across calls just like any other.
    assert_eq!(hash, canonical_filter_hash(&InterestShape::default()));
}

#[test]
fn canonical_filter_hash_emits_sixteen_hex_chars() {
    // Full 64-bit FNV-1a digest: a 16-char lowercase hex string. Widened
    // from the prior 8-char (32-bit) stop-gap to eliminate birthday
    // collisions at a few-thousand-shape scale (audit finding P-1).
    for shape in [
        InterestShape::default(),
        InterestShape::timeline_for([hex("aa")].into_iter().collect(), tl_kinds()),
        InterestShape::profile_for(hex("cc")),
    ] {
        let hash = canonical_filter_hash(&shape);
        assert_eq!(hash.len(), 16, "hash must be 16 chars: {hash}");
        assert!(
            hash.chars().all(|c| c.is_ascii_hexdigit()),
            "hash must be hex: {hash}"
        );
    }
}

#[test]
fn canonical_filter_hash_emits_full_64bit_digest_not_truncated() {
    // Regression guard: the old code truncated to 32 bits (`hash & 0xffff_ffff`).
    // Verify the new code emits the full 64-bit FNV-1a digest by computing the
    // expected value independently via `stable_hash64` and asserting the full
    // 16-hex-char form matches.
    use crate::stable_hash::stable_hash64;

    let shape = InterestShape::profile_for(hex("aa"));
    let json = serde_json::to_string(&shape).expect("serialise shape");
    let expected_full_u64 = stable_hash64(("canonical-filter", json));

    // The hash string must equal the full 64-bit digest in hex (16 chars).
    let actual = canonical_filter_hash(&shape);
    assert_eq!(actual, format!("{:016x}", expected_full_u64));
    assert_eq!(actual.len(), 16);

    // Verify the old 32-bit truncation differs for this input (ensuring the
    // fix is observable — if by coincidence the upper 32 bits were zero the
    // guard would be vacuous, but FNV-1a over non-trivial inputs never is).
    let truncated = format!("{:08x}", expected_full_u64 & 0xffff_ffff);
    assert_ne!(
        actual, truncated,
        "full 64-bit digest must differ from 32-bit truncation for this input"
    );
}

// ── SubShape::recompute_hash ─────────────────────────────────────────────

#[test]
fn recompute_hash_refreshes_a_stale_hash_from_the_current_shape() {
    // Start with a SubShape carrying a deliberately wrong cached hash.
    let mut sub = SubShape {
        shape: InterestShape::timeline_for([hex("aa")].into_iter().collect(), tl_kinds()),
        originating_interests: vec![InterestId(1)],
        canonical_filter_hash: "deadbeef".to_string(),
    };
    let expected_before = canonical_filter_hash(&sub.shape);

    // Mutate the shape (the M4 coverage gate bumps `since` post-compile).
    sub.shape.since = Some(1_700_000_000);
    let expected_after = canonical_filter_hash(&sub.shape);
    // The mutation genuinely changed the canonical hash.
    assert_ne!(expected_before, expected_after);

    // recompute_hash must adopt the post-mutation shape's hash, discarding
    // both the stale "deadbeef" and the pre-mutation value.
    sub.recompute_hash();
    assert_eq!(sub.canonical_filter_hash, expected_after);
    assert_ne!(sub.canonical_filter_hash, "deadbeef");
}

#[test]
fn recompute_hash_ignores_originating_interests() {
    // Only `shape` is hashed — the originating-interest provenance list is
    // not part of wire identity. Two SubShapes with identical shapes but
    // different originating interests must produce the same hash.
    let shape = InterestShape::timeline_for([hex("aa")].into_iter().collect(), tl_kinds());
    let mut one = SubShape {
        shape: shape.clone(),
        originating_interests: vec![InterestId(1)],
        canonical_filter_hash: String::new(),
    };
    let mut many = SubShape {
        shape,
        originating_interests: vec![InterestId(7), InterestId(9), InterestId(42)],
        canonical_filter_hash: String::new(),
    };
    one.recompute_hash();
    many.recompute_hash();
    assert_eq!(one.canonical_filter_hash, many.canonical_filter_hash);
}

// ── CompiledPlan::empty ──────────────────────────────────────────────────

#[test]
fn compiled_plan_empty_carries_the_given_id_and_no_relay_plans() {
    let plan = CompiledPlan::empty("plan-abc123");
    // The supplied id is carried verbatim.
    assert_eq!(plan.plan_id, "plan-abc123");
    // No relay plans and no unroutable authors — a truly empty plan.
    assert!(plan.per_relay.is_empty());
    assert!(plan.unroutable_authors.is_empty());

    // `impl Into<String>` accepts an owned String too.
    let owned = CompiledPlan::empty(String::from("owned-id"));
    assert_eq!(owned.plan_id, "owned-id");
}
