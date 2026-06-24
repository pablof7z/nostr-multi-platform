//! ADR-0063 — the kernel-owned `RefResolver` reference-resolution primitive.
//!
//! Profile and event refs are two instances of one shape:
//! refcounted consumer ownership → kernel-owned fetch/routing/cache policy
//! (`register_interest` / `OneshotApi`) → a push-updating keyed projection →
//! release. This module NAMES that primitive. It does not invent a new machine;
//! it generalizes the two existing claim bodies (`requests/profile.rs`,
//! `requests/event.rs`) behind one origin-blind seam:
//!
//! ```text
//! resolve_ref(namespace, key, consumer_id, shape, liveness)
//! release_ref(namespace, key, consumer_id)
//! ```
//!
//! ## Why enum-dispatch, not a `dyn RefResolver` object (subjective decision)
//!
//! Both resolver bodies mutate the kernel's own state — the refcount map, the
//! interest registry, the per-key rev, the live-claim set. A `dyn` trait object
//! holding state *apart from* the kernel could not take `&mut Kernel` without a
//! self-borrow conflict (the resolver would be a field of the kernel it mutates).
//! So the namespace markers ([`ProfileNs`] / [`EventNs`]) are **zero-sized**: the
//! [`RefResolver`] trait captures the shared *contract* (the closed shape type,
//! the namespace discriminant, the resolve/release entry points) while the kernel
//! owns the state and the seam (`resolve_ref`) dispatches with a `match`.
//!
//! ## Closed, typed surface (ADR-0063 D2/D3/D4 — NOT a stringly registry)
//!
//! * [`RefNamespace`] — `Profile` + `Event` ONLY. No `zap_total` / reply-counts;
//!   those are aggregate queries over event *sets*, a different invalidation
//!   model (ADR-0063 D2 scope limit).
//! * Shape — a small **closed, namespace-owned** enum ([`ProfileShape`],
//!   [`EventShape`]); NOT a per-field mask. `Card`/`Raw` are the widest shapes.
//! * [`RefLiveness`] — `CacheOk | Live`, kept **strictly separate from shape**
//!   (shape = what bytes; liveness = how fresh). `Live` wins on dedup.
//!
//! ## `allow(dead_code)` scope
//!
//! Parts of the primitive's surface are the integration-branch deliverable that
//! later lanes consume, so they read as dead in a pure `nmp-core` build even
//! though they are live across the migration and exercised by this module's
//! unit tests: `RefLiveness::from_ffi` + `RefShape::Event` (Lane D, the C-ABI),
//! `RefResolver::NAMESPACE` (the trait contract / Lane C dispatch), and the
//! `ref_row_rev` / `ref_demanded_*_shape` read API (Lane A's per-key wire
//! row-delta). The allow is scoped to this module so a genuinely-unused symbol
//! elsewhere is still caught (mirrors `kernel/projection_rev/mod.rs`).
#![allow(dead_code)]

use super::{Instant, Kernel, OutboundMessage};
use crate::kernel::ProfileLiveness;

/// Closed, typed set of reference resolvers (ADR-0063 D2). Adding a namespace is
/// a deliberate, reviewed change — never a runtime string.
///
/// Promoted to `pub` in Lane D so `nmp-ffi` can carry it in
/// `ActorCommand::ResolveRef` / `ActorCommand::ReleaseRef` without an opaque
/// int round-trip through the actor dispatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefNamespace {
    Profile,
    Event,
}

/// Closed shape enum for the `profile` namespace (ADR-0063 D3). `Card` is the
/// widest (the full `ProfileCard`); `Ref` is the feed-avatar subset.
///
/// Promoted to `pub` in Lane D (see [`RefNamespace`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileShape {
    /// `{pubkey, display_name, picture_url}` — the feed-avatar shape.
    Ref,
    /// The full ~16-field `ProfileCard` — the profile-screen shape.
    Card,
}

