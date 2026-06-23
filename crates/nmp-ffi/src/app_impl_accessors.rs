//! Accessor + identity + observer `impl NmpApp` methods — extracted from
//! `lib.rs` to keep each file under the 500-LOC ceiling (AGENTS.md
//! file-size rule).
//!
//! Covers: `register_event_observer`, `unregister_event_observer`,
//! `event_observers_slot`, `swap_singleton_event_observer`,
//! `push_interest`, `dispatch_capability`, `mls_local_nsec`,
//! `active_local_keys`, `active_account_handle`,
//! `register_identity_change_observer`, `event_store_handle`,
//! `pull_cursor_registry_handle`, `event_observers_handle`,
//! `command_sender`, `event_by_id`, `routing_trace`,
//! `publish_signed_explicit`, `actor_sender`, `add_signer`,
//! `remove_account`, `recall_local_nsec`, `register_action_result_observer`,
//! and the `impl ActionRegistrar for NmpApp` block.

use std::sync::Arc;

use nmp_core::__ffi_internal::{
    dispatch_capability, register_rust_observer, unregister_observer, KernelEventObserverSlot,
};
use nmp_core::slots::{
    event_by_id_from_store, ActiveAccountSlot, ActiveLocalKeysSlot, EventStoreSlot,
    PullCursorRegistryHandleSlot,
};
use nmp_core::{ActorCommand, KernelEventObserver, KernelEventObserverId};
use zeroize::Zeroizing;

use crate::app_struct::NmpApp;

impl NmpApp {
    /// T146 — register a typed Rust observer. Returns an opaque id the
    /// caller retains to unregister later via
    /// [`Self::unregister_event_observer`].
    #[must_use]
    pub fn register_event_observer(
        &self,
        observer: Arc<dyn KernelEventObserver>,
    ) -> KernelEventObserverId {
        register_rust_observer(&self.event_observers, observer)
    }

    /// T146 — unregister a previously-registered observer. Idempotent;
    /// unknown ids are silent no-ops (D6).
    pub fn unregister_event_observer(&self, id: KernelEventObserverId) {
        unregister_observer(&self.event_observers, id);
    }

    /// T146 — clone of the kernel event observer slot. The `ffi::event_observer`
    /// FFI surface uses this to plug C-ABI registrations into the same slot
    /// that backs the typed Rust API above. Crate-private because external
    /// Rust callers should go through
    /// [`Self::register_event_observer`] / [`Self::unregister_event_observer`].
    #[must_use]
    pub(crate) fn event_observers_slot(&self) -> KernelEventObserverSlot {
        Arc::clone(&self.event_observers)
    }

    /// Atomically swap the per-app's singleton kernel-event observer-id slot:
    /// store `new` and return whatever was previously installed there.
    ///
    /// Idempotent-re-invoke contract: a per-app crate that wires exactly one
    /// auxiliary `KernelEventObserver` per app uses this slot to ensure a
    /// second registration unregisters the first one before installing itself.
    /// A poisoned mutex degrades to `None` (D6).
    #[must_use]
    pub fn swap_singleton_event_observer(
        &self,
        new: Option<KernelEventObserverId>,
    ) -> Option<KernelEventObserverId> {
        let mut guard = self.singleton_event_observer_id.lock().ok()?;
        let prev = guard.take();
        *guard = new;
        prev
    }

    /// Push a `LogicalInterest` into the subscription registry and schedule a
    /// recompile. Idempotent: same `InterestId` replaces the prior entry.
    pub fn push_interest(&self, interest: nmp_planner::LogicalInterest) {
        self.send_cmd(ActorCommand::PushInterest(interest));
    }

