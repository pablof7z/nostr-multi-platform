//! Durable LMDB inverted index — the persistent backend of the FTS seam (#1811).
//!
//! Parity target: the in-memory `mem/fts` index. Both run byte-for-byte the same
//! shared tokenizer (`crate::text_search::tokenizer`) at index AND query time, so
//! a token written at ingest is found at query time on either backend.
//!
//! # Sub-databases (registered in `lmdb/open.rs`)
//!
//! * **`nmp-fts-postings`** — the inverted index. Key (packed, ordered,
//!   newest-first):
//!   `scope_discriminant(4 BE) || token_bytes || 0x00 || (!created_at)(8 BE) ||
//!   doc_key(32)` → empty. `!created_at` = `u64::MAX - created_at`, so a forward
//!   cursor over one token's range yields documents newest-first; a `range` from
//!   `scope || prefix` gives a typeahead prefix scan WITHOUT a full token walk.
//! * **`nmp-fts-doc-terms`** — `doc_key(32)` → packed `Vec<token>`. Drives
//!   DOC-KEY-driven removal: every delete path reads the doc's term list and
//!   deletes exactly its postings without re-tokenizing the body.
//! * **`nmp-fts-term-stats`** — `scope_discriminant(4 BE) || token_bytes` →
//!   doc-frequency(8 BE). The planner seeds the scan from the RAREST root term.
//!
//! # Maintenance discipline (mirrors `relay_index`)
//!
//! Every event-write `RwTxn` runs the installed extractors → tokenizer → posting
//! inserts for each spec whose `kinds` contains the event kind, and every removal
//! path (`insert.rs` replace, `insert_kind5.rs`, `delete.rs`, `gc.rs` expiry-reap
//! + LRU-eviction) calls [`fts_remove_by_id`] DOC-KEY-driven, in the SAME txn as
//! the event delete. The store names no protocol concept (D0) — it only runs the
//! opaque extractor + the shared tokenizer.

#![cfg(feature = "lmdb-backend")]

mod backfill;
mod codec;
mod query;

pub(super) use backfill::backfill_fts_index;
pub(super) use query::text_search_visit;

use std::collections::BTreeSet;

use heed::types::Bytes;
use heed::{Database, RwTxn};

use self::codec::{
    decode_doc_terms, encode_doc_terms, postings_decode_tail, postings_key, postings_token_bounds,
    rev_created_at, term_stats_key,
};
use super::Inner;
use crate::text_search::tokenizer::{tokenize, TOKENIZER_VERSION};
use crate::text_search::{CompiledIndexSpec, SearchScopeId};
use crate::types::{EventId, RawEvent, StoredEvent};
use crate::StoreError;

/// `domain_versions` gate key for the one-time FTS backfill. The
/// `TOKENIZER_VERSION` suffix means a tokenizer bump writes a NEW key, so the old
/// index is dropped + rebuilt (see `backfill`).
pub(super) fn fts_backfill_key() -> Vec<u8> {
    format!("nmp-fts-v{TOKENIZER_VERSION}").into_bytes()
}

// ─── Maintenance: add / remove ────────────────────────────────────────────────

/// Build a transient `StoredEvent` so an installed extractor can run on a raw
/// event inside the write txn (extractors are typed against `&StoredEvent`).
fn as_stored(raw: &RawEvent, received_at_ms: u64) -> StoredEvent {
    StoredEvent {
        raw: std::sync::Arc::new(raw.clone()),
        received_at_ms,
    }
}

/// Index `event` into every installed scope whose `kinds` contains the event's
/// kind, inside the caller's event-write `RwTxn`. Idempotent per doc: any prior
/// posting rows for this `doc_key` are removed first (DOC-KEY-driven), then the
/// fresh tokens are written.
///
/// `specs` is the snapshot of installed compiled specs (read once per event
/// write by the caller). No-op when empty.
pub(super) fn fts_add_event(
    inner: &Inner,
    txn: &mut RwTxn,
    specs: &[CompiledIndexSpec],
    event: &RawEvent,
    received_at_ms: u64,
) -> Result<(), StoreError> {
    if specs.is_empty() {
        return Ok(());
    }
    let Some(doc) = event.id_bytes() else {
        return Ok(());
    };
    let kind = event.kind;
    let created_at = event.created_at;

    // Idempotency: drop any prior rows for this doc across all scopes first.
    fts_remove_by_id(inner, txn, &doc)?;

    let stored = as_stored(event, received_at_ms);
    let rev = rev_created_at(created_at);

    // Union of (scope_discriminant, token) written for this doc → doc-terms.
    let mut doc_entries: Vec<(u32, String)> = Vec::new();

    for spec in specs {
        if !spec.kinds.contains(&kind) {
            continue;
        }
        // De-dup tokens within one (doc, scope) so doc-frequency counts each
        // term once and a doc never holds duplicate posting rows.
        let mut tokens: BTreeSet<String> = BTreeSet::new();
        for (_field, text) in (spec.extract)(&stored) {
            for tok in tokenize(&text) {
                tokens.insert(tok);
            }
        }
        for tok in tokens {
            let key = postings_key(spec.scope_id, &tok, rev, &doc);
            inner
                .fts_postings
                .put(txn, &key, &[])
                .map_err(|e| StoreError::Io(format!("fts postings put: {e}")))?;
            term_stats_bump(inner.fts_term_stats, txn, spec.scope_id, &tok, 1)?;
            doc_entries.push((spec.scope_id.discriminant(), tok));
        }
    }

    if !doc_entries.is_empty() {
        let val = encode_doc_terms(&doc_entries);
        inner
            .fts_doc_terms
            .put(txn, doc.as_slice(), &val)
            .map_err(|e| StoreError::Io(format!("fts doc-terms put: {e}")))?;
    }
    Ok(())
}

