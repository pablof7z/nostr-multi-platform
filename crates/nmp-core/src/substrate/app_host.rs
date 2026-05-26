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
use crate::{
    ActorCommand, KernelEventObserver, KernelEventObserverId, KindFilter, RawEventObserver,
    RawEventObserverId, RelayEditRowsSlot,
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

    fn set_coverage_hook(&self, hook: PlanCoverageHook);

    fn set_req_frame_interceptor(&self, interceptor: Arc<dyn ReqFrameInterceptor>);

    fn add_relay_text_interceptor(&self, interceptor: Arc<dyn RelayTextInterceptor>);

    fn register_ingest_parser(&self, kind: u32, parser: Arc<dyn IngestParser>);

    fn set_dm_inbox_relay_lookup(&self, lookup: Arc<dyn DmInboxRelayLookup>);

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

    fn swap_nip17_dm_inbox_observer(
        &self,
        new: Option<RawEventObserverId>,
    ) -> Option<RawEventObserverId>;

    fn relay_edit_rows_handle(&self) -> RelayEditRowsSlot;
}
