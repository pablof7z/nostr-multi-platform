//! Pending op store — deferred completion for KP-gated Marmot ops.
//!
//! ## Problem
//!
//! `create_group` / `invite` require all invitees' signed KeyPackage events
//! up front. When a KP is missing from the in-memory cache the op
//! immediately triggers a `key_package_lookup_interest` (the relay fetch
//! goes out) but previously returned a terminal `{"ok":false}` — the op
//! never retried after the KP arrived.
//!
//! ## Solution
//!
//! When `ops::create_group` / `ops::invite` hits `key_package_unavailable`,
//! it stores the original action JSON + `correlation_id` + the set of still-
//! missing pubkey hexes here. The ingest path (kind 30443 arm of
//! `ops::ingest_signed_event_core`, actor thread) calls
//! [`PendingOpsStore::retry_for_pubkey`] after each cache insert; any op
//! whose missing set is now fully covered is re-executed and the terminal
//! verdict is handed back to the caller as a [`RetryOutcome`] so the actor
//! channel can record `Accepted` / `Failed` under the ORIGINAL
//! `correlation_id`.
//!
//! ## Expiry
//!
//! Wall-clock gated on **KP-ingest + snapshot edges** (NO timers, NO polling,
//! compliant with doctrine D8). Any pending op older than
//! [`PENDING_OP_EXPIRY_SECS`] is evicted with a terminal
//! `key_package_unavailable` failure on the next edge that provides a
//! `now_secs` reference. The `check_expired` helper is the single eviction
//! mechanism; it is driven from two call sites:
//!   * the kind:30443 ingest arm of `ops::ingest_signed_event_core`
//!     (so an arriving KP for *another* peer still ages out stale ops), and
//!   * the top of `MarmotProjection::snapshot` (so an op whose KP NEVER
//!     arrives still expires within a tick of its deadline — snapshots are
//!     emitted on every frame-producing actor tick, a dense wall-clock edge).
//! Without the snapshot edge a parked op could hang forever if no further KP
//! events were ever ingested.
//!
//! ## Lifetime / durability
//!
//! The store is **in-memory only** (it lives in `MarmotProjection`'s `Inner`,
//! rebuilt fresh on each process launch). On app relaunch any parked ops
//! vanish silently — there is no terminal `last_op_error` write and no
//! re-park. This is an accepted deferral: a deferred `create_group`/`invite`
//! that was still pending when the process died is simply lost, and the user
//! re-initiates it. Persisting parked ops across launches would require a
//! durable queue keyed by `correlation_id` and is out of scope here.
//!
//! ## Single-flight
//!
//! A second `create_group` or `invite` with the same `(op, sorted invitee
//! pubkeys)` fingerprint while one is pending is REJECTED as a duplicate
//! (returns [`StoreResult::Duplicate`] with the already-pending
//! `correlation_id`). Policy: the first enqueue wins; callers should surface
//! "operation in progress" feedback rather than allow the user to fire
//! concurrent identical creates. This avoids duplicate MLS group creation
//! if the KP arrives while the user re-taps the button.

use std::collections::{HashMap, HashSet};

/// Seconds before a pending op is declared expired and a terminal failure
/// is recorded under its `correlation_id`.
pub const PENDING_OP_EXPIRY_SECS: u64 = 60;

/// One stored pending op. `action_json` is the ORIGINAL serialised
/// [`crate::projection::action::MarmotAction`] envelope (already validated
/// by the `ActionModule::start` gate); re-running it by feeding it back
/// through `ops::dispatch` is safe because `start()` already accepted it.
#[derive(Debug, Clone)]
pub struct PendingOp {
    /// Original action JSON (serialised `MarmotAction`, e.g. `create_group`
    /// / `invite` envelope) to re-dispatch once the missing KPs arrive.
    pub action_json: String,
    /// Correlation id minted by the action registry when the original
    /// dispatch was enqueued. The terminal verdict is recorded under this id.
    pub correlation_id: String,
    /// Hex pubkeys whose KPs must be in cache before this op can run.
    /// Shrinks as KPs arrive; empty means the op is ready to execute.
    pub missing_pubkeys: HashSet<String>,
    /// Wall-clock seconds (from `now_secs` at store time) of when this op
    /// was first parked. Used for expiry gate.
    pub created_at_secs: u64,
    /// Stable fingerprint — `(op_tag, sorted-invitee-hex-joined)` — for
    /// single-flight deduplication. Computed once at store time.
    pub fingerprint: String,
}

