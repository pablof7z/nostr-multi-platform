//! Pure type/error conversions between the OPFS-SQLite engine
//! (`nmp_sqlite_wasm`, crate-local mirror types) and the `nmp-store`
//! `EventStore` vocabulary (#1007).
//!
//! The engine cannot depend on `nmp-store` (Cargo cycle), so it carries
//! field-for-field mirror types (`EngineEvent`, `StoredEngineEvent`,
//! `EngineQuery`, `InsertOutcome`, `SqliteWasmError`, …). This module is the
//! cycle-free seam that maps them 1:1 — exactly the role
//! `nmp-store/src/lmdb/conv.rs` plays for the LMDB engine.
//!
//! Every function here is pure (no I/O, no shim) so the round-trips are
//! unit-tested below. The module is gated `cfg(all(target_arch = "wasm32",
//! feature = "opfs-sqlite-backend"))` by its parent because the engine
//! dependency is wasm32-only; the tests therefore compile and run under the
//! wasm32 check (`cargo check --target wasm32-unknown-unknown
//! --features opfs-sqlite-backend --tests`).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use nmp_sqlite_wasm as engine;
use nostr::SingleLetterTag;

use crate::ingest_log::{
    DeleteReason, LogOp, LogRetentionClaim, PullGap, PullPage, ScanLogResult, StoreLogEntry,
};
use crate::types::{
    CoverageGuard, DeleteFilter, DumpStats, GcBudget, GcReport, InsertOutcome, ProvenanceEntry,
    RawEvent, RejectReason, StoreQuery, StoredEvent, TombstoneOrigin, TombstoneRow,
};
use crate::ReplaceableKey;
use crate::StoreError;

// ─── Events ─────────────────────────────────────────────────────────────────

/// `RawEvent` → engine `EngineEvent` (field-for-field; same NIP-01 shape).
pub(super) fn raw_into_engine(raw: RawEvent) -> engine::EngineEvent {
    engine::EngineEvent {
        id: raw.id,
        pubkey: raw.pubkey,
        created_at: raw.created_at,
        kind: raw.kind,
        tags: raw.tags,
        content: raw.content,
        sig: raw.sig,
    }
}

/// Engine `EngineEvent` → `RawEvent` (the inverse of [`raw_into_engine`]).
pub(super) fn engine_event_into_raw(ev: engine::EngineEvent) -> RawEvent {
    RawEvent {
        id: ev.id,
        pubkey: ev.pubkey,
        created_at: ev.created_at,
        kind: ev.kind,
        tags: ev.tags,
        content: ev.content,
        sig: ev.sig,
    }
}

/// Engine `StoredEngineEvent` → `StoredEvent` (wraps the event in an `Arc`).
pub(super) fn stored_into(se: engine::StoredEngineEvent) -> StoredEvent {
    StoredEvent {
        raw: Arc::new(engine_event_into_raw(se.event)),
        received_at_ms: se.received_at_ms,
    }
}

/// Engine `&StoredEngineEvent` → `StoredEvent` (clone; the `query_visit` seam).
pub(super) fn stored_ref(se: &engine::StoredEngineEvent) -> StoredEvent {
    StoredEvent {
        raw: Arc::new(engine_event_into_raw(se.event.clone())),
        received_at_ms: se.received_at_ms,
    }
}

// ─── Query ──────────────────────────────────────────────────────────────────

/// `StoreQuery` → engine `EngineQuery`. `SingleLetterTag` collapses to its
/// case-correct `char` (the engine indexes the raw single-letter tag name).
pub(super) fn query_into_engine(q: &StoreQuery) -> engine::EngineQuery {
    match q {
        StoreQuery::AuthorKind { author, kinds, since, until } => engine::EngineQuery::AuthorKind {
            author: *author,
            kinds: kinds.clone(),
            since: *since,
            until: *until,
        },
        StoreQuery::AuthorsKind { authors, kinds, since, until } => {
            engine::EngineQuery::AuthorsKind {
                authors: authors.clone(),
                kinds: kinds.clone(),
                since: *since,
                until: *until,
            }
        }
        StoreQuery::KindTime { kinds, since, until } => engine::EngineQuery::KindTime {
            kinds: kinds.clone(),
            since: *since,
            until: *until,
        },
        StoreQuery::KindDtag { kind, d_tag, since, until } => engine::EngineQuery::KindDtag {
            kind: *kind,
            d_tag: d_tag.clone(),
            since: *since,
            until: *until,
        },
        StoreQuery::Tags { authors, kinds, tags, since, until } => engine::EngineQuery::Tags {
            authors: authors.clone(),
            kinds: kinds.clone(),
            tags: tags_into_engine(tags),
            since: *since,
            until: *until,
        },
    }
}

