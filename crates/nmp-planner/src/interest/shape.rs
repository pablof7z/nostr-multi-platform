//! `InterestShape` — the normalised filter description of a `LogicalInterest`,
//! plus its `#p`-tag inbox routing mode (`PTagRouting`).
//!
//! Owns the deterministically-hashable wire-filter shape (authors, kinds, tags,
//! since/until/limit/search, event ids, address coordinates) and the two
//! client-side-only routing fields (`relay_pin`, `p_tag_routing`) that steer
//! the compiler's relay selection without ever touching the wire filter.
//!
//! Doctrine: D8 (composite reverse index, zero per-event allocs after warmup).

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};

use super::{bounded_search_query, EventId, NaddrCoord, Pubkey, RelayUrl, TagKey, UnixSeconds};

// ─── P-tag routing ──────────────────────────────────────────────────────────

/// Which relay set the compiler must use for `#p`-tag inbox routing.
///
/// The default is the generic NIP-65 read mailbox used by notifications and
/// public inbox-style queries. NIP-17 gift-wrap inboxes deliberately opt into
/// kind:10050 DM relays instead, because those relays are separate from the
/// public kind:10002 read list.
#[derive(Clone, Copy, Debug, Default, Hash, Eq, PartialEq)]
pub enum PTagRouting {
    #[default]
    Nip65ReadRelays,
    Nip17DmRelays,
}

impl PTagRouting {
    pub(crate) fn plan_hash_tag(self) -> u8 {
        match self {
            Self::Nip65ReadRelays => 0,
            Self::Nip17DmRelays => 1,
        }
    }
}

// ─── InterestShape ───────────────────────────────────────────────────────────

/// The normalised filter description for a `LogicalInterest`.
///
/// Mirrors the Nostr filter shape closely. All collections use sorted-container
/// types so equality and hashing are deterministic — required for plan-id
/// stability across recompilations (§3.4 plan-id contract).
///
/// Empty collections mean "wildcard" except where noted.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct InterestShape {
    /// Authors whose events are wanted. Empty = any author (rare; prefer scoped).
    pub authors: BTreeSet<Pubkey>,

    /// Event kinds wanted. Empty = any kind (rare).
    pub kinds: BTreeSet<u32>,

    /// Tag filter dimensions. Each entry is a tag key → sorted set of values.
    /// Sorted for hash stability (D8 composite index invariant).
    pub tags: BTreeMap<TagKey, BTreeSet<String>>,

    /// Lower bound for `created_at`. `None` = no lower bound.
    pub since: Option<UnixSeconds>,

    /// Upper bound for `created_at`. `None` = no upper bound.
    pub until: Option<UnixSeconds>,

    /// Maximum events to return. `None` = relay default.
    /// When set, merge is refused (broadening would mask intent). See Rule 5.
    pub limit: Option<u32>,

    /// Relay-evaluated NIP-50 full-text search string.
    ///
    /// This is a wire filter field (`{"search":"..."}`), but it is not a local
    /// cache predicate: the cache-serve path has no FTS index and must leave
    /// search-bearing shapes to relays. `None` is skipped from serde so existing
    /// non-search shapes keep stable canonical hashes across this field's
    /// introduction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search: Option<String>,

    /// Specific event ids for pointer / thread hydration.
    pub event_ids: BTreeSet<EventId>,

    /// Parameterized-replaceable event coordinates for address-pointer hydration.
    ///
    /// Non-empty when a view needs to resolve a specific `naddr` (e.g., a NIP-23
    /// article in `nmp_nip01::ThreadView` or `nmp_nip01::Nip10ModularTimelineView`).
    /// The compiler routes each coordinate to the addressed author's write relays
    /// (Stage 1 Outbox direction keyed on `NaddrCoord::pubkey`). See Rule 8 and §7
    /// of the design doc.
    ///
    /// Adding `addresses` as a first-class field gives the merge lattice a stable
    /// key to union on, rather than encoding coords into opaque `#a` tag strings.
    ///
    /// Design: `docs/design/subscription-compilation/intro.md` §2.1 (T24).
    pub addresses: BTreeSet<NaddrCoord>,

    /// Hard routing pin: when `Some`, all four-lane routing (Cases A/B/C/D)
    /// is suppressed and the interest goes to exactly this relay.
    ///
    /// This is the third routing lane: some protocols require subscriptions
    /// and publishes to be addressed to a specific host relay regardless of
    /// the author's NIP-65 mailboxes. When a consumer needs that semantics,
    /// it sets `relay_pin = Some(host)` and the planner short-circuits the
    /// four-lane dispatch in `planner::compiler::partition::case_e_relay_pinned`.
    ///
    /// Merge lattice **Rule 9** (in `planner::lattice::rules::rule9_relay_pin`):
    /// two shapes with different `relay_pin` values refuse to merge — they go
    /// to different relays and must produce distinct wire frames. Wildcard
    /// (`None`) does NOT absorb a concrete pin (unlike Rule 1's wildcard for
    /// kinds): a pinned interest is a hard routing override, mixing it with
    /// an unpinned interest would either narrow the unpinned scope or leak the
    /// pinned content to other relays. Two pinned shapes that share the same
    /// host coalesce normally — Rule 2's tag-value union is what collapses
    /// many per-room subscriptions into a single per-host REQ (the "h-tag
    /// coalesce" pattern the third lane is named after).
    ///
    /// `relay_pin` is purely an out-of-band routing hint; it is NEVER
    /// serialized onto the wire as part of the filter. The relay receives only
    /// the regular filter shape (kinds + tags + `since/until/limit/event_ids`
    /// + addresses); routing happens entirely on the client side.
    ///
    /// Example use case: NIP-29 relay-based groups (each group is bound to its
    /// host relay; cross-host merging is forbidden).
    pub relay_pin: Option<RelayUrl>,

    /// Routing mode for `#p`-tag inbox interests.
    ///
    /// This is client-side routing metadata, not a Nostr filter field. It is
    /// skipped by serde so `filter_json_for` / `canonical_filter_hash` remain
    /// wire-filter-only. `compute_plan_id` hashes it explicitly because it
    /// affects relay selection.
    #[serde(skip)]
    pub p_tag_routing: PTagRouting,
}

