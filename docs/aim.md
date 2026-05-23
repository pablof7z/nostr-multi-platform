# Project Aim — Rust Multiplatform Nostr Application Framework

## Purpose of this document

This document is the cold-start context for a brand-new working session. Read it before doing anything else. It defines the project's north star, architectural foundation, the bodies of prior work being synthesized, the doctrine the design must enforce, and the crate layout. It does not contain implementation. It contains the *aim*.

---

## 1. The north star

We are designing a **Rust multiplatform framework for building Nostr applications** that ships a single Rust core consumed identically by iOS (SwiftUI), Android (Jetpack Compose), desktop (iced or Tauri), and web (wasm). The core owns all protocol logic, all state, all caching, all relay management, all signing orchestration, all derived views. Platform code is a thin rendering shell.

The framing concern is one sentence: **make it nearly impossible to build a broken Nostr application.** Today, building a Nostr client involves dozens of subtle correctness pitfalls — stale replaceable events, lost subscriptions, wrong relays for wrong events, race conditions between local state and relay state, leaked signing operations, multi-account state desync. The framework's job is to make each of those classes of bug structurally impossible through the safe app-kernel and FFI API: not merely documented as a footgun or caught by a linter, but ruled out by the type system, actor ownership, and public API surface. Lower-level Rust escape hatches, when they exist for internal subsystems or tests, must be explicitly named, instrumented, and covered by regression tests.

The success criterion is qualitative: **a developer should be able to one-shot a working Nostr application** — login, timeline, compose, profile, DMs, wallet — using the framework's CLI scaffold and a few hundred lines of platform UI code, and have it ship with sane defaults on all four platforms without the developer ever touching relay routing, cache invalidation, replaceable-event semantics, or subscription lifecycle. If they don't go out of their way to defeat the framework, the app will be correct.

---

## 2. Architectural foundation: the RMP bible

The architectural skeleton is taken wholesale from the **`rust-multiplatform/rmp`** project and its central design document, `rmp-architecture-bible.md`. This is non-negotiable; the bible is the standard. The bible's commandments — quoted and paraphrased below — are the load-bearing structure of the entire framework. A new session must read the bible before writing code; we are not re-deriving it.

The architectural model is **The Elm Architecture (TEA)**, also called Model-View-Update. Three primitives:

- **`AppState`** — a single struct containing all data the UI needs to render.
- **`AppAction`** — an enum of every user intent or lifecycle event.
- **`handle_message(state, action) -> state`** — a pure update function that takes current state and a message and produces new state.

Data flow is **strictly unidirectional**: user interaction → action dispatch → actor processes synchronously → state emission → platform re-renders. From the bible: *"no data races. A single actor thread owns all mutable state. No locks, no concurrent mutation, no race conditions."*

The execution model is the **actor pattern**. A dedicated OS thread owns `AppState` and runs a synchronous event loop reading from a `flume` channel. A separate tokio runtime handles async I/O (relay connections, signing, database) and feeds results back through the same channel as `InternalEvent`. Only the actor thread mutates state.

The cross-FFI flow:

```
Native UI calls dispatch(action)         [fire-and-forget, never blocks]
  → flume channel
  → Actor thread recv()
  → handle_message() mutates AppState, increments rev
  → AppUpdate emitted on update channel
  → Listener thread invokes AppReconciler.reconcile(update) callback
  → Native code hops to main/UI thread
  → State replaced via @Observable / mutableStateOf / runes / signals
  → UI re-renders
```

Critical invariants pulled directly from the bible, all of which the framework must honor:

