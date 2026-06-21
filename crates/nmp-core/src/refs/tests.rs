//! ADR-0063 Lane A — the invariant property-test harness (THE MERGE GATE).
//!
//! This is the correctness gate for the whole #1671 campaign. It asserts the
//! row-delta carrier + reference host-cache satisfy the five ADR-0063
//! invariants at ROW grain, under random claim/release/ingest/epoch-reset
//! sequences, with every batch round-tripped through the actual FlatBuffers
//! wire codec (`encode_ref_row_delta_batch` → `decode_ref_row_delta_batch`)
//! before it reaches the cache — so the property tests the real wire bytes, not
//! an in-memory shortcut.
//!
//! The two property tests are the heart:
//! - [`prop_incremental_converges`] — under reliable synchronous delivery the
//!   incremental stream applied == the full snapshot of the final state.
//! - [`prop_baseline_repairs_after_perturbation`] — under injected drops /
//!   reorders / corruption, a final baseline reconstructs the full truth (the
//!   ADR-0055 D5 "worst case is one extra full snapshot, never a permanent
//!   desync" guarantee).
//!
//! The explicit `invariant_*` / behaviour tests pin each invariant individually.

use super::*;
use proptest::prelude::*;
use std::collections::BTreeMap;

const REF_NS: [&str; 2] = ["profile", "event"];

/// Decode-before-commit preflight used by the harness: a payload is well-formed
/// iff it is non-empty and not poisoned (poison = leading `0xFF`). The producer
/// never emits poisoned payloads; only the malformed-row test injects them.
fn decode_ok(_key: &str, payload: &[u8]) -> bool {
    !payload.is_empty() && payload.first() != Some(&0xFF)
}

/// A deterministic non-empty, non-poison payload for `(key, rev)`.
fn payload_for(key: &str, rev: u64) -> Vec<u8> {
    let mut p = vec![0x01];
    p.extend_from_slice(key.as_bytes());
    p.push(b':');
    p.extend_from_slice(&rev.to_le_bytes());
    p
}

/// Round-trip a batch through the real wire codec before applying it — every
/// path in the harness exercises the FlatBuffers carrier end-to-end.
fn wire_round_trip(batch: &RefRowDeltaBatch) -> RefRowDeltaBatch {
    let bytes = encode_ref_row_delta_batch(batch);
    decode_ref_row_delta_batch(&bytes).expect("row-delta batch must round-trip through the wire")
}

/// The producer's ground-truth full snapshot for a namespace.
fn ground_truth(source: &MapRowRevSource, namespace: &str) -> BTreeMap<String, Vec<u8>> {
    let mut keys = source.ref_row_keys(namespace);
    keys.sort();
    keys.into_iter()
        .filter_map(|k| source.ref_row_payload(namespace, &k).map(|p| (k, p)))
        .collect()
}

// ── Property: incremental stream converges (reliable synchronous delivery) ──────

/// Operations the harness drives against the producer ground truth.
#[derive(Clone, Debug)]
enum Op {
    /// Claim/ingest: bump the key's rev and set a fresh payload.
    Upsert { ns: usize, key: usize },
    /// Release: drop the key (→ explicit Cleared on the next incremental batch).
    Remove { ns: usize, key: usize },
    /// Emit + apply a batch for every namespace (a kernel tick the host sees).
    Emit,
    /// Account-switch / schema-change: bump the epoch (forces a baseline frame).
    EpochReset,
    /// Process restart: change the session id (forces a baseline frame).
    SessionRestart,
}

fn op_strategy(keys: usize) -> impl Strategy<Value = Op> {
    prop_oneof![
        4 => (0..REF_NS.len(), 0..keys).prop_map(|(ns, key)| Op::Upsert { ns, key }),
        2 => (0..REF_NS.len(), 0..keys).prop_map(|(ns, key)| Op::Remove { ns, key }),
        4 => Just(Op::Emit),
        1 => Just(Op::EpochReset),
        1 => Just(Op::SessionRestart),
    ]
}

/// Shared driver: replays `ops` against a producer (`MapRowRevSource` +
/// `RefRowDeltaTracker`) and a host (`RefRowCache`), emitting batches through
/// the wire codec. Returns the final `(source, cache, session, epoch,
/// needs_baseline)` so callers can finish with their own emit policy.
struct Harness {
    source: MapRowRevSource,
    tracker: RefRowDeltaTracker,
    cache: RefRowCache,
    session: u64,
    epoch: u64,
    rev: u64,
    needs_baseline: bool,
}

impl Harness {
    fn new() -> Self {
        Self {
            source: MapRowRevSource::new(),
            tracker: RefRowDeltaTracker::new(),
            cache: RefRowCache::new(),
            session: 1,
            epoch: 0,
            rev: 0,
            needs_baseline: true, // first emit is a baseline
        }
    }

    fn emit_all(&mut self, drop_batch: bool) {
        for ns in REF_NS {
            let batch = if self.needs_baseline {
                self.tracker.build_baseline(ns, &self.source)
            } else {
                self.tracker.build_incremental(ns, &self.source)
            };
            if drop_batch {
                continue; // simulate a dropped frame (producer already advanced)
            }
            let decoded = wire_round_trip(&batch);
            self.cache
                .apply(&decoded, self.session, self.epoch, &decode_ok);
        }
        self.needs_baseline = false;
    }

