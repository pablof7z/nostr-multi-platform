//! Core `impl NmpApp` methods — extracted from `lib.rs` to keep each file
//! under the 500-LOC ceiling (AGENTS.md file-size rule).
//!
//! Covers: `send_cmd`, `show_toast`, `mark_changed_since_emit`,
//! action-registry methods, composition-ledger helpers,
//! `set_pending_mls_autopublish`, `take_pending_mls_autopublish`.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use nmp_core::actor::LifecycleCommand;
#[cfg(any(test, feature = "test-support"))]
use nmp_core::actor::PublishCommand;
use nmp_core::actor::{ActorCommand, CommandSendStatus};

use crate::app_struct::NmpApp;

impl NmpApp {
    pub fn start_runtime(&self, visible_limit: usize, emit_hz: u32) {
        let initial_relays = self
            .composition
            .initial_relays_for_start
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();

        if self.consumed_projections_are_undeclared() {
            tracing::warn!(
                "NmpApp::start_runtime: host expressed no projection-consumption intent; \
                 call declare_consumed_projections or consume_all_builtin_projections before start",
            );
            #[cfg(not(any(test, feature = "test-support")))]
            debug_assert!(
                false,
                "NmpApp::start_runtime: projection-consumption intent is undeclared"
            );
        }

        let was_started = self.started.swap(true, Ordering::SeqCst);
        if !was_started {
            self.spawn_actor_if_needed();
        }
        self.start(visible_limit, emit_hz, initial_relays);
    }

    pub fn configure_runtime(&self, visible_limit: usize, emit_hz: u32) {
        self.configure(visible_limit, emit_hz);
    }

    pub fn stop_runtime(&self) {
        self.stop();
    }

    pub fn reset_runtime(&self) {
        self.reset();
    }

