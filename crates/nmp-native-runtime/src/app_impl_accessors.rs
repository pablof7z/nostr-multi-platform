//! Accessor + identity `impl NmpApp` methods — extracted from
//! `lib.rs` to keep each file under the 500-LOC ceiling (AGENTS.md
//! file-size rule).
//!
//! Covers: `ensure_interest`, `dispatch_capability`, `active_local_keys`,
//! `active_account_handle`,
//! `register_identity_change_observer`, `event_store_handle`,
//! `pull_cursor_registry_handle`, `event_observers_handle`,
//! `command_sender`, `event_by_id`, `routing_trace`,
//! `publish_signed_explicit`, `actor_sender`, `add_signer`,
//! `remove_account`, `recall_local_nsec`, `register_action_result_observer`,
//! and the `impl ActionRegistrar for NmpApp` block.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use nmp_core::__ffi_internal::dispatch_capability;
use nmp_core::actor::ActorCommand;
use nmp_core::actor::{
    IdentityCommand, InterestsCommand, PublishCommand, RefsCommand, RelayCommand, SignCommand,
};
use nmp_core::slots::{
    event_by_id_from_store, ActiveAccountSlot, ActiveLocalKeysSlot, EventStoreSlot,
    PullCursorRegistryHandleSlot,
};

use crate::app_struct::NmpApp;

