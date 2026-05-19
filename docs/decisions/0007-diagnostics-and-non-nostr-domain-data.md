# ADR 0007: Diagnostics and non-Nostr domain data over the app bridge

**Date:** 2026-05-17
**Status:** accepted

## Context

ADR-0005 defines the platform shadow: components read from typed, domain-keyed platform caches while Rust owns source-of-truth state and emits bounded deltas. ADR-0006 narrows the first runtime proof to one profile metadata vertical slice.

Two adjacent questions must be settled before that slice grows:

1. How can an app show what is happening at the networking level: relay connection state, wire subscriptions, logical view interests, cache coverage, reconnects, EOSE/CLOSED, and relay capabilities?
2. How does non-Nostr data cross the same bridge: network monitor facts, wallet state, Blossom upload progress, sync jobs, push/decrypt status, local media imports, diagnostics, and capability results?

Both surfaces are easy to get wrong. If relay status becomes raw socket callbacks, platform code starts owning protocol policy. If every diagnostic transition crosses FFI immediately, the status UI becomes its own firehose. If non-Nostr data is forced into fake Nostr events, the event store becomes polluted with records that do not obey Nostr semantics.

## Decision

Use the same actor-owned state model and `AppUpdate` bridge for diagnostics and non-Nostr data, but keep their domain records distinct from Nostr events.

The platform receives:

- Ordinary app state through `FullState`.
- Bounded view/domain deltas through `ViewBatch`.
- Ephemeral one-shot notifications through `SideEffect`.

The platform never receives raw relay socket callbacks, raw planner callbacks, or raw capability callbacks as a parallel reactive channel. Native code may display status, but Rust owns status derivation, retry policy, subscription policy, and cache coverage policy.

## Network observability model

Networking is represented at three levels. Each level has stable identities, low-cardinality summaries, and optional debug detail.

### 1. Relay status

One record per relay URL known to the actor.

```rust
pub struct RelayStatus {
    pub relay_url: String,
    pub connection: RelayConnectionState,
    pub auth: RelayAuthState,
    pub capabilities: RelayCapabilities,
    pub active_wire_subscriptions: u32,
    pub reconnect_count: u32,
    pub last_connected_at_ms: Option<u64>,
    pub last_event_at_ms: Option<u64>,
    pub last_notice: Option<String>,
    pub last_error: Option<String>,
    pub rtt_ms: Option<u32>,
    pub bytes_rx: u64,
    pub bytes_tx: u64,
}

pub enum RelayConnectionState {
    Offline,
    Connecting,
    Connected,
    BackingOff,
    Closed,
}

pub enum RelayAuthState {
    NotRequired,
    ChallengeReceived,
    Authenticating,
    Authenticated,
    Failed,
}

pub struct RelayCapabilities {
    pub nip42_auth: CapabilityState,
    pub nip77_negentropy: CapabilityState,
}

pub enum CapabilityState {
    Unknown,
    Supported,
    Unsupported,
}
```

Relay status answers: "Is this relay reachable, what can it do, and is it healthy?"

### 2. Wire subscription status

One record per actual REQ on an actual relay. This is the planner's network-level output after logical interests have been merged.

```rust
pub struct WireSubscriptionStatus {
    pub wire_id: String,
    pub relay_url: String,
    pub canonical_filter_hash: String,
    pub state: WireSubscriptionState,
    pub logical_consumer_count: u32,
    pub opened_at_ms: u64,
    pub last_event_at_ms: Option<u64>,
    pub eose_at_ms: Option<u64>,
    pub close_reason: Option<CloseReason>,
}

pub enum WireSubscriptionState {
    Opening,
    Live,
    Eose,
    Closing,
    Closed,
    ClosedByRelay,
    Retrying,
}
```

Wire subscription status answers: "What did we actually ask the relay for?"

### 3. Logical interest status

One record per app-kernel interest: view, domain wrapper, monitor, pointer loader, sync job, or action dependency. This is the level app developers usually care about.

```rust
pub struct LogicalInterestStatus {
    pub key: InterestKey,
    pub state: LogicalInterestState,
    pub refcount: u32,
    pub view_ids: Vec<ViewId>,
    pub relay_urls: Vec<String>,
    pub cache_coverage: CacheCoverage,
    pub backfill: Option<BackfillStatus>,
    pub warming_until_ms: Option<u64>,
}

pub enum InterestKey {
    Profile { pubkey: String },
    Timeline { spec_hash: String },
    Thread { root_event_id: String },
    Action { action_id: String },
    Monitor { monitor_id: String },
    Sync { sync_id: String },
}

pub enum LogicalInterestState {
    ServingCache,
    Opening,
    Tailing,
    Backfilling,
    Complete,
    Partial,
    Degraded,
    BlockedNoRelays,
    WarmClosing,
    Closed,
}
```

Logical interest status answers: "What is this screen/component/action waiting on, and is the answer complete, partial, degraded, or local-only?"

## How status crosses the bridge

Network diagnostics are actor state. They can appear in two forms:

1. **Summary fields** in `AppState`, suitable for normal product UI:
   - online/offline/degraded.
   - number of connected relays.
   - number of active subscriptions.
   - number of running sync jobs.
   - last user-visible network error.

