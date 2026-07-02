//! ADR-0070 (#1671) — unified reference-resolution surface for
//! [`super::KernelReducer`] (the wasm dispatch path).
//!
//! Split from `kernel_reducer.rs` to keep that file under the 500-LOC hard
//! ceiling (AGENTS.md). These are thin delegators onto the SAME
//! `Kernel::resolve_ref` / `Kernel::release_ref` the actor
//! `ActorCommand::ResolveRef` / `ActorCommand::ReleaseRef` arms call — there is
//! no divergent web-only resolution path.

use super::KernelReducer;
use crate::kernel::{RefLiveness, RefNamespace, RefResolveMetadata, RefShape};
use crate::relay::OutboundMessage;
use crate::time::Instant;

impl KernelReducer {
    /// ADR-0070 D1 — the unified, origin-blind reference-resolution seam.
    /// Refcounts the consumer per `(namespace, key)` (a raw hex pubkey for
    /// `profile`, a hex event-id / `kind:pubkey:d` coordinate for `event`),
    /// registers the kernel-owned fetch on the cold-resolve transition, and feeds
    /// the keyed `refs.*` row-delta projection. `force = false` + no NIP-19 hints:
    /// web component resolves on mount are background, bare-key. A `(namespace,
    /// shape)` mismatch fails closed inside `Kernel::resolve_ref` (D6). Returns
    /// any immediately-sendable outbound.
    #[cfg(any(test, feature = "test-support"))]
    pub fn resolve_ref(
        &mut self,
        namespace: RefNamespace,
        key: String,
        consumer_id: String,
        shape: RefShape,
        liveness: RefLiveness,
    ) -> Vec<OutboundMessage> {
        self.resolve_ref_with_hints(namespace, key, consumer_id, shape, liveness, Vec::new())
    }

    /// Same unified resolver with caller-supplied relay hints. Used by the
    /// wasm dispatch surface after the app decodes NIP-19 relay TLVs at its
    /// own boundary.
    #[cfg(any(test, feature = "test-support"))]
    pub fn resolve_ref_with_hints(
        &mut self,
        namespace: RefNamespace,
        key: String,
        consumer_id: String,
        shape: RefShape,
        liveness: RefLiveness,
        hints: Vec<String>,
    ) -> Vec<OutboundMessage> {
        self.resolve_ref_with_hints_at(
            namespace,
            key,
            consumer_id,
            shape,
            liveness,
            hints,
            crate::kernel::test_support::test_support_now(),
        )
    }

    pub fn resolve_ref_with_hints_at(
        &mut self,
        namespace: RefNamespace,
        key: String,
        consumer_id: String,
        shape: RefShape,
        liveness: RefLiveness,
        hints: Vec<String>,
        now: Instant,
    ) -> Vec<OutboundMessage> {
        let outbound = self.kernel.resolve_ref_at(
            namespace,
            key,
            consumer_id,
            shape,
            liveness,
            false,
            hints,
            now,
        );
        self.kernel.partition_auth_paused(outbound)
    }

    /// Same unified resolver with full caller-supplied metadata. Used by wasm and
    /// native app-owned URI adapters after decoding NIP-19/NIP-21 relay and
    /// author TLVs at their own boundary.
    #[cfg(any(test, feature = "test-support"))]
    pub fn resolve_ref_with_metadata(
        &mut self,
        namespace: RefNamespace,
        key: String,
        consumer_id: String,
        shape: RefShape,
        liveness: RefLiveness,
        metadata: RefResolveMetadata,
    ) -> Vec<OutboundMessage> {
        self.resolve_ref_with_metadata_at(
            namespace,
            key,
            consumer_id,
            shape,
            liveness,
            metadata,
            crate::kernel::test_support::test_support_now(),
        )
    }

    pub fn resolve_ref_with_metadata_at(
        &mut self,
        namespace: RefNamespace,
        key: String,
        consumer_id: String,
        shape: RefShape,
        liveness: RefLiveness,
        metadata: RefResolveMetadata,
        now: Instant,
    ) -> Vec<OutboundMessage> {
        let outbound = self.kernel.resolve_ref_with_metadata_at(
            namespace,
            key,
            consumer_id,
            shape,
            liveness,
            false,
            metadata,
            now,
        );
        self.kernel.partition_auth_paused(outbound)
    }

    /// Drop `consumer_id`'s reference to `(namespace, key)` through the unified
    /// seam. Tears the slot down on the last owner. Returns an empty vec.
    pub fn release_ref(
        &mut self,
        namespace: RefNamespace,
        key: &str,
        consumer_id: &str,
    ) -> Vec<OutboundMessage> {
        let outbound = self.kernel.release_ref(namespace, key, consumer_id);
        self.kernel.partition_auth_paused(outbound)
    }
}
