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
#[derive(Clone, Debug, Default, Hash, Eq, PartialEq, Serialize, Deserialize)]
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

impl InterestShape {
    /// Convenience constructor for a tailing author+kind timeline interest.
    ///
    /// `kinds` is **caller-supplied policy**, not a planner default. The planner
    /// is substrate: it carries whatever kind set a host or NIP module declares
    /// as filter data, but it must not choose app concepts like "a social
    /// timeline means kind:1 + kind:6" (V-68 / D0). The follow-feed call site
    /// threads the host-declared set (e.g. Chirp's `{1, 6}` from
    /// `ActorCommand::OpenContactFeed { kinds }`); a long-form app would pass
    /// `{30023}`. An empty set yields a wildcard-kinds shape.
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
    /// predicates — they never appear on the wire filter a relay evaluates, so
    /// they must not gate admission. A default (all-wildcard) shape matches
    /// every event, mirroring an empty `{}` REQ filter.
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
mod tests {
    use super::*;

    /// Deterministic 64-char hex pubkey/event-id fixture from a single byte.
    fn hex(byte: &str) -> String {
        byte.repeat(32)
    }

    // ─── matches_event (ADR-0042 §5.1 store-admission predicate) ─────────────

    #[test]
    fn matches_event_default_shape_is_wildcard() {
        // An all-default shape is the `{}` REQ filter: matches everything.
        let shape = InterestShape::default();
        assert!(shape.matches_event(&hex("aa"), 1, 100, &[]));
        assert!(shape.matches_event(&hex("bb"), 30023, 0, &[vec![
            "t".into(),
            "x".into()
        ]]));
    }

    #[test]
    fn matches_event_author_and_kind_and() {
        let mut shape = InterestShape::default();
        shape.authors.insert(hex("aa"));
        shape.kinds.insert(1);

        // Both dimensions satisfied.
        assert!(shape.matches_event(&hex("aa"), 1, 100, &[]));
        // Wrong author.
        assert!(!shape.matches_event(&hex("bb"), 1, 100, &[]));
        // Wrong kind.
        assert!(!shape.matches_event(&hex("aa"), 6, 100, &[]));
    }

    #[test]
    fn matches_event_hashtag_or_within_dimension() {
        let mut shape = InterestShape::default();
        shape.kinds.insert(1);
        shape
            .tags
            .insert("t".to_string(), ["nostr".into(), "bitcoin".into()].into_iter().collect());

        // Event carrying one of the wanted #t values matches.
        assert!(shape.matches_event(&hex("aa"), 1, 100, &[vec!["t".into(), "bitcoin".into()]]));
        // Event with a #t value NOT in the set does not match.
        assert!(!shape.matches_event(&hex("aa"), 1, 100, &[vec!["t".into(), "ethereum".into()]]));
        // Event with no #t tag at all does not match a required #t dimension.
        assert!(!shape.matches_event(&hex("aa"), 1, 100, &[vec!["e".into(), hex("cc")]]));
    }

    #[test]
    fn matches_event_since_until_bounds() {
        let mut shape = InterestShape::default();
        shape.since = Some(100);
        shape.until = Some(200);

        assert!(shape.matches_event(&hex("aa"), 1, 150, &[]));
        assert!(shape.matches_event(&hex("aa"), 1, 100, &[])); // inclusive lower
        assert!(shape.matches_event(&hex("aa"), 1, 200, &[])); // inclusive upper
        assert!(!shape.matches_event(&hex("aa"), 1, 99, &[]));
        assert!(!shape.matches_event(&hex("aa"), 1, 201, &[]));
    }

    #[test]
    fn matches_event_with_id_honours_ids_dimension() {
        let mut shape = InterestShape::default();
        shape.event_ids.insert(hex("11"));

        assert!(shape.matches_event_with_id(&hex("11"), &hex("aa"), 1, 100, &[]));
        assert!(!shape.matches_event_with_id(&hex("22"), &hex("aa"), 1, 100, &[]));
        // `matches_event` (no id dimension) ignores event_ids — the wire-tag
        // path is what thread feeds actually use.
        assert!(shape.matches_event(&hex("22"), 1, 100, &[]));
    }

    #[test]
    fn matches_event_ignores_client_side_only_fields() {
        // `limit` is a client-side cap, never a relay match predicate.
        let mut shape = InterestShape::default();
        shape.kinds.insert(1);
        shape.limit = Some(1);
        // Two events both match despite limit=1 — limit must not gate admission.
        assert!(shape.matches_event(&hex("aa"), 1, 100, &[]));
        assert!(shape.matches_event(&hex("bb"), 1, 101, &[]));
    }