2. **Diagnostics views** opened explicitly by proof apps, debug UIs, and tests:
   - `ViewSpec::NetworkDiagnostics`.
   - `ViewSpec::RelayDiagnostics { relay_url }`.
   - `ViewSpec::SubscriptionDiagnostics { interest_key }`.

Diagnostics updates are coalesced separately from content views. They emit on material state transitions immediately, and otherwise at a low fixed cadence, initially 1 to 4 Hz. They do not emit once per event, once per byte counter change, or once per socket frame.

`EmitDiagnosticSnapshot` remains the release-build escape hatch for high-detail state. It writes a JSON file and returns `Effect::DiagnosticReady { path }`, rather than streaming a huge diagnostic payload through normal UI state.

## Vertical-slice implications

The ADR-0006 profile slice implements the smallest useful subset:

- One `RelayStatus` for the hardcoded relay.
- One `LogicalInterestStatus::Profile { pubkey }`.
- At most one `WireSubscriptionStatus` for the kind:0 REQ on that relay.
- A small diagnostics panel in the desktop shell may render those records.
- `firehose-bench live` for the slice records whether the Profile interest is cache-served, tailing, backfilling, warm-closing, or closed.

This validates the shape without requiring the full outbox planner, NIP-77 sync engine, or FFI.

## Non-Nostr data model

Non-Nostr data must not be encoded as fake Nostr events. It enters one of four actor-owned lanes.

| Lane | Purpose | Examples | Bridge shape |
|---|---|---|---|
| Domain store | Durable or queryable non-Nostr records owned by Rust | wallet balances, Blossom uploads, media metadata, sync reports, relay metadata | `ViewPayload` + `ViewBatch` |
| Action ledger | Status of side effects and user intents | publish attempts, uploads, downloads, NWC requests, signer prompts | `AppState` summary + action/status views |
| Capability report | Raw facts returned by native capabilities | network online/offline, keyring load result, file picker result, push token | internal actor message, then state/action update |
| Side effect | Ephemeral one-shot data that should not persist as state | toast, QR pairing URI, auth challenge, diagnostic file path | `AppUpdate::SideEffect` |

The actor decides which lane a capability result belongs to. Native reports raw facts only; Rust derives policy.

Examples:

- `NetworkMonitorCapability` reports online/offline reachability. Rust updates `NetworkState`, pauses/retries relays, and emits a coalesced status update.
- Blossom upload progress lives in the media domain store and action ledger. UI opens `useUpload(upload_id)` or `useMedia(asset_id)`.
- Wallet balance and pending payments live in wallet domain records. UI opens wallet views; payment actions transition through the ledger.
- NIP-46 pairing URI is ephemeral until accepted; it crosses as `SideEffect::BunkerPairingReady`.
- A diagnostic snapshot path is ephemeral; it crosses as `Effect::DiagnosticReady`.

## Platform shadow behavior

Generated wrappers treat diagnostic and non-Nostr views like any other domain view:

- Network summary can be a singleton key, e.g. `useNetworkStatus()`.
- Relay diagnostics key by relay URL, e.g. `useRelayStatus(relay_url)`.
- Logical interest diagnostics key by `InterestKey`.
- Wallet and media views key by wallet id, upload id, asset id, or account pubkey.

The platform shadow remains a cache. It is not durable and not source of truth. All non-Nostr records that matter across restart live in Rust storage backends under namespaced domain stores.

## Consequences

- Apps can render useful network status without becoming protocol participants.
- Debug UIs can inspect relay and subscription state without raw socket callbacks crossing FFI.
- Non-Nostr modules share the same update/reconciler/platform-shadow path as Nostr views.
- The event store remains semantically clean: Nostr events only.
- Capability bridges stay policy-free: native reports facts, Rust decides what to do.
- Diagnostics can be throttled independently from content updates.

## Alternatives considered

- **Raw callback stream for relay events.** Rejected. It creates a second reactive system, leaks protocol details into native code, and can exceed FFI budgets under firehose traffic.
- **Put full diagnostics directly in every `AppState`.** Rejected. Normal product UI needs summaries; full diagnostics are too large and too churny for routine snapshots.
- **Encode non-Nostr records as synthetic Nostr events.** Rejected. This pollutes event-store invariants and makes canonical filter/query behavior ambiguous.
- **Platform-owned domain caches for wallet/media/network.** Rejected. It duplicates policy and persistence outside Rust, violating the actor-owned state model.

## Validation

- The ADR-0006 desktop slice exposes relay status, one profile logical interest, and the backing wire subscription.
- Mounting N avatar components for the same pubkey shows one logical profile interest and one wire REQ.
- Closing all avatar components moves the logical interest to `WarmClosing`, then `Closed`, and the wire subscription to `Closed` after grace.
- Disconnecting the relay moves `RelayStatus.connection` through `BackingOff` and restores the wire REQ after reconnect.
- Diagnostics updates stay below their low-rate cadence during sustained event traffic.
- A synthetic non-Nostr module test writes domain records, action-ledger rows, capability reports, and side effects, then verifies each crosses the correct bridge lane.
