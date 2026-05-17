# Build & Validation Plan

> Companion to `docs/product-spec.md` (what we ship) and the design docs in `docs/design/` (how each subsystem works). This document defines **the single ladder of milestones**, each one a runnable product that proves a specific architectural claim with real (not modeled) evidence.

> **Four arcs:** Kernel substrate + Nostr social stack (M0–M10) → FFI hardening + iOS empirical proof (M10.5) → kernel-boundary proof with a non-social-domain app (M11, the **`../podcast` rebuild on NMP**) → wallet/WoT + cross-platform + release (M12–M17).

> **Each milestone is gated.** Every milestone ends with: a runnable artifact, automated tests in `nmp-testing`, a measured-numbers report in `docs/perf/m<N>/`, and an explicit ADR if a design decision was revised in flight. **No silent endings.** **No "for later" carve-outs** — if a slice is in the milestone scope, it ships in that milestone, or the milestone is not done.

> **The doctrine is final** (`docs/product-spec.md` §1.5): D0 kernel never grows app nouns · D1 best-effort rendering with placeholders · D2 reactivity contract (composite reverse index, ≤60Hz/view, working-set bound) · D3 errors never cross FFI · D4 one writer per fact · D5 capabilities report, never decide. Every PR is reviewed against this rubric; a change that makes any doctrine harder to enforce is rewritten or rejected.

---

## 0. Where we are right now

Honest accounting before forecasting forward.

### Implemented and running

- **Kernel substrate** in `crates/nmp-core` (~3,800 LOC): actor on a dedicated OS thread, mailbox-driven (ADR feedback adopted — relay reads happen in tokio reader tasks, the actor blocks on its own channel with deadline timeouts), substrate trait families (`DomainModule`, `ViewModule`, `ActionModule`, `CapabilityModule`, `IdentityModule`) in `nmp-core/src/substrate/`, ingest pipeline (`kernel/ingest.rs`), claim/release refcounting for profile interest (commit `23ae829`), composite reverse-index dependency tracking.
- **Live Nostr-connected iOS app** in `ios/NmpStress` (~1,375 LOC Swift): SwiftUI shell wired to the Rust kernel via raw C FFI. Connects to `wss://relay.primal.net` (content) + `wss://purplepag.es` (indexer). Renders seed-driven timeline from union of pablof7z + fiatjaf + jb55 follow lists. Profile resolution with placeholders → in-place refinement on kind:0 arrival per doctrine D1. Thread view. Diagnostics screen showing relay status, logical interests, wire subscriptions (ADR-0007).
- **Fixture proving the kernel boundary** in `crates/fixture-todo-core` (~304 LOC) plus generated `apps/fixture/nmp-app-fixture`: a non-Nostr TODO module implementing all five substrate trait families, with codegen producing the per-app crate. Proves the kernel works for arbitrary domains, not just Nostr.
- **Codegen tool** in `crates/nmp-codegen` (~423 LOC): reads `nmp.toml`, produces a per-app crate, has determinism tests.
- **Benches** in `crates/nmp-testing`: `reactivity-bench` (composite-key reverse index + coalescer + working set; run 002 passed all ADR-0001..0004 gates) and `firehose-bench` (replay + capture + live modes; replay scenarios pass the modeled budget contract).
- **Perf reports** in `docs/perf/` documenting reactivity-bench run 002, firehose-bench replay runs, and three iOS measurement reports (relay lifecycle, profile/thread subscriptions, the primal slice baseline).
- **Architecture decisions** locked in 10 ADRs (`docs/decisions/0001`–`0010`).

### Designed but not implemented

- LMDB / IndexedDB persistent storage (in-memory only today).
- NIP-65 outbox routing (hardcoded content + indexer relays today).
- NIP-77 negentropy sync.
- NIP-42 relay auth.
- Multi-account / multi-session model and account switching.
- Signer trait + local-key signer + NIP-46 bunker signer.
- Action ledger + write path (compose / react / repost / quote).
- NIP-17 messaging and the NSE companion crate.
- Blossom uploads / downloads with resumable progress.
- Wallet stack (NWC, NIP-57 zaps, Cashu, nutzaps).
- Web-of-Trust subsystem.
- UniFFI bindings (current iOS bridge is raw C FFI).
- Android shell, Desktop shell, Web shell.
- The `nmp` CLI scaffolding tool.
- A non-Nostr-shaped product (podcast app) demonstrating the kernel boundary in production.

### Gaps in the prior plan that this rewrite addresses

- The prior plan was phase-numbered (Phase 1, 2, …) without explicit *demoable products* per phase.
- NIP-42 wasn't covered.
- Subscription compilation (the load-bearing NDK/Applesauce lesson) wasn't elevated as its own milestone.
- Blossom and media-capability lifecycle (long-running, resumable, background) were one bullet under Phase 6.
- No milestone proved the kernel boundary for a fundamentally non-social product.
- The plan didn't reflect that M0 and M1 are largely done.
- **No dedicated FFI hardening + iOS empirical proof gate before the kernel-boundary proof.** The prior M11 implicitly assumed the FFI surface was ready; this rewrite makes it a separate milestone (M10.5).
- **M11 was generic.** This rewrite ties it concretely to `/Users/pablofernandez/src/podcast` (the fully-functional Swift app) as the rebuild target, with copy-first UI fidelity and an explicit view-by-view module mapping.

The plan below is a single ladder of eighteen milestones (M0–M17, with M10.5 inserted as the FFI gate), each producing a runnable artifact, ordered so that each milestone strictly adds capabilities to the prior demoable product.

---

## 1. Principles of execution

1. **Each milestone is a runnable product.** Not a feature branch; a thing you can build, launch on real hardware, and demo. Unit tests verify correctness; the milestone product validates the architecture.
2. **Real measured evidence over modeled budgets.** Modeled passes in `firehose-bench` replay establish the budget contract. Real passes in `firehose-bench live` against the iOS / Android / Desktop / Web app are the actual gate.
3. **Capability layering is strict.** Each milestone adds exactly one new architectural ingredient on top of the previous demo. No "we'll wire it up later" — wiring is the milestone.
4. **The doctrine rubric is final.** Every PR is reviewed against the cardinal doctrines (`product-spec.md` §1.5, D0–D5). A change that makes any doctrine harder to enforce is rewritten or rejected.
5. **The kernel never grows app nouns.** ADR-0009 doctrine D0 is enforced by review and by the M11 podcast-app proof.
6. **No phase ends silently.** Each milestone exit produces: regression tests added to `nmp-testing`, a perf report in `docs/perf/m<N>/`, an ADR if a design decision was revised, and a runnable artifact tagged in git.

---

## 2. The milestone ladder

Each milestone has: **demo product**, **scope (what gets built)**, **subsystem deliverables**, **exit gate (measurable)**, and **runnable artifact**. Estimates are for one experienced developer focused on the work; they are not commitments.

### M0 — Kernel substrate + non-Nostr fixture *(DONE)*

**Demo product:** `apps/fixture/nmp-app-fixture` — a TODO list app driven by the kernel substrate with no Nostr concepts in it.

**Scope.** Five extension trait families. Composite reverse index. Delta buffer with coalescing. Claim-based GC. Codegen producing a working per-app crate from a fixture module.

