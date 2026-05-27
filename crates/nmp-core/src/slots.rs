//! Shared substrate slot aliases the FFI shell (`nmp-ffi`) and the actor
//! runtime (`crate::actor`) both reach into.
//!
//! Step 11 final of `docs/architecture/crate-boundaries.md` §5 extracted the
//! C-ABI surface to a standalone `nmp-ffi` crate. The slot type aliases
//! these two layers shared used to live in `crate::ffi::mod.rs` (private to
//! `nmp-core`); after the move the actor side cannot name them through
//! `crate::ffi::*` any more. They are substrate-grade (just shared
//! `Arc<Mutex<…>>` wrappers around already-public types), so the home that
//! satisfies both consumers is `nmp-core` itself, public.
//!
//! D14 (`crates/nmp-testing/bin/doctrine-lint/rules/d14.rs`) disciplines
//! new bare `Arc<Mutex<Vec<…>>>` shapes on `NmpApp`; the typed aliases here
//! make the slot's purpose visible at every call site so D14 continues to
//! catch shape regressions.

use std::sync::{Arc, Mutex};

use zeroize::Zeroizing;

/// Typed slot for the active account's MLS nsec (bech32, zeroized on overwrite).
///
/// The actor is the sole writer (D4); per-app crates read via
/// `NmpApp::mls_local_nsec`. Follows the same slot-alias pattern as
/// [`crate::kernel::IndexerRelaysSlot`] so D14 catches shape regressions.
pub type MlsLocalNsecSlot = Arc<Mutex<Option<Zeroizing<String>>>>;

/// Typed slot for the active account's parsed `nostr::Keys`.
///
/// Substrate-generic — the slot holds the active local-keys handle the actor
/// derives from `IdentityRuntime::active_local_keys()` on every identity
/// mutation; the substrate names no NIP. Non-substrate readers (today:
/// `nmp-nip17` for gift-wrap unsealing, `nmp-nip57` for self-zap-receipt
/// pubkey reads) consume the slot through `nmp-ffi`'s `NmpApp` accessor.
///
/// Parallel in shape to [`MlsLocalNsecSlot`] (which is the ADR-0025 raw-key
/// escape, deliberately MLS-scoped — see D13). The actor is the sole writer;
/// `None` means no account is active OR the active account uses a remote
/// signer (NIP-46 bunker) that does not expose raw `Keys`.
pub type ActiveLocalKeysSlot = Arc<Mutex<Option<nostr::Keys>>>;

/// Typed slot for the FFI-supplied LMDB storage directory path.
///
/// Written by `nmp_app_set_storage_path` before `nmp_app_start`; the actor
/// reads it once at kernel construction. `None` keeps the in-memory store.
pub type StoragePathSlot = Arc<Mutex<Option<String>>>;

/// V-51 phase 4 — typed slot the actor publishes the kernel's
/// `RoutingTraceProjection` clone into, right after kernel construction.
pub type RoutingTraceSlot =
    Arc<Mutex<Option<Arc<crate::kernel::routing_trace::RoutingTraceProjection>>>>;

/// V-51 phase 5 — per-app substrate-routing factory.
///
/// `Fn` (not `FnOnce`) so the `Reset` dispatch arm can re-invoke the
/// factory against the rebuilt kernel's fresh projection clone.
pub type RoutingSubstrateFactory = dyn Fn(
        Arc<dyn crate::substrate::RoutingTraceObserver>,
    ) -> (
        Arc<dyn crate::substrate::OutboxRouter>,
        Arc<dyn crate::substrate::MailboxCache>,
    ) + Send
    + Sync;

/// Slot wrapper for [`RoutingSubstrateFactory`]. `None` until the per-app
/// crate calls `NmpApp::set_routing_substrate`.
pub type RoutingSubstrateSlot = Arc<Mutex<Option<Arc<RoutingSubstrateFactory>>>>;

/// Construct a fresh, empty [`MlsLocalNsecSlot`].
#[must_use]
pub fn new_mls_local_nsec_slot() -> MlsLocalNsecSlot {
    Arc::new(Mutex::new(None))
}

/// Construct a fresh, empty [`ActiveLocalKeysSlot`].
#[must_use]
pub fn new_active_local_keys_slot() -> ActiveLocalKeysSlot {
    Arc::new(Mutex::new(None))
}

/// Construct a fresh, empty [`StoragePathSlot`].
#[must_use]
pub fn new_storage_path_slot() -> StoragePathSlot {
    Arc::new(Mutex::new(None))
}

/// Construct a fresh, empty [`RoutingTraceSlot`].
#[must_use]
pub fn new_routing_trace_slot() -> RoutingTraceSlot {
    Arc::new(Mutex::new(None))
}

/// Construct a fresh, empty [`RoutingSubstrateSlot`].
#[must_use]
pub fn new_routing_substrate_slot() -> RoutingSubstrateSlot {
    Arc::new(Mutex::new(None))
}

// ─── Publish-resolver factory (spec §271, 2026-05-25) ───────────────────────
//
// Per-app substrate-publish-resolver factory. Mirrors `RoutingSubstrateFactory`:
// production composition (`nmp-app-template::register_defaults`) writes a
// closure into the [`PublishResolverSlot`] via
// `NmpApp::set_publish_resolver_factory`; the actor reads it right after
// kernel construction and applies the produced `Arc<dyn OutboxResolver>`
// via `Kernel::set_publish_resolver`.
//
// The closure receives the four kernel-owned handles the router-side
// `Nip65OutboxResolver` needs (`EventStore` + indexer / local-write /
// active-account slots) so the resolver reads through the same shared
// state the kernel actor writes to. `Fn` (not `FnOnce`) so the `Reset`
// dispatch arm can re-invoke against the rebuilt kernel's fresh handles.
pub type PublishResolverFactory = dyn Fn(
        Arc<dyn crate::store::EventStore>,
        IndexerRelaysSlot,
        LocalWriteRelaysSlot,
        ActiveAccountSlot,
    ) -> Arc<dyn crate::publish::OutboxResolver>
    + Send
    + Sync;

/// Slot wrapper for [`PublishResolverFactory`]. `None` until production
/// composition calls `NmpApp::set_publish_resolver_factory`; the actor
/// then reads it after kernel construction (and on `Reset`) and applies
/// the produced resolver. `None` leaves the kernel's
/// `NoopOutboxResolver` default in place (every publish fails closed
/// with `NoTargets`, matching the production `Nip65OutboxResolver`'s
/// behaviour for an uncached author).
pub type PublishResolverSlot = Arc<Mutex<Option<Arc<PublishResolverFactory>>>>;

/// Construct a fresh, empty [`PublishResolverSlot`].
#[must_use]
pub fn new_publish_resolver_slot() -> PublishResolverSlot {
    Arc::new(Mutex::new(None))
}

// ─── Raw-event forwarding policy factory ──────────────────────────────────
//
// Per-app raw signed-event forwarding policy factory. `nmp-core` owns the
// generic dispatch seam and native pool send; reusable policy crates provide
// the target-selection policy through this slot.
pub type RawEventForwardPolicyFactory = dyn Fn(
        crate::substrate::RawEventForwardPolicyContext,
    ) -> Vec<Arc<dyn crate::substrate::RawEventForwardPolicy>>
    + Send
    + Sync;

/// Slot wrapper for [`RawEventForwardPolicyFactory`]. `None` leaves the
/// generic raw-event forwarder uninstalled.
pub type RawEventForwardPolicySlot = Arc<Mutex<Option<Arc<RawEventForwardPolicyFactory>>>>;

/// Construct a fresh, empty [`RawEventForwardPolicySlot`].
#[must_use]
pub fn new_raw_event_forward_policy_slot() -> RawEventForwardPolicySlot {
    Arc::new(Mutex::new(None))
}

/// Typed slot for the previously-installed DM-inbox observer raw-event observer id.
///
/// Used by the idempotent `NmpApp::swap_dm_inbox_observer` seam so
/// per-app crates can re-register on account-switch without stacking observers.
pub type DmInboxObserverIdSlot = Arc<Mutex<Option<crate::RawEventObserverId>>>;

/// Typed slot for the singleton kernel-event observer id.
///
/// Used by the idempotent `NmpApp::swap_singleton_event_observer` seam so
/// per-app crates can re-register on account-switch without stacking observers.
pub type SingletonEventObserverIdSlot = Arc<Mutex<Option<crate::KernelEventObserverId>>>;

/// Construct a fresh, empty [`DmInboxObserverIdSlot`].
#[must_use]
pub fn new_dm_inbox_observer_id_slot() -> DmInboxObserverIdSlot {
    Arc::new(Mutex::new(None))
}

/// Construct a fresh, empty [`SingletonEventObserverIdSlot`].
#[must_use]
pub fn new_singleton_event_observer_id_slot() -> SingletonEventObserverIdSlot {
    Arc::new(Mutex::new(None))
}

// ─── Publish-resolver slots (re-exported for `nmp-router::Nip65OutboxResolver`) ──
//
// Crate-boundary spec §271 (2026-05-25): the `Nip65OutboxResolver` lives in
// `nmp-router`, not `nmp-core`. The kernel still owns these slots (the actor
// is the sole writer per D4), but the resolver — now in a sibling crate —
// reads through them. The slot type aliases (and their constructors) are
// re-exported here so external production composition can construct a
// resolver whose handles are shared with the kernel's actor side.
//
// `RelayUrls` itself is intentionally NOT re-exported — its `replace()`
// writer is `pub(crate)`, so an external reader cannot mutate the slot.
// External callers only `lock()` + `as_slice()` to read, which is exactly
// what the resolver needs.
pub use crate::kernel::{
    new_active_account_slot, new_indexer_relays_slot, new_local_write_relays_slot,
    ActiveAccountSlot, IndexerRelaysSlot, LocalWriteRelaysSlot,
};
