//! `ParentResolver` — per-NIP plug for the kind-agnostic [`crate::Grouper`].
//!
//! `nmp-nip01` impls it over NIP-10 markers (`Nip10Refs`). The grouper
//! never sees kind numbers or tag conventions — it only asks "what is this
//! event's parent / root / parent-author / supersession-target?".

use nmp_core::substrate::{EventId, KernelEvent};

use crate::pointer::ThreadPointer;

/// Resolve thread relationships from a `KernelEvent`. Implementors are
/// per-NIP and stateless — the grouper owns its own state.
pub trait ParentResolver: Send + Sync + 'static {
    /// Direct parent — the thing this event replies to. `None` for top-level
    /// events that aren't part of a thread.
    fn parent(&self, event: &KernelEvent) -> Option<ThreadPointer>;

    /// Thread root — the original anchor (article, note, URI). For top-level
    /// replies this may equal `parent`. `None` when the event is itself a
    /// root or when no root marker is decodable.
    fn root(&self, event: &KernelEvent) -> Option<ThreadPointer>;

    /// Pubkey of the parent's author, when recoverable from the event's `p`
    /// tags. Optional — used by UI for "X replied to Y" stitching; the
    /// grouper itself does not consult this.
    fn parent_author(&self, event: &KernelEvent) -> Option<String>;

    /// Event id this event supersedes in the block layout, if any.
    ///
    /// Used for feed-composition rules where one event should *replace* (not
    /// extend) another in the displayed block list — the canonical case is
    /// a NIP-18 repost whose target note is already in the feed. The
    /// grouper removes the named block before placing this event, so the
    /// reposted note bumps to the new event's position and renders once.
    ///
    /// Default `None`: parent edges, not supersession, is the common case.
    fn supersedes(&self, _event: &KernelEvent) -> Option<EventId> {
        None
    }
}