1. **Monotonic revision guard.** `rev: u64` increments on every state change. Platforms compare incoming `rev` to last applied and skip stale updates.
2. **Errors do not cross FFI.** Operational errors become `toast: Option<String>` fields in state; long-running operation errors clear `busy` flags. Native `dispatch` calls never need try/catch.
3. **`dispatch()` is fire-and-forget.** No return value. Results come back as state changes.
4. **No native business logic.** If you would write an `if` statement in Swift or Kotlin that decides what the app should *do* (not how it should *look*), that logic belongs in Rust. Native is rendering plus capability execution. Nothing else.
5. **Bounded native state.** Native holds only transient OS handles (keychain refs, audio sessions, network monitors). No caches, no derived state, no policy.
6. **Capability bridge pattern.** When Rust needs an OS API (keychain, push, location, external signer app), it requests the capability via a typed callback interface. Native executes and reports raw data. Rust decides policy. Native never decides "should we retry?" or "is this recoverable?"
7. **Idempotent capability lifecycle.** Start/stop/restart of any bridge must be safe.
8. **Avoid the god module.** When the core actor file exceeds ~1,000 lines, split by domain into submodules with `pub(super)` visibility.
9. **No high-frequency FFI loops.** Callbacks above ~60Hz across FFI must be batched or delivered via direct memory, not serialized per event.
10. **Snapshot semantics.** State crosses FFI as a full `Clone`d snapshot by default; granular updates are an optimization, not a default.

The bible's reference crate layout — `rust/` for the core (cdylib + staticlib + rlib), `uniffi-bindgen/` for the binding generator, `ios/`, `android/`, `crates/<app>-desktop/`, a `justfile` for build orchestration, an optional Nix flake — is the layout this framework will scaffold for users. We adopt RMP's `rmp-cli` as the model for our own scaffolding tool.

The bible's anti-patterns must be enforced against:

- Duplicated formatting logic across platforms (timestamps, display names) — Rust pre-formats into strings, native renders them.
- Business logic in ViewState derivation — derivation should be field renames and type conversions only.
- Navigation state leaking to native — Rust's router is the single source of truth.
- Native-side caches of derived values — caching lives in Rust.
- Capability bridge scope creep — bridges report, they do not decide.

These are not best practices. They are constraints the framework's public API must make difficult to violate.

---

## 3. Protocol foundation: existing Rust primitives

The framework does not reimplement the Nostr protocol. The Rust ecosystem already has a mature, modular set of protocol crates that we wrap and orchestrate:

- A **protocol crate** providing `Event`, `EventBuilder`, `Filter`, `Keys`, `Tag`, all NIP-defined types, bech32 encoding, NIP-19 entities, no_std support, and around 60 implemented NIPs.
- A **client/SDK crate** (`nostr-sdk`) providing `Client`, relay pool management, subscription routing, async streaming over tokio. **NMP does not use this crate.** Its relay pool is tokio-async and reference-counted; NMP's kernel is a single synchronous actor (§2). NMP instead depends on the `nostr` protocol crate for types/crypto and maintains its own relay transport (`crates/nmp-core/src/relay_worker/`, raw `tungstenite`) shaped to the actor model — generational relay handles, idle-tick-gated `recv_timeout`, interest-lattice subscription coalescing. See **ADR-0022** (`docs/decisions/0022-relay-transport-reimplementation.md`) for the full rationale.
- A **database trait** with multiple swappable backends: in-memory, LMDB, nostrdb, SQLite (native and WASM via OPFS/IndexedDB VFS).
- A **gossip/outbox trait** with in-memory and SQLite backends, implementing the NIP-65 relay-list metadata model and per-pubkey relay discovery.
- A **NIP-46 (Nostr Connect / bunker) signer crate** for remote signing.
- A **NIP-07 browser signer crate** plus a native-side proxy to use NIP-07 from desktop/mobile.
- An **OS keyring crate** wrapping macOS Keychain, Windows Credential Manager, and Secret Service.
- A **NIP-47 NWC client crate** for wallet operations.
- A **Blossom client crate** for media storage.
- A **relay builder crate** providing `LocalRelay` (full in-process relay) and `MockRelay` (ephemeral, for tests).

These crates are **dependencies, not forks**. The framework's job is to compose them into an opinionated whole; their authors do the protocol correctness work, we do the application-layer work. Where they have gaps relative to the framework's goals (reactive queries, models, sessions, web-of-trust, opinionated outbox routing on every operation), the framework adds those layers above — it does not push them down into the protocol layer.

---

## 4. High-level functionality being synthesized

