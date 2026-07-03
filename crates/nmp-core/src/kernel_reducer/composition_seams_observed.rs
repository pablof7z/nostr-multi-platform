//! Observed-projection composition seam for `KernelReducer`.
//!
//! Split out of `composition_seams.rs` (file-size ceiling, AGENTS.md) as a
//! cohesive cluster: open/close a declared scoped read-model sink on the
//! reducer/browser path, plus the cloneable command-backed registrar for
//! post-start runtime controllers. All three share the `observer_slot` +
//! `observed_projection_sessions` state and the same interest open/close body.
//!
//! Doctrine mirrors `composition_seams.rs`: D0 substrate-level surface types
//! only, D6 poisoned-mutex is a silent no-op, D8 O(n-observers) with no I/O.

use std::sync::Arc;

use crate::actor::{register_rust_observer_muted, unregister_observer_internal};
use crate::substrate::{ObservedProjectionCommandHandle, ObservedProjectionSessionMap};
use crate::ObservedProjectionId;

impl super::KernelReducer {
    // ── Observed-projection seam ─────────────────────────────────────────

    /// Open a declared observed projection on the reducer/browser path.
    ///
    /// Mirrors `NmpApp::open_observed_projection`: register the sink muted,
    /// open the declared interest, replay matching cached rows, then activate
    /// future delivery scoped to the declaration's replay shapes.
    pub fn open_observed_projection(
        &mut self,
        decl: crate::substrate::ObservedProjection,
    ) -> ObservedProjectionId {
        if !decl.has_declared_shape() {
            return ObservedProjectionId(0);
        }
        let observer_id = register_rust_observer_muted(&self.observer_slot, decl.observer);
        if observer_id.0 == 0 {
            return observer_id;
        }
        let Some((identity, interest)) = crate::subs::interest_builder::build_interest_pair(
            &decl.filter_json,
            &decl.consumer_id,
            decl.scope,
            decl.relay_pin.as_deref(),
            decl.is_indexer_discovery,
            decl.lifecycle.clone(),
        ) else {
            unregister_observer_internal(&self.observer_slot, observer_id);
            return ObservedProjectionId(0);
        };
        self.observed_projection_sessions.insert(
            observer_id,
            (
                decl.filter_json.clone(),
                decl.consumer_id.clone(),
                decl.scope,
                decl.relay_pin.clone(),
                decl.is_indexer_discovery,
            ),
        );
        let replay = crate::kernel::ObserverReplayRequest {
            observer_id,
            shapes: decl.replay_shapes,
            limit: decl.replay_limit,
        };
        let _ = self.kernel.open_interest_with_observer_replay(
            identity,
            interest,
            replay,
            "open-observed-projection",
        );
        let outbound = self.kernel.drain_lifecycle_outbound();
        let _ = self.kernel.partition_auth_paused(outbound);
        observer_id
    }

    /// Close a reducer/browser observed projection by id.
    pub fn close_observed_projection(&mut self, id: ObservedProjectionId) {
        let Some((filter_json, consumer_id, scope, relay_pin, is_indexer_discovery)) =
            self.observed_projection_sessions.remove(&id)
        else {
            return;
        };
        // Close is identity-only; lifecycle is not part of the registry key.
        if let Some((identity, _interest)) = crate::subs::interest_builder::build_interest_pair(
            &filter_json,
            &consumer_id,
            scope,
            relay_pin.as_deref(),
            is_indexer_discovery,
            crate::planner::InterestLifecycle::Tailing,
        ) {
            let _ = self.kernel.close_interest_sub(&identity);
        }
        unregister_observer_internal(&self.observer_slot, id);
        let outbound = self.kernel.drain_lifecycle_outbound();
        let _ = self.kernel.partition_auth_paused(outbound);
    }

    /// Build a cloneable command-backed observed-projection registrar for
    /// post-start runtime controllers.
    #[must_use]
    pub fn observed_projection_command_handle(
        &self,
        sessions: ObservedProjectionSessionMap,
        sender: crate::CommandSender,
    ) -> ObservedProjectionCommandHandle {
        ObservedProjectionCommandHandle::new(Arc::clone(&self.observer_slot), sessions, sender)
    }
}
