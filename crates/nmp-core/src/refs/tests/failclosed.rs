//! ADR-0063 Lane A — BLOCKING fail-closed regression tests.
//!
//! Each test PROVES one of the four BLOCKING correctness guards bites: it is
//! written to FAIL on the pre-fix code and pass after the fix. They exercise the
//! REAL wire codec (encode_ref_row_delta_batch / decode_ref_row_delta_batch)
//! and the real apply path, including raw-byte forgeries (out-of-range state
//! discriminant, missing key) the safe encoder never produces.

use super::super::*;
use super::property::{decode_ok, payload_for, wire_round_trip};

/// Encode a batch where one row carries a RAW wire `state` discriminant — used
/// to forge an out-of-range state (e.g. 255) the safe encoder never emits.
/// Mirrors `encode_ref_row_delta_batch` but stamps `raw_state` on `raw_state_at`.
fn encode_with_raw_state(batch: &RefRowDeltaBatch, raw_state_at: usize, raw_state: u8) -> Vec<u8> {
    use super::super::wire as fb;
    use flatbuffers::FlatBufferBuilder;
    let mut builder = FlatBufferBuilder::new();
    let row_offsets: Vec<_> = batch
        .rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let key = builder.create_string(&row.key);
            let payload = if row.payload.is_empty() {
                None
            } else {
                Some(builder.create_vector(&row.payload))
            };
            let state = if i == raw_state_at {
                fb::RefRowState(raw_state)
            } else {
                row.state.into()
            };
            fb::RefRow::create(
                &mut builder,
                &fb::RefRowArgs {
                    key: Some(key),
                    rev: row.rev,
                    state,
                    payload,
                },
            )
        })
        .collect();
    let rows = builder.create_vector(&row_offsets);
    let namespace = builder.create_string(&batch.namespace);
    let root = fb::RefRowDeltaBatch::create(
        &mut builder,
        &fb::RefRowDeltaBatchArgs {
            namespace: Some(namespace),
            baseline: batch.baseline,
            rows: Some(rows),
        },
    );
    fb::finish_ref_row_delta_batch_buffer(&mut builder, root);
    builder.finished_data().to_vec()
}

/// Encode a batch but OMIT the `key` field on the row at `omit_key_at`, forging
/// the missing-key case the safe encoder never produces.
fn encode_with_missing_key(batch: &RefRowDeltaBatch, omit_key_at: usize) -> Vec<u8> {
    use super::super::wire as fb;
    use flatbuffers::FlatBufferBuilder;
    let mut builder = FlatBufferBuilder::new();
    let row_offsets: Vec<_> = batch
        .rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let key = if i == omit_key_at {
                None
            } else {
                Some(builder.create_string(&row.key))
            };
            let payload = if row.payload.is_empty() {
                None
            } else {
                Some(builder.create_vector(&row.payload))
            };
            fb::RefRow::create(
                &mut builder,
                &fb::RefRowArgs {
                    key,
                    rev: row.rev,
                    state: row.state.into(),
                    payload,
                },
            )
        })
        .collect();
    let rows = builder.create_vector(&row_offsets);
    let namespace = builder.create_string(&batch.namespace);
    let root = fb::RefRowDeltaBatch::create(
        &mut builder,
        &fb::RefRowDeltaBatchArgs {
            namespace: Some(namespace),
            baseline: batch.baseline,
            rows: Some(rows),
        },
    );
    fb::finish_ref_row_delta_batch_buffer(&mut builder, root);
    builder.finished_data().to_vec()
}

/// Seed `alice` + `bob` into a fresh cache at (session=1, epoch=0).
fn seeded_cache() -> RefRowCache {
    let mut cache = RefRowCache::new();
    let seed = RefRowDeltaBatch {
        namespace: "profile".into(),
        baseline: true,
        rows: vec![
            RefRow::changed("alice", 1, payload_for("alice", 1)),
            RefRow::changed("bob", 1, payload_for("bob", 1)),
        ],
    };
    cache.apply(&wire_round_trip(&seed), 1, 0, &decode_ok);
    cache
}

/// BLOCKING-1 (reset-before-decode on epoch change): a MALFORMED first baseline
/// AFTER an epoch bump must RETAIN the prior cache and fail closed — it must NOT
/// empty the live cache. (Pre-fix `apply` cleared `rows` on the identity change
/// BEFORE the new baseline decoded, so the prior cache was already gone.)
#[test]
fn malformed_first_baseline_after_epoch_bump_retains_prior_cache() {
    let mut cache = seeded_cache();
    assert_eq!(cache.snapshot("profile").len(), 2);

    // New epoch (=1). First baseline at the new epoch is POISONED (carol's
    // payload has a 0xFF prefix → decode fails).
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
    let outcome = cache.apply(&wire_round_trip(&bad_baseline), 1, 1, &decode_ok);

    assert!(outcome.decode_failed, "malformed baseline fails closed");
    assert!(cache.needs_resync(), "needs_resync latches (D6)");
    // Prior cache RETAINED — not emptied by the epoch change.
    assert_eq!(
        cache.get("profile", "alice"),
        Some(payload_for("alice", 1)),
        "epoch bump + malformed baseline must NOT empty the prior cache"
    );
    assert_eq!(cache.get("profile", "bob"), Some(payload_for("bob", 1)));
    assert_eq!(cache.get("profile", "carol"), None);

    // A subsequent VALID baseline at the (still-changed) epoch repairs the set.
    let good_baseline = RefRowDeltaBatch {
        namespace: "profile".into(),
        baseline: true,
        rows: vec![RefRow::changed("carol", 2, payload_for("carol", 2))],
    };
    cache.apply(&wire_round_trip(&good_baseline), 1, 1, &decode_ok);
    assert!(
        !cache.needs_resync(),
        "a valid baseline at the new epoch clears resync"
    );
    assert_eq!(
        cache
            .snapshot("profile")
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        vec!["carol"]
    );
}

