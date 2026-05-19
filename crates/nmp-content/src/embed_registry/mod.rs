//! `EmbedClaimRegistry` — refcounted per-id claim/release for embedded events.
//!
//! When N timeline rows render the same `nevent1…` (or `naddr1…`), the
//! registry **deduplicates the in-memory claim state**: repeated `claim`s
//! on the same target share one [`Entry`] and one [`ResolvedEvent`], and
//! the registry can answer "is anything still observing this target?"
//! ([`is_claimed`] / [`refcount`] / [`claim_count`]). `claim` returns an
//! opaque [`ClaimHandle`]; the last matching `release` decays the entry.
//!
//! Implemented as a [`ViewModule`] per PD-013 — D0-clean (no kernel
//! coupling beyond `ViewContext` + `KernelEvent`), debug-inspectable via
//! standard snapshot machinery, namespace `nmp.content.embed_registry`.
//!
//! # Scope today vs. Phase 2 (no overclaim)
//! This module is the **claim-dedupe primitive only**. It does *not* open
//! or close upstream subscriptions, and it does not by itself guarantee
//! that a claimed embed becomes resolved — resolution happens only when
//! the kernel independently ingests the event and calls
//! [`on_event_inserted`]. The `ViewModule` dependency contract is
//! spec-driven and static (`dependencies(&Spec)`); the spec here is
//! unit-shaped, so the registry declares no dependencies and the kernel
//! does not subscribe on its behalf.
//!
//! Wiring claims to actually drive subscription open/close (so a claim
//! *guarantees* resolution + grace-period teardown) is a Phase-2 seam
//! requiring a claim-driven dependency channel the `ViewModule` trait
//! does not yet expose (`content-rendering.md` §9 Phase 2). Apps get the
//! dedupe + refcount primitive now; the kernel fetch cycle lands later.
//!
//! [`Entry`]: state::Entry
//! [`is_claimed`]: EmbedClaimRegistry::is_claimed
//! [`refcount`]: EmbedClaimRegistry::refcount
//! [`claim_count`]: EmbedClaimRegistry::claim_count
//! [`on_event_inserted`]: nmp_core::substrate::ViewModule::on_event_inserted

mod state;
mod target;
mod view;

#[cfg(test)]
mod tests;

pub use state::EmbedClaimState;
pub use target::{ClaimHandle, EmbedTarget, ResolvedEvent};
pub use view::{EmbedClaimDelta, EmbedClaimSpec, EmbedRegistrySnapshot};

/// The registry type — methods are inherent (not on the trait) because the
/// `ViewModule` shape is callback-driven. The free `claim` / `release`
/// methods are the API apps actually call from FFI bindings.
pub struct EmbedClaimRegistry;

impl EmbedClaimRegistry {
    /// Module namespace (matches the brief — `nmp.content.embed_registry`).
    /// Kept in lockstep with `<Self as ViewModule>::NAMESPACE` in `view.rs`.
    pub const NAMESPACE: &'static str = "nmp.content.embed_registry";

    /// Initialise a fresh state — apps that don't run the full
    /// `ViewModule::open` machinery can hold an [`EmbedClaimState`]
    /// directly and call the inherent methods.
    pub fn state() -> EmbedClaimState {
        EmbedClaimState::new()
    }

    /// Claim a target. Registers a fresh handle; returns it plus the
    /// current [`ResolvedEvent`] when present (cold-start → `None`; warm
    /// or post-fetch → `Some`). Release it with [`release`] when the
    /// embed is no longer visible.
    ///
    /// [`release`]: EmbedClaimRegistry::release
    pub fn claim(
        state: &mut EmbedClaimState,
        target: EmbedTarget,
    ) -> (ClaimHandle, Option<ResolvedEvent>) {
        state::claim(state, target)
    }

    /// Release a previously-claimed handle. Returns `true` iff this was
    /// the *last* live claim for that target (so the caller can act on
    /// the "all observers gone" signal — e.g. start a grace-period timer
    /// before closing the upstream subscription).
    ///
    /// A double-release of the same handle, or a phantom handle (unknown
    /// target *or* a stale/already-released handle id), is a no-op
    /// returning `false`; it never decrements another live claim's
    /// refcount.
    pub fn release(state: &mut EmbedClaimState, handle: &ClaimHandle) -> bool {
        state::release(state, handle)
    }

    /// True if any handle is currently outstanding for `target`.
    pub fn is_claimed(state: &EmbedClaimState, target: &EmbedTarget) -> bool {
        state::is_claimed(state, target)
    }

    /// Current refcount for `target` (0 if absent).
    pub fn refcount(state: &EmbedClaimState, target: &EmbedTarget) -> usize {
        state::refcount(state, target)
    }

    /// Number of distinct targets currently being claimed. This is the
    /// "how many upstream subscriptions would be open" count — apps assert
    /// `claim_count == 1` when many components claim the same id.
    pub fn claim_count(state: &EmbedClaimState) -> usize {
        state::claim_count(state)
    }

    /// Look up a resolved payload, if any.
    pub fn resolved(state: &EmbedClaimState, target: &EmbedTarget) -> Option<ResolvedEvent> {
        state::resolved(state, target)
    }
}