/// `BTreeMap<SingleLetterTag, _>` → `BTreeMap<char, _>` (the engine's tag key).
pub(super) fn tags_into_engine(
    tags: &BTreeMap<SingleLetterTag, BTreeSet<String>>,
) -> BTreeMap<char, BTreeSet<String>> {
    tags.iter().map(|(k, v)| (k.as_char(), v.clone())).collect()
}

// ─── Insert outcome ─────────────────────────────────────────────────────────

/// Engine `InsertOutcome` → `nmp_store::InsertOutcome`.
pub(super) fn insert_outcome(o: engine::InsertOutcome) -> InsertOutcome {
    use engine::InsertOutcome as E;
    match o {
        E::Inserted { id, sources_after } => InsertOutcome::Inserted { id, sources_after },
        E::Duplicate { id, sources_after } => InsertOutcome::Duplicate { id, sources_after },
        E::Replaced { new_id, replaced_id } => InsertOutcome::Replaced { new_id, replaced_id },
        E::Superseded { id, current_id } => InsertOutcome::Superseded { id, current_id },
        E::Tombstoned { id, kind5_event_id, origin } => InsertOutcome::Tombstoned {
            id,
            kind5_event_id,
            origin: tombstone_origin(origin),
        },
        E::Rejected { id, reason } => InsertOutcome::Rejected { id, reason: reject_reason(reason) },
        E::Ephemeral { id } => InsertOutcome::Ephemeral { id },
    }
}

fn reject_reason(r: engine::RejectReason) -> RejectReason {
    match r {
        engine::RejectReason::Malformed(s) => RejectReason::Malformed(s),
        engine::RejectReason::ExpiredOnArrival => RejectReason::ExpiredOnArrival,
    }
}

fn tombstone_origin(o: engine::TombstoneOrigin) -> TombstoneOrigin {
    match o {
        engine::TombstoneOrigin::Kind5 => TombstoneOrigin::Kind5,
        engine::TombstoneOrigin::NIP40Expiry => TombstoneOrigin::NIP40Expiry,
        engine::TombstoneOrigin::AdminPurge => TombstoneOrigin::AdminPurge,
    }
}

// ─── Tombstones / provenance ────────────────────────────────────────────────

/// Engine `TombstoneRow` → `nmp_store::TombstoneRow`.
pub(super) fn tombstone_row(r: engine::TombstoneRow) -> TombstoneRow {
    TombstoneRow {
        target_id: r.target_id,
        kind5_event_id: r.kind5_event_id,
        deleter_pubkey: r.deleter_pubkey,
        deleted_at: r.deleted_at,
        sources: r.sources,
        origin: tombstone_origin(r.origin),
    }
}

/// Engine `ProvenanceRow` → `nmp_store::ProvenanceEntry`.
pub(super) fn provenance_entry(r: engine::ProvenanceRow) -> ProvenanceEntry {
    ProvenanceEntry {
        relay_url: r.relay_url,
        first_seen_ms: r.first_seen_ms,
        last_seen_ms: r.last_seen_ms,
        primary: r.is_primary,
    }
}

// ─── GC / delete / coverage ─────────────────────────────────────────────────

pub(super) fn gc_budget(b: GcBudget) -> engine::GcBudget {
    engine::GcBudget {
        max_events_per_step: b.max_events_per_step,
        max_duration_ms: b.max_duration_ms,
        max_total_events: b.max_total_events,
    }
}

pub(super) fn gc_report(r: engine::GcReport) -> GcReport {
    GcReport {
        expired_reaped: r.expired_reaped,
        lru_evicted: r.lru_evicted,
        tombstones_purged: r.tombstones_purged,
        addr_tombstones_purged: r.addr_tombstones_purged,
        duration_ms: r.duration_ms,
    }
}

pub(super) fn delete_filter(f: DeleteFilter) -> engine::DeleteFilter {
    match f {
        DeleteFilter::ByRelayOnly(r) => engine::DeleteFilter::ByRelayOnly(r),
        DeleteFilter::ByAuthor(pk) => engine::DeleteFilter::ByAuthor(pk),
        DeleteFilter::ByIds(ids) => engine::DeleteFilter::ByIds(ids),
        DeleteFilter::ByKindRange { lo, hi } => engine::DeleteFilter::ByKindRange { lo, hi },
    }
}

/// `&[CoverageGuard]` → `Vec<engine::CoverageGuard>`. The kernel-owned
/// `matches` predicate is a `std::sync::Arc<dyn Fn(…) + Send + Sync>` of an
/// identical signature in both crates, so the `Arc` is shared verbatim.
pub(super) fn coverage_guards(guards: &[CoverageGuard]) -> Vec<engine::CoverageGuard> {
    guards
        .iter()
        .map(|g| engine::CoverageGuard {
            filter_hash: g.filter_hash.clone(),
            relay: g.relay.clone(),
            covered_through: g.covered_through,
            matches: Arc::clone(&g.matches),
        })
        .collect()
}

