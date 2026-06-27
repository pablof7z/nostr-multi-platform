//! Predicate: does a `RawEvent` match a `StoreQuery`?
//!
//! Mirrors the per-index `matches` function in `nmp_store::mem::query` so the
//! `InterestShape` post-filter exercises exactly the same matching logic as the
//! existing store scan paths (no duplicated index semantics).
//!
//! Additional guards are applied for `StoreQuery::KindDtag` when the source
//! shape carries address coordinates (`NaddrCoord`): the `KindDtag` index does
//! NOT encode the event author, so a post-filter must add
//! `raw.pubkey == coord.pubkey` to avoid matching a different author's event
//! that happens to share the same (kind, d_tag) pair.

use crate::planner::{InterestShape, NaddrCoord};
use crate::store::{RawEvent, StoreQuery};

// ─── helpers ─────────────────────────────────────────────────────────────────

/// Bytes → lowercase hex string (mirrors `nmp_store::mem::bytes_to_hex`).
fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// `created_at` window check (inclusive on both ends).
fn in_range(raw: &RawEvent, since: Option<u64>, until: Option<u64>) -> bool {
    since.is_none_or(|s| raw.created_at >= s) && until.is_none_or(|u| raw.created_at <= u)
}

// ─── Public surface ───────────────────────────────────────────────────────────

/// Returns `true` if `raw` satisfies `query`.
///
/// Mirrors the per-variant filter closures in `nmp_store::mem::query` so the
/// `InterestShape` post-filter and the store scans agree on what constitutes a
/// match. This is a **pure** helper — no allocation beyond field reads.
pub(super) fn raw_matches_store_query(raw: &RawEvent, query: &StoreQuery) -> bool {
    match query {
        StoreQuery::AuthorKind {
            author,
            kinds,
            since,
            until,
        } => {
            raw.pubkey == bytes_to_hex(author)
                && kinds.contains(&raw.kind)
                && in_range(raw, *since, *until)
        }

        StoreQuery::AuthorsKind {
            authors,
            kinds,
            since,
            until,
        } => {
            authors.iter().any(|a| bytes_to_hex(a) == raw.pubkey)
                && kinds.contains(&raw.kind)
                && in_range(raw, *since, *until)
        }

        StoreQuery::KindTime {
            kinds,
            since,
            until,
        } => (kinds.is_empty() || kinds.contains(&raw.kind)) && in_range(raw, *since, *until),

        StoreQuery::KindDtag {
            kind,
            d_tag,
            since,
            until,
        } => {
            let want = String::from_utf8_lossy(d_tag).into_owned();
            raw.kind == *kind
                && raw
                    .d_tag()
                    .is_some_and(|d| String::from_utf8_lossy(&d).into_owned() == want)
                && in_range(raw, *since, *until)
            // NOTE: `KindDtag` alone does NOT constrain the author. Callers
            // that derive this variant from an `addresses` shape MUST apply the
            // `pubkey_guard_for_address` check in addition.
        }

        StoreQuery::Tags {
            authors,
            kinds,
            tags,
            since,
            until,
        } => {
            // Mirrors `nmp_store::mem::query::event_matches_tag_query`: empty
            // tags / empty value set → no match; empty authors = any author;
            // empty kinds = any kind; each `(tag, values)` needs ≥1 matching row
            // (AND across keys, OR within values); `since`/`until` inclusive.
            if tags.is_empty() || tags.values().any(std::collections::BTreeSet::is_empty) {
                return false;
            }
            if !authors.is_empty() && !authors.iter().any(|a| bytes_to_hex(a) == raw.pubkey) {
                return false;
            }
            if !kinds.is_empty() && !kinds.contains(&raw.kind) {
                return false;
            }
            in_range(raw, *since, *until)
                && tags.iter().all(|(tag, values)| {
                    let key = tag.as_str();
                    raw.tags.iter().any(|row| {
                        row.first().is_some_and(|k| k == key)
                            && row.get(1).is_some_and(|val| values.contains(val))
                    })
                })
        }
    }
}

/// Extra pubkey+kind guard for events matched via a `KindDtag` query that
/// originated from an `addresses` (`NaddrCoord`) shape field.
///
/// `StoreQuery::KindDtag` does not encode the author, so a multi-author store
/// may return events from any author that shares the same `(kind, d_tag)` pair.
/// This guard enforces the exact pubkey and kind from the originating coord.
pub(super) fn pubkey_guard_for_address(raw: &RawEvent, coord: &NaddrCoord) -> bool {
    raw.pubkey == coord.pubkey && raw.kind == coord.kind
}

// ─── Shape → per-event predicate ─────────────────────────────────────────────

/// Returns `true` if `raw` matches `shape` via the `queries` derived by
/// `shape_to_store_queries`.
///
/// A row matches if it matches ≥ 1 generated `StoreQuery`. For `KindDtag`
/// queries originating from `shape.addresses`, the extra pubkey guard is
/// applied automatically.
///
/// `queries` is the slice returned by `shape_to_store_queries(shape)`. It is
/// passed explicitly so callers can reuse the compiled set across many rows.
pub(super) fn raw_matches_shape(
    raw: &RawEvent,
    queries: &[StoreQuery],
    shape: &InterestShape,
) -> bool {
    // Address shapes generate KindDtag queries but KindDtag doesn't encode
    // the author. We need to recover which coord each KindDtag query came from
    // so we can apply the pubkey guard. Build a lookup: (kind, d_tag_bytes) →
    // &NaddrCoord. Built lazily only when shape has addresses.
    let addr_coords: Vec<(u32, Vec<u8>, &NaddrCoord)> = if !shape.addresses.is_empty() {
        shape
            .addresses
            .iter()
            .map(|c| (c.kind, c.d_tag.as_bytes().to_vec(), c))
            .collect()
    } else {
        Vec::new()
    };

    for query in queries {
        if let StoreQuery::KindDtag { kind, d_tag, .. } = query {
            if !raw_matches_store_query(raw, query) {
                continue;
            }
            // For KindDtag from address shapes, also check pubkey.
            if !addr_coords.is_empty() {
                let guard_ok = addr_coords.iter().any(|(c_kind, c_dtag, coord)| {
                    c_kind == kind && c_dtag == d_tag && pubkey_guard_for_address(raw, coord)
                });
                if guard_ok {
                    return true;
                }
            } else {
                // KindDtag with no addresses in shape (unusual, but be safe).
                return true;
            }
        } else if raw_matches_store_query(raw, query) {
            return true;
        }
    }
    false
}