impl Hash for InterestShape {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.authors.hash(state);
        self.kinds.hash(state);
        self.tags.hash(state);
        self.since.hash(state);
        self.until.hash(state);
        self.limit.hash(state);
        self.event_ids.hash(state);
        self.addresses.hash(state);
        self.relay_pin.hash(state);
        self.p_tag_routing.hash(state);
        if let Some(search) = &self.search {
            "search".hash(state);
            search.hash(state);
        }
    }
}

/// Does `(author, kind, tags)` satisfy the address coordinate `coord`?
/// (`InterestShape::matches_event` §`addresses`, #3091.)
///
/// `kind == coord.kind && author == coord.pubkey` are required unconditionally.
/// The `d`-tag check then branches on whether `coord.d_tag` is empty:
/// - non-empty: the event must carry a `["d", coord.d_tag]` row (addressable
///   kinds 30000–39999, which always have a meaningful `d` tag).
/// - empty: the event must carry NO `d` tag, or a `d` tag whose value is
///   itself empty (non-parameterized replaceable kinds 10000–19999, which
///   never carry a meaningful `d` tag but may echo an empty one).
fn address_coord_matches(
    coord: &NaddrCoord,
    author: &str,
    kind: u32,
    tags: &[Vec<String>],
) -> bool {
    if kind != coord.kind || author != coord.pubkey {
        return false;
    }
    let d_tag_value = tags
        .iter()
        .find(|row| row.first().is_some_and(|k| k == "d"))
        .and_then(|row| row.get(1));
    if coord.d_tag.is_empty() {
        d_tag_value.is_none_or(String::is_empty)
    } else {
        d_tag_value.is_some_and(|v| v == &coord.d_tag)
    }
}

