//! LMDB sub-database for the generic e-tag reference-counter sidecar (#2512).
//!
//! Sub-db: `nmp-interaction-counters` (historical name; kept for on-disk
//! continuity — the storage layer is now a noun-free reference counter).
//! Key encoding: `target_event_id(32 bytes) || bucket_discriminant(1 byte)`
//! Value encoding: `count(8 bytes, big-endian u64)`
//!
//! # Noun-free (issue #2512)
//!
//! This module assigns NO meaning to the bucket discriminant. The protocol-aware
//! classification (which kinds count, which NIP-10 marker picks the target, and
//! what a bucket means) lives in `nmp-relations` (Layer 4) and is injected as the
//! opaque [`ReferenceClassifyFn`] via
//! `EventStore::install_reference_counter_classifier`. The store just runs the
//! closure at ingest and increments/decrements the bucket it returns.
//!
//! # Design constraints
//!
//! D4: only insert/delete/gc write this sub-db — no concurrent writers.
//! ADR-0011: writes happen inside the SAME `RwTxn` as the triggering event
//!           write, so the counter always reflects actual stored events.
//! D8: no timers, no polling, no wall-clock reads.
//!
//! # Schema version
//!
//! Keyed `b"nmp-interaction-counters"` in `nmp-domain-versions`. Absent on
//! first open → write version=1 and use. If version==1 → use. If version>1
//! → the `interaction_counters_usable` flag on `Inner` is set false and all
//! reads return an empty [`TargetReferenceCounts`] (forward-compat safeguard
//! — unknown schema is not a fatal error).

#![cfg(feature = "lmdb-backend")]

use std::sync::{Arc, RwLock};

use heed::types::Bytes;
use heed::{Database, RwTxn};

use super::Inner;
use crate::reference_counts::{ReferenceClassifyFn, TargetReferenceCounts};
use crate::types::EventId;
use crate::StoreError;

/// Schema version key in `nmp-domain-versions`.
pub(super) const SCHEMA_KEY: &[u8] = b"nmp-interaction-counters";

/// Current schema version.
const SCHEMA_VERSION: u32 = 1;

// ─── Key encoding ─────────────────────────────────────────────────────────────

/// Build the 33-byte LMDB key: `target_id(32) || bucket(1)`.
fn counter_key(target_id: &[u8; 32], bucket: u8) -> [u8; 33] {
    let mut key = [0u8; 33];
    key[..32].copy_from_slice(target_id);
    key[32] = bucket;
    key
}

// ─── Schema init ──────────────────────────────────────────────────────────────

/// Called from `open.rs::open_impl` after the sub-db is created.
///
/// Returns `true` if the schema is usable (version == 1), `false` if a
/// future unknown version is present (the caller sets
/// `inner.interaction_counters_usable = false`).
pub(super) fn init_schema(
    env: &heed::Env,
    domain_versions: Database<Bytes, Bytes>,
) -> Result<bool, StoreError> {
    let txn = env
        .read_txn()
        .map_err(|e| StoreError::Io(format!("ic schema read_txn: {e}")))?;
    match domain_versions
        .get(&txn, SCHEMA_KEY)
        .map_err(|e| StoreError::Io(format!("ic schema get: {e}")))?
    {
        None => {
            // First open — write version=1.
            drop(txn);
            let mut wtxn = env
                .write_txn()
                .map_err(|e| StoreError::Io(format!("ic schema write_txn: {e}")))?;
            domain_versions
                .put(&mut wtxn, SCHEMA_KEY, &SCHEMA_VERSION.to_be_bytes())
                .map_err(|e| StoreError::Io(format!("ic schema put: {e}")))?;
            wtxn.commit()
                .map_err(|e| StoreError::Io(format!("ic schema commit: {e}")))?;
            Ok(true)
        }
        Some(v) => {
            let stored = v
                .first_chunk::<4>()
                .map(|b| u32::from_be_bytes(*b))
                .unwrap_or(0);
            Ok(stored == SCHEMA_VERSION)
        }
    }
}

// ─── Write helpers ────────────────────────────────────────────────────────────

/// Decode hex target-id, read current count, saturating_add(1), write back.
///
/// Silently skips malformed hex (non-32-byte decode) — parity with the rest
/// of the store which skips unresolvable e-tag values.
fn increment(
    db: Database<Bytes, Bytes>,
    txn: &mut RwTxn,
    target_hex: &str,
    bucket: u8,
) -> Result<(), StoreError> {
    let Some(target_id) = decode_hex32(target_hex) else {
        return Ok(()); // Malformed hex — skip silently.
    };
    let key = counter_key(&target_id, bucket);
    let current = read_u64(db, txn, &key)?;
    let next = current.saturating_add(1);
    db.put(txn, key.as_slice(), &next.to_be_bytes())
        .map_err(|e| StoreError::Io(format!("ic put: {e}")))?;
    Ok(())
}

