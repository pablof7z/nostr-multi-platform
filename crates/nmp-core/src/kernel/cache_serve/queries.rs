//! `InterestShape` → `StoreQuery` mapping (ADR-0045 §3) and completion-key
//! derivation.
//!
//! ## E1 shapes (authors + kinds, KindTime)
//!
//! - exactly 1 author + ≥1 kind → one `AuthorKind` query.
//! - >1 author + ≥1 kind → one multi-author `AuthorsKind` query (#1497).
//! - 0 authors + ≥1 kind + 0 tags + 0 addrs → `KindTime`.
//!
//! ## E2 shape (DM gift-wrap inbox)
//!
//! - `#p` tag + kind:1059 → `Ptag` (the DM inbox interest).
//!   Served ciphertext is fed through `notify_raw_event_observers`, which is
//!   the same seam live relay-delivered kind:1059 events use after
//!   `Inserted | Replaced` (ADR R2.4(f)). The seam is provenance-agnostic:
//!   `DmInboxProjection::ingest_gift_wrap` accepts `source_relay_url = None`,
//!   which cache-serve passes (local-store events have no relay source).
//!
//! ## E3 shapes (threads, addressable, mentions)
//!
//! - `#e` tag + ≥1 kind → `Etag` (thread replies, per target event id).
//! - `#p` tag + ≥1 kind (non-DM) → `Ptag` (mention inbox).
//! - `addresses` (NaddrCoord) → `KindDtag` per coord.

use super::super::hex_to_pubkey_bytes;
use crate::planner::InterestShape;
use crate::store::StoreQuery;