/// Outcome of [`PendingOpsStore::retry_for_pubkey`] for one op.
#[derive(Debug)]
pub struct RetryOutcome {
    pub correlation_id: String,
    pub action_json: String,
}

/// Result of attempting to store a new pending op.
#[derive(Debug)]
pub enum StoreResult {
    /// Successfully stored as a new pending op.
    Stored,
    /// An identical op (same fingerprint) is already pending. The
    /// caller should return a "duplicate" pending envelope — no new
    /// `correlation_id` is minted; the host's spinner already shows the
    /// original.
    Duplicate {
        existing_correlation_id: String,
    },
}

/// In-memory store for pending (deferred) Marmot ops.
///
/// Keyed by `correlation_id` for O(1) lookup on expiry eviction.
/// Also maintains a `fingerprint → correlation_id` map for single-flight
/// deduplication.
#[derive(Debug, Default)]
pub struct PendingOpsStore {
    /// `correlation_id → PendingOp`.
    ops: HashMap<String, PendingOp>,
    /// `fingerprint → correlation_id` — single-flight dedup index.
    by_fingerprint: HashMap<String, String>,
}

impl PendingOpsStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Compute the single-flight fingerprint for a `create_group` / `invite`
    /// op given its `op` tag and the sorted hex pubkeys of invitees whose KPs
    /// are missing.
    ///
    /// The fingerprint intentionally only covers the missing pubkeys (not the
    /// full invitee set) — it is the missing set that makes the op blocked, and
    /// adding the same missing invitee to the same group twice while one attempt
    /// is pending would be a duplicate (second attempt can't succeed until the
    /// first receives the KP, at which point the first fires anyway).
    pub fn fingerprint(op_tag: &str, missing_pubkeys_hex: &[String]) -> String {
        let mut sorted = missing_pubkeys_hex.to_vec();
        sorted.sort_unstable();
        format!("{op_tag}:{}", sorted.join(","))
    }

    /// Park a new pending op. Returns [`StoreResult::Stored`] or
    /// [`StoreResult::Duplicate`] when an identical op is already pending.
    ///
    /// `action_json` is the serialised original action (e.g. `create_group`
    /// envelope). `missing_pubkeys_hex` is the set of pubkey hexes whose KPs
    /// the op is blocked on.
    pub fn store(
        &mut self,
        correlation_id: String,
        action_json: String,
        op_tag: &str,
        missing_pubkeys_hex: Vec<String>,
        now_secs: u64,
    ) -> StoreResult {
        let fingerprint = Self::fingerprint(op_tag, &missing_pubkeys_hex);
        if let Some(existing_cid) = self.by_fingerprint.get(&fingerprint) {
            return StoreResult::Duplicate {
                existing_correlation_id: existing_cid.clone(),
            };
        }
        let missing_pubkeys: HashSet<String> = missing_pubkeys_hex.into_iter().collect();
        self.by_fingerprint
            .insert(fingerprint.clone(), correlation_id.clone());
        self.ops.insert(
            correlation_id.clone(),
            PendingOp {
                action_json,
                correlation_id,
                missing_pubkeys,
                created_at_secs: now_secs,
                fingerprint,
            },
        );
        StoreResult::Stored
    }

    /// Mark `pubkey_hex` as resolved: remove it from the `missing_pubkeys` set
    /// of every pending op that was waiting for it. Ops whose missing set
    /// becomes empty are returned as [`RetryOutcome`]s and removed from the
    /// store (they are ready to execute). The caller is responsible for
    /// re-dispatching the action and recording the verdict.
    pub fn retry_for_pubkey(&mut self, pubkey_hex: &str) -> Vec<RetryOutcome> {
        let mut ready = Vec::new();
        // Two-pass: collect ready correlation_ids, then drain them.
        let ready_cids: Vec<String> = self
            .ops
            .values_mut()
            .filter_map(|op| {
                op.missing_pubkeys.remove(pubkey_hex);
                if op.missing_pubkeys.is_empty() {
                    Some(op.correlation_id.clone())
                } else {
                    None
                }
            })
            .collect();
        for cid in ready_cids {
            if let Some(op) = self.ops.remove(&cid) {
                self.by_fingerprint.remove(&op.fingerprint);
                ready.push(RetryOutcome {
                    correlation_id: op.correlation_id,
                    action_json: op.action_json,
                });
            }
        }
        ready
    }

    /// Evict all pending ops whose `created_at_secs + PENDING_OP_EXPIRY_SECS
    /// <= now_secs` and return them as expired [`PendingOp`]s so the caller
    /// can record terminal failures.
    ///
    /// Call on every ingest/snapshot edge (provides `now_secs`). Never spawns
    /// a timer or sleeps (D8).
    pub fn check_expired(&mut self, now_secs: u64) -> Vec<PendingOp> {
        let expired_cids: Vec<String> = self
            .ops
            .values()
            .filter(|op| {
                now_secs >= op.created_at_secs.saturating_add(PENDING_OP_EXPIRY_SECS)
            })
            .map(|op| op.correlation_id.clone())
            .collect();
        let mut expired = Vec::new();
        for cid in expired_cids {
            if let Some(op) = self.ops.remove(&cid) {
                self.by_fingerprint.remove(&op.fingerprint);
                expired.push(op);
            }
        }
        expired
    }

    /// Number of currently parked ops (for snapshot reporting / tests).
    #[must_use]
    pub fn len(&self) -> usize {
        self.ops.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// Iterate over pending ops (immutable; for snapshot building).
    pub fn iter(&self) -> impl Iterator<Item = &PendingOp> {
        self.ops.values()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_and_retry_single_op() {
        let mut store = PendingOpsStore::new();
        let missing = vec!["aabbcc".to_string()];
        let result = store.store(
            "corr-1".to_string(),
            r#"{"op":"create_group","name":"g"}"#.to_string(),
            "create_group",
            missing,
            1_000,
        );
        assert!(matches!(result, StoreResult::Stored));
        assert_eq!(store.len(), 1);

        // Unrelated pubkey arrival — op still pending.
        let ready = store.retry_for_pubkey("ddeeff");
        assert!(ready.is_empty());
        assert_eq!(store.len(), 1);

        // The required pubkey arrives.
        let ready = store.retry_for_pubkey("aabbcc");
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].correlation_id, "corr-1");
        assert_eq!(store.len(), 0); // evicted from store.
    }

    #[test]
    fn duplicate_fingerprint_rejected() {
        let mut store = PendingOpsStore::new();
        let missing = vec!["aabbcc".to_string()];
        store.store(
            "corr-1".to_string(),
            "{}".to_string(),
            "create_group",
            missing.clone(),
            1_000,
        );
        let r = store.store(
            "corr-2".to_string(),
            "{}".to_string(),
            "create_group",
            missing,
            1_001,
        );
        assert!(
            matches!(r, StoreResult::Duplicate { existing_correlation_id } if existing_correlation_id == "corr-1")
        );
        assert_eq!(store.len(), 1, "duplicate must not be stored");
    }

    #[test]
    fn expiry_fires_after_threshold() {
        let mut store = PendingOpsStore::new();
        store.store(
            "corr-exp".to_string(),
            "{}".to_string(),
            "create_group",
            vec!["aabbcc".to_string()],
            1_000,
        );
        // Before threshold.
        let expired = store.check_expired(1_059);
        assert!(expired.is_empty());
        // At threshold.
        let expired = store.check_expired(1_060);
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].correlation_id, "corr-exp");
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn multi_missing_pubkey_waits_for_all() {
        let mut store = PendingOpsStore::new();
        store.store(
            "corr-m".to_string(),
            "{}".to_string(),
            "invite",
            vec!["aa".to_string(), "bb".to_string()],
            1_000,
        );
        // First pubkey arrives — still blocked on "bb".
        let ready = store.retry_for_pubkey("aa");
        assert!(ready.is_empty());
        // Second pubkey arrives — now ready.
        let ready = store.retry_for_pubkey("bb");
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].correlation_id, "corr-m");
    }

    #[test]
    fn fingerprint_uses_sorted_missing_pubkeys() {
        // Order-independent fingerprint.
        let f1 = PendingOpsStore::fingerprint("create_group", &["bb".to_string(), "aa".to_string()]);
        let f2 = PendingOpsStore::fingerprint("create_group", &["aa".to_string(), "bb".to_string()]);
        assert_eq!(f1, f2);
        // Different op tags produce different fingerprints.
        let f3 = PendingOpsStore::fingerprint("invite", &["aa".to_string()]);
        let f4 = PendingOpsStore::fingerprint("create_group", &["aa".to_string()]);
        assert_ne!(f3, f4);
    }
}
