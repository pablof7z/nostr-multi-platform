//! Desync-resistance proof for the shared [`CoverageGate`] in the substrate
//! tier.
//!
//! The whole reason `coverage_gate` is a [`nmp_defaults::NmpDefaults`] field (and
//! a `register_substrate(app, gate)` parameter) rather than a hardcoded literal:
//! the D2 coverage hook AND the NIP-77 negentropy runtime are **two
//! collaborators reading one policy**. If a refactor ever split them into two
//! gates — or hardcoded one of them — they would desync, and a caller's
//! coverage override would silently apply to only half the system.
//!
//! This test installs `register_substrate` against a minimal spy `AppHost` that
//! captures the two seams the gate feeds — `set_coverage_hook` and
//! `set_req_frame_interceptor` (the negentropy runtime) — using a **custom**
//! gate whose `max_relay_connections` differs from the default. It then proves:
//!
//! 1. The captured coverage hook trims a compiled plan to the **custom** cap
//!    (not the default 30) — the gate reached the hook.
//! 2. The negentropy req-frame interceptor (constructed from the *same single*
//!    `gate` value in the same call) is installed — the gate reached the
//!    runtime.
//!
//! Together: one custom gate, fed to both collaborators in one call, with the
//! hook's behaviour pinned to the custom value. A regression that constructs a
//! second gate for the hook (or hardcodes `CoverageGate::default()`) fails (1);
//! a regression that drops the negentropy runtime fails (2).
//!
//! The full Kernel-driven negentropy *threshold* assertion (driving
//! `intercept_req` to confirm the runtime honours the custom
//! `filter_fanout_negentropy_threshold`) requires a live `Kernel` + relay
//! support state and belongs to a NIP-77 integration harness, not this unit; we
//! deliberately assert the runtime's *presence* here, not its internal decision.

use std::ops::Range;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};

use nmp_core::planner::{CompiledPlan, RelayPlan};
use nmp_core::publish::OutboxResolver;
use nmp_core::slots::{
    ActiveAccountSlot, ActiveLocalKeysSlot, IndexerRelaysSlot, LocalWriteRelaysSlot,
};
use nmp_core::store::EventStore;
use nmp_core::subs::PlanCoverageHook;
use nmp_core::substrate::{
    ActionModule, ActionRegistrar, AppHost, DmInboxRelayLookup, IngestParser, MailboxCache,
    OutboxRouter, RawEventForwardPolicy, RawEventForwardPolicyContext, RelayTextInterceptor,
    ReqFrameInterceptor, RoutingTraceObserver,
};
use nmp_core::{
    ActorCommand, AppRelaySlot, ChangeGate, KernelEventObserver, KernelEventObserverId, KindFilter,
    RawEventObserver, RawEventObserverId, TypedProjectionData,
};
use nmp_coverage_gate::CoverageGate;

/// Minimal [`AppHost`] spy capturing only the two gate-fed seams. Every other
/// substrate seam used by `register_substrate` is recorded as "happened" but
/// otherwise ignored; seams `register_substrate` never touches are
/// `unreachable!()` so an accidental new call surfaces loudly.
#[derive(Default)]
struct GateSpy {
    coverage_hook: Mutex<Option<PlanCoverageHook>>,
    req_interceptor: Mutex<Option<Arc<dyn ReqFrameInterceptor>>>,
    relay_interceptors: Mutex<usize>,
}

impl ActionRegistrar for GateSpy {
    fn register_action<M: ActionModule + 'static>(&mut self) {
        // Substrate wires `nmp.nip65.publish_relay_list` here — capture-free
        // no-op; this test asserts on the coverage gate, not actions.
    }
}

impl AppHost for GateSpy {
    fn register_snapshot_projection<K, F>(&self, _key: K, _f: F)
    where
        K: Into<String>,
        F: Fn() -> serde_json::Value + Send + Sync + 'static,
    {
        unreachable!("register_substrate does not register snapshot projections");
    }

    fn register_snapshot_projection_gated<K, F>(&self, _key: K, _gate: Arc<dyn ChangeGate>, _f: F)
    where
        K: Into<String>,
        F: Fn() -> serde_json::Value + Send + Sync + 'static,
    {
        unreachable!("register_substrate does not register gated snapshot projections");
    }

