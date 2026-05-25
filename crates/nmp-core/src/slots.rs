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

/// Typed slot for the previously-installed NIP-17 DM-inbox raw-event observer id.
///
/// Used by the idempotent `NmpApp::swap_nip17_dm_inbox_observer` seam so
/// per-app crates can re-register on account-switch without stacking observers.
pub type DmInboxObserverIdSlot =
    Arc<Mutex<Option<crate::RawEventObserverId>>>;

/// Typed slot for the singleton kernel-event observer id.
///
/// Used by the idempotent `NmpApp::swap_singleton_event_observer` seam so
/// per-app crates can re-register on account-switch without stacking observers.
pub type SingletonEventObserverIdSlot =
    Arc<Mutex<Option<crate::KernelEventObserverId>>>;

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
