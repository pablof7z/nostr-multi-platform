//! `LogicalInterest`, `InterestShape`, and `NaddrCoord` types.
//!
//! A logical interest is what a kernel-side consumer (view, action, monitor,
//! sync job, or pointer loader) wants alive on the wire. The compiler in
//! `planner::compiler` turns N logical interests into M ≤ N per-relay plans.
//!
//! Design: `docs/design/subscription-compilation/intro.md` §2.1
//! Doctrine: D3 (outbox routing automatic), D6 (errors are internal Results),
//!           D8 (composite reverse index, zero per-event allocs after warmup).

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};

// ─── Type aliases (lightweight; no nostr-sdk dep) ────────────────────────────

/// Hex-encoded 64-char pubkey.
pub type Pubkey = String;

/// Hex-encoded 64-char event id.
pub type EventId = String;

/// A `wss://` URL for a relay.
///
/// Transparent `String` alias (grep-able, swappable). The same alias lives in
/// `nmp_core::relay::RelayUrl` and `nmp_store::RelayUrl`; the three are
/// definitionally identical (`pub type RelayUrl = String`) so a value
/// produced in one crate flows into the others without conversion.
pub type RelayUrl = String;

/// Unix timestamp in seconds.
pub type UnixSeconds = u64;

/// A Nostr tag key (e.g. "e", "p", "t", "a").
pub type TagKey = String;

/// Maximum UTF-8 scalar count accepted for a relay NIP-50 search query.
///
/// This is a substrate safety bound, not product policy. Search modules/apps
/// remain free to reject or refine user-entered queries before they reach the
/// planner, but the planner never forwards unbounded text into a relay filter.
pub const MAX_SEARCH_QUERY_CHARS: usize = 256;

/// Normalize and bound a relay NIP-50 search query.
///
/// Empty / whitespace-only input is treated as absent. Non-empty input is
/// trimmed and truncated by Unicode scalar count so UTF-8 boundaries are never
/// split.
#[must_use]
pub fn bounded_search_query(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.chars().take(MAX_SEARCH_QUERY_CHARS).collect())
}

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

// ─── InterestId ──────────────────────────────────────────────────────────────

/// Stable identity assigned by the planner registry on first insertion.
/// Two interests with identical content get distinct ids if registered by
/// distinct claims (the registry is the authority, not content hashing).
#[derive(Clone, Debug, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct InterestId(pub u64);

// ─── NaddrCoord ──────────────────────────────────────────────────────────────

/// A parameterized-replaceable event coordinate: the triple that uniquely
/// identifies an addressable event (kinds 10000–19999, 30000–39999) across
/// all relays. Equivalent to the `naddr` bech32 encoding without the relay hint.
///
/// Used by `InterestShape::addresses` for address-pointer hydration (Rule 8
/// of the merge lattice) and by the D8 composite reverse index to deduplicate
/// address-pointer interests across views.
///
/// Design: `docs/design/subscription-compilation/intro.md` §2.1
#[derive(Clone, Debug, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct NaddrCoord {
    /// Author of the addressed event.
    pub pubkey: Pubkey,
    /// Addressable kind (10000–19999 or 30000–39999).
    pub kind: u32,
    /// The `d` tag value; empty string for events with no `d` tag.
    pub d_tag: String,
}

// Phase 2 (nmp-nip19): NaddrCoord::from_naddr_bech32 / to_naddr_bech32 helpers
// land when the nmp-nip19 bech32 codec crate joins the workspace. Both helpers
// are needed for `nmp_nip01::ThreadView` and `nmp_nip01::Nip10ModularTimelineView`
// (the latter wrapping `nmp_threading::Grouper`) to accept user-facing naddr
// strings from the host-language FFI surface.

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
    /// C-ABI surface (ADR-0042): the host passes a verbatim Nostr filter string
    /// (e.g. `{"kinds":[1,6],"authors":["<hex>"]}`) and the substrate derives a
    /// deterministically-hashable shape from it. Two call sites passing the same
    /// filter — regardless of JSON key ordering or array element ordering — map
    /// to the same shape (every collection field is a sorted container), which
    /// gives the registry deterministic `(scope, key)` dedup.
    ///
    /// Field mapping (NIP-01 → `InterestShape`):
    /// - `kinds`   → `kinds`
    /// - `authors` → `authors`
    /// - `ids`     → `event_ids`
    /// - `#<x>`    → `tags` (one entry per single-letter generic-tag key)
    /// - `since` / `until` / `limit` → the same-named fields
    /// - `search`  → bounded NIP-50 relay search string
    ///
    /// Client-side-only routing fields (`relay_pin`, `p_tag_routing`,
    /// `addresses`) have no NIP-01 wire representation and are never set by this
    /// parser; they keep their `Default` values. The `#a` address-coordinate
    /// tag, if present, is carried through as an opaque `tags["a"]` entry rather
    /// than decoded into `NaddrCoord` — `open_interest` feeds are plain tailing
    /// subscriptions, not address-pointer hydration (that path is `claim_event`).
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

    /// Does an inbound event match this interest's wire filter? (ADR-0042 §5.1)
    ///
    /// This is the client-side analogue of a relay's NIP-01 REQ filter match,
    /// used by `Kernel::should_store_event` to admit an event that satisfies an
    /// active generic `open_interest` (a non-followed author, an arbitrary
    /// thread, or a `#t` hashtag feed) into the read-cache so the feed-engine
    /// observer fan-out can expose it.
    ///
    /// Only the **wire** dimensions are checked (the ones a relay would honour):
    /// `authors`, `kinds`, `event_ids` (NIP-01 `ids`), `since`/`until`, and the
    /// single-letter generic `tags` (`#e`/`#p`/`#t`/…). Empty collection =
    /// wildcard (NIP-01 semantics). The client-side-only routing fields
    /// (`relay_pin`, `p_tag_routing`, `addresses`, `limit`) are NOT match
    /// predicates. `search` is relay-evaluated only in this substrate slice
    /// because there is no local FTS index yet. A default (all-wildcard) shape
    /// matches every event, mirroring an empty `{}` REQ filter.
    ///
    /// `tags` is the event's raw tag rows (`[["t","nostr"],["e","<id>"],…]`),
    /// exactly the `Vec<Vec<String>>` the kernel ingest path holds. Only
    /// single-letter tag keys participate (NIP-01 generic-tag query semantics).
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

