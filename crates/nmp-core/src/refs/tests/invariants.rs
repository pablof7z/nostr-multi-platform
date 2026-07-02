//! ADR-0070 Lane A — explicit invariant + behaviour unit tests.
//!
//! Each test pins one ADR-0070 invariant or BLOCKING-N contract individually.
//! The property-test harness (the merge gate) lives in the sibling `property`
//! module.

use super::super::*;
use super::property::{decode_ok, ground_truth, payload_for, wire_round_trip, REF_NS};

// ── Explicit invariant + behaviour tests ────────────────────────────────────────

/// Invariant #1: a key ABSENT from a batch is Unchanged (retained), never
/// Cleared. Clearing is always an explicit `Cleared` row.
#[test]
fn invariant1_absence_is_unchanged_not_cleared() {
    let mut source = MapRowRevSource::new();
    let mut tracker = RefRowDeltaTracker::new();
    let mut cache = RefRowCache::new();

    source.upsert("profile", "alice", 1, payload_for("alice", 1));
    source.upsert("profile", "bob", 1, payload_for("bob", 1));
    let baseline = wire_round_trip(&tracker.build_baseline("profile", &source));
    cache.apply(&baseline, 1, 0, &decode_ok);
    assert_eq!(cache.snapshot("profile").len(), 2);

    // Bump only alice. The incremental batch must carry ONLY alice's row;
    // bob's row is ABSENT — and bob must remain cached (Unchanged, not cleared).
    source.upsert("profile", "alice", 2, payload_for("alice", 2));
    let incr = wire_round_trip(&tracker.build_incremental("profile", &source));
    assert_eq!(incr.rows.len(), 1, "only the changed row crosses the wire");
    assert_eq!(incr.rows[0].key, "alice");
    cache.apply(&incr, 1, 0, &decode_ok);

    assert_eq!(
        cache.get("profile", "bob"),
        Some(payload_for("bob", 1)),
        "absent row must be retained (Unchanged), never silently cleared"
    );
    assert_eq!(cache.get("profile", "alice"), Some(payload_for("alice", 2)));
}

/// Invariant #2: decode-before-commit. A malformed `Changed` row leaves the
/// prior cached row intact, does not corrupt the cache, does not panic, and
/// latches `needs_resync` — while OTHER rows in the same batch still commit.
#[test]
fn invariant2_decode_before_commit_keeps_prior_on_malformed() {
    let mut cache = RefRowCache::new();

    // Seed alice + bob via a baseline.
    let seed = RefRowDeltaBatch {
        namespace: "profile".into(),
        baseline: true,
        rows: vec![
            RefRow::changed("alice", 1, payload_for("alice", 1)),
            RefRow::changed("bob", 1, payload_for("bob", 1)),
        ],
    };
    cache.apply(&wire_round_trip(&seed), 1, 0, &decode_ok);

    // A batch where alice's new row is POISONED (0xFF prefix → decode fails) but
    // bob's row is valid. alice must keep her prior value; bob must update.
    let mut poisoned = payload_for("alice", 2);
    poisoned[0] = 0xFF;
    let batch = RefRowDeltaBatch {
        namespace: "profile".into(),
        baseline: false,
        rows: vec![
            RefRow::changed("alice", 2, poisoned),
            RefRow::changed("bob", 2, payload_for("bob", 2)),
        ],
    };
    let outcome = cache.apply(&wire_round_trip(&batch), 1, 0, &decode_ok);

    assert!(
        outcome.decode_failed,
        "malformed row must report decode failure"
    );
    assert!(
        cache.needs_resync(),
        "needs_resync must latch (fail-closed, D6)"
    );
    assert_eq!(
        cache.get("profile", "alice"),
        Some(payload_for("alice", 1)),
        "malformed row must NOT corrupt the prior cached row"
    );
    assert_eq!(
        cache.get("profile", "bob"),
        Some(payload_for("bob", 2)),
        "a sibling valid row in the same batch still commits"
    );
}

