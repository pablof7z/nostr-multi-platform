//! Publish-policy one-door (Workstream C of the event-flow architecture).
//!
//! # Why this module exists
//!
//! Generic publish / outbox routing used to switch on raw NIP **kind literals**
//! (`if kind == 0`, `if kind == 3`, …) scattered through `publish/action.rs`.
//! That is the write-side mirror of the ingest-side D0 violation that
//! ADR-0071 unified: the kernel substrate must not name NIP kind numbers in its
//! routing logic — the only legal home for a kind→policy mapping is this
//! registrar-backed publish-policy door (the substrate-honest equivalent of
//! the ingest parser registry).
//!
//! This module is that door. [`classify_publish_behavior`] is the ONLY place
//! in the publish path where a NIP kind number is consulted to decide publish
//! behaviour; every routing / builder-guard decision elsewhere consults the
//! returned [`PublishBehavior`] enum instead of re-deriving the policy from a
//! literal. Protocol crates register their owned public classifications through
//! [`PublishPolicyRegistrar`](crate::substrate::PublishPolicyRegistrar). A regression gate (`policy/tests.rs`) asserts no new raw kind
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
//!   path using an explicit `VerifiedPrivateInbox` route (never `Auto`),
//!   enforced by the D10 doctrine lint. [`PublishBehavior::PrivateFailClosed`]
//!   records that invariant in the table so the policy is declared in one place,
//!   but the mechanism that *enforces* it (Explicit-only routing + D10 lint) is
//!   unchanged by this module — this is a refactor of how the policy is
//!   *expressed*, not a behaviour change.

use std::ops::RangeInclusive;
use std::sync::{OnceLock, RwLock};

use crate::kinds::{is_addressable, is_replaceable, KIND_CHAT_MESSAGE, KIND_GIFT_WRAP};

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
    ReservedBuilderOnly(ReservedBuilderPolicy),
    /// Private / encrypted-envelope event (NIP-59 gift-wrap and the sealed
    /// NIP-17 chat message) that MUST fail closed: it is never routed to
    /// `Auto` / public Content relays — only to a verified recipient-inbox
    /// relay set (D10). The DM publish path enforces this via route provenance;
    /// this class declares the invariant so the policy is visible in one place.
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
/// Protocol crates register the rejection message next to the builder/runtime
/// they own, not inlined at the publish-action guard site.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReservedBuilderPolicy {
    raw_publish_rejection: &'static str,
}

impl ReservedBuilderPolicy {
    #[must_use]
    pub const fn new(raw_publish_rejection: &'static str) -> Self {
        Self {
            raw_publish_rejection,
        }
    }

    /// The rejection message a raw-publish guard surfaces when an app tries to
    /// publish a reserved kind directly. Single source of truth for the
    /// builder-only invariant's wording.
    pub(crate) fn raw_publish_rejection(self) -> String {
        self.raw_publish_rejection.to_string()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublishPolicyRegistrationError {
    kind: u32,
    existing: ReservedBuilderPolicy,
    incoming: ReservedBuilderPolicy,
}

impl std::fmt::Display for PublishPolicyRegistrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "publish policy conflict for kind:{}: existing reserved-builder reason {:?}, incoming {:?}",
            self.kind, self.existing.raw_publish_rejection, self.incoming.raw_publish_rejection
        )
    }
}

impl std::error::Error for PublishPolicyRegistrationError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ExactPublishPolicy {
    kind: u32,
    reserved_builder: Option<ReservedBuilderPolicy>,
    discovery_indexable: bool,
}

#[derive(Clone, Debug, Default)]
struct PublishPolicyRegistry {
    exact: Vec<ExactPublishPolicy>,
    discovery_ranges: Vec<RangeInclusive<u32>>,
}

impl PublishPolicyRegistry {
    fn register_reserved_builder(
        &mut self,
        kind: u32,
        policy: ReservedBuilderPolicy,
    ) -> Result<(), PublishPolicyRegistrationError> {
        let slot = self.exact_slot(kind);
        match slot.reserved_builder {
            Some(existing) if existing != policy => Err(PublishPolicyRegistrationError {
                kind,
                existing,
                incoming: policy,
            }),
            Some(_) => Ok(()),
            None => {
                slot.reserved_builder = Some(policy);
                Ok(())
            }
        }
    }

    fn register_discovery_kind(&mut self, kind: u32) {
        self.exact_slot(kind).discovery_indexable = true;
    }

    fn register_discovery_range(&mut self, range: RangeInclusive<u32>) {
        if !self
            .discovery_ranges
            .iter()
            .any(|existing| existing == &range)
        {
            self.discovery_ranges.push(range);
        }
    }

