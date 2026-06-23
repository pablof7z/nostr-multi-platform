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

// ─── StoreQueryPlan ──────────────────────────────────────────────────────────

/// The compiled output of [`compile_store_query_plan`]: queries to run plus
/// structural metadata derived from the shape at compile time.
pub(in crate::kernel) struct StoreQueryPlan {
    /// `StoreQuery` variants to run against the store, in order.
    pub queries: Vec<StoreQuery>,
    /// `true` when the shape has authors (enabling the aggregate-window
    /// `since` floor in timeline-bound serves).
    pub timeline_bound: bool,
    /// `true` when every query in the plan carries an `until` cursor that can
    /// be used for cursor-paged continuation (`AuthorKind`, `AuthorsKind`,
    /// `KindTime`, `KindDtag`). `false` for tag-target index queries
    /// (`Etag`, `Ptag`) which have no time-bounded pagination.
    pub has_until_cursor: bool,
    /// `true` when the floor probe can use an index-level time bound.
    /// `false` for pure `Etag`/`Ptag` shapes where only visitor-level
    /// enforcement is possible (no `until` in the index key).
    pub floor_probe_allowed: bool,
}

/// The reason a shape cannot be compiled to a store query plan.
///
/// Each variant names the tracked exception documented in the
/// `shape_to_store_queries` doc comment (ADR-0045 §3).
pub(in crate::kernel) enum UnsupportedShapeReason {
    /// `kinds` is empty — wildcard scan, unbounded.
    WildcardKinds,
    /// `search` is set — full-text path, not a structural `StoreQuery`.
    SearchShape,
    /// `event_ids` is non-empty — pointer-loader covers these on ingest.
    EventIdsOnly,
    /// More than one tag key — set-intersection not supported by single index.
    MultiKeyTags,
    /// Exactly one tag key but multiple values — single-value index only.
    MultiValueTag,
    /// Tag key is not `"e"` or `"p"` — not yet mapped (post-v1 follow-up).
    UnrecognizedTagKey,
    /// Author hex string(s) all failed to decode — no valid pubkey bytes.
    AuthorHexDecodeFailure,
}