impl ProfileShape {
    /// Monotonic widen toward `Card` — the projection row carries the widest
    /// shape any currently-live consumer of the key demanded (ADR-0063 D5).
    pub fn widen(self, other: Self) -> Self {
        if matches!(self, Self::Card) || matches!(other, Self::Card) {
            Self::Card
        } else {
            Self::Ref
        }
    }
}

/// Closed shape enum for the `event` namespace (ADR-0063 D3). `Raw` is the
/// widest (the full raw event); `Embed` is the render-an-embed-card subset.
///
/// Promoted to `pub` in Lane D (see [`RefNamespace`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventShape {
    /// The render-an-embed-card shape.
    Embed,
    /// The full raw event.
    Raw,
}

impl EventShape {
    /// Monotonic widen toward `Raw` (ADR-0063 D5).
    pub fn widen(self, other: Self) -> Self {
        if matches!(self, Self::Raw) || matches!(other, Self::Raw) {
            Self::Raw
        } else {
            Self::Embed
        }
    }
}

/// The seam-level shape discriminant. Carries its namespace so a
/// `(namespace, shape)` mismatch at the front door fails closed (D6).
///
/// Promoted to `pub` in Lane D (see [`RefNamespace`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefShape {
    Profile(ProfileShape),
    Event(EventShape),
}

impl RefShape {
    /// The namespace this shape belongs to.
    pub fn namespace(self) -> RefNamespace {
        match self {
            Self::Profile(_) => RefNamespace::Profile,
            Self::Event(_) => RefNamespace::Event,
        }
    }
}

/// Client-hintable freshness, **strictly separate from shape** (ADR-0063 D4):
/// shape = *what bytes are needed*, liveness = *how fresh the resolver keeps the
/// entity*. `CacheOk` serves from the store + one-shot on a miss; `Live` keeps a
/// tailing sub so replacements arrive reactively. Same key dedups to one slot;
/// `Live` wins.
///
/// Promoted to `pub` in Lane D (see [`RefNamespace`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefLiveness {
    CacheOk,
    Live,
}

impl RefLiveness {
    /// Decode the FFI `liveness` int (`0 = CacheOk`, anything else = `Live`).
    #[must_use]
    pub fn from_ffi(liveness: i32) -> Self {
        if liveness == 0 {
            Self::CacheOk
        } else {
            Self::Live
        }
    }
}

impl From<ProfileLiveness> for RefLiveness {
    fn from(p: ProfileLiveness) -> Self {
        match p {
            ProfileLiveness::CacheOk => Self::CacheOk,
            ProfileLiveness::Live => Self::Live,
        }
    }
}

impl From<RefLiveness> for ProfileLiveness {
    fn from(l: RefLiveness) -> Self {
        match l {
            RefLiveness::CacheOk => Self::CacheOk,
            RefLiveness::Live => Self::Live,
        }
    }
}

/// Optional caller-supplied metadata for a raw-key ref resolve.
///
/// This is NOT a URI front door. App-owned URI adapters decode `nostr:` /
/// NIP-19 values before crossing the boundary, then pass the raw key plus this
/// metadata so relay TLVs and nevent author TLVs keep the behavior the deleted
/// URI adapter had.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RefResolveMetadata {
    /// Relay hints decoded by the caller from NIP-19/NIP-21 TLVs.
    pub hints: Vec<String>,
    /// Optional event author decoded from a nevent author TLV. Ignored for
    /// profile refs and superseded by coordinate-derived authors for naddr keys.
    pub event_author: Option<String>,
}

impl RefResolveMetadata {
    /// Metadata carrying only relay hints.
    #[must_use]
    pub fn from_hints(hints: Vec<String>) -> Self {
        Self {
            hints,
            event_author: None,
        }
    }
}

