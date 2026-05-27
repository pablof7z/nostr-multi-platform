# Product Spec: Overview And Developer Experience

[Back to Product Specification - Nostr Multi-Platform Framework](../product-spec.md)

# Product Specification — Nostr Multi-Platform Framework

> **Working name:** `nmp` (Nostr Multi-Platform). Final name TBD per `aim.md` §7.7. Crate names below use the `nmp-*` prefix; substitute when renamed.

> **Status:** Draft 0, revised for ADR-0009 and ADR-0010. The kernel/module split is now architectural ground truth; product modules still graduate by the phased plan in [`docs/plan.md`](../plan.md).

> **Required prior reading:** `docs/aim.md`.

---

## 1. Product summary

The framework treats common Nostr-correctness failures — stale replaceable events, lost subscriptions, mis-routed publishes, double-publication, multi-account desync, leaked secrets across FFI, naive cache invalidation, withheld cached data, blocking-on-fetch UI patterns — as **product defects in the framework** rather than as developer mistakes. The public API is designed so that the wrong thing is hard to type.

NMP is a Cargo workspace shipping a Nostr-native **app kernel** (`nmp-core`), reusable **Nostr protocol modules** (`nmp-nip01`, `nmp-nip17`, `nmp-nip65`, etc.), app-owned extension modules, a codegen tool (`nmp gen modules`) that produces per-app concrete FFI enums/wrappers, FFI bindings for Swift/Kotlin/TypeScript, a wasm target, a scaffolding CLI, and reference platform shells.

The kernel composes the `rust-nostr` crate family plus OS capability crates into a substrate. It owns actor runtime, verified event store, subscription planner, relay routing pipeline, signer/session plumbing, durable action ledger, domain-store substrate, typed view registry, capability bridge, platform shadow/codegen machinery, diagnostics, and test harnesses.

The kernel does **not** own Profile, Timeline, Thread, Reactions, Conversation, Wallet, DM, Blossom, or app-specific domain concepts. Those live in reusable protocol modules or app crates. Platform code renders state and dispatches user intents — nothing else.

---

## 1.5 Cardinal doctrines D0–D10

See [`docs/product-spec/doctrine.md`](./doctrine.md) for the full text of all eleven doctrines.

Summary: D0–D5 and D10 are *policy* doctrines (user-facing semantics); D6–D9 are *substrate invariants* (runtime implementation constraints). Both are equally binding. Every API decision answers to at least one; conflicts resolve in the order D0 > D1 > … > D10.

| # | Name | Kind |
|---|------|------|
| D0 | No app nouns in `nmp-core`; test surface gated behind `test-support` feature | Policy |
| D1 | Best-effort rendering — render now, refine in place | Policy |
| D2 | Negentropy first, REQ second | Policy |
| D3 | Outbox routing automatic; manual relay selection is the opt-out | Policy |
| D4 | Single writer per fact; caches derive | Policy |
| D5 | Snapshots bounded by what's open | Policy |
| D6 | Errors never cross FFI as exceptions | Substrate |
| D7 | Capabilities report; never decide policy | Substrate |
| D8 | Reactivity contract: composite reverse index · ≤60 Hz/view · working-set bounded; idle-tick emit gated on `changed_since_emit()` | Substrate |
| D9 | The kernel owns time — signing, replaceable resolution, and NIP-40 expiration are kernel decisions read through the injected `Clock` | Substrate |
| D10 | Provenance — private events (kind:1059 gift-wrap, NIP-17 DM rumors) never escape to public relays; received events are not laundered between relays | Policy |

---

## 1.6 API design philosophy — pit-of-success principles

The doctrines D0–D10 are the *rules* the framework enforces. The principles below are the *design stance* those doctrines exist to serve. They are named so that "is this API hard to misuse?" can be asked and answered with a shared vocabulary, not adjudicated case-by-case.

The stance, in one sentence: **it should be hard to fuck up, there should be one way of doing things, it should work reliably, and it should be really hard to do something really stupid or annoying.** Code review, ADRs, and module proposals cite these principles by label.

