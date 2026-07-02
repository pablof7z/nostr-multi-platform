# Project Aim — Rust Multiplatform Nostr Application Framework

## Purpose of this document

This document is the cold-start context for a brand-new working session. Read it before doing anything else. It defines the project's north star, architectural foundation, the bodies of prior work being synthesized, the doctrine the design must enforce, and the crate layout. It does not contain implementation. It contains the *aim*.

---

## 1. The north star

We are designing a **Rust multiplatform framework for building Nostr applications** that ships a single Rust core consumed identically by iOS (SwiftUI), Android (Jetpack Compose), and desktop (egui). The v1 platform contract is iOS, Android, and desktop. Web/wasm is a first-class supported target: OPFS-SQLite persistence (#1007, closed) and the NmpApp-actor-in-Worker browser runtime (nmp-browser-runtime, ADR-0072) have shipped. The core owns all protocol logic, all state, all caching, all relay management, all signing orchestration, all derived views. Platform code is a thin rendering shell.

The framing concern is one sentence: **make it nearly impossible to build a broken Nostr application.** Today, building a Nostr client involves dozens of subtle correctness pitfalls — stale replaceable events, lost subscriptions, wrong relays for wrong events, race conditions between local state and relay state, leaked signing operations, multi-account state desync. The framework's job is to make each of those classes of bug structurally impossible through the safe app-kernel and FFI API: not merely documented as a footgun or caught by a linter, but ruled out by the type system, actor ownership, and public API surface. A capability the sound design cannot express through a typed seam is a design gap to close, not an exception to whitelist. The only mechanically-gated exception is test-only synthetic injection behind a `cfg`/`test-support` gate; external consumers read through the store via a bounded, backpressured cursor (the pull model), never a kernel bypass — see [`docs/escape-hatches.md`](escape-hatches.md).

The success criterion is qualitative: **a developer should be able to one-shot a working Nostr application** — login, timeline, compose, profile, and eventually DMs and wallet — using the framework's CLI scaffold and a few hundred lines of platform UI code, and have it ship with sane defaults on the v1 native platforms without the developer ever touching relay routing, cache invalidation, replaceable-event semantics, or subscription lifecycle. Browser runtime support has shipped; the same one-shot web scaffold claim is gated by browser-shell DX, component-host conformance, and clean-room onboarding proof. The current per-NIP v1 support boundary, including browser signer caveats, lives in [`docs/nips.md`](nips.md). If they don't go out of their way to defeat the framework, the app will be correct.

---

## 2. Architectural foundation: The Elm Architecture

The architectural skeleton follows the **`rust-multiplatform/rmp`** project's design. The load-bearing model is **The Elm Architecture (TEA)**, also called Model-View-Update. Three primitives:

- **`AppState`** — a single struct containing all data the UI needs to render.
- **typed actions / commands** — validated user intent, lifecycle events, and capability completions entering the actor.
- **reducers** — pure update functions that take current state and a message and produce new state plus typed effects.

Data flow is **strictly unidirectional**: user interaction → action dispatch → actor processes synchronously → state emission → platform re-renders. *"No data races. A single actor thread owns all mutable state. No locks, no concurrent mutation, no race conditions."*

The execution model is the **actor pattern**. A dedicated OS thread owns the kernel state and runs a synchronous event loop over typed commands, relay events, and capability completions. Blocking workers and capability handlers report results back into that same loop. Only the actor thread mutates state.

The cross-FFI flow:

```
Native UI calls typed intent helper      [fire-and-forget, never blocks]
  → bridge encodes DispatchEnvelope bytes
  → Actor thread recv()
  → reducer mutates state, increments rev
  → UpdateFrame emitted on update channel
  → Listener thread invokes update callback
  → Native code hops to main/UI thread
  → State replaced via @Observable / mutableStateOf / runes / signals
  → UI re-renders
```

Critical invariants all framework implementation must honor:

1. **Monotonic revision guard.** `rev: u64` increments on every state change. Platforms compare incoming `rev` to last applied and skip stale updates.
2. **Errors do not cross FFI.** Operational errors become `toast: Option<String>` fields in state; long-running operation errors clear `busy` flags. Native `dispatch` calls never need try/catch.
3. **`dispatch()` is fire-and-forget.** No return value. Results come back as state changes.
4. **No native domain logic.** If you would write an `if` statement in Swift or Kotlin that decides what the app should *do* (not how it should *look*), that logic belongs in Rust. Native has exactly three responsibilities:
   - **Render** — translate Rust-produced state snapshots into UI.
   - **Execute capabilities** — call OS APIs (Keychain, AVPlayer, push, location) and report raw results back to Rust. Never decide policy; never retry; never cache.
   - **Hold ephemeral presentation state** — purely local, throwaway UI state that no other platform would have to reimplement to behave correctly: in-flight / optimistic indicators (e.g. a spinner keyed to a dispatched action's correlation id), scroll position, focus, input-buffer text, animation/transition state, and per-platform look choices (icon names, colors, layout). This state never decides protocol behavior and is never the source of truth — it is discarded and rebuilt from the next Rust snapshot.

   **The discriminating test** is *not* "is it logic?" — that question is too broad and sweeps in presentation. It is: **would a second platform have to reimplement this to stay correct?** If yes, it is **domain** logic and belongs in Rust — state, business rules, derived data, routing decisions, error recovery, protocol logic, all of it. If it is only *how this platform shows or stages* something, it is presentation and may live in the shell.

   Apply this in both directions. Do **not** let domain logic leak into the shell. But equally, do **not** push presentation concerns into the core to satisfy an absolutist reading of this rule — forcing icon names, accent colors, or local in-flight tracking through Rust buys no cross-platform reuse and only adds core complexity (and churn when it is later moved back out). The boundary is **domain vs. presentation**, not "native does nothing by default."
5. **Bounded native state.** Native holds only transient OS handles (keychain refs, audio sessions, network monitors). No caches, no derived state, no policy.
6. **Capability bridge pattern.** When Rust needs an OS API (keychain, push, location, external signer app), it requests the capability via a typed callback interface. Native executes and reports raw data. Rust decides policy. Native never decides "should we retry?" or "is this recoverable?"
7. **Idempotent capability lifecycle.** Start/stop/restart of any bridge must be safe.
8. **Avoid the god module.** When the core actor file exceeds ~1,000 lines, split by domain into submodules with `pub(super)` visibility.
9. **No high-frequency FFI loops.** Callbacks above ~60Hz across FFI must be batched or delivered via direct memory, not serialized per event.
10. **Snapshot semantics.** The full `Clone`d snapshot is the canonical baseline shape — it is what a host receives on first frame and on every epoch/session re-baseline. As of ADR-0070 (profiling-driven), per-projection emission is **incremental by default** for any host that advertises rev-aware apply through the public binding capability: unchanged projection rows are omitted from the frame and an NMP-owned generated `ProjectionCache` reconstructs the full set host-side, so app code stays oblivious. Omission is gated on that capability; a host that does not advertise it still gets full rows. This is realized for Tier-2 kernel projections; Tier-1 feed-class projections now also support opt-in omit-unchanged via ADR-0070 Rung 6 (shipped, `crates/nmp-core/src/projection_emission.rs`). Measured (S6 capstone): ~18% frame-byte reduction + 68.8% Tier-2 row suppression with zero data loss on the churn workload.

The reference crate layout from `rust-multiplatform/rmp` — `rust/` for the core (cdylib + staticlib + rlib), `uniffi-bindgen/` for the binding generator, `ios/`, `android/`, `crates/<app>-desktop/`, a `justfile` for build orchestration, an optional Nix flake — is the layout this framework will scaffold for users. We adopt RMP's `rmp-cli` as the model for our own scaffolding tool.

Anti-patterns the framework must prevent:

- Presentation formatting in the backend — Rust sends raw data: pubkeys as hex, timestamps as Unix integers, display names verbatim from kind:0 (no truncation, no fallback-npub substitution). Presentation layers (Swift, Kotlin, TypeScript, TUI) own all formatting decisions: how to truncate a pubkey, how to display a timestamp, what to show when kind:0 is absent. Rust display helpers (`short_npub`, `avatar_initials`, `avatar_color_hex`, `format_ago_secs`, etc.) are legitimate only in TUI render code, CLI output, and test fixtures — never inside projection builders, snapshot types, or FFI serialization paths.
- Business logic in ViewState derivation — derivation should be field renames and type conversions only.
- Navigation state leaking to native — Rust's router is the single source of truth.
- Native-side caches of derived values — caching lives in Rust.
- Capability bridge scope creep — bridges report, they do not decide.

These are not best practices. They are constraints the framework's public API must make difficult to violate.

---

## 3. Protocol foundation: existing Rust primitives

The framework does not reimplement the Nostr protocol. The Rust ecosystem already has a mature, modular set of protocol crates that we wrap and orchestrate:

- A **protocol crate** providing `Event`, `EventBuilder`, `Filter`, `Keys`, `Tag`, all NIP-defined types, bech32 encoding, NIP-19 entities, no_std support, and around 60 implemented NIPs.
- A **client/SDK crate** (`nostr-sdk`) providing `Client`, relay pool management, subscription routing, async streaming over tokio. **NMP does not use this crate.** Its relay pool is tokio-async and reference-counted; NMP's kernel is a single synchronous actor (§2). NMP instead depends on the `nostr` protocol crate for types/crypto and maintains its own relay transport (`crates/nmp-core/src/relay_worker/`, raw `tungstenite`) shaped to the actor model — generational relay handles, idle-tick-gated `recv_timeout`, interest-lattice subscription coalescing. See **ADR-0072** (`docs/decisions/0072-runtime-capability-and-shell-boundary.md`) for the full rationale.
- A **database trait** with multiple swappable backends: in-memory, LMDB, nostrdb, SQLite (native and WASM via OPFS-SQLite).
- A **gossip/outbox trait** with in-memory and SQLite backends, implementing the NIP-65 relay-list metadata model and per-pubkey relay discovery.
- A **NIP-46 (Nostr Connect / bunker) signer crate** for remote signing.
- A **NIP-07 browser signer crate** plus a native-side proxy to use NIP-07 from desktop/mobile.
- An **OS keyring crate** wrapping macOS Keychain, Windows Credential Manager, and Secret Service.
- Post-v1 **NIP-47 NWC client mechanics** for wallet operations. NMP v1 does
  not claim wallet product support.
- A **Blossom client crate** for media storage.
- A **relay builder crate** providing `LocalRelay` (full in-process relay) and `MockRelay` (ephemeral, for tests).

These crates are **dependencies, not forks**. The framework's job is to compose them into an opinionated whole; their authors do the protocol correctness work, we do the application-layer work. Where they have gaps relative to the framework's goals (reactive queries, models, sessions, web-of-trust, opinionated outbox routing on every operation), the framework adds those layers above — it does not push them down into the protocol layer.

---

## 4. High-level functionality being synthesized

Two existing TypeScript libraries in the broader Nostr ecosystem, **NDK** and **Applesauce**, have between them many of the high-level patterns a polished Nostr client framework needs. This Rust framework is a deliberate synthesis of the useful lessons from both. The functionality below is not invented from scratch; our work is to translate the right pieces into Rust + RMP idiom.

The translation is selective. Applesauce is a strong reference for reactive event stores, derived models, fallback loaders, action runners, relay adapters, and product-layer packages, but its RxJS streams, mutable symbol metadata, and browser-first API surface are not the architecture we ship. NDK is a strong reference for relay pools, cache adapters, subscription grouping, per-relay provenance, sessions, sync, wallet, Blossom, WoT, and messaging modules, but NMP should avoid growing one monolithic cache trait or embedding product policy in the v1 kernel.

The architectural delta is the core idea of this project: use the Rust Nostr SDK family for protocol primitives, then build a new Rust application kernel above it. We are not forking the Rust SDK and we are not porting Applesauce or NDK APIs. We are creating the missing multiplatform app layer: actor-owned state, bounded FFI projections, canonical store semantics, subscription and action lifecycle, storage/metrics/test harnesses, and extension seams for later product modules. The subsections below describe the target capability set; [`docs/nips.md`](nips.md) is the release-status source for what is supported, partial, experimental, blocked, or post-v1 today.

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

Every write path goes through an **action/publish workflow** — an asynchronous operation that takes an action context (event store, signer, publish function, current user) and produces zero or more signed events that are published and added to the store atomically. The framework's action model covers the common cases: send a note, follow/unfollow a user, update profile, send a DM, repost, react, publish a long-form article, manage lists, configure relays, and post-v1 wallet surfaces such as zaps. Current v1 support varies by NIP and platform; consult [`docs/nips.md`](nips.md) before treating any one of those actions as a complete product surface. Actions compose: one action can run another as a sub-action. Custom actions are first-class.

The read/write split is rigid. **Reads happen via store-backed typed sessions. Writes happen via actor-owned workflows.** Apps may compose unsigned event drafts through helpers such as "reply to this event", "react to this event", or "new article"; the unmanaged sequence NMP forbids is "build an event, sign it, publish it, choose relays, and remember to update local state." NMP owns finalization, signing, route policy, local ingest, retry/status, and the terminal result.

### 4.4 Outbox / smart relay routing (NIP-65)

The framework implements the outbox model **by default and automatically**. Subscriptions with `authors` filters automatically route reads to those authors' write relays. Publishes for an event automatically go to the author's write relays plus inbox relays of any `p`-tagged recipients (for DMs and notifications). The developer does not pick relays per operation; the framework does. They can override, but the override is the exception.

Per-pubkey relay lists are fetched lazily via a gossip layer, cached in a swappable backend (in-memory or SQLite), and refreshed when a fresher kind-10002 arrives. When a user's outbox changes, dependent subscriptions automatically re-resolve their relay sets.

### 4.5 Subscription planner

The actor maintains a **global subscription planner**. Concurrent UI subscriptions with overlapping filters are coalesced into a single REQ on the wire — the kind of work clients typically do manually with hand-rolled grouping windows and dedup LRUs. Subscriptions auto-close when the last consumer drops them and when EOSE arrives if marked as one-shot. The planner buffers high-throughput events into batched UI updates (configurable; default ≤60Hz) to satisfy the D8 constraint against high-frequency FFI loops.

### 4.6 Multi-account sessions

**Sessions are state.** `AppState` carries a vector of accounts and an active pubkey. Each account has a signer reference, a derived profile view, a follow list view, a mute list view, a relay-list view, and a status flag (e.g., loading, syncing, online). Switching the active account is an action; the UI re-renders against the new context with no further work.

Account persistence is automatic via OS keychain-style capabilities where a native shell provides them. Signer support is deliberately backend-specific: local keys, NIP-46 bunker/Nostr Connect, browser NIP-07, and Android NIP-55 each have different platform and NIP-44 capability boundaries. The supported matrix and caveats are tracked in [`docs/nips.md`](nips.md), not inferred from upstream protocol-crate features.

### 4.7 Web of Trust

The framework includes a **web-of-trust subsystem**: load the follow graph rooted at the active user to a configurable depth, compute per-pubkey trust scores, expose a reactive filter that can be turned on globally to score-rank or score-filter every subscription. This is the kind of feature an app developer would normally never get to ship; the framework ships it.

### 4.8 NIP-77 Negentropy sync

A **high-level synchronization API** wraps NIP-77 negentropy set reconciliation: pick a filter, a relay, and an optional time window, and the framework efficiently brings local state into agreement with the relay's state. Background sync, initial backfill, incremental top-up — all expressible as actions.

### 4.9 Wallet integration

The target wallet abstraction is Rust-owned and spans Nostr Wallet Connect (NIP-47), LUD-16 Lightning zaps (NIP-57), and later Cashu/nutzap work (NIP-60/NIP-61). This is not v1 scope: NIP-47/NWC, NIP-57/zaps, and NIP-60/NIP-61 are post-v1; see [`docs/nips.md`](nips.md), [#2318](https://github.com/pablof7z/nostr-multi-platform/issues/2318), and [#1001](https://github.com/pablof7z/nostr-multi-platform/issues/1001).

### 4.10 Messaging

The target conversation layer wraps NIP-17 private DMs (gift-wrapped via NIP-59, encrypted via NIP-44) into conversation-list and message-list views. Current v1 support is partial and signer-dependent: enabled private-message paths keep plaintext inside Rust, while browser extension support depends on `window.nostr.nip44` and NIP-46 delegated decrypt backfill remains staged. See [`docs/nips.md`](nips.md), [#2255](https://github.com/pablof7z/nostr-multi-platform/issues/2255), and [#1259](https://github.com/pablof7z/nostr-multi-platform/issues/1259).

### 4.11 Blossom media

A **media client** for the Blossom protocol (BUD-01/BUD-02), with reactive upload/download status flowing through `AppState` like every other operation.

### 4.12 Developer guardrails (post-v1 target)

Developer guardrails are a post-v1 target: debug-build-only checks for common
Nostr-development mistakes such as bech32 entities passed where hex pubkeys are
required, replaceable-event filters with too-large `limit`, missing cache
coverage, incomplete event drafts, and broad filter shapes. The intended bar is
that an LLM-driven developer or a novice should be unable to leave a debug
session with a broken filter. v1 currently relies on typed APIs, doctrine
gates, targeted tests, and perf gates; a general guardrails crate must not be
documented as shipped until it exists.

### 4.13 Testing

The framework ships **test utilities**: a mock relay (already provided by the relay-builder crate), event/key factories with deterministic seeds, simulated time, simulated network failures, snapshot helpers for `AppState`. The core actor is testable by sending it actions and asserting on emitted state snapshots — no networking required.

### 4.14 Scaffolding CLI

A **scaffolding CLI** (`<framework> init`) generates a complete starter project: the Rust core crate, the native binding layer (UniFFI over `nmp-native-runtime`), an iOS SwiftUI app, an Android Compose app, a desktop app, the `justfile` build orchestrator, and an optional Nix flake. Browser shells are built on `nmp-browser-runtime` through wasm-bindgen; one-shot web scaffold generation is gated by browser-shell DX and component-host conformance, not by a missing runtime. The v1 starter prioritizes login, timeline, compose, and profile flows; DM and wallet starter claims must follow the support matrix rather than the north-star target text. It builds and runs on the v1 native platforms (iOS, Android, desktop) immediately. This is modeled directly on RMP's `rmp init`.

---

## 5. Crate layout

The repository is a Cargo workspace plus per-platform shells. The layout below is the long-term workspace shape. v1 publishes only the kernel subset tracked in GitHub Issues; product crates remain placeholders or later milestones until the kernel proves its invariants.

```
<framework>/
├── crates/
│   ├── <framework>-core         # Actor, AppState, typed commands, UpdateFrame,
│   │                              # event store, subscription planner, sessions,
│   │                              # outbox routing. Pure Rust, no FFI.
│   ├── <framework>-uniffi       # Public native binding scaffolding over the
│   │                              # native runtime: app handle, update sink,
│   │                              # typed byte dispatch, capabilities, and
│   │                              # state-type carriers across the binding seam.
│   ├── <framework>-browser-runtime
│   │                              # wasm-bindgen Worker export + browser runtime.
│   ├── <framework>-actions      # Built-in actions: send, follow, profile,
│   │                              # react, repost, list management, DM, etc.
│   ├── <framework>-views        # Derived view types (profile, timeline,
│   │                              # thread, contacts, reactions) and the
│   │                              # view-handle subscription protocol.
│   ├── <framework>-wot          # Web of Trust graph + auto-filter.
│   ├── <framework>-sync         # NIP-77 high-level sync API.
│   ├── <framework>-wallet       # Post-v1 NIP-47/57/60/61 wallet mechanics.
│   ├── <framework>-messages     # NIP-17 conversation layer.
│   ├── <framework>-blossom      # Blossom client wrapper.
│   ├── <framework>-guardrails   # Post-v1 debug-build runtime checks.
│   ├── <framework>-testing      # Mock relay, factories, simulated time.
│   └── <framework>-cli          # Scaffolding tool.
├── bindings/
│   ├── swift/                   # Generated Swift bindings, checked in.
│   │                              # UniFFI native bindings plus generated
│   │                              # FlatBuffers decoders/action builders.
│   ├── kotlin/                  # Generated Kotlin bindings, checked in.
│   │                              # UniFFI native bindings plus generated
│   │                              # FlatBuffers decoders/action builders.
│   └── typescript/              # Generated wasm-bindgen TS, checked in.
├── examples/
│   ├── chat-ios/
│   ├── chat-android/
│   ├── chat-desktop/
│   └── chat-web/
├── justfile
└── flake.nix
```

The core crate compiles as `cdylib + staticlib + rlib`. Desktop and CLI consumers link the rlib directly (no FFI). iOS links the staticlib via xcframework. Android links the cdylib via cargo-ndk. Web compiles to wasm32-unknown-unknown through `nmp-browser-runtime`; OPFS-SQLite persistence (#1007) and the NmpApp-actor-in-Worker browser runtime (nmp-browser-runtime, ADR-0072) have shipped. **One source of truth; v1 delivery = iOS, Android, desktop (egui), and browser runtime support; full one-shot web scaffold/proof parity is gated by browser-shell DX.**

---

## 6. Doctrine — the rules the API must make hard to violate

These rules are the framework's identity. They derive from the TEA + actor model and from the protocol-correctness lessons of the libraries we are synthesizing:

1. **One event store per application.** Singleton enforced at the FFI boundary.
2. **All reads through the store.** No "fetch from relay, return to caller" API exists. Relay results land in the store; callers subscribe to the store.
3. **All writes through actor-owned workflows.** Apps may compose unsigned drafts, but NMP owns finalization, signing, routing, local ingest, retry/status, and terminal result.
4. **Replaceable-event invariants enforced on insert.** Stale kind-0/3/10002/parameterized-replaceable events are impossible to retain.
5. **Outbox routing automatic.** Manual relay selection is the opt-out, not the default.
6. **Subscriptions auto-group, auto-close, auto-dedup, auto-buffer.** The developer never writes grouping/dedup/cleanup code.
7. **Sessions are state, switching is an action.** No imperative "log out, then log in, then reload" dance.
8. **No errors cross FFI.** All operational failure surfaces as state fields.
9. **No business logic in native code.** Enforced by docs, examples, and an architectural lint where feasible.
10. **Provenance preserved.** Every event in the store remembers which relays delivered it; private events cannot be accidentally republished to public relays.
11. **Capabilities, not callbacks.** Native↔Rust interactions go through bounded, idempotent capability bridges modeled on the same capability-bridge pattern.
12. **Snapshot is the baseline; incremental emission is the profiled default.** The full snapshot remains the canonical baseline/resync shape. Per-projection incremental emission — omit-unchanged on the wire, NMP-owned rev-aware reconstruction host-side (durable rule owned by `docs/product-spec/doctrine.md`: "Under incremental apply, omitted keys mean retain cached state and an explicit `Cleared` row means drop it") — is the default once a host advertises rev-aware apply, exactly the "add granular updates where profiling demands" this doctrine called for, now driven by the S6 measurement. The granular path must never require app code to handle deltas: the generated `ProjectionCache` makes incremental-apply impossible for an app developer to get wrong.

---

## 7. References

- **`rust-multiplatform/rmp`** on GitHub — the architectural anchor. This framework's TEA + actor model and crate layout follow its design.
- **`rust-nostr`** workspace on GitHub — the protocol foundation. We depend on its `nostr`, `nostr-database`, `nostr-lmdb`, `nostr-ndb`, `nostr-sqlite`, `nostr-gossip`, `nostr-keyring`, `nostr-blossom`, `nostr-relay-builder`, and `nwc` crates. We **do not** depend on `nostr-sdk` (own relay transport — see **ADR-0072**) or `nostr-connect` (own NIP-46 broker — see **ADR-0072** `docs/decisions/0072-runtime-capability-and-shell-boundary.md`).
- Two pre-existing TypeScript Nostr libraries — intentionally unnamed here — supply the high-level application architecture (event store, models, actions, sessions, outbox routing, NIP-77 sync, wallet, messaging, web-of-trust, developer guardrails) being translated into Rust idiom under the RMP architectural skeleton.

---

## 8. What this document is not

It is not a design document. It is not a roadmap. It does not commit to APIs, file structures beyond the workspace sketch, dependency versions, or scheduling. It defines the **aim** so that subsequent design and implementation work proceeds from shared, durable context.