Two existing TypeScript libraries in the broader Nostr ecosystem, **NDK** and **Applesauce**, have between them many of the high-level patterns a polished Nostr client framework needs. This Rust framework is a deliberate synthesis of the useful lessons from both. The functionality below is not invented from scratch; our work is to translate the right pieces into Rust + RMP idiom.

The translation is selective. Applesauce is a strong reference for reactive event stores, derived models, fallback loaders, action runners, relay adapters, and product-layer packages, but its RxJS streams, mutable symbol metadata, and browser-first API surface are not the architecture we ship. NDK is a strong reference for relay pools, cache adapters, subscription grouping, per-relay provenance, sessions, sync, wallet, Blossom, WoT, and messaging modules, but NMP should avoid growing one monolithic cache trait or embedding product policy in the v1 kernel.

The architectural delta is the core idea of this project: use the Rust Nostr SDK family for protocol primitives, then build a new Rust application kernel above it. We are not forking the Rust SDK and we are not porting Applesauce or NDK APIs. We are creating the missing multiplatform app layer: actor-owned state, bounded FFI projections, canonical store semantics, subscription and action lifecycle, storage/metrics/test harnesses, and extension seams for later product modules.

### 4.1 Reactive single source of truth ("EventStore")

The central abstraction is a **reactive event store** that owns every event the application has ever seen. Every read goes through it. Every write — once a signed event is produced — passes through it before going to relays. It enforces NIP-01 replaceable-event semantics on insert (a new kind-0, kind-3, or parameterized replaceable kind automatically supersedes its predecessor — there is no way to have a stale version in memory). It tracks delete events (kind 5) and removes referenced events automatically. It tracks expiration tags (NIP-40). It deduplicates by event id while merging metadata (relay provenance, verification flags) across duplicate arrivals. It exposes three top-level reactive streams (`insert$`, `update$`, `remove$`) plus targeted subscription methods.

The store has built-in **helper subscriptions** for common queries — get a user's profile, contacts, mailboxes, mutes, blossom servers, reactions to an event, replies to a thread, comments on an event. These are not separate library calls; they are methods on the store itself, so the right query is always one obvious method away.

A **fallback event loader** is a single user-provided async function the store calls when a subscription asks for an event it doesn't have. The store handles cache misses transparently; the developer never writes "if missing, fetch from relay, then update local state" logic — that pattern is the source of an enormous fraction of Nostr-client bugs.

A **claim-based GC system** tracks which subscriptions reference which events. When subscriptions drop, claims drop. A `prune()` pass collects events with no active claims. Memory does not grow without bound; this is automatic.

In our Rust framing, the actor owns the event store as internal substrate. `AppState` is not the store; it is the bounded UI projection of currently open views plus small app metadata. The full event store never crosses FFI.

### 4.2 Reactive models / derived views

On top of the event store, the framework provides **pre-built derived views** — a "profile view" composes kind-0 events into a typed profile struct, a "timeline view" composes filter-matching events into a sorted list, a "contacts view" exposes a parsed follow list, a "thread view" assembles replies into a tree, a "reactions view" tallies kind-7 reactions for a target event. These are pure functions of the event store's contents; they recompute automatically when underlying events arrive or update.

Views are **cached and shared**. Two UI components asking for the same view get the same handle. A view stays "warm" for a configurable interval after its last subscriber drops, so navigation that briefly tears down and rebuilds the same view doesn't trigger a cold fetch.

### 4.3 Action-based writes

Every write path goes through an **action** — an asynchronous operation that takes an action context (event store, signer, publish function, current user) and produces zero or more signed events that are published and added to the store atomically. The framework ships actions for the common cases: send a note, follow/unfollow a user, update profile, send a DM, zap, repost, react, publish a long-form article, manage lists, configure relays. Actions compose: one action can run another as a sub-action. Custom actions are first-class.

The read/write split is rigid. **Reads happen via store subscriptions. Writes happen via actions.** There is no API that lets a developer "build an event, sign it, publish it, and remember to also update local state." Actions do that atomically and the developer cannot forget the local-state step because it is the action's responsibility, not theirs.