/// Compile an `InterestShape` into a [`StoreQueryPlan`].
///
/// Returns `Err(UnsupportedShapeReason)` when the shape has no structural
/// `StoreQuery` mapping. Shapes not covered are marked served immediately
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
/// The following shapes return `Err` — they are **not** accidental gaps
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
///   return `Err` from THIS function — full-text matching has no `StoreQuery`
///   variant (its index is the tokenized inverted index, not a structural
///   `idx_*` scan). Such a shape is NOT necessarily relay-only, though: when a
///   cache search scope is registered for the shape's kinds, cache-serve routes
///   it to the store's `text_search_visit` via
///   [`Kernel::try_cache_serve_search`](super::Kernel::try_cache_serve_search)
///   (issue #1811, the `search` sibling module) BEFORE this function is
///   consulted. With no matching scope the search stays relay-served (NIP-50).
///   So: covered when a cache scope is registered; relay-only otherwise — never
///   an accidental broad structural scan.
///
/// See `issue_1517_every_scope_shape_has_a_plan_or_tracked_exception` in
/// `cache_serve_budget_tests` for the contract guard.
pub(in crate::kernel) fn compile_store_query_plan(
    shape: &InterestShape,
) -> Result<StoreQueryPlan, UnsupportedShapeReason> {
    // Search-bearing shapes have no STRUCTURAL `StoreQuery`: full-text matching
    // is the tokenized inverted index, served separately by
    // `Kernel::try_cache_serve_search` (#1811) before this function runs. Do NOT
    // degrade a search+kind shape into KindTime/AuthorKind local replay here —
    // that would serve non-search results. (Cache-coverage vs. relay-only for a
    // search shape is decided by scope registration in the `search` module, not
    // by this Err.)
    if shape.search.is_some() {
        return Err(UnsupportedShapeReason::SearchShape);
    }

    // Wildcard kinds: not covered (too broad, no safe bounded index).
    if shape.kinds.is_empty() {
        return Err(UnsupportedShapeReason::WildcardKinds);
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
        return Ok(StoreQueryPlan {
            queries,
            timeline_bound: !shape.authors.is_empty(),
            has_until_cursor: true,
            floor_probe_allowed: true,
        });
    }

    // ── E2/E3: tag-filtered shapes (exactly one tag key with one value) ──────
    // The planner uses a `BTreeMap<TagKey, BTreeSet<String>>` for tags. The
    // single-target index queries (`Etag`, `Ptag`) are only safe when there is
    // exactly one tag key with exactly one value (multi-key or multi-value
    // shapes would require intersection logic the index cannot provide in a
    // single scan — those shapes stay uncovered and relay delivers in full per
    // the original E1 watermark refusal).
    if !shape.tags.is_empty() {
        if shape.tags.len() > 1 {
            return Err(UnsupportedShapeReason::MultiKeyTags);
        }
        // let-else (queries.rs idiom): a non-empty single-entry BTreeMap
        // always yields one entry; treat absence as "not covered".
        let Some((tag_key, values)) = shape.tags.iter().next() else {
            return Err(UnsupportedShapeReason::MultiKeyTags);
        };
        if values.len() != 1 {
            return Err(UnsupportedShapeReason::MultiValueTag);
        }
        let Some(target_hex) = values.iter().next() else {
            return Err(UnsupportedShapeReason::MultiValueTag);
        };
        let kinds: Vec<u32> = shape.kinds.iter().copied().collect();

        if tag_key == "e" {
            // ── E3: #e tag → Etag (thread replies) ──────────────────
            if let Some(target) = hex_to_pubkey_bytes(target_hex) {
                // EventId and PubKey are both [u8; 32] — same decode.
                return Ok(StoreQueryPlan {
                    queries: vec![StoreQuery::Etag { target, kinds }],
                    timeline_bound: false,
                    has_until_cursor: false,
                    floor_probe_allowed: false,
                });
            }
        } else if tag_key == "p" {
            // ── E2/E3: #p tag → Ptag ────────────────────────────────
            // E2: kind:1059 only → DM inbox gift-wrap serve.
            // E3: other kinds (including mixed) → mention inbox serve.
            if let Some(target) = hex_to_pubkey_bytes(target_hex) {
                return Ok(StoreQueryPlan {
                    queries: vec![StoreQuery::Ptag { target, kinds }],
                    timeline_bound: false,
                    has_until_cursor: false,
                    floor_probe_allowed: false,
                });
            }
        } else {
            return Err(UnsupportedShapeReason::UnrecognizedTagKey);
        }
        // hex decode failed for "e" or "p" tag — treat as not covered.
        return Err(UnsupportedShapeReason::UnrecognizedTagKey);
    }

    // ── E1: author+kind or KindTime (no tags, no addresses) ─────────────────
    // event_ids shapes are not covered: an id-lookup can only return zero or
    // one event per id, and the pointer-load path already retrieves them on
    // ingest. There is no gain from replaying them.
    if !shape.event_ids.is_empty() {
        return Err(UnsupportedShapeReason::EventIdsOnly);
    }

    let kinds: Vec<u32> = shape.kinds.iter().copied().collect();

    if shape.authors.is_empty() {
        // KindTime — global / hashtag feed (0 authors + ≥1 kind).
        return Ok(StoreQueryPlan {
            queries: vec![StoreQuery::KindTime {
                kinds,
                since: shape.since,
                until: shape.until,
            }],
            timeline_bound: false,
            has_until_cursor: true,
            floor_probe_allowed: true,
        });
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
        [] => Err(UnsupportedShapeReason::AuthorHexDecodeFailure),
        // Single author → `AuthorKind` (the dedicated single-author index path).
        [author] => Ok(StoreQueryPlan {
            queries: vec![StoreQuery::AuthorKind {
                author: *author,
                kinds,
                since: shape.since,
                until: shape.until,
            }],
            timeline_bound: true,
            has_until_cursor: true,
            floor_probe_allowed: true,
        }),
        // Multi-author shape (#1497 follow-feed collapse) → ONE `AuthorsKind`
        // scan over the combined author set, newest-first. Replaces the prior
        // per-author `AuthorKind` fan-out so a 300–500-follow cold start serves
        // via a single multi-author query, not one per author.
        _ => Ok(StoreQueryPlan {
            queries: vec![StoreQuery::AuthorsKind {
                authors: decoded.iter().copied().collect(),
                kinds,
                since: shape.since,
                until: shape.until,
            }],
            timeline_bound: true,
            has_until_cursor: true,
            floor_probe_allowed: true,
        }),
    }
}

/// Map an `InterestShape` to the `StoreQuery` variants this seam covers.
///
/// Returns an empty vec when the shape has no mapping (not covered by any
/// engineering increment). This is a thin wrapper over
/// [`compile_store_query_plan`] for callers that only need the query list.
pub(in crate::kernel) fn shape_to_store_queries(shape: &InterestShape) -> Vec<StoreQuery> {
    match compile_store_query_plan(shape) {
        Ok(plan) => plan.queries,
        Err(_) => Vec::new(),
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