/// Map an `InterestShape` to the `StoreQuery` variants this seam covers.
///
/// Returns an empty vec when the shape has no mapping (not covered by any
/// engineering increment). Shapes not covered are marked served immediately
/// at enqueue time (no retry, no queue entry) — see `enqueue_cache_serve`
/// in `cache_serve/mod.rs`.
///
/// ## Coverage table (ADR §3, E1–E3)
///
/// | Shape pattern | `StoreQuery` | LMDB index | Increment |
/// |---|---|---|---|
/// | 1 author + ≥1 kind | `AuthorKind` | `idx_author_kind` | E1 |
/// | >1 author + ≥1 kind | one `AuthorsKind` (multi-author) | `idx_author_kind` (multi-scan) | E1 (#1497) |
/// | 0 authors + ≥1 kind + 0 tags + 0 addrs | `KindTime` | `idx_kind_time` | E1 |
/// | `#p` single-value + kind:1059 only | `Ptag` (DM inbox) | `idx_ptag_time` | E2 |
/// | `#p` single-value + ≥1 kind (non-DM) | `Ptag` (mention) | `idx_ptag_time` | E3 |
/// | `#e` single-value + ≥1 kind | `Etag` (thread) | `idx_etag_time` | E3 |
/// | `addresses` non-empty | `KindDtag` per coord | `idx_kind_dtag_time` | E3 |
///
/// Note: `Etag` and `Ptag` carry no time cursor (`query_until_mut` /
/// `query_since_mut` return `None` for those variants). This is intentional
/// conservative over-serve: the relay fills the tail; the index has no
/// time-bounded pagination on those paths.
///
/// ## Intentionally uncovered (tracked)
///
/// The following shapes return an empty vec — they are **not** accidental gaps
/// but deliberate exceptions documented here for auditors:
///
/// - **Empty kinds (wildcard):** no safe bounded index — a kinds-wildcard scan
///   would read unbounded data. Marked served immediately; relay delivers.
/// - **Multi-key / multi-value tags:** single-key indexes cannot perform
///   set-intersection in one scan (e.g. `#e` ∩ `#p`). Relay delivers in full.
/// - **Event-ids-only shapes:** the pointer-loader hydrates on ingest; replaying
///   via a store scan adds no value (each id returns at most one event).
/// - **Unrecognized single tag keys (`#t`, `#a`, etc.):** not yet mapped per
///   ADR-0045 E1–E3; relay serves. Tracked as post-v1 follow-up; deliberately
///   out of scope, not a bug.
/// - **Text / full-text search candidates:** shapes with `search` set always
///   return empty. There is no `StoreQuery` variant or FTS index; search shapes
///   rely on relay NIP-50. Tracked as post-v1 follow-up; not an accidental
///   broad scan.
///
/// See `issue_1517_every_scope_shape_has_a_plan_or_tracked_exception` in
/// `cache_serve_budget_tests` for the contract guard.
pub(in crate::kernel) fn shape_to_store_queries(shape: &InterestShape) -> Vec<StoreQuery> {
    // Relay NIP-50 only for now. Do not degrade a search+kind shape into
    // KindTime/AuthorKind local replay; that would serve non-search results.
    if shape.search.is_some() {
        return Vec::new();
    }

    // Wildcard kinds: not covered (too broad, no safe bounded index).
    if shape.kinds.is_empty() {
        return Vec::new();
    }

    // ── E3: address-pointer (NaddrCoord → KindDtag) ─────────────────────────
    // Addressable shapes carry a non-empty `addresses` set. Map each coord to
    // a `KindDtag` query (idx_kind_dtag_time — no new index needed, ADR §3).
    if !shape.addresses.is_empty() {
        let queries: Vec<StoreQuery> = shape
            .addresses
            .iter()
            .map(|coord| {
                let d_tag = coord.d_tag.as_bytes().to_vec();
                StoreQuery::KindDtag {
                    kind: coord.kind,
                    d_tag,
                    since: shape.since,
                    until: shape.until,
                }
            })
            .collect();
        return queries;
    }

    // ── E2/E3: tag-filtered shapes (exactly one tag key with one value) ──────
    // The planner uses a `BTreeMap<TagKey, BTreeSet<String>>` for tags. The
    // single-target index queries (`Etag`, `Ptag`) are only safe when there is
    // exactly one tag key with exactly one value (multi-key or multi-value
    // shapes would require intersection logic the index cannot provide in a
    // single scan — those shapes stay uncovered and relay delivers in full per
    // the original E1 watermark refusal).
    if !shape.tags.is_empty() {
        if shape.tags.len() == 1 {
            // let-else (queries.rs:156 idiom): a non-empty single-entry BTreeMap
            // always yields one entry; treat absence as "not covered".
            let Some((tag_key, values)) = shape.tags.iter().next() else {
                return Vec::new();
            };
            if values.len() == 1 {
                let Some(target_hex) = values.iter().next() else {
                    return Vec::new();
                };
                let kinds: Vec<u32> = shape.kinds.iter().copied().collect();

                if tag_key == "e" {
                    // ── E3: #e tag → Etag (thread replies) ──────────────────
                    if let Some(target) = hex_to_pubkey_bytes(target_hex) {
                        // EventId and PubKey are both [u8; 32] — same decode.
                        return vec![StoreQuery::Etag { target, kinds }];
                    }
                } else if tag_key == "p" {
                    // ── E2/E3: #p tag → Ptag ────────────────────────────────
                    // E2: kind:1059 only → DM inbox gift-wrap serve.
                    // E3: other kinds (including mixed) → mention inbox serve.
                    if let Some(target) = hex_to_pubkey_bytes(target_hex) {
                        return vec![StoreQuery::Ptag { target, kinds }];
                    }
                }
            }
        }
        // Multi-key, multi-value, or unrecognized key: not covered.
        return Vec::new();
    }

    // ── E1: author+kind or KindTime (no tags, no addresses) ─────────────────
    // event_ids shapes are not covered: an id-lookup can only return zero or
    // one event per id, and the pointer-load path already retrieves them on
    // ingest. There is no gain from replaying them.
    if !shape.event_ids.is_empty() {
        return Vec::new();
    }

    let kinds: Vec<u32> = shape.kinds.iter().copied().collect();

    if shape.authors.is_empty() {
        // KindTime — global / hashtag feed (0 authors + ≥1 kind).
        return vec![StoreQuery::KindTime {
            kinds,
            since: shape.since,
            until: shape.until,
        }];
    }

    // Decode the author hexes once; skip any that fail to parse. A slice
    // pattern selects the query variant panic-free (D6: no `expect`/`unwrap`).
    let decoded: Vec<crate::store::PubKey> = shape
        .authors
        .iter()
        .filter_map(|author_hex| hex_to_pubkey_bytes(author_hex))
        .collect();

    match decoded.as_slice() {
        // Every author hex failed to decode → not covered.
        [] => Vec::new(),
        // Single author → `AuthorKind` (the dedicated single-author index path).
        [author] => vec![StoreQuery::AuthorKind {
            author: *author,
            kinds,
            since: shape.since,
            until: shape.until,
        }],
        // Multi-author shape (#1497 follow-feed collapse) → ONE `AuthorsKind`
        // scan over the combined author set, newest-first. Replaces the prior
        // per-author `AuthorKind` fan-out so a 300–500-follow cold start serves
        // via a single multi-author query, not one per author.
        _ => vec![StoreQuery::AuthorsKind {
            authors: decoded.iter().copied().collect(),
            kinds,
            since: shape.since,
            until: shape.until,
        }],
    }
}

