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
//! - `#p` tag + kind:1059 → one `Tags` query (the DM inbox interest).
//!   Served ciphertext is fed through `notify_raw_event_observers`, which is
//!   the same seam live relay-delivered kind:1059 events use after
//!   `Inserted | Replaced` (ADR R2.4(f)). The seam is provenance-agnostic:
//!   `DmInboxProjection::ingest_gift_wrap` accepts `source_relay_url = None`,
//!   which cache-serve passes (local-store events have no relay source).
//!
//! ## E3 / generic tag shapes (threads, mentions, groups, hashtags, …)
//!
//! - Any single-letter tag map (`#e`, `#p`, `#h`, `#t`, `#a`, `#d`, …) → one
//!   generic `Tags` query carrying the full map plus authors and kinds. `#e`
//!   thread replies and `#p` mentions are just ordinary entries in this path —
//!   there is no longer a per-letter special case.
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
}

/// The reason a shape cannot be compiled to a store query plan.
///
/// Each variant names the tracked exception documented in the
/// `shape_to_store_queries` doc comment (ADR-0045 §3).
pub(in crate::kernel) enum UnsupportedShapeReason {
    /// `kinds` is empty AND the shape carries no generic tag dimension — a pure
    /// kind/author wildcard scan would read unbounded data. (Tag-bearing shapes
    /// with empty `kinds` ARE supported via [`StoreQuery::Tags`].)
    WildcardKinds,
    /// `search` is set — full-text path, not a structural `StoreQuery`.
    SearchShape,
    /// `event_ids` is non-empty — pointer-loader covers these on ingest, and
    /// the structural store-query variants cannot encode the id constraint.
    EventIds,
    /// A tag key is not a single ASCII-alphabetic letter — not a valid
    /// `SingleLetterTag` dimension.
    InvalidTagKey,
    /// A tag dimension carries an empty value set — nothing to match.
    EmptyTagValues,
    /// The shape carries BOTH `addresses` and generic `tags`. A coordinate
    /// (kind+pubkey+d_tag unit) AND a flat tag map cannot be expressed as one
    /// exact `StoreQuery` without over-serving; relay delivers in full.
    AddressesWithTags,
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
/// | any single-letter tag map with no `ids` (incl. `#e`/`#p`/`#h`/`#t`) | one `Tags` | `tci`/`atci`/`ktci` generic-tag indexes | E2/E3 |
/// | `addresses` non-empty | `KindDtag` per coord | `idx_kind_dtag_time` | E3 |
///
/// Note: `Tags` carries a `since`/`until` window like every other variant —
/// the LMDB tag indexes are keyed by reverse-created-at, so tag scans are
/// time-bounded and page via `query_until_mut`. Multi-key tag maps are an exact
/// AND (and multi-value sets an exact OR) carried in ONE query — cache-serve
/// does not post-filter, so the whole map must travel together.
///
/// ## Intentionally uncovered (tracked)
///
/// The following shapes return `Err` — they are **not** accidental gaps
/// but deliberate exceptions documented here for auditors:
///
/// - **Empty kinds with no tag dimension (wildcard):** no safe bounded index —
///   a pure kinds-wildcard scan would read unbounded data. Marked served
///   immediately; relay delivers. (A tag-bearing shape with empty kinds IS
///   covered: the tag index bounds the scan.)
/// - **`addresses` + generic `tags` together:** a coordinate unit and a flat
///   tag map cannot be one exact query without over-serving. Relay delivers.
/// - **Event-id shapes:** the pointer-loader hydrates on ingest; replaying via a
///   structural store scan adds no value when ids are the only constraint, and
///   would be unsound when ids are combined with tags/authors/kinds because
///   those store-query variants cannot encode the id predicate.
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

    // Event-id-bearing shapes are deliberately not structural cache-serve
    // shapes. When `ids` is the only pointer, point lookup is handled by the
    // pointer-loader/replay paths. When `ids` is combined with other dimensions
    // (for example `{ids:[...], kinds:[9], #h:["room"]}`), dropping the id
    // predicate would over-serve every matching tag/kind row because cache-serve
    // trusts StoreQuery exactness and does not post-filter.
    if !shape.event_ids.is_empty() {
        return Err(UnsupportedShapeReason::EventIds);
    }

