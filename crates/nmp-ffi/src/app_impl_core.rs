//! Core `impl NmpApp` methods — extracted from `lib.rs` to keep each file
//! under the 500-LOC ceiling (AGENTS.md file-size rule).
//!
//! Covers: `send_cmd`, `show_toast`, `mark_changed_since_emit`,
//! `declare_active_follows_feed`, `clear_active_follows_feed`,
//! action-registry methods, composition-ledger helpers,
//! `set_pending_mls_autopublish`, `take_pending_mls_autopublish`.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use nmp_core::ActorCommand;

use crate::app_struct::NmpApp;

impl NmpApp {
    /// Send a command to the actor thread.
    ///
    /// D6: a disconnected channel (actor thread panicked or exited) must
    /// degrade gracefully — never panic, never write to stderr from library
    /// code. The send is best-effort; the dropped command is the failure
    /// signal.
    ///
    /// D7 (actor-death visibility): if the actor thread panics, the
    /// supervisor closure in `nmp_app_new` emits one
    /// `UpdateEnvelope::Panic` frame on the update channel before the channel
    /// closes — see [`crate::update_envelope`]'s actor-death contract. So a
    /// dropped command here is no longer *silent*: the host has already
    /// received (or will receive) the terminal panic frame and is expected
    /// to surface a fatal error rather than keep sending.
    pub(crate) fn send_cmd(&self, cmd: ActorCommand) {
        // G-S4 — straddle counter: increment before the send so the kernel
        // never observes a command "in flight" with a stale-low depth. The
        // actor decrements as it dequeues. `Relaxed` is sufficient — the value
        // is approximate observability, not a synchronization edge.
        self.queue_depth.fetch_add(1, Ordering::Relaxed);
        // Test-only monotone counter: never decremented.
        #[cfg(test)]
        self.send_cmd_count.fetch_add(1, Ordering::Relaxed);
        // Test-only last-variant tag: records which `ActorCommand` was most
        // recently sent, so tests can assert the SPECIFIC variant (e.g.
        // `CancelPublish`, not just "some command") without inspecting the actor's
        // internal state. Only the discriminant names needed by existing tests are
        // listed; the `_` arm covers all others.
        #[cfg(test)]
        if let Ok(mut tag) = self.last_cmd_tag.lock() {
            *tag = Some(match &cmd {
                ActorCommand::CancelPublish { .. } => "CancelPublish",
                ActorCommand::RetryPublish { .. } => "RetryPublish",
                _ => "_other",
            });
        }
        let _ = self.tx.send(cmd);
    }

    /// Surface a user-visible toast message (D6: best-effort delivery).
    ///
    /// Typed wrapper for [`ActorCommand::ShowToast`]. Callers inside `nmp-ffi`
    /// must use this method instead of constructing the raw command directly.
    pub(crate) fn show_toast(&self, message: String) {
        self.send_cmd(ActorCommand::ShowToast { message });
    }

    /// Mark the kernel dirty so host-registered snapshot projections re-emit.
    ///
    /// Typed wrapper for [`ActorCommand::MarkChangedSinceEmit`]. Used when
    /// reusable NMP extension state changes outside a typed kernel field (e.g.
    /// a registered feed viewport expanding older rows).
    pub(crate) fn mark_changed_since_emit(&self) {
        self.send_cmd(ActorCommand::MarkChangedSinceEmit);
    }


    /// Declare a feed of app-owned primary kinds from the active account's
    /// reactive follows perspective.
    ///
    /// The caller supplies primary content kinds only. Repost wrappers are
    /// derived here before the actor receives the compiled acquisition set, so
    /// `nmp-core` never owns the app's primary-kind policy.
    pub fn declare_active_follows_feed<I>(&self, primary_kinds: I) -> bool
    where
        I: IntoIterator<Item = u32>,
    {
        let acquisition_kinds = match nmp_nip18::try_acquisition_kinds_for_primary(primary_kinds) {
            Ok(kinds) => kinds,
            Err(_) => {
                self.show_toast(
                    "declare_active_follows_feed: primary kinds must not include repost wrappers or the delete kind"
                        .to_string(),
                );
                return false;
            }
        };
        self.send_cmd(ActorCommand::DeclareActiveFollowsFeed { acquisition_kinds });
        true
    }

    /// Clear the active-follows feed declaration.
    pub fn clear_active_follows_feed(&self) {
        self.send_cmd(ActorCommand::ClearActiveFollowsFeed);
    }

    /// Register a typed [`nmp_core::substrate::ActionModule`] `M` against the
    /// app's action registry — ADR-0027's single-call typed seam, and the
    /// sole host action-registration path on master.
    ///
    /// `M::start` handles validation AND `M::execute` handles execution, both
    /// under the same typed namespace (`M::NAMESPACE`): there is no possible
    /// partial-registration gap.
    ///
    /// Registration MUST happen during host init — before `nmp_app_start`
    /// and before any [`action::nmp_app_dispatch_action_bytes`] call. ADR-0052 rung
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

    /// Typed-only byte-doorway gate probe (ADR-0064 / #1756).
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn untyped_action_namespaces(&self) -> Vec<String> {
        self.action_registry.untyped_namespaces()
    }

    /// ADR-0049 — read-only handle to the composition ledger for
    /// `nmp_app_composition_report`.
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
    pub(crate) fn record_slot_decision(
        &self,
        seam: &'static str,
        key: &'static str,
        had_previous: bool,
    ) {
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
    pub(crate) fn set_pending_mls_autopublish(&self, enabled: bool) {
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
    pub(crate) fn open_uri(&self, uri: String) {
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
        self.send_cmd(ActorCommand::Start {
            visible_limit,
            emit_hz,
            initial_relays,
        });
    }

    /// Reconfigure the kernel's visible-limit and emit-hz without a full
    /// restart. Typed wrapper for [`ActorCommand::Configure`].
    pub(crate) fn configure(&self, visible_limit: usize, emit_hz: u32) {
        self.send_cmd(ActorCommand::Configure {
            visible_limit,
            emit_hz,
        });
    }

    /// Signal the kernel to stop. Typed wrapper for [`ActorCommand::Stop`].
    pub(crate) fn stop(&self) {
        self.send_cmd(ActorCommand::Stop);
    }

    /// Signal the kernel to reset. Typed wrapper for [`ActorCommand::Reset`].
    pub(crate) fn reset(&self) {
        self.send_cmd(ActorCommand::Reset);
    }

    /// Report an app-lifecycle phase transition to the actor (T118 / G3).
    ///
    /// Typed wrapper for [`ActorCommand::LifecycleEvent`]. Used by the
    /// lifecycle FFI symbols so they do not construct `ActorCommand` directly.
    pub(crate) fn lifecycle_event(&self, phase: nmp_core::__ffi_internal::LifecyclePhase) {
        self.send_cmd(ActorCommand::LifecycleEvent(phase));
    }

    /// Request clean actor shutdown.
    ///
    /// Typed wrapper for [`ActorCommand::Shutdown`]; used by `Drop` so the
    /// impl does not construct `ActorCommand` directly.
    pub(crate) fn shutdown_actor(&self) {
        self.send_cmd(ActorCommand::Shutdown);
    }
}
