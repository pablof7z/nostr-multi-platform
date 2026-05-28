# NMP Crate Boundaries — Canonical Specification

> **Status:** decided architectural reference. Not a proposal, not a survey.
>
> **Scope:** the long-term shape of the NMP workspace's crate graph. Every crate that
> exists or is going to exist has exactly one home, one responsibility, and one set
> of dependencies. Disagreements with this document are resolved by editing this
> document, not by routing around it.
>
> **LOC ceiling exception.** The hand-authored file ceiling is 500 LOC
> (`AGENTS.md` §File Size). This document is a deliberate exception because it is
> the canonical architectural reference for the workspace. There is no per-section
> file we can split it into without violating the "single source of truth per fact"
> rule (`AGENTS.md` §Planning discipline).
>
> **Pseudocode convention.** The task brief forbids new Rust code. Where this
> document shows Rust-shaped pseudocode (trait signatures, enum shapes), those are
> illustrative — only the *shape* is normative; bodies are sketches.

---

## Status boundary

This document owns the durable crate-boundary rules. It does not own migration
status, completed-step history, active PR state, or "what is currently real"
claims. Those are temporal coordination facts and belong in `docs/plan.md`,
`docs/BACKLOG.md`, and `WIP.md` while they are live. When a migration step
finishes, this document should retain only the resulting durable boundary rule.

---

## 0. Why the current shape is wrong

The current workspace has 30 crates. `nmp-core` is the kitchen sink: ~80k LOC of
substrate (actor, kernel, AppState, planner, capability sockets) **mixed with** NIP
runtimes (NIP-17 DM send + kind:10050 ingest + dm_relay_lists cache; NIP-47 NWC
wallet runtime + dependency-inverted dep direction; NIP-57 LNURL fetcher + zap
receipt routing; a single hardwired NIP-65 outbox algorithm).

Two specific structural failures motivate this redesign:

1. **`nmp-core` depends on `nmp-nwc`.** No other NIP crate inverts the dependency
   direction. Every other NIP crate depends on `nmp-core`. This single edge proves
   the substrate has been compromised — the kernel is consuming protocol semantics.
   See V-38.

2. **The kernel hardwires one outbox routing strategy** (`kernel/outbox.rs`, 447
   LOC) that knows about kind:10002 by name. Every other event kind that needs a
   different relay set (kind:14/1059 → recipient's kind:10050; NIP-29 events →
   `h`-tag-derived group relay; Marmot MLS events → MLS group relay) either leaks
   into the kernel (V-40) or cannot be expressed at all (V-50). The fix is NOT a
   per-NIP routing-rule registry — three independent design agents converged on
   the same conclusion: NIP crates whose action-side already knows the relay
   set (NIP-17 reads its kind:10050 cache; NIP-29 reads its group state; Marmot
   reads its group relay) should pass those relays through an explicit override
   on `RoutingContext`, and the router itself stays a single generic algorithm
   (NIP-65 outbox/inbox + relay hints + p-tag inbox + indexer eligibility). A
   registry would re-introduce protocol nouns into the routing layer; the
   override seam keeps the routing layer one algorithm wide.

Together these break the "no NIP knowledge in the substrate" rule (D0) and make a
competing outbox algorithm impossible to plug in.

---

## 1. Dependency-layer diagram

Dependencies flow strictly upward. A crate at layer N may depend on any crate at
layer < N. It MUST NOT depend on any crate at layer ≥ N. Sibling siblings within
the same layer never depend on each other unless explicitly noted (Layer 0 has the
trivial `nmp-nip42-types` → nothing dependency).

> **Dependency inversion exception (Layer 3 contracts).** `nmp-core` (Layer 3)
> defines the substrate **contracts** — `OutboxRouter`, `MailboxCache`,
> `IngestParser`, `ProtocolCommand`, `ActionModule`, etc. Layer-2 crates like
> `nmp-router` **implement** those contracts and depend on `nmp-core` for the
> trait definitions, even though `nmp-router` sits below `nmp-core` in the
> conceptual layering. This is classic dependency inversion: the trait lives
> in the layer that *owns the contract* (Layer 3 substrate); the impl lives
> in the layer that *runs the algorithm* (Layer 2 routing). At runtime the
> kernel actor (Layer 3) holds the impl as `Arc<dyn OutboxRouter>` — the
> dependency the linker sees is `nmp-router → nmp-core`, but the
> dependency the kernel sees at runtime is `nmp-core → nmp-router` (via dyn
> dispatch). Both are correct; neither violates the "upward" rule because
> upward refers to the conceptual stack, not the compile-time edge direction
> for trait crates.

```
┌─────────────────────────────────────────────────────────────────────────────┐
│ Layer 6 — Bindings & deliverables (siblings, never depend on each other)    │
│                                                                             │
│   nmp-ffi          nmp-wasm         nmp-android-ffi                         │
│   (C-ABI iOS/      (wasm-bindgen,   (JNI shim re-exporting                  │
│    macOS/desktop)   browser)         nmp-ffi symbols)                       │
│                                                                             │
└───────────────────────────────▲─────────────────────────────────────────────┘
                                │
┌───────────────────────────────┴─────────────────────────────────────────────┐
│ Layer 5 — App composition                                                   │
│                                                                             │
│   nmp-app-template     (canonical NmpAppBuilder + default registrations.    │
│                         The crate `nmp init` scaffolds onto. V-48.)         │
│                                                                             │
│   apps/<app>/nmp-app-<app>  (per-app Rust crate. NOT in /crates/.           │
│                              Composes NIPs + app-specific Rust state.)      │
│                                                                             │
└───────────────────────────────▲─────────────────────────────────────────────┘
                                │
┌───────────────────────────────┴─────────────────────────────────────────────┐
│ Layer 4 — NIP crates (each complete; none leaks half its logic into core)   │
│                                                                             │
│   nmp-nip01   nmp-nip02   nmp-nip17   nmp-nip22   nmp-nip29                 │
│   nmp-nip42   nmp-nip47   nmp-nip57   nmp-nip59   nmp-nip77                 │
│   nmp-nwc     nmp-marmot  nmp-threading                                     │
│                                                                             │
│   (Each depends on Layer 0–3, never vice versa.)                            │
│                                                                             │
└───────────────────────────────▲─────────────────────────────────────────────┘
                                │
┌───────────────────────────────┴─────────────────────────────────────────────┐
│ Layer 3 — Kernel substrate (pure; zero NIP knowledge)                       │
│                                                                             │
│   nmp-core   (actor, AppState, KernelReducer, ActorCommand including the    │
│               open Protocol(Box<dyn ProtocolCommand>) seam, capability      │
│               sockets, session/account model, the OutboxRouter +            │
│               MailboxCache + EventIngestDispatcher + ActionModule trait     │
│               definitions, the SubscriptionPlanner interface,               │
│               the snapshot envelope, display helpers.)                      │
│                                                                             │
│   nmp-coverage-gate   (D2 enforcement policy data — substrate seam input.)  │
│                                                                             │
└───────────────────────────────▲─────────────────────────────────────────────┘
                                │
┌───────────────────────────────┴─────────────────────────────────────────────┐
│ Layer 2 — Routing & planning (substrate impls)                              │
│                                                                             │
│   nmp-router       (one generic OutboxRouter algorithm: NIP-65 mailbox,     │
│                     relay hints, p-tag inbox, indexer eligibility, plus     │
│                     RoutingContext::explicit_targets override seam.         │
│                     Owns InMemoryMailboxCache (kind:10002 only) and the     │
│                     nmp.nip65.publish_relay_list ActionModule. NO           │
│                     routing-rule registry; NIP crates register nothing.)    │
│                                                                             │
│   nmp-planner      (subscription compilation, interest coalescing, EOSE     │
│                     handling, per-relay filter projection — the body of     │
│                     today's nmp-core::planner)                              │
│                                                                             │
└───────────────────────────────▲─────────────────────────────────────────────┘
                                │
┌───────────────────────────────┴─────────────────────────────────────────────┐
│ Layer 1 — Storage, network, signers (leaf protocol-glue)                    │
│                                                                             │
│   nmp-store        (EventStore trait + LMDB / in-memory / IndexedDB         │
│                     backends. Today: nmp-nostr-lmdb is one backend.)        │
│                                                                             │
│   nmp-network      (raw WebSocket frame I/O + pool lifecycle. Native        │
│                     tungstenite + mio; wasm web_sys::WebSocket driver.     │
│                     Push-model PoolEvent channel, generational              │
│                     RelayHandle, per-relay reconnect token bucket, LRU      │
│                     socket-budget eviction, NIP-42 wire handshake only.    │
│                     NO routing logic, NO subscription semantics.)           │
│                                                                             │
│   nmp-signers          (Local nsec / NIP-07 / NIP-46 signer impls)          │
│   nmp-signer-broker    (NIP-46 bunker transport. Substrate seam:            │
│                         depends on nmp-network, not its own client.)        │
│                                                                             │
└───────────────────────────────▲─────────────────────────────────────────────┘
                                │
┌───────────────────────────────┴─────────────────────────────────────────────┐
│ Layer 0 — Pure protocol vocabulary (no I/O, no async, depends on nothing)   │
│                                                                             │
│   nmp-proto         (re-exports of upstream `nostr`: Event, Filter, Keys,   │
│                      Tag, NIP-19, bech32. Adds NMP-canonical type aliases.) │
│   nmp-signer-iface  (SignerError, SignerOp, Nip46Rpc, Nip46Transport)       │
│   nmp-nip42-types   (RelayAuthState + AUTH/OK frame parsers)                │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘

Sidecars (never linked into runtime; depend on the runtime stack):

   nmp-cli           — scaffolds nmp-app-template; runs nmp-codegen.
   nmp-codegen       — emits Swift Decodables / Kotlin from schemars schemas.
   nmp-testing       — mock relay, factories, simulated time.
   nmp-content       — markdown / NIP-19 rendering substrate (Layer A).
   nmp-content-fixtures — offline signed-event + DTO bundles for nmp-content.
```