    // ── Generic single-letter tag shapes → one `StoreQuery::Tags` ───────────
    // Every single-letter tag dimension (`#e`, `#p`, `#h`, `#t`, `#a`, `#d`, …)
    // compiles into ONE `Tags` query carrying the full tag map plus authors and
    // kinds. Multi-key (AND) and multi-value (OR) are exact: cache-serve trusts
    // `StoreQuery` to be exact and does not post-filter, so the whole map must
    // travel in a single query, never as multiple unioned single-tag queries.
    //
    // `kinds` MAY be empty here (tag-only feeds are locally hydratable) — that is
    // why this block runs BEFORE the wildcard-kinds rejection below. The empty
    // `kinds` then means "any kind" for `StoreQuery::Tags` only (see its docs).
    if !shape.tags.is_empty() {
        // A coordinate (kind+pubkey+d_tag) AND a flat tag map cannot be one
        // exact query without over-serving. Reject rather than over-serve.
        if !shape.addresses.is_empty() {
            return Err(UnsupportedShapeReason::AddressesWithTags);
        }

        let mut tags: std::collections::BTreeMap<
            nostr::SingleLetterTag,
            std::collections::BTreeSet<String>,
        > = std::collections::BTreeMap::new();
        for (tag_key, values) in &shape.tags {
            if values.is_empty() {
                return Err(UnsupportedShapeReason::EmptyTagValues);
            }
            // The stored `TagKey` is the single letter without the leading `#`.
            let mut chars = tag_key.chars();
            let (Some(c), None) = (chars.next(), chars.next()) else {
                return Err(UnsupportedShapeReason::InvalidTagKey);
            };
            let Ok(letter) = nostr::SingleLetterTag::from_char(c) else {
                return Err(UnsupportedShapeReason::InvalidTagKey);
            };
            tags.entry(letter)
                .or_default()
                .extend(values.iter().cloned());
        }

        // Decode authors (if any). An author-bearing shape whose hexes ALL fail
        // to decode is not covered (mirrors the E1 author path); empty authors
        // is the legitimate "any author" wildcard for tag feeds.
        let authors: std::collections::BTreeSet<crate::store::PubKey> = shape
            .authors
            .iter()
            .filter_map(|author_hex| hex_to_pubkey_bytes(author_hex))
            .collect();
        if !shape.authors.is_empty() && authors.is_empty() {
            return Err(UnsupportedShapeReason::AuthorHexDecodeFailure);
        }

        let kinds: Vec<u32> = shape.kinds.iter().copied().collect();
        return Ok(StoreQueryPlan {
            queries: vec![StoreQuery::Tags {
                authors,
                kinds,
                tags,
                since: shape.since,
                until: shape.until,
            }],
            timeline_bound: !shape.authors.is_empty(),
        });
    }

    // Wildcard kinds: not covered (too broad, no safe bounded index). Reached
    // only when the shape has NO generic tag dimension (tag shapes above accept
    // empty kinds).
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
        });
    }

    // ── E1: author+kind or KindTime (no tags, no addresses) ─────────────────
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
        }),
    }
}

/// Map an `InterestShape` to the `StoreQuery` variants this seam covers.
///
/// Returns an empty vec when the shape has no mapping (not covered by any
/// engineering increment). This is a thin wrapper over
/// [`compile_store_query_plan`] for callers that only need the query list.
#[cfg(test)]
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

/// Mutable access to a query's `until` cursor.
///
/// Every variant carries a `since`/`until` window — including
/// [`StoreQuery::Tags`], whose LMDB tag indexes are keyed by reverse-created-at
/// and so support time-bounded pagination. The chunk loop lowers `until` to
/// page deeper.
pub(in crate::kernel) fn query_until_mut(query: &mut StoreQuery) -> Option<&mut Option<u64>> {
    match query {
        StoreQuery::AuthorKind { until, .. }
        | StoreQuery::AuthorsKind { until, .. }
        | StoreQuery::KindTime { until, .. }
        | StoreQuery::KindDtag { until, .. }
        | StoreQuery::Tags { until, .. } => Some(until),
    }
}

/// Read the `until` cursor from a query without taking a mutable borrow.
pub(in crate::kernel) fn query_until(query: &StoreQuery) -> Option<u64> {
    match query {
        StoreQuery::AuthorKind { until, .. }
        | StoreQuery::AuthorsKind { until, .. }
        | StoreQuery::KindTime { until, .. }
        | StoreQuery::KindDtag { until, .. }
        | StoreQuery::Tags { until, .. } => *until,
    }
}

/// Mutable access to a query's `since` bound — used to apply the aggregate
/// timeline-window floor.
pub(in crate::kernel) fn query_since_mut(query: &mut StoreQuery) -> Option<&mut Option<u64>> {
    match query {
        StoreQuery::AuthorKind { since, .. }
        | StoreQuery::AuthorsKind { since, .. }
        | StoreQuery::KindTime { since, .. }
        | StoreQuery::KindDtag { since, .. }
        | StoreQuery::Tags { since, .. } => Some(since),
    }
}
