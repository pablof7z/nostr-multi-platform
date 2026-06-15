//! Publish-policy one-door (Workstream C of the event-flow architecture).
//!
//! # Why this module exists
//!
//! Generic publish / outbox routing used to switch on raw NIP **kind literals**
//! (`if kind == 0`, `if kind == 3`, …) scattered through `publish/action.rs`.
//! That is the write-side mirror of the ingest-side D0 violation that
//! ADR-0057 unified: the kernel substrate must not name NIP kind numbers in its
//! routing logic — the only legal home for a kind→policy mapping is a single
//! **declared policy table** (the substrate-honest equivalent of the ingest
//! parser registry).
//!
//! This module is that table. [`classify_publish_behavior`] is the ONLY place
//! in the publish path where a NIP kind number is consulted to decide publish
//! behaviour; every routing / builder-guard decision elsewhere consults the
//! returned [`PublishBehavior`] enum instead of re-deriving the policy from a
//! literal. A regression gate (`policy/tests.rs`) asserts no new raw kind
//! literal can drive publish behaviour outside this table.
//!
//! # What is NOT classified here (and why)
//!
//! - **`Auto` relay resolution** (write-relays vs. discovery indexers vs.
//!   fail-closed-when-uncached) is owned by the `OutboxResolver` impl in
//!   `nmp-router` (Layer 2, `is_discovery_kind`). That classification correctly
//!   lives with the resolver — it is the resolver's own routing policy, not a
//!   kernel substrate concern, and inverting the dependency arrow to consult it
//!   from `nmp-core` would break D0. This table covers only the policy the
//!   **kernel/action substrate** owns: which kinds a *raw app publish* may emit
//!   versus which are reserved to a typed builder, and the routing-class shape
//!   that makes invalid `Auto` impossible by type.
//! - **Private / gift-wrap fail-closed routing** is achieved by the DM publish
//!   path using `PublishTarget::Explicit { relays }` (never `Auto`), enforced by
//!   the D10 doctrine lint. [`PublishBehavior::PrivateFailClosed`] records that
//!   invariant in the table so the policy is declared in one place, but the
//!   mechanism that *enforces* it (Explicit-only routing + D10 lint) is
//!   unchanged by this module — this is a refactor of how the policy is
//!   *expressed*, not a behaviour change.

use crate::kinds::{
    is_parameterized_replaceable, is_replaceable, KIND_BLOCKED_RELAYS, KIND_CHAT_MESSAGE,
    KIND_CONTACT_LIST, KIND_DM_RELAY_LIST, KIND_GIFT_WRAP, KIND_MUTE_LIST, KIND_PROFILE_METADATA,
    KIND_RELAY_LIST,
};

/// Typed behaviour class for a kind on the publish/outbox path.
///
/// One value per event kind, derived solely by [`classify_publish_behavior`].
/// The publish action / routing code consults this enum instead of comparing
/// `kind` against a raw literal, so the kind→policy mapping has exactly one
/// home (D0: no scattered kind literals on the write path).
///
/// The variants are ordered most-specific-first; [`classify_publish_behavior`]
/// returns the first matching class.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PublishBehavior {
    /// Reserved replaceable that ONLY a dedicated typed builder may publish —
    /// a raw app publish (`PublishRaw`) must be rejected so it cannot bypass
    /// the builder's protocol-specific processing.
    ///
    /// Carries the typed reason the builder owns the kind so the rejection
    /// message is derived from the table, not hand-written per call site.
    ReservedBuilderOnly(ReservedKind),
    /// Private / encrypted-envelope event (NIP-59 gift-wrap and the sealed
    /// NIP-17 chat message) that MUST fail closed: it is never routed to
    /// `Auto` / public Content relays — only to an `Explicit` recipient-inbox
    /// relay set (D10). The DM publish path enforces this via
    /// `PublishTarget::Explicit`; this class declares the invariant so the
    /// policy is visible in one place.
    PrivateFailClosed,
    /// Replaceable / addressable kind that an app may publish raw AND that the
    /// resolver additionally fans out to discovery indexers when the author has
    /// no cached write relays (the resolver owns the indexer fan-out itself;
    /// this class only records that the kind is discovery-indexable).
    DiscoveryIndexable,
    /// An ordinary public event (notes, reactions, custom app kinds) that an
    /// app may publish raw and that routes through the standard outbox.
    PublicRoutable,
}