### 4.4 Outbox / smart relay routing (NIP-65)

The framework implements the outbox model **by default and automatically**. Subscriptions with `authors` filters automatically route reads to those authors' write relays. Publishes for an event automatically go to the author's write relays plus inbox relays of any `p`-tagged recipients (for DMs and notifications). The developer does not pick relays per operation; the framework does. They can override, but the override is the exception.

Per-pubkey relay lists are fetched lazily via a gossip layer, cached in a swappable backend (in-memory or SQLite), and refreshed when a fresher kind-10002 arrives. When a user's outbox changes, dependent subscriptions automatically re-resolve their relay sets.

### 4.5 Subscription planner

The actor maintains a **global subscription planner**. Concurrent UI subscriptions with overlapping filters are coalesced into a single REQ on the wire — the kind of work clients typically do manually with hand-rolled grouping windows and dedup LRUs. Subscriptions auto-close when the last consumer drops them and when EOSE arrives if marked as one-shot. The planner buffers high-throughput events into batched UI updates (configurable; default ≤60Hz) to satisfy the RMP bible's constraint against high-frequency FFI loops.

### 4.6 Multi-account sessions

**Sessions are state.** `AppState` carries a vector of accounts and an active pubkey. Each account has a signer reference, a derived profile view, a follow list view, a mute list view, a relay-list view, and a status flag (e.g., loading, syncing, online). Switching the active account is an action; the UI re-renders against the new context with no further work.

Account persistence is automatic via the OS keyring crate (Keychain / Credential Manager / Secret Service). Signers cover the spectrum: private key (encrypted at rest), NIP-49 password-encrypted, NIP-07 browser extension (with a proxy for native apps), NIP-46 bunker (Nostr Connect), and platform-native external signers (e.g. the Android Amber app via NIP-55, a capability bridge in RMP terms).

### 4.7 Web of Trust

The framework includes a **web-of-trust subsystem**: load the follow graph rooted at the active user to a configurable depth, compute per-pubkey trust scores, expose a reactive filter that can be turned on globally to score-rank or score-filter every subscription. This is the kind of feature an app developer would normally never get to ship; the framework ships it.

### 4.8 NIP-77 Negentropy sync

A **high-level synchronization API** wraps NIP-77 negentropy set reconciliation: pick a filter, a relay, and an optional time window, and the framework efficiently brings local state into agreement with the relay's state. Background sync, initial backfill, incremental top-up — all expressible as actions.

### 4.9 Wallet integration

A unified **wallet abstraction** exposes Cashu (NIP-60), Nostr Wallet Connect (NIP-47), nutzaps (NIP-61), and LUD-16 Lightning zaps (NIP-57). The wallet is part of `AppState`: balance is a reactive field, payment status flows through state, pending nutzaps appear in a queue the UI renders directly. The developer sets a wallet implementation; the rest is automatic.

### 4.10 Messaging

A **conversation layer** wraps NIP-17 private DMs (gift-wrapped via NIP-59, encrypted via NIP-44) into a conversation-list and message-list view. Sending a DM is an action; receiving DMs is a subscription. Decryption happens in the Rust core, never in platform code. On mobile this includes background-decryption via a platform-specific notification service extension that calls into the Rust core.

### 4.11 Blossom media

A **media client** for the Blossom protocol (BUD-01/BUD-02), with reactive upload/download status flowing through `AppState` like every other operation.

### 4.12 Developer guardrails

In **debug builds only**, the framework runs runtime checks for the common Nostr-development mistakes: bech32 entities mistakenly passed where hex pubkeys are required, replaceable-event filters with too-large `limit`, subscriptions opened without a cache adapter, missing required fields on events being built, anti-patterns in filter shape. These produce loud, educational errors with actionable fixes. In release builds they compile to nothing. The bar is that an LLM-driven developer or a novice should be unable to ship a broken filter from a debug session.

### 4.13 Testing

The framework ships **test utilities**: a mock relay (already provided by the relay-builder crate), event/key factories with deterministic seeds, simulated time, simulated network failures, snapshot helpers for `AppState`. The core actor is testable by sending it actions and asserting on emitted state snapshots — no networking required.