impl InterestShape {
    /// Convenience constructor for a tailing author+kind timeline interest.
    ///
    /// `kinds` is **caller-supplied policy**, not a planner default. The planner
    /// is substrate: it carries whatever kind set a host or NIP module declares
    /// as filter data, but it must not choose app concepts like "a social
    /// timeline means kind:1 + kind:6" (V-68 / D0). The follow-feed call site
    /// threads a compiled acquisition set derived above the planner; app-facing
    /// primary-kind declarations do not live here. An empty set yields a
    /// wildcard-kinds shape.
    #[must_use]
    pub fn timeline_for(authors: BTreeSet<Pubkey>, kinds: BTreeSet<u32>) -> Self {
        Self {
            authors,
            kinds,
            ..Default::default()
        }
    }

    /// Parse a standard NIP-01 REQ filter JSON object into an [`InterestShape`].
    ///
    /// This is the inverse of `nmp_core::subs::wire::filter_json_for` and the
    /// app-facing entry point for the M2 `open_interest` / `close_interest`
    /// C-ABI surface (ADR-0076): the host passes a verbatim Nostr filter string
    /// (e.g. `{"kinds":[1],"#t":["nostr"]}`) and the substrate derives a
    /// deterministically-hashable shape from it. Two call sites passing the same
    /// filter — regardless of JSON key ordering or array element ordering — map
    /// to the same shape (every collection field is a sorted container), which
    /// gives the registry deterministic `(scope, key)` dedup.
    ///
    /// Field mapping (NIP-01 → `InterestShape`):
    /// - `kinds`   → `kinds`
    /// - `authors` → `authors`
    /// - `ids`     → `event_ids`
    /// - `#a`      → `addresses`, decoded into `NaddrCoord` triples — the
    ///   exact inverse of `filter_json_for`'s `Coordinate::to_string()`
    ///   serialisation (`"<kind>:<pubkey>:<d-tag>"`, always exactly two
    ///   colons since `Coordinate::Display` always emits the identifier
    ///   segment even when empty)
    /// - `#<x>` (any other single letter) → `tags` (one entry per
    ///   single-letter generic-tag key)
    /// - `since` / `until` / `limit` → the same-named fields
    /// - `search`  → bounded NIP-50 relay search string
    ///
    /// Client-side-only routing fields (`relay_pin`, `p_tag_routing`) have no
    /// NIP-01 wire representation and are never set by this parser; they keep
    /// their `Default` values. `addresses`, in contrast, DOES round-trip
    /// (#3091): previously a `#a` tag fell through the generic
    /// single-letter-tag branch into the opaque `shape.tags["a"]` entry
    /// instead of `shape.addresses`, which was lossy — any caller that
    /// re-derives its shape from the compiled wire filter JSON (e.g. the
    /// `open_live_only` fallback path in
    /// `nmp_core::kernel::observer_replay::open_interest_with_observer_replay`)
    /// would see an empty `addresses` set and lose the coordinate predicate
    /// `matches_event` now applies (see below).
    ///
    /// Returns `None` when `json` is not a JSON object (D6 — the FFI shim maps
    /// `None` to a silent no-op + diagnostic toast, never a panic). Unknown
    /// top-level keys and malformed field *values* (e.g. a non-array `kinds`)
    /// are tolerated: the field is skipped, mirroring `filter_json_for`'s
    /// drop-on-invalid behaviour, so a partially-malformed filter still yields
    /// the well-formed subset rather than failing the whole subscription.
    #[must_use]
    pub fn from_filter_json(json: &str) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_str(json).ok()?;
        let obj = value.as_object()?;

        let mut shape = Self::default();

