//! `RefsCommand` — reference resolution (ADR-0063 unified + legacy).
//!
//! Grouped under `ActorCommand::Refs(RefsCommand)`. Dispatch home:
//! `actor/dispatch/mod.rs` (thin delegator — the kernel's `resolve_ref` /
//! `claim_event` / `release_ref` / `release_event` are one-liners).

/// Refcounted reference-resolution verbs.
///
/// The kernel's `Kernel::resolve_ref` dispatches the typed `(namespace, shape)`
/// pair to the matching resolver body, failing closed (D6) on a mismatch. The
/// legacy `ClaimEvent` / `ReleaseEvent` pair predates ADR-0063's unified seam
/// and is preserved for callers that have not migrated.
#[derive(Debug)]
pub enum RefsCommand {
    /// Refcounted event claim — drives the generic `claim_event` kernel
    /// primitive (F-CR-06 / ADR-0034). `uri` is a `nostr:` URI
    /// (nevent/note/naddr); profile URIs should use [`Self::Resolve`] instead.
    /// `force` (F-TTL) bypasses the TTL freshness gate for addressable (naddr)
    /// coordinates; it is a silent no-op for immutable nevent/note URIs.
    ClaimEvent {
        uri: String,
        consumer_id: String,
        force: bool,
    },
    /// Release a previously claimed event (the same `uri` + `consumer_id`
    /// pair). On the last consumer's release the `event_claims[primary_id]`
    /// row is removed and `event_claim_requested` is cleared so a re-claim can
    /// re-fetch.
    ReleaseEvent {
        uri: String,
        consumer_id: String,
    },
    /// ADR-0063 Lane D/H — unified, origin-blind reference-resolution seam.
    ///
    /// Generalizes the legacy `ClaimEvent` + `ClaimProfile` into one variant.
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