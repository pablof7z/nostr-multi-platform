//! Kernel ports facade — typed newtypes for kernel capabilities.
//!
//! ADR-0054 §X — KernelPorts provides a structured newtype facade over the
//! kernel's core capability ports. Each port is a thin Arc-wrapping newtype
//! that makes the kernel's dependency injections type-safe and explicit at
//! the call site (D14).
//!
//! **Design:** This is PURE ADDITIVE — all 8 ports coexist with the existing
//! kernel fields. Callers migrate from raw `Arc<dyn X>` to typed
//! `pub struct Port(pub Arc<...>)` newtypes in slice 2+. No breaking changes
//! in this slice; all ports are re-exported from `kernel::mod.rs` in the
//! public API.
//!
//! ## Port List
//!
//! - **PublishPort**: `Arc<dyn crate::publish::PublishStore>` — durable publish state.
//! - **SignerPort**: `AuthSignerFn` (Arc<dyn Fn>) — signing capability.
//! - **InterestPort**: Placeholder for interest/planner capability (future).
//! - **RelayLifecyclePort**: `Arc<dyn OutboxRouter>` — relay routing decisions.
//! - **ProtocolDispatchPort**: `Arc<RwLock<EventIngestDispatcher>>` — ingest parser registry.
//! - **IdentityPort**: `Arc<ActiveAccountSlot>` — active account state.
//! - **FollowPort**: `Arc<dyn ContactsLookup>` — follow-set cache.
//! - **ReferencePort**: Reference-tracking capability (future).
//! - **PullCursorPort**: `Arc<PullCursorRegistrySlot>` — pull-cursor registry.
//! - **UiPort**: `Arc<RoutingTraceProjection>` — diagnostics projection.

use std::sync::Arc;

use crate::publish::PublishStore;
use crate::substrate::{ContactsLookup, EventIngestDispatcher, OutboxRouter};

use super::pull_cursor::PullCursorRegistrySlot;
use super::routing_trace::RoutingTraceProjection;
use super::{ActiveAccountSlot, AuthSignerFn};

/// Publish-state capability port.
///
/// Holds the kernel's durable publish-store backend. Injected at composition
/// time; default is in-memory. Shared `Arc` so tests can construct multiple
/// kernels sharing the same store to prove resume-from-store semantics.
#[derive(Clone)]
pub struct PublishPort(pub Arc<dyn PublishStore>);

/// Signing capability port.
///
/// Holds the `AuthSignerFn` callback for remote signing operations. Injected
/// at composition time. May be replaced per-identity via account switching.
/// The type itself is already `Arc<dyn Fn>`, so the port is a newtype wrapper.
#[derive(Clone)]
pub struct SignerPort(pub AuthSignerFn);

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

/// Follow-set capability port.
///
/// Holds the contact-list (kind:3) cache. Injected at composition time via
/// `Kernel::set_contacts_lookup`. Default is `EmptyContactsLookup` (every
/// lookup returns `None`). Production composition injects
/// `nmp_nip01::ContactsCache`.
#[derive(Clone)]
pub struct FollowPort(pub Arc<dyn ContactsLookup>);

/// Reference-tracking capability port.
///
/// Placeholder for reference-resolution capability. Reserved for future use
/// in ADR-0063 ref-resolver expansion.
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
/// Holds the non-durable pull-cursor registry (ADR-0058 §10, step 3a).
/// Actor-written via dispatch arms; FFI-read-only (snapshots a registration
/// on another thread). Rebuilt at restart from consumer-persisted state.
#[derive(Clone)]
pub struct PullCursorPort(pub Arc<PullCursorRegistrySlot>);

/// UI diagnostics capability port.
///
/// Holds the routing-trace projection for diagnostics. Shared `Arc` between
/// the kernel and the `OutboxRouter` impl (the projection is the only
/// concrete `RoutingTraceObserver` impl). Read by the FFI surface in
/// `recent_routing_decisions` snapshot field.
#[derive(Clone)]
pub struct UiPort(pub Arc<RoutingTraceProjection>);