    /// Route a typed capability request through the registered native
    /// callback.
    #[must_use]
    pub fn dispatch_capability(
        &self,
        request: &nmp_core::substrate::CapabilityRequest,
    ) -> nmp_core::substrate::CapabilityEnvelope {
        let json = serde_json::to_string(request).unwrap_or_else(|_| "{}".to_string());
        let payload = dispatch_capability(&self.capability_callback, &json);
        serde_json::from_str(&payload).unwrap_or_else(|_| nmp_core::substrate::CapabilityEnvelope {
            namespace: request.namespace.clone(),
            correlation_id: request.correlation_id.clone(),
            result_json: r#"{"status":"error","os_status":-50}"#.to_string(),
        })
    }

    /// Return the active local (nsec-backed) secret key in `nsec1…` bech32
    /// form, or `None` when no local account is active.
    #[must_use]
    pub fn mls_local_nsec(&self) -> Option<Zeroizing<String>> {
        self.mls_local_nsec.lock().ok()?.clone()
    }

    /// Clone of the active-local-`nostr::Keys` slot — substrate-generic.
    #[must_use]
    pub fn active_local_keys(&self) -> ActiveLocalKeysSlot {
        Arc::clone(&self.active_local_keys)
    }

    /// V-82 — clone of the kernel's active-account hex-pubkey slot (`Arc`).
    #[must_use]
    pub fn active_account_handle(&self) -> ActiveAccountSlot {
        Arc::clone(&self.active_account_handle)
    }

    /// Register a Rust-side callback for active-account changes.
    ///
    /// The callback runs on the update-listener thread after the actor has
    /// written [`Self::active_account_handle`] and emitted an update frame.
    pub fn register_identity_change_observer<F>(&self, callback: F)
    where
        F: Fn(Option<String>) + Send + Sync + 'static,
    {
        if let Ok(mut observers) = self.identity_change_observers.lock() {
            observers.push(Arc::new(callback));
        }
    }

    /// V-83 — clone of the kernel's `EventStore` publish-back slot (`Arc`).
    #[must_use]
    pub fn event_store_handle(&self) -> EventStoreSlot {
        Arc::clone(&self.event_store_handle)
    }

    /// ADR-0058 step 3b — clone of the kernel's pull-cursor registry handle slot.
    #[must_use]
    pub fn pull_cursor_registry_handle(&self) -> PullCursorRegistryHandleSlot {
        Arc::clone(&self.pull_cursor_registry)
    }

    /// #1740 step 2 — clone the actor command sender.
    ///
    /// Captured into a feed-session teardown closure so it can post a
    /// `MarkChangedSinceEmit` after removing the session's registrations.
    #[must_use]
    pub fn command_sender(&self) -> nmp_core::CommandSender {
        self.tx.clone()
    }

    /// V-83 — synchronous event-by-id read against the kernel's event store.
    #[must_use]
    pub fn event_by_id(&self, id: &str) -> Option<nmp_core::substrate::KernelEvent> {
        event_by_id_from_store(&self.event_store_handle, id)
    }

    /// V-51 phase 4 — clone of the kernel's [`RoutingTraceProjection`]
    /// (`Arc`).
    ///
    /// Returns `None` until `nmp_app_start` spawns the actor and the actor has
    /// constructed the kernel.
    #[must_use]
    pub fn routing_trace(&self) -> Option<Arc<nmp_core::RoutingTraceProjection>> {
        self.routing_trace.lock().ok()?.clone()
    }

    /// Clone of the actor command sender. Used by Rust-side runtime
    /// controllers that need to report work back to the actor without a C
    /// round-trip.
    #[must_use]
    pub fn actor_sender(&self) -> nmp_core::CommandSender {
        self.tx.clone()
    }

    /// Add a signer through the actor-owned identity reducer — the **single
    /// documented entry point** for all sign-in paths.
    pub fn add_signer(&self, source: nmp_core::SignerSource, make_active: bool) {
        if make_active && matches!(source, nmp_core::SignerSource::LocalNsec(_)) {
            self.set_pending_mls_autopublish(true);
        }
        self.send_cmd(ActorCommand::AddSigner {
            source,
            make_active,
        });
    }