/// Invariant #3: epoch bump → full baseline reconstructs a corrupt host cache.
#[test]
fn invariant3_epoch_resync_repairs_corrupt_cache() {
    let mut source = MapRowRevSource::new();
    let mut tracker = RefRowDeltaTracker::new();
    let mut cache = RefRowCache::new();

    source.upsert("profile", "alice", 1, payload_for("alice", 1));
    source.upsert("profile", "bob", 1, payload_for("bob", 1));
    cache.apply(
        &wire_round_trip(&tracker.build_baseline("profile", &source)),
        1,
        0,
        &decode_ok,
    );

    // Corrupt the host cache out from under the protocol + leave a stale ghost.
    cache.corrupt_for_test("profile", "alice", b"GARBAGE".to_vec());
    source.upsert("event", "ev1", 2, payload_for("ev1", 2)); // a row the host never saw

    // Epoch bump → producer reset → baseline for every namespace, applied at the
    // new epoch. The cache clears and reconstructs the COMPLETE live set.
    tracker.reset();
    let new_epoch = 1;
    for ns in REF_NS {
        let baseline = wire_round_trip(&tracker.build_baseline(ns, &source));
        cache.apply(&baseline, 1, new_epoch, &decode_ok);
    }

    assert_eq!(cache.snapshot("profile"), ground_truth(&source, "profile"));
    assert_eq!(cache.snapshot("event"), ground_truth(&source, "event"));
    assert!(
        !cache.needs_resync(),
        "epoch re-baseline clears needs_resync"
    );
}

/// A release produces an EXPLICIT `Cleared` row that removes the host's cached
/// row (the counterpart to invariant #1).
#[test]
fn cleared_is_explicit_and_removes() {
    let mut source = MapRowRevSource::new();
    let mut tracker = RefRowDeltaTracker::new();
    let mut cache = RefRowCache::new();

    source.upsert("profile", "alice", 1, payload_for("alice", 1));
    cache.apply(
        &wire_round_trip(&tracker.build_baseline("profile", &source)),
        1,
        0,
        &decode_ok,
    );
    assert!(cache.get("profile", "alice").is_some());

    source.remove("profile", "alice", 2);
    let incr = wire_round_trip(&tracker.build_incremental("profile", &source));
    assert_eq!(incr.rows.len(), 1);
    assert_eq!(incr.rows[0].state, RefRowState::Cleared);
    assert!(
        incr.rows[0].rev > 1,
        "clear carries the release rev, not the prior live rev"
    );
    cache.apply(&incr, 1, 0, &decode_ok);
    assert_eq!(
        cache.get("profile", "alice"),
        None,
        "Cleared row removes the cached row"
    );
}

/// The per-key reorder guard skips a `Changed` row whose rev is not newer than
/// the cached rev (last-rev-wins; never applies a stale intermediate).
#[test]
fn reorder_guard_skips_stale_rev() {
    let mut cache = RefRowCache::new();
    let newer = RefRowDeltaBatch {
        namespace: "profile".into(),
        baseline: true,
        rows: vec![RefRow::changed("alice", 5, payload_for("alice", 5))],
    };
    cache.apply(&wire_round_trip(&newer), 1, 0, &decode_ok);

    // A reordered older row (rev 3) must NOT clobber the rev-5 value.
    let older = RefRowDeltaBatch {
        namespace: "profile".into(),
        baseline: false,
        rows: vec![RefRow::changed("alice", 3, payload_for("alice", 3))],
    };
    let outcome = cache.apply(&wire_round_trip(&older), 1, 0, &decode_ok);
    assert!(outcome.changed_keys.is_empty(), "stale rev must be a no-op");
    assert_eq!(cache.get("profile", "alice"), Some(payload_for("alice", 5)));
}