/// Which dedicated builder reserves a kind, and why a raw publish is refused.
///
/// Each variant maps 1:1 to a `PublishAction` builder variant; the rejection
/// message is rendered from [`ReservedKind::raw_publish_rejection`] so the
/// reason lives next to the classification, not inlined at the guard site.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReservedKind {
    /// kind:0 profile metadata — owned by `PublishAction::PublishProfile`
    /// (flat-JSON field validation + string-typed content guarantee).
    Profile,
    /// kind:3 contact list — owned by `nmp.follow` / `nmp.unfollow`
    /// (follow-list merge; a raw payload would silently overwrite the follow
    /// set).
    Contacts,
}

impl ReservedKind {
    /// The rejection message a raw-publish guard surfaces when an app tries to
    /// publish a reserved kind directly. Single source of truth for the
    /// builder-only invariant's wording.
    pub(crate) fn raw_publish_rejection(self) -> String {
        match self {
            Self::Profile => {
                "use PublishProfile (not PublishRaw) for kind:0 profile updates".to_string()
            }
            Self::Contacts => "kind:3 contact-list must be modified via nmp.follow / nmp.unfollow, \
                 not PublishRaw (the actor owns the follow-list state)"
                .to_string(),
        }
    }
}

/// Classify an event kind into its [`PublishBehavior`]. **The single door** for
/// kind→publish-policy on the write path: every routing / builder-guard
/// decision consults this, and this is the only function permitted to compare a
/// publish kind against a named kind constant.
///
/// Most-specific-first: reserved-builder kinds, then private envelopes, then
/// discovery-indexable replaceables, then the public-routable default.
pub(crate) fn classify_publish_behavior(kind: u32) -> PublishBehavior {
    // 1. Reserved replaceables — a raw app publish is refused so the typed
    //    builder's protocol processing cannot be bypassed.
    if kind == KIND_PROFILE_METADATA {
        return PublishBehavior::ReservedBuilderOnly(ReservedKind::Profile);
    }
    if kind == KIND_CONTACT_LIST {
        return PublishBehavior::ReservedBuilderOnly(ReservedKind::Contacts);
    }

    // 2. Private / encrypted envelopes — fail closed (Explicit-only, D10).
    //    kind:14 (sealed NIP-17 chat message) and kind:1059 (gift-wrap) are the
    //    private envelope kinds the workspace writes; they never route to
    //    public relays via Auto.
    if kind == KIND_GIFT_WRAP || kind == KIND_CHAT_MESSAGE {
        return PublishBehavior::PrivateFailClosed;
    }

    // 3. Discovery-indexable replaceables — relay lists / mute / blocked-relay
    //    lists and other NIP-51 replaceables (10000–19999) the resolver also
    //    fans out to indexers. (kind:0 / kind:3 are discovery kinds too but are
    //    already classified ReservedBuilderOnly above — the reserved-builder
    //    invariant is the stricter, governing class for them.)
    if kind == KIND_RELAY_LIST
        || kind == KIND_DM_RELAY_LIST
        || kind == KIND_MUTE_LIST
        || kind == KIND_BLOCKED_RELAYS
        || (10_000..=19_999).contains(&kind)
    {
        return PublishBehavior::DiscoveryIndexable;
    }

    // 4. Everything else — notes, reactions, custom app kinds, parameterized
    //    addressables (30000–39999) — is publicly routable through the standard
    //    outbox. The replaceable predicates are referenced so the table stays
    //    honest about the full kind space even though the default arm covers
    //    them: a future contributor adding a behaviour split for a replaceable
    //    range edits THIS function, never a literal at a call site.
    let _is_replaceable = is_replaceable(kind);
    let _is_param_replaceable = is_parameterized_replaceable(kind);
    PublishBehavior::PublicRoutable
}

impl PublishBehavior {
    /// `true` when a raw app publish (`PublishRaw`) of this kind must be
    /// rejected in favour of a dedicated typed builder. Returns the typed
    /// reason so the guard renders its message from the table.
    pub(crate) fn reserved_builder(self) -> Option<ReservedKind> {
        match self {
            Self::ReservedBuilderOnly(reserved) => Some(reserved),
            _ => None,
        }
    }
}

#[cfg(test)]
#[path = "policy/tests.rs"]
mod tests;