    #[test]
    fn timeline_for_carries_caller_kinds_verbatim() {
        let authors: BTreeSet<Pubkey> = [hex("aa"), hex("bb")].into_iter().collect();
        // V-68: pass an ARBITRARY, non-social kind set to prove the
        // constructor is kind-agnostic — it must not inject {1, 6} or any
        // other app default. A long-form host would declare {30023}.
        let caller_kinds: BTreeSet<u32> = [30023u32, 9999u32].into_iter().collect();
        let shape = InterestShape::timeline_for(authors.clone(), caller_kinds.clone());

        // Authors carried through verbatim.
        assert_eq!(shape.authors, authors);
        // Kinds are exactly what the caller supplied — no substrate policy.
        assert_eq!(shape.kinds, caller_kinds);
        // Every other dimension stays at its wildcard / default.
        assert!(shape.tags.is_empty());
        assert!(shape.event_ids.is_empty());
        assert!(shape.addresses.is_empty());
        assert_eq!(shape.since, None);
        assert_eq!(shape.until, None);
        assert_eq!(shape.limit, None);
        assert_eq!(shape.relay_pin, None);
    }

    #[test]
    fn profile_for_has_exactly_one_author_and_indexer_kinds() {
        let pubkey = hex("cc");
        let shape = InterestShape::profile_for(pubkey.clone());

        // Exactly one author — the requested pubkey.
        assert_eq!(shape.authors.len(), 1);
        assert!(shape.authors.contains(&pubkey));
        // kind:0 profile + kind:3 contacts + kind:10002 NIP-65 relay list.
        assert_eq!(
            shape.kinds,
            [0u32, 3u32, 10002u32]
                .into_iter()
                .collect::<BTreeSet<u32>>()
        );
        // One-shot profile fetch caps at 3 replaceable events.
        assert_eq!(shape.limit, Some(3));
        // No tags / pointers / time bounds / routing pin.
        assert!(shape.tags.is_empty());
        assert!(shape.event_ids.is_empty());
        assert!(shape.addresses.is_empty());
        assert_eq!(shape.since, None);
        assert_eq!(shape.until, None);
        assert_eq!(shape.relay_pin, None);
    }

    #[test]
    fn naddr_coord_equality_depends_on_all_three_fields() {
        let base = NaddrCoord {
            pubkey: hex("aa"),
            kind: 30023,
            d_tag: "my-article".to_string(),
        };
        // Identical triple → equal.
        let same = NaddrCoord {
            pubkey: hex("aa"),
            kind: 30023,
            d_tag: "my-article".to_string(),
        };
        assert_eq!(base, same);

        // Differing pubkey → not equal.
        let other_pubkey = NaddrCoord {
            pubkey: hex("bb"),
            ..base.clone()
        };
        assert_ne!(base, other_pubkey);

        // Differing kind → not equal.
        let other_kind = NaddrCoord {
            kind: 30024,
            ..base.clone()
        };
        assert_ne!(base, other_kind);

        // Differing d_tag → not equal.
        let other_d_tag = NaddrCoord {
            d_tag: "another-article".to_string(),
            ..base.clone()
        };
        assert_ne!(base, other_d_tag);
    }

