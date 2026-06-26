//! `StoreQuery` — the NMP-internal read filter for `EventStore::query_visit`.
//!
//! This is **not** a pass-through to `nostr::Filter`. Each variant maps 1:1
//! onto an existing secondary index path so the visitor API exercises the
//! same index logic as the specialized `scan_by_*` methods (no duplicate
//! index code). See `docs/design/nostrdb-notedeck-lessons.md` §2.3.

use std::collections::{BTreeMap, BTreeSet};

use nostr::SingleLetterTag;

use super::ids::PubKey;

/// A read query over the event store, expressed in terms of the index that
/// will serve it. `since`/`until` are unix-seconds bounds (inclusive);
/// `limit` is the maximum number of events the scan yields, newest-first.
#[derive(Clone, Debug)]
pub enum StoreQuery {
    /// `idx_author_kind` — events by `author` with kind in `kinds`.
    ///
    /// Empty-kinds semantics (identical across `MemEventStore` and
    /// `LmdbEventStore`): an empty `kinds` set matches **nothing** — this is a
    /// positive `(author, kinds)` selection, never an author-wildcard over all
    /// kinds.
    AuthorKind {
        author: PubKey,
        kinds: Vec<u32>,
        since: Option<u64>,
        until: Option<u64>,
    },
    /// `idx_author_kind` (multi-author) — events by any author in `authors` with kind in `kinds`,
    /// newest-first across the combined author set.
    ///
    /// Empty-set semantics (identical across `MemEventStore` and
    /// `LmdbEventStore`): an empty `authors` set **or** an empty `kinds` set
    /// matches **nothing** — this variant is a positive selection over a
    /// concrete author set and kind set, never a wildcard. (Use [`StoreQuery::KindTime`]
    /// for the no-author "any kind" global feed.) This mirrors the single-author
    /// [`StoreQuery::AuthorKind`] contract, where an empty `kinds` likewise
    /// matches nothing.
    AuthorsKind {
        authors: BTreeSet<PubKey>,
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
    ///
    /// This is the addressable/replaceable point-scan path (it backs
    /// [`crate::EventStore::get_param_replaceable`] and `InterestShape.addresses`);
    /// it is **not** a generic `#d` feed. Generic `{"#d":[...]}` subscriptions use
    /// [`StoreQuery::Tags`].
    KindDtag {
        kind: u32,
        d_tag: Vec<u8>,
        since: Option<u64>,
        until: Option<u64>,
    },
    /// Generic single-letter tag scan — the one read path for **every**
    /// single-letter tag dimension (`#e`, `#p`, `#h`, `#t`, `#a`, `#d`, …).
    /// Backed by the LMDB fork's `tci`/`atci`/`ktci` generic-tag indexes; the
    /// `MemEventStore` matches against the full raw tag matrix.
    ///
    /// Matching semantics (identical across `MemEventStore` and `LmdbEventStore`):
    ///
    /// - `authors`: **empty = any author** (no author constraint); otherwise the
    ///   event author must be one of the set.
    /// - `kinds`: **empty = any kind** (no kind constraint). This differs from
    ///   [`StoreQuery::AuthorKind`]/[`StoreQuery::AuthorsKind`], where an empty
    ///   `kinds` matches **nothing** — tag-only feeds are a required shape, so an
    ///   empty `kinds` here is a wildcard, never "match nothing".
    /// - `tags`: a `(SingleLetterTag → values)` map combined as a logical **AND**
    ///   across keys and a logical **OR** within each key's value set. An event
    ///   matches a `(tag, values)` entry iff it carries at least one tag row whose
    ///   first element is the single letter and whose second element is one of
    ///   `values`. Values are **exact UTF-8 strings** — for `#e`/`#p` these are
    ///   the 64-char NIP-01 hex strings exactly as they appear in tag rows (no
    ///   byte decode/re-encode). An empty `tags` map, or any entry with an empty
    ///   value set, matches **nothing** (a programming error at the builder
    ///   level; backends defensively yield an empty result).
    /// - `since`/`until`: inclusive unix-seconds bounds.
    Tags {
        authors: BTreeSet<PubKey>,
        kinds: Vec<u32>,
        tags: BTreeMap<SingleLetterTag, BTreeSet<String>>,
        since: Option<u64>,
        until: Option<u64>,
    },
}