### P1. One happy path

For each operation the framework supports, there is exactly one public-API way to do it. Reads happen through store subscriptions; writes happen through actions; sessions switch by dispatching a session action; relay selection is automatic. There is no "alternative" API surface that bypasses the canonical one — alternatives are escape hatches, not parallel happy paths.

- **Enforced by:** D0 (typed module seams, not freelance APIs), D3 (manual relay selection is the opt-out), the read/write split in `aim.md` §4.3.
- **Escape hatch:** Action Module Seam and Raw Event Tap (`docs/escape-hatches.md` §3, §1) — both named, instrumented, opt-in.

### P2. Structural prevention over documented footguns

Bad patterns are not banned in prose; they are made *hard to type*. View payload fields are non-`Option` so a developer cannot accidentally render a spinner over cached data (D1). Errors are not in the FFI signature so a developer cannot write `try/catch` glue around `dispatch` (D6). Relay URLs do not appear in the safe publish path so a developer cannot route a DM to a public relay (D3, D10). If a doctrine relies on the developer reading a comment, the API surface is wrong.

- **Enforced by:** D0, D1, D3, D6, D7, D10, and `aim.md` §1 ("ruled out by the type system, actor ownership, and public API surface").
- **Test:** §3.3 bug-class extinction — each class regression-tested in `crates/nmp-testing`.

### P3. Reliable by default; correctness is not opt-in

The default behavior of every safe API is the correct behavior. Outbox routing is on, replaceable supersession is on, subscription dedup is on, idle-tick gating is on, provenance is recorded, NIP-40 expiration is honored — none of these require the developer to enable them. The framework never ships a default that requires a follow-up call to make safe.

- **Enforced by:** D2 (negentropy is default backfill), D3 (outbox is default routing), D4 (one writer, derived caches are automatic), D9 (kernel clock is injected, not opt-in), D10 (provenance is recorded on every insert).
- **Inverse smell:** any API whose docs say "remember to call X after" is a P3 violation.

### P4. Graceful degradation; never all-or-nothing

Apps function with partial data, partial connectivity, partial relay availability, partial profile metadata, partial follow graphs. There is no bootstrap phase that gates rendering on completeness. This is the user-visible face of D1 ("render now, refine in place") generalized beyond display fields to every dimension of partial state: offline composes queue and replay; an unknown recipient inbox surfaces as diagnostic state, never as a crash or a silent drop; an empty cache renders placeholders, never a blank screen.

- **Enforced by:** D1 (display fields are typed, not optional), D2 (gaps fill via negentropy without re-architecting reads), D8 (working-set bounded — degradation does not unbound memory).
- **Inverse smell:** any code path with a `Loading` state that hides already-renderable data.

### P5. The framework absorbs protocol complexity

Relay routing, replaceable-event resolution, gift-wrap encryption, EOSE handling, REQ/CLOSE bookkeeping, negentropy reconciliation, subscription coalescing, kind-5 delete propagation, and NIP-40 expiration are *invisible* to app code. The developer-never-has-to-do list (§5.3) is the operational statement of this principle; if a new feature would require the developer to learn a protocol subtlety to use it correctly, the feature is incomplete.

- **Enforced by:** the kernel substrate (`aim.md` §2), D2, D3, D4, D8, and the exhaustive §5.3 list.
- **Inverse smell:** a recipe in `docs/recipes/` that teaches the developer to handle a protocol-layer concern rather than to call a framework method.

### Reviewing against the principles

A change passes pit-of-success review when:

1. It does not introduce a second happy path for an operation the framework already supports (P1).
2. The wrong usage is hard to type, not just documented as discouraged (P2).
3. Its default behavior is the correct behavior; correctness is not gated on a follow-up call (P3).
4. It degrades gracefully under partial data / connectivity / cache (P4).
5. It does not push a protocol subtlety into app code (P5).

A change that cannot satisfy all five is either an escape hatch (named in `docs/escape-hatches.md`) or a framework defect.