    #[test]
    fn naddr_coord_dedup_in_btreeset_keys_on_full_triple() {
        // Two coords that share kind+d_tag but differ on pubkey must NOT
        // collapse — the D8 composite index relies on the full triple as key.
        let mut set: BTreeSet<NaddrCoord> = BTreeSet::new();
        set.insert(NaddrCoord {
            pubkey: hex("aa"),
            kind: 30023,
            d_tag: "post".to_string(),
        });
        set.insert(NaddrCoord {
            pubkey: hex("bb"),
            kind: 30023,
            d_tag: "post".to_string(),
        });
        // Re-inserting an exact duplicate is a no-op.
        set.insert(NaddrCoord {
            pubkey: hex("aa"),
            kind: 30023,
            d_tag: "post".to_string(),
        });
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn logical_interest_default_is_one_shot_global_empty() {
        let interest = LogicalInterest::default();

        // Default lifecycle is OneShot (CLOSE on EOSE), not a tailing sub.
        assert_eq!(interest.lifecycle, InterestLifecycle::OneShot);
        // Default scope is Global — no account context.
        assert_eq!(interest.scope, InterestScope::Global);
        // Registry-assigned id starts at the sentinel 0.
        assert_eq!(interest.id, InterestId(0));
        // No hints, and the shape is the empty wildcard default.
        assert!(interest.hints.is_empty());
        assert_eq!(interest.shape, InterestShape::default());
    }

    #[test]
    fn interest_shape_multi_field_round_trips_field_contents() {
        // Build a richly-populated shape and verify each dimension lands.
        let mut tags: BTreeMap<TagKey, BTreeSet<String>> = BTreeMap::new();
        tags.insert(
            "t".to_string(),
            ["nostr".to_string(), "rust".to_string()]
                .into_iter()
                .collect(),
        );

        let addr = NaddrCoord {
            pubkey: hex("dd"),
            kind: 30023,
            d_tag: "long-form".to_string(),
        };

        let shape = InterestShape {
            authors: [hex("aa")].into_iter().collect(),
            kinds: [1u32, 7u32].into_iter().collect(),
            tags: tags.clone(),
            since: Some(1_700_000_000),
            until: Some(1_700_086_400),
            limit: Some(50),
            event_ids: [hex("ee")].into_iter().collect(),
            addresses: [addr.clone()].into_iter().collect(),
            relay_pin: Some("wss://relay.example.com".to_string()),
            p_tag_routing: PTagRouting::Nip65ReadRelays,
        };

        assert_eq!(shape.authors.len(), 1);
        assert!(shape.authors.contains(&hex("aa")));
        assert_eq!(
            shape.kinds,
            [1u32, 7u32].into_iter().collect::<BTreeSet<u32>>()
        );
        assert_eq!(shape.tags.get("t").map(|v| v.len()), Some(2),);
        assert!(shape.tags["t"].contains("nostr"));
        assert!(shape.tags["t"].contains("rust"));
        assert_eq!(shape.since, Some(1_700_000_000));
        assert_eq!(shape.until, Some(1_700_086_400));
        assert_eq!(shape.limit, Some(50));
        assert!(shape.event_ids.contains(&hex("ee")));
        assert!(shape.addresses.contains(&addr));
        assert_eq!(shape.relay_pin.as_deref(), Some("wss://relay.example.com"));
    }

    #[test]
    fn from_filter_json_maps_every_nip01_field() {
        let json = format!(
            r##"{{"kinds":[1,6],"authors":["{}"],"ids":["{}"],"#e":["{}"],"#t":["bitcoin","nostr"],"since":100,"until":200,"limit":50}}"##,
            hex("aa"),
            hex("bb"),
            hex("cc"),
        );
        let shape = InterestShape::from_filter_json(&json).expect("valid object");

        assert_eq!(shape.kinds, [1u32, 6u32].into_iter().collect());
        assert_eq!(shape.authors, [hex("aa")].into_iter().collect());
        assert_eq!(shape.event_ids, [hex("bb")].into_iter().collect());
        assert_eq!(
            shape.tags.get("e").map(|s| s.iter().cloned().collect::<Vec<_>>()),
            Some(vec![hex("cc")])
        );
        assert_eq!(
            shape.tags.get("t").map(|s| s.len()),
            Some(2)
        );
        assert!(shape.tags["t"].contains("bitcoin"));
        assert!(shape.tags["t"].contains("nostr"));
        assert_eq!(shape.since, Some(100));
        assert_eq!(shape.until, Some(200));
        assert_eq!(shape.limit, Some(50));
        // Client-side-only fields are never set by the parser.
        assert!(shape.addresses.is_empty());
        assert_eq!(shape.relay_pin, None);
    }

    #[test]
    fn from_filter_json_is_order_independent_for_dedup() {
        // The whole point of the InterestShape-hash dedup: two filter strings
        // that differ only in JSON key order AND array element order must parse
        // to byte-identical shapes so the registry collapses them to one slot.
        let a = InterestShape::from_filter_json(
            r#"{"kinds":[1,6],"authors":["aa","bb"]}"#,
        )
        .unwrap();
        let b = InterestShape::from_filter_json(
            r#"{"authors":["bb","aa"],"kinds":[6,1]}"#,
        )
        .unwrap();
        assert_eq!(a, b, "key/element ordering must not affect the shape");
    }

    #[test]
    fn from_filter_json_rejects_non_object() {
        assert!(InterestShape::from_filter_json("[]").is_none());
        assert!(InterestShape::from_filter_json("42").is_none());
        assert!(InterestShape::from_filter_json("not json").is_none());
        assert!(InterestShape::from_filter_json("\"a string\"").is_none());
    }

    #[test]
    fn from_filter_json_tolerates_malformed_and_unknown_fields() {
        // Non-array kinds is skipped; unknown top-level key ignored; the valid
        // subset still lands. Multi-char tag keys (`#foo`) are not NIP-01 and
        // are dropped.
        let shape = InterestShape::from_filter_json(
            r##"{"kinds":"oops","authors":["aa"],"weird":true,"#foo":["x"]}"##,
        )
        .expect("still a valid object");
        assert!(shape.kinds.is_empty());
        assert_eq!(shape.authors, ["aa".to_string()].into_iter().collect());
        assert!(shape.tags.is_empty(), "multi-char tag key dropped");
    }

    #[test]
    fn from_filter_json_empty_object_is_wildcard_default() {
        let shape = InterestShape::from_filter_json("{}").unwrap();
        assert_eq!(shape, InterestShape::default());
    }

    #[test]
    fn interest_shape_equality_is_field_wise_and_deterministic() {
        // Two shapes built independently with the same field values must be
        // equal — the §3.4 plan-id stability contract depends on this.
        let kinds: BTreeSet<u32> = [30023u32].into_iter().collect();
        let a = InterestShape::timeline_for([hex("aa")].into_iter().collect(), kinds.clone());
        let b = InterestShape::timeline_for([hex("aa")].into_iter().collect(), kinds.clone());
        assert_eq!(a, b);

        // A different author set breaks equality.
        let c = InterestShape::timeline_for([hex("bb")].into_iter().collect(), kinds);
        assert_ne!(a, c);
    }
}
