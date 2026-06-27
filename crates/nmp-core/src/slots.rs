//! Shared substrate slot aliases the FFI shell (`nmp-ffi`) and the actor
//! runtime (`crate::actor`) both reach into.
//!
//! The C-ABI surface was extracted to a standalone `nmp-ffi` crate (see
//! `docs/architecture/crate-boundaries.md` §10a). The slot type aliases
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

/// V-83 — typed slot the actor publishes the kernel's `EventStore` handle into,
/// right after kernel construction (and re-publishes on `Reset`).
///
/// The `EventStore` is kernel-owned (built by `build_event_store` inside the
/// kernel constructor — it is NOT created host-side and handed down, unlike
/// [`ActiveAccountSlot`]). So this follows the [`RoutingTraceSlot`]
/// **publish-back** pattern, not the V-82 hand-down pattern: the actor (the
/// sole writer per D4) clones `Kernel::event_store_handle()` into the slot, and
/// host code reads through it synchronously via `NmpApp::event_by_id`.
///
/// `EventStore::get_by_id` is a `&self` read; the actor reducer is the only
/// writer (`EventStore::insert`, ordered before the observer fan-out — see
/// `kernel/ingest/timeline.rs`). A read from another thread therefore never
/// observes a torn write. Substrate-generic: an event id maps to a
/// [`KernelEvent`] with no NIP noun (D0 stays clean).
pub type EventStoreSlot = Arc<Mutex<Option<Arc<dyn crate::store::EventStore>>>>;

/// ADR-0058 step 3b — typed slot the actor publishes the kernel's pull-cursor
/// registry handle into, right after kernel construction (and re-publishes on
/// `Reset`).
///
/// Follows the [`EventStoreSlot`] **publish-back** pattern: the registry is
/// kernel-owned (a fresh `Arc<RwLock<PullCursorRegistry>>` is minted per kernel,
/// so `Reset` requires a re-publish). The actor is the sole writer of the inner
/// registry (the three `RegisterPullCursor` / `AdvancePullCursor` /
/// `UnregisterPullCursor` command arms); the synchronous FFI `pull_page` path
/// reads through this slot, read-locks the registry to snapshot a registration,
/// then releases before touching the store. Substrate-generic: a cursor id and a
/// `PullScope` carry no NIP noun (D0 stays clean).
pub type PullCursorRegistryHandleSlot =
    Arc<Mutex<Option<crate::kernel::pull_cursor::PullCursorRegistrySlot>>>;

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

/// Construct a fresh, empty [`EventStoreSlot`].
#[must_use]
pub fn new_event_store_slot() -> EventStoreSlot {
    Arc::new(Mutex::new(None))
}

/// Construct a fresh, empty [`PullCursorRegistryHandleSlot`].
#[must_use]
pub fn new_pull_cursor_registry_handle_slot() -> PullCursorRegistryHandleSlot {
    Arc::new(Mutex::new(None))
}

/// V-83 — synchronous event-by-id read over the kernel's published
/// [`EventStoreSlot`].
///
/// Returns the [`KernelEvent`] the store holds for `id` (a 64-char lowercase
/// hex event id), or `None` when: the slot has not been published yet
/// (pre-`nmp_app_start`), `id` is malformed, the store has no such event, or
/// the store lock / slot lock is poisoned (D6 — a missing lookup degrades
/// gracefully; it never panics across the FFI boundary).
///
/// This is the substrate-generic body behind `NmpApp::event_by_id`. The
/// mapping is lossless across the fields the substrate guarantees for every
/// protocol (`id`, `author`, `kind`, `created_at`, `tags`, `content`) — the
/// same field set `ObservedProjectionSink` sees on the ingest fan-out, so a card
/// rebuilt from a lookup is byte-identical to one rebuilt from the observer.
#[must_use]
pub fn event_by_id_from_store(
    slot: &EventStoreSlot,
    id: &str,
) -> Option<crate::substrate::KernelEvent> {
    // Event ids and pubkeys are both 32-byte values rendered as 64 lowercase
    // hex chars; `hex_to_pubkey_bytes` is the in-tree generic 64-hex → `[u8;32]`
    // decoder (`None` on malformed input). Re-aliased so this V-83 call site
    // reads honestly without renaming the V-82-shared original.
    use crate::kernel::hex_to_pubkey_bytes as hex_to_id_bytes;
    let key = hex_to_id_bytes(id)?;
    let store = slot.lock().ok()?.clone()?;
    let stored = store.get_by_id(&key).ok()??;
    let raw = &stored.raw;
    Some(crate::substrate::KernelEvent {
        id: raw.id.clone(),
        author: raw.pubkey.clone(),
        kind: raw.kind,
        created_at: raw.created_at,
        tags: raw.tags.clone(),
        content: raw.content.clone(),
        relay_provenance: relay_provenance_for_key(store.as_ref(), &key),
    })
}