---

## 2. Audience and use cases

**Primary audience.** Application developers building Nostr clients for production distribution on iOS, Android, desktop, and web — including LLM-driven developers and teams who want to ship correct Nostr apps without becoming Nostr protocol experts first.

**Secondary audience.** Existing Nostr client teams considering a port to Rust + multi-platform, who want a substrate they can compose rather than reimplement.

**Tertiary audience.** Tooling, agent, and bot authors who want the framework's event store + actions + sync as a headless Rust library, without UI.

**In scope.**

- General-purpose social clients (timeline, threads, profiles, follows, reactions, reposts, quotes).
- DM-first messengers (NIP-17 over NIP-44 + NIP-59).
- Long-form publishing tools (NIP-23).
- Wallets and zap-centric apps (NIP-47 / NIP-57 / NIP-60 / NIP-61).
- Media-heavy clients (Blossom BUD-01/02).
- List managers and curation tools.

**Out of scope for v1.**

- Relay implementations (we depend on `relay-builder` for tests; we do not ship a production relay).
- New NIP authorship.
- Game engines, AR, low-latency audio/video pipelines (voice/video calls are out of scope for v1).
- Non-Nostr protocol support (Bluesky, ActivityPub, etc.).

---

## 3. Success criteria

Acceptance is **demonstrable, not aspirational**. A claim that the framework works is provable by running these:

### 3.1 Zero-to-running starter

```bash
nmp init my-app
cd my-app && just run-ios   # works
just run-android            # works
just run-desktop            # works
just run-web                # works
```

Result on each platform: a starter app with login (private key + NIP-46 bunker), a "following" timeline, compose, profile view, profile edit, and a DM inbox + thread. End-to-end build + first launch ≤ 5 minutes on a developer laptop with the framework's `nix develop` shell active, ≤ 15 minutes from cold without Nix.

### 3.2 The "few hundred lines" test

Across the four platform shells of the starter app, total non-generated platform code must fit within these budgets (excluding asset declarations and boilerplate `main`):

| Platform | Budget (LOC, hand-written) |
|----------|----------------------------|
| iOS (SwiftUI) | ≤ 400 |
| Android (Compose) | ≤ 400 |
| Desktop (iced) | ≤ 600 (iced is more verbose than SwiftUI/Compose) |
| Web (wasm + TS/JSX shell) | ≤ 400 |

Exceeding any budget is a framework-design failure: it means rendering logic is being forced to compensate for missing surface in the core.

### 3.3 Bug class extinction

Each of these classes must be structurally impossible to introduce through the safe framework public API. Lower-level Rust escape hatches used for tests or internal policy modules must be named, instrumented, and regression-tested. Each bug class below is paired with a regression test in `crates/nmp-testing`.

1. Stale replaceable event (kind 0/3/10000-19999/30000-39999) retained in state after a newer one arrives.
2. Subscription leaked after its UI is destroyed.
3. Publish of an event to relays the author has not declared as write relays, without explicit override.
4. DM published to public relays.
5. Two account contexts having overlapping mutable state.
6. Cache miss returning empty without triggering a fallback fetch.
7. Profile-edit action that updates the local cache but fails to publish (or vice versa).
8. Two concurrent UI subscriptions for the same filter producing two relay REQs.
9. NIP-46 signing session lost on app suspend/resume without prompt.
10. Re-published event missing its original `id` due to re-signing.

Each test asserts the framework refuses the broken usage (compile-time, type-system, or runtime panic in debug) or routes around it transparently.

### 3.4 LLM-friendliness

A novice or LLM-driven developer, given only `docs/aim.md`, `docs/product-spec.md`, the generated bindings, and the starter app, can implement a new screen (e.g., "show all kind-1 events tagged with a given hashtag") that:

- compiles on first try with no edits to the core,
- correctly routes to the right relays automatically,
- correctly closes its subscriptions when navigated away from,
- correctly handles cache misses and live updates.

