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

/// A `wss://` URL for a relay. Single crate-wide definition lives in
/// `crate::relay`; re-exported here so `planner` import paths are unchanged.
pub use crate::relay::RelayUrl;

/// Unix timestamp in seconds.
pub type UnixSeconds = u64;

/// A Nostr tag key (e.g. "e", "p", "t", "a").
pub type TagKey = String;

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
// strings from the Swift/Kotlin FFI surface.

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
    /// the regular filter shape (kinds + tags + since/until/limit/event_ids
    /// + addresses); routing happens entirely on the client side.
    ///
    /// Example use case: NIP-29 relay-based groups (each group is bound to its
    /// host relay; cross-host merging is forbidden).
    pub relay_pin: Option<RelayUrl>,
}

impl InterestShape {
    /// Convenience constructor for a tailing author+kind timeline interest.
    pub fn timeline_for(authors: BTreeSet<Pubkey>) -> Self {
        Self {
            authors,
            kinds: [1u32, 6u32].into_iter().collect(),
            ..Default::default()
        }
    }

    /// Convenience constructor for a one-shot profile fetch.
    ///
    /// Fetches all indexer-relevant replaceable events for the author:
    /// kind:0 (profile), kind:3 (contact list), kind:10002 (NIP-65 relay list).
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
    /// Send CLOSE on EOSE or when the deadline (Unix ms) passes.
    BoundedTime { until_ms: u64 },
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
}

impl Default for LogicalInterest {
    fn default() -> Self {
        Self {
            id: InterestId(0),
            scope: InterestScope::Global,
            shape: InterestShape::default(),
            hints: Vec::new(),
            lifecycle: InterestLifecycle::OneShot,
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

    #[test]
    fn timeline_for_sets_authors_and_kinds_only() {
        let authors: BTreeSet<Pubkey> = [hex("aa"), hex("bb")].into_iter().collect();
        let shape = InterestShape::timeline_for(authors.clone());

        // Authors carried through verbatim.
        assert_eq!(shape.authors, authors);
        // Timeline = text notes (kind 1) + reposts (kind 6).
        assert_eq!(
            shape.kinds,
            [1u32, 6u32].into_iter().collect::<BTreeSet<u32>>()
        );
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
            [0u32, 3u32, 10002u32].into_iter().collect::<BTreeSet<u32>>()
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
            ["nostr".to_string(), "rust".to_string()].into_iter().collect(),
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
        };

        assert_eq!(shape.authors.len(), 1);
        assert!(shape.authors.contains(&hex("aa")));
        assert_eq!(
            shape.kinds,
            [1u32, 7u32].into_iter().collect::<BTreeSet<u32>>()
        );
        assert_eq!(
            shape.tags.get("t").map(|v| v.len()),
            Some(2),
        );
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
    fn interest_shape_equality_is_field_wise_and_deterministic() {
        // Two shapes built independently with the same field values must be
        // equal — the §3.4 plan-id stability contract depends on this.
        let a = InterestShape::timeline_for([hex("aa")].into_iter().collect());
        let b = InterestShape::timeline_for([hex("aa")].into_iter().collect());
        assert_eq!(a, b);

        // A different author set breaks equality.
        let c = InterestShape::timeline_for([hex("bb")].into_iter().collect());
        assert_ne!(a, c);
    }
}