/// The shared contract both reference resolvers implement.
///
/// Enum-dispatched through [`Kernel::resolve_ref`] (see module docs for why this
/// is not a `dyn` object). The markers are zero-sized; all state lives on the
/// `Kernel`.
pub(crate) trait RefResolver {
    /// The closed, namespace-owned shape enum.
    type Shape: Copy + core::fmt::Debug;
    /// The namespace discriminant.
    const NAMESPACE: RefNamespace;

    /// Refcount a consumer's interest and register/upgrade the kernel-owned
    /// fetch interest (generalized `claim_*` body).
    fn resolve(
        kernel: &mut Kernel,
        key: String,
        consumer_id: String,
        shape: Self::Shape,
        liveness: RefLiveness,
        force: bool,
        hints: Vec<String>,
        now: Instant,
    ) -> Vec<OutboundMessage>;

    /// Drop a consumer's interest; tear the slot down on the last owner
    /// (generalized `release_*` body).
    fn release(kernel: &mut Kernel, key: &str, consumer_id: &str) -> Vec<OutboundMessage>;
}

/// Zero-sized marker for the `profile` resolver.
pub(crate) struct ProfileNs;
/// Zero-sized marker for the `event` resolver.
pub(crate) struct EventNs;

impl RefResolver for ProfileNs {
    type Shape = ProfileShape;
    const NAMESPACE: RefNamespace = RefNamespace::Profile;

    fn resolve(
        kernel: &mut Kernel,
        key: String,
        consumer_id: String,
        shape: ProfileShape,
        liveness: RefLiveness,
        force: bool,
        hints: Vec<String>,
        _now: Instant,
    ) -> Vec<OutboundMessage> {
        kernel.resolve_profile_ref(key, consumer_id, shape, liveness, force, hints)
    }

    fn release(kernel: &mut Kernel, key: &str, consumer_id: &str) -> Vec<OutboundMessage> {
        kernel.release_profile_ref(key, consumer_id)
    }
}

impl RefResolver for EventNs {
    type Shape = EventShape;
    const NAMESPACE: RefNamespace = RefNamespace::Event;

    fn resolve(
        kernel: &mut Kernel,
        key: String,
        consumer_id: String,
        shape: EventShape,
        liveness: RefLiveness,
        force: bool,
        hints: Vec<String>,
        now: Instant,
    ) -> Vec<OutboundMessage> {
        // Origin-blind seam: readiness is a kernel-owned transport fact
        // (`any_relay_connected`), not a caller-supplied flag (the ADR seam has
        // no `can_send`).
        let can_send = kernel.any_relay_connected();
        kernel.resolve_event_ref_at(
            key,
            consumer_id,
            shape,
            liveness,
            force,
            can_send,
            hints,
            now,
        )
    }

    fn release(kernel: &mut Kernel, key: &str, consumer_id: &str) -> Vec<OutboundMessage> {
        kernel.release_event_ref(key, consumer_id)
    }
}

impl Kernel {
    /// The unified, origin-blind reference-resolution seam (ADR-0063 D1).
    ///
    /// Generalizes the former per-kind profile/event claims. A `(namespace, shape)`
    /// mismatch fails closed (D6: log + no-op, never an FFI error). `hints` are
    /// NIP-19 relay TLVs seeding the registered interest (empty for the bare
    /// key path).
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn resolve_ref(
        &mut self,
        namespace: RefNamespace,
        key: String,
        consumer_id: String,
        shape: RefShape,
        liveness: RefLiveness,
        force: bool,
        hints: Vec<String>,
    ) -> Vec<OutboundMessage> {
        self.resolve_ref_at(
            namespace,
            key,
            consumer_id,
            shape,
            liveness,
            force,
            hints,
            crate::kernel::test_support::test_support_now(),
        )
    }

    pub(crate) fn resolve_ref_at(
        &mut self,
        namespace: RefNamespace,
        key: String,
        consumer_id: String,
        shape: RefShape,
        liveness: RefLiveness,
        force: bool,
        hints: Vec<String>,
        now: Instant,
    ) -> Vec<OutboundMessage> {
        self.resolve_ref_with_metadata_at(
            namespace,
            key,
            consumer_id,
            shape,
            liveness,
            force,
            RefResolveMetadata::from_hints(hints),
            now,
        )
    }