    fn register_typed_snapshot_projection<K, F>(&self, _key: K, _f: F)
    where
        K: Into<String>,
        F: Fn() -> Option<TypedProjectionData> + Send + Sync + 'static,
    {
        unreachable!("register_substrate does not register typed snapshot projections");
    }

    fn register_snapshot_tick_observer<F>(&self, _f: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        unreachable!("register_substrate does not register tick observers");
    }

    fn set_coverage_hook(&self, hook: PlanCoverageHook) {
        *self.coverage_hook.lock().unwrap() = Some(hook);
    }

    fn set_req_frame_interceptor(&self, interceptor: Arc<dyn ReqFrameInterceptor>) {
        *self.req_interceptor.lock().unwrap() = Some(interceptor);
    }

    fn add_relay_text_interceptor(&self, _interceptor: Arc<dyn RelayTextInterceptor>) {
        *self.relay_interceptors.lock().unwrap() += 1;
    }

    fn add_relay_connected_hook(
        &self,
        _hook: Arc<dyn nmp_core::substrate::RelayConnectedHook>,
    ) {
    }

    fn register_ingest_parser(&self, _kind: u32, _parser: Arc<dyn IngestParser>) {
        // kind:10002 parser — recorded as a no-op; not under test here.
    }

    fn replace_ingest_parser(
        &self,
        _kind: u32,
        _slot_key: &'static str,
        _parser: Arc<dyn IngestParser>,
    ) -> Option<Arc<dyn IngestParser>> {
        unreachable!("register_substrate does not replace ingest parsers");
    }

    fn unregister_ingest_parser(&self, _kind: u32, _slot_key: &'static str) {
        unreachable!("register_substrate does not unregister ingest parsers");
    }

    fn replace_ingest_parser_range(
        &self,
        _range: Range<u32>,
        _slot_key: &'static str,
        _parser: Arc<dyn IngestParser>,
    ) -> Option<Arc<dyn IngestParser>> {
        unreachable!("register_substrate does not replace ingest-parser ranges");
    }

    fn unregister_ingest_parser_range(&self, _slot_key: &'static str) {
        unreachable!("register_substrate does not unregister ingest-parser ranges");
    }

    fn set_dm_inbox_relay_lookup(&self, _lookup: Arc<dyn DmInboxRelayLookup>) {
        unreachable!("register_substrate does not set the DM inbox relay lookup");
    }

    fn set_mailbox_cache_reader(&self, _cache: Arc<dyn MailboxCache>) {
        // Shared mailbox-cache reader — no-op; not under test here.
    }

    fn set_routing_substrate<F>(&self, _factory: F)
    where
        F: Fn(Arc<dyn RoutingTraceObserver>) -> (Arc<dyn OutboxRouter>, Arc<dyn MailboxCache>)
            + Send
            + Sync
            + 'static,
    {
        // Routing factory — no-op; not under test here.
    }

    fn set_publish_resolver_factory<F>(&self, _factory: F)
    where
        F: Fn(
                Arc<dyn EventStore>,
                IndexerRelaysSlot,
                LocalWriteRelaysSlot,
                ActiveAccountSlot,
            ) -> Arc<dyn OutboxResolver>
            + Send
            + Sync
            + 'static,
    {
        // Publish-resolver factory — no-op; not under test here.
    }

    fn set_raw_event_forward_policy_factory<F>(&self, _factory: F)
    where
        F: Fn(RawEventForwardPolicyContext) -> Vec<Arc<dyn RawEventForwardPolicy>>
            + Send
            + Sync
            + 'static,
    {
        // Raw-event forward policy — no-op; not under test here.
    }

    fn active_local_keys(&self) -> ActiveLocalKeysSlot {
        unreachable!("register_substrate does not read active local keys");
    }

    fn active_pubkey(&self) -> ActiveAccountSlot {
        unreachable!("register_substrate does not read the active pubkey");
    }

    fn actor_sender(&self) -> Sender<ActorCommand> {
        unreachable!("register_substrate does not read the actor sender");
    }