/// Count the distinct follows in `author_hex`'s latest kind:3 in the kernel's
/// published [`EventStoreSlot`].
///
/// Reads the newest kind:3 for the author from the local store and counts its
/// distinct, hex-valid `p` tags. Because every locally-published event is
/// ingested into this same store before its observer fan-out (read-your-writes,
/// `kernel/ingest/timeline.rs` / ADR-0057), a kind:3 just published by a
/// `follow` / `follow_many` write is visible here synchronously — no relay
/// round-trip is required for the count to reflect it.
///
/// Returns:
/// - `Some(n)` — the author HAS a kind:3 in the store with `n` distinct follows
///   (including `Some(0)` for an explicit empty list).
/// - `None` — `author_hex` is malformed, the slot has not been published yet
///   (pre-`nmp_app_start`), a lock is poisoned, or no kind:3 exists for the
///   author yet (brand-new account that has published none). Hosts render
///   `None` as `0` for display; the distinction (`None` vs `Some(0)`) is kept
///   so a caller can tell "no list yet" from "loaded, empty".
#[must_use]
pub fn following_count_from_store(slot: &EventStoreSlot, author_hex: &str) -> Option<usize> {
    let author = crate::kernel::hex_to_pubkey_bytes(author_hex)?;
    let store = slot.lock().ok()?.clone()?;
    let mut iter = store
        .scan_by_author_kind(&author, &[3], None, None, 1)
        .ok()?;
    let stored = iter.next()?.ok()?;
    let mut seen = std::collections::HashSet::new();
    let count = stored
        .raw
        .tags
        .iter()
        .filter(|t| t.first().map(String::as_str) == Some("p"))
        .filter_map(|t| t.get(1))
        // Reuse the 64-hex decoder as the validity gate (drops malformed `p`
        // values), then dedup so a duplicated follow is not double-counted.
        .filter(|pk| crate::kernel::hex_to_pubkey_bytes(pk).is_some())
        .filter(|pk| seen.insert((*pk).clone()))
        .count();
    Some(count)
}

/// Synchronous event-by-id read over a directly-held [`Arc<dyn EventStore>`].
///
/// Use this over [`event_by_id_from_store`] when the composition root has
/// already extracted an `Arc<dyn EventStore>` handle (e.g. via
/// `KernelReducer::event_store_handle`) that must be captured into a closure
/// outliving any `RefCell` borrow. The wasm32 `EventLookup` closure pattern
/// in the wasm composition root (see #2038) is the primary caller; the native
/// `KernelReducer::event_by_id` seam also delegates here so both paths
/// share one body (no duplication per ADR-rule §4-B).
///
/// Returns `None` when `id` is not valid 64-char hex, the event is absent
/// from the store, or the store returns an error (D6 — graceful degrade).
#[must_use]
pub fn event_by_id_from_arc(
    store: &std::sync::Arc<dyn crate::store::EventStore>,
    id: &str,
) -> Option<crate::substrate::KernelEvent> {
    use crate::kernel::hex_to_pubkey_bytes as hex_to_id_bytes;
    let key = hex_to_id_bytes(id)?;
    let stored = store.get_by_id(&key).ok()??;
    let raw = &stored.raw;
    Some(crate::substrate::KernelEvent {
        id: raw.id.clone(),
        author: raw.pubkey.clone(),
        kind: raw.kind,
        created_at: raw.created_at,
        tags: raw.tags.clone(),
        content: raw.content.clone(),
        relay_provenance: relay_provenance_for_key(store.as_ref(), &key),
    })
}

