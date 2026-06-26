//! Pure SQL builders for the OPFS-SQLite scan/query paths (#1007 PR-4).
//!
//! This module is **target-agnostic and pure** — it builds `(sql, params)`
//! pairs from crate-local query inputs and is unit-tested on native (no shim,
//! no SQLite), exactly like [`crate::conv`]. The wasm inherent methods in the
//! parent [`crate::query`] module bind the params and step the statement.
//!
//! ## Why every builder maps onto an existing index (no full table scan)
//!
//! Each shape mirrors an LMDB access path and is served by one of the composite
//! indexes the schema declares (`schema.rs`):
//!
//! | builder                | index served                              |
//! |------------------------|-------------------------------------------|
//! | [`build_author_kind`]  | `idx_events_akci` (pubkey, kind, …)       |
//! | [`build_authors_kind`] | `idx_events_aci`/`akci` per author        |
//! | [`build_kind_time`]    | `idx_events_kci` (kind, …) / `idx_events_ci` (all kinds) |
//! | [`build_kind_dtag`]    | `idx_events_kind_dtag` (kind, d_tag, …)   |
//! | [`build_tags`]         | `idx_tags_tci`/`atci`/`ktci` on `event_tags` |
//! | [`build_expiring_before`] | `idx_events_expires` (ascending)       |
//!
//! Newest-first ordering is `created_at DESC, id ASC` — byte-for-byte the
//! `(created_at desc, id asc)` global order the `EventStore` trait specifies,
//! and the exact column order of the `*ci` indexes so the `ORDER BY` is an
//! index walk, never a sort.

use std::collections::{BTreeMap, BTreeSet};

use crate::outcome::PubKey;

/// An owned bound-parameter value. Owned (not borrowed like
/// [`crate::store_impl::SqlVal`]) because the builders synthesize values
/// (placeholder lists, `to_vec`'d blobs) that must outlive the `&str` SQL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum OwnedVal {
    /// 64-bit integer (kind, created_at, limit, …).
    Int(i64),
    /// UTF-8 text (tag name / tag value / relay url).
    Text(String),
    /// Byte blob (id / pubkey / d-tag).
    Blob(Vec<u8>),
}

/// A read query over the event store, expressed in terms of the index that will
/// serve it. Crate-local mirror of `nmp_store::StoreQuery` (the `nmp-store`
/// `EventStore` wrapper maps `StoreQuery -> EngineQuery` 1:1 at the cycle-free
/// seam; `nostr::SingleLetterTag` collapses to its `char`). `since`/`until` are
/// inclusive unix-seconds bounds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineQuery {
    /// Events by `author` with kind in `kinds`. Empty `kinds` matches nothing
    /// (a positive `(author, kinds)` selection, never an author wildcard).
    AuthorKind {
        /// The single author whose events to scan.
        author: PubKey,
        /// Kinds to include; empty matches nothing.
        kinds: Vec<u32>,
        /// Inclusive lower `created_at` bound.
        since: Option<u64>,
        /// Inclusive upper `created_at` bound.
        until: Option<u64>,
    },
    /// Events by any author in `authors` with kind in `kinds`, globally
    /// newest-first across the combined set. Empty `authors` **or** empty
    /// `kinds` matches nothing.
    AuthorsKind {
        /// The authors whose events to scan (empty matches nothing).
        authors: BTreeSet<PubKey>,
        /// Kinds to include (empty matches nothing).
        kinds: Vec<u32>,
        /// Inclusive lower `created_at` bound.
        since: Option<u64>,
        /// Inclusive upper `created_at` bound.
        until: Option<u64>,
    },
    /// Events with kind in `kinds`. **Empty `kinds` = any kind** (the only shape
    /// where empty is a wildcard).
    KindTime {
        /// Kinds to include; empty scans all kinds.
        kinds: Vec<u32>,
        /// Inclusive lower `created_at` bound.
        since: Option<u64>,
        /// Inclusive upper `created_at` bound.
        until: Option<u64>,
    },
    /// Parameterized-replaceable scan for `(kind, d_tag)` across all authors.
    KindDtag {
        /// Addressable kind (30000–39999 in practice).
        kind: u32,
        /// The `d`-tag identifier bytes (empty for an implicit empty `d`).
        d_tag: Vec<u8>,
        /// Inclusive lower `created_at` bound.
        since: Option<u64>,
        /// Inclusive upper `created_at` bound.
        until: Option<u64>,
    },
    /// Generic single-letter tag scan — AND across letters, OR within a letter's
    /// values. Empty `authors` = any author; empty `kinds` = any kind; an empty
    /// `tags` map (or any empty value set) matches nothing.
    Tags {
        /// Author constraint; empty = any author.
        authors: BTreeSet<PubKey>,
        /// Kind constraint; empty = any kind.
        kinds: Vec<u32>,
        /// `letter -> {exact values}`, AND across letters / OR within values.
        tags: BTreeMap<char, BTreeSet<String>>,
        /// Inclusive lower `created_at` bound.
        since: Option<u64>,
        /// Inclusive upper `created_at` bound.
        until: Option<u64>,
    },
}