    fn register_event_observer(
        &self,
        _observer: Arc<dyn KernelEventObserver>,
    ) -> KernelEventObserverId {
        unreachable!("register_substrate does not register event observers");
    }

    fn unregister_event_observer(&self, _id: KernelEventObserverId) {
        unreachable!();
    }

    fn swap_singleton_event_observer(
        &self,
        _new: Option<KernelEventObserverId>,
    ) -> Option<KernelEventObserverId> {
        unreachable!();
    }

    fn register_raw_event_observer(
        &self,
        _kinds: KindFilter,
        _observer: Arc<dyn RawEventObserver>,
    ) -> RawEventObserverId {
        unreachable!("register_substrate does not register raw event observers");
    }

    fn unregister_raw_event_observer(&self, _id: RawEventObserverId) {
        unreachable!();
    }

    fn configured_relays_handle(&self) -> AppRelaySlot {
        unreachable!("register_substrate does not read the configured relays handle");
    }

    fn set_nostrconnect_bootstrap_relay(&self, _url: String) {
        unreachable!(
            "the bootstrap relay is wired by register_defaults_with, not register_substrate"
        );
    }

    fn register_identity_change_observer<F>(&self, _f: F)
    where
        F: Fn(Option<String>) + Send + Sync + 'static,
    {
        unreachable!("register_substrate does not register identity-change observers");
    }
}

/// A `CompiledPlan` with `n` distinct per-relay entries (the hook only reads
/// `per_relay.len()` / `keys()` and retains by key, so empty `RelayPlan`s
/// suffice).
fn plan_with_relays(n: usize) -> CompiledPlan {
    let mut plan = CompiledPlan::empty("desync-test-plan");
    for i in 0..n {
        let url = format!("wss://relay-{i}.example.test");
        plan.per_relay.insert(
            url.clone(),
            RelayPlan {
                relay_url: url,
                role_tags: Default::default(),
                sub_shapes: Vec::new(),
            },
        );
    }
    plan
}

#[test]
fn coverage_hook_honours_custom_cap_and_negentropy_runtime_is_installed() {
    const CUSTOM_CAP: usize = 7;
    assert_ne!(CUSTOM_CAP, CoverageGate::default().max_relay_connections);

    let gate = CoverageGate {
        max_relay_connections: CUSTOM_CAP,
        ..CoverageGate::default()
    };

    let mut spy = GateSpy::default();
    nmp_defaults::register_substrate(&mut spy, gate);

    // (1) The coverage hook trims to the CUSTOM cap — proves the gate reached
    //     the hook (a hardcoded `CoverageGate::default()` would trim to 30).
    let hook = spy
        .coverage_hook
        .lock()
        .unwrap()
        .clone()
        .expect("register_substrate must install a coverage hook");
    let mut plan = plan_with_relays(CUSTOM_CAP + 5);
    assert_eq!(plan.per_relay.len(), CUSTOM_CAP + 5);
    hook(&mut plan);
    assert_eq!(
        plan.per_relay.len(),
        CUSTOM_CAP,
        "the coverage hook must trim to the CUSTOM max_relay_connections, proving the \
         caller-supplied gate (not a hardcoded default) drives the hook"
    );

    // (2) The NIP-77 negentropy req-frame interceptor — constructed from the
    //     SAME single `gate` value in the SAME call — is installed. Its
    //     presence proves the gate reached the runtime collaborator too; a
    //     regression that drops the runtime leaves this `None`.
    assert!(
        spy.req_interceptor.lock().unwrap().is_some(),
        "register_substrate must install the NIP-77 negentropy req-frame interceptor \
         (the second collaborator fed by the shared gate)"
    );
    assert_eq!(
        *spy.relay_interceptors.lock().unwrap(),
        1,
        "register_substrate must install exactly one relay-text interceptor (the same \
         negentropy runtime)"
    );
}

// Silence unused-import warnings for the slot/channel imports kept for the
// `AppHost` impl signatures.
#[allow(dead_code)]
fn _assert_channel_types() {
    let (_tx, _rx): (Sender<ActorCommand>, _) = channel();
}
