//! `InterestShape` → `StoreQuery` mapping (ADR-0045 §3) and completion-key
//! derivation.
//!
//! ## E1 shapes (authors + kinds, KindTime)
//!
//! - ≥1 author + ≥1 kind → one `AuthorKind` query per author.
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
use crate::substrate::EventIngestDispatcher;

/// Map an `InterestShape` to the `StoreQuery` variants this seam covers.
///
/// Returns an empty vec when the shape has no mapping (not covered by any
/// engineering increment). Shapes not covered are marked served immediately
/// at enqueue time (no retry, no queue entry) — see
/// [`crate::kernel::Kernel::enqueue_cache_serve`].
///
/// ## Coverage table (ADR §3, E1–E3)
///
/// | Shape pattern | `StoreQuery` | Increment |
/// |---|---|---|
/// | ≥1 author + ≥1 kind | `AuthorKind` per author | E1 |
/// | 0 authors + ≥1 kind + 0 tags + 0 addrs | `KindTime` | E1 |
/// | `#p` single-value + kind:1059 only | `Ptag` (DM inbox) | E2 |
/// | `#p` single-value + ≥1 kind (non-DM) | `Ptag` (mention) | E3 |
/// | `#e` single-value + ≥1 kind | `Etag` (thread) | E3 |
/// | `addresses` non-empty | `KindDtag` per coord | E3 |
pub(in crate::kernel) fn shape_to_store_queries(shape: &InterestShape) -> Vec<StoreQuery> {
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
        vec![StoreQuery::KindTime {
            kinds,
            since: shape.since,
            until: shape.until,
        }]
    } else {
        // AuthorKind — one query per author; results merged under the shared
        // budget. Mirrors the per-author watermark scan `#1091` uses.
        shape
            .authors
            .iter()
            .filter_map(|author_hex| {
                let author = hex_to_pubkey_bytes(author_hex)?;
                Some(StoreQuery::AuthorKind {
                    author,
                    kinds: kinds.clone(),
                    since: shape.since,
                    until: shape.until,
                })
            })
            .collect()
    }
}

/// Compute a watermark floor for `shape` by folding the per-query newest
/// timestamps returned by `scan` over `shape_to_store_queries(shape)` — the
/// SINGLE source of shape→store-query truth (ADR-0045 §6 "one table read two
/// ways" / issue #1119).
///
/// `scan` runs the actual store probe for one query (typically a
/// `query_visit(q, 1, …)` that returns the newest stored `created_at`, or
/// `None` when nothing matches). It is invoked once per query in
/// [`shape_to_store_queries`] order, with `since`/`until` already normalized to
/// `None` (the watermark scan wants the newest match regardless of window).
///
/// Folding policy (preserves the prior `watermark_fn` semantics exactly):
///
/// - **`AuthorKind`** (per-author feed): `min` across authors, but any author
///   with no stored events (`scan → None`) aborts the whole floor (`None`) —
///   their history must be fetched in full (V-118).
/// - **`KindDtag`** (addressable): `min` across coords, AND any coord with no
///   stored match (`scan → None`) aborts the whole floor (`None`) — the SAME
///   min/abort rule the authors branch uses (K3 Stage B1). A multi-coord
///   addressable shape with one unfetched replaceable coordinate must not be
///   floored above that coord's never-fetched history; max-ignoring-empties
///   (the prior policy) was the opposite, unsafe rule for the identical hazard.
/// - **`Etag` / `Ptag`**: the single query's value (or `None`).
/// - **`KindTime`** (zero-author global feed): never floored — no safe
///   per-author floor exists, so the presence of any `KindTime` query forces
///   `None`.
///
/// `is_truncated` reports whether a CURSOR-LESS (`Etag`/`Ptag`) query was
/// budget-truncated mid-serve this session (K3 Stage B3). A truncated
/// cursor-less query stranded the stored tail within serve depth, so flooring
/// it would suppress the relay re-send of that tail — the fold therefore
/// refuses the floor for that shape (returns `None`). Pass a closure that is
/// always `false` to opt out (the prior, truncation-unaware behaviour).
///
/// Returns `None` when the shape maps to no queries (uncovered → unfloored),
/// which is exactly the §6 implication `floored ⇒ served`, now structural.
pub(in crate::kernel) fn watermark_from_queries(
    shape: &InterestShape,
    mut scan: impl FnMut(&StoreQuery) -> Option<u64>,
    mut is_truncated: impl FnMut(u64) -> bool,
) -> Option<u64> {
    let queries = shape_to_store_queries(shape);
    if queries.is_empty() {
        return None;
    }

    let mut author_min: Option<u64> = None;
    let mut addr_min: Option<u64> = None;
    let mut single: Option<u64> = None;

    for query in &queries {
        match query {
            // Zero-author global feed: no safe floor exists.
            StoreQuery::KindTime { .. } => return None,
            StoreQuery::AuthorKind { .. } => {
                // Any empty author aborts the whole floor (must backfill).
                let ts = scan(query)?;
                author_min = Some(author_min.map_or(ts, |prev| prev.min(ts)));
            }
            StoreQuery::KindDtag { .. } => {
                // K3 Stage B1: same min/abort rule as AuthorKind. Any coord
                // with no stored match aborts the whole floor (the unfetched
                // coord must backfill in full); otherwise floor at the MIN so
                // no coord is floored above its own newest stored event.
                let ts = scan(query)?;
                addr_min = Some(addr_min.map_or(ts, |prev| prev.min(ts)));
            }
            StoreQuery::Etag { .. } | StoreQuery::Ptag { .. } => {
                // K3 Stage B3: a budget-truncated cursor-less serve stranded
                // the stored tail; refuse the floor so the relay re-sends it.
                if let Some(key) = cursor_less_query_key(query) {
                    if is_truncated(key) {
                        return None;
                    }
                }
                single = scan(query);
            }
        }
    }

    // A shape maps to a homogeneous query family, so at most one of these is
    // populated; `.or` picks whichever the family produced.
    author_min.or(addr_min).or(single)
}

