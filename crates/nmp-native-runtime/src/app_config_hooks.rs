//! Read-once host hook and interceptor setters for `NmpApp`.

use nmp_core::subs::PlanCoverageHook;

use crate::{NmpApp, NmpConfigStatus};

impl NmpApp {
    /// Install the D2 coverage-gate hook. MUST be called before
    /// [`nmp_app_start`]. The hook is a closure that receives a
    /// [`nmp_planner::CompiledPlan`] after M2 compile and may mutate it
    /// (e.g. prune relays or mark sub-shapes for negentropy). See
    /// [`crate::subs::PlanCoverageHook`].
    ///
    /// D0: `nmp-core` defines the seam; the assembly crate installs the policy
    /// closure (today `nmp-app-chirp` consumes [`nmp_coverage_gate::CoverageGate`]).
    ///
    /// The hook lives in an `Arc<Mutex<Option<…>>>` pre-start slot. Actor
    /// startup snapshots it into config and binds that snapped hook onto the
    /// `SubscriptionLifecycle`; `Reset` re-applies the same snapped hook.
    /// A later call only mutates the dormant FFI-side slot and does not affect
    /// the already-running actor.
    ///
    /// D6 — a poisoned slot mutex is a silent no-op (the host's hook is
    /// dropped); the lifecycle keeps whatever policy was previously
    /// installed (or `None`).
    pub fn set_coverage_hook(&self, hook: PlanCoverageHook) -> NmpConfigStatus {
        if let Err(status) =
            self.ensure_prestart_config("coverage_hook", "coverage_hook", "coverage_hook")
        {
            return status;
        }
        if let Ok(mut slot) = self.composition.coverage_hook.lock() {
            // ADR-0069 Part 2 — record before overwriting so `had_previous`
            // reflects the pre-write state of this last-writer-wins slot.
            self.record_slot_decision("coverage_hook", "coverage_hook", slot.is_some());
            *slot = Some(hook);
            NmpConfigStatus::Ok
        } else {
            NmpConfigStatus::Unavailable
        }
    }

    /// Install the outbound planner REQ interceptor.
    ///
    /// MUST be called before `nmp_app_start`. Actor startup snapshots this slot
    /// and re-applies the same interceptor after `Reset`; absent means every
    /// planner REQ follows the raw NIP-01 path.
    pub fn set_req_frame_interceptor(
        &self,
        interceptor: std::sync::Arc<dyn nmp_core::substrate::ReqFrameInterceptor>,
    ) -> NmpConfigStatus {
        if let Err(status) = self.ensure_prestart_config(
            "req_frame_interceptor",
            "req_frame_interceptor",
            "req_frame_interceptor",
        ) {
            return status;
        }
        if let Ok(mut slot) = self.composition.req_frame_interceptor.lock() {
            self.record_slot_decision(
                "req_frame_interceptor",
                "req_frame_interceptor",
                slot.is_some(),
            );
            *slot = Some(interceptor);
            NmpConfigStatus::Ok
        } else {
            NmpConfigStatus::Unavailable
        }
    }

    /// Install the substrate-generic [`nmp_core::substrate::HostOpHandler`].
    ///
    /// The handler is the bridge between an [`nmp_core::substrate::ActionModule`]
    /// whose `execute()` body emits an `ActorCommand::Protocol` carrying a
    /// `nmp_core::substrate::HostOpCommand` (ADR-0072 §D4, K2 rung 5.4 - the
    /// bespoke `DispatchHostOp` arm was merged into the single `Protocol` write
    /// seam) and app-owned state the op mutates. Marmot no longer uses this
    /// path; it installs a crate-owned runtime. The actor snapshots the
    /// handler at `nmp_app_start`;
    /// `HostOpCommand` clones that handler at `run` time and calls
    /// `handle(action_json, correlation_id)`.
    ///
    /// `nmp-core` deliberately does NOT name the app's typed action enum
    /// (D0 - no app-specific nouns in the kernel); the handler
    /// speaks only `&str` + [`serde_json::Value`]. The matching `ActionModule`
    /// lives in the app crate and serializes its typed action into the same
    /// JSON envelope the handler parses back out.
    ///
    /// The slot is `Arc<Mutex<Option<Arc<dyn HostOpHandler>>>>` so app
    /// composition can install the handler without `&mut self` before start.
    /// Stage 2 of #618 snapshots it into actor config at `nmp_app_start`; a
    /// later setter only mutates the dormant FFI-side slot and does not affect
    /// the already-running actor.
    ///
    /// D6 — a poisoned slot mutex is a silent no-op (the host's handler is
    /// dropped on the floor); the slot keeps whatever value was previously
    /// installed (or `None`, in which case the `HostOpCommand` records the
    /// `Failed { reason: "no host op handler installed" }` terminal). MUST
    /// be called before `nmp_app_start` for any app whose
    /// `ActionModule::execute` emits a host-op `Protocol` command.
    pub fn set_host_op_handler(
        &self,
        handler: std::sync::Arc<dyn nmp_core::substrate::HostOpHandler>,
    ) -> NmpConfigStatus {
        if let Err(status) =
            self.ensure_prestart_config("host_op_handler", "host_op_handler", "host_op_handler")
        {
            return status;
        }
        if let Ok(mut slot) = self.composition.host_op_handler.lock() {
            self.record_slot_decision("host_op_handler", "host_op_handler", slot.is_some());
            *slot = Some(handler);
            NmpConfigStatus::Ok
        } else {
            NmpConfigStatus::Unavailable
        }
    }