/// BLOCKING-4 (rev-safe clears): a STALE reordered `Cleared` row — one whose rev
/// is NOT newer than the cached live row — must NOT delete that newer row. This
/// is the reorder hazard the per-key rev guard closes for clears (symmetric to
/// the `Changed` reorder guard).
#[test]
fn reorder_guard_skips_stale_clear() {
    let mut cache = RefRowCache::new();
    // Cache alice live at rev 6.
    let live = RefRowDeltaBatch {
        namespace: "profile".into(),
        baseline: true,
        rows: vec![RefRow::changed("alice", 6, payload_for("alice", 6))],
    };
    cache.apply(&wire_round_trip(&live), 1, 0, &decode_ok);

    // A reordered STALE clear (rev 5 < cached rev 6) must be ignored — alice is
    // live at a newer rev, so the clear is not the latest word on her.
    let stale_clear = RefRowDeltaBatch {
        namespace: "profile".into(),
        baseline: false,
        rows: vec![RefRow::cleared("alice", 5)],
    };
    let outcome = cache.apply(&wire_round_trip(&stale_clear), 1, 0, &decode_ok);
    assert!(
        outcome.changed_keys.is_empty(),
        "stale clear must be a no-op"
    );
    assert_eq!(
        cache.get("profile", "alice"),
        Some(payload_for("alice", 6)),
        "a stale reordered clear must NOT delete a newer live row"
    );

    // A clear with a NEWER rev (7 > 6) is the latest word → it removes the row.
    let fresh_clear = RefRowDeltaBatch {
        namespace: "profile".into(),
        baseline: false,
        rows: vec![RefRow::cleared("alice", 7)],
    };
    let outcome = cache.apply(&wire_round_trip(&fresh_clear), 1, 0, &decode_ok);
    assert_eq!(outcome.changed_keys, vec!["alice".to_string()]);
    assert_eq!(
        cache.get("profile", "alice"),
        None,
        "a newer clear removes the row"
    );
}

/// BLOCKING-1 (scratch-then-commit baseline): a malformed row INSIDE a baseline
/// batch must NOT drop or corrupt the prior cache. The baseline decodes into a
/// scratch map first and commits only if EVERY required row decodes; one bad row
/// fails the whole baseline closed — prior cache intact, `needs_resync` latched.
#[test]
fn baseline_decode_failure_preserves_prior_cache() {
    let mut cache = RefRowCache::new();
    // Seed a good prior cache (alice + bob) via a clean baseline.
    let seed = RefRowDeltaBatch {
        namespace: "profile".into(),
        baseline: true,
        rows: vec![
            RefRow::changed("alice", 1, payload_for("alice", 1)),
            RefRow::changed("bob", 1, payload_for("bob", 1)),
        ],
    };
    cache.apply(&wire_round_trip(&seed), 1, 0, &decode_ok);
    assert_eq!(cache.snapshot("profile").len(), 2);

    // A NEW baseline whose carol row is POISONED (0xFF prefix → decode fails).
    // Under scratch-then-commit the prior namespace must stay fully intact.
    let mut poisoned = payload_for("carol", 1);
    poisoned[0] = 0xFF;
    let bad_baseline = RefRowDeltaBatch {
        namespace: "profile".into(),
        baseline: true,
        rows: vec![
            RefRow::changed("alice", 9, payload_for("alice", 9)),
            RefRow::changed("carol", 1, poisoned),
        ],
    };
    let outcome = cache.apply(&wire_round_trip(&bad_baseline), 1, 0, &decode_ok);

    assert!(
        outcome.decode_failed,
        "a malformed baseline row fails the batch"
    );
    assert!(
        cache.needs_resync(),
        "needs_resync latches (fail-closed, D6)"
    );
    assert!(
        outcome.changed_keys.is_empty(),
        "no slot is committed on a failed baseline"
    );
    // Prior cache is byte-for-byte intact: alice was NOT advanced to rev 9, bob
    // (absent from the bad baseline) was NOT dropped, carol was NOT inserted.
    assert_eq!(
        cache.get("profile", "alice"),
        Some(payload_for("alice", 1)),
        "prior row must not be clobbered by a malformed baseline"
    );
    assert_eq!(cache.get("profile", "bob"), Some(payload_for("bob", 1)));
    assert_eq!(cache.get("profile", "carol"), None);
}