/// Derive the completion key for an interest.
///
/// A stable hash of the interest's `SubKey` + the shape's content fields
/// (authors, kinds, tags, addresses, search). `since/until/limit` and pure routing
/// metadata are excluded — a shape that widens its time window should not
/// retrigger a full re-serve (the watermark+relay refinement handles the delta).
pub(in crate::kernel) fn completion_key_for_interest(
    sub_key: &crate::subs::SubKey,
    shape: &InterestShape,
) -> u64 {
    use crate::stable_hash::stable_hash64;
    let authors: Vec<&str> = shape.authors.iter().map(|s| s.as_str()).collect();
    let kinds: Vec<u32> = shape.kinds.iter().copied().collect();
    // Include tags (E2/E3) so DM-inbox and mention-inbox shapes for different
    // target pubkeys get distinct completion keys.
    let tags: Vec<(&str, Vec<&str>)> = shape
        .tags
        .iter()
        .map(|(k, vs)| (k.as_str(), vs.iter().map(|v| v.as_str()).collect()))
        .collect();
    // Include addresses (E3) so different addressable interests get distinct keys.
    let addresses: Vec<(u32, &str, &str)> = shape
        .addresses
        .iter()
        .map(|c| (c.kind, c.pubkey.as_str(), c.d_tag.as_str()))
        .collect();
    stable_hash64((sub_key, &authors, &kinds, &tags, &addresses, &shape.search))
}

/// Mutable access to a query's `until` cursor — `None` for variants without
/// one (the aggregate-window floor is then simply not applied).
///
/// `Etag` and `Ptag` do not carry `until` cursors (the index does not support
/// time-bounded pagination). The chunk loop advances to the next query when
/// the cursor is absent rather than re-scanning from the top.
pub(in crate::kernel) fn query_until_mut(query: &mut StoreQuery) -> Option<&mut Option<u64>> {
    match query {
        StoreQuery::AuthorKind { until, .. }
        | StoreQuery::AuthorsKind { until, .. }
        | StoreQuery::KindTime { until, .. }
        | StoreQuery::KindDtag { until, .. } => Some(until),
        StoreQuery::Etag { .. } | StoreQuery::Ptag { .. } => None,
    }
}

/// Read the `until` cursor from a query without taking a mutable borrow.
pub(in crate::kernel) fn query_until(query: &StoreQuery) -> Option<u64> {
    match query {
        StoreQuery::AuthorKind { until, .. }
        | StoreQuery::AuthorsKind { until, .. }
        | StoreQuery::KindTime { until, .. }
        | StoreQuery::KindDtag { until, .. } => *until,
        StoreQuery::Etag { .. } | StoreQuery::Ptag { .. } => None,
    }
}

/// Mutable access to a query's `since` bound — `None` for variants without
/// one (the aggregate-window floor is then simply not applied).
pub(in crate::kernel) fn query_since_mut(query: &mut StoreQuery) -> Option<&mut Option<u64>> {
    match query {
        StoreQuery::AuthorKind { since, .. }
        | StoreQuery::AuthorsKind { since, .. }
        | StoreQuery::KindTime { since, .. }
        | StoreQuery::KindDtag { since, .. } => Some(since),
        StoreQuery::Etag { .. } | StoreQuery::Ptag { .. } => None,
    }
}