pub fn relay_provenance_for_event(store: &dyn crate::store::EventStore, id: &str) -> Vec<String> {
    use crate::kernel::hex_to_pubkey_bytes as hex_to_id_bytes;
    hex_to_id_bytes(id)
        .map(|key| relay_provenance_for_key(store, &key))
        .unwrap_or_default()
}

fn relay_provenance_for_key(store: &dyn crate::store::EventStore, key: &[u8; 32]) -> Vec<String> {
    store
        .provenance_for(key)
        .map(|entries| entries.into_iter().map(|entry| entry.relay_url).collect())
        .unwrap_or_default()
}

/// Construct a fresh, empty [`RoutingSubstrateSlot`].
#[must_use]
pub fn new_routing_substrate_slot() -> RoutingSubstrateSlot {
    Arc::new(Mutex::new(None))
}

// ─── Publish-resolver factory (spec §271, 2026-05-25) ───────────────────────
//
// Per-app substrate-publish-resolver factory. Mirrors `RoutingSubstrateFactory`:
// production composition (`nmp-defaults::register_defaults`) writes a
// closure into the [`PublishResolverSlot`] via
// `NmpApp::set_publish_resolver_factory`; actor startup snapshots it and
// applies the produced `Arc<dyn OutboxResolver>` via
// `Kernel::set_publish_resolver`.
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
/// composition calls `NmpApp::set_publish_resolver_factory`; actor startup
/// snapshots it, and `Reset` re-invokes the same snapped factory against the
/// rebuilt kernel. `None` leaves the kernel's
/// `NoopOutboxResolver` default in place (every publish fails closed
/// with `NoTargets`, matching the production `Nip65OutboxResolver`'s
/// behaviour for an uncached author).
pub type PublishResolverSlot = Arc<Mutex<Option<Arc<PublishResolverFactory>>>>;

/// Construct a fresh, empty [`PublishResolverSlot`].
#[must_use]
pub fn new_publish_resolver_slot() -> PublishResolverSlot {
    Arc::new(Mutex::new(None))
}

// ─── Kernel-clock injection (test-support only) ─────────────────────────────
//
// Per-app injectable wall-clock. Production never writes this slot, so the
// kernel keeps its `SystemClock` default. The test-support FFI seam
// `NmpApp::set_kernel_clock_for_test` writes an `Arc<dyn Clock>` here; the
// actor reads it once right after kernel construction (and on `Reset`) and
// applies it via `Kernel::set_clock`. This lets end-to-end FFI tests that
// publish two replaceable events stamp strictly-increasing `created_at`
// deterministically (no wall-clock sleep — D8), exactly mirroring the existing
// in-crate `Kernel::set_clock` deterministic-replay seam.
//
// `Arc<dyn Clock>` is `Send + Sync` (the `Clock` trait requires `Sync` so the
// host thread that may advance a test clock and the actor thread that reads it
// can share one `Arc`). The slot is always compiled (a bare `Option` costs
// nothing on the production path) but only ever written by the test-support
// FFI method.
pub type KernelClockSlot = Arc<Mutex<Option<Arc<dyn crate::kernel::Clock>>>>;

/// Construct a fresh, empty [`KernelClockSlot`].
#[must_use]
pub fn new_kernel_clock_slot() -> KernelClockSlot {
    Arc::new(Mutex::new(None))
}