### 4.14 Scaffolding CLI

A **scaffolding CLI** (`<framework> init`) generates a complete starter project: the Rust core crate, the binding layer (today: hand-rolled C-ABI, CI-frozen at 71 symbols; UniFFI migration deferred to M14 when the write surface stabilizes — see **ADR-0030** (`docs/decisions/0030-uniffi-vs-c-abi.md`)), an iOS SwiftUI app, an Android Compose app, an iced desktop app, a web wasm shell, the `justfile` build orchestrator, an optional Nix flake. The starter app implements login, a timeline, compose, a profile screen, and DMs. It builds and runs on all four platforms immediately. This is modeled directly on RMP's `rmp init`.

---

## 5. Crate layout

The repository is a Cargo workspace plus per-platform shells. The layout below is the long-term workspace shape. v1 publishes only the kernel subset described in [`docs/plan.md`](plan.md); product crates remain placeholders or later milestones until the kernel proves its invariants.

```
<framework>/
├── crates/
│   ├── <framework>-core         # Actor, AppState, AppAction, AppUpdate,
│   │                              # event store, subscription planner, sessions,
│   │                              # outbox routing. Pure Rust, no FFI.
│   ├── <framework>-ffi          # Binding scaffolding. FfiApp object,
│   │                              # AppReconciler callback interface,
│   │                              # state-type carriers across the FFI seam.
│   │                              # TODAY: hand-rolled C-ABI (CI-frozen at 71
│   │                              # symbols). UniFFI migration deferred to M14
│   │                              # when the write surface stabilizes — see
│   │                              # ADR-0030.
│   ├── <framework>-wasm         # wasm-bindgen wrapper for web/Node/RN.
│   ├── <framework>-actions      # Built-in actions: send, follow, profile,
│   │                              # zap, react, repost, list management, DM, etc.
│   ├── <framework>-views        # Derived view types (profile, timeline,
│   │                              # thread, contacts, reactions) and the
│   │                              # view-handle subscription protocol.
│   ├── <framework>-wot          # Web of Trust graph + auto-filter.
│   ├── <framework>-sync         # NIP-77 high-level sync API.
│   ├── <framework>-wallet       # NIP-47/57/60/61 unified wallet.
│   ├── <framework>-messages     # NIP-17 conversation layer.
│   ├── <framework>-blossom      # Blossom client wrapper.
│   ├── <framework>-guardrails   # Debug-build runtime checks.
│   ├── <framework>-testing      # Mock relay, factories, simulated time.
│   └── <framework>-cli          # Scaffolding tool.
├── bindings/
│   ├── swift/                   # Generated Swift bindings, checked in.
│   │                              # TODAY: hand-mirrored Decodables in
│   │                              # KernelBridge.swift; nmp-codegen Swift
│   │                              # emitter is the planned replacement
│   │                              # (ADR-0030 §(b)). UniFFI deferred to M14.
│   ├── kotlin/                  # Generated Kotlin bindings, checked in.
│   │                              # Same pattern as Swift; not yet wired.
│   └── typescript/              # Generated wasm-bindgen TS, checked in.
├── examples/
│   ├── chat-ios/
│   ├── chat-android/
│   ├── chat-desktop/
│   └── chat-web/
├── justfile
└── flake.nix
```

The core crate compiles as `cdylib + staticlib + rlib`. Desktop and CLI consumers link the rlib directly (no FFI). iOS links the staticlib via xcframework. Android links the cdylib via cargo-ndk. Web compiles to wasm32-unknown-unknown via the wasm crate. **One source of truth, four delivery paths.**

---

## 6. Doctrine — the rules the API must make hard to violate

These rules are the framework's identity. They derive from the RMP bible and from the protocol-correctness lessons of the libraries we are synthesizing:

1. **One event store per application.** Singleton enforced at the FFI boundary.
2. **All reads through the store.** No "fetch from relay, return to caller" API exists. Relay results land in the store; callers subscribe to the store.
3. **All writes through actions.** No "build event, sign, publish" sequence the developer assembles manually.
4. **Replaceable-event invariants enforced on insert.** Stale kind-0/3/10002/parameterized-replaceable events are impossible to retain.
5. **Outbox routing automatic.** Manual relay selection is the opt-out, not the default.
6. **Subscriptions auto-group, auto-close, auto-dedup, auto-buffer.** The developer never writes grouping/dedup/cleanup code.
7. **Sessions are state, switching is an action.** No imperative "log out, then log in, then reload" dance.
8. **No errors cross FFI.** All operational failure surfaces as state fields.
9. **No business logic in native code.** Enforced by docs, examples, and an architectural lint where feasible.
10. **Provenance preserved.** Every event in the store remembers which relays delivered it; private events cannot be accidentally republished to public relays.
11. **Capabilities, not callbacks.** Native↔Rust interactions go through bounded, idempotent capability bridges modeled exactly on the RMP bible's pattern.
12. **Snapshots by default, granular updates as optimization.** Start with `AppUpdate::FullState`; add granular `AppUpdate::*` variants only where profiling demands.

---

## 7. Open design questions (must resolve before substantive coding)

1. **State granularity across FFI.** Full-state snapshots are clean but expensive for large stores. Where do we draw the line, and what granular update variants are needed (e.g. `EventAdded`, `ViewChanged { view_id }`, `SessionSwitched`)?
2. **Where do views live?** (a) Materialized in `AppState`, (b) lazy with `ViewHandle` opaque references the UI subscribes to, (c) computed in platform code. Bible rules out (c). Pick between (a) and (b) — leaning (b) for efficiency, but it complicates the FFI surface.
3. **Reactive cross-FFI subscription protocol.** UniFFI gives callback interfaces, not native reactive streams. Swift wants `@Observable`, Kotlin wants `Flow`, JS wants Observables/Promises. Define a single `Subscription` opaque handle + reconciler-style callback that adapts cleanly per platform.
4. **NIP-46 bunker as a capability bridge.** Long-lived, stateful, involves user approval on another device. Needs careful design as an RMP-style capability bridge.
5. **Background notification decryption.** iOS Notification Service Extensions and Android background workers must call into the Rust core for NIP-17 decryption without spinning up the full actor. Likely a smaller "decrypt-only" surface area in a sibling crate.
6. **Frozen offline action queue.** Actions dispatched while offline must persist and replay on reconnect, with correct ordering and timestamping. Where does the queue live — in the actor, in SQLite, in a separate durable channel?
7. **Naming.** Working name only. The eventual name should be memorable, available on crates.io and npm, and not conflict with existing Nostr or Rust-multiplatform projects.

---

## 8. References

- **`rust-multiplatform/rmp`** on GitHub — the architectural anchor. `rmp-architecture-bible.md` is required reading. Quoted commandments in this document are paraphrases of that file's content.
- **`rust-nostr`** workspace on GitHub — the protocol foundation. We depend on its `nostr`, `nostr-database`, `nostr-lmdb`, `nostr-ndb`, `nostr-sqlite`, `nostr-gossip`, `nostr-connect`, `nostr-keyring`, `nostr-blossom`, `nostr-relay-builder`, and `nwc` crates. We **do not** depend on `nostr-sdk`: NMP maintains its own relay transport instead of consuming the SDK's tokio-async relay pool — see **ADR-0022** (`docs/decisions/0022-relay-transport-reimplementation.md`).
- Two pre-existing TypeScript Nostr libraries — intentionally unnamed here — supply the high-level application architecture (event store, models, actions, sessions, outbox routing, NIP-77 sync, wallet, messaging, web-of-trust, developer guardrails) being translated into Rust idiom under the RMP architectural skeleton.

---

## 9. What this document is not

It is not a design document. It is not a roadmap. It does not commit to APIs, file structures beyond the workspace sketch, dependency versions, or scheduling. It defines the **aim** so that subsequent design and implementation work proceeds from shared, durable context. The next session should read this, read the RMP bible, and then begin design work on the items in §7 — in approximately that order.