    /// V-38 — install a substrate-generic [`nmp_core::substrate::RelayTextInterceptor`].
    /// Today the only consumer is `nmp-nip47`'s NWC runtime, which peeks
    /// at every inbound text frame from the wallet relay to decode
    /// kind:23195 responses before the kernel drops them as unknown kinds.
    ///
    /// MUST be called before `nmp_app_start`; otherwise the wallet runtime is
    /// unreachable and the actions surface a `Failed` terminal. A poisoned
    /// mutex is a silent no-op (D6).
    pub fn set_relay_text_interceptor(
        &self,
        interceptor: std::sync::Arc<dyn nmp_core::substrate::RelayTextInterceptor>,
    ) -> NmpConfigStatus {
        if let Err(status) = self.ensure_prestart_config(
            "relay_text_interceptor",
            "relay_text_interceptor",
            "relay_text_interceptor",
        ) {
            return status;
        }
        if let Ok(mut slot) = self.composition.relay_text_interceptor.lock() {
            self.record_slot_decision(
                "relay_text_interceptor",
                "relay_text_interceptor",
                !slot.is_empty(),
            );
            slot.clear();
            slot.push(interceptor);
            NmpConfigStatus::Ok
        } else {
            NmpConfigStatus::Unavailable
        }
    }

    /// Add a relay-text interceptor without removing existing ones.
    ///
    /// Protocol runtimes installed by different composition layers can share
    /// the same inbound frame stream. Each runtime filters the frames it owns
    /// and returns an empty vector for the rest. MUST be called before
    /// `nmp_app_start`; actor startup snapshots the installed interceptor list.
    pub fn add_relay_text_interceptor(
        &self,
        interceptor: std::sync::Arc<dyn nmp_core::substrate::RelayTextInterceptor>,
    ) -> NmpConfigStatus {
        if let Err(status) = self.ensure_prestart_config(
            "relay_text_interceptor",
            "relay_text_interceptor",
            "relay_text_interceptor",
        ) {
            return status;
        }
        if let Ok(mut slot) = self.composition.relay_text_interceptor.lock() {
            self.record_slot_decision(
                "relay_text_interceptor",
                "relay_text_interceptor",
                !slot.is_empty(),
            );
            slot.push(interceptor);
            NmpConfigStatus::Ok
        } else {
            NmpConfigStatus::Unavailable
        }
    }

    /// ADR-0072 — install a [`nmp_core::substrate::RelayConnectedHook`]
    /// (today `nmp-nip11`); the actor snapshots installed hooks at start and
    /// fans them on `PoolEvent::Opened`. Additive.
    pub fn add_relay_connected_hook(
        &self,
        hook: std::sync::Arc<dyn nmp_core::substrate::RelayConnectedHook>,
    ) -> NmpConfigStatus {
        if let Err(status) = self.ensure_prestart_config(
            "relay_connected_hook",
            "relay_connected_hook",
            "relay_connected_hook",
        ) {
            return status;
        }
        if let Ok(mut hooks) = self.composition.relay_connected_hook.lock() {
            self.record_slot_decision(
                "relay_connected_hook",
                "relay_connected_hook",
                !hooks.is_empty(),
            );
            hooks.push(hook);
            NmpConfigStatus::Ok
        } else {
            NmpConfigStatus::Unavailable
        }
    }
}
