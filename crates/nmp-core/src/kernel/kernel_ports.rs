//! Kernel ports facade — typed newtypes for kernel capabilities.
//!
//! ADR-0072 §X — KernelPorts provides a structured newtype facade over the
//! kernel's core capability ports. Each port is a thin Arc-wrapping newtype
//! that makes the kernel's dependency injections type-safe and explicit at
//! the call site (D14).
//!
//! **Design:** This is PURE ADDITIVE — all 10 ports coexist with the existing
//! kernel fields. Callers migrate from raw `Arc<dyn X>` to typed
//! `pub struct Port(pub Arc<...>)` newtypes in slice 2+. No breaking changes
//! in this slice; all ports are re-exported from `kernel::mod.rs` in the
//! public API.
//!
//! ## Port List
//!
//! - **PublishPort**: `Arc<dyn crate::publish::PublishStore>` — durable publish state.
//! - **InterestPort**: Placeholder for interest/planner capability (future).
//! - **RelayLifecyclePort**: `Arc<dyn OutboxRouter>` — relay routing decisions.
//! - **ProtocolDispatchPort**: `Arc<RwLock<EventIngestDispatcher>>` — ingest parser registry.
//! - **IdentityPort**: `Arc<ActiveAccountSlot>` — active account state.
//! - **ReferencePort**: Reference-tracking capability (future).
//! - **PullCursorPort**: `Arc<PullCursorRegistrySlot>` — pull-cursor registry.
//! - **UiPort**: `Arc<RoutingTraceProjection>` — diagnostics projection.
//!
//! **SignerPort is intentionally absent.** The kernel stores signers per-relay
//! (not as a single kernel-wide `AuthSignerFn`). A typed `SignerPort` field
//! will be added in slice 3 once the correct per-relay-signer shape is known.

use std::sync::Arc;

use crate::publish::PublishStore;
use crate::substrate::{EventIngestDispatcher, OutboxRouter};

use super::pull_cursor::PullCursorRegistrySlot;
use super::routing_trace::RoutingTraceProjection;
use super::ActiveAccountSlot;

/// Publish-state capability port.
///
/// Holds the kernel's durable publish-store backend. Injected at composition
/// time; default is in-memory. Shared `Arc` so tests can construct multiple
/// kernels sharing the same store to prove resume-from-store semantics.
#[derive(Clone)]
pub struct PublishPort(pub Arc<dyn PublishStore>);

/// Interest/subscription capability port.
///
/// Placeholder for future planner-side subscription capability. Reserved for
/// future use in interest registration and coalescing.
#[derive(Clone)]
pub struct InterestPort {
    _phantom: std::marker::PhantomData<()>,
}

impl InterestPort {
    /// Construct an InterestPort placeholder.
    pub fn new() -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
}

impl Default for InterestPort {
    fn default() -> Self {
        Self::new()
    }
}

/// Relay-lifecycle capability port.
///
/// Holds the outbox router for relay routing decisions. Injected at composition
/// time via `Kernel::set_routing`. Default is `EmptyOutboxRouter` (every call
/// returns `Unroutable`). Production composition injects
/// `nmp_router::GenericOutboxRouter`.
#[derive(Clone)]
pub struct RelayLifecyclePort(pub Arc<dyn OutboxRouter>);

/// Protocol-dispatch capability port.
///
/// Holds the ingest parser registry behind an RwLock. Accessible for
/// registration via `NmpApp::register_ingest_parser`; the kernel reads
/// through it on every ingest wildcard arm.
#[derive(Clone)]
pub struct ProtocolDispatchPort(pub Arc<std::sync::RwLock<EventIngestDispatcher>>);

/// Identity capability port.
///
/// Holds the shared active-account slot. Synced by the kernel on every
/// identity mutation; FFI-readable via `NmpApp::active_account_handle`.
/// Survives kernel resets (the `Reset` dispatch arm rebuilds through the
/// actor-held slot).
#[derive(Clone)]
pub struct IdentityPort(pub Arc<ActiveAccountSlot>);

/// Reference-tracking capability port.
///
/// Placeholder for reference-resolution capability. Reserved for future use
/// in ADR-0070 ref-resolver expansion.
#[derive(Clone)]
pub struct ReferencePort {
    _phantom: std::marker::PhantomData<()>,
}

impl ReferencePort {
    /// Construct a ReferencePort placeholder.
    pub fn new() -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
}

impl Default for ReferencePort {
    fn default() -> Self {
        Self::new()
    }
}

/// Pull-cursor capability port.
///
/// Holds the non-durable pull-cursor registry (ADR-0072 §10, step 3a).
/// Actor-written via dispatch arms; FFI-read-only (snapshots a registration
/// on another thread). Rebuilt at restart from consumer-persisted state.
#[derive(Clone)]
pub struct PullCursorPort(pub PullCursorRegistrySlot);

/// UI diagnostics capability port.
///
/// Holds the routing-trace projection for diagnostics. Shared `Arc` between
/// the kernel and the `OutboxRouter` impl (the projection is the only
/// concrete `RoutingTraceObserver` impl). Read by the FFI surface in
/// `recent_routing_decisions` snapshot field.
#[derive(Clone)]
pub struct UiPort(pub Arc<RoutingTraceProjection>);

/// Facade grouping 9 typed capability ports for a constructed Kernel.
///
/// Returned by `Kernel::ports()`. Cloneable; each clone holds the same
/// shared Arcs as the kernel itself. Callers can store a snapshot or pass
/// it into worker threads (e.g. `ProtocolCommandContext` in a future slice).
///
/// **`signer` is absent by design.** The kernel stores signers per-relay;
/// a typed `SignerPort` will be added in slice 3 once that shape is settled.
#[derive(Clone)]
pub struct KernelPorts {
    pub publish: PublishPort,
    pub interest: InterestPort,
    pub relay_lifecycle: RelayLifecyclePort,
    pub protocol_dispatch: ProtocolDispatchPort,
    pub identity: IdentityPort,
    pub reference: ReferencePort,
    pub pull_cursor: PullCursorPort,
    pub ui: UiPort,
}
