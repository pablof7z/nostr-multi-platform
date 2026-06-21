//! ADR-0063 (#1671) — unified reference-resolution surface for
//! [`super::KernelReducer`] (the wasm dispatch path).
//!
//! Split from `kernel_reducer.rs` to keep that file under the 500-LOC hard
//! ceiling (AGENTS.md). These are thin delegators onto the SAME
//! `Kernel::resolve_ref` / `Kernel::release_ref` the actor
//! `ActorCommand::ResolveRef` / `ActorCommand::ReleaseRef` arms call — there is
//! no divergent web-only resolution path.

use super::KernelReducer;
use crate::kernel::{RefLiveness, RefNamespace, RefShape};
use crate::relay::OutboundMessage;

impl KernelReducer {
    /// ADR-0063 D1 — the unified, origin-blind reference-resolution seam.
    /// Refcounts the consumer per `(namespace, key)` (a raw hex pubkey for
    /// `profile`, a hex event-id / `kind:pubkey:d` coordinate for `event`),
    /// registers the kernel-owned fetch on the cold-resolve transition, and feeds
    /// the keyed `refs.*` row-delta projection. `force = false` + no NIP-19 hints:
    /// web component resolves on mount are background, bare-key. A `(namespace,
    /// shape)` mismatch fails closed inside `Kernel::resolve_ref` (D6). Returns
    /// any immediately-sendable outbound.
    pub fn resolve_ref(
        &mut self,
        namespace: RefNamespace,
        key: String,
        consumer_id: String,
        shape: RefShape,
        liveness: RefLiveness,
    ) -> Vec<OutboundMessage> {
        let outbound =
            self.kernel
                .resolve_ref(namespace, key, consumer_id, shape, liveness, false, Vec::new());
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