/// BLOCKING-3 (decode-before-commit seam, non-empty invalid): the `decode_ok`
/// seam — the per-namespace typed-row validator the host invokes before commit —
/// must reject a NON-EMPTY but invalid payload, keeping the prior row. (Lane C
/// wires the real ProfileRef / EventEmbed decoder into this seam; here a test
/// decoder proves the contract beyond the empty-payload case.)
#[test]
fn decode_seam_rejects_non_empty_invalid_row() {
    let mut cache = RefRowCache::new();
    // A decoder that requires a 4-byte magic prefix — non-empty garbage fails.
    let strict_decode = |_key: &str, payload: &[u8]| payload.starts_with(b"OK::");
    let good = |k: &str| {
        let mut p = b"OK::".to_vec();
        p.extend_from_slice(k.as_bytes());
        p
    };

    let seed = RefRowDeltaBatch {
        namespace: "profile".into(),
        baseline: true,
        rows: vec![RefRow::changed("alice", 1, good("alice-v1"))],
    };
    cache.apply(&wire_round_trip(&seed), 1, 0, &strict_decode);
    assert_eq!(cache.get("profile", "alice"), Some(good("alice-v1")));

    // A NON-EMPTY but invalid (no magic) update must be rejected by the seam.
    let bad = RefRowDeltaBatch {
        namespace: "profile".into(),
        baseline: false,
        rows: vec![RefRow::changed(
            "alice",
            2,
            b"non-empty-but-invalid".to_vec(),
        )],
    };
    let outcome = cache.apply(&wire_round_trip(&bad), 1, 0, &strict_decode);
    assert!(
        outcome.decode_failed,
        "non-empty invalid payload fails the seam"
    );
    assert!(cache.needs_resync());
    assert_eq!(
        cache.get("profile", "alice"),
        Some(good("alice-v1")),
        "prior row retained when a non-empty payload fails typed decode"
    );
}

/// Invariant #4 (typed per namespace): the two namespaces never cross-pollute —
/// the same key in `profile` and `event` are independent cached rows.
#[test]
fn typed_per_namespace_isolation() {
    let mut source = MapRowRevSource::new();
    let mut tracker = RefRowDeltaTracker::new();
    let mut cache = RefRowCache::new();

    source.upsert("profile", "shared", 1, b"profile-bytes".to_vec());
    source.upsert("event", "shared", 1, b"event-bytes".to_vec());
    for ns in REF_NS {
        cache.apply(
            &wire_round_trip(&tracker.build_baseline(ns, &source)),
            1,
            0,
            &decode_ok,
        );
    }
    assert_eq!(
        cache.get("profile", "shared"),
        Some(b"profile-bytes".to_vec())
    );
    assert_eq!(cache.get("event", "shared"), Some(b"event-bytes".to_vec()));

    // Clearing the profile row must not touch the event row.
    source.remove("profile", "shared", 2);
    cache.apply(
        &wire_round_trip(&tracker.build_incremental("profile", &source)),
        1,
        0,
        &decode_ok,
    );
    assert_eq!(cache.get("profile", "shared"), None);
    assert_eq!(cache.get("event", "shared"), Some(b"event-bytes".to_vec()));
}

/// A `baseline` batch replaces its namespace wholesale: a row that existed
/// before but is no longer live is dropped even without an explicit Cleared.
#[test]
fn baseline_replaces_namespace_wholesale() {
    let mut cache = RefRowCache::new();
    let first = RefRowDeltaBatch {
        namespace: "profile".into(),
        baseline: true,
        rows: vec![
            RefRow::changed("alice", 1, payload_for("alice", 1)),
            RefRow::changed("ghost", 1, payload_for("ghost", 1)),
        ],
    };
    cache.apply(&wire_round_trip(&first), 1, 0, &decode_ok);
    assert_eq!(cache.snapshot("profile").len(), 2);

    // A new baseline (same session/epoch) without `ghost` must drop it.
    let second = RefRowDeltaBatch {
        namespace: "profile".into(),
        baseline: true,
        rows: vec![RefRow::changed("alice", 2, payload_for("alice", 2))],
    };
    cache.apply(&wire_round_trip(&second), 1, 0, &decode_ok);
    assert_eq!(
        cache
            .snapshot("profile")
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        vec!["alice"]
    );
}

/// The wire codec is lossless for every field (key/rev/state/payload + batch
/// namespace/baseline), including the Cleared (empty-payload) case.
#[test]
fn wire_round_trip_is_lossless() {
    let batch = RefRowDeltaBatch {
        namespace: "event".into(),
        baseline: true,
        rows: vec![
            RefRow::changed("ev_a", 7, vec![1, 2, 3, 4]),
            RefRow::cleared("ev_b", 9),
            RefRow::changed("ev_c", 12, vec![0xAB; 64]),
        ],
    };
    let decoded = decode_ref_row_delta_batch(&encode_ref_row_delta_batch(&batch)).unwrap();
    assert_eq!(decoded, batch);
}

/// A malformed buffer fails closed at decode (no panic).
#[test]
fn decode_rejects_garbage() {
    assert!(decode_ref_row_delta_batch(&[0u8; 3]).is_err());
    assert!(decode_ref_row_delta_batch(b"not a flatbuffer").is_err());
}
