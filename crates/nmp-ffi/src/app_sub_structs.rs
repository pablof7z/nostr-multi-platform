//! Sub-struct definitions for [`NmpApp`] — extracted from `app_struct.rs` to
//! keep both files under the 500-LOC ceiling (AGENTS.md file-size rule).
//!
//! Three private groupings:
//! - [`CompositionConfig`] — immutable pre-start slots consumed by the actor.
//! - [`CapabilityPorts`]   — pluggable substrate handles shared with the actor.
//! - [`ReadHandles`]       — handles published back by the actor after kernel
//!   construction.

use std::sync::{Arc, Mutex};

use nmp_core::slots::{
    ActiveAccountSlot, ActiveLocalKeysSlot, EventStoreSlot, ExternalEventSinkPolicySlot,
    MlsLocalNsecSlot, NostrConnectBootstrapRelaySlot, NostrConnectPermsSlot, PublishResolverSlot,
    PullCursorRegistryHandleSlot, RoutingSubstrateSlot, RoutingTraceSlot, StoragePathSlot,
};
use nmp_core::subs::PlanCoverageHook;

use crate::app_struct::SearchRelaySourceSlot;

// ── CompositionConfig ─────────────────────────────────────────────────────────

/// Immutable pre-start configuration slots — set once before `nmp_app_start`,
/// consumed (snapshotted) by the actor when it starts.
///
/// Access path on `NmpApp`: `self.composition.<field>`.
pub(crate) struct CompositionConfig {
    /// FFI-supplied persistent storage directory for the LMDB `EventStore`
    /// backend.
    pub(crate) storage_path: StoragePathSlot,
    /// V-65 — host-supplied bootstrap relay URL for client-initiated NIP-46
    /// `nostrconnect://` handshakes when the user has no configured write relay.
    pub(crate) nostrconnect_bootstrap_relay: NostrConnectBootstrapRelaySlot,
    /// #1493 P9 — host-supplied NIP-46 permission request for client-initiated
    /// `nostrconnect://` handshakes.
    pub(crate) nostrconnect_perms: NostrConnectPermsSlot,
    /// Pre-start initial relay configuration.
    pub(crate) initial_relays_for_start: Mutex<Vec<(String, String)>>,
    /// D2 coverage-gate hook slot.
    pub(crate) coverage_hook: Arc<Mutex<Option<PlanCoverageHook>>>,
    /// Outbound planner REQ interceptor slot.
    pub(crate) req_frame_interceptor: nmp_core::substrate::ReqFrameInterceptorSlot,
    /// Host-installed host-op handler slot.
    pub(crate) host_op_handler: nmp_core::substrate::HostOpHandlerSlot,
    /// V-38: substrate-generic relay-text interceptor slot.
    pub(crate) relay_text_interceptor: nmp_core::substrate::RelayTextInterceptorSlot,
    /// ADR-0051 — relay-connected hook slot.
    pub(crate) relay_connected_hook: nmp_core::substrate::RelayConnectedHookSlot,
    /// ADR-0052 §D3 — per-app bunker-URI hook slot.
    pub(crate) bunker_hook: nmp_core::BunkerHookSlot,
    /// ADR-0052 §D3 — per-app NIP-55 restore hook slot.
    pub(crate) external_signer_hook: nmp_core::ExternalSignerHookSlot,
    /// Test-support kernel-clock injection slot.
    pub(crate) kernel_clock: nmp_core::slots::KernelClockSlot,
    /// External event sink policy factory slot.
    pub(crate) external_event_sink_policy: ExternalEventSinkPolicySlot,
    /// V-51 phase 5 — per-app substrate-routing factory slot.
    pub(crate) routing_substrate: RoutingSubstrateSlot,
    /// Spec §271 (2026-05-25) — per-app substrate-publish-resolver factory slot.
    pub(crate) publish_resolver: PublishResolverSlot,
    /// Per-app override for the active-account bootstrap Tailing self-kinds
    /// list. Snapshotted by the actor in `ActorConfigSources` at start; a late
    /// write after `nmp_app_start` has no effect.
    pub(crate) bootstrap_self_kinds: Arc<Mutex<Option<Vec<u64>>>>,
}