The arrows go **up** (consumer at higher layer depends on producer at lower
layer). Inverted edges (most importantly `nmp-core → nmp-nwc` today) are the
violations this document closes.

`nmp-router` and `nmp-network` are two crates, not one. Routing (Layer 2,
pure algorithm + cache) and networking (Layer 1, sockets + pool lifecycle)
are different responsibilities on different layers; folding them together
would re-create exactly the "protocol-aware pool" failure mode this document
exists to prevent.

---

## 2. Per-crate table

Every crate currently in `crates/` plus every crate this document creates or
deletes. Status column key:

- ✅ — exists, in its correct final home, doing its correct job
- ⚠️ — exists, wrong location or wrong responsibility — must be moved/rewritten
- 🆕 — proposed new crate
- ❌ — proposed for deletion

### Layer 0 — pure protocol vocabulary

| Crate | Layer | Single responsibility | Owns | Does NOT own | Depends on | Status |
|---|---|---|---|---|---|---|
| `nmp-proto` | 0 | Re-export upstream rust-nostr protocol vocabulary under one NMP-canonical path. | • Re-exports of `Event`, `Filter`, `Keys`, `Tag`, `NIP-19` types, `bech32`<br>• NMP-canonical type aliases (`Pubkey = nostr::PublicKey`)<br>• Pure helper free functions that don't reach `Event` semantics | • Any I/O<br>• Any async runtime<br>• Storage<br>• Signing | upstream `nostr` only | 🆕 (today everyone re-imports `nostr` directly; centralising the re-export keeps upstream churn behind one wall) |
| `nmp-signer-iface` | 0 | Dependency-free transport interface for signers. | • `SignerError`<br>• `SignerOp`<br>• `Nip46Rpc`, `Nip46Transport` traits | • Any signer impl<br>• Any transport impl | nothing | ✅ |
| `nmp-nip42-types` | 0 | Dependency-free NIP-42 wire vocabulary so the kernel-inlined FSM and the standalone NIP-42 protocol crate cannot drift. | • `RelayAuthState` enum<br>• AUTH/OK frame shapes + parsers | • The FSM itself (lives in `nmp-core::subs::AuthGate`)<br>• The kind:22242 builder (lives in `nmp-nip42`) | nothing | ✅ — keep. Folding it into `nmp-nip42` would re-create the dep cycle (kernel needs the vocabulary; protocol crate needs the kernel for the gate) it exists to break. |

### Layer 1 — storage, transport, signers

