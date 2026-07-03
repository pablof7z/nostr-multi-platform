//! Sub-struct definitions for [`NmpApp`] — extracted from `app_struct.rs` to
//! keep both files under the 500-LOC ceiling (AGENTS.md file-size rule).
//!
//! Three private groupings:
//! - [`CompositionConfig`] — immutable pre-start slots consumed by the actor.
//! - [`CapabilityPorts`]   — pluggable substrate handles (exactly 3 fields:
//!   `ingest_dispatcher_slot`, `search_relay_source`, `external_signer_hook`)
//!   that are live `ActorRuntimeSlots` or mutex-wrapped slots writable by
//!   post-start FFI calls.
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

/// Immutable pre-start configuration — set once before `nmp_app_start`.
/// Includes both snapshotted scalar config AND pre-start registered Arc lookup
/// handles shared with the actor.
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
    /// Flow A — relay-handshake User-Agent override (from ClientIdentity).
    pub(crate) user_agent: Arc<Mutex<Option<String>>>,
    /// Flow B — substrate-generic outbound public tag rows (NIP-89 client tag, opaque here).
    pub(crate) outbound_public_tags: Arc<Mutex<Option<Vec<Vec<String>>>>>,
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
    /// ADR-0072 — relay-connected hook slot.
    pub(crate) relay_connected_hook: nmp_core::substrate::RelayConnectedHookSlot,
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
    /// V-40 — shared [`nmp_core::substrate::DmInboxRelayLookup`] slot.
    pub(crate) dm_inbox_relays_slot: Arc<Mutex<Arc<dyn nmp_core::substrate::DmInboxRelayLookup>>>,
    /// #2788 — shared protocol-owned contact-list reader slot.
    pub(crate) contact_list_reader_slot: nmp_core::slots::ContactListReaderSlot,
    /// ADR-0070 PR 2 — shared [`nmp_core::substrate::ProfileLookup`] slot.
    pub(crate) profile_lookup_slot: Arc<Mutex<Arc<dyn nmp_core::substrate::ProfileLookup>>>,
    /// Substrate [`nmp_core::substrate::BlockedRelayLookup`] slot.
    pub(crate) blocked_relays_slot: Arc<Mutex<Arc<dyn nmp_core::substrate::BlockedRelayLookup>>>,
    /// H4 — read-only [`nmp_core::substrate::MailboxCache`] handle used by the
    /// UniFFI NIP-19 `encode_profile` identity helper.
    pub(crate) mailbox_cache_reader: Mutex<Option<Arc<dyn nmp_core::substrate::MailboxCache>>>,
    /// #1811 — shared crate-registered FTS scope registry.
    pub(crate) search_scope_registry: Arc<nmp_core::substrate::SearchScopeRegistry>,
    /// #1804 — shared crate-registered input-scope recognizer registry.
    pub(crate) input_scope_registry: Arc<nmp_core::substrate::InputScopeRegistry>,
    /// ADR-0072 §D3 — per-app bunker-URI hook slot. Belongs here (not in
    /// `CapabilityPorts`) because `nmp_signer_broker_init` guards it with
    /// `ensure_prestart_config` — it cannot be refreshed after start.
    // Feature-conditionally live: read only under the `signer-broker` feature
    // (via `install_bunker_hook`) or `test`/`test-support` (via
    // `signer_ports_test_support`). Always constructed to keep
    // `CompositionConfig` layout uniform across feature combinations.
    #[allow(dead_code)]
    pub(crate) bunker_hook: nmp_core::BunkerHookSlot,
}

// ── CapabilityPorts ───────────────────────────────────────────────────────────

/// Live substrate handles that can be refreshed after `nmp_app_start`. These
/// are `ActorRuntimeSlots` or mutex-wrapped slots writable by post-start FFI
/// calls. Exactly 3 fields: `ingest_dispatcher_slot`, `search_relay_source`,
/// `external_signer_hook`.
///
/// Note: `bunker_hook` is NOT here — it lives in [`CompositionConfig`] because
/// `nmp_signer_broker_init` guards it with `ensure_prestart_config` (pre-start
/// only).
///
/// Access path on `NmpApp`: `self.capability_ports.<field>`.
pub(crate) struct CapabilityPorts {
    /// V-40 — shared [`nmp_core::substrate::EventIngestDispatcher`] slot.
    pub(crate) ingest_dispatcher_slot:
        Arc<std::sync::RwLock<nmp_core::substrate::EventIngestDispatcher>>,
    /// NIP-50 higher-order search relay source (kind:10007 read seam +
    /// app-default fallback).
    // Feature-conditionally live: read only when the `search` feature composes
    // the NIP-50 SearchHost implementation.
    #[allow(dead_code)]
    pub(crate) search_relay_source: SearchRelaySourceSlot,
    /// ADR-0072 §D3 — per-app NIP-55 restore hook slot. Lives here (not in
    /// `CompositionConfig`) because `nmp_external_signer_init` can refresh it
    /// after start.
    // Feature-conditionally live: read only under the `external-signer` feature
    // (via `install_external_signer_hook`) or `test`/`test-support` (via
    // `signer_ports_test_support`). Always constructed to keep `CapabilityPorts`
    // layout uniform across feature combinations.
    #[allow(dead_code)]
    pub(crate) external_signer_hook: nmp_core::ExternalSignerHookSlot,
}

// ── ReadHandles ───────────────────────────────────────────────────────────────

/// Handles published back by the actor after kernel construction.
///
/// Access path on `NmpApp`: `self.read_handles.<field>`.
pub(crate) struct ReadHandles {
    /// V-83 — the kernel's `EventStore` handle, published back by the actor
    /// right after kernel construction (and re-published on `Reset`).
    pub(crate) event_store_handle: EventStoreSlot,
    /// ADR-0072 step 3b — the kernel's pull-cursor registry handle.
    pub(crate) pull_cursor_registry: PullCursorRegistryHandleSlot,
    /// V-51 phase 4 — slot the actor publishes the kernel's
    /// `RoutingTraceProjection` clone into right after kernel construction.
    pub(crate) routing_trace: RoutingTraceSlot,
    /// V-82 — the active account's raw hex pubkey slot.
    pub(crate) active_account_handle: ActiveAccountSlot,
    /// Active local `nostr::Keys` slot — substrate-generic.
    pub(crate) active_local_keys: ActiveLocalKeysSlot,
    /// Raw bech32 nsec slot handed to Marmot's credential wrapper.
    /// Only `nmp-marmot` parses this value; native runtime only owns the slot.
    #[allow(dead_code)]
    // Remove when default validation enables `marmot` or this slot is cfg-split.
    pub(crate) mls_local_nsec: MlsLocalNsecSlot,
}