    #[must_use]
    pub fn is_alive(&self) -> bool {
        self.actor
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(|handle| !handle.is_finished()))
            .unwrap_or(false)
    }

    pub fn lifecycle_foreground(&self) {
        self.lifecycle_event(nmp_core::__ffi_internal::LifecyclePhase::Foreground);
    }

    pub fn lifecycle_background(&self) {
        self.lifecycle_event(nmp_core::__ffi_internal::LifecyclePhase::Background);
    }

    /// Install or clear the lifecycle observer slot.
    pub fn set_lifecycle_observer(
        &self,
        registration: Option<nmp_core::__ffi_internal::LifecycleObserverRegistration>,
    ) {
        if let Ok(mut slot) = self.lifecycle_observer.lock() {
            *slot = registration;
        }
    }

    /// Test-support: configure the kernel GC budget before start.
    #[cfg(any(test, feature = "test-support"))]
    pub fn configure_gc_budget_for_test(&self, max_events: u64) -> crate::NmpConfigStatus {
        if let Err(status) =
            self.ensure_prestart_config("gc_budget", "gc_budget_ceiling", "configure_gc_budget")
        {
            return status;
        }
        if let Ok(mut guard) = self.gc_budget_ceiling.lock() {
            *guard = Some(max_events as usize);
        }
        crate::NmpConfigStatus::Ok
    }

    /// Send a command to the actor thread.
    ///
    /// D6: a disconnected channel (actor thread panicked or exited) must
    /// degrade gracefully — never panic, never write to stderr from library
    /// code. The send is best-effort; the dropped command is the failure
    /// signal.
    ///
    /// D7 (actor-death visibility): if the actor thread panics, the
    /// actor supervisor closure emits one
    /// `UpdateEnvelope::Panic` frame on the update channel before the channel
    /// closes — see [`crate::update_envelope`]'s actor-death contract. So a
    /// dropped command here is no longer *silent*: the host has already
    /// received (or will receive) the terminal panic frame and is expected
    /// to surface a fatal error rather than keep sending.
    pub fn send_cmd(&self, cmd: ActorCommand) {
        #[cfg(any(test, feature = "test-support"))]
        let cmd_tag = match &cmd {
            ActorCommand::Publish(PublishCommand::CancelPublish { .. }) => "CancelPublish",
            ActorCommand::Publish(PublishCommand::RetryPublish { .. }) => "RetryPublish",
            _ => "_other",
        };
        // G-S4 — straddle counter: increment before the send so the actor
        // cannot dequeue an accepted command before depth observes it. If the
        // bounded send sheds or the actor is gone, roll the increment back.
        // `Relaxed` is sufficient — the value is approximate observability, not
        // a synchronization edge.
        self.queue_depth.fetch_add(1, Ordering::Relaxed);
        if matches!(self.tx.send(cmd), Ok(CommandSendStatus::Enqueued)) {
            // Test-only monotone counter: never decremented.
            #[cfg(any(test, feature = "test-support"))]
            self.send_cmd_count.fetch_add(1, Ordering::Relaxed);
            // Test-only last-variant tag: records which `ActorCommand` was most
            // recently accepted, so tests can assert the SPECIFIC variant (e.g.
            // `CancelPublish`, not just "some command") without inspecting the
            // actor's internal state.
            #[cfg(any(test, feature = "test-support"))]
            if let Ok(mut tag) = self.last_cmd_tag.lock() {
                *tag = Some(cmd_tag);
            }
        } else {
            self.queue_depth
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |d| {
                    Some(d.saturating_sub(1))
                })
                .ok();
        }
    }

    /// Surface a user-visible toast message (D6: best-effort delivery).
    ///
    /// Typed wrapper for [`ActorCommand::ShowToast`]. Runtime callers use this
    /// method instead of constructing the raw command directly.
    pub fn show_toast(&self, message: String) {
        self.send_cmd(ActorCommand::ShowToast { message });
    }

    /// Mark the kernel dirty so host-registered snapshot projections re-emit.
    ///
    /// Typed wrapper for [`ActorCommand::Lifecycle(LifecycleCommand::MarkChangedSinceEmit)`]. Used when
    /// reusable NMP extension state changes outside a typed kernel field (e.g.
    /// a registered feed viewport expanding older rows).
    pub fn mark_changed_since_emit(&self) {
        self.send_cmd(ActorCommand::Lifecycle(
            LifecycleCommand::MarkChangedSinceEmit,
        ));
    }

    /// Register a typed [`nmp_core::substrate::ActionModule`] `M` against the
    /// app's action registry — ADR-0027's single-call typed seam, and the
    /// sole host action-registration path on master.
    ///
    /// `M::start` handles validation AND `M::execute` handles execution, both
    /// under the same typed namespace (`M::NAMESPACE`): there is no possible
    /// partial-registration gap.
    ///
    /// Registration MUST happen during host init, before runtime start and
    /// before any dispatch-action call. ADR-0052 rung
    /// 5.2: takes the module **value** so a stateful module (e.g. one owning
    /// an `Arc<WalletRuntimeHandle>`) carries its deps, captured at
    /// composition time, instead of reaching a process-global.
    pub fn register_action<M: nmp_core::substrate::ActionModule + 'static>(
        &mut self,
        module: M,
    ) -> Result<(), nmp_core::substrate::RegistrationError> {
        self.action_registry.register(module)
    }

    /// Register a typed action module as a **yielding default** (ADR-0049
    /// Part 1): install it only if its namespace is unclaimed; otherwise yield
    /// to the existing registration regardless of call order.
    pub fn register_default_action<M: nmp_core::substrate::ActionModule + 'static>(
        &mut self,
        module: M,
    ) -> bool {
        self.action_registry.register_default(module)
    }

    /// Read-only access to the action registry for native host adapters.
    #[must_use]
    pub fn action_registry(&self) -> &nmp_core::__ffi_internal::ActionRegistry {
        &self.action_registry
    }

    /// Typed-only byte-doorway gate probe (ADR-0064 / #1756).
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn untyped_action_namespaces(&self) -> Vec<String> {
        self.action_registry.untyped_namespaces()
    }

    /// Test-support probe for the contract-driven default action registry gate.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn registered_action_namespaces(&self) -> Vec<String> {
        self.action_registry.action_namespaces()
    }

    /// ADR-0049 — read-only handle to the composition ledger for
    /// the C ABI composition-report wrapper.
    #[must_use]
    pub fn composition_ledger(&self) -> &Arc<nmp_core::CompositionLedger> {
        &self.composition_ledger
    }

    /// ADR-0049 Part 2 — record a last-writer-wins **wiring-slot** decision.
    ///
    /// `seam`/`key` name the slot (e.g. `"routing_substrate"`). When the app is
    /// already started the value is dropped by the actor (it read the slot once
    /// at kernel construction), so this records [`nmp_core::Disposition::DroppedLateWiring`];
    /// otherwise the slot is being (re)written pre-start.
    pub fn record_slot_decision(&self, seam: &'static str, key: &'static str, had_previous: bool) {
        let disposition = if self.started.load(Ordering::SeqCst) {
            nmp_core::Disposition::DroppedLateWiring
        } else if had_previous {
            nmp_core::Disposition::ReplacedPrevious
        } else {
            nmp_core::Disposition::Installed
        };
        self.composition_ledger
            .record(seam, key, key, disposition, None);
    }

    /// Set the one-shot MLS-autopublish intent (consumed by
    /// [`Self::take_pending_mls_autopublish`] in `register_with_keys`).
    pub fn set_pending_mls_autopublish(&self, enabled: bool) {
        self.pending_mls_autopublish
            .store(enabled, Ordering::Release);
    }

    /// Reads the one-shot MLS-autopublish intent and clears it in the same
    /// atomic step (`swap`), so a second caller cannot re-observe the flag.
    #[must_use]
    pub fn take_pending_mls_autopublish(&self) -> bool {
        self.pending_mls_autopublish.swap(false, Ordering::AcqRel)
    }

    /// Route a `nostr:` URI (or bare NIP-19 entity) through the kernel reducer
    /// (T95/T80). Typed wrapper for
    /// `ActorCommand::Kernel(KernelAction::OpenUri { uri })`.
    ///
    /// D6: best-effort send; a disconnected channel is a silent no-op (see
    /// [`Self::send_cmd`]).
    pub fn open_uri(&self, uri: String) {
        self.send_cmd(ActorCommand::Kernel(nmp_core::KernelAction::OpenUri {
            uri,
        }));
    }

    /// Start the kernel with the given visible-limit and emit-hz configuration.
    /// Typed wrapper for [`ActorCommand::Start`].
    pub(crate) fn start(
        &self,
        visible_limit: usize,
        emit_hz: u32,
        initial_relays: Vec<(String, String)>,
    ) {
        self.send_cmd(ActorCommand::Lifecycle(LifecycleCommand::Start {
            visible_limit,
            emit_hz,
            initial_relays,
        }));
    }

    /// Reconfigure the kernel's visible-limit and emit-hz without a full
    /// restart. Typed wrapper for [`ActorCommand::Configure`].
    pub(crate) fn configure(&self, visible_limit: usize, emit_hz: u32) {
        self.send_cmd(ActorCommand::Lifecycle(LifecycleCommand::Configure {
            visible_limit,
            emit_hz,
        }));
    }

    /// Signal the kernel to stop. Typed wrapper for [`ActorCommand::Lifecycle(LifecycleCommand::Stop)`].
    pub(crate) fn stop(&self) {
        self.send_cmd(ActorCommand::Lifecycle(LifecycleCommand::Stop));
    }

    /// Signal the kernel to reset. Typed wrapper for [`ActorCommand::Lifecycle(LifecycleCommand::Reset)`].
    pub(crate) fn reset(&self) {
        self.send_cmd(ActorCommand::Lifecycle(LifecycleCommand::Reset));
    }

    /// Report an app-lifecycle phase transition to the actor (T118 / G3).
    ///
    /// Typed wrapper for [`ActorCommand::LifecycleEvent`]. Used by the
    /// lifecycle FFI symbols so they do not construct `ActorCommand` directly.
    pub(crate) fn lifecycle_event(&self, phase: nmp_core::__ffi_internal::LifecyclePhase) {
        self.send_cmd(ActorCommand::Lifecycle(LifecycleCommand::LifecycleEvent(
            phase,
        )));
    }

    /// Request clean actor shutdown.
    ///
    /// Typed wrapper for [`ActorCommand::Lifecycle(LifecycleCommand::Shutdown)`]; used by `Drop` so the
    /// impl does not construct `ActorCommand` directly.
    pub(crate) fn shutdown_actor(&self) {
        self.send_cmd(ActorCommand::Lifecycle(LifecycleCommand::Shutdown));
    }

    /// Explicit idempotent teardown: clear the update listener, send the
    /// `Shutdown` command, and join both the actor and listener threads.
    ///
    /// Safe to call multiple times — the `Mutex<Option<JoinHandle>>` pattern
    /// means a second call sees `None` and is a no-op. `Drop` calls this as a
    /// fallback so hosts that never call `shutdown()` explicitly are still safe.
    ///
    /// **UniFFI contract**: named `shutdown` (not `close`) to avoid Kotlin
    /// `AutoCloseable` friction discovered in #2149.
    pub fn shutdown(&self) {
        // 1. Clear the update listener so no more callbacks fire.
        if let Ok(mut inner) = self.update_listener.inner.lock() {
            inner.listener = None;
        }
        // 2. Send Shutdown so the actor exits its event loop.
        self.shutdown_actor();
        // 3. Drop BOTH update-channel senders so the listener thread can exit.
        //
        //    There are two update-tx clones alive when the actor was never started:
        //    (a) inside actor_starter (the original update_tx captured by the Box),
        //    (b) startup_update_tx (a clone stored in the mutex).
        //    When the actor IS running, (a) was already transferred into the actor
        //    thread via spawn_actor_if_needed(); dropping (a) here is a no-op since
        //    actor_starter contains None. Either way, we must drop both.
        if let Ok(mut starter) = self.actor_starter.lock() {
            starter.take(); // drops original update_tx captured in the Box (pre-start only)
        }
        if let Ok(mut startup_tx) = self.startup_update_tx.lock() {
            startup_tx.take(); // drops the startup clone
        }
        // 4. Join the actor thread (idempotent via Option::take).
        if let Ok(mut actor) = self.actor.lock() {
            if let Some(handle) = actor.take() {
                let _ = handle.join();
            }
        }
        // 5. Join the update-listener thread (idempotent via Option::take).
        //    Now that all senders are dropped (or the actor thread exited), the
        //    update_rx.recv() loop in the listener thread will return Err and exit.
        if let Ok(mut listener) = self.update_listener_thread.lock() {
            if let Some(handle) = listener.take() {
                let _ = handle.join();
            }
        }
    }
}