/// Newest-first index-walk tail shared by every primary-table scan. The `LIMIT`
/// placeholder is bound last by the caller (after appending the limit value).
const ORDER_LIMIT: &str = " ORDER BY created_at DESC, id ASC LIMIT ?";

/// Build `SELECT raw, received_at_ms FROM events WHERE 1{conds} ORDER BY …
/// LIMIT ?`. The `WHERE 1` seed lets every condition append as ` AND …`,
/// avoiding first-clause special-casing; SQLite folds the constant away.
fn events_select(
    conds: impl FnOnce(&mut String, &mut Vec<OwnedVal>),
    limit: usize,
) -> (String, Vec<OwnedVal>) {
    let mut sql = String::from("SELECT raw, received_at_ms FROM events WHERE 1");
    let mut params = Vec::new();
    conds(&mut sql, &mut params);
    sql.push_str(ORDER_LIMIT);
    params.push(OwnedVal::Int(limit as i64));
    (sql, params)
}

/// Append ` AND <col> IN (?,?,…)` for a non-empty integer set (kinds).
fn push_in_ints(sql: &mut String, params: &mut Vec<OwnedVal>, col: &str, vals: &[u32]) {
    sql.push_str(" AND ");
    sql.push_str(col);
    sql.push_str(" IN (");
    for (i, k) in vals.iter().enumerate() {
        if i > 0 {
            sql.push(',');
        }
        sql.push('?');
        params.push(OwnedVal::Int(i64::from(*k)));
    }
    sql.push(')');
}

/// Append ` AND <col> IN (?,?,…)` for a non-empty blob set (pubkeys).
fn push_in_blobs(sql: &mut String, params: &mut Vec<OwnedVal>, col: &str, vals: &BTreeSet<PubKey>) {
    sql.push_str(" AND ");
    sql.push_str(col);
    sql.push_str(" IN (");
    for (i, b) in vals.iter().enumerate() {
        if i > 0 {
            sql.push(',');
        }
        sql.push('?');
        params.push(OwnedVal::Blob(b.to_vec()));
    }
    sql.push(')');
}

/// Append the inclusive `created_at` bounds (each optional).
fn push_time(sql: &mut String, params: &mut Vec<OwnedVal>, since: Option<u64>, until: Option<u64>) {
    if let Some(s) = since {
        sql.push_str(" AND created_at >= ?");
        params.push(OwnedVal::Int(s as i64));
    }
    if let Some(u) = until {
        sql.push_str(" AND created_at <= ?");
        params.push(OwnedVal::Int(u as i64));
    }
}

/// `idx_events_akci` scan. `None` when `kinds` is empty (matches nothing).
pub(crate) fn build_author_kind(
    author: &PubKey,
    kinds: &[u32],
    since: Option<u64>,
    until: Option<u64>,
    limit: usize,
) -> Option<(String, Vec<OwnedVal>)> {
    if kinds.is_empty() {
        return None;
    }
    Some(events_select(
        |sql, params| {
            sql.push_str(" AND pubkey = ?");
            params.push(OwnedVal::Blob(author.to_vec()));
            push_in_ints(sql, params, "kind", kinds);
            push_time(sql, params, since, until);
        },
        limit,
    ))
}

/// `idx_events_aci`/`akci` multi-author scan, globally newest-first. `None` when
/// `authors` **or** `kinds` is empty (both match nothing). The single `events`
/// table makes the result inherently duplicate-free — one row per id.
pub(crate) fn build_authors_kind(
    authors: &BTreeSet<PubKey>,
    kinds: &[u32],
    since: Option<u64>,
    until: Option<u64>,
    limit: usize,
) -> Option<(String, Vec<OwnedVal>)> {
    if authors.is_empty() || kinds.is_empty() {
        return None;
    }
    Some(events_select(
        |sql, params| {
            push_in_blobs(sql, params, "pubkey", authors);
            push_in_ints(sql, params, "kind", kinds);
            push_time(sql, params, since, until);
        },
        limit,
    ))
}

/// `idx_events_kci` scan (or `idx_events_ci` when `kinds` is empty — the one
/// shape where empty kinds is the "any kind" wildcard).
pub(crate) fn build_kind_time(
    kinds: &[u32],
    since: Option<u64>,
    until: Option<u64>,
    limit: usize,
) -> (String, Vec<OwnedVal>) {
    events_select(
        |sql, params| {
            if !kinds.is_empty() {
                push_in_ints(sql, params, "kind", kinds);
            }
            push_time(sql, params, since, until);
        },
        limit,
    )
}

/// `idx_events_kind_dtag` scan — the parameterized-replaceable `(kind, d_tag)`
/// path across all authors. Seeks the dedicated `(kind, d_tag, created_at DESC,
/// id ASC)` index; the `d_tag` column carries the canonical addressable
/// identifier (`""` for an implicit empty `d`), so a `d_tag = b""` query finds
/// param-replaceable events with no explicit `d` tag — which `event_tags` (rows
/// only for value-bearing tags) could not.
pub(crate) fn build_kind_dtag(
    kind: u32,
    d_tag: &[u8],
    since: Option<u64>,
    until: Option<u64>,
    limit: usize,
) -> (String, Vec<OwnedVal>) {
    events_select(
        |sql, params| {
            sql.push_str(" AND kind = ?");
            params.push(OwnedVal::Int(i64::from(kind)));
            sql.push_str(" AND d_tag = ?");
            params.push(OwnedVal::Blob(d_tag.to_vec()));
            push_time(sql, params, since, until);
        },
        limit,
    )
}