/// Whether a shape needs `IngestParser` dispatch in addition to normal
/// `notify_event_observers` fan-out.
///
/// Owner doctrine (single-mechanism cache-serve): dispatch decisions MUST honor
/// ACTUAL dispatcher registrations rather than a hardcoded shape allowlist.
/// This function checks `dispatcher.is_interested(kind)` for every kind in the
/// shape — a fast read that short-circuits as soon as one match is found.
///
/// The old hardcoded `#p`+kind:1059 allowlist is retired here. Any registered
/// `IngestParser` — including all-kinds range parsers (e.g. chirp-tui's
/// `RawCacheIngestParser` for `0..u32::MAX`) — now transparently causes
/// cache-serve dispatch without code changes.
///
/// If `dispatcher` is `None` (e.g. poisoned lock graceful-degrade), returns
/// `false` — no dispatch fires, which is the safe degraded state.
///
/// Note: this does NOT control `notify_raw_event_observers` (the verbatim
/// forwarding tap). The raw tap fires only on live relay ingest, never on
/// cache-served replay.
pub(in crate::kernel) fn shape_needs_ingest_parser_dispatch(
    shape: &InterestShape,
    dispatcher: Option<&EventIngestDispatcher>,
) -> bool {
    if shape.kinds.is_empty() {
        return false;
    }
    let Some(d) = dispatcher else {
        return false;
    };
    shape.kinds.iter().any(|&kind| d.is_interested(kind))
}

/// Derive the completion key for an interest.
///
/// A stable hash of the interest's `SubKey` + the shape's content fields
/// (authors, kinds, tags, addresses). `since/until/limit` and pure routing
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
    stable_hash64((sub_key, &authors, &kinds, &tags, &addresses))
}

/// Stable session-key for a CURSOR-LESS store query (`Etag` / `Ptag`).
///
/// K3 Stage B3 — `Etag`/`Ptag` serves carry no resume cursor (the index does
/// not support time-bounded pagination), so a budget-truncated chunk advances
/// PAST the query and silently skips the stored tail within serve depth. The
/// watermark floor (enabled for these shapes since ADR-0045 E2/E3) would then
/// suppress the relay re-send of that skipped tail. The serve records the
/// truncation under this key; [`watermark_from_queries`] refuses to floor a
/// shape whose cursor-less query is in the truncated set.
///
/// Keyed by the query's content identity (target + kinds) so the serve-side
/// writer and the watermark-side reader compute the SAME key from the same
/// shape — exactly the "one table read two ways" discipline the floor already
/// rides on. Returns `None` for cursored variants (which resume by cursor and
/// never strand a tail this way).
pub(in crate::kernel) fn cursor_less_query_key(query: &StoreQuery) -> Option<u64> {
    use crate::stable_hash::stable_hash64;
    match query {
        StoreQuery::Etag { target, kinds } => Some(stable_hash64(("etag", target, kinds))),
        StoreQuery::Ptag { target, kinds } => Some(stable_hash64(("ptag", target, kinds))),
        StoreQuery::AuthorKind { .. }
        | StoreQuery::KindTime { .. }
        | StoreQuery::KindDtag { .. } => None,
    }
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
        | StoreQuery::KindTime { until, .. }
        | StoreQuery::KindDtag { until, .. } => Some(until),
        StoreQuery::Etag { .. } | StoreQuery::Ptag { .. } => None,
    }
}

/// Read the `until` cursor from a query without taking a mutable borrow.
pub(in crate::kernel) fn query_until(query: &StoreQuery) -> Option<u64> {
    match query {
        StoreQuery::AuthorKind { until, .. }
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
        | StoreQuery::KindTime { since, .. }
        | StoreQuery::KindDtag { since, .. } => Some(since),
        StoreQuery::Etag { .. } | StoreQuery::Ptag { .. } => None,
    }
}
