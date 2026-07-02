//! `LogicalInterest` and its constituents.
//!
//! Owns the actor-internal interest aggregate together with the satellite types
//! that make it up: its registry identity (`InterestId`), account-scope context
//! (`InterestScope`), REQ lifecycle (`InterestLifecycle`), and consumer routing
//! hints (`RelayHint` / `HintSource`).
//!
//! Doctrine: D3 (outbox routing), D6 (planner errors never cross FFI),
//!           D8 (zero per-event allocs after warmup).

use serde::{Deserialize, Serialize};

use super::{EventId, InterestShape, RelayUrl, TagKey};

// ─── InterestId ──────────────────────────────────────────────────────────────

/// Stable identity assigned by the planner registry on first insertion.
/// Two interests with identical content get distinct ids if registered by
/// distinct claims (the registry is the authority, not content hashing).
#[derive(Clone, Debug, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct InterestId(pub u64);

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