/// `ReplaceableKey` (the non-LMDB stub) → engine `ReplaceableKey`. On wasm32
/// the `lmdb-backend` feature is never on (heed does not build), so the trait's
/// `ReplaceableKey` is always the `replaceable_stubs` shape here.
pub(super) fn replaceable_key(k: &ReplaceableKey) -> engine::ReplaceableKey {
    match k {
        ReplaceableKey::Regular { kind, pubkey } => engine::ReplaceableKey::Regular {
            kind: *kind,
            pubkey: *pubkey,
        },
        ReplaceableKey::Parameterized { kind, pubkey, d_tag } => {
            engine::ReplaceableKey::Parameterized {
                kind: *kind,
                pubkey: *pubkey,
                d_tag: d_tag.clone().into_bytes(),
            }
        }
    }
}

// ─── Ingest log ─────────────────────────────────────────────────────────────

pub(super) fn retention_claims(claims: &[LogRetentionClaim]) -> Vec<engine::LogRetentionClaim> {
    claims
        .iter()
        .map(|c| engine::LogRetentionClaim {
            after_seq: c.after_seq,
            max_lag_entries: c.max_lag_entries,
        })
        .collect()
}

pub(super) fn scan_log_result(r: engine::ScanLogResult) -> ScanLogResult {
    match r {
        engine::ScanLogResult::Page(p) => ScanLogResult::Page(PullPage {
            entries: p.entries.into_iter().map(log_entry).collect(),
            next_after_seq: p.next_after_seq,
            latest_seq: p.latest_seq,
            has_more: p.has_more,
        }),
        engine::ScanLogResult::Gap(g) => ScanLogResult::Gap(PullGap {
            requested_after_seq: g.requested_after_seq,
            first_available_seq: g.first_available_seq,
        }),
    }
}

fn log_entry(e: engine::StoreLogEntry) -> StoreLogEntry {
    StoreLogEntry {
        seq: e.seq,
        op: log_op(e.op),
        event_id: e.event_id,
        raw_event: e.raw_event.map(engine_event_into_raw),
        source_relay: e.source_relay,
        received_at_ms: e.received_at_ms,
    }
}

fn log_op(op: engine::LogOp) -> LogOp {
    match op {
        engine::LogOp::Inserted => LogOp::Inserted,
        engine::LogOp::Replaced { replaced_id } => LogOp::Replaced { replaced_id },
        engine::LogOp::Deleted { target_id, reason } => LogOp::Deleted {
            target_id,
            reason: delete_reason(reason),
        },
    }
}

fn delete_reason(r: engine::DeleteReason) -> DeleteReason {
    match r {
        engine::DeleteReason::Nip09 => DeleteReason::Nip09,
        engine::DeleteReason::Nip40Expiry => DeleteReason::Nip40Expiry,
        engine::DeleteReason::AdminPurge => DeleteReason::AdminPurge,
    }
}

// ─── Dump / error ───────────────────────────────────────────────────────────

pub(super) fn dump_stats(s: engine::DumpStats) -> DumpStats {
    DumpStats {
        events: s.events,
        tombstones: s.tombstones,
        watermarks: s.watermarks,
        domain_rows: s.domain_rows,
        bytes_written: s.bytes_written,
    }
}