We treat this as a property of the spec: if it fails repeatedly with capable LLMs, the API surface is wrong, not the LLM.

### 3.5 Cross-platform consistency

A scripted action sequence (defined in `crates/nmp-testing`) run against the starter app on all four platforms produces byte-identical decoded `AppState` snapshots after each action. The runtime transport is FlatBuffers; JSON may be used only as a deterministic test/export representation for comparison. Divergence is a framework defect, not a platform issue.

---

## 4. Deliverables

### 4.1 Workspace

The on-disk layout from `aim.md` §5 is canonical. The long-term workspace contains the following crates as published artifacts on crates.io:

| Crate | Role | FFI? |
|---|---|---|
| `nmp-core` | Kernel substrate: actor, store, planner, ledger, registries, extension traits, diagnostics | Pure Rust |
| `nmp-codegen` | `nmp gen modules`; produces per-app concrete enums and wrappers | Binary + library |
| `nmp-ffi` | UniFFI building blocks used by generated app crates | UniFFI |
| `nmp-wasm` | wasm-bindgen building blocks used by generated app crates | wasm-bindgen |
| `nmp-nip01` | Event, Filter, Profile/Timeline views, SendNote/Delete actions | Pure Rust |
| `nmp-nip02` | Contacts view convenience module | Pure Rust |
| `nmp-nip10` | Reply marker/thread modules | Pure Rust |
| `nmp-nip17` | Conversation view and SendDm action | Pure Rust |
| `nmp-nip25` | Reactions view and React action | Pure Rust |
| `nmp-nip65` | Mailboxes view and outbox routing helpers | Pure Rust |
| `nmp-nip77` | Negentropy sync module | Pure Rust |
| `nmp-blossom` | Blossom upload action and upload view | Pure Rust |
| `nmp-wot` | Web-of-trust graph + filter | Pure Rust |
| `nmp-guardrails` | Debug-build runtime checks | Pure Rust |
| `nmp-metrics` | Performance instrumentation (counters, budgets, exposed via `AppState.debug`) | Pure Rust |
| `nmp-testing` | Mock relay, factories, simulated time, perf-replay harness | Pure Rust |
| `nmp-cli` | Scaffolding tool | Binary |

The CLI is also published to npm as `@nmp/cli` for non-Rust developers, wrapping the same binary via npx.

The v1 release does **not** ship every module above as a finished product module. Per [`docs/plan.md`](../plan.md), v1 first proves the kernel substrate and codegen with a non-Nostr fixture module, then grows Chirp from the first social baseline into NMP's reference client. Any reusable module that does ship should be demonstrated in Chirp, or carry a documented platform exception.

### 4.2 Bindings

Generated bindings are **checked into git** under `bindings/{swift,kotlin,typescript}/`. Developers consuming the workspace as a path dependency do not need a host build to regenerate. The CI lane regenerates and diffs on every PR touching FFI-exposed types; binding drift fails the build.

### 4.3 Starter app

The CLI scaffolds a complete starter project. Behavior is detailed in §8.

### 4.4 Examples

`examples/chat-{ios,android,desktop,web}` track the starter app but include richer features (groups via NIP-29, zaps end-to-end, Blossom uploads, NIP-46 bunker pairing) and serve as the canonical "what does production-grade integration look like" reference for each platform.

### 4.5 The proof app (`nmp-proof`)

A kitchen-sink stress-test app, built using the framework, on all four platforms. It is **not** the starter app — the starter stays minimal so newcomers can read it. The proof app exists to validate the framework at scale and to gate v1 release.

Feature set:

- Multi-account login (3 signer kinds), 5 simultaneous accounts visible in a switcher.
- Following timeline subscribed to a user with 1,000+ follows.
- Hashtag firehose subscribed to a high-throughput tag (e.g., `#nostr`).
- Thread view rendering a controversial event with hundreds of replies + reactions + zaps.
- Search over the local store.
- DM inbox with 50+ active conversations (NIP-17 gift-wrapped).
- Long-form reader (NIP-23).
- Wallet operations: NWC + Cashu + zaps in both directions + nutzap claim.
- Blossom upload + view.
- Background sync via NIP-77 negentropy on foreground.
- Web-of-trust toggle visibly reordering the timeline.
- Offline queue: airplane mode → compose → reconnect → publishes land.

