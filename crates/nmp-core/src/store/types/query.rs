//! `StoreQuery` — the NMP-internal read filter for `EventStore::query_visit`.
//!
//! This is **not** a pass-through to `nostr::Filter`. Each variant maps 1:1
//! onto an existing secondary index path so the visitor API exercises the
//! same index logic as the specialized `scan_by_*` methods (no duplicate
//! index code). See `docs/design/nostrdb-notedeck-lessons.md` §2.3.

use super::ids::{EventId, PubKey};

/// A read query over the event store, expressed in terms of the index that
/// will serve it. `since`/`until` are unix-seconds bounds (inclusive);
/// `limit` is the maximum number of events the scan yields, newest-first.
#[derive(Clone, Debug)]
pub enum StoreQuery {
    /// `idx_author_kind` — events by `author` with kind in `kinds`.
    AuthorKind {
        author: PubKey,
        kinds: Vec<u32>,
        since: Option<u64>,
        until: Option<u64>,
    },
    /// `idx_kind_time` — events with kind in `kinds` (empty = any kind).
    KindTime {
        kinds: Vec<u32>,
        since: Option<u64>,
        until: Option<u64>,
    },
    /// `idx_kind_dtag_time` — parameterized-replaceable scan for `(kind, d_tag)`.
    KindDtag {
        kind: u32,
        d_tag: Vec<u8>,
        since: Option<u64>,
        until: Option<u64>,
    },
    /// `idx_etag_time` — events with kind in `kinds` that `e`-tag `target`.
    Etag { target: EventId, kinds: Vec<u32> },
    /// `idx_ptag_time` — events with kind in `kinds` that `p`-tag `target`.
    Ptag { target: PubKey, kinds: Vec<u32> },
}