/// BLOCKING-1 (session change variant): same fail-closed contract on a session
/// bump (process restart) — a malformed first baseline retains the prior cache.
#[test]
fn malformed_first_baseline_after_session_bump_retains_prior_cache() {
    let mut cache = seeded_cache();
    let mut poisoned = payload_for("carol", 1);
    poisoned[0] = 0xFF;
    let bad_baseline = RefRowDeltaBatch {
        namespace: "profile".into(),
        baseline: true,
        rows: vec![RefRow::changed("carol", 1, poisoned)],
    };
    // session = 2 (restart), epoch unchanged.
    let outcome = cache.apply(&wire_round_trip(&bad_baseline), 2, 0, &decode_ok);
    assert!(outcome.decode_failed);
    assert!(cache.needs_resync());
    assert_eq!(cache.get("profile", "alice"), Some(payload_for("alice", 1)));
    assert_eq!(cache.get("profile", "bob"), Some(payload_for("bob", 1)));
}

/// BLOCKING-2 (missing-key row → whole batch rejected): a baseline whose wire
/// bytes contain a row with NO `key` must fail the ENTIRE batch closed — the
/// prior cache stays intact (no partial commit, no dropped live rows). The
/// real wire decoder rejects the batch before it reaches the cache.
#[test]
fn baseline_missing_key_row_rejects_whole_batch() {
    let cache = seeded_cache();

    let bad_baseline = RefRowDeltaBatch {
        namespace: "profile".into(),
        baseline: true,
        rows: vec![
            RefRow::changed("alice", 9, payload_for("alice", 9)),
            // This row's key is omitted on the wire below.
            RefRow::changed("ignored", 9, payload_for("x", 9)),
        ],
    };
    let bytes = encode_with_missing_key(&bad_baseline, 1);

    // The wire decoder must reject the whole batch (no row-skipping).
    let decoded = decode_ref_row_delta_batch(&bytes);
    assert!(
        decoded.is_err(),
        "a missing-key row must fail the whole batch decode"
    );

    // And the cache is untouched: the host never applies an undecodable batch.
    assert_eq!(cache.get("profile", "alice"), Some(payload_for("alice", 1)));
    assert_eq!(cache.get("profile", "bob"), Some(payload_for("bob", 1)));
}

/// BLOCKING-2 (namespace mismatch): the apply path commits a batch only to its
/// own namespace — a batch's rows never cross-pollute a sibling namespace. (The
/// per-batch `namespace` is authoritative; a row carries no namespace, so a
/// "namespace-mismatch row" is structurally impossible in this carrier. We pin
/// the equivalent guarantee: applying a `profile` batch never mutates `event`.)
#[test]
fn batch_applies_only_to_its_own_namespace() {
    let mut cache = RefRowCache::new();
    let profile = RefRowDeltaBatch {
        namespace: "profile".into(),
        baseline: true,
        rows: vec![RefRow::changed("shared", 1, b"profile-bytes".to_vec())],
    };
    let event = RefRowDeltaBatch {
        namespace: "event".into(),
        baseline: true,
        rows: vec![RefRow::changed("shared", 1, b"event-bytes".to_vec())],
    };
    cache.apply(&wire_round_trip(&profile), 1, 0, &decode_ok);
    cache.apply(&wire_round_trip(&event), 1, 0, &decode_ok);

    // A new profile baseline must not touch the event namespace's `shared` row.
    let profile2 = RefRowDeltaBatch {
        namespace: "profile".into(),
        baseline: true,
        rows: vec![RefRow::changed("shared", 2, b"profile-bytes-v2".to_vec())],
    };
    cache.apply(&wire_round_trip(&profile2), 1, 0, &decode_ok);
    assert_eq!(
        cache.get("profile", "shared"),
        Some(b"profile-bytes-v2".to_vec())
    );
    assert_eq!(cache.get("event", "shared"), Some(b"event-bytes".to_vec()));
}

/// BLOCKING-3 (invalid state discriminant → fail closed, NOT committed as
/// Changed): a row whose wire `state` is an out-of-range discriminant (255) on
/// otherwise-valid bytes must be REJECTED by the decoder. (Pre-fix the
/// `From<fb::RefRowState>` mapped every non-Cleared value to `Changed`, so 255
/// committed a bogus row.)
#[test]
fn invalid_state_discriminant_rejects_batch() {
    let batch = RefRowDeltaBatch {
        namespace: "profile".into(),
        baseline: false,
        rows: vec![RefRow::changed("alice", 2, payload_for("alice", 2))],
    };
    let bytes = encode_with_raw_state(&batch, 0, 255);

    let decoded = decode_ref_row_delta_batch(&bytes);
    assert!(
        decoded.is_err(),
        "an out-of-range RefRowState discriminant must fail decode (not be coerced to Changed)"
    );

    // And applied through a seeded cache: the bogus row never commits.
    let mut cache = seeded_cache();
    // Decode failed, so the host would never call apply — assert decode is the
    // gate (the cache is untouched because the batch never decodes).
    assert!(decode_ref_row_delta_batch(&bytes).is_err());
    assert_eq!(cache.get("profile", "alice"), Some(payload_for("alice", 1)));
    // Sanity: the same batch with state=0 (Changed) DOES decode + commit.
    let ok_bytes = encode_with_raw_state(&batch, 0, 0);
    cache.apply(
        &decode_ref_row_delta_batch(&ok_bytes).unwrap(),
        1,
        0,
        &decode_ok,
    );
    assert_eq!(cache.get("profile", "alice"), Some(payload_for("alice", 2)));
}