// ─── InterestLifecycle ───────────────────────────────────────────────────────

/// Controls when the compiler's wire-emitter closes the REQ.
#[derive(Clone, Debug, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub enum InterestLifecycle {
    /// Stay open after EOSE (tailing subscription).
    Tailing,
    /// Send CLOSE on EOSE.
    OneShot,
}

// ─── InterestScope ───────────────────────────────────────────────────────────

/// Determines which account context the compiler uses for mailbox resolution.
#[derive(Clone, Debug, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub enum InterestScope {
    /// Bound to the active account in `SessionState`. Re-routes on account switch.
    ActiveAccount,
    /// Bound to a specific account. Re-routes on that account's mailbox refresh.
    Account(String),
    /// No account context. Used for global pointer loaders and indexer probes.
    Global,
}

// ─── RelayHint ───────────────────────────────────────────────────────────────

/// A routing hint the consumer wants honoured.
/// The compiler may ignore hints that conflict with policy (e.g. privacy).
#[derive(Clone, Debug, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct RelayHint {
    /// The relay URL suggested as a hint source.
    pub url: RelayUrl,
    /// Why this hint was provided.
    pub source: HintSource,
}

/// Origin of a relay hint.
#[derive(Clone, Debug, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub enum HintSource {
    /// Encoded in an event tag (e.g., `e`-tag position 2).
    EventTag {
        event_id: EventId,
        tag: TagKey,
        position: u8,
    },
    /// Declared by the user in app config.
    UserConfigured,
    /// Observed as the provenance relay for a prior event.
    Provenance { event_id: EventId },
}

// ─── LogicalInterest ─────────────────────────────────────────────────────────

/// A logical interest is the actor-internal, semantics-preserving description
/// of what a view, action, or monitor wants the kernel to keep alive on the
/// wire. It is the input to compilation; it is *not* a Nostr filter.
///
/// Design: `docs/design/subscription-compilation/intro.md` §2
/// Doctrine: D3 (outbox routing), D6 (planner errors never cross FFI),
///           D8 (zero per-event allocs after warmup).
#[derive(Clone, Debug, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct LogicalInterest {
    /// Stable identity assigned by the registry. Survives recompilation.
    pub id: InterestId,

    /// Account-scope context for mailbox resolution.
    pub scope: InterestScope,

    /// What the consumer wants (normalised, deterministically hashable).
    pub shape: InterestShape,

    /// Optional routing hints (may be ignored by policy).
    pub hints: Vec<RelayHint>,

    /// Lifecycle: when to close the resulting REQ.
    pub lifecycle: InterestLifecycle,

    /// PD-033-C planner-extension gate: marks an interest as a
    /// discovery-direction probe (kind:0 profile, kind:3 contacts,
    /// kind:10002 NIP-65 relay list, kind:10050 DM-relay list, …) for
    /// authors whose NIP-65 mailbox isn't cached yet.
    ///
    /// When `true` AND the author's NIP-65 mailbox is unknown AND no
    /// `app_relays` are configured, `case_a_authors` routes the interest
    /// onto `bootstrap_indexer_relays` (the same lane the retired M1
    /// `kernel/discovery.rs::drain_unknown_oneshots` profile-oneshot arm
    /// used). When `false`, the same author falls through to
    /// `unroutable` so the kernel can surface the standard UI toast.
    ///
    /// Defaults to `false` so non-bootstrap call sites (view modules,
    /// reactive timeline subscriptions, follow-feed registrations)
    /// retain the pre-PD-033-C unroutable semantics without an explicit
    /// opt-out. `#[serde(default)]` so older serialised interests
    /// without the field round-trip cleanly through reload paths.
    #[serde(default)]
    pub is_indexer_discovery: bool,
}

impl Default for LogicalInterest {
    fn default() -> Self {
        Self {
            id: InterestId(0),
            scope: InterestScope::Global,
            shape: InterestShape::default(),
            hints: Vec::new(),
            lifecycle: InterestLifecycle::OneShot,
            is_indexer_discovery: false,
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "interest/tests.rs"]
mod tests;