// ── CapabilityPorts ───────────────────────────────────────────────────────────

/// Pluggable substrate lookup/dispatch handles installed by composition,
/// shared with the actor.
///
/// Access path on `NmpApp`: `self.capability_ports.<field>`.
pub(crate) struct CapabilityPorts {
    /// V-40 — shared [`nmp_core::substrate::DmInboxRelayLookup`] slot.
    pub(crate) dm_inbox_relays_slot: Arc<Mutex<Arc<dyn nmp_core::substrate::DmInboxRelayLookup>>>,
    /// ADR-0057 PR 2 — shared [`nmp_core::substrate::ProfileLookup`] slot.
    pub(crate) profile_lookup_slot: Arc<Mutex<Arc<dyn nmp_core::substrate::ProfileLookup>>>,
    /// ADR-0057 PR 3 — shared [`nmp_core::substrate::ContactsLookup`] slot.
    pub(crate) contacts_lookup_slot: Arc<Mutex<Arc<dyn nmp_core::substrate::ContactsLookup>>>,
    /// Substrate [`nmp_core::substrate::BlockedRelayLookup`] slot.
    pub(crate) blocked_relays_slot: Arc<Mutex<Arc<dyn nmp_core::substrate::BlockedRelayLookup>>>,
    /// V-40 — shared [`nmp_core::substrate::EventIngestDispatcher`] slot.
    pub(crate) ingest_dispatcher_slot:
        Arc<std::sync::RwLock<nmp_core::substrate::EventIngestDispatcher>>,
    /// #1811 — shared crate-registered FTS scope registry.
    pub(crate) search_scope_registry: Arc<nmp_core::substrate::SearchScopeRegistry>,
    /// #1804 — shared crate-registered input-scope recognizer registry.
    pub(crate) input_scope_registry: Arc<nmp_core::substrate::InputScopeRegistry>,
    /// NIP-50 higher-order search relay source (kind:10007 read seam +
    /// app-default fallback).
    pub(crate) search_relay_source: SearchRelaySourceSlot,
    /// H4 — read-only [`nmp_core::substrate::MailboxCache`] handle used by the
    /// `nmp_app_encode_profile` NIP-19 identity encoder.
    pub(crate) mailbox_cache_reader: Mutex<Option<Arc<dyn nmp_core::substrate::MailboxCache>>>,
}

// ── ReadHandles ───────────────────────────────────────────────────────────────

/// Handles published back by the actor after kernel construction.
///
/// Access path on `NmpApp`: `self.read_handles.<field>`.
pub(crate) struct ReadHandles {
    /// V-83 — the kernel's `EventStore` handle, published back by the actor
    /// right after kernel construction (and re-published on `Reset`).
    pub(crate) event_store_handle: EventStoreSlot,
    /// ADR-0058 step 3b — the kernel's pull-cursor registry handle.
    pub(crate) pull_cursor_registry: PullCursorRegistryHandleSlot,
    /// V-51 phase 4 — slot the actor publishes the kernel's
    /// `RoutingTraceProjection` clone into right after kernel construction.
    pub(crate) routing_trace: RoutingTraceSlot,
    /// V-82 — the active account's raw hex pubkey slot.
    pub(crate) active_account_handle: ActiveAccountSlot,
    /// Active local `nostr::Keys` slot — substrate-generic.
    pub(crate) active_local_keys: ActiveLocalKeysSlot,
    /// Raw bech32 nsec for app crates that need local key material for MLS.
    /// ADR-0025 exception: only MLS-based app crates need the raw nsec.
    pub(crate) mls_local_nsec: MlsLocalNsecSlot,
}