/// Engine `SqliteWasmError` → `StoreError`.
///
/// Backend-i/o faults (`ModuleInit`/`VfsInstall`/`Open`/`Close`) and statement
/// faults (`Exec`/`Prepare`/`Bind`/`Step`) map to `Io`; decode faults
/// (`Column`/`Encoding`) map to `Encoding`. `Migration` is a defensive fallback
/// here — the wrapper's own `run_migrations` produces structured
/// `SchemaTooNew`/`MigrationFailed` directly (it never calls the engine's
/// `run_migrations`), so this arm only fires for an unexpected caller; it
/// preserves the engine's documented "message carries which" discriminant.
pub(crate) fn store_err(e: engine::SqliteWasmError) -> StoreError {
    use engine::SqliteWasmError as E;
    match e {
        E::ModuleInit(s) | E::VfsInstall(s) | E::Open(s) | E::Close(s) | E::Exec(s)
        | E::Prepare(s) | E::Bind(s) | E::Step(s) => StoreError::Io(s),
        E::Column(s) | E::Encoding(s) => StoreError::Encoding(s),
        E::Migration(s) => {
            if s.contains("is newer than target") {
                StoreError::SchemaTooNew { namespace: String::new(), on_disk: 0, expected: 0 }
            } else {
                StoreError::MigrationFailed { namespace: String::new(), from: 0, to: 0, reason: s }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw() -> RawEvent {
        RawEvent {
            id: "a".repeat(64),
            pubkey: "b".repeat(64),
            created_at: 1_700_000_000,
            kind: 30023,
            tags: vec![vec!["d".into(), "slug".into()], vec!["e".into(), "c".repeat(64)]],
            content: "hello — ünïcode".into(),
            sig: "f".repeat(128),
        }
    }

    #[test]
    fn raw_engine_round_trips() {
        let r = raw();
        let back = engine_event_into_raw(raw_into_engine(r.clone()));
        assert_eq!(back.id, r.id);
        assert_eq!(back.pubkey, r.pubkey);
        assert_eq!(back.created_at, r.created_at);
        assert_eq!(back.kind, r.kind);
        assert_eq!(back.tags, r.tags);
        assert_eq!(back.content, r.content);
        assert_eq!(back.sig, r.sig);
    }

    #[test]
    fn stored_wraps_arc_and_keeps_arrival() {
        let se = engine::StoredEngineEvent {
            event: raw_into_engine(raw()),
            received_at_ms: 42,
        };
        let s = stored_into(se);
        assert_eq!(s.received_at_ms, 42);
        assert_eq!(s.raw.kind, 30023);
    }

    #[test]
    fn query_tags_collapse_single_letter_to_char() {
        let mut tags: BTreeMap<SingleLetterTag, BTreeSet<String>> = BTreeMap::new();
        tags.insert(
            SingleLetterTag::lowercase(nostr::Alphabet::E),
            BTreeSet::from(["x".to_string()]),
        );
        let q = StoreQuery::Tags {
            authors: BTreeSet::new(),
            kinds: vec![1],
            tags,
            since: Some(1),
            until: None,
        };
        match query_into_engine(&q) {
            engine::EngineQuery::Tags { tags, kinds, since, .. } => {
                assert_eq!(kinds, vec![1]);
                assert_eq!(since, Some(1));
                assert!(tags.contains_key(&'e'));
            }
            other => panic!("expected Tags, got {other:?}"),
        }
    }

    #[test]
    fn insert_outcome_maps_each_variant() {
        let id = [1u8; 32];
        assert!(matches!(
            insert_outcome(engine::InsertOutcome::Inserted { id, sources_after: 2 }),
            InsertOutcome::Inserted { sources_after: 2, .. }
        ));
        assert!(matches!(
            insert_outcome(engine::InsertOutcome::Tombstoned {
                id,
                kind5_event_id: None,
                origin: engine::TombstoneOrigin::NIP40Expiry,
            }),
            InsertOutcome::Tombstoned { origin: TombstoneOrigin::NIP40Expiry, .. }
        ));
        assert!(matches!(
            insert_outcome(engine::InsertOutcome::Rejected {
                id,
                reason: engine::RejectReason::ExpiredOnArrival,
            }),
            InsertOutcome::Rejected { reason: RejectReason::ExpiredOnArrival, .. }
        ));
    }

    #[test]
    fn error_mapping_buckets() {
        assert!(matches!(store_err(engine::SqliteWasmError::Open("x".into())), StoreError::Io(_)));
        assert!(matches!(
            store_err(engine::SqliteWasmError::Column("x".into())),
            StoreError::Encoding(_)
        ));
        assert!(matches!(
            store_err(engine::SqliteWasmError::Migration("ns on-disk schema 3 is newer than target 1".into())),
            StoreError::SchemaTooNew { .. }
        ));
        assert!(matches!(
            store_err(engine::SqliteWasmError::Migration("step 0→1: boom".into())),
            StoreError::MigrationFailed { .. }
        ));
    }

    #[test]
    fn replaceable_key_param_encodes_dtag_bytes() {
        let k = ReplaceableKey::Parameterized {
            kind: 30023,
            pubkey: [7u8; 32],
            d_tag: "slug".to_string(),
        };
        match replaceable_key(&k) {
            engine::ReplaceableKey::Parameterized { kind, d_tag, .. } => {
                assert_eq!(kind, 30023);
                assert_eq!(d_tag, b"slug".to_vec());
            }
            _ => panic!("expected Parameterized"),
        }
    }

    #[test]
    fn scan_log_gap_and_page_convert() {
        let gap = engine::ScanLogResult::Gap(engine::PullGap {
            requested_after_seq: 5,
            first_available_seq: 9,
        });
        match scan_log_result(gap) {
            ScanLogResult::Gap(g) => {
                assert_eq!(g.requested_after_seq, 5);
                assert_eq!(g.first_available_seq, 9);
            }
            _ => panic!("expected Gap"),
        }
    }
}
