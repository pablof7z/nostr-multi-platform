//! `RefsCommand` — reference resolution (ADR-0063 unified).
//!
//! Grouped under `ActorCommand::Refs(RefsCommand)`. Dispatch home:
//! `actor/dispatch/mod.rs` (thin delegator to the kernel's `resolve_ref` /
//! `release_ref` seam).

/// Refcounted reference-resolution verbs.
///
/// The kernel's `Kernel::resolve_ref` dispatches the typed `(namespace, shape)`
/// pair to the matching resolver body, failing closed (D6) on a mismatch.
#[derive(Debug)]
pub enum RefsCommand {
    /// ADR-0063 Lane D/H — unified, origin-blind reference-resolution seam.
    ///
    /// The kernel's `Kernel::resolve_ref` dispatches the typed `(namespace,
    /// shape)` pair to the matching resolver body, failing closed (D6) on a
    /// mismatch. `force` threads into the resolver's F-TTL gate. `hints` are
    /// optional NIP-19 relay TLVs seeding the registered interest.
    Resolve {
        namespace: crate::kernel::RefNamespace,
        key: String,
        consumer_id: String,
        shape: crate::kernel::RefShape,
        liveness: crate::kernel::RefLiveness,
        force: bool,
        hints: Vec<String>,
    },
    /// Same raw-key resolve with metadata decoded by an app-owned URI adapter.
    /// This is deliberately not a URI front door: callers must pass the
    /// canonical raw key and only use metadata for relay/author TLVs.
    ResolveWithMetadata {
        namespace: crate::kernel::RefNamespace,
        key: String,
        consumer_id: String,
        shape: crate::kernel::RefShape,
        liveness: crate::kernel::RefLiveness,
        force: bool,
        metadata: crate::kernel::RefResolveMetadata,
    },
    /// ADR-0063 Lane D — release a reference previously registered via
    /// [`Self::Resolve`]. Decrements the refcount; the slot is torn down when
    /// the last consumer releases. `(namespace, key, consumer_id)` must match
    /// the original `Resolve` call. A release of an unknown key is a silent
    /// no-op (D6).
    Release {
        namespace: crate::kernel::RefNamespace,
        key: String,
        consumer_id: String,
    },
}