    /// Same raw-key resolver with caller-supplied metadata from an app-owned URI
    /// adapter. The metadata does not create a second resolution door: the key is
    /// still raw, shape/namespace are still checked here, and dispatch still
    /// lands in the namespace-owned resolver body.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn resolve_ref_with_metadata(
        &mut self,
        namespace: RefNamespace,
        key: String,
        consumer_id: String,
        shape: RefShape,
        liveness: RefLiveness,
        force: bool,
        metadata: RefResolveMetadata,
    ) -> Vec<OutboundMessage> {
        self.resolve_ref_with_metadata_at(
            namespace,
            key,
            consumer_id,
            shape,
            liveness,
            force,
            metadata,
            crate::kernel::test_support::test_support_now(),
        )
    }

    pub(crate) fn resolve_ref_with_metadata_at(
        &mut self,
        namespace: RefNamespace,
        key: String,
        consumer_id: String,
        shape: RefShape,
        liveness: RefLiveness,
        force: bool,
        metadata: RefResolveMetadata,
        now: Instant,
    ) -> Vec<OutboundMessage> {
        if shape.namespace() != namespace {
            self.log(format!(
                "resolve_ref: shape namespace {:?} != requested {namespace:?} — ignoring",
                shape.namespace()
            ));
            return Vec::new();
        }
        match shape {
            RefShape::Profile(s) => ProfileNs::resolve(
                self,
                key,
                consumer_id,
                s,
                liveness,
                force,
                metadata.hints,
                now,
            ),
            RefShape::Event(s) => {
                let can_send = self.any_relay_connected();
                self.resolve_event_ref_with_metadata_at(
                    key,
                    consumer_id,
                    s,
                    liveness,
                    force,
                    can_send,
                    metadata.event_author,
                    metadata.hints,
                    now,
                )
            }
        }
    }

    /// Drop `consumer_id`'s reference to `(namespace, key)`. The slot tears down
    /// on the last owner.
    pub(crate) fn release_ref(
        &mut self,
        namespace: RefNamespace,
        key: &str,
        consumer_id: &str,
    ) -> Vec<OutboundMessage> {
        match namespace {
            RefNamespace::Profile => ProfileNs::release(self, key, consumer_id),
            RefNamespace::Event => EventNs::release(self, key, consumer_id),
        }
    }

    /// Lane A read API (ADR-0063 D6a): the per-KEY revision for a resolved-ref
    /// row. Advances at resolve, release, and the ingest chokepoint that
    /// rewrites the row's data. The wire/manifest row-delta is Lane A's; this is
    /// the kernel-owned source of truth it consults.
    pub(crate) fn ref_row_rev(&self, namespace: RefNamespace, key: &str) -> u64 {
        self.projection_rev_tracker
            .source_versions
            .ref_row_rev(namespace, key)
    }

    /// The widest `profile` shape any currently-live consumer of `key` demanded
    /// (ADR-0063 D5). Folds the widen lattice over the per-consumer shapes so a
    /// release that drops the widest consumer narrows the result (HIGH 4). `None`
    /// once no consumer holds the key.
    pub(crate) fn ref_demanded_profile_shape(&self, key: &str) -> Option<ProfileShape> {
        let consumers = self.ref_profile_shapes.get(key)?;
        consumers.values().copied().reduce(|acc, s| acc.widen(s))
    }

    /// The widest `event` shape any currently-live consumer of `key` demanded.
    /// Folds the widen lattice over the per-consumer shapes (HIGH 4).
    pub(crate) fn ref_demanded_event_shape(&self, key: &str) -> Option<EventShape> {
        let consumers = self.ref_event_shapes.get(key)?;
        consumers.values().copied().reduce(|acc, s| acc.widen(s))
    }
}