        for (key, val) in obj {
            match key.as_str() {
                "kinds" => {
                    if let Some(arr) = val.as_array() {
                        for k in arr.iter().filter_map(serde_json::Value::as_u64) {
                            shape.kinds.insert(k as u32);
                        }
                    }
                }
                "authors" => {
                    if let Some(arr) = val.as_array() {
                        for a in arr.iter().filter_map(serde_json::Value::as_str) {
                            shape.authors.insert(a.to_string());
                        }
                    }
                }
                "ids" => {
                    if let Some(arr) = val.as_array() {
                        for id in arr.iter().filter_map(serde_json::Value::as_str) {
                            shape.event_ids.insert(id.to_string());
                        }
                    }
                }
                "since" => {
                    if let Some(n) = val.as_u64() {
                        shape.since = Some(n);
                    }
                }
                "until" => {
                    if let Some(n) = val.as_u64() {
                        shape.until = Some(n);
                    }
                }
                "limit" => {
                    if let Some(n) = val.as_u64() {
                        shape.limit = Some(n as u32);
                    }
                }
                "search" => {
                    if let Some(search) = val.as_str() {
                        shape.search = bounded_search_query(search);
                    }
                }
                "#a" => {
                    // Address-coordinate tag: exact inverse of
                    // `filter_json_for`'s `Coordinate::to_string()`
                    // (`"<kind>:<pubkey>:<d-tag>"`). Split into at most 3
                    // parts so a `d_tag` containing a literal `:` round-trips
                    // intact (unlike `nostr::Coordinate::from_kpi_format`,
                    // which truncates at the first extra colon). Malformed
                    // entries (missing a segment, non-numeric kind) are
                    // tolerated and skipped, mirroring every other field's
                    // drop-on-invalid behaviour in this parser.
                    if let Some(arr) = val.as_array() {
                        for coord_str in arr.iter().filter_map(serde_json::Value::as_str) {
                            let mut parts = coord_str.splitn(3, ':');
                            let (Some(kind_str), Some(pubkey), Some(d_tag)) =
                                (parts.next(), parts.next(), parts.next())
                            else {
                                continue;
                            };
                            let Ok(kind) = kind_str.parse::<u32>() else {
                                continue;
                            };
                            shape.addresses.insert(NaddrCoord {
                                pubkey: pubkey.to_string(),
                                kind,
                                d_tag: d_tag.to_string(),
                            });
                        }
                    }
                }
                other => {
                    // Generic single-letter tag filter: `#e`, `#t`, `#p`, …
                    // NIP-01 generic tag keys are `#` + a single ASCII letter;
                    // the stored `TagKey` drops the leading `#` to match the
                    // `InterestShape::tags` convention (`filter_json_for`
                    // re-adds it via `custom_tags`).
                    if let Some(letter) = other.strip_prefix('#') {
                        let mut chars = letter.chars();
                        let (Some(_c), None) = (chars.next(), chars.next()) else {
                            continue;
                        };
                        if let Some(arr) = val.as_array() {
                            let entry = shape.tags.entry(letter.to_string()).or_default();
                            for v in arr.iter().filter_map(serde_json::Value::as_str) {
                                entry.insert(v.to_string());
                            }
                        }
                    }
                    // Any other unknown key is tolerated and skipped.
                }
            }
        }