/// Generic single-letter tag scan — the hard one. `None` when `tags` is empty or
/// any value set is empty (matches nothing).
///
/// Index-served, never a full table scan: the inner subquery selects candidate
/// `event_id`s from `event_tags` via `idx_tags_tci` (and, when `authors`/`kinds`
/// are constrained, the `atci`/`ktci` composites — `event_tags` carries the
/// redundant `pubkey`/`kind`/`created_at` for exactly this), OR-ing each
/// letter's values and keeping only ids that satisfy **every** required letter
/// via `HAVING COUNT(DISTINCT tag_name) = <#letters>` (the AND). All of an
/// event's tag rows share its `pubkey`/`kind`/`created_at`, so pushing the
/// author/kind/time filters into the subquery never corrupts the distinct-letter
/// count. The outer join only fetches `raw`/`received_at_ms` and applies the
/// newest-first order over the small candidate set.
pub(crate) fn build_tags(
    authors: &BTreeSet<PubKey>,
    kinds: &[u32],
    tags: &BTreeMap<char, BTreeSet<String>>,
    since: Option<u64>,
    until: Option<u64>,
    limit: usize,
) -> Option<(String, Vec<OwnedVal>)> {
    if tags.is_empty() || tags.values().any(BTreeSet::is_empty) {
        return None;
    }
    let mut sql = String::from(
        "SELECT e.raw, e.received_at_ms FROM events e JOIN \
         (SELECT event_id FROM event_tags WHERE ",
    );
    let mut params: Vec<OwnedVal> = Vec::new();

    // OR across letters; OR within each letter's exact-string value set.
    for (i, (letter, values)) in tags.iter().enumerate() {
        if i > 0 {
            sql.push_str(" OR ");
        }
        sql.push_str("(tag_name = ? AND tag_value IN (");
        params.push(OwnedVal::Text(letter.to_string()));
        for (j, v) in values.iter().enumerate() {
            if j > 0 {
                sql.push(',');
            }
            sql.push('?');
            params.push(OwnedVal::Text(v.clone()));
        }
        sql.push_str("))");
    }

    // Push author/kind/time into the subquery (seeks atci/ktci, not a post-filter).
    if !authors.is_empty() {
        push_in_blobs(&mut sql, &mut params, "pubkey", authors);
    }
    if !kinds.is_empty() {
        push_in_ints(&mut sql, &mut params, "kind", kinds);
    }
    push_time(&mut sql, &mut params, since, until);

    sql.push_str(" GROUP BY event_id HAVING COUNT(DISTINCT tag_name) = ?");
    params.push(OwnedVal::Int(tags.len() as i64));
    sql.push_str(") m ON m.event_id = e.id ORDER BY e.created_at DESC, e.id ASC LIMIT ?");
    params.push(OwnedVal::Int(limit as i64));
    Some((sql, params))
}

/// `idx_events_expires` ascending scan — the NIP-40 reaper path. Strict `<`
/// matches `MemEventStore`/`LmdbEventStore` (`exp < unix_seconds`).
pub(crate) fn build_expiring_before(unix_seconds: u64, limit: usize) -> (String, Vec<OwnedVal>) {
    (
        "SELECT raw, received_at_ms FROM events \
         WHERE expires_at IS NOT NULL AND expires_at < ? \
         ORDER BY expires_at ASC LIMIT ?"
            .to_owned(),
        vec![
            OwnedVal::Int(unix_seconds as i64),
            OwnedVal::Int(limit as i64),
        ],
    )
}

/// Dispatch an [`EngineQuery`] to its builder (the `query_visit` entry). `None`
/// propagates the "matches nothing" shapes so the caller visits no rows.
pub(crate) fn build_query(query: &EngineQuery, limit: usize) -> Option<(String, Vec<OwnedVal>)> {
    match query {
        EngineQuery::AuthorKind {
            author,
            kinds,
            since,
            until,
        } => build_author_kind(author, kinds, *since, *until, limit),
        EngineQuery::AuthorsKind {
            authors,
            kinds,
            since,
            until,
        } => build_authors_kind(authors, kinds, *since, *until, limit),
        EngineQuery::KindTime {
            kinds,
            since,
            until,
        } => Some(build_kind_time(kinds, *since, *until, limit)),
        EngineQuery::KindDtag {
            kind,
            d_tag,
            since,
            until,
        } => Some(build_kind_dtag(*kind, d_tag, *since, *until, limit)),
        EngineQuery::Tags {
            authors,
            kinds,
            tags,
            since,
            until,
        } => build_tags(authors, kinds, tags, *since, *until, limit),
    }
}

#[cfg(test)]
mod tests;