/// Decode hex target-id, read current count, saturating_sub(1), write or
/// delete (deletes the row when the result would be 0 to avoid storing zeros).
fn decrement(
    db: Database<Bytes, Bytes>,
    txn: &mut RwTxn,
    target_hex: &str,
    bucket: u8,
) -> Result<(), StoreError> {
    let Some(target_id) = decode_hex32(target_hex) else {
        return Ok(());
    };
    let key = counter_key(&target_id, bucket);
    let current = read_u64(db, txn, &key)?;
    if current == 0 {
        // Already zero — do not write a zero row; nothing to do.
        return Ok(());
    }
    let next = current - 1;
    if next == 0 {
        db.delete(txn, key.as_slice())
            .map_err(|e| StoreError::Io(format!("ic delete: {e}")))?;
    } else {
        db.put(txn, key.as_slice(), &next.to_be_bytes())
            .map_err(|e| StoreError::Io(format!("ic put: {e}")))?;
    }
    Ok(())
}

/// Call on every newly stored event (Inserted or first-time store of kind:5).
///
/// Runs the installed reference classifier (if any) and, if the event is a
/// counted reference, increments the bucket for its target. No classifier
/// installed → no-op.
pub(super) fn apply_on_insert(
    db: Database<Bytes, Bytes>,
    classifier: &RwLock<Option<Arc<ReferenceClassifyFn>>>,
    txn: &mut RwTxn,
    kind: u32,
    tags: &[Vec<String>],
) -> Result<(), StoreError> {
    if let Some((bucket, target_hex)) = classify(classifier, kind, tags) {
        increment(db, txn, &target_hex, bucket)?;
    }
    Ok(())
}

/// Call when an event is being removed (kind:5 / delete_by_filter / GC).
///
/// Runs the installed classifier and decrements the matching bucket.
pub(super) fn apply_on_remove(
    db: Database<Bytes, Bytes>,
    classifier: &RwLock<Option<Arc<ReferenceClassifyFn>>>,
    txn: &mut RwTxn,
    kind: u32,
    tags: &[Vec<String>],
) -> Result<(), StoreError> {
    if let Some((bucket, target_hex)) = classify(classifier, kind, tags) {
        decrement(db, txn, &target_hex, bucket)?;
    }
    Ok(())
}

/// Run the installed classifier closure, returning `(bucket_discriminant,
/// target_hex)` or `None`. A poisoned lock / absent classifier → `None`.
fn classify(
    classifier: &RwLock<Option<Arc<ReferenceClassifyFn>>>,
    kind: u32,
    tags: &[Vec<String>],
) -> Option<(u8, String)> {
    let guard = classifier.read().ok()?;
    let classify = guard.as_ref()?;
    classify(kind, tags).map(|(bucket, target)| (bucket.discriminant(), target))
}

// ─── Read helpers ─────────────────────────────────────────────────────────────

/// Read all bucket counts for `target_id` via a prefix scan over
/// `target_id(32) || *`.
///
/// If `inner.interaction_counters_usable` is false (unknown schema), returns an
/// empty [`TargetReferenceCounts`].
pub(crate) fn reference_counts(
    inner: &Arc<Inner>,
    target_id: &EventId,
) -> Result<TargetReferenceCounts, StoreError> {
    let mut counts = TargetReferenceCounts::default();
    if !inner.interaction_counters_usable {
        return Ok(counts);
    }
    let txn = inner
        .env
        .read_txn()
        .map_err(|e| StoreError::Io(format!("ic read_txn: {e}")))?;
    for entry in inner
        .interaction_counters
        .prefix_iter(&txn, target_id.as_slice())
        .map_err(|e| StoreError::Io(format!("ic prefix_iter: {e}")))?
    {
        let (k, v) = entry.map_err(|e| StoreError::Io(format!("ic entry: {e}")))?;
        // Key is `target_id(32) || bucket(1)`; the trailing byte is the bucket.
        if let Some(&bucket) = k.get(32) {
            let n = v
                .first_chunk::<8>()
                .map(|b| u64::from_be_bytes(*b))
                .unwrap_or(0);
            counts.set(bucket, n);
        }
    }
    Ok(counts)
}

// ─── Internal helpers ─────────────────────────────────────────────────────────

fn read_u64(
    db: Database<Bytes, Bytes>,
    txn: &heed::RoTxn,
    key: &[u8],
) -> Result<u64, StoreError> {
    match db
        .get(txn, key)
        .map_err(|e| StoreError::Io(format!("ic get: {e}")))?
    {
        // `first_chunk` yields the leading 8 bytes (or `None` if malformed/short),
        // so a truncated value reads as 0 instead of panicking.
        Some(v) => Ok(v
            .first_chunk::<8>()
            .map(|b| u64::from_be_bytes(*b))
            .unwrap_or(0)),
        None => Ok(0),
    }
}

fn decode_hex32(hex: &str) -> Option<[u8; 32]> {
    crate::types::hex_to_event_id(hex)
}