**Subsystem deliverables.** `nmp-core::substrate`, `nmp-codegen`, `fixture-todo-core`, `nmp-testing` harness skeletons.

**Exit gate.** ✅ Fixture compiles and runs; codegen determinism test passes; substrate registry test passes (`crates/nmp-core/tests/substrate_registry.rs`).

**Runnable artifact.** `cargo test --workspace`; the fixture module loads in any host.

---

### M1 — Read-only Twitter slice on iOS *(LARGELY DONE; final hardening in flight)*

**Demo product:** `ios/NmpStress` — SwiftUI app pulling live from primal, rendering seed-driven timeline, profile cards, threads, diagnostics screen.

**Scope.** Per ADR-0006 + ADR-0008 + ADR-0009: kind:0 Profile path end-to-end against a real relay, on iOS, through real FFI. Seed-driven discovery (union of follow lists from pablof7z + fiatjaf + jb55). Refcounted claim/release pattern per ADR-0005 (profile interest commit `23ae829`). Diagnostics surface per ADR-0007.

**Subsystem deliverables.**

- ✅ Kernel actor with mailbox-driven relay ingestion (commit `9e9ce04`).
- ✅ Real WebSocket connections via `tungstenite` + `rustls`.
- ✅ Profile / Timeline / Thread view kinds wired through the kernel.
- ✅ Best-effort rendering (D1): placeholders → in-place refinement on kind:0 arrival.
- ✅ iOS bridge (`KernelBridge.swift`, `KernelModel.swift`, content views).
- ✅ Diagnostics screen showing relay state, logical interests, wire subs (ADR-0007).
- 🟡 Firehose-bench `live` scenarios `cold_start` + `profile_thrashing` running against the iOS app's kernel with **measured numbers** documented as the M1 baseline. (Initial reports exist in `docs/perf/ios-demo/` but should be promoted to `docs/perf/m1/` and gated.)

**Exit gate.**

- Avatar / name / picture / NIP-05 fields update in place when kind:0 arrives mid-scroll without any spinner gate.
- Mount/unmount of 100 avatar components rapidly produces correct refcount lifecycle (no leaks, claim drops on grace period).
- Primal connection survives a 30-second disconnect via reconnect with no observable data loss in a retried scroll.
- Firehose-bench `live cold_start` against primal: time to first profile rendered ≤ 800 ms p99, time to filled timeline (200 items) ≤ 5 s p99 on developer hardware.
- Firehose-bench `live profile_thrashing` (50/sec mount/unmount over 10 min) against primal: zero subscription leaks; `OpenView`/`CloseView` dispatch rate ≤ 60% of mount rate (grace-period absorption working).
- All reactivity-bench `--standard` gates continue to pass against the real kernel code path, not just the synthetic model.

**Runnable artifact.** `just run-ios` launches the app on iPhone simulator pulled from real primal. `docs/perf/m1/baseline.md` published with measured numbers.

---

### M2 — Subscription compilation + outbox routing

**Demo product:** Same iOS app as M1, but timeline subscriptions are routed per-author to those authors' write relays per NIP-65, not to the hardcoded primal/purplepag.es pair. Diagnostics screen visibly shows the per-relay fan-out and which authors each relay covers.

**Scope.** The planner becomes a **subscription compilation stage** per the NDK/Applesauce lessons doc. Logical interests get compiled into per-relay plans; recompilation happens when relay metadata arrives late. NIP-65 routing is the default for both reads and writes. Provenance / NIP-65 / relay hints / user-configured relays are four distinct facts that inform each other but never collapse.

**Subsystem deliverables.**

- `nmp-nip65` protocol module: Mailboxes view module (parsed kind:10002); outbox routing as a planner subsystem; recompilation triggers (kind:10002 arrival, view open, relay reconnect).
- Planner refactored from "hardcoded relay set" to "compiler of logical interests → per-relay plans." See `docs/design/ndk-applesauce-lessons.md` §7 (subscription compilation lessons).
- Per-pubkey relay-list cache (durable, even before LMDB lands — keep it in-memory until M3, but the data model is correct).
- Indexer fallback when a pubkey's kind:10002 is unknown: opportunistic discovery from a configurable indexer relay set.
- Reverse-relay-coverage view for diagnostics: "this relay is serving N authors of our timeline."

**Exit gate.**

- Bug-extinction test #3 (publish to wrong relays): no public API path lets the developer specify relays for a publish; explicit override action exists and produces a debug warning.
- Subscription compilation correctness: for a timeline of 1000 authors, the compiled plan opens REQs only against the union of those authors' write relays (de-duplicated). Test asserts on the wire frame count.
- Late-arriving kind:10002 triggers recompilation: an author whose mailbox was unknown gets re-routed once their kind:10002 lands, without the platform observing protocol churn.
- Distinct-source visibility: the diagnostics screen shows the four relay-fact lanes (NIP-65 / hint / provenance / user-configured) separately.

**Runnable artifact.** iOS app with measurably different relay-fan-out behavior; demo screenshot in `docs/perf/m2/outbox-routing.md` showing per-relay coverage.

---

### M3 — Persistence (LMDB) + full insert invariants

**Demo product:** iOS app cold-starts in ≤ 1.5 s with the previous session's events already on screen.

**Scope.** Swap in-memory `EventStore` for LMDB via `Box<dyn EventStore>`. Implement the full insert invariants from `product-spec.md` §7.1: parameterized replaceable events (kind 30000–39999 by `(pubkey, kind, d-tag)`), kind:5 delete handling with tombstone persistence, NIP-40 expiration scheduling, dedup with provenance merge, claim-based GC running.

**Subsystem deliverables.**

- LMDB schema design doc (`docs/design/lmdb-schema.md`) — key encoding, secondary indexes, tombstones, watermarks table (populated in M4), backup/export format.
- `EventStore` trait abstracted; LMDB backend; in-memory backend kept for tests.
- Migration plumbing (ties into `DomainModule::migrations()`).
- GC working set policy per ADR-0003: hot ≤ 10k events resident + claim-pinned set; cold on disk.

**Exit gate.**

- Cold-start with primed LMDB: time-to-first-painted-timeline ≤ 1.5 s on iPhone 12.
- Working-set memory under sustained scroll: ≤ 100 MB at 100 active views / 10k hot events / 1 M cached on disk.
- Replaceable correctness across restart: a kind:0 written, app killed, app reopened — the latest version is served, not stale.
- Kind:5 self-delete persists; foreign kind:5 ignored.

**Runnable artifact.** iOS app surviving termination + relaunch with state preserved. Report in `docs/perf/m3/persistence.md`.

---

### M4 — NIP-77 negentropy sync engine

**Demo product:** Profile screen for a new author cold-syncs via NIP-77 against primal, visibly faster and with measured bytes savings vs REQ scan.

**Scope.** Per `product-spec.md` §7.8 and ADR (sync as engine, not feature):

**Subsystem deliverables.**