impl NmpApp {
    /// Attach one scoped owner to a `LogicalInterest`.
    pub fn ensure_interest(
        &self,
        identity: nmp_core::subs::SubIdentity,
        interest: nmp_planner::LogicalInterest,
    ) {
        self.send_cmd(ActorCommand::Interests(InterestsCommand::EnsureInterest {
            identity,
            interest,
        }));
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

    /// Clone of the active-local-`nostr::Keys` slot — substrate-generic.
    #[must_use]
    pub fn active_local_keys(&self) -> ActiveLocalKeysSlot {
        Arc::clone(&self.read_handles.active_local_keys)
    }

    /// V-82 — clone of the kernel's active-account hex-pubkey slot (`Arc`).
    #[must_use]
    pub fn active_account_handle(&self) -> ActiveAccountSlot {
        Arc::clone(&self.read_handles.active_account_handle)
    }

    /// Register a Rust-side callback for active-account changes.
    ///
    /// The callback runs on the update-listener thread after the actor has
    /// written [`Self::active_account_handle`] and emitted an update frame.
    pub fn register_identity_change_observer<F>(
        &self,
        callback: F,
    ) -> crate::IdentityChangeObserverId
    where
        F: Fn(Option<String>) + Send + Sync + 'static,
    {
        let id = self
            .next_identity_change_observer_id
            .fetch_add(1, Ordering::Relaxed);
        if let Ok(mut observers) = self.identity_change_observers.lock() {
            observers.push(crate::app_struct::IdentityChangeObserverRegistration {
                id,
                callback: Arc::new(callback),
            });
        }
        id
    }

    /// Revoke a Rust-side active-account callback registered by
    /// [`Self::register_identity_change_observer`]. Idempotent for unknown ids.
    pub fn unregister_identity_change_observer(&self, id: crate::IdentityChangeObserverId) {
        crate::app_struct::unregister_identity_change_observer(&self.identity_change_observers, id);
    }

    pub fn register_configured_relays_change_observer<F>(&self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        if let Ok(mut observers) = self.configured_relays_change_observers.lock() {
            observers.push(Arc::new(callback));
        }
    }

    /// V-83 — clone of the kernel's `EventStore` publish-back slot (`Arc`).
    #[must_use]
    pub fn event_store_handle(&self) -> EventStoreSlot {
        Arc::clone(&self.read_handles.event_store_handle)
    }

    /// ADR-0072 step 3b — clone of the kernel's pull-cursor registry handle slot.
    #[must_use]
    pub fn pull_cursor_registry_handle(&self) -> PullCursorRegistryHandleSlot {
        Arc::clone(&self.read_handles.pull_cursor_registry)
    }

    /// #1740 step 2 — clone the actor command sender.
    ///
    /// Captured into a feed-session teardown closure so it can post a
    /// `MarkChangedSinceEmit` after removing the session's registrations.
    #[must_use]
    pub fn command_sender(&self) -> nmp_core::CommandSender {
        self.tx.clone()
    }

    /// V-83 — bounded helper read against the kernel's published event-store
    /// handle.
    ///
    /// D5 exception: normal host UI data still flows through typed projections
    /// and update frames. This helper is a narrow hydration seam for callers
    /// that already hold a concrete event id (for example OP-feed embed
    /// hydration); it cannot scan, query by author/kind, or synthesize snapshot
    /// state. The `event_by_id_tests` suite proves the handle is kernel-authored
    /// and is republished across reset.
    #[must_use]
    pub fn event_by_id(&self, id: &str) -> Option<nmp_core::substrate::KernelEvent> {
        event_by_id_from_store(&self.read_handles.event_store_handle, id)
    }

    /// V-51 phase 4 — clone of the kernel's [`RoutingTraceProjection`]
    /// (`Arc`).
    ///
    /// Returns `None` until `nmp_app_start` spawns the actor and the actor has
    /// constructed the kernel.
    #[must_use]
    pub fn routing_trace(&self) -> Option<Arc<nmp_core::RoutingTraceProjection>> {
        self.read_handles.routing_trace.lock().ok()?.clone()
    }

    /// Clone of the actor command sender. Used by Rust-side runtime
    /// controllers that need to report work back to the actor without a C
    /// round-trip.
    #[must_use]
    pub fn actor_sender(&self) -> nmp_core::CommandSender {
        self.tx.clone()
    }

    /// Add a signer through the actor-owned identity reducer - the single
    /// documented entry point for all sign-in paths.
    pub fn add_signer(&self, source: nmp_core::SignerSource, make_active: bool) {
        self.send_cmd(ActorCommand::Identity(IdentityCommand::AddSigner {
            source,
            make_active,
        }));
    }

    /// Remove an identity through the actor-owned identity reducer.
    pub fn remove_account(&self, identity_id: String) {
        self.send_cmd(ActorCommand::Identity(IdentityCommand::RemoveAccount {
            identity_id,
        }));
    }

    /// Sign an event draft and park the result in `signed_events`.
    /// Typed wrapper for [`ActorCommand::SignEventForReturn`].
    pub fn sign_event_for_return(
        &self,
        account_pubkey: String,
        unsigned_json: String,
        correlation_id: String,
    ) {
        self.send_cmd(ActorCommand::Sign(SignCommand::EventForReturn {
            account_pubkey,
            unsigned_json,
            correlation_id,
        }));
    }

    /// Create a new account through the actor-owned identity reducer.
    /// Typed wrapper for [`ActorCommand::CreateAccount`].
    pub fn create_account(
        &self,
        profile: std::collections::HashMap<String, String>,
        relays: Vec<(String, String)>,
        initial_follows: Vec<String>,
        mls: bool,
        make_active: bool,
    ) {
        self.send_cmd(ActorCommand::Identity(IdentityCommand::CreateAccount {
            profile,
            relays,
            initial_follows,
            mls,
            make_active,
        }));
    }

    /// Switch the active account. Typed wrapper for [`ActorCommand::SwitchActive`].
    pub fn switch_active(&self, identity_id: String) {
        self.send_cmd(ActorCommand::Identity(IdentityCommand::SwitchActive {
            identity_id,
        }));
    }

    /// Add a relay to the active account's relay list.
    /// Typed wrapper for [`ActorCommand::AddRelay`].
    pub fn add_relay(&self, url: String, role: String) {
        self.send_cmd(ActorCommand::Relay(RelayCommand::AddRelay { url, role }));
    }

    /// Remove a relay from the active account's relay list.
    /// Typed wrapper for [`ActorCommand::RemoveRelay`].
    pub fn remove_relay(&self, url: String) {
        self.send_cmd(ActorCommand::Relay(RelayCommand::RemoveRelay { url }));
    }

    /// Retry a failed publish, addressed by its handle.
    /// Typed wrapper for [`ActorCommand::RetryPublish`].
    pub fn retry_publish(&self, handle: String) {
        self.send_cmd(ActorCommand::Publish(PublishCommand::RetryPublish {
            handle,
        }));
    }

    /// Cancel an in-flight operation by its `correlation_id`.
    /// Typed wrapper for [`ActorCommand::CancelPublish`].
    pub fn cancel_publish(&self, correlation_id: String) {
        self.send_cmd(ActorCommand::Publish(PublishCommand::CancelPublish {
            correlation_id,
        }));
    }

    /// Resolve a ref (profile or event) in the kernel's ref resolver.
    /// Typed wrapper for [`ActorCommand::ResolveRef`].
    pub fn resolve_ref(
        &self,
        namespace: nmp_core::RefNamespace,
        key: String,
        consumer_id: String,
        shape: nmp_core::RefShape,
        liveness: nmp_core::RefLiveness,
    ) {
        self.send_cmd(ActorCommand::Refs(RefsCommand::Resolve {
            namespace,
            key,
            consumer_id,
            shape,
            liveness,
            force: false,
            hints: Vec::new(),
        }));
    }

    /// Resolve a ref with app-decoded metadata from a URI adapter.
    /// Typed wrapper for [`RefsCommand::ResolveWithMetadata`].
    pub fn resolve_ref_with_metadata(
        &self,
        namespace: nmp_core::RefNamespace,
        key: String,
        consumer_id: String,
        shape: nmp_core::RefShape,
        liveness: nmp_core::RefLiveness,
        metadata: nmp_core::RefResolveMetadata,
    ) {
        self.send_cmd(ActorCommand::Refs(RefsCommand::ResolveWithMetadata {
            namespace,
            key,
            consumer_id,
            shape,
            liveness,
            force: false,
            metadata,
        }));
    }

    /// Release a ref previously registered via [`Self::resolve_ref`].
    /// Typed wrapper for [`ActorCommand::ReleaseRef`].
    pub fn release_ref(&self, namespace: nmp_core::RefNamespace, key: String, consumer_id: String) {
        self.send_cmd(ActorCommand::Refs(RefsCommand::Release {
            namespace,
            key,
            consumer_id,
        }));
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

    /// Clear the host-supplied action-result observer and drain any in-flight
    /// callback before returning.
    pub fn clear_action_result_observer(&self) {
        self.action_registry.clear_result_observer();
    }

    /// Test-only: run every registered **typed** snapshot projection directly
    /// against the app's shared registry, bypassing the actor/kernel tick.
    #[cfg(any(test, feature = "test-support"))]
    pub fn run_typed_snapshot_projections_for_test(&self) -> Vec<nmp_core::TypedProjectionData> {
        self.snapshot_projections
            .lock()
            .map(|mut registry| registry.run_typed())
            .unwrap_or_default()
    }

    /// Test-only direct execution path into the action registry.
    #[cfg(any(test, feature = "test-support"))]
    pub fn test_execute_action(&self, namespace: &str, action_json: &str) -> Result<(), String> {
        let ctx =
            nmp_core::substrate::ActionContext::with_event_store_slot(self.event_store_handle());
        self.action_registry
            .execute(
                &ctx,
                namespace,
                action_json,
                "test-correlation-id",
                &|cmd| self.send_cmd(cmd),
            )
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
        self.send_cmd(ActorCommand::Publish(PublishCommand::SignedEvent {
            raw,
            target: nmp_core::publish::PublishTarget::Explicit {
                relays,
                route_class: nmp_core::publish::PublishRouteClass::ImportedOrPresigned,
            },
            correlation_id: None,
        }));
    }
}

impl nmp_core::substrate::ActionRegistrar for NmpApp {
    fn register_action<M: nmp_core::substrate::ActionModule + 'static>(
        &mut self,
        module: M,
    ) -> Result<(), nmp_core::substrate::RegistrationError> {
        self.action_registry.register(module)
    }

    /// ADR-0069 Part 1 — override the trait default so the canonical NMP
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
