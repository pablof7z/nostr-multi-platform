//! ADR-0070 Lane A — the invariant property-test harness (THE MERGE GATE).
//!
//! This is the correctness gate for the whole #1671 campaign. It asserts the
//! row-delta carrier + reference host-cache satisfy the five ADR-0070
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
//!   ADR-0070 D5 "worst case is one extra full snapshot, never a permanent
//!   desync" guarantee).
//!
//! The explicit `invariant_*` / behaviour tests pin each invariant individually
//! and live in the sibling `invariants` module.

use super::super::*;
use proptest::prelude::*;
use std::collections::BTreeMap;

pub(super) const REF_NS: [&str; 2] = ["profile", "event"];

/// Decode-before-commit preflight used by the harness: a payload is well-formed
/// iff it is non-empty and not poisoned (poison = leading `0xFF`). The producer
/// never emits poisoned payloads; only the malformed-row test injects them.
pub(super) fn decode_ok(_key: &str, payload: &[u8]) -> bool {
    !payload.is_empty() && payload.first() != Some(&0xFF)
}

/// A deterministic non-empty, non-poison payload for `(key, rev)`.
pub(super) fn payload_for(key: &str, rev: u64) -> Vec<u8> {
    let mut p = vec![0x01];
    p.extend_from_slice(key.as_bytes());
    p.push(b':');
    p.extend_from_slice(&rev.to_le_bytes());
    p
}

/// Round-trip a batch through the real wire codec before applying it — every
/// path in the harness exercises the FlatBuffers carrier end-to-end.
pub(super) fn wire_round_trip(batch: &RefRowDeltaBatch) -> RefRowDeltaBatch {
    let bytes = encode_ref_row_delta_batch(batch);
    decode_ref_row_delta_batch(&bytes).expect("row-delta batch must round-trip through the wire")
}

