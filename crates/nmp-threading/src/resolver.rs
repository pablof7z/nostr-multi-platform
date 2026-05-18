//! `ParentResolver` — per-NIP plug for the kind-agnostic [`crate::Grouper`].
//!
//! `nmp-nip01` impls it over NIP-10 markers (`Nip10Refs`); `nmp-nip22` impls
//! it over NIP-22 lowercase root / parent tags (`CommentRecord`). The grouper
//! never sees kind numbers or tag conventions — it only asks "what is this
//! event's parent / root / parent-author?".

use nmp_core::substrate::KernelEvent;

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
}