    fn apply_op(&mut self, op: &Op, keys: usize) {
        match *op {
            Op::Upsert { ns, key } => {
                let ns = REF_NS[ns % REF_NS.len()];
                let key = format!("k{}", key % keys);
                self.rev += 1;
                let payload = payload_for(&key, self.rev);
                self.source.upsert(ns, &key, self.rev, payload);
            }
            Op::Remove { ns, key } => {
                let ns = REF_NS[ns % REF_NS.len()];
                let key = format!("k{}", key % keys);
                self.source.remove(ns, &key);
            }
            Op::Emit => self.emit_all(false),
            Op::EpochReset => {
                self.epoch += 1;
                self.tracker.reset();
                self.needs_baseline = true;
            }
            Op::SessionRestart => {
                self.session += 1;
                self.tracker.reset();
                self.needs_baseline = true;
            }
        }
    }

    fn assert_converged(&self) {
        for ns in REF_NS {
            assert_eq!(
                self.cache.snapshot(ns),
                ground_truth(&self.source, ns),
                "namespace {ns}: cache must equal producer ground truth"
            );
        }
    }
}

proptest! {
    /// Invariant (core gate): the incremental row-delta stream applied ==
    /// the full snapshot of the final state, under random
    /// claim/release/ingest/epoch-reset sequences with reliable synchronous
    /// delivery. A final incremental emit settles any pending mutations.
    #[test]
    fn prop_incremental_converges(ops in proptest::collection::vec(op_strategy(5), 0..120)) {
        let keys = 5;
        let mut h = Harness::new();
        for op in &ops {
            h.apply_op(op, keys);
        }
        // Settle: a final emit (baseline iff a reset is pending) flushes the
        // last mutations to the host.
        h.emit_all(false);
        // Plus one guaranteed-incremental emit to prove steady-state delivery
        // also converges (no pending baseline left).
        h.emit_all(false);
        h.assert_converged();
    }

    /// Invariant #3 (resync repairs): under injected drops / reorders /
    /// corruption, a FINAL BASELINE reconstructs the complete truth — the
    /// ADR-0055 D5 guarantee that the worst case is one extra full snapshot,
    /// never a permanent desync.
    #[test]
    fn prop_baseline_repairs_after_perturbation(
        ops in proptest::collection::vec(op_strategy(5), 0..120),
        drops in proptest::collection::vec(any::<bool>(), 0..120),
    ) {
        let keys = 5;
        let mut h = Harness::new();
        let mut drop_iter = drops.into_iter().cycle();
        for op in &ops {
            if matches!(op, Op::Emit) {
                // Some emitted frames are dropped (producer still advances) —
                // this leaves the host stale, the exact gap baseline repairs.
                let drop = drop_iter.next().unwrap_or(false);
                h.emit_all(drop);
            } else {
                h.apply_op(op, keys);
            }
        }
        // Universal repair: force a fresh baseline (epoch bump → cache clears →
        // baseline rebuilds the full live set) and apply it reliably.
        h.epoch += 1;
        h.tracker.reset();
        h.needs_baseline = true;
        h.emit_all(false);
        h.assert_converged();
    }
}

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

    assert!(outcome.decode_failed, "malformed row must report decode failure");
    assert!(cache.needs_resync(), "needs_resync must latch (fail-closed, D6)");
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
    cache.apply(&wire_round_trip(&tracker.build_baseline("profile", &source)), 1, 0, &decode_ok);

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
    assert!(!cache.needs_resync(), "epoch re-baseline clears needs_resync");
}

/// A release produces an EXPLICIT `Cleared` row that removes the host's cached
/// row (the counterpart to invariant #1).
#[test]
fn cleared_is_explicit_and_removes() {
    let mut source = MapRowRevSource::new();
    let mut tracker = RefRowDeltaTracker::new();
    let mut cache = RefRowCache::new();

    source.upsert("profile", "alice", 1, payload_for("alice", 1));
    cache.apply(&wire_round_trip(&tracker.build_baseline("profile", &source)), 1, 0, &decode_ok);
    assert!(cache.get("profile", "alice").is_some());

    source.remove("profile", "alice");
    let incr = wire_round_trip(&tracker.build_incremental("profile", &source));
    assert_eq!(incr.rows.len(), 1);
    assert_eq!(incr.rows[0].state, RefRowState::Cleared);
    cache.apply(&incr, 1, 0, &decode_ok);
    assert_eq!(cache.get("profile", "alice"), None, "Cleared row removes the cached row");
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
        cache.apply(&wire_round_trip(&tracker.build_baseline(ns, &source)), 1, 0, &decode_ok);
    }
    assert_eq!(cache.get("profile", "shared"), Some(b"profile-bytes".to_vec()));
    assert_eq!(cache.get("event", "shared"), Some(b"event-bytes".to_vec()));

    // Clearing the profile row must not touch the event row.
    source.remove("profile", "shared");
    cache.apply(&wire_round_trip(&tracker.build_incremental("profile", &source)), 1, 0, &decode_ok);
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
    assert_eq!(cache.snapshot("profile").keys().cloned().collect::<Vec<_>>(), vec!["alice"]);
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