    /// Remove an identity through the actor-owned identity reducer.
    pub fn remove_account(&self, identity_id: String) {
        self.send_cmd(ActorCommand::RemoveAccount { identity_id });
    }

    // `remove_account_forgetting_keyring` lives in `keyring_forget.rs` (kept
    // out of this file to respect its LOC ceiling — the D6 fail-loud body is
    // larger than the one-liner it replaced).

    /// Recall a previously-persisted local secret from the keyring capability.
    pub fn recall_local_nsec(&self, account_id: &str) -> Option<String> {
        let req = nmp_core::substrate::KeyringIdentityWiring::recall_secret(
            "nmp.identity.recall",
            account_id,
        );
        let envelope = self.dispatch_capability(&req);
        let result = nmp_core::substrate::KeyringIdentityWiring::decode_result(&envelope);
        match result.status {
            nmp_core::substrate::KeyringStatus::Ok => result.secret,
            nmp_core::substrate::KeyringStatus::NotFound
            | nmp_core::substrate::KeyringStatus::Error => None,
        }
    }

    /// Register a host-supplied action-result observer — the *push*
    /// counterpart to [`Self::register_snapshot_projection`]'s pull seam.
    pub fn register_action_result_observer(
        &self,
        f: impl Fn(nmp_core::substrate::ActionResult) + Send + Sync + 'static,
    ) {
        self.action_registry.set_result_observer(f);
    }

    /// Test-only: run every registered **typed** snapshot projection directly
    /// against the app's shared registry, bypassing the actor/kernel tick.
    #[cfg(test)]
    pub(crate) fn run_typed_snapshot_projections_for_test(
        &self,
    ) -> Vec<nmp_core::TypedProjectionData> {
        self.snapshot_projections
            .lock()
            .map(|mut registry| registry.run_typed())
            .unwrap_or_default()
    }

    /// Test-only direct execution path into the action registry.
    #[cfg(test)]
    pub(crate) fn test_execute_action(
        &self,
        namespace: &str,
        action_json: &str,
    ) -> Result<(), String> {
        self.action_registry
            .execute(namespace, action_json, "test-correlation-id", &|cmd| {
                self.send_cmd(cmd)
            })
            .map_err(|failure| failure.message)
    }

    /// Workspace-internal kernel publish API — verbatim publish of an
    /// already-signed `nostr::Event` to an EXPLICIT relay set.
    pub fn publish_signed_explicit(&self, event: nostr::Event, relays: &[nostr::RelayUrl]) {
        let raw = nmp_store::RawEvent {
            id: event.id.to_hex(),
            pubkey: event.pubkey.to_hex(),
            created_at: event.created_at.as_secs(),
            kind: u32::from(event.kind.as_u16()),
            tags: event.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
            content: event.content.clone(),
            sig: event.sig.to_string(),
        };
        let relays: Vec<nmp_core::publish::RelayUrl> = relays
            .iter()
            .map(std::string::ToString::to_string)
            .collect();
        self.send_cmd(ActorCommand::PublishSignedEvent {
            raw,
            target: nmp_core::publish::PublishTarget::Explicit { relays },
            correlation_id: None,
        });
    }
}

impl nmp_core::substrate::ActionRegistrar for NmpApp {
    fn register_action<M: nmp_core::substrate::ActionModule + 'static>(&mut self, module: M) {
        NmpApp::register_action(self, module);
    }

    /// ADR-0049 Part 1 — override the trait default so the canonical NMP
    /// defaults (`nmp_nip02` / `nmp_nip17` / `nmp_nip57` / `nmp_router`, which
    /// register through `&mut impl AppHost`) get true entry-or-insert yielding
    /// semantics.
    fn register_default_action<M: nmp_core::substrate::ActionModule + 'static>(
        &mut self,
        module: M,
    ) -> bool {
        NmpApp::register_default_action(self, module)
    }
}