        Some(shape)
    }

    /// Does an inbound event match this interest's wire filter? (ADR-0076 §5.1)
    ///
    /// This is the client-side analogue of a relay's NIP-01 REQ filter match,
    /// used by `Kernel::should_store_event` to admit an event that satisfies an
    /// active generic `open_interest` (a non-followed author, an arbitrary
    /// thread, or a `#t` hashtag feed) into the read-cache so the feed-engine
    /// observer fan-out can expose it.
    ///
    /// Only the **wire** dimensions are checked (the ones a relay would honour):
    /// `authors`, `kinds`, `event_ids` (NIP-01 `ids`), `since`/`until`, the
    /// single-letter generic `tags` (`#e`/`#p`/`#t`/…), and `addresses` (the
    /// `#a` coordinate dimension — see below). Empty collection = wildcard
    /// (NIP-01 semantics). The client-side-only routing fields (`relay_pin`,
    /// `p_tag_routing`, `limit`) are NOT match predicates. `search` is
    /// relay-evaluated only in this substrate slice because there is no local
    /// FTS index yet. A default (all-wildcard) shape matches every event,
    /// mirroring an empty `{}` REQ filter.
    ///
    /// `tags` is the event's raw tag rows (`[["t","nostr"],["e","<id>"],…]`),
    /// exactly the `Vec<Vec<String>>` the kernel ingest path holds. Only
    /// single-letter tag keys participate (NIP-01 generic-tag query semantics).
    ///
    /// ## `addresses` (#3091)
    ///
    /// When `addresses` is non-empty the event must match **at least one**
    /// coordinate (OR within the dimension, same as a relay's `#a` filter,
    /// then AND'd with every other populated dimension — same composition as
    /// `tags`). An event matches a coordinate `NaddrCoord { pubkey, kind,
    /// d_tag }` iff `kind == coord.kind && author == coord.pubkey` AND:
    /// - `coord.d_tag` non-empty: the event carries a `["d", coord.d_tag]` tag
    ///   row (parameterized-replaceable / addressable kinds 30000–39999).
    /// - `coord.d_tag` empty: the event carries NO `d` tag, or a `d` tag with
    ///   an empty value (non-parameterized replaceable kinds 10000–19999,
    ///   which never carry a meaningful `d` tag — `Coordinate::Display`
    ///   still emits the trailing empty identifier segment for these).
    ///
    /// Previously `addresses` was ignored entirely here, so live delivery for
    /// an addressable-target shape matched on kind alone — over-delivery that
    /// downstream consumers (e.g. `PointerIngest::on_kernel_event`) happened
    /// to self-filter on id/coordinate. This predicate makes the kernel
    /// itself precise, matching `cache_serve`'s existing `KindDtag` +
    /// `pubkey_guard_for_address` semantics (`kernel/cache_serve/queries.rs`,
    /// `kernel/pull/predicate.rs`).
    #[must_use]
    pub fn matches_event(
        &self,
        author: &str,
        kind: u32,
        created_at: UnixSeconds,
        tags: &[Vec<String>],
    ) -> bool {
        if !self.authors.is_empty() && !self.authors.contains(author) {
            return false;
        }
        if !self.kinds.is_empty() && !self.kinds.contains(&kind) {
            return false;
        }
        if let Some(since) = self.since {
            if created_at < since {
                return false;
            }
        }
        if let Some(until) = self.until {
            if created_at > until {
                return false;
            }
        }
        if !self.addresses.is_empty()
            && !self
                .addresses
                .iter()
                .any(|coord| address_coord_matches(coord, author, kind, tags))
        {
            return false;
        }
        // NIP-01 `ids`: the event's own id must be one of `event_ids`. The
        // event id is row-independent, but the kernel holds it separately; the
        // caller threads it as a synthetic `["id", <hex>]`-free check is not
        // possible here, so `event_ids` matching is delegated to the caller via
        // the `authors`/`kinds`/`tags` path. (Thread feeds match via `#e` tags,
        // not bare `ids`, so this is not load-bearing for the M2 verbs.)
        //
        // Generic single-letter tag query: for each required tag dimension the
        // event must carry at least one row `[key, value, …]` whose value is in
        // the required set (AND across dimensions, OR within a dimension —
        // NIP-01 §generic-tag-queries).
        for (tag_key, wanted) in &self.tags {
            if wanted.is_empty() {
                continue;
            }
            // Only single-letter keys are wire-queryable.
            if tag_key.len() != 1 {
                continue;
            }
            let satisfied = tags.iter().any(|row| {
                row.first().is_some_and(|k| k == tag_key)
                    && row.get(1).is_some_and(|v| wanted.contains(v))
            });
            if !satisfied {
                return false;
            }
        }
        true
    }

    /// Like [`matches_event`](Self::matches_event) but also honours the NIP-01
    /// `ids` dimension (`event_ids`): when non-empty, the event's own id must be
    /// listed. Threaded separately because the kernel holds the event id apart
    /// from the tag rows.
    #[must_use]
    pub fn matches_event_with_id(
        &self,
        event_id: &str,
        author: &str,
        kind: u32,
        created_at: UnixSeconds,
        tags: &[Vec<String>],
    ) -> bool {
        if !self.event_ids.is_empty() && !self.event_ids.contains(event_id) {
            return false;
        }
        self.matches_event(author, kind, created_at, tags)
    }

    /// Convenience constructor for a one-shot profile fetch.
    ///
    /// Fetches all indexer-relevant replaceable events for the author:
    /// kind:0 (profile), kind:3 (contact list), kind:10002 (NIP-65 relay list).
    #[must_use]
    pub fn profile_for(pubkey: Pubkey) -> Self {
        Self {
            authors: [pubkey].into_iter().collect(),
            kinds: [0u32, 3, 10002].into_iter().collect(),
            limit: Some(3),
            ..Default::default()
        }
    }
}
