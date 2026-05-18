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
pub type RelayUrl = String;

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
// are needed for the ThreadViewModule and MetaTimelineViewModule address-pointer
// loaders to accept user-facing naddr strings from the Swift/Kotlin FFI surface.

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
    /// article in `ThreadViewModule` or `MetaTimelineViewModule`). The compiler
    /// routes each coordinate to the addressed author's write relays (Stage 1
    /// Outbox direction keyed on `NaddrCoord::pubkey`). See Rule 8 and §7 of
    /// the design doc.
    ///
    /// Adding `addresses` as a first-class field gives the merge lattice a stable
    /// key to union on, rather than encoding coords into opaque `#a` tag strings.
    ///
    /// Design: `docs/design/subscription-compilation/intro.md` §2.1 (T24).
    pub addresses: BTreeSet<NaddrCoord>,
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
    pub fn profile_for(pubkey: Pubkey) -> Self {
        Self {
            authors: [pubkey].into_iter().collect(),
            kinds: [0u32].into_iter().collect(),
            limit: Some(1),
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
