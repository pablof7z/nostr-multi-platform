//! One-time FTS backfill (#1811).
//!
//! Builds the durable inverted index for events already stored before the index
//! existed — and rebuilds it after a `TOKENIZER_VERSION` bump.
//!
//! ## Gate
//!
//! Keyed in `nmp-domain-versions` by `fts_backfill_key()` =
//! `nmp-fts-v<TOKENIZER_VERSION>`. If the key is present the O(store) scan is
//! skipped. Because the key embeds the tokenizer version, a bump produces a NEW
//! gate key whose absence triggers a fresh build; the old version's posting rows
//! are first DROPPED so the rebuilt index is internally consistent (no stale
//! tokens from the prior pipeline).
//!
//! ## Driver
//!
//! Called from `install_search_index_specs` AFTER the spec set is stored on
//! `Inner`, so the backfill runs the SAME extractors + tokenizer the live write
//! path will use. With no installed specs the backfill is a no-op (nothing to
//! extract).

#![cfg(feature = "lmdb-backend")]

use super::{fts_add_event, fts_backfill_key};
use crate::lmdb::Inner;
use crate::types::RawEvent;
use crate::StoreError;

/// Run the one-time backfill if the gate key for the current tokenizer version
/// is absent. Drops any prior FTS rows first (handles a `TOKENIZER_VERSION`
/// bump), then re-indexes every stored event against the installed specs.
pub(in crate::lmdb) fn backfill_fts_index(inner: &Inner) -> Result<(), StoreError> {
    let gate = fts_backfill_key();

    // O(1) gate read.
    {
        let txn = inner
            .env
            .read_txn()
            .map_err(|e| StoreError::Io(format!("fts backfill gate read_txn: {e}")))?;
        if inner
            .domain_versions
            .get(&txn, gate.as_slice())
            .map_err(|e| StoreError::Io(format!("fts backfill gate get: {e}")))?
            .is_some()
        {
            return Ok(());
        }
    }

    // Snapshot the installed specs. With none installed there is nothing to
    // index — still write the gate so we don't re-scan on every open.
    let specs = match inner.fts_specs.read() {
        Ok(g) => g.clone(),
        Err(_) => Vec::new(),
    };

    // Collect indexable kinds across all specs so the scan loads only events a
    // spec actually indexes (never a blind whole-store re-tokenize of every kind
    // — only the kinds some scope cares about).
    let indexable_kinds: std::collections::BTreeSet<u32> =
        specs.iter().flat_map(|s| s.kinds.iter().copied()).collect();

    let mut txn = inner
        .env
        .write_txn()
        .map_err(|e| StoreError::Io(format!("fts backfill write_txn: {e}")))?;

    // Drop any prior FTS rows (tokenizer-version rebuild safety).
    inner
        .fts_postings
        .clear(&mut txn)
        .map_err(|e| StoreError::Io(format!("fts backfill clear postings: {e}")))?;
    inner
        .fts_doc_terms
        .clear(&mut txn)
        .map_err(|e| StoreError::Io(format!("fts backfill clear doc-terms: {e}")))?;
    inner
        .fts_term_stats
        .clear(&mut txn)
        .map_err(|e| StoreError::Io(format!("fts backfill clear term-stats: {e}")))?;

    if !indexable_kinds.is_empty() {
        // Collect the events to index first (cannot hold the fork's query
        // iterator borrow while writing FTS rows into the same txn).
        let to_index: Vec<RawEvent> = {
            use nostr::prelude::*;
            let kinds: Vec<Kind> = indexable_kinds.iter().map(|k| Kind::from(*k as u16)).collect();
            let filter = Filter::new().kinds(kinds);
            let iter = inner
                .lmdb
                .query(&txn, filter)
                .map_err(|e| StoreError::Io(format!("fts backfill query: {e}")))?;
            let mut out = Vec::new();
            for ev in iter {
                let owned: nostr::Event = ev.into_owned();
                let json = owned
                    .try_as_json()
                    .map_err(|e| StoreError::Encoding(format!("fts backfill json: {e}")))?;
                let raw: RawEvent = serde_json::from_str(&json)
                    .map_err(|e| StoreError::Encoding(format!("fts backfill parse: {e}")))?;
                out.push(raw);
            }
            out
        };

        for raw in &to_index {
            // received_at_ms is unknown for historical events; use created_at*1000.
            // Extractors that read it (rare) degrade gracefully; the index keys on
            // created_at, which is exact.
            fts_add_event(inner, &mut txn, &specs, raw, raw.created_at.saturating_mul(1000))?;
        }
    }

    // Mark the gate done for this tokenizer version.
    inner
        .domain_versions
        .put(&mut txn, gate.as_slice(), &1u32.to_be_bytes())
        .map_err(|e| StoreError::Io(format!("fts backfill gate put: {e}")))?;
    txn.commit()
        .map_err(|e| StoreError::Io(format!("fts backfill commit: {e}")))?;
    Ok(())
}