- `nmp-nip77` protocol module: negentropy reconciliation client (use `nostr-sdk`'s implementation or `negentropy` crate directly).
- Sync watermarks table active per-`(filter, relay)`.
- Planner consults watermarks before issuing historical REQ; sync-first backfill with REQ as fallback (when relay doesn't support NIP-77).
- Three built-in triggers: app foreground, view-open-with-gap, relay reconnect.
- `RunSync` manual action module.
- Per-relay NIP-77 capability negotiation (probe + cache result).
- Bytes-saved counter in diagnostics.

**Exit gate.**

- Cold open of a profile against primal: completes via negentropy, not REQ. Bytes-on-wire ≤ 5% of equivalent REQ on a 10k-event backfill.
- Cache-miss against a fully-synced `(filter, relay)` pair answers authoritatively (no fallback fetch).
- Relay reconnect after 10 min resumes from watermark; gap filled by sync.
- Mixed-capability test (one NIP-77 relay, one non-NIP-77): both populate the same store; non-NIP-77 falls back to REQ; bytes-saved diagnostic reflects the split.

**Runnable artifact.** iOS app with measurably faster profile cold-opens. Report in `docs/perf/m4/negentropy.md`.

---

### M5 — NIP-42 auth

**Demo product:** iOS app connects to an NIP-42-required relay (such as a private nostr.wine subscription) and successfully authenticates + receives content.

**Scope.** Per-relay auth state machine: relay sends `AUTH` challenge → kernel routes to active signer → signer produces kind:22242 → kernel sends `AUTH` back → relay acknowledges → subscriptions resume. Auth failures surface as `RelayAuthState::Failed` in diagnostics (ADR-0007 §1).

**Subsystem deliverables.**

- `nmp-nip42` protocol module: auth challenge handling, kind:22242 builder, per-relay auth state.
- Planner pauses subscriptions on a relay while it's in `ChallengeReceived` / `Authenticating` states.
- `KeyringCapability` minimal API used to sign auth events (full signer trait still M6).
- Diagnostics: `RelayAuthState` rendered per relay.

**Exit gate.**

- Test relay configured with NIP-42 required: connection completes with auth, subscriptions deliver events.
- Auth failure (wrong signer) produces a visible diagnostic state and a toast in the app; subscriptions stay paused until resolved.
- Re-authentication on reconnect works without re-issuing logical subscriptions.

**Runnable artifact.** iOS app working against an NIP-42-required relay. Report in `docs/perf/m5/nip42.md`.

---

### M6 — Sessions + signers + write path

**Demo product:** iOS app gets a login screen. After login the user can compose and publish a kind:1 note to primal that atomically appears in their own timeline.

**Scope.** Per `product-spec.md` §7.4, §7.5, §7.15:

**Subsystem deliverables.**

- `IdentityModule::HumanAccount` with local-key signer (raw nsec, NIP-49 encrypted).
- `IdentityModule::ExternalSigner` with NIP-46 (Nostr Connect / bunker) signer.
- `KeychainCapability` real implementation: encrypted nsec storage via iOS Keychain, app-private access group.
- Action ledger in `nmp-core::kernel::ledger`: durable rows with ULID action IDs, status transitions, retry/cancel, restart recovery.
- Action atomicity contract: a `SendNote` action's publish to relays and local store insert happen as one actor message; partial failure rolls back.
- `nmp-nip01::SendNoteActionModule` as the first write-path action.
- Login UX (single nsec field for now; multi-step onboarding deferred to M16).

**Exit gate.**

- Bug-extinction #7 (action partial-success): inject "publish OK / store fail" and "store OK / publish fail" — both roll back atomically.
- Bug-extinction #9 (NIP-46 lost on suspend): simulate suspend mid-publish; resume retries or surfaces failure as toast.
- Bug-extinction #10 (re-publish keeps event id): re-publish of an event preserves `id` and `sig`.
- Compose flow on iOS: login → compose → publish → note visible on primal externally and in local timeline within one ViewBatch.

**Runnable artifact.** iOS Twitter slice with working compose. Report in `docs/perf/m6/write-path.md`.

---

### M7 — Reactions + Thread + Reply (the interaction loop)

**Demo product:** Twitter slice user can like a post, reply to it, see the thread, and have the reply land in primal.

**Scope.** `nmp-nip25` (Reactions view module + React action), `nmp-nip10` (Thread view module with NIP-10 reply-marker handling), `SendNote` extended for `reply_to`.

**Subsystem deliverables.**

- Reactions view module with NIP-25 emoji normalization (`+` and missing content → "like"; deduplicate by `(pubkey, emoji)`).
- React action module on the action ledger.
- Thread view module with reply-marker handling (NIP-10 `marker = reply | root | mention` plus legacy positional fallback). Orphan support.
- iOS UI: like button on each timeline row; tap → thread screen with nested replies; reply composer.

**Exit gate.**

- Tap-to-thread → see reply tree built correctly; orphan storm test (1000 replies in random order, 50% parents arriving after children) builds tree identical to known-good single-pass; build time ≤ 50 ms.
- Reactions aggregation: 10k reactions over 30 s coalesce to ≤ 60 deltas/sec/view per ADR-0002.
- Reply published from iOS arrives back via the live tail and slots into the thread tree without flicker.

**Runnable artifact.** iOS Twitter slice with complete read/like/reply loop. Report in `docs/perf/m7/interaction-loop.md`.

---

### M8 — Multi-session (multi-account) clients

**Demo product:** Twitter slice gets an account switcher. Logged-in users can add a second account, switch between them, and each account's timeline / contacts / reactions are correctly isolated.

**Scope.** Per spec doctrine D4 (single writer per fact) extended to account scope:

**Subsystem deliverables.**

- Session model in the kernel: `SessionState { accounts, active, status }` with N accounts simultaneously valid.
- View specs that depend on the active account (Timeline of "your follows", DMs inbox, zap history) get account-scoped composite keys.
- Account switch is an action with full rebuild semantics — open views for the new active account, close the prior ones, projection caches stay populated across switches when overlap exists.
- Per-account signer binding (each account has its own `IdentityId`).
- Per-account secure storage namespacing in `KeychainCapability`.

**Exit gate.**

- Bug-extinction #5 (account-context overlap): two accounts active, switch between them, assert no state bleed. `AppState` snapshot for account A never contains data scoped to account B's session-aware views.
- Switching accounts during an in-flight publish: the publish is account-tagged, completes correctly, lands in the originating account's timeline only.
- Per-account signer never signs an event for the wrong account (test forces dispatch through a wrong-account signer; the action ledger rejects).

**Runnable artifact.** Account switcher in iOS demo with two real test accounts. Report in `docs/perf/m8/multi-account.md`.

---

### M9 — NIP-17 DMs + NSE

**Demo product:** Twitter slice gets a DMs tab. End-to-end NIP-17 gift-wrapped messages between two test accounts. Background push triggers iOS Notification Service Extension decryption; opening the app shows the message already in place.

**Scope.** Per spec §7.10 and §7.14:

**Subsystem deliverables.**

- `nmp-nip17` protocol module: Conversation view module + ConversationList view module; SendDm action module; NIP-44 encryption / NIP-59 gift-wrapping; outbox routing for DMs (recipient inbox relays only — never public).
- `nmp-nip17-nse` companion crate: `decrypt_push()` API with bounded memory (≤ 24 MB peak, ≤ 200 ms p99), reading from shared keychain and shared App Group storage.
- iOS NSE target wiring: silent push from APNs → NSE invokes `decrypt_push` → notification posted with decrypted preview.
- Action atomicity for `SendDm`: gift-wrap → publish to all recipient inboxes → insert locally — atomic.

**Exit gate.**

- Bug-extinction #4 (DM to public): no API path can send a DM to a non-inbox relay; planner refuses non-inbox relays for `p`-tagged-only events.
- DM round-trip in `MockRelay` (alice ↔ bob): content matches; no plaintext crosses FFI other than as `ConversationMessage.body`.
- NSE decrypt of an incoming gift-wrap: p99 ≤ 200 ms, peak memory ≤ 24 MB.
- Backgrounded app receives a push, NSE decrypts and posts notification, app foregrounded shows the message in place (no re-fetch from relay).

**Runnable artifact.** iOS Twitter slice with working DMs + push notifications. Report in `docs/perf/m9/messaging.md`.

---

### M10 — Blossom + media + long-running capabilities

**Demo product:** Twitter slice user can attach a photo to a compose, see upload progress, and the published note has a valid Blossom URL. Profile-picture upload also works.

**Scope.** Per spec §7.11. Establishes the **long-running capability lifecycle pattern** that the podcast app (M11) builds on:

**Subsystem deliverables.**

- `nmp-blossom` protocol module: upload action module + download action module + media view module + upload-status view (progress).
- `FilePickerCapability` real implementation on iOS (PHPicker for photos / `UIDocumentPicker` for files).
- `BlossomCapability` callback interface: kernel asks platform to perform an HTTP PUT with progress; platform reports progress + completion back via reverse callback into the actor.
- Long-running action lifecycle: upload registers in the action ledger as `AwaitingCapability`; capability progress updates the ledger row; restart recovery resumes from the last checkpointed progress.
- Resumable uploads (Blossom range support where the server allows).
- BUD-01 / BUD-02 protocol support.

**Exit gate.**

- Upload a 5 MB photo on iOS, kill the app mid-upload, restart — upload resumes from the checkpoint, does not restart from byte 0.
- Cancellation works mid-upload (capability reports back `Cancelled`; ledger row finalizes correctly).
- Slow-network upload remains responsive — main UI is never blocked.
- Profile picture update through compose → kind:0 republish with new Blossom URL → in-place refinement across all open Profile / Timeline payloads (per doctrine D1).

**Runnable artifact.** iOS Twitter slice with media compose. Report in `docs/perf/m10/blossom.md`.

---

### M10.5 — FFI hardening + iOS empirical proof *(hard gate before M11 starts)*

**Demo product:** The iOS Twitter slice from M1–M10 subjected to a published, exhaustive stress harness on the iOS simulator and a real iPhone 12. The kernel↔FFI↔SwiftUI path is proven, in measured numbers, to be **rock-solid and demonstrably performant** before a single line of the podcast app is written.

**Why this milestone exists separately.** Every milestone M1–M10 has run iOS measurements, but each in service of its own feature. M10.5 is the dedicated *FFI surface* hardening pass — finding and fixing every shape of FFI bug that a non-social-domain consumer (M11 podcast app) would otherwise discover the hard way. This is also the milestone where we delete every shortcut and "TODO: revisit" comment in the FFI layer.

**Scope.**

- **Stress harness** (`crates/nmp-testing/bin/ffi-stress` + `ios/NmpStress/StressUITests/`):
  - Mount/unmount churn: 1000 view-handle wrappers cycled per second for 10 minutes; assert zero leaks (via Instruments leak instrument scripted run).
  - Dispatch flood: 10k `dispatch(...)` calls per second from Swift across multiple threads; assert no dropped messages, no main-thread block > 16 ms.
  - Snapshot pressure: `AppUpdate::FullState` with 100k events forced; measure marshal time, allocations, and that the reconciler stays ≤ 60 Hz via batching.
  - Reconciler back-pressure: deliberately stall the Swift main thread for 250 ms; assert no actor stall, deltas accumulate and replay correctly when the main thread resumes.
  - Reentrancy: dispatch from inside a reconciler callback (a known footgun); assert ordered, deadlock-free.
  - Capability lifecycle storms: start/stop/restart each registered capability 1000 times; assert idempotency per RMP bible.
  - Error-shape exhaustion: every typed FFI error path exercised; assert each one becomes a `toast: Option<String>` state field, never a thrown exception across the boundary (D3).
- **Real-device measurement on iPhone 12** (one full battery of `firehose-bench live` against primal, all 8 scenarios from `docs/design/firehose-bench.md` §3); produces `docs/perf/m10.5/iphone12-baseline.md` with hardware-tagged numbers.
- **Simulator-driven UI test fleet** (parallel Sonnet agents via the `mcp__xcode` and `BrowserAgent`/`QATester` skills) exercising the app from the outside — boot sim, launch app, tap, scroll, swipe, kill-relaunch — capturing screenshots and assertions per scripted scenario. Every M1–M10 user-visible feature gets a UI test; failures block the milestone.
- **Memory + leak audit** with Xcode Instruments (Leaks, Allocations, Time Profiler) on canonical workflows; zero retained-by-cycle leaks; allocations after warmup linear-or-better in active-view count, never in cached-event count.
- **Profile-Guided Optimization sweep** on the kernel hot paths surfaced by Time Profiler; document tradeoffs taken.
- **All M1–M10 perf reports re-run** on the final FFI surface to confirm no regressions.
- **FFI surface documentation audit** in `docs/ffi-surface.md` — every exported type, function, capability trait, and ownership/lifetime invariant called out; reviewed against `RMP-ARCHITECTURE-BIBLE.md` commandments and ADR-0010.
- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.

**Subsystem deliverables.**

- `crates/nmp-testing/bin/ffi-stress` — new bench binary.
- `ios/NmpStress/StressUITests` — XCUITest target driven by both XCTest and (where relevant) a scripted Sonnet-agent runner.
- `docs/design/ffi-hardening.md` — design doc enumerating every FFI failure mode and how the harness exercises it.
- `docs/ffi-surface.md` — the canonical FFI surface reference.
- `docs/perf/m10.5/` — measured numbers from simulator, M-series Mac, iPhone 12; plus screenshots from the Sonnet-driven UI runs.

**Exit gate.**

- All stress-harness scenarios pass on simulator and iPhone 12 with the numeric thresholds enumerated in `docs/design/ffi-hardening.md` §exit-gate.
- All M1–M10 perf reports re-run cleanly on the post-M10.5 binaries; no regression > 5 % on any p99 number.
- Instruments-recorded Leaks count = 0 over the 10-minute canonical workflow.
- Every UI-scripted scenario (Sonnet-agent and XCUITest) passes on a fresh boot of the iPhone 16 Pro simulator and on iPhone 12 hardware.
- `docs/ffi-surface.md` reviewed and tagged.
- Doctrine review (D0–D5) signed off on the FFI surface in writing in `docs/perf/m10.5/doctrine-review.md`.

**Runnable artifact.** Same iOS Twitter app, now load-bearing. Report bundle in `docs/perf/m10.5/`.

---

### M11 — Podcast app (the `../podcast` rebuild on NMP — the kernel-boundary proof)

**Demo product:** A 1:1 rebuild of `/Users/pablofernandez/src/podcast` (the fully-functional Swift app, 20 SwiftUI views, ~8.8k LOC of Swift) running on NMP. **UI is pixel-identical** to the reference Swift app; **all business logic, LLM, audio orchestration, downloads, transcripts, RAG, recommendations** are in Rust extension modules driving the kernel.

**This is the load-bearing kernel-boundary check.** If the kernel needs even one podcast noun to make this work, the boundary is wrong and we go back to fix it.

**Reference inputs** (read before scoping):

- `/Users/pablofernandez/src/podcast/` — canonical Swift implementation. Source of truth for UI and feature behavior. **Every view in `PodcastApp/Views/` is copied verbatim into `ios/NmpPodcast/Views/`** as step 1; only the data source is rewired.
- `/Users/pablofernandez/src/podcast-rmp/` — prior WIP RMP rewrite (incomplete). **Not a code source** but a lessons source: read its `RMP-ARCHITECTURE-BIBLE.md`, `FINAL_PLAN.md`, `docs/plans/iphone-feature-parity-plan.md`, and `docs/plans/iphone-feature-parity-checklist.md` before scoping. That repo's `AGENTS.md` is the working guide for any agent touching that tree.
- `/Users/pablofernandez/src/podcast/docs/plans/` — original feature design docs (podcast-app-design, discovery-tab-redesign, insights-feature-design).

**Reference inventory of the Swift app** (so the scope is explicit, not vibes):

| Swift `Views/` group | Files | NMP target |
|---|---|---|
| `Ask/` | AskView.swift | `ask-core` ActionModule + ViewModule wrapping `rig.rs` LLM call |
| `Components/` | CachedAsyncImage, DiscoveryCards | reusable Swift components, ported as-is; image cache backed by NMP Blossom-aware capability |
| `Feed/` | FeedView, EpisodeRow | `podcast-core::FeedViewModule` + `EpisodeRowViewModule` |
| `Insights/` | InsightsView | `insights-core` ViewModule + ActionModule (uses RAG via `rig.rs`) |
| `Library/` | ActivityView, AddPodcastView, DiscoverView, EpisodeDetailView, LibraryView, PodcastDetailSheet, PodcastDetailView, QueueView | `podcast-core` ViewModules + ActionModules |
| `Player/` | ChaptersPanel, GuestAgentSheet, MiniPlayer, PlayerSheet, TranscriptView | `player-core` ViewModule + `AudioPlaybackCapability` |
| `Settings/` | SettingsView | `settings-core` ActionModule (mostly capability invocations) |

Swift `Services/` (AIService, AudioService, DownloadService, GuestEnrichmentService, ImageCache, InsightService, PodcastIndexService, PodcastService, ProcessingQueue, RAGService, RecommendationService, ServiceContainer, TranscriptionService, VectorDatabase) **all move to Rust** as ActionModules + ProjectionCaches + capability bridges; Swift loses its Services/ directory entirely.

Swift `Models/` (AITypes, Chapter, Episode, Guest, Insight, Podcast, Settings, Transcript) **all move to Rust** as DomainRecords inside `podcast-core` and sibling crates.

Swift `ViewModels/` **disappear** — they become Rust ViewModules whose output crosses FFI as typed ViewBatch deltas.

**Scope.**

**Step 0 — copy step (UI-fidelity invariant lock):**

- Copy every file in `/Users/pablofernandez/src/podcast/PodcastApp/Views/` into `ios/NmpPodcast/NmpPodcast/Views/` verbatim. Commit immediately. No edits except the minimum needed to compile against placeholder data sources (`// MARK: NMP-WIRE` markers).
- Copy `Resources/Assets.xcassets` and `Info.plist` (sanitized) verbatim.
- The result compiles and renders against stubbed data; UI is visually identical to `../podcast` per a side-by-side simulator screenshot diff (≤ 1 px tolerance, font-rendering exceptions documented).

**Step 1 — domain + view modules in Rust** (per the table above):

- `apps/podcast/podcast-core/` — main app crate. `DomainModule`s: `Podcast`, `Episode`, `Transcript`, `Chapter`, `Guest`, `Insight`, `Subscription`, `PlayerState`, `QueueEntry`, `Activity`.
- `apps/podcast/podcast-core/` — `ViewModule`s: `PodcastLibrary`, `EpisodeDetail`, `NowPlaying`, `EpisodeQueue`, `Discover`, `Insights`, `Activity`, `PodcastDetail`, `Feed`, `EpisodeRow`, `Chapters`, `Transcript`, `MiniPlayer`, `PlayerSheet`, `GuestAgent`, `Ask`, `Settings`.
- `apps/podcast/podcast-core/` — `ActionModule`s: `SubscribePodcast`, `UnsubscribePodcast`, `RefreshFeed`, `DownloadEpisode`, `CancelDownload`, `Play`, `Pause`, `Seek`, `SkipForward`, `SkipBack`, `MarkPlayed`, `EnqueueEpisode`, `ReorderQueue`, `ImportRss`, `ImportOpml`, `AskQuestion`, `EnrichGuest`, `RunInsight`, `SearchPodcasts`.
- `apps/podcast/podcast-llm/` — LLM-driven actions via `rig.rs`: `AskQuestion`, `EnrichGuest`, `RunInsight`. Uses the kernel's capability bridge for HTTP + key storage.
- `apps/podcast/podcast-rag/` — RAG + vector DB store; uses a swappable `EmbeddingCapability` and a Rust-side vector store (sqlite-vss or qdrant-client).
- `apps/podcast/podcast-feeds/` — RSS + Atom + JSON Feed + Podcast 2.0 namespaces parsing; transcripts; chapters; value-for-value. Pure Rust; pulls via `HttpCapability`.

**Step 2 — capabilities added to the kernel's reusable set** (these are general, not podcast-specific):

- `AudioPlaybackCapability`: play URL or local file; report position events + state transitions back; iOS impl via `AVPlayer` + background-audio entitlement + lock-screen `MPNowPlayingInfoCenter`/`MPRemoteCommandCenter`.
- `BackgroundWorkCapability`: register periodic background tasks; iOS impl via `BGTaskScheduler`.
- `LocalNotificationCapability`: extended for episode-available alerts.
- `HttpCapability`: long-running streaming response support (RSS, transcripts).
- `EmbeddingCapability`: callable embedding model; kernel-owned policy, platform-owned execution (CoreML on iOS, ONNX or remote API as fallback).
- `KeyValueStoreCapability`: typed persistent KV (for saved playback position when persistence-by-store is overkill).

**Step 3 — protocol module integration:**

- `nmp-podcast` (Nostr podcast NIP integration where it exists — NIP-XX podcast feed events, value-for-value zaps, episode discussion threads). Where Nostr coverage is incomplete, the app uses RSS via `podcast-feeds` and Nostr for social overlay (zaps, discussions, recommendations from the WoT subsystem).

**Step 4 — wire each copied Swift view to its Rust view module:**

- Replace stubbed data with the generated wrapper hooks (`@PodcastLibrary`, `@NowPlaying`, etc. — produced by `nmp gen modules`).
- The Swift file shape stays the same; only the data source changes.
- After every Library/Feed/Player/Insights/Ask/Settings group is wired, run the side-by-side screenshot diff again.

**Exit gate (kernel boundary).**

- **`nmp-core` gains zero podcast nouns.** No `Podcast`, `Episode`, `Transcript`, `Chapter`, `Player`, `Feed`, `Insight`, `Guest` types added to the kernel. Verified by grep + manual review at the commit.
- **The capability families added in M11 are general** (audio playback, background work, local notifications, HTTP, embedding, KV-store). Their request/response shapes are not podcast-specific.
- **Reactivity behavior is identical** to the Twitter slice — composite-key dependencies, delta coalescing, claim-based GC, ADR-0007 diagnostics all work for podcast view modules.
- **No app-state leaks across the boundary in either direction:** no Nostr type appears in `podcast-core`'s public surface; no podcast type appears in `nmp-core`'s public surface.

**Exit gate (product fidelity to `../podcast`).**

- **UI parity:** side-by-side screenshot of every screen in `../podcast` vs `ios/NmpPodcast` matches at ≤ 1 px tolerance (font/rendering differences whitelisted explicitly in `docs/perf/m11/parity-screenshots.md`).
- **Feature parity:** every user flow exercised in `/Users/pablofernandez/src/podcast/Tests/` (or its equivalent on the canonical Swift app) reproduced as a scripted Sonnet-agent run on `ios/NmpPodcast`. No "feature dropped" footnotes.
- **Subscribe to 10 real podcasts** spanning RSS + (where available) Nostr feeds; library populates correctly.
- **Download an episode in the background** while the app is suspended; resumable on relaunch.
- **Play with background audio** while the iPhone is locked; lock-screen artwork, scrubber, skip/seek controls all functional.
- **Resume playback at the correct position** after a kill-relaunch.
- **Push notification on a new episode arrival.**
- **Ask a question** about an episode; answer streams in via `rig.rs` LLM with the transcript as RAG context.
- **Insights** view generates a structured episode summary on demand.
- **Guest enrichment** populates guest cards via external lookup, identical to the Swift impl behavior.

**Stress + perf gates.**

- Library of 100 podcasts × 50 episodes (5k episodes total) scrolls at 60 fps on iPhone 12.
- Player UI updates every 250 ms during playback without visible jank.
- Download queue with 20 concurrent downloads keeps the UI responsive.
- LLM ask flow streams first token in ≤ 1500 ms over Wi-Fi; full answer in ≤ 8 s for an average-length episode (measured).
- Battery drain during 1 hour of background playback ≤ Swift baseline + 10 %.

**Runnable artifact.** `ios/NmpPodcast` — distinct binary, same Rust kernel, different module set, **same UI as `../podcast`**. Report in `docs/perf/m11/podcast-app.md` documenting kernel-boundary verification, parity screenshots, and the perf measurements above.

---

### M12 — Wallet (NWC + zaps + Cashu + nutzaps)

**Demo product:** Twitter slice gets a zap button on each post. Tapping it pays via NWC. Receiving zaps shows up in a zap-history view. Cashu nutzap claim works.

**Scope.** Per spec §7.9:

**Subsystem deliverables.**

- `nmp-nwc` protocol module: NIP-47 client; pay/receive/balance.
- `nmp-nip57` protocol module: LUD-16 discovery + zap request building + receipt verification.
- `nmp-nip60` protocol module: Cashu wallet event types + proof state in domain store.
- `nmp-nip61` protocol module: Nutzap action module; pending-nutzap claim flow.
- `WalletBalance` view module; `ZapHistory` view module.
- Zap action module: `Zap { target, sats, comment }` on the action ledger.

**Exit gate.**

- Pay a 100-sat zap via NWC to a real LUD-16 endpoint; receipt verifies; balance updates within one ViewBatch.
- Receive a zap (test via a separate device or simulated): zap-history view reflects within one ViewBatch.
- Nutzap claim from a Cashu mint: proofs land in the wallet; balance updates.
- Wallet operations never block the UI thread.

**Runnable artifact.** iOS Twitter slice with working zaps. Report in `docs/perf/m12/wallet.md`.

---

### M13 — Web-of-Trust

**Demo product:** Twitter slice gets a "score-filtered timeline" toggle. With it on, low-WoT-score authors are de-prioritized; toggling off restores chronological order.

**Scope.** Per spec §7.7:

**Subsystem deliverables.**

- `nmp-wot` protocol module:
  - Action: `LoadFollowGraph { root: PubKey, depth: u8 }` — populates an in-memory follow graph.
  - Projection cache: `wot_score: HashMap<PubKey, f32>`.
  - View module: `WotRank` exposes per-pubkey score + reasoning.
  - Filter view module wrapper: composes with Timeline to produce a score-filtered variant.
- Pluggable scoring trait (default: depth-weighted in-degree).

**Exit gate.**

- Load follow graph rooted at the active account to depth 2; computes scores for 10k+ pubkeys in ≤ 5 s on iPhone 12.
- Score-filtered timeline visibly reorders / hides low-score authors; toggle off restores chronological.
- New kind:3 arrival incrementally updates scores without full recompute.

**Runnable artifact.** iOS Twitter slice with WoT toggle. Report in `docs/perf/m13/wot.md`.

---

### M14 — UniFFI migration

**Demo product:** iOS app, podcast app, and (incoming) Android/Desktop/Web shells all bind to the kernel via UniFFI-generated bindings produced by `nmp gen modules`, not raw C FFI.

**Scope.** Replace the current raw C FFI surface in `crates/nmp-core/src/ffi.rs` with the per-app generated `nmp-app-<name>` crate per ADR-0010. The iOS app stops importing `NmpCore.h` and instead imports the generated Swift module.

**Subsystem deliverables.**

- `nmp-codegen` extended to produce UniFFI scaffolding in the generated per-app crate.
- `apps/twitter/nmp-app-twitter` and `apps/podcast/nmp-app-podcast` as the first two real per-app crates.
- `xcframework` build pipeline for each per-app crate.
- Generated Swift wrappers: `useProfile`, `@Profile`, `useTimeline`, `@Wallet`, etc.
- CI gate: `nmp gen modules --check` fails the build if bindings drift.

**Exit gate.**

- iOS app builds and runs against UniFFI-generated bindings; no raw C FFI in the app target.
- Cross-platform consistency test (next milestone) is unblocked because the FFI shape is now identical across platforms.
- Codegen determinism: repeated runs produce byte-identical output.

**Runnable artifact.** iOS Twitter + iOS Podcast apps both using UniFFI. Report in `docs/perf/m14/uniffi-migration.md`.

---

### M15 — Cross-platform: Android + Desktop + Web

**Demo product:** Same Twitter slice and (where capabilities allow) podcast slice running on Android (Compose), Desktop (iced), and Web (wasm + React/Solid TBD). Cross-platform consistency test passes — same scripted scenario produces byte-identical `AppState` JSON on all four platforms.

**Scope.**

**Android port (~3 weeks):**

- Kotlin bindings via UniFFI; cargo-ndk + Gradle pipeline.
- Compose shell mirroring the iOS SwiftUI shell.
- `KeychainCapability` Android impl via `EncryptedSharedPreferences`.
- `nmp-nip55` Amber external-signer capability module.
- Android `FirebaseMessagingService` integration with `nmp-nip17-nse` for DM push.

**Desktop port (~2 weeks):**

- iced shell (the development-time reference target lives on; this milestone graduates it to a shipping target).
- macOS + Linux + Windows.
- `KeychainCapability` impls per OS (macOS Keychain, Secret Service, Windows Credential Manager — already exists in `nostr-keyring`).

**Web port (~3 weeks):**

- `nmp-wasm` mature.
- IndexedDB storage backend; OPFS where supported.
- `nmp-nip07` browser-signer capability module.
- Web shell stack TBD (React + signals / Solid / Svelte — pick at start of milestone).

**Subsystem deliverables.**

- Cross-platform consistency test in `nmp-testing` — drives same scripted action sequence on all four targets, snapshots `AppState` JSON at checkpoints, asserts byte-equal.
- Per-platform performance reports.

**Exit gate.**

- Twitter clone identical scripted scenario produces byte-identical `AppState` snapshots on iOS / Android / Desktop / Web.
- All §7.16 performance budgets met on reference devices (iPhone 12, Pixel 6a, M1 mini, modern browsers).
- Web works in incognito mode by falling back to in-memory store with a visible warning.

**Runnable artifact.** Four-platform demo. Report in `docs/perf/m15/cross-platform.md`.

---

### M16 — CLI + starter app + recipe book

**Demo product:** A developer with no prior framework knowledge runs `nmp init my-app`, follows recipes, ships a working hashtag-feed app on all four platforms in ≤ 2 hours.

**Scope.**

**Subsystem deliverables.**

- `nmp init`, `nmp add module`, `nmp gen modules`, `nmp doctor`, `nmp upgrade` commands.
- A minimal **starter app** (distinct from the proof/Twitter app) implementing only: login + timeline + compose + profile + DMs. Stays under the platform LOC budgets from spec §3.2.
- Recipe book in `docs/recipes/`: one recipe per common app shape (timeline-only viewer, kind-filtered explorer, long-form reader, etc.).
- NIP support matrix in `docs/nips.md`.
- Migration guide in `docs/migration.md`.

**Exit gate.**

- §3 success criteria of the spec reproducible from published docs alone, no insider knowledge.
- One external developer (or an LLM agent with no prior context) succeeds at building a small custom app from the starter + recipes in ≤ 2 hours.

**Runnable artifact.** Public `nmp init` flow. Report in `docs/perf/m16/dx.md`.

---

### M17 — v1 release

**Scope.**

- Resolve naming (`aim.md` §7.7).
- Publish crates to crates.io.
- Publish CLI to npm as `@<name>/cli`.
- Tag release; publish bindings; deploy example apps; write release announcement.

**Exit gate.**

- Public availability on crates.io and npm.
- Three external developers ship a real app within 30 days of release.
- v1 release report in `docs/perf/v1/release.md`.

---

## 3. Subsystem coverage matrix

Cross-reference of which milestone delivers which user-specified concern.

| Concern | Milestone(s) | Notes |
|---|---|---|
| **Outbox routing (NIP-65)** | M2 | First-class as a planner stage, not a side feature. Diagnostics show per-relay coverage. |
| **NDK-style subscription aggregation** | M2 | Per `docs/design/ndk-applesauce-lessons.md` §7, the planner becomes a subscription compiler. Logical interests → per-relay plans → wire REQs, semantics-preserving merge/split. |
| **Reactivity as planned** | M0–M7 | Already validated by reactivity-bench run 002 against the model; M1 runs the same code path against real iOS; subsequent milestones add view modules that exercise the contract under varied loads. |
| **Non-Nostr data bridge** | M0 (substrate), M10 (long-running capabilities), M11 (podcast app proves it in production) | DomainModule trait + ADR-0007 bridge lanes; first proven by fixture-todo-core; production proof in podcast app. |
| **FFI hardening + empirical iOS proof** | M10.5 | Dedicated stress harness, real-device measurement, simulator-driven Sonnet-agent UI suite; hard gate before M11. |
| **UI parity to `../podcast`** | M11 (copy step) | Every Swift view copied verbatim, screenshot-diff gated. |
| **NIP-42 auth** | M5 | Per-relay auth state machine; integrates with diagnostics; works with both local-key and NIP-46 signers. |
| **Blossom** | M10 | Upload + download with resumable progress; long-running capability lifecycle. |
| **Multi-session clients** | M8 | Per-account view-spec scoping; account switcher; isolation tests. |
| **NIP-77 negentropy** | M4 | Sync engine with watermarks; planner consults before REQ; capability negotiation; bytes-saved diagnostic. |
| **Podcast-class apps** | M11 (proof), M10 (capabilities prerequisite) | AudioPlaybackCapability, BackgroundWorkCapability, BlossomDownloadCapability all generic; podcast-specific domain in `podcast-core` app crate. |

### NIP support roadmap at v1

| NIP | Module | Milestone | Status |
|---|---|---|---|
| 01 | nmp-nip01 | M1, M6 | partial (reads in M1; writes in M6) |
| 02 | nmp-nip02 | M2 | follow-list parsing (contacts view) |
| 04 | not v1 | — | superseded by NIP-44/17; not implemented |
| 05 | nmp-nip01 | M1 | NIP-05 verification in Profile module |
| 07 | nmp-nip07 | M15 | web-only browser signer |
| 09 | nmp-nip01 | M3 | kind:5 deletes (full handling) |
| 10 | nmp-nip10 | M7 | reply markers in thread building |
| 17 | nmp-nip17 | M9 | DMs |
| 19 | nmp-nip19 | M1 | bech32 utility used throughout |
| 23 | not v1 | — | long-form reader is post-v1 |
| 25 | nmp-nip25 | M7 | reactions |
| 40 | nmp-nip01 | M3 | expiration scheduling |
| 42 | nmp-nip42 | M5 | relay auth |
| 44 | nmp-nip17 | M9 | encryption (via NIP-17) |
| 46 | nmp-nip46 | M6 | bunker signer |
| 47 | nmp-nwc | M12 | wallet connect |
| 49 | nmp-nip01 / nmp-nip46 | M6 | encrypted-key import |
| 55 | nmp-nip55 | M15 | Android Amber bridge |
| 57 | nmp-nip57 | M12 | zaps |
| 59 | nmp-nip17 | M9 | gift wrap (via NIP-17) |
| 60 | nmp-nip60 | M12 | Cashu |
| 61 | nmp-nip61 | M12 | nutzaps |
| 65 | nmp-nip65 | M2 | mailboxes + outbox |
| 77 | nmp-nip77 | M4 | negentropy |
| Blossom BUD-01/02 | nmp-blossom | M10 | media |

NIPs not in v1 (e.g., NIP-29 groups, NIP-23 long-form, NIP-71 video) become post-v1 extension modules; the kernel boundary makes them additive.

---

## 4. Parallelization opportunities

The ladder above is the **dependency order** — what must precede what — not a wall-clock schedule. Genuine parallel work tracks:

- **M2 (outbox), M3 (LMDB), M4 (negentropy)** can pipeline tightly: M3 + M4 are almost mechanically pluggable once M2's compiled-plan abstraction exists.
- **M5 (NIP-42)** is independent of M3/M4 and can be done alongside.
- **M6 (signer + write path) is a serialization point** — most downstream milestones (M7, M8, M9, M10, M12) depend on it. Land this fast.
- **M10.5 (FFI hardening)** is itself parallelizable: the stress harness, the iPhone-12 perf rerun, the UI-script Sonnet-agent fleet, and the FFI surface audit are four independent workstreams.
- **M11 (podcast app)** starts only after M10.5 passes. Its own internal parallelism is wide: the copy step + each `*-core` Rust extension crate + each view-wiring batch can be split across agents (one per view group: Library, Feed, Player, Insights, Ask, Settings, Components, plus one agent per LLM/RAG/feeds module).
- **M15 (Android + Desktop + Web)** is three parallel tracks once M14 (UniFFI) lands.

A team of two could run M5 alongside the M2–M4 sequence with no integration risk. With parallel-agent execution (this session's mode), the practical limit is conflict surface: independent crates, independent docs, and independent platform shells fan out cleanly; shared mutable files (e.g. `nmp.toml`, the codegen output, `Cargo.toml`) serialize.

### Worktree hygiene

Every parallel worker that mutates source operates in its own git worktree under `.claude/worktrees/`. **On merge, the worktree is removed** (`git worktree remove --force` + branch cleanup) by the worker before the parent acknowledges done — otherwise DerivedData and `target/` clones blow out the disk fast. The known precedent is podcast-rmp's `~/Library/Developer/Xcode/DerivedData/Podcastr-*` sprawl; we share `CARGO_TARGET_DIR` and `-derivedDataPath` across worktrees from the start to avoid it.

---

## 5. Test pyramid

| Level | Tooling | What it covers | Where it lives |
|---|---|---|---|
| Unit | `cargo test` per crate | Pure-function correctness, substrate trait invariants, codegen determinism | Each crate's `tests/` |
| Subsystem integration | `cargo test --test '*'` in `nmp-testing` | EventStore + planner + sync against MockRelay | `crates/nmp-testing/tests/` |
| Cross-FFI | UniFFI binding round-trip tests | Bindings stability, rev ordering, callback delivery | `apps/<name>/nmp-app-<name>/tests/` (post-M14) |
| Cross-platform consistency | Script harness | Same scenario on iOS sim + Android emu + desktop + headless web; assert `AppState` JSON byte-equal | `nmp-testing/scenarios/` |
| Reactivity bench | `reactivity-bench --standard --fail-on-gate` | Composite reverse index, delta coalescing, working-set memory, allocation gates | `crates/nmp-testing/bin/reactivity-bench/` |
| Firehose bench (modeled) | `firehose-bench replay --standard --fail-on-gate` | Budget contract for the runtime | `crates/nmp-testing/bin/firehose-bench/` |
| Firehose bench (live) | `firehose-bench live` against the real iOS app | Runtime evidence end-to-end | reports in `docs/perf/m<N>/` |
| Per-app UI smoke | XCUITest + Espresso + iced UI test + Playwright | End-to-end flows render without error | `ios/<app>/UITests/` etc. |
| Manual exploratory | Humans on reference devices | What metrics can't catch | per-milestone manual checklist |

The cross-platform consistency tests are the highest-value tier post-M15.

---

## 6. CI / pre-merge hygiene

Required CI gates (apply from the milestone they become possible):

- `cargo fmt --all -- --check` (always).
- `cargo test --workspace` (always).
- `cargo run -p nmp-codegen -- gen modules --check` (codegen determinism, from M0).
- `cargo run -p nmp-testing --bin reactivity-bench --release -- --standard --fail-on-gate` (from M0).
- `cargo run -p nmp-testing --bin firehose-bench --release -- replay --standard --fail-on-gate` (from M0).
- iOS build (`just build-ios`) from M1.
- iOS UI test (`xcrun simctl test`) from M1.
- Android build from M15.
- Desktop build from M15.
- Web build from M15.
- Cross-platform consistency test from M15.

Live firehose runs are not in pre-merge CI (would block on relay flakes); they run nightly on a dedicated runner and produce reports tagged `live` in `docs/perf/m<N>/`.

---

## 7. Decision log

ADRs live in `docs/decisions/`. Format per the template in older revisions of this plan. Currently:

- **ADR-0001**: Composite dependency keys (composite-first reverse index; broad axes guardrailed).
- **ADR-0002**: Per-view delta budget (60/view/sec, not absolute).
- **ADR-0003**: Working-set memory budget (hot/cold split, not total events).
- **ADR-0004**: Allocation measurement via counting allocator.
- **ADR-0005**: Domain-keyed platform shadow + refcounted component wrappers.
- **ADR-0006**: Vertical-slice-first delivery (modified by ADR-0009; the slice now layers on the kernel substrate).
- **ADR-0007**: Diagnostics and non-Nostr data over the actor-owned bridge with explicit records, not raw callbacks or fake Nostr events.
- **ADR-0008**: Twitter-clone iOS as the Phase 1a demo target (modified by ADR-0009 — repositioned as first canonical extension-module set).
- **ADR-0009**: App-extension kernel boundary. Five trait families, four layers, no app nouns in nmp-core.
- **ADR-0010**: Per-app concrete enums generated at the FFI boundary. Codegen is critical-path v1 infrastructure.

New ADRs land alongside any milestone whose execution revises a design.

### The harness-first pattern

Every design doc has measurable gates. Gates run on the reactivity-bench harness (or `firehose-bench` for end-to-end behavior). Failures revise the design **before** implementation. Pre-implementation measurement is cheaper than post-implementation rework. Run 001 of reactivity-bench established the pattern: the reverse-index direction was validated (100×–1000× headroom), one design refinement landed (composite keys), and two budget bugs surfaced (per-view delta, working-set memory) — all before any view-kind code shipped.

### Modeled budget contract vs runtime evidence

Two distinct claims about the same harness:

- **Modeled budget contract.** Replay mode runs deterministic synthetic workloads through a model of the runtime. Passing here proves budgets are internally consistent and the harness scaffolding is sound. Does **not** prove the real runtime hits those budgets.
- **Runtime evidence.** Live mode (or replay mode with real adapters substituted for modeled segments) runs against actual LMDB, actual WebSockets, actual UniFFI marshaling. Passing here is real evidence.

Each milestone moves the boundary rightward — replaces another modeled segment with a real adapter and graduates the corresponding firehose-bench scenarios from `modeled` to `measured` in `docs/perf/`.

---

## 8. What this plan is not

- **Not a schedule.** No dates, no person-months. Milestones are sequential; their durations depend on team size and surface complexity. Estimates per milestone are guidance only.
- **Not a marketing roadmap.** v1 ships when M17 gates are met, not on a calendar.
- **Not exhaustive about post-v1 work.** NIP-29 groups, NIP-23 long-form, NIP-71 video, additional protocol modules, additional app demonstrations (Highlighter-lite, TENEX-lite, etc.) are post-v1 — they validate the kernel boundary further but are not v1 deliverables.
- **Not silent about gaps.** §0 names exactly what is and isn't built. As the ladder progresses, §0 gets revised so the plan stays honest about state.

The plan exists so that any single milestone can be picked up cold by someone reading this doc + `product-spec.md` + the relevant ADRs and design docs, and they can execute without bothering the rest of the team.