/// Remove `doc` from the inverted index, DOC-KEY-driven: read the doc's term list
/// from `nmp-fts-doc-terms`, delete exactly its posting rows + decrement
/// term-stats, then drop the doc-terms row. Never re-tokenizes the body — the
/// only input is `doc` (so it works on replace/expiry paths that lack the text).
///
/// No-op when the doc has no doc-terms row (not indexed / FTS not installed).
pub(super) fn fts_remove_by_id(
    inner: &Inner,
    txn: &mut RwTxn,
    doc: &EventId,
) -> Result<(), StoreError> {
    let raw = match inner
        .fts_doc_terms
        .get(txn, doc.as_slice())
        .map_err(|e| StoreError::Io(format!("fts doc-terms get: {e}")))?
    {
        Some(v) => v.to_vec(),
        None => return Ok(()),
    };
    let entries = decode_doc_terms(&raw);
    // We need the doc's rev_created_at to address the exact posting key. It is
    // not stored in doc-terms; instead delete by scanning the token's range for
    // this doc. Posting lists per token are bounded by doc-frequency, so this is
    // O(terms × docs/term) — and per-term ranges are short for real corpora.
    for (scope_disc, token) in &entries {
        let scope = SearchScopeId::new(*scope_disc, "");
        let (lo, hi) = postings_token_bounds(scope, token);
        let range = (
            std::ops::Bound::Included(lo.as_slice()),
            std::ops::Bound::Excluded(hi.as_slice()),
        );
        // Collect the matching key for this doc first (cannot delete while the
        // range cursor borrows the txn immutably).
        let mut to_delete: Vec<Vec<u8>> = Vec::new();
        for entry in inner
            .fts_postings
            .range(txn, &range)
            .map_err(|e| StoreError::Io(format!("fts postings range: {e}")))?
        {
            let (k, _) = entry.map_err(|e| StoreError::Io(format!("fts postings step: {e}")))?;
            if let Some((_rev, kdoc)) = postings_decode_tail(k, token.len()) {
                if &kdoc == doc {
                    to_delete.push(k.to_vec());
                }
            }
        }
        for k in to_delete {
            inner
                .fts_postings
                .delete(txn, &k)
                .map_err(|e| StoreError::Io(format!("fts postings del: {e}")))?;
        }
        term_stats_bump(inner.fts_term_stats, txn, scope, token, -1)?;
    }
    inner
        .fts_doc_terms
        .delete(txn, doc.as_slice())
        .map_err(|e| StoreError::Io(format!("fts doc-terms del: {e}")))?;
    Ok(())
}

/// Adjust the doc-frequency for `(scope, token)` by `delta` (+1 on add, -1 on
/// remove). Removes the row when the count reaches zero so the planner's rarest-
/// term pick and the prefix scan never see dead terms.
fn term_stats_bump(
    db: Database<Bytes, Bytes>,
    txn: &mut RwTxn,
    scope: SearchScopeId,
    token: &str,
    delta: i64,
) -> Result<(), StoreError> {
    let key = term_stats_key(scope, token);
    let cur: u64 = match db
        .get(txn, &key)
        .map_err(|e| StoreError::Io(format!("fts term-stats get: {e}")))?
    {
        Some(v) if v.len() >= 8 => u64::from_be_bytes(v[..8].try_into().unwrap()),
        _ => 0,
    };
    let next = if delta >= 0 {
        cur.saturating_add(delta as u64)
    } else {
        cur.saturating_sub((-delta) as u64)
    };
    if next == 0 {
        db.delete(txn, &key)
            .map_err(|e| StoreError::Io(format!("fts term-stats del: {e}")))?;
    } else {
        db.put(txn, &key, &next.to_be_bytes())
            .map_err(|e| StoreError::Io(format!("fts term-stats put: {e}")))?;
    }
    Ok(())
}

/// Read the doc-frequency for `(scope, token)`. Returns `u64::MAX` for a token
/// with no stats row so an absent term sorts LAST in the rarest-term pick (the
/// query then short-circuits to empty on the missing exact term).
pub(super) fn term_doc_frequency(
    inner: &Inner,
    txn: &heed::RoTxn,
    scope: SearchScopeId,
    token: &str,
) -> Result<u64, StoreError> {
    let key = term_stats_key(scope, token);
    Ok(
        match inner
            .fts_term_stats
            .get(txn, &key)
            .map_err(|e| StoreError::Io(format!("fts term-stats get: {e}")))?
        {
            Some(v) if v.len() >= 8 => u64::from_be_bytes(v[..8].try_into().unwrap()),
            _ => u64::MAX,
        },
    )
}