/// Erase a concrete [`crate::kernel::MonotonicSecondClock`] to the
/// `Arc<dyn Clock>` the [`KernelClockSlot`] stores. Test-support only: lets
/// the `nmp-ffi` `NmpApp::set_kernel_clock_for_test` seam install a deterministic
/// clock without naming the crate-private `Clock` trait directly.
#[cfg(any(test, feature = "test-support"))]
#[must_use]
pub fn erase_kernel_clock(
    clock: Arc<crate::kernel::MonotonicSecondClock>,
) -> Arc<dyn crate::kernel::Clock> {
    clock
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

// ─── Nostrconnect bootstrap relay (V-65) ───────────────────────────────────
//
// Host-supplied fallback relay for client-initiated NIP-46 `nostrconnect://`
// handshakes when the user has no configured write relay.
//
// The slot holds a single URL (`Option<String>`), mirroring `StoragePathSlot`.
// `None` (the default) means the host has not registered a bootstrap relay;
// the substrate then surfaces a typed diagnostic rather than silently using a
// hardcoded third-party URL (V-65 fix: removes `NOSTRCONNECT_DEFAULT_RELAY_URL`
// from nmp-core entirely).
//
// D0: the slot holds a plain URL string — no protocol noun in the type.
// The `nostrconnect` qualifier appears here (and in the `AppHost` method name)
// because this is a narrow, well-known seam — the same tradeoff made for
// `set_coverage_hook` and `set_routing_substrate`.
// D14: `Arc<Mutex<Option<String>>>` is NOT the banned `Arc<Mutex<Vec<…>>>` shape.

/// Typed slot for the host-supplied `nostrconnect://` bootstrap relay URL.
///
/// Written by the composition root via
/// [`AppHost::set_nostrconnect_bootstrap_relay`] before `nmp_app_start`;
/// read synchronously on the FFI thread when building the `nostrconnect://`
/// URI. `None` (the default) means no bootstrap relay is registered; the
/// substrate returns a typed error rather than falling back to any hardcoded
/// URL (V-65 / D0).
pub type NostrConnectBootstrapRelaySlot = Arc<Mutex<Option<String>>>;

/// Construct a fresh, empty [`NostrConnectBootstrapRelaySlot`].
#[must_use]
pub fn new_nostrconnect_bootstrap_relay_slot() -> NostrConnectBootstrapRelaySlot {
    Arc::new(Mutex::new(None))
}

/// Typed slot for the host-supplied `nostrconnect://` permission request.
///
/// Written by the composition root via
/// [`AppHost::set_nostrconnect_perms`](crate::substrate::AppHost::set_nostrconnect_perms)
/// before `nmp_app_start`; read synchronously on the FFI thread when building
/// the `nostrconnect://` URI. The stored `String` is the comma-joined NIP-46
/// perm list in plain (NOT percent-encoded) form, e.g.
/// `"sign_event:1,sign_event:7"` — the broker percent-encodes it when it
/// assembles the `&perms=` query parameter.
///
/// `None` (the default) means NMP supplies NO perms: the handshake omits the
/// `&perms=` parameter entirely. Which event kinds an app asks the signer to
/// sign is leaf-app product policy, not framework policy (#1493), so the
/// substrate ships no default — a protocol crate must not decide that apps only
/// need `sign_event:1,7`.
///
/// D14: `Arc<Mutex<Option<String>>>` is NOT the banned `Arc<Mutex<Vec<…>>>`
/// shape — same shape as [`NostrConnectBootstrapRelaySlot`] above.
pub type NostrConnectPermsSlot = Arc<Mutex<Option<String>>>;

/// Construct a fresh, empty [`NostrConnectPermsSlot`].
#[must_use]
pub fn new_nostrconnect_perms_slot() -> NostrConnectPermsSlot {
    Arc::new(Mutex::new(None))
}

// ─── External event sink policy factory ───────────────────────────────────────
//
// Factory slot for the single canonical external event sink policy path.
// The dispatcher uses `ExternalEventSinkPolicy` objects produced by this
// factory to route `SignedEventFrame`s to their destinations.

/// Factory closure type for the external event sink dispatcher.
///
/// Receives a [`crate::substrate::RawEventForwardPolicyContext`] and returns a
/// list of boxed [`crate::substrate::ExternalEventSinkPolicy`] objects.
pub type ExternalEventSinkPolicyFactory = dyn Fn(
        crate::substrate::RawEventForwardPolicyContext,
    ) -> Vec<Arc<dyn crate::substrate::ExternalEventSinkPolicy>>
    + Send
    + Sync;

/// Slot wrapper for [`ExternalEventSinkPolicyFactory`].
/// `None` leaves the dispatcher without any policy — no frames are forwarded.
pub type ExternalEventSinkPolicySlot = Arc<Mutex<Option<Arc<ExternalEventSinkPolicyFactory>>>>;

/// Construct a fresh, empty [`ExternalEventSinkPolicySlot`].
#[must_use]
pub fn new_external_event_sink_policy_slot() -> ExternalEventSinkPolicySlot {
    Arc::new(Mutex::new(None))
}

#[cfg(test)]
mod following_count_tests;
