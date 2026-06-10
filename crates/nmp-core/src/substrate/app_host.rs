//! App-host registration seams.
//!
//! Reusable protocol and routing crates must not depend on the native C-ABI
//! crate just to wire their modules into an application. These traits live at
//! the substrate layer so crates can register actions, parsers, observers, and
//! runtime projections against any host that implements the same Rust contract.
//! `nmp-ffi::NmpApp` is one implementation, not the type every reusable crate
//! has to name.

use std::sync::mpsc::Sender;
use std::sync::Arc;

use crate::publish::OutboxResolver;
use crate::slots::{
    ActiveAccountSlot, ActiveLocalKeysSlot, IndexerRelaysSlot, LocalWriteRelaysSlot,
};
use crate::store::EventStore;
use crate::subs::PlanCoverageHook;
use crate::update_envelope::TypedProjectionData;
use crate::{
    ActorCommand, AppRelaySlot, KernelEventObserver, KernelEventObserverId, KindFilter,
    RawEventObserver, RawEventObserverId,
};

use super::{
    ActionRegistrar, DmInboxRelayLookup, IngestParser, MailboxCache, OutboxRouter,
    RawEventForwardPolicy, RawEventForwardPolicyContext, RelayTextInterceptor, ReqFrameInterceptor,
    RoutingTraceObserver,
};

/// Host surface needed by reusable NMP composition crates.
///
/// This is intentionally a Rust trait rather than an FFI handle. Protocol
/// crates can depend on `nmp-core`, register their substrate pieces, and leave
/// the actual host implementation to `nmp-ffi` or another embedding layer.
pub trait AppHost: ActionRegistrar {
    fn register_snapshot_projection<K, F>(&self, key: K, f: F)
    where
        K: Into<String>,
        F: Fn() -> serde_json::Value + Send + Sync + 'static;

    /// Register a **typed** FlatBuffers projection closure under `key` — the
    /// typed-sidecar counterpart to [`AppHost::register_snapshot_projection`]
    /// (ADR-0037). The closure returns the projection's opaque, host-declared
    /// FlatBuffers payload ([`TypedProjectionData`]) carried verbatim in every
    /// `SnapshotFrame`'s `typed_projections` sidecar, or `None` when there is
    /// nothing to emit this tick.
    ///
    /// This method lives on the trait — not only on the concrete `NmpApp` — so
    /// reusable protocol/feed crates that register through `&impl AppHost`
    /// (e.g. `register_runtime`) can wire typed projections without depending
    /// on the C-ABI crate. It mirrors `register_snapshot_projection`: `&self`
    /// (the registry mutation is a lock-and-insert), and the same host-chosen
    /// key space shared with the generic registry (ADR-0037 Commitment 4).
    ///
    /// Like the generic closure, `f` runs on the actor thread inside the
    /// snapshot tick — it MUST be non-blocking (D8).
    fn register_typed_snapshot_projection<K, F>(&self, key: K, f: F)
    where
        K: Into<String>,
        F: Fn() -> Option<TypedProjectionData> + Send + Sync + 'static;

    /// Register a **per-tick observer** — a no-result callback fired once on
    /// every snapshot tick, the generic projection-free counterpart to
    /// [`AppHost::register_snapshot_projection`].
    ///
    /// Where a projection closure produces snapshot *data* under a key, a tick
    /// observer produces nothing: it is a pure per-tick side-effect seam for
    /// host-side reconcilers that need a "the kernel just ticked" callback but
    /// contribute no projection output. The canonical consumer is an
    /// active-account subscription reconciler that diffs the active pubkey each
    /// tick and enqueues `PushInterest` / `WithdrawInterest` actor commands —
    /// previously such reconcilers abused the projection registry by returning a
    /// `Value::Null` projection purely to obtain the per-tick callback.
    ///
    /// This method lives on the trait — not only on the concrete `NmpApp` — so
    /// reusable protocol/runtime crates that register through `&impl AppHost`
    /// (e.g. `register_zap_receipts_runtime`) can wire a per-tick reconciler
    /// without depending on the C-ABI crate. It mirrors
    /// `register_snapshot_projection`: `&self` (the registry mutation is a
    /// lock-and-push), and the same shared registry/slot.
    ///
    /// Like a projection closure, `f` runs on the actor thread inside the
    /// snapshot tick — it MUST be non-blocking (D8: enqueue only, no I/O or
    /// lock waits). A panicking observer is contained (D6) and cannot crash the
    /// tick.
    fn register_snapshot_tick_observer<F>(&self, f: F)
    where
        F: Fn() + Send + Sync + 'static;