The proof app also ships a **performance overlay** (toggleable, debug-build default-on) rendering the live counters and budgets from §7.16. The overlay is implemented entirely in platform code reading from `AppState.debug` — no Rust-side UI logic.

The proof app is the substrate for cross-platform consistency tests (§3.5): the same scripted action sequence runs against the proof app on all four platforms and decoded `AppState` snapshots must match. Any JSON in that harness is a comparison artifact, not the runtime update transport.

### 4.6 Documentation set

| Document | Purpose | Owner |
|---|---|---|
| `docs/aim.md` | North star | Stable |
| `docs/product-spec.md` | This doc | Stable, versioned |
| `docs/design/*.md` | Per-subsystem design docs (filled in by the next session) | Iterates |
| `docs/recipes/*.md` | How to build common features (timeline, thread, zap, DM, group) | Iterates |
| `docs/nips.md` | NIP support matrix with version pins | Iterates |
| `docs/migration.md` | Upgrade guidance per minor/major | Iterates |

The architectural foundation stays upstream at `rust-multiplatform/rmp`; we link, not vendor.

---

## 5. Developer experience

### 5.1 The path from nothing to a running app

```
$ npx @nmp/cli init relay-cat
? Organization (reverse-DNS): com.example
? Platforms: ◉ iOS  ◉ Android  ◉ Desktop  ◉ Web
? Storage backend (default for non-web): ◉ LMDB  ○ SQLite  ○ nostrdb  ○ In-memory
? Web storage backend: ◉ IndexedDB (default)  ○ OPFS
? Default relays (comma-separated): wss://relay.damus.io,wss://nos.lol
? Wallet: ◉ NWC  ○ Cashu  ○ None
? Signers to include: ◉ Local key  ◉ NIP-46 bunker  ◉ NIP-07 (web)  ◉ Amber (Android)
? Use Nix flake: Yes
✓ Scaffolded relay-cat in ./relay-cat
```

Then:

```
$ cd relay-cat && nix develop
$ just run-desktop      # native window opens to login screen in ~30s
$ just run-ios          # simulator boots, app launches
$ just run-android      # emulator boots, app launches
$ just run-web          # vite dev server on :5173
```

The starter app on first launch presents login. Logging in with a private key or pairing a bunker yields a working following-timeline with live updates, compose, profile, and DMs.

### 5.2 The platform-developer's day

After scaffold, the developer's loop is:

1. Touch SwiftUI/Compose/iced/TSX files in the platform shell.
2. Touch action variants in `app/src/actions.rs` and action handlers in `app/src/core/` for new features.
3. Touch view definitions in `app/src/views/` to add new derived views.
4. `just gen-bindings` after changing FFI-visible types.
5. Re-run.

The developer should not be writing relay code, subscription bookkeeping, cache invalidation, or replaceable-event handling. Ever. If they catch themselves doing so, that is the symptom of either a missing built-in or a framework bug.

### 5.3 What the developer never has to do

Concrete list, exhaustive:

- Pick relays per subscription or publish (outbox handles it).
- Implement REQ/CLOSE bookkeeping.
- De-duplicate events across relays.
- Track replaceable-event supersession.
- Wire a kind-5 delete event into their UI state.
- Wire NIP-40 expiration into their UI state.
- Persist signed events anywhere other than via actions.
- Encrypt/decrypt DMs.
- Wrap/unwrap NIP-59 gift wraps.
- Schedule background relay reconnection.
- Cache profile metadata.
- Maintain a follow-graph cache.
- Implement zap receipt verification.
- Implement NWC request/response correlation.
- Implement Blossom upload chunking.
- Hop to main thread on platform — the framework's reconciler emits hints; the platform shim handles it.

---