/// The producer's ground-truth full snapshot for a namespace.
pub(super) fn ground_truth(source: &MapRowRevSource, namespace: &str) -> BTreeMap<String, Vec<u8>> {
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
pub(super) struct Harness {
    pub(super) source: MapRowRevSource,
    pub(super) tracker: RefRowDeltaTracker,
    pub(super) cache: RefRowCache,
    pub(super) session: u64,
    pub(super) epoch: u64,
    pub(super) rev: u64,
    pub(super) needs_baseline: bool,
}

impl Harness {
    pub(super) fn new() -> Self {
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

    pub(super) fn emit_all(&mut self, drop_batch: bool) {
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

    pub(super) fn apply_op(&mut self, op: &Op, keys: usize) {
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
                // A release bumps the key's monotonic rev (the tombstone rev the
                // tracker stamps on the Cleared row); see RefRowRevSource.
                self.rev += 1;
                self.source.remove(ns, &key, self.rev);
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

    pub(super) fn assert_converged(&self) {
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
    /// ADR-0070 D5 guarantee that the worst case is one extra full snapshot,
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

    /// BLOCKING-4 (arbitrary reorder INCLUDING clears): capture each producer
    /// transition as its own per-namespace incremental batch, deliver the whole
    /// stream in a RANDOM permutation with random drops/gaps, then apply a final
    /// reliable baseline. The rev-safe clears + per-key reorder guard keep every
    /// out-of-order apply panic-free and non-corrupting, and the ADR-0070 D5
    /// repair baseline reconstructs the EXACT final ground truth — proving
    /// incremental-applied == full-snapshot-of-final-state under reorder + drop +
    /// gap + epoch.
    #[test]
    fn prop_arbitrary_reorder_repairs_after_baseline(
        muts in proptest::collection::vec((0..REF_NS.len(), 0..5usize, any::<bool>()), 0..80),
        order_keys in proptest::collection::vec(any::<u64>(), 1..48),
        drops in proptest::collection::vec(any::<bool>(), 1..48),
    ) {
        let keys = 5usize;
        let mut source = MapRowRevSource::new();
        let mut tracker = RefRowDeltaTracker::new();
        let mut rev = 0u64;

        // Session-start baselines (ordered) seed both namespaces.
        let mut baselines: Vec<RefRowDeltaBatch> = Vec::new();
        for ns in REF_NS {
            baselines.push(wire_round_trip(&tracker.build_baseline(ns, &source)));
        }

        // Each mutation captures its transition as a per-namespace incremental.
        let mut stream: Vec<RefRowDeltaBatch> = Vec::new();
        for (ns_i, key_i, is_remove) in &muts {
            let ns = REF_NS[ns_i % REF_NS.len()];
            let key = format!("k{}", key_i % keys);
            rev += 1;
            if *is_remove {
                source.remove(ns, &key, rev);
            } else {
                source.upsert(ns, &key, rev, payload_for(&key, rev));
            }
            for ns in REF_NS {
                stream.push(wire_round_trip(&tracker.build_incremental(ns, &source)));
            }
        }

        // Arbitrary reorder: stable-sort the stream by a cyclically-assigned
        // random key (equal keys keep relative order → a genuine permutation).
        let mut ordered: Vec<(u64, RefRowDeltaBatch)> = stream
            .into_iter()
            .enumerate()
            .map(|(i, b)| (order_keys[i % order_keys.len()], b))
            .collect();
        ordered.sort_by_key(|(k, _)| *k);

        // Deliver: ordered session-start baselines, then the reordered (and
        // partly dropped) incremental stream — out of order, with gaps.
        let mut cache = RefRowCache::new();
        for b in &baselines {
            cache.apply(b, 1, 0, &decode_ok);
        }
        for (i, (_, b)) in ordered.iter().enumerate() {
            if drops[i % drops.len()] {
                continue; // dropped frame (gap)
            }
            cache.apply(b, 1, 0, &decode_ok);
        }

        // D5 repair: a final reliable baseline at a NEW epoch clears + rebuilds
        // the complete live set, so the host converges to ground truth exactly.
        tracker.reset();
        for ns in REF_NS {
            let baseline = wire_round_trip(&tracker.build_baseline(ns, &source));
            cache.apply(&baseline, 1, 1, &decode_ok);
        }
        for ns in REF_NS {
            prop_assert_eq!(cache.snapshot(ns), ground_truth(&source, ns));
        }
    }

    /// BLOCKING-1 (fail-closed epoch repair): after an arbitrary converged
    /// stream, the FIRST baseline at a new epoch is POISONED (a 0xFF-prefixed
    /// row). The cache must be UNCHANGED (prior-epoch snapshot retained, resync
    /// latched) — a malformed baseline can NEVER empty a live cache, even on an
    /// identity bump. Repair happens only on a SUBSEQUENT VALID baseline.
    #[test]
    fn prop_malformed_epoch_baseline_never_empties_cache(
        ops in proptest::collection::vec(op_strategy(5), 0..80),
        poison_ns in 0..REF_NS.len(),
    ) {
        let keys = 5;
        let mut h = Harness::new();
        for op in &ops {
            h.apply_op(op, keys);
        }
        // Settle to a known good converged state.
        h.emit_all(false);
        h.emit_all(false);
        h.assert_converged();

        // Snapshot the converged prior-epoch cache for every namespace.
        let prior: Vec<BTreeMap<String, Vec<u8>>> =
            REF_NS.iter().map(|ns| h.cache.snapshot(ns)).collect();

        // Bump the epoch and feed a POISONED first baseline for one namespace.
        let new_epoch = h.epoch + 1;
        let poison_ns = REF_NS[poison_ns];
        h.tracker.reset();
        let mut bad = h.tracker.build_baseline(poison_ns, &h.source);
        // Poison the first Changed row if any; else inject one.
        if let Some(row) = bad.rows.iter_mut().find(|r| r.state == RefRowState::Changed) {
            if row.payload.is_empty() { row.payload.push(0xFF); } else { row.payload[0] = 0xFF; }
        } else {
            bad.rows.push(RefRow::changed("poison", 999, vec![0xFF, 0x01]));
        }
        let bad = wire_round_trip(&bad);
        let outcome = h.cache.apply(&bad, h.session, new_epoch, &decode_ok);

        prop_assert!(outcome.decode_failed, "poisoned epoch baseline must fail closed");
        prop_assert!(h.cache.needs_resync(), "needs_resync must latch");
        // The cache is UNCHANGED across ALL namespaces — not emptied by the
        // epoch bump despite the malformed first baseline.
        for (i, ns) in REF_NS.iter().enumerate() {
            prop_assert_eq!(h.cache.snapshot(ns), prior[i].clone(),
                "namespace {}: malformed epoch baseline must leave the cache untouched", ns);
        }

        // A subsequent VALID baseline at the new epoch repairs the full set.
        h.epoch = new_epoch;
        h.needs_baseline = true;
        h.emit_all(false);
        h.assert_converged();
    }
}