    fn exact_slot(&mut self, kind: u32) -> &mut ExactPublishPolicy {
        if let Some(index) = self.exact.iter().position(|entry| entry.kind == kind) {
            return &mut self.exact[index];
        }
        self.exact.push(ExactPublishPolicy {
            kind,
            reserved_builder: None,
            discovery_indexable: false,
        });
        let index = self.exact.len() - 1;
        &mut self.exact[index]
    }

    fn reserved_builder(&self, kind: u32) -> Option<ReservedBuilderPolicy> {
        self.exact
            .iter()
            .find(|entry| entry.kind == kind)
            .and_then(|entry| entry.reserved_builder)
    }

    fn is_discovery_indexable(&self, kind: u32) -> bool {
        self.exact
            .iter()
            .any(|entry| entry.kind == kind && entry.discovery_indexable)
            || self
                .discovery_ranges
                .iter()
                .any(|range| range.contains(&kind))
    }
}

static PUBLISH_POLICY_REGISTRY: OnceLock<RwLock<PublishPolicyRegistry>> = OnceLock::new(); // doctrine-allow: D21 - process-wide protocol facts only; no app handles/runtimes/secrets; D10 private fail-closed is hardcoded when registration is absent.

fn registry() -> &'static RwLock<PublishPolicyRegistry> {
    PUBLISH_POLICY_REGISTRY.get_or_init(|| RwLock::new(PublishPolicyRegistry::default()))
}

pub fn register_reserved_publish_builder(
    kind: u32,
    raw_publish_rejection: &'static str,
) -> Result<(), PublishPolicyRegistrationError> {
    registry()
        .write()
        .map(|mut registry| {
            registry
                .register_reserved_builder(kind, ReservedBuilderPolicy::new(raw_publish_rejection))
        })
        .unwrap_or(Ok(()))
}

pub fn register_discovery_indexable_publish_kind(kind: u32) {
    if let Ok(mut registry) = registry().write() {
        registry.register_discovery_kind(kind);
    }
}

pub fn register_discovery_indexable_publish_range(range: RangeInclusive<u32>) {
    if let Ok(mut registry) = registry().write() {
        registry.register_discovery_range(range);
    }
}

/// Classify an event kind into its [`PublishBehavior`]. **The single door** for
/// kind→publish-policy on the write path: every routing / builder-guard
/// decision consults this, and this is the only function permitted to compare a
/// publish kind against a named kind constant or registered protocol policy.
///
/// Most-specific-first: hardcoded private envelopes, then registered
/// reserved-builder kinds, then registered discovery-indexable replaceables,
/// then the public-routable default.
pub(crate) fn classify_publish_behavior(kind: u32) -> PublishBehavior {
    // 1. Private / encrypted envelopes — fail closed (Explicit-only, D10).
    //    This security classification is intentionally hardcoded in core, not
    //    delegated to protocol registration: missing or late composition must
    //    never downgrade a private envelope into PublicRoutable. kind:14
    //    (sealed NIP-17 chat message) and kind:1059 (gift-wrap) are the
    //    private envelope kinds the workspace writes; they never route to
    //    public relays via Auto.
    if kind == KIND_GIFT_WRAP || kind == KIND_CHAT_MESSAGE {
        return PublishBehavior::PrivateFailClosed;
    }

    let Ok(registry) = registry().read() else {
        return PublishBehavior::PublicRoutable;
    };

    // 2. Reserved replaceables — a raw app publish is refused so the typed
    //    builder's protocol processing cannot be bypassed. Protocol crates own
    //    these registrations because the reason is protocol-specific.
    if let Some(policy) = registry.reserved_builder(kind) {
        return PublishBehavior::ReservedBuilderOnly(policy);
    }

    // 3. Discovery-indexable replaceables — protocol crates declare which of
    //    their public kinds the resolver may also fan out to indexers.
    if registry.is_discovery_indexable(kind) {
        return PublishBehavior::DiscoveryIndexable;
    }

    // 4. Everything else — notes, reactions, custom app kinds, parameterized
    //    addressables (30000–39999) — is publicly routable through the standard
    //    outbox. The replaceable predicates are referenced so the table stays
    //    honest about the full kind space even though the default arm covers
    //    them: a future contributor adding a behaviour split for a replaceable
    //    range edits THIS function, never a literal at a call site.
    let _is_replaceable = is_replaceable(kind);
    let _is_addressable = is_addressable(kind);
    PublishBehavior::PublicRoutable
}