| Crate | Layer | Single responsibility | Owns | Does NOT own | Depends on | Status |
|---|---|---|---|---|---|---|
| `nmp-store` | 1 | The EventStore trait + every NMP-shipped backend behind one switchable surface. | • `trait EventStore` (insert/get/range/delete/replaceable semantics)<br>• `InMemoryStore` backend<br>• `LmdbStore` backend (wraps today's `nmp-nostr-lmdb`)<br>• `IndexedDbStore` backend (Stage F-01) | • Any routing decision<br>• Any subscription tracking<br>• Replaceable-event policy (lives in the trait contract — backends enforce it on insert) | `nmp-proto` | 🆕 (consolidation: `nmp-nostr-lmdb` becomes a backend of this) |
| `nmp-nostr-lmdb` | 1 | LMDB backend implementation of `EventStore`. | • LMDB on-disk format<br>• `heed` env management<br>• Env-injection seam (ADR-0011) | • The trait itself | `nmp-store`, `nmp-proto` | ✅ (responsibility unchanged; becomes a backend behind the unified `nmp-store` trait) |
| `nmp-network` | 1 | Raw WebSocket frame I/O + per-URL pool lifecycle. Nothing else. | • Native worker (tungstenite + mio, today in `nmp-core::relay_worker`)<br>• Browser driver (today in `nmp-wasm::BrowserRelayDriver`)<br>• Per-URL connection state machine (connecting → connected → closing → reconnecting)<br>• Exponential backoff + full jitter, bounded by per-relay token bucket for storm protection<br>• Socket budget (max N concurrent connections, LRU eviction)<br>• Per-relay outbound queue (in-order delivery across reconnects)<br>• Per-relay `RelayHealth` (latency, error counts)<br>• HTTP 401/403/429/503 denial classification<br>• Keepalive ping/pong<br>• Push-model `PoolEvent` channel (`Opened`/`Frame`/`Closed`/`Failed`/`Health`)<br>• Generational `RelayHandle` (URL + open-count)<br>• `Pool::ensure_open` / `Pool::send` / `Pool::close` / `Pool::shutdown` / `Pool::health` / `Pool::snapshot`<br>• Raw `RelayFrame` surfacing (`Event`, `Eose`, `Notice`, `Ok`, `Auth` — zero semantic interpretation)<br>• NIP-42 AUTH **wire handshake only** (sends/receives the frame; does NOT compute kind:22242, does NOT pause/replay subscriptions) | • Routing decisions<br>• Subscription ID semantics<br>• EOSE handling<br>• EVENT deduplication<br>• Relay selection<br>• "Send to all connected relays" — no such method exists; all sends are constrained to a `RelayHandle` (the structural answer to NDK issue #175)<br>• The kind:22242 AUTH event builder (lives in `nmp-nip42`)<br>• The pause/replay FSM (lives in the planner's `AuthGate`) | `nmp-proto` | 🆕 (consolidation: extract `nmp-core::relay_worker` + `relay_protocol` + `nmp-wasm::relay_pool::BrowserRelayDriver` + pool lifecycle to here.) |
| `nmp-signers` | 1 | Concrete signer implementations + multi-account `AccountManager`. | • Local-nsec signer<br>• NIP-07 browser signer<br>• NIP-46 remote signer<br>• `AccountManager` | • NIP-46 wire protocol (lives in `nmp-signer-broker`)<br>• Signer traits (live in `nmp-signer-iface`) | `nmp-signer-iface`, `nmp-proto` | ✅ |
| `nmp-signer-broker` | 1 | NIP-46 bunker handshake + per-relay transport multiplexing. | • Bunker URI parse / handshake<br>• NIP-46 RPC fan-out across the broker's relays<br>• `BunkerConnectionState` projection (V-14 Stage 1) | • Its own WebSocket loop (DEPENDS on `nmp-network`'s `Pool` primitive — V-13 Stage 2 deduplicates) | `nmp-signer-iface`, `nmp-signers`, `nmp-network`, `nmp-proto` | ⚠️ (today carries its own polling tungstenite client — V-13. Post-refactor: ride `nmp-network`'s shared primitive. Decision: **`nmp-signer-broker` depends on `nmp-network`**, not its own socket. One readiness-driven WebSocket implementation in the workspace, period.) |

### Layer 2 — routing & planning (substrate impls)

| Crate | Layer | Single responsibility | Owns | Does NOT own | Depends on | Status |
|---|---|---|---|---|---|---|
| `nmp-router` | 2 | One generic outbox routing algorithm + NIP-65 mailbox cache + NIP-65 `publish_relay_list` action + NIP-65 publish-side `Nip65OutboxResolver` + indexer-republish policy. | • The single `OutboxRouter` impl (generic algorithm: consults `evt.kind` for indexer eligibility, `evt.pubkey` for the author's NIP-65 write set, `evt.tags` for relay hints and p-tag recipient inbox, `ctx.session_keys` for AppRelay/Indexer/UserConfigured lanes, `ctx.mailbox_cache` for NIP-65 lookups, `ctx.blocked_relays` for the post-filter)<br>• `RoutingContext::explicit_targets` override seam — when populated by a NIP crate's action, the generic algorithm is skipped and the override URLs are returned attributed to the `ClassRouted` lane (minus blocked-relay post-filter hits)<br>• `MailboxCache` impl (`InMemoryMailboxCache`) — kind:10002 only<br>• The kind:10002 ingest parser (writes into `MailboxCache` via `EventIngestDispatcher`)<br>• The seven-lane `RoutingSource` resolver<br>• `selectOptimalRelays` (greedy coverage with per-user cap)<br>• Blocked-relay (kind:10006) post-filter<br>• The `nmp.nip65.publish_relay_list` `ActionModule` (absorbed from `nmp-nip65`)<br>• The publish-side `Nip65OutboxResolver` (`nmp_core::publish::OutboxResolver` impl that reads kind:10002 write-relays from an `EventStore` + active-account local-write fallback + recipient-inbox fanout; spec §271, moved out of `nmp_core::publish::nip65` 2026-05-25)<br>• The `IndexerRepublishPolicy` (`nmp_core::substrate::RawEventForwardPolicy` impl that selects indexer targets for accepted replaceable events, with source/provenance loop prevention and bounded dedup) | • The `OutboxRouter` *trait* (lives in `nmp-core` substrate)<br>• The `OutboxResolver` *trait* (lives in `nmp-core::publish`)<br>• The `RawEventForwardPolicy` *trait* and pool dispatch (live in `nmp-core` substrate)<br>• Any NIP-specific routing logic — there is no per-NIP routing-rule registry; NIP crates register nothing with the router<br>• Pool lifecycle, sockets, reconnect (lives in `nmp-network`)<br>• The kind:10050 DM-inbox cache (lives in `nmp-nip17`; the router never sees kind:10050)<br>• Per-relay filter projection (lives in `nmp-planner::project_per_relay`)<br>• Any pool reference — the router is pure CPU + lookup; the kernel actor is the only object that holds both a router and a pool handle | `nmp-core`, `nmp-network`, `nmp-proto` | 🆕 (replaces today's `crates/nmp-nip65` + extracts `nmp-core::kernel::outbox.rs` + `InMemoryMailboxCache`.) |
| `nmp-planner` | 2 | Subscription compilation + EOSE/coalescing/auto-close + per-relay filter projection. | • `InterestRegistry`, `LogicalInterest`, `CompiledPlan`<br>• Per-relay filter projection (`project_per_relay`): given a `LogicalInterest{authors:[A,B,C], kinds:[1]}` and a `RoutedRelaySet` from the router (which maps each author to their write relays), produces a per-relay filter where each relay's `authors` field is restricted to the subset that writes to it. This is the only thing meant by "per-relay filter execution strategy" — there is no per-relay `since`/cursor customization at this layer (novel, orthogonal to routing; would belong in `nmp-store` if ever added, separate ADR).<br>• Coverage-maximising greedy `select_optimal`<br>• Per-author NIP-65 union, app-relay fallback, partition cases<br>• Buffered ≤60 Hz publish to the kernel | • Routing dispatch (the *who*-decides-which-relay-set part lives in `nmp-router`)<br>• Replaceable-event semantics | `nmp-core`, `nmp-router`, `nmp-proto` | 🆕 (extract today's `nmp-core::planner` — it's already a coherent module; the extraction makes the "the kernel doesn't know the planner's implementation, only its interface" rule structural rather than aspirational) |

### Layer 3 — kernel substrate

| Crate | Layer | Single responsibility | Owns | Does NOT own | Depends on | Status |
|---|---|---|---|---|---|---|
| `nmp-core` | 3 | The pure substrate every NMP app composes onto. Zero NIP knowledge. | • Actor (single OS thread, flume channel)<br>• `AppState`, `KernelUpdate`, `KernelReducer`, `rev` monotonicity<br>• `ActorCommand` enum **including the open `Protocol(Box<dyn ProtocolCommand>)` variant** (§4)<br>• Capability sockets (keychain, push, network monitor)<br>• Session / account model (`AccountManager` integration; switch_active)<br>• `EventStore` interface (consumes `nmp-store`)<br>• `OutboxRouter` + `MailboxCache` traits<br>• `EventIngestDispatcher` (input-side projection seam, §4)<br>• `RawEventForwardPolicy` trait + generic raw-event forwarding dispatch<br>• `SubscriptionPlanner` interface (consumes `nmp-planner`)<br>• `ActionModule` trait + registry<br>• `KernelEventObserver` / `RawEventObserver` registries<br>• Snapshot envelope + `KernelReducer`<br>• Canonical FlatBuffers update-frame schema under `crates/nmp-core/schema/` and Rust encode/decode helpers<br>• `display::` cross-surface formatting helpers<br>• `coverage_hook` seam | • Any NIP-specific parser, builder, or routing rule<br>• Wallet runtime, DM send, LNURL fetcher, NWC client<br>• NIP-65 outbox algorithm body<br>• kind:10050 / kind:10002 / kind:30023 specific ingest paths<br>• Indexer-republish target-selection policy<br>• Any `Wallet*` / `Dm*` / `Zap*` `ActorCommand` variant | `nmp-store`, `nmp-network`, `nmp-signer-iface`, `nmp-nip42-types`, `nmp-proto` | ⚠️ (today carries Layer 4 work that must move out per V-38/V-39/V-40/V-41/V-50 — see §5 migration order. Once those land, `nmp-core` becomes a coherent substrate.) |
| `nmp-coverage-gate` | 3 | D2 negentropy-before-REQ policy data (thresholds, back-off rules). | • Threshold constants<br>• Back-off policy data | • Any kernel-side hook installation | `nmp-proto` | ✅ |

### Layer 4 — NIP crates (each complete)

Each NIP crate is the **single home** for its NIP. Wire builders, ingest parsers,
projections, `ActionModule`s, `ProtocolCommand`s, and routing-rule registrations
all live together. A NIP crate cannot leak into Layer 3.

| Crate | Layer | Single responsibility | Owns | Does NOT own | Depends on | Status |
|---|---|---|---|---|---|---|
| `nmp-nip01` | 4 | NIP-01 short text notes (kind:1) — decoder, builder, relations, view. | • `NoteRecord` + NIP-10 ref decoder<br>• `NoteBuilder`<br>• `RepliesView`, `ThreadView`<br>• Kernel-owned canonical timeline projection (so apps don't sort notes in Swift — V-37c / V-45) | • Reactions (kind:7 lives in `nmp-nip02`)<br>• Threading algorithm (lives in `nmp-threading`) | `nmp-core`, `nmp-threading`, `nmp-proto` | ✅ (extend with the canonical follow-set timeline projection to close V-37c / V-45) |
| `nmp-nip02` | 4 | NIP-02 follow lists (kind:3) + NIP-25 reactions (kind:7) as ActionModules. | • Follow / Unfollow / React `ActionModule`s<br>• `FollowListProjection`<br>• The `LogicalInterest::FollowSetKind1` registration that makes "show me notes from people I follow" a single-line affordance (V-45) | • Anything else | `nmp-core`, `nmp-proto` | ✅ |
| `nmp-nip17` | 4 | NIP-17 private DMs end-to-end (build, gift-wrap orchestrate, send, ingest, project, route). | • `build_dm_rumor`<br>• DM send handler (the body of today's `nmp-core::actor::commands::dm.rs`, dispatched via `ProtocolCommand` — V-39)<br>• `DmInboxProjection`<br>• `DmRelayCache` (a simple `HashMap<Pubkey, Vec<RelayUrl>>` — owned by this crate, NOT by the router; the router's `MailboxCache` is NIP-65 kind:10002 only)<br>• kind:10050 ingest parser registered via `EventIngestDispatcher` (the router never sees kind:10050)<br>• The DM send action reads `DmRelayCache` for the recipient's write relays and populates `RoutingContext::explicit_targets` so the generic router returns exactly those URLs without consulting any NIP-17-specific rule<br>• `nmp.nip17.publish_relay_list` ActionModule | • Gift-wrap primitives (in `nmp-nip59`)<br>• The router's `MailboxCache` (that is NIP-65 only)<br>• Any kernel state | `nmp-core`, `nmp-nip59`, `nmp-router`, `nmp-proto` | ⚠️ (today the send handler + kind:10050 cache + ingest live in `nmp-core` — V-39 / V-40) |
| `nmp-nip22` | 4 | NIP-22 generic comments (kind:1111). | • kind:1111 decoder + builder<br>• `ActionModule` to publish a comment | • NIP-29 (kind:1111 is not a NIP-29 group event; tagging it with `h` is a use-the-h-tag-on-any-event case the **router** handles, not a NIP-29 concern) | `nmp-core`, `nmp-proto` | 🆕 — **decision: NIP-22 is its own crate**. The substrate does not handle kind:1111 generically because NIP-22 has its own semantic shape (root + parent threading); leaving it in "the store handles it" would defer the missing semantics rather than place them. |
| `nmp-nip29` | 4 | NIP-29 relay-based groups — the kinds that ONLY make sense as group ownership (9/10/11/12, 9000-9022, 39000-39003). | • Decoders / builders / `ActionModule`s for the NIP-29-owned kinds only<br>• `DiscoveredGroupsProjection`, `GroupChatProjection`, `MemberListProjection`<br>• Group state owning the host-relay URL per group<br>• Every `nmp-nip29` action that publishes (whether an NIP-29-owned kind, or a kind:1 / kind:7 / kind:1111 carrying an `h` tag) populates `RoutingContext::explicit_targets` with the group's host relay before dispatch. NIP-29 registers no routing logic with the router; the router stays a single generic algorithm. | • kind:1111 (lives in `nmp-nip22`)<br>• Marmot/MLS (lives in `nmp-marmot`) | `nmp-core`, `nmp-router`, `nmp-proto` | ✅ (responsibility clarified: the `h`-tag is an event-level signal any kind can carry; only the NIP-29-owned kinds belong here as semantics; routing happens via explicit-target override populated by the action, not by a registered rule) |
| `nmp-nip42` | 4 | NIP-42 relay AUTH protocol crate. | • kind:22242 builder<br>• Per-relay handshake driver<br>• The `AuthGate` install hook the kernel calls into | • Wire vocabulary (`nmp-nip42-types`)<br>• The wire-frame pause/flush FSM (lives in `nmp-core::subs::AuthGate`) | `nmp-core`, `nmp-nip42-types`, `nmp-proto` | ✅ |
| `nmp-nip47` | 4 | NIP-47 NWC wallet runtime, end-to-end. | • `WalletRuntime`, `WalletConnection`, `WalletStatus`<br>• kind:23194 builder + kind:23195 response decoder<br>• NWC URI parse, NIP-04 encrypt bridge<br>• `WalletConnect` / `WalletDisconnect` / `WalletPayInvoice` `ProtocolCommand`s (V-38)<br>• `nmp.wallet.pay_invoice` `ActionModule` (already exists, currently in `nmp-core::wallet`) | • The NWC protocol crate (lives in `nmp-nwc`) | `nmp-core`, `nmp-nwc`, `nmp-proto` | 🆕 (today this whole runtime is inside `nmp-core::actor::commands::wallet.rs` + `nmp-core::wallet` — the V-38 inversion. After migration: `nmp-nip47 → nmp-core` and `nmp-nip47 → nmp-nwc`. `nmp-core → nmp-nwc` edge is deleted.) |
| `nmp-nwc` | 4 (within `nmp-nip47`) | NIP-47 protocol primitives (no actor, no FFI). | • NWC URI parse<br>• NIP-44 encrypted request/response<br>• kind:23194/23195 codecs | • The runtime that orchestrates them (lives in `nmp-nip47`) | `nmp-proto` | ⚠️ → ✅ (the *crate* is fine; the *dep direction* `nmp-core → nmp-nwc` is the violation. After V-38, only `nmp-nip47` depends on `nmp-nwc`.) |
| `nmp-nip57` | 4 | NIP-57 lightning zaps — request + receipt + LNURL fetcher, all in one crate. | • `ZapRequestBuilder` (kind:9734) + `ZapReceiptRecord` decoder (kind:9735)<br>• `ZapsAggregateProjection`<br>• `ZapAction` `ActionModule`<br>• The LNURL-pay round-trip handler (the body of today's `nmp-core::actor::commands::zap.rs` + `zap_lnurl.rs`, dispatched via `ProtocolCommand` — V-41)<br>• kind:9735 `#p <viewer>` `LogicalInterest` registration | • Wallet payment (lives in `nmp-nip47`; the zap-pay chain is a multi-step `dispatch_action` contract — V-43) | `nmp-core`, `nmp-nip47`, `nmp-proto` | ⚠️ (LNURL fetcher in `nmp-core` today — V-41) |
| `nmp-nip59` | 4 | NIP-59 gift-wrap / seal / rumor primitives. | • `gift_wrap`, `unwrap_gift_wrap` free functions<br>• `gift_wrap_with_signer` | • Anything else; substrate-grade per its own docs | `nmp-proto`; **MAY** be depended on by `nmp-core` (gift-wrap is a substrate-grade NIP — the kernel uses it to seal DM rumors on the actor thread without owning DM semantics) | ✅ (the one NIP crate that legitimately sits below the substrate's policy boundary because it carries no NIP-17 / Marmot nouns — only the wrap primitive) |
| `nmp-nip77` | 4 | NIP-77 negentropy sync. | • Negentropy reconciler<br>• Sync action surface | • The coverage gate policy (`nmp-coverage-gate`) | `nmp-core`, `nmp-proto` | 🆕 (today implicit / partial under various names; promote to a discrete NIP crate) |
| `nmp-threading` | 4 | Reply-convention-agnostic timeline grouping algorithm. | • `ThreadPointer`, `ParentResolver`, `ModulePolicy`, `TimelineBlock`, `Grouper` | • Any kind semantics<br>• Any app nouns | `nmp-core` | ✅ (sibling to NIP crates — it's a generic algorithm consumed by them; arguably its layer is "between" 3 and 4 but with no NIP knowledge it is correctly modeled as a Layer-4 substrate sibling consumed by `nmp-nip01`) |
| `nmp-wot` | 4 | Client-side Web-of-Trust scoring over local follow and public mute-list signals. | • In-memory follow/mute signal graph<br>• Bounded personalized trust scoring<br>• Author ranking and hidden-author helpers for app projections | • Relay crawling / fetching<br>• NIP-85 trusted assertions or delegated provider scores<br>• Native-side filtering policy | `serde` | ✅ |
| `nmp-marmot` | 4 | Marmot/MLS encrypted groups end-to-end. | • MLS group lifecycle + welcome handling<br>• Group state owning the MLS group relay URL per group<br>• Every `nmp.marmot.*` publish action populates `RoutingContext::explicit_targets` with the group's MLS relay before dispatch. Marmot registers no routing logic with the router.<br>• `nmp.marmot.*` `ActionModule`s<br>• Group-member projection (V-17)<br>• The kernel-shaped `nmp_marmot_*` per-app FFI cluster (observer registration, opaque handle, snapshot read, group-messages read, key-package fetch enqueue, unregister) — same structural pattern as Chirp's `nmp_app_chirp_*` cluster. | • Anything else (no business logic in Swift) | `nmp-core`, `nmp-ffi`, `nmp-nip59` | ✅ (lives at `crates/nmp-marmot/` after step 12, 2026-05-25). The ADR-0025 bespoke write-side `nmp_marmot_dispatch` C symbol was deleted in PR 3 (2026-05-23) — mutating ops now flow through the generic `nmp_app_dispatch_action("nmp.marmot", action_json)` seam. The surviving FFI cluster is explicitly NOT a `dispatch_action` violation: those symbols are kernel-shaped observer / projection / opaque-handle lifecycle, the same per-app FFI pattern any Layer-4 NIP crate may expose when its runtime needs a stateful handle. Compare to Chirp's `nmp_app_chirp_*` cluster. |

### Layer 5 — app composition

| Crate | Layer | Single responsibility | Owns | Does NOT own | Depends on | Status |
|---|---|---|---|---|---|---|
| `nmp-app-template` | 5 | Canonical `NmpAppBuilder` + default NIP registrations. The crate `nmp init` scaffolds onto. | • `NmpAppBuilder` (composition root)<br>• Default action registrations (NIP-01 publish, NIP-02 follow, NIP-17 send, NIP-57 zap, NIP-65 publish_relay_list)<br>• Default ingest registrations (kind:10002 → `MailboxCache` via `nmp-router`; kind:10050 → `DmRelayCache` via `nmp-nip17`)<br>• Default `LogicalInterest::SocialTimeline` wiring<br>• Default coverage hook installation | • Any app-specific logic | All Layer 4 NIP crates the canonical app needs, `nmp-core`, `nmp-router`, `nmp-planner` | 🆕 (V-48). Closes the "second-app developer must read 403 LOC of Chirp to learn registration" gap. |
| `apps/<app>/nmp-app-<app>` | 5 | Per-app composition + app-specific Rust state. | • This app's `NmpAppBuilder` invocation<br>• App-specific projections, actions, and Rust state not generalizable to other Nostr apps (per AGENTS.md §What belongs in NMP crates vs. app-specific Rust crates) | • Anything generalizable (lives in NMP crates) | `nmp-app-template`, selected NIP crates | ✅ — app crates live in `apps/`, NOT in `crates/` |

### Layer 6 — bindings (siblings; never depend on each other)

| Crate | Layer | Single responsibility | Owns | Does NOT own | Depends on | Status |
|---|---|---|---|---|---|---|
| `nmp-ffi` | 6 | C-ABI surface for iOS / macOS / desktop / Android JNI shim. | • `nmp_app_*` `extern "C"` symbols<br>• `NmpApp` opaque handle<br>• catch_unwind guard wrapping<br>• Per-app generic snapshot pull path `nmp_app_get_snapshot(app, namespace)` (V-37b) | • Any business logic<br>• Any per-app symbols (those live in `apps/<app>/nmp-app-<app>`'s own thin shell) | `nmp-app-template` (or the specific app crate it's linking), `nmp-core` | 🆕 (consolidation: extract today's `nmp-core::ffi` module to its own crate so substrate cannot accidentally grow C-ABI). UniFFI migration deferred to M14 per ADR-0030. |
| `nmp-wasm` | 6 | wasm-bindgen surface for browser. | • `NmpWasmRuntime`<br>• Snapshot push callback<br>• JS `dispatch_app_action_async` Promise wrapper<br>• NIP-07 signer bridge (today `nmp_signers::nip07::wasm`)<br>• IndexedDB driver (F-01) | • The kernel itself (consumes `KernelReducer`)<br>• The browser relay driver (consumed from `nmp-network`)<br>• Any C-ABI | `nmp-app-template`, `nmp-core`, `nmp-store` (IndexedDB backend), `nmp-network` (browser driver) | ✅ (responsibility unchanged; network extraction makes it correctly thin) |
| `nmp-android-ffi` | 6 | JNI shim re-exporting `nmp-ffi` symbols through the rlib. | • `extern "C"` re-export declarations only | • Any logic | `nmp-ffi`, `nmp-core` (via `android-ffi` feature) | ✅ |

### Sidecars (never linked into the runtime stack)

| Crate | Role | Status |
|---|---|---|
| `nmp-cli` | Developer CLI: `nmp init <app>` scaffolds onto `nmp-app-template`; `nmp gen modules` invokes `nmp-codegen`. | ✅ (extend with the `init` recipes — V-48 follow-up) |
| `nmp-codegen` | Emits Swift `Decodable` (and later Kotlin) bindings from `schemars::JsonSchema` derives. | ✅ |
| `nmp-testing` | Mock relay, factories, simulated time, fixture helpers. | ✅ |
| `nmp-content` | Layer A content-rendering substrate — tokenizer, embed claim registry, recursion guard. | ✅ |
| `nmp-content-fixtures` | Offline signed-event + DTO bundles for `nmp-content`. | ✅ |
| `nmp-repl` | Diagnostic REPL for the planner + outbox. | ✅ |
| `nmp-chirp-config` | Shared Chirp app configuration object. | ⚠️ — belongs in `apps/chirp/` (Chirp-specific), not in `crates/`. Move alongside V-02's nmp-marmot precedent. |
| `chirp-repl`, `chirp-tui` | Chirp diagnostic shells. | ⚠️ — same — move to `apps/chirp/` per AGENTS.md §What belongs in NMP crates. |
| `fixture-todo-core` | Per-app fixture state. | ✅ — lives at `apps/fixture/fixture-todo-core/`. |

---

## 3. The routing split — `OutboxRouter` + `MailboxCache` + `explicit_targets`

This is the single most important design decision in this document. The current
state — one hardwired NIP-65 algorithm in 447 LOC of `nmp-core::kernel::outbox.rs`
— is V-50. The replacement keeps routing as **one generic algorithm** (NIP-65
write-set + relay hints + p-tag inbox + indexer eligibility + AppRelay fallback)
and adds **one override seam** (`RoutingContext::explicit_targets`) for the NIP
crates whose actions already know the relay set (NIP-17 DM, NIP-29 group post,
Marmot MLS publish). Three independent design agents converged on this shape:
a per-NIP routing-rule registry would re-introduce protocol nouns into the
routing layer; the explicit-target override keeps the routing layer one
algorithm wide.

### 3.0 The three-layer pipeline (canonical data flow)

```
┌─────────────────────────────────────────────────────────────────────────────┐
│ Step 1 — nmp-router                                                         │
│   Input:  LogicalInterest | UnsignedEvent  +  RoutingContext                │
│   Output: RoutedRelaySet                                                    │
│           (which relays + which authors subset writes to each relay)        │
│                                                                             │
│   If ctx.explicit_targets is Some(&[…]), return those URLs directly         │
│   (minus blocked-relay post-filter hits). Otherwise run the generic         │
│   algorithm: kind → indexer eligibility; pubkey → NIP-65 write set;         │
│   tags → relay hints + p-tag recipient inbox; session_keys → AppRelay /     │
│   Indexer / UserConfigured lanes; mailbox_cache → NIP-65 lookups;           │
│   blocked_relays → subtractive post-pass.                                   │
└──────────────────────────────────▼──────────────────────────────────────────┘
┌─────────────────────────────────────────────────────────────────────────────┐
│ Step 2 — nmp-planner::project_per_relay                                     │
│   Input:  LogicalInterest  +  RoutedRelaySet                                │
│   Output: Vec<(RelayUrl, Filter, SubId)>                                    │
│           (per-relay filters with `authors` partitioned to the subset       │
│            that actually writes to each relay; this is what "per-relay      │
│            filter execution strategy" means.)                               │
└──────────────────────────────────▼──────────────────────────────────────────┘
┌─────────────────────────────────────────────────────────────────────────────┐
│ Step 3 — Kernel actor (the only object that holds both handles)             │
│   For each (url, filter, sub_id):                                           │
│     let h = pool.ensure_open(&url);                                         │
│     pool.send(h, WireFrame::Req { sub_id, filter_json });                   │
│                                                                             │
│   The router does NOT hold a pool reference. The planner does NOT hold a    │
│   pool reference. The actor is the orchestrator.                            │
└──────────────────────────────────▼──────────────────────────────────────────┘
┌─────────────────────────────────────────────────────────────────────────────┐
│ Step 4 — nmp-network                                                        │
│   Executes WebSocket I/O. Surfaces inbound frames via the push-model        │
│   PoolEvent channel back to the actor: Opened / Frame / Closed / Failed /   │
│   Health. The pool has NO "send to all" method; all sends are constrained   │
│   to a RelayHandle the actor obtained from ensure_open.                     │
└─────────────────────────────────────────────────────────────────────────────┘
```

The router is pure CPU + lookup. The planner is pure CPU + projection. The
pool is pure I/O + lifecycle. The actor is the only object that crosses
between them.

### 3.1 The seven routing lanes (from the relay synthesis doc)

> **Note on count:** the design-agent brief mentioned "6 lanes." The primary
> source — `docs/research/SYNTHESIS-app-relays.md` §5 — enumerates **7**. This
> document tracks the synthesis doc.

The substrate enum the router attributes each relay URL to (either from the
generic algorithm or from the `explicit_targets` override, which always
attributes to `ClassRouted`):

```rust
pub enum RoutingSource {
    /// Lane 1 — per-author NIP-65 outbox/inbox (kind:10002).
    Nip65 { direction: Direction },
    /// Lane 2 — relay hint from event tag.
    Hint,
    /// Lane 3 — provenance from a prior event.
    Provenance,
    /// Lane 4 — user-configured (active-account read/write, debug).
    UserConfigured(UserConfiguredCategory),
    /// Lane 5 — NIP-51 class routing (search/draft/wiki — ADR-0020).
    ClassRouted { class: EventClass, via: ClassRoutingPath },
    /// Lane 6 — operator-configured indexer relays.
    /// ALWAYS-ON for kind:0, kind:3, kind:10000–19999. R+W symmetric.
    Indexer,
    /// Lane 7 — operator-configured app relays.
    /// Fallback substitution when author has no NIP-65 mailbox.
    AppRelay { mode: AppRelayMode },
}
```

Plus a subtractive global filter for blocked relays (kind:10006), applied as a
post-pass — not a lane.

### 3.2 The `OutboxRouter` trait — substrate seam in `nmp-core`

```rust
/// Substrate trait. Defined in nmp-core. Implemented by nmp-router (single
/// impl, generic algorithm). NIP crates do NOT implement this trait and do
/// NOT register anything with the router; they shape its decision per-call
/// by setting `RoutingContext::explicit_targets`.
pub trait OutboxRouter: Send + Sync {
    /// Resolve relays for publishing an event.
    ///
    /// The kernel calls this BEFORE signing — `evt` is the unsigned event so the
    /// router can read its kind, tags, and author. The router must NOT mutate.
    fn route_publish(&self, evt: &UnsignedEvent, ctx: &RoutingContext)
        -> Result<RoutedRelaySet, RoutingError>;

    /// Resolve relays for a subscription (REQ).
    ///
    /// Discovery kinds (kind:0, kind:3, kind:10000–19999) consult the indexer
    /// lane in addition to the per-author NIP-65 read set. Content kinds do not
    /// touch the indexer lane (§3.1 lane 6 R+W symmetry rule applies to
    /// discovery only — see synthesis doc shipped update).
    fn route_subscription(&self, interest: &LogicalInterest, ctx: &RoutingContext)
        -> Result<RoutedRelaySet, RoutingError>;
}

pub struct RoutingContext<'a> {
    pub active_account: Option<&'a Pubkey>,
    pub session_keys: SessionKeySet<'a>,        // active read/write/app/indexer
    pub mailbox_cache: &'a dyn MailboxCache,    // injected — §3.3 (NIP-65 only)
    pub blocked_relays: &'a BlockedRelaySet,    // kind:10006 — post-filter input

    /// When Some, the router's generic algorithm is skipped entirely. The
    /// router returns exactly these URLs, attributed to the ClassRouted lane,
    /// minus any blocked-relay post-filter hits.
    ///
    /// Populated by NIP crates whose actions already know the relay set:
    ///   - nmp-nip17:  DM send populates from its own kind:10050 cache
    ///                 (DmRelayCache, owned by nmp-nip17; the router never
    ///                 sees kind:10050).
    ///   - nmp-nip29:  group post populates from its group state (host relay
    ///                 URL per group).
    ///   - nmp-marmot: MLS group publish populates from its group state
    ///                 (MLS group relay URL).
    ///
    /// The router does not know what NIP populated the field; it only knows
    /// the override is present and which lane to attribute the URLs to. This
    /// is the only mechanism by which NIP-specific relay knowledge enters
    /// routing.
    pub explicit_targets: Option<&'a [RelayUrl]>,
}

pub struct RoutedRelaySet {
    /// Per-relay-URL, which lane(s) put it on the slice. Empty means no relay
    /// is willing to carry this event — surface as `unroutable` rather than
    /// silently broadcast to a random fallback.
    pub relays: BTreeMap<RelayUrl, BTreeSet<RoutingSource>>,
}

impl RoutedRelaySet {
    /// Build from an explicit-target slice, attributing every URL to the
    /// ClassRouted lane and dropping any URL hit by the blocked-relay
    /// post-filter.
    pub fn from_explicit(urls: &[RelayUrl], blocked: &BlockedRelaySet) -> Self { /* … */ }
}

pub enum RoutingError {
    /// Author has no NIP-65 AND no AppRelay set AND no other lane applied
    /// AND no explicit_targets were provided. Kernel surfaces as
    /// `CompiledPlan::unroutable_authors` toast.
    Unroutable(Pubkey),
}
```

Router behavior (pseudocode for `nmp-router`):

```rust
impl OutboxRouter for GenericOutboxRouter {
    fn route_publish(&self, evt: &UnsignedEvent, ctx: &RoutingContext)
        -> Result<RoutedRelaySet, RoutingError>
    {
        if let Some(explicit) = ctx.explicit_targets {
            return Ok(RoutedRelaySet::from_explicit(explicit, ctx.blocked_relays));
        }
        self.generic_resolve_publish(evt, ctx)
    }
    // route_subscription is analogous; explicit_targets shortcuts identically.
}
```

The generic algorithm operates ONLY on: `evt.kind` (indexer eligibility),
`evt.pubkey` (author's NIP-65 write set), `evt.tags` (relay hints and p-tags
for recipient inbox), `ctx.session_keys` (AppRelay / Indexer / UserConfigured
lanes), `ctx.mailbox_cache` (NIP-65 lookups only), `ctx.blocked_relays`
(post-filter). Nothing else. No kind:10050 lookup, no `h`-tag inspection,
no MLS group state — those are the explicit-targets path.

### 3.3 The `MailboxCache` trait — substrate seam in `nmp-core` (NIP-65 only)

```rust
/// Substrate trait. Defined in nmp-core. Implemented by nmp-router.
/// The kind:10002 ingest parser (in nmp-router, registered via the
/// EventIngestDispatcher seam — §4) is the single writer of this cache.
///
/// This trait is for NIP-65 (kind:10002) ONLY. The router consults it from
/// the generic algorithm. It is NOT a generic "any relay-list-bearing kind"
/// abstraction.
pub trait MailboxCache: Send + Sync {
    fn read_relays(&self, author: &Pubkey)  -> Option<Vec<RelayUrl>>;
    fn write_relays(&self, author: &Pubkey) -> Option<Vec<RelayUrl>>;
    fn known(&self, author: &Pubkey)        -> bool;

    /// Single writer — only called by the nmp-router kind:10002 ingest path.
    /// The trait makes the contract structural, not just convention.
    fn upsert(&self, author: Pubkey, list: ParsedRelayList);
}
```

NIP-17's kind:10050 cache is **not** behind this trait and does not live in
`nmp-router`. It is a plain `HashMap<Pubkey, Vec<RelayUrl>>` (`DmRelayCache`)
owned by `nmp-nip17`. NIP-17's DM send action reads its own cache and
populates `RoutingContext::explicit_targets`; the router never sees kind:10050
and never queries a DM-inbox cache. There is no `DmInboxMailboxCache` /
`DmRelayMailboxCache` abstraction in the substrate — the two caches don't
share a trait because they are consulted from different code paths (router's
generic algorithm vs. NIP-17's DM send) and serving different protocol
semantics.

### 3.4 The explicit-target override — how a NIP crate forces a relay set

There is no per-NIP routing-rule registry and no builder method by which a NIP
crate registers routing logic. NIP crates do not implement any routing trait.
The router is one algorithm; NIP-specific knowledge enters per-call through
`RoutingContext::explicit_targets`.

The control flow on the action side:

```rust
// In nmp_nip17::dm_send (pseudocode shape):
let recipient_relays: Vec<RelayUrl> = dm_relay_cache
    .write_relays(&recipient)
    .ok_or(DmSendError::RecipientHasNoDmInbox)?;

let ctx = RoutingContext {
    active_account: Some(&me),
    session_keys,
    mailbox_cache,
    blocked_relays,
    explicit_targets: Some(&recipient_relays),  // ← the override
};

let routed = outbox_router.route_publish(&gift_wrap_evt, &ctx)?;
// `routed` is exactly `recipient_relays` minus blocked-relay hits,
// attributed to the ClassRouted lane. No NIP-17 code ran inside the router.
```

The router has no idea kind:14 / kind:1059 / kind:10050 exist. It only knows
"the caller handed me a slice; return it (minus blocks)." This is the
structural answer to "how do I plug a new NIP into routing": you don't. You
look up the relay set in your NIP crate and pass it through `explicit_targets`.

### 3.5 Three worked examples

**(a) Default — public note (kind:1) on the author's NIP-65 write relays.**

`explicit_targets = None`. The router runs its generic algorithm:
- Publish: author's kind:10002 write set (lane 1) ∪ AppRelays (lane 7).
- Subscription: each author's kind:10002 read set (lane 1) ∪ AppRelays.

No NIP crate involvement on the publish path — this is the generic-algorithm
default for any event whose action did not populate `explicit_targets`.

**(b) DM correctness — kind:14 / kind:1059 on the RECIPIENT's kind:10050 write set.**

`nmp-nip17`'s DM send action looks up the recipient's kind:10050 write relays
from `DmRelayCache` (the cache `nmp-nip17` owns and writes via its own
kind:10050 ingest parser) and populates `explicit_targets` with them before
calling `route_publish`. The router skips its generic algorithm and returns
exactly those URLs attributed to `RoutingSource::ClassRouted`.

```rust
// In nmp_nip17::dm_send (the SendGiftWrappedDmCommand body):
let recipient = first_p_tag(&gift_wrap_evt)?;
let recipient_relays = self.dm_relay_cache
    .write_relays(&recipient)
    .ok_or(DmSendError::RecipientHasNoDmInbox)?;

let routed = outbox_router.route_publish(
    &gift_wrap_evt,
    &RoutingContext {
        explicit_targets: Some(&recipient_relays),
        ..base_ctx
    },
)?;
```

This is the case the kernel's current hardwired `outbox.rs` cannot express.
With the explicit-target seam it becomes ~10 lines in the correct crate, and
zero lines of NIP-17 knowledge in the router.

**(c) NIP-29 — any kind with an `h` tag → host relay from the group state.**

`nmp-nip29`'s `ActionModule`s, when they publish any event (whether an
NIP-29-owned kind like kind:9, or a kind:1 / kind:7 / kind:1111 that happens
to carry an `h` tag), look up the group's host relay from `nmp-nip29`'s own
group state and populate `explicit_targets`:

```rust
// In an nmp_nip29 ActionModule that publishes a group message:
let group_id = require_h_tag(&action_input)?;
let host_relay = self.group_state
    .host_relay(&group_id)
    .ok_or(GroupActionError::UnknownGroup)?;

let routed = outbox_router.route_publish(
    &unsigned,
    &RoutingContext {
        explicit_targets: Some(std::slice::from_ref(&host_relay)),
        ..base_ctx
    },
)?;
```

The router never inspects `h` tags; it never queries group state. NIP-29
knowledge lives entirely in `nmp-nip29`. Same shape for Marmot's MLS group
publish (`explicit_targets` from the MLS group relay) — the router stays
identical across NIP-17, NIP-29, and Marmot.

### 3.6 The DM read path — input-side projection seam

For DM publish to populate `explicit_targets` correctly, the recipient's
kind:10050 cache must be populated from incoming events. The kernel ingests
events; it cannot know how to parse kind:10050. That requires the **input-side
projection seam** (§4):

```rust
// In nmp_nip17::register_actions(app):
app.register_ingest_parser_kind(10050, Box::new(|evt: &VerifiedEvent| {
    let parsed = parse_kind_10050_relays(evt);
    self.dm_relay_cache.upsert(evt.pubkey, parsed);
}));
```

The kernel's `EventIngestDispatcher` calls registered parsers on every ingested
event. The kernel does not know what kind:10050 is, and the router does not
know what kind:10050 is. After V-40 lands, the existing `Kernel::dm_relay_lists`
field is deleted; the cache (`DmRelayCache`) lives in `nmp-nip17` and is read
by the DM send action to populate `RoutingContext::explicit_targets` (§3.5b).

Similarly, kind:10002 ingest registers in `nmp-router` (it owns the
`MailboxCache` and is the only consumer of kind:10002 data, so it is the right
home for the kind:10002 parser):

```rust
// In nmp_router::register(app):
app.register_ingest_parser_kind(10002, Box::new(|evt| {
    mailbox_cache.upsert(evt.pubkey, parse_kind_10002(evt));
}));
```

### 3.7 Why this is the only correct shape

- **Substrate carries no NIP names.** Read every line of `nmp-core` post-refactor
  and grep for "10002" / "10050" / "nip17": zero hits. The substrate trait
  vocabulary is `OutboxRouter`, `MailboxCache`, `EventIngestDispatcher`,
  `RoutingContext::explicit_targets`. NIP names enter only at composition time
  when a Layer 4 crate calls `app.register_ingest_parser_kind(kind, …)` and at
  call time when an action populates `explicit_targets`.

- **`nmp-router` carries no NIP names either.** Grep `nmp-router` post-refactor
  for "10050" / "nip17" / "nip29" / "marmot" / "h_tag": zero hits. The router
  knows about kind:10002 (its own concern as `MailboxCache` writer + indexer
  eligibility table) and that is it. No NIP-17 / NIP-29 / Marmot logic lives
  in routing.

- **A competing outbox is a swap.** A new `nmp-router-v2` crate that implements
  `OutboxRouter` differently is dropped in at composition time:
  `builder.outbox_router(Arc::new(MyAlternativeRouter::new()))`. No kernel
  change required. The user's "what if I want a competing outbox algorithm"
  question is answered structurally.

- **Routing is one algorithm, not N rules.** There is no rule-vector to walk,
  no rule precedence to reason about, no question of "which rule won?" The
  algorithm is generic or the action overrode it. Two states, not N.

- **The DM correctness bug is impossible to write.** `nmp-nip17`'s send
  action populates `explicit_targets` with the recipient's kind:10050 write
  set; the router returns exactly that set. The kernel cannot accidentally
  route a DM through the author's kind:10002 because the override skips the
  generic algorithm entirely.

### 3.8 The pool API — `nmp-network` public surface

The pool is push-model. Callers do not poll; they ensure a connection exists,
fire frames at a handle, and receive a stream of `PoolEvent`s on a channel the
constructor accepts.

```rust
// In nmp-network.
pub struct Pool { /* Arc inside, cheap to clone */ }

/// Generational handle: (url, open_count). A stale handle from before a
/// reconnect is structurally rejected by send/health/close — it cannot
/// silently target the wrong generation of the same URL.
pub struct RelayHandle(/* generational ID */);

impl Pool {
    pub fn new(cfg: PoolConfig, events: Sender<PoolEvent>) -> Self;

    /// Idempotent. If the URL is already open and healthy, returns its
    /// current handle. If it is closing or closed, kicks off a fresh
    /// connection and returns the new generation's handle.
    pub fn ensure_open(&self, url: &RelayUrl) -> RelayHandle;

    pub fn close(&self, h: RelayHandle);
    pub fn shutdown(&self);

    /// Fire-and-forget send to one specific (relay, generation). There is
    /// NO "send to all connected relays" method on this type. The kernel
    /// actor (the only caller above this crate) iterates RoutedRelaySet
    /// itself and issues one constrained send per URL. This is the
    /// structural answer to NDK issue #175.
    pub fn send(&self, h: RelayHandle, frame: WireFrame);

    pub fn health(&self, h: RelayHandle) -> RelayHealth;
    pub fn snapshot(&self) -> PoolSnapshot;
}

pub enum PoolEvent {
    Opened { h: RelayHandle, url: RelayUrl, generation: u64 },
    Frame  { h: RelayHandle, generation: u64, frame: RelayFrame },
    Closed { h: RelayHandle, generation: u64, reason: ClosedReason },
    Failed { h: RelayHandle, generation: u64, error: TransportError },
    Health { h: RelayHandle, snapshot: RelayHealth },
}
```

The `Pool` type is public so the kernel actor can hold a handle, but every
internal collaborator (connection state machines, reconnect token bucket,
backoff timers, frame writer/reader) is `pub(crate)` within `nmp-network`.
The kernel actor is the only caller of `Pool::send` / `Pool::ensure_open`
above this crate; the router never holds a pool reference and the planner
never holds a pool reference.

NIP-42 AUTH handling is split: `nmp-network` performs the wire handshake
(send and receive the `AUTH` frame; surface the inbound frame as a
`RelayFrame::Auth` variant) but does NOT compute the kind:22242 event (lives
in `nmp-nip42`) and does NOT pause / replay subscriptions during a challenge
(lives in the planner's `AuthGate`). Splitting it this way keeps the network
layer protocol-aware about exactly one thing (the wire frame shape) and
leaves the semantic FSM to the higher layers.

---

## 4. The `ActorCommand` open seam — two seams, not one

To move V-38 (NWC) / V-39 (DM send) / V-41 (LNURL fetcher) out of `nmp-core`,
two substrate seams must open in lock-step. Opening one without the other
leaves the migrations half-done.

### 4.1 Open ActorCommand — the write-path seam

Today `ActorCommand` is a closed enum that the kernel pattern-matches
exhaustively in `actor/dispatch.rs`. Every NIP protocol command that needs to
run on the actor thread (DM send, LNURL fetch, NWC pay) is a hardcoded variant.
The kernel knows protocol nouns.

**Replacement:**

```rust
/// In nmp-core.
pub trait ProtocolCommand: Send + 'static {
    /// Run on the actor thread. May enqueue follow-up ActorCommands via `send`
    /// (e.g. the LNURL fetcher spawns an HTTP worker and feeds bolt11 back as
    /// a follow-up command).
    fn run(self: Box<Self>, ctx: &mut ActorContext, send: &dyn Fn(ActorCommand))
        -> Result<(), ProtocolCommandError>;
}

pub enum ActorCommand {
    // ... existing substrate-grade variants (Start, Stop, Shutdown, IngestPreVerifiedEvents,
    // PublishUnsignedEvent, IngestSignedEvent, AddRelay, RemoveRelay, the lifecycle
    // variants, the publish control plane, observer registration, etc.) ...

    // NEW: the open variant. NIP crates dispatch protocol-level commands
    // through this. The kernel doesn't pattern-match the body; it calls run().
    Protocol(Box<dyn ProtocolCommand>),
}
```

Migration shape — V-39's `SendGiftWrappedDm` becomes (in `nmp-nip17`):

```rust
struct SendGiftWrappedDmCommand {
    recipient_pubkey: Pubkey,
    rumor: UnsignedEvent,
    correlation_id: Option<String>,
}

impl ProtocolCommand for SendGiftWrappedDmCommand {
    fn run(self: Box<Self>, ctx: &mut ActorContext, send: &dyn Fn(ActorCommand))
        -> Result<(), ProtocolCommandError>
    {
        // The body of today's nmp-core::actor::commands::dm.rs::send_gift_wrapped_dm.
        // Resolves signer via SignerForSealCapability, calls
        // nmp_nip59::gift_wrap_with_signer twice, dispatches each kind:1059 as
        // PublishSignedEvent via send().
    }
}
```

The kernel dispatch arm becomes:

```rust
ActorCommand::Protocol(cmd) => {
    if let Err(e) = cmd.run(&mut ctx, &send) {
        kernel.set_last_error_toast(e.to_user_message());
    }
}
```

NIP crates no longer add variants to `ActorCommand`. The enum stops being a god
object.

### 4.2 Open ingest — the read-path seam

Today, kind:10050 / kind:10002 (and any other kind with custom ingest semantics)
are pattern-matched in `nmp-core::kernel::ingest::mod.rs`. The kernel knows
protocol kinds by number. To move V-40 (kind:10050) and to let NIP crates own
their own ingest (kind:30023 long-form, NIP-51 list kinds), the ingest path
must accept registered parsers.

```rust
/// In nmp-core.
pub trait IngestParser: Send + Sync {
    /// Called for every ingested event whose kind matches the registered kind
    /// (or kind range, for the kind:10000–19999 NIP-51 group).
    /// MUST be side-effect-free against the kernel's own state — parsers
    /// write to their NIP crate's own caches/projections.
    fn parse(&self, evt: &VerifiedEvent);
}

pub struct EventIngestDispatcher {
    by_kind: HashMap<u16, Vec<Arc<dyn IngestParser>>>,
    by_range: Vec<(Range<u16>, Arc<dyn IngestParser>)>,
}

impl NmpAppBuilder {
    pub fn register_ingest_parser_kind(&mut self, kind: u16, parser: Arc<dyn IngestParser>);
    pub fn register_ingest_parser_range(&mut self, range: Range<u16>, parser: Arc<dyn IngestParser>);
}
```

The kernel ingest path becomes one call:

```rust
fn on_event_ingested(&self, evt: &VerifiedEvent) {
    self.ingest_dispatcher.dispatch(evt);  // calls all registered parsers
    self.notify_raw_event_observers(evt);  // existing path
}
```

After V-40 lands: `Kernel::dm_relay_lists` is deleted, the kind:10050 match arm
in `kernel/ingest/mod.rs` is deleted, and `CompileTrigger::DmRelayListChanged`
generalizes or is removed.

### 4.3 Why both seams are required

V-38 / V-39 / V-41 cannot be completed by §4.1 alone: NWC needs both a
`ProtocolCommand` to run (§4.1) AND an input-side parser for kind:23195 NWC
responses (§4.2). NIP-17 DM send needs §4.1 for the send command AND §4.2 for
kind:10050 ingest. NIP-57 LNURL needs §4.1 for the LNURL fetcher AND §4.2 to
let `nmp-nip57` own kind:9735 ingest cleanly rather than rely on the existing
raw-event-observer escape hatch.

Open one without the other and the migrations stall in half-states.

### 4.4 What this is NOT

This is **not** a generic plugin system. `ProtocolCommand` carries no
serialization, no versioning, no discovery — it is in-process trait dispatch by
crates known at compile time. The seam exists so the substrate stops carrying
protocol nouns, not so third-party plugins can be loaded at runtime. (Runtime
plugins are out of scope; if they ever become in scope they will be a separate
ADR on top of these seams.)

---

## 5. Migration order

Strict dependency order. Each step has a prerequisite cited.

1. **Define the four substrate seams in `nmp-core`** (no migrations yet):
   - `trait OutboxRouter` (§3.2)
   - `trait MailboxCache` (§3.3)
   - `trait ProtocolCommand` + `ActorCommand::Protocol(Box<dyn ProtocolCommand>)` variant (§4.1)
   - `trait IngestParser` + `EventIngestDispatcher` + `register_ingest_parser_kind/_range` builder methods (§4.2)

   **Why first:** every subsequent migration depends on these trait definitions
   existing. Adding traits + one new enum variant is non-breaking; the existing
   closed-enum dispatch arms keep working.

2. **Create `nmp-router`. Port kernel's `outbox.rs` + `InMemoryMailboxCache` + absorb `nmp-nip65`'s `PublishRelayListAction`. Register kind:10002 ingest parser via `EventIngestDispatcher`. Implement the seven-lane generic resolver + `explicit_targets` override seam. Implement `selectOptimalRelays`. Delete `nmp-nip65`. The router has no per-NIP routing-rule registry. NIP crates do not register anything with the router.**

   **Why second:** without `OutboxRouter` and `MailboxCache` having concrete
   impls, the kernel cannot wire them as `Arc<dyn …>` injected dependencies.
   The kernel still uses its old hardwired path until step 3 cuts over. NIP-65
   is too thin to stand alone — its single ActionModule + the relay-list cache
   live in the same crate that owns routing. V-50 closes here.

   **Temporary dep edge:** at this step, `nmp-router` calls into `nmp-core`'s
   existing `relay_worker` / `relay_protocol` modules directly (the
   `nmp-network` extraction is step 8). The per-crate table's
   `nmp-router → nmp-network` edge is the *post-step-8* shape. Between steps
   2 and 8 the edge points into `nmp-core`'s native module instead, and step
   8 retargets it without changing `nmp-router`'s public surface.

3. **Cut the kernel over to `Arc<dyn OutboxRouter>`. Delete `kernel/outbox.rs` body.**

   **Why third:** with step 2's impl in place, the cut-over is a single
   injection swap. The hardwired algorithm in the kernel is deleted; routing
   happens entirely through the trait.

4. **V-41 — `nmp-nip57` absorbs LNURL fetcher.** Move `actor/commands/zap.rs` + `zap_lnurl.rs` to `nmp-nip57::lnurl`. The `FetchLnurlInvoice` `ActorCommand` variant becomes `Protocol(Box<FetchLnurlInvoiceCommand>)`. Delete the variant.

   **Why fourth:** V-41 is the smallest of the three protocol-command
   migrations — it has no input-side parser dependency (kind:9735 ingest is
   already on the raw-event-observer path; the kernel doesn't carry a kind:9735
   special case). It is the cleanest proof the `ProtocolCommand` seam works.

5. **V-39 — `nmp-nip17` absorbs DM send.** Move `actor/commands/dm.rs` to `nmp-nip17::dm_send`. The `SendGiftWrappedDm` `ActorCommand` variant becomes `Protocol(Box<SendGiftWrappedDmCommand>)`. Add `SignerForSealCapability` trait on `ActorContext`. Delete the variant.

   **Why fifth:** depends on step 1 (open ActorCommand) and adds the
   `SignerForSealCapability` substrate trait. Doesn't require V-40 to land
   first (the kind:10050 cache stays in `nmp-core` for one more step).

6. **V-40 — `nmp-nip17` absorbs kind:10050 ingest + cache.** Move `kernel/ingest/dm_relay_list.rs` to `nmp-nip17::dm_relay_list_ingest`. Move the cache type (`DmRelayCache`) into `nmp-nip17`. Register the parser via `register_ingest_parser_kind(10050, ...)`. Update the DM send action (already in `nmp-nip17` from step 5) to read `DmRelayCache` and populate `RoutingContext::explicit_targets` before calling `route_publish`. Delete `Kernel::dm_relay_lists`, the kind:10050 match arm, and `CompileTrigger::DmRelayListChanged`.

   **Why sixth:** depends on steps 1 (ingest parser seam), 2 (the `nmp-router`
   `OutboxRouter` impl exposes `explicit_targets`), and 5 (V-39 already moved
   the send path that consults the cache; if V-40 ran first the send path in
   `nmp-core` would have to read across a crate boundary into `nmp-nip17`'s
   cache).

7. **V-38 — create `nmp-nip47`. Move all wallet code out of `nmp-core`.** Move `actor/commands/wallet.rs` + `wallet/` to `nmp-nip47`. The three `Wallet*` variants become `Protocol(Box<…>)`. The `wallet` Cargo feature on `nmp-core` is deleted. The `nmp-core → nmp-nwc` dependency edge is deleted. `nmp-nip47 → nmp-nwc` and `nmp-nip47 → nmp-core` are added. The three bespoke FFI symbols (`nmp_app_wallet_*`) become thin shims over `dispatch_action`.

   **Why seventh:** depends on step 1 (open ActorCommand). Saved for last
   because it is the biggest single migration AND it flips a dep direction
   that has been wrong since `nmp-core` first grew NWC support. Doing it last
   means the seam has already been exercised three times.

8. **Create `nmp-network`. Extract `nmp-core::relay_worker` + `relay_protocol` + `nmp-wasm::BrowserRelayDriver` + pool lifecycle. Implement `RelayHealth`, NIP-11 capability probe hook, push-model `PoolEvent` channel, generational `RelayHandle`, per-relay token bucket reconnect storm protection, LRU eviction under budget. Migrate `nmp-signer-broker` onto `nmp-network`'s `Pool` primitive (V-13 dedupe). Retarget the temporary `nmp-router → nmp-core` relay-worker edge so it now depends on `nmp-network` instead.**

   **Why eighth:** this is V-13 Stage 1 + V-14 follow-up + the broker dedupe
   + the step-2 dep-edge retargeting. It is independent of the NIP migrations
   above and can land in parallel with steps 4–7. The ordering here just
   reflects "after the routing/protocol refactor settles." Post-step-8, the
   per-crate table's `nmp-router → nmp-network` edge is real; before it, the
   edge was temporarily into `nmp-core`'s native module (see step 2 note).

9. **Extract `nmp-store` (consolidate the `EventStore` trait + LMDB/in-memory/IndexedDB backends). Extract `nmp-planner` from `nmp-core::planner`.**

   **Why ninth:** these are crate-housekeeping extractions that don't change
   behavior. They become cleaner once the substrate seams above have stopped
   the bleeding. `nmp-store` extraction unblocks F-01 IndexedDB without
   touching `nmp-core`.

10. **Create `nmp-app-template`. Wire `nmp init` in `nmp-cli` to scaffold it.**

    **Why tenth:** V-48. Depends on every prior step because the template wires
    the canonical registrations the new architecture exposes.

11. **Extract `nmp-ffi` from `nmp-core::ffi`. Move `nmp-chirp-config`, `chirp-repl`, `chirp-tui`, `fixture-todo-core` out of `crates/`.**

    **Why eleventh:** final housekeeping. Once `nmp-core` is the substrate this
    document describes, the C-ABI surface is a separate concern and the
    app-specific shells stop polluting `crates/`.

12. **Return `nmp-marmot` from `apps/marmot/` to `crates/nmp-marmot/`.** ✅ done 2026-05-25.

    **Why twelfth:** ADR-0025 carved Marmot out because its FFI cluster was
    Chirp-coupled. The bespoke write-side `nmp_marmot_dispatch` C symbol
    ported to the generic `nmp_app_dispatch_action("nmp.marmot", …)` seam
    in ADR-0025 PR 3 (2026-05-23). The remaining `nmp_marmot_*` symbols
    (register, snapshot, group_messages, string_free, unregister,
    fetch_key_packages) are kernel-shaped per-app FFI — observer /
    projection / opaque-handle lifecycle — and follow the same pattern as
    Chirp's `nmp_app_chirp_*` cluster. That pattern is sanctioned for
    Layer-4 NIP crates whose runtime needs a stateful handle (per ADR-0025
    update 2026-05-23). App-specific identity/keyring wrappers belong in
    app crates and call Marmot's caller-supplied keyring helpers. Done as
    Path B from the step-12 brief: per-app FFI accepted as-is, crate moved
    to `crates/nmp-marmot/`, workspace + dependent path deps + CI
    header-drift gate updated; the Marmot FFI surface (ABI, header, Swift
    bridge) is byte-stable.

---

## 6. What stays in `nmp-core` forever

The kernel substrate. Everything below MUST stay in `nmp-core`; nothing on this
list belongs in a NIP crate, an app crate, or a binding crate.

- **The actor model** — single OS thread, flume channel, `run_actor`, the
  synchronous `recv()` loop. The TEA primitives: `AppState`, `KernelUpdate`,
  `KernelReducer`, `handle_message`. The `rev: u64` monotonicity guard.
- **The `ActorCommand` enum's substrate-grade variants** — `Start`, `Stop`,
  `Shutdown`, `IngestPreVerifiedEvents`, `IngestSignedEvent`,
  `PublishUnsignedEvent`, `PublishSignedEvent`, `AddRelay`, `RemoveRelay`,
  lifecycle callbacks, publish control plane, observer registration, action
  registry, and `Protocol(Box<dyn ProtocolCommand>)` (the open seam). NO
  NIP-specific variant ever lands here.
- **Capability sockets** — keychain, push, network monitor, NSE
  decrypt-only sockets. The pattern, not specific bridges.
- **Session + account model** — `AccountManager` integration, `switch_active`,
  active-account state, the `AccountSummary` projection.
- **The `EventStore` interface** — the trait the kernel holds as `Arc<dyn
  EventStore>` (impls live in `nmp-store`).
- **The `SubscriptionPlanner` interface** — `InterestRegistry`,
  `LogicalInterest`, `CompileTrigger`, `CompiledPlan` as substrate types
  (impl lives in `nmp-planner`).
- **The `OutboxRouter` + `MailboxCache` traits** (§3.2, §3.3) — substrate seams.
- **The `EventIngestDispatcher` + `IngestParser` trait** (§4.2) — substrate
  seam.
- **The `ActionModule` trait + `dispatch_action` registry + `ActionContext`** —
  the write-path seam every NIP crate uses to expose an action namespace.
- **`KernelEventObserver` / `RawEventObserver` / projection registries** — the
  observer pattern itself; specific observers are app-owned.
- **The snapshot envelope** (`UpdateEnvelope`, `WireEnvelope`,
  `SNAPSHOT_SCHEMA_VERSION`) — the FFI-bound serialization wrapper.
- **`display::` helpers** — cross-surface formatting primitives (V-22 / V-25
  / V-26 / V-33 precedent). djb2 avatar color, npub abbreviation, relative
  time bucketing. They are display substrate the whole workspace shares.
- **`relay::canonical_relay_url`** — the canonicalization function every
  routing rule depends on.
- **The `coverage_hook` seam** — D2 enforcement plug-in point.
- **The `auth_signer` seam** — kernel-side NIP-42 signing slot.
- **The `RelayFrame` enum** — the wire-transport-agnostic frame the kernel
  ingests (the impl is in `nmp-network`).

If a future PR proposes adding to this list, the addition must be substrate-
grade: pure trait + pure data type, no protocol nouns, no app nouns, no I/O.
If it does not fit, it belongs in a NIP crate.

---

## 7. Crates to delete

| Crate | Why | Replacement |
|---|---|---|
| `nmp-nip65` | Too thin to stand alone (the user's words: "it's too simple!"). Its single `PublishRelayListAction` + the kind:10002 ingest + `MailboxCache` belong in the crate that owns routing. | Absorbed into `nmp-router` (step 2 of the migration order). The action namespace `nmp.nip65.publish_relay_list` is byte-stable for callers. |

That is the only crate this document deletes. Every other current crate either
stays put or moves to its correct layer with the same name. No other deletions
are justified by SRP alone — `nmp-nip42-types` exists to break a real dep cycle
(see Layer 0 table note); `nmp-coverage-gate` is a real substrate policy seam
even though tiny; `nmp-nwc` is a legitimate protocol crate that `nmp-nip47` will
depend on (the violation is the dep direction `nmp-core → nmp-nwc`, not the
crate's existence).

---

## 8. Decision log (entries for this document)

- **2026-05-24** — `nmp-nip65` deleted; relay routing centralized in
  `nmp-router`. Routing is one generic algorithm; NIP-65 ingest + cache +
  `publish_relay_list` action absorbed.
- **2026-05-24** — Per-NIP routing-rule registry rejected; NIP crates do not
  register routing rules. Explicit relay targeting uses
  `RoutingContext::explicit_targets: Option<&[RelayUrl]>`. Routing is one
  generic algorithm. Three independent design agents converged on this shape
  in preference to a per-NIP rule registry.
- **2026-05-24** — Routing and networking live in two crates, not one:
  `nmp-router` (Layer 2, routing algorithm + NIP-65 mailbox cache) and
  `nmp-network` (Layer 1, sockets + pool lifecycle). Pool API is push-model
  `PoolEvent` channel + generational `RelayHandle`. The pool exposes only
  constrained per-handle sends; there is no "send to all" method — the
  structural answer to NDK issue #175. The `Pool` type is public but the
  kernel actor is the only caller above `nmp-network`.
- **2026-05-24** — kind:10050 DM-inbox cache (`DmRelayCache`) lives in
  `nmp-nip17`, not `nmp-router`. The router's `MailboxCache` is NIP-65
  (kind:10002) only. NIP-17's DM send action reads its own cache and passes
  the relays via `explicit_targets`; the router never sees kind:10050.
- **2026-05-24** — "Per-relay filter execution strategy" = authors
  partitioning in `nmp-planner::project_per_relay`. Given a `LogicalInterest`
  and a `RoutedRelaySet`, the planner restricts each per-relay filter's
  `authors` field to the subset of authors that actually writes to that
  relay. Per-relay `since` cursors are out of scope (novel, orthogonal to
  routing; would belong in `nmp-store` if ever added — separate ADR).
- **2026-05-24** — `ActorCommand::Protocol(Box<dyn ProtocolCommand>)` open
  seam confirmed as the write-path mechanism for V-38 / V-39 / V-41 / future
  NIP commands.
- **2026-05-24** — `EventIngestDispatcher` + `IngestParser` confirmed as the
  read-path mechanism. V-40's kind:10050 migration is the first user.
- **2026-05-24** — `nmp-nip22` is its own crate, not a NIP-29 concern.
- **2026-05-24** — `nmp-signer-broker` depends on `nmp-network`'s shared
  `Pool` primitive. The workspace has exactly one readiness-driven WebSocket
  implementation.
- **2026-05-24** — `nmp-marmot` returns to `crates/` once its FFI cluster
  ports to `nmp.marmot.*` `dispatch_action`s (ADR-0025 retirement schedule).
- **2026-05-25** — `nmp-marmot` returned to `crates/nmp-marmot/` (step 12,
  Path B). The bespoke write-side `nmp_marmot_dispatch` C symbol was
  already retired (ADR-0025 PR 3, 2026-05-23); the surviving
  `nmp_marmot_*` cluster is kernel-shaped per-app FFI (the same pattern
  Chirp's `nmp_app_chirp_*` cluster uses), so the per-app FFI is
  sanctioned and the spec no longer requires those symbols to port to a
  generic seam.
- **2026-05-24** — Layer 6 binding crates (`nmp-ffi`, `nmp-wasm`,
  `nmp-android-ffi`) are siblings. No cross-binding dependency exists or
  may be introduced.

Future amendments to this document edit it in place per the planning
discipline rule (`AGENTS.md` §Planning discipline — single source of truth
per fact). No parallel "v2" document.