    fn set_coverage_hook(&self, hook: PlanCoverageHook);

    fn set_req_frame_interceptor(&self, interceptor: Arc<dyn ReqFrameInterceptor>);

    fn add_relay_text_interceptor(&self, interceptor: Arc<dyn RelayTextInterceptor>);

    fn register_ingest_parser(&self, kind: u32, parser: Arc<dyn IngestParser>);

    fn set_dm_inbox_relay_lookup(&self, lookup: Arc<dyn DmInboxRelayLookup>);

    /// H4 — install the read-only [`MailboxCache`] handle the host's NIP-19
    /// identity encoder (`nmp_app_encode_profile`) reads kind:10002 relay
    /// hints from. The composition root passes the SAME `MailboxCache`
    /// instance it hands [`AppHost::set_routing_substrate`] and the
    /// kind:10002 [`IngestParser`], so the encoder can prefer `nprofile` over
    /// a bare `npub` using the hints the parser writes on ingest. Read-only,
    /// synchronous — no network, no actor round-trip.
    fn set_mailbox_cache_reader(&self, cache: Arc<dyn MailboxCache>);

    fn set_routing_substrate<F>(&self, factory: F)
    where
        F: Fn(Arc<dyn RoutingTraceObserver>) -> (Arc<dyn OutboxRouter>, Arc<dyn MailboxCache>)
            + Send
            + Sync
            + 'static;

    fn set_publish_resolver_factory<F>(&self, factory: F)
    where
        F: Fn(
                Arc<dyn EventStore>,
                IndexerRelaysSlot,
                LocalWriteRelaysSlot,
                ActiveAccountSlot,
            ) -> Arc<dyn OutboxResolver>
            + Send
            + Sync
            + 'static;

    fn set_raw_event_forward_policy_factory<F>(&self, factory: F)
    where
        F: Fn(RawEventForwardPolicyContext) -> Vec<Arc<dyn RawEventForwardPolicy>>
            + Send
            + Sync
            + 'static;

    fn active_local_keys(&self) -> ActiveLocalKeysSlot;

    fn actor_sender(&self) -> Sender<ActorCommand>;

    fn register_event_observer(
        &self,
        observer: Arc<dyn KernelEventObserver>,
    ) -> KernelEventObserverId;

    fn unregister_event_observer(&self, id: KernelEventObserverId);

    fn swap_singleton_event_observer(
        &self,
        new: Option<KernelEventObserverId>,
    ) -> Option<KernelEventObserverId>;

    fn register_raw_event_observer(
        &self,
        kinds: KindFilter,
        observer: Arc<dyn RawEventObserver>,
    ) -> RawEventObserverId;

    fn unregister_raw_event_observer(&self, id: RawEventObserverId);

    fn swap_dm_inbox_observer(&self, new: Option<RawEventObserverId>)
        -> Option<RawEventObserverId>;

    fn configured_relays_handle(&self) -> AppRelaySlot;

    /// Register the host-supplied fallback relay URL for client-initiated
    /// NIP-46 `nostrconnect://` handshakes.
    ///
    /// Must be called before `nmp_app_start`. The composition root
    /// (`nmp_app_template::register_defaults`) supplies a sane default; a
    /// per-app crate may override it. When no URL has been registered the
    /// substrate surfaces a typed error rather than silently using a hardcoded
    /// URL (V-65 / D0).
    fn set_nostrconnect_bootstrap_relay(&self, url: String);
}