impl PublishBehavior {
    /// `true` when a raw app publish (`PublishRaw`) of this kind must be
    /// rejected in favour of a dedicated typed builder. Returns the typed
    /// reason so the guard renders its message from the table.
    pub(crate) fn reserved_builder(self) -> Option<ReservedBuilderPolicy> {
        match self {
            Self::ReservedBuilderOnly(reserved) => Some(reserved),
            _ => None,
        }
    }

    /// `true` when this kind is a private / encrypted envelope that must never
    /// be routed to public relays — it requires an `Explicit` non-empty
    /// recipient-inbox relay set, never `Auto` (D10).
    pub(crate) fn is_private_fail_closed(self) -> bool {
        matches!(self, Self::PrivateFailClosed)
    }
}

/// The D6 toast / rejection message surfaced when a private (gift-wrap /
/// sealed) kind is published without an explicit relay pin. Single source of
/// truth for the wording so the kernel-level guard, the action-layer guard,
/// and the engine chokepoint all speak with one voice.
pub(crate) fn private_fail_closed_rejection(kind: u32) -> String {
    format!(
        "cannot publish kind:{kind} private/encrypted envelope with automatic routing: \
         it requires an explicit non-empty recipient-inbox relay set classified as \
         verified_private_inbox (PublishTarget::Explicit). Auto, empty routing, or an \
         unverified explicit route would leak the encrypted envelope to public relays (D10)."
    )
}

/// Validate a `(kind, target)` pair against the typed publish-routing policy.
/// **The single door** every publish path consults before an event reaches the
/// publish engine — signed AND unsigned, dispatched AND internal.
///
/// The only structural rule the policy table enforces on routing today is the
/// D10 private-envelope invariant: a [`PublishBehavior::PrivateFailClosed`]
/// kind may ONLY use `PublishTarget::Explicit` with a non-empty relay set and
/// `VerifiedPrivateInbox` provenance; `Auto`, empty `Explicit`, and other
/// explicit route classes are rejected so a private event can never be routed
/// to public/manual relays under an anonymous pin. All other kinds pass routing
/// validation here (relay selection itself is the `OutboxResolver`'s concern,
/// Layer 2).
pub(crate) fn validate_publish_routing(
    kind: u32,
    target: &super::PublishTarget,
) -> Result<(), String> {
    if classify_publish_behavior(kind).is_private_fail_closed()
        && !target_is_verified_private_inbox(target)
    {
        return Err(private_fail_closed_rejection(kind));
    }
    Ok(())
}

/// Structural predicate for private-envelope routing: the target must be an
/// explicit, non-empty relay pin whose provenance says the relays are verified
/// private inboxes. Manual overrides, group host pins, diagnostic sends, and
/// imported/presigned routes are explicit, but they are not private-inbox proof.
pub(crate) fn target_is_verified_private_inbox(target: &super::PublishTarget) -> bool {
    matches!(
        target,
        super::PublishTarget::Explicit {
            relays,
            route_class: super::PublishRouteClass::VerifiedPrivateInbox,
        } if !relays.is_empty()
    )
}

/// **The universal per-relay emit gate** (D10 fail-closed). Returns `true` when
/// it is safe to emit an `["EVENT", …]` frame for a `kind` event to a single
/// relay that was selected for the reasons in `relay_reasons`.
///
/// Every publish emit path — initial publish, resume-from-store on restart, and
/// manual/availability retry — converges on the engine's `dispatch_due` loop,
/// so calling this immediately before `dispatcher.dispatch(relay, frame)` makes
/// the private-envelope fail-closed invariant **truly universal**: it cannot be
/// bypassed by a persisted row replayed on resume or a re-dispatched retry.
///
/// Rule: a [`PublishBehavior::PrivateFailClosed`] kind (gift-wrap kind:1059,
/// sealed kind:14) may ONLY be emitted to a relay explicitly classified as a
/// verified private inbox. A private event targeted at a write-relay /
/// discovery-indexer / recipient-inbox-derived / manual override /
/// reason-less relay is REFUSED — that is exactly the public-relay leak D10
/// forbids. Every other (public/reserved/discovery) kind may emit to any
/// selected relay; their relay selection is the resolver's concern (Layer 2).
pub(crate) fn relay_emit_is_sanctioned(
    kind: u32,
    relay_reasons: &[super::RelaySelectionReason],
) -> bool {
    if !classify_publish_behavior(kind).is_private_fail_closed() {
        return true;
    }
    // Private kind: emit only to a verified private inbox pin.
    relay_reasons.iter().any(|r| {
        matches!(
            r,
            super::RelaySelectionReason::Explicit {
                route_class: super::PublishRouteClass::VerifiedPrivateInbox
            }
        )
    })
}

#[cfg(test)]
#[path = "policy/tests.rs"]
mod tests;
