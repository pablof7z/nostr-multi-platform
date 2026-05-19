Reading additional input from stdin...
2026-05-17T23:14:22.658440Z ERROR codex_core::session: failed to load skill /Users/pablofernandez/.agents/skills/voice-capture-sheet/SKILL.md: invalid YAML: mapping values are not allowed in this context at line 2 column 116
OpenAI Codex v0.129.0 (research preview)
--------
workdir: /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
model: gpt-5.5
provider: openai
approval: never
sandbox: workspace-write [workdir, /tmp, $TMPDIR, /Users/pablofernandez/.codex/memories]
reasoning effort: xhigh
reasoning summaries: none
session id: 019e3838-1f44-74c3-825a-5324a253e9b9
--------
user
You are reviewing merge e9cbafa on master in nostr-multi-platform. Doctrine D0-D5:
- D0: kernel never grows app nouns (nmp-core stays substrate-only; per ADR-0009)
- D1: best-effort rendering — render now, refine in place; no spinners gating renderable content
- D2: negentropy first, REQ second; every filter/relay pair is a tracked sync target
- D3: outbox routing is automatic; manual relay selection is the opt-out
- D4: single writer per fact; caches derive
- D5: snapshots bounded by what's open; AppState carries view payloads only for open views

File size: 300 LOC soft, 500 LOC hard.
Session goal: complete v1 with zero technical debt; M9 DMs + M12 Wallet deferred; M11 podcast + M11.5 Highlighter pending.

=== diff stat ===
 docs/plan.md | 807 ++++++++++++++++++++++++++++++++++++++---------------------
 1 file changed, 519 insertions(+), 288 deletions(-)

=== commit log ===
e9cbafa docs(plan): consolidate into single milestone-driven plan with product checks

=== diff (first 8000 chars) ===
diff --git a/docs/plan.md b/docs/plan.md
index f3de4b6..81e1702 100644
--- a/docs/plan.md
+++ b/docs/plan.md
@@ -1,449 +1,680 @@
 # Build & Validation Plan
 
-> Companion to `docs/product-spec.md`. The spec defines **what we ship**; this plan defines **how we get there and how we know it works**.
+> Companion to `docs/product-spec.md` (what we ship) and the design docs in `docs/design/` (how each subsystem works). This document defines **the single ladder of milestones**, each one a runnable product that proves a specific architectural claim with real (not modeled) evidence.
 
-> **Two arcs:** Build the infrastructure → build a stress-proof app on top → measure on real devices → tune → release.
+> **Three arcs:** Kernel substrate + Nostr social stack (M0–M10) → kernel-boundary proof with a non-social-domain app (M11) → wallet/WoT + cross-platform + release (M12–M17).
 
-> **The plan is gated.** Each phase has an automated exit gate plus a manual sign-off. Subsequent phases must not regress prior gates. The proof app (Arc 2) is the load-bearing artifact — it is how we know the framework actually works at scale, not just in isolation.
+> **Each milestone is gated.** Every milestone ends with: a runnable artifact, automated tests in `nmp-testing`, a measured-numbers report in `docs/perf/m<N>/`, and an explicit ADR if a design decision was revised in flight. No silent endings.
 
 ---
 
-## 0. Principles of execution
+## 0. Where we are right now
+
+Honest accounting before forecasting forward.
+
+### Implemented and running
+
+- **Kernel substrate** in `crates/nmp-core` (~3,800 LOC): actor on a dedicated OS thread, mailbox-driven (ADR feedback adopted — relay reads happen in tokio reader tasks, the actor blocks on its own channel with deadline timeouts), substrate trait families (`DomainModule`, `ViewModule`, `ActionModule`, `CapabilityModule`, `IdentityModule`) in `nmp-core/src/substrate/`, ingest pipeline (`kernel/ingest.rs`), claim/release refcounting for profile interest (commit `23ae829`), composite reverse-index dependency tracking.
+- **Live Nostr-connected iOS app** in `ios/NmpStress` (~1,375 LOC Swift): SwiftUI shell wired to the Rust kernel via raw C FFI. Connects to `wss://relay.primal.net` (content) + `wss://purplepag.es` (indexer). Renders seed-driven timeline from union of pablof7z + fiatjaf + jb55 follow lists. Profile resolution with placeholders → in-place refinement on kind:0 arrival per doctrine D1. Thread view. Diagnostics screen showing relay status, logical interests, wire subscriptions (ADR-0007).
+- **Fixture proving the kernel boundary** in `crates/fixture-todo-core` (~304 LOC) plus generated `apps/fixture/nmp-app-fixture`: a non-Nostr TODO module implementing all five substrate trait families, with codegen producing the per-app crate. Proves the kernel works for arbitrary domains, not just Nostr.
+- **Codegen tool** in `crates/nmp-codegen` (~423 LOC): reads `nmp.toml`, produces a per-app crate, has determinism tests.
+- **Benches** in `crates/nmp-testing`: `reactivity-bench` (composite-key reverse index + coalescer + working set; run 002 passed all ADR-0001..0004 gates) and `firehose-bench` (replay + capture + live modes; replay scenarios pass the modeled budget contract).
+- **Perf reports** in `docs/perf/` documenting reactivity-bench run 002, firehose-bench replay runs, and three iOS measurement reports (relay lifecycle, profile/thread subscriptions, the primal slice baseline).
+- **Architecture decisions** locked in 10 ADRs (`docs/decisions/0001`–`0010`).
+
+### Designed but not implemented
+
+- LMDB / IndexedDB persistent storage (in-memory only today).
+- NIP-65 outbox routing (hardcoded content + indexer relays today).
+- NIP-77 negentropy sync.
+- NIP-42 relay auth.
+- Multi-account / multi-session model and account switching.
+- Signer trait + local-key signer + NIP-46 bunker signer.
+- Action ledger + write path (compose / react / repost / quote).
+- NIP-17 messaging and the NSE companion crate.
+- Blossom uploads / downloads with resumable progress.
+- Wallet stack (NWC, NIP-57 zaps, Cashu, nutzaps).
+- Web-of-Trust subsystem.
+- UniFFI bindings (current iOS bridge is raw C FFI).
+- Android shell, Desktop shell, Web shell.
+- The `nmp` CLI scaffolding tool.
+- A non-Nostr-shaped product (podcast app) demonstrating the kernel boundary in production.
+
+### Gaps in the prior plan that this rewrite addresses
+
+- The prior plan was phase-numbered (Phase 1, 2, …) without explicit *demoable products* per phase.
+- NIP-42 wasn't covered.
+- Subscription compilation (the load-bearing NDK/Applesauce lesson) wasn't elevated as its own milestone.
+- Blossom and media-capability lifecycle (long-running, resumable, background) were one bullet under Phase 6.
+- No milestone proved the kernel boundary for a fundamentally non-social product.
+- The plan didn't reflect that M0 and M1 are largely done.
+
+The plan below is a single ladder of seventeen milestones (M0–M17), each producing a runnable artifact, ordered so that each milestone strictly adds capabilities to the prior demoable product.
 
-1. **Infrastructure before features.** Get the actor model, FFI marshaling, planner, event store, and sync engine right and verified before layering wallet, WoT, messaging, etc. on top.
+---
+
+## 1. Principles of execution
+
+1. **Each milestone is a runnable product.** Not a feature branch; a thing you can build, launch on real hardware, and demo. Unit tests verify correctness; the milestone product validates the architecture.
+2. **Real measured evidence over modeled budgets.** Modeled passes in `firehose-bench` replay establish the budget contract. Real passes in `firehose-bench live` against the iOS / Android / Desktop / Web app are the actual gate.
+3. **Capability layering is strict.** Each milestone adds exactly one new architectural ingredient on top of the previous demo. No "we'll wire it up later" — wiring is the milestone.
+4. **The doctrine rubric is final.** Every PR is reviewed against the cardinal doctrines (`product-spec.md` §1.5, D0–D5). A change that makes any doctrine harder to enforce is rewritten or rejected.
+5. **The kernel never grows app nouns.** ADR-0009 doctrine D0 is enforced by review and by the M11 podcast-app proof.
+6. **No phase ends silently.** Each milestone exit produces: regression tests added to `nmp-testing`, a perf report in `docs/perf/m<N>/`, an ADR if a design decision was revised, and a runnable artifact tagged in git.
+
+---
+
+## 2. The milestone ladder
+
+Each milestone has: **demo product**, **scope (what gets built)**, **subsystem deliverables**, **exit gate (measurable)**, and **runnable artifact**. Estimates are for one experienced developer focused on the work; they are not commitments.
+
+### M0 — Kernel substrate + non-Nostr fixture *(DONE)*
 
-2. **A real app validates the framework.** Unit tests prove pieces work; the proof app proves they work together at scale. The proof app is not optional — it is the v1 release gate.
+**Demo product:** `apps/fixture/nmp-app-fixture` — a TODO list app driven by the kernel substrate with no Nostr concepts in it.
 
-3. **Measure on the device.** Synthetic benchmarks lie. Performance budgets (§7.16 of the spec) are validated against the proof app running on real mid-range phones, real desktops, real browsers — not on a developer's M-series laptop.
+**Scope.** Five extension trait families. Composite reverse index. Delta buffer with coalescing. Claim-based GC. Codegen producing a working per-app crate from a fixture module.
 
-4. **No phase ends silently.** Each phase ends with: regression tests added to `nmp-testing`, a brief write-up in `docs/perf/phaseN.md` if relevant, and an explicit gate sign-off.
+**Subsystem deliverables.** `nmp-core::substrate`, `nmp-codegen`, `fixture-todo-core`, `nmp-testing` harness skeletons.
 
-5. **The doctrine list (D1–D5) is the rubric.** Every PR is reviewed against the cardinal doctrines. If a change makes any doctrine harder to enforce, it gets rewritten or rejected.
+**Exit gate.** ✅ Fixture compiles and runs; codegen determinism test passes; substrate registry test passes (`crates/nmp-core/tests/substrate_registry.rs`).
+
+**Runnable artifact.** `cargo test --workspace`; the fixture module loads in any host.
 
 ---
 
-## Arc 1 — Infrastructure (Phases 0–7)
+### M1 — Read-only Twitter slice on iOS *(LARGELY DONE; final hardening in flight)*
 
-### Phase 0 — Foundations
+**Demo product:** `ios/NmpStress` — SwiftUI app pulling live from primal, rendering seed-driven timeline, profile cards, threads, diagnostics screen.
 
-**Scope.**
+**Scope.** Per ADR-0006 + ADR-0008 + ADR-0009: kind:0 Profile path end-to-end against a real relay, on iOS, through real FFI. Seed-driven discovery (union of follow lists from pablof7z + fiatjaf + jb55). Refcounted claim/release pattern per ADR-0005 (profile interest commit `23ae829`). Diagnostics surface per ADR-0007.
+
+**Subsystem deliverables.**
+
+- ✅ Kernel actor with mailbox-driven relay ingestion (commit `9e9ce04`).
+- ✅ Real WebSocket connections via `tungstenite` + `rustls`.
+- ✅ Profile / Timeline / Thread view kinds wired through the kernel.
+- ✅ Best-effort rendering (D1): placeholders → in-place refinement on kind:0 arrival.
+- ✅ iOS bridge (`KernelBridge.swift`, `KernelModel.swift`, content views).
+- ✅ Diagnostics screen showing relay state, logical interests, wire subs (ADR-0007).
+- 🟡 Firehose-bench `live` scenarios `cold_start` + `profile_thrashing` running against the iOS app's kernel with **measured numbers** documented as the M1 baseline. (Initial reports exist in `docs/perf/ios-demo/` but should be promoted to `docs/perf/m1/` and gated.)
+
+**Exit gate.**
+
+- Avatar / name / picture / NIP-05 fields update in place when kind:0 arrives mid-scroll without any spinner gate.
+- Mount/unmount of 100 avatar components rapidly produces correct refcount lifecycle (no leaks, claim drops on grace period).
+- Primal connection survives a 30-second disconnect via reconnect with no observable data loss in a retried scroll.
+- Firehose-bench `live cold_start` against primal: time to first profile rendered ≤ 800 ms p99, time to filled timeline (200 items) ≤ 5 s p99 on developer hardware.
+- Firehose-bench `live profile_thrashing` (50/sec mount/unmount over 10 min) against primal: zero subscription leaks; `OpenView`/`CloseView` dispatch rate ≤ 60% of mount rate (grace-period absorption working).
+- All reactivity-bench `--standard` gates continue to pass against the real kernel code path, not just the synthetic model.
+
+**Runnable artifact.** `just run-ios` launches the app on iPhone simulator pulled from real primal. `docs/perf/m1/baseline.md` published with measured numbers.
+
+---
+
+### M2 — Subscription compilation + outbox routing
+
+**Demo product:** Same iOS app as M1, but timeline subscriptions are routed per-author to those authors' write relays per NIP-65, not to the hardcoded primal/purplepag.es pair. Diagnostics screen visibly shows the per-relay fan-out and which authors each relay covers.
+
+**Scope.** The planner becomes a **subscription compilation stage** per the NDK/Applesauce lessons doc. Logical interests get compiled into per-relay plans; recompilation happens when relay metadata arrives late. NIP-65 routing is the default for both reads and writes. Provenance / NIP-65 / relay hints / user-configured relays are four distinct facts that inform each other but never collapse.
+
+**Subsystem deliverables.**
+
+- `nmp-nip65` protocol module: Mailboxes view module (parsed kind:10002); outbox routing as a planner subsystem; recompilation triggers (kind:10002 arrival, view open, relay reconnect).
+- Planner refactored from "hardcoded relay set" to "compiler of logical interests → per-relay plans." See `docs/design/ndk-applesauce-lessons.md` §7 (subscription compilation lessons).
+- Per-pubkey relay-list cache (durable, even before LMDB lands — keep it in-memory until M3, but the data model is correct).
+- Indexer fallback when a pubkey's kind:10002 is unknown: opportunistic discovery from a configurable indexer relay set.
+- Reverse-relay-coverage view for diagnostics: "this relay is serving N authors of our timeline."
+
+**Exit gate.**
+
+- Bug-extinction test #3 (publish to wrong relays): no public API path lets the developer specify relays for a publish; explicit override action exists and produces a debug warning.
+- Subscription compilation correctness: for a timeline of 1000 authors, the compiled plan opens REQs only against the union of those authors' write relays (de-duplicated). Test asserts on the wire frame count.
+- Late-arriving kind:10002 triggers recompilation: an author whose mailbox was unknown gets re-routed once their kind:10002 lands, without the platform observing protocol churn.
+- Distinct-source visibility: the diagnostics screen shows the four relay-fact lanes (NIP-65 / hint / provenance / user-configured) separately.
+
+**Runnable artifact.** iOS app with measurably different relay-fan-out behavior; demo screenshot in `docs/perf/m2/outbox-routing.md` showing per-relay coverage.
+
+---
 
-- Cargo workspace with the kernel + codegen crate roster from spec §4.1 (kernel crates only; protocol modules and app modules come in later phases).
-- `nmp-core` kernel skeleton: actor on one OS thread + flume channel + tokio runtime; empty `KernelAction` / `AppState` / `AppUpdate` types with `rev: u64`; module registry stubs.
-- `nmp-codegen` skeleton: parses `nmp.toml`, produces a no-op `nmp-app-empty` crate with empty composed enums.
-- `nmp-ffi` building blocks (UniFFI primitives the generated crate will use).
-- `nmp-wasm` skeleton with wasm-bindgen building blocks.
-- `nmp-testing` skeleton with `MockRelay` re-export, snapshot helpers, and a `TestHarness` stub.
-- `justfile` recipes: `rust-build-host`, `gen-modules`, `gen-bindings`, `run-desktop`, `test`, `fmt`.
-- CI on GitHub Actions: `cargo fmt --check`, `cargo test --workspace`, codegen determinism check.
-- Nix flake.
+### M3 — Persistence (LMDB) + full insert invariants
 
-**Out of scope.** No extension modules yet (those land in 1a.1); no event handling; no relay code; no FFI targets beyond determinism check (iOS / Android / web compile in later phases).
+**Demo product:** iOS app cold-starts in ≤ 1.5 s with the previous session's events already on screen.
+
+**Scope.** Swap in-memory `EventStore` for LMDB via `Box<dyn EventStore>`. Implement the full insert invariants from `product-spec.md` §7.1: parameterized replaceable events (kind 30000–39999 by `(pubkey, kind, d-tag)`), kind:5 delete handling with tombstone persistence, NIP-40 expiration scheduling, dedup with provenance merge, claim-based GC running.
+
+**Subsystem deliverables.**
+
+- LMDB schema design doc (`docs/design/lmdb-schema.md`) — key encoding, secondary indexes, tombstones, watermarks table (populated in M4), backup/export format.
+- `EventStore` trait abstracted; LMDB backend; in-memory backend kept for tests.
+- Migration plumbing (ties into `DomainModule::migrations()`).
+- GC working set policy per ADR-0003: hot ≤ 10k events resident + claim-pinned set; cold on disk.
 
 **Exit gate.**
 
-- The kernel actor starts and stops cleanly under `cargo test`.
-- `nmp gen modules` invoked against an empty `nmp.toml` produces a working `nmp-app-empty` crate that compiles.
-- Round-trip determinism: `nmp gen modules` produces byte-identical output on repeat invocations.
-- `cargo test --workspace` passes on Linux + macOS.
+- Cold-start with primed LMDB: time-to-first-painted-timeline ≤ 1.5 s on iPhone 12.
+- Working-set memory under sustained scroll: ≤ 100 MB at 100 active views / 10k hot events / 1 M cached on disk.
+- Replaceable correctness across restart: a kind:0 written, app killed, app reopened — the latest version is served, not stale.
+- Kind:5 self-delete persists; foreign kind:5 ignored.
+
+**Runnable artifact.** iOS app surviving termination + relaunch with state preserved. Report in `docs/perf/m3/persistence.md`.
 
-**Regression test added.** `tests/kernel_lifecycle.rs` (actor start/stop), `tests/codegen_determinism.rs` (deterministic output).
+---
+
+### M4 — NIP-77 negentropy sync engine
 
-### Phase 1 — Kernel substrate + Twitter demo on iOS
+**Demo product:** Profile screen for a new author cold-syncs via NIP-77 against primal, visibly faster and with measured bytes savings vs REQ scan.
 
-Per ADR-0006 (slice discipline), ADR-0008 (Twitter clone iOS target), ADR-0009 (kernel + extension modules), and ADR-0010 (generated app enum), Phase 1 grows the kernel substrate first, then layers Nostr protocol modules and the Twitter clone on top. Every sub-phase has running code at its exit gate.
+**Scope.** Per `product-spec.md` §7.8 and ADR (sync as engine, not feature):
 
-**1a. Eight sub-phases (~12–15 weeks total).**
+**Subsystem deliverables.**
 
-- **1a.0** Foundations — workspace, kernel actor scaffolding, `nmp-codegen` skeleton, empty `KernelAction`/`AppUpdate` types, headless test harness. ~3–5 days.
-- **1a.1** **Kernel substrate prototype + non-Nostr fixture** — the five extension trait families (`DomainModule`, `ViewModule`, `ActionModule`, `CapabilityModule`, `IdentityModule`); composite reverse index; delta buffer with coalescing; claim-based GC; codegen producing a working `nmp-app-fixture` crate from a `fixture-todo-core` app module that implements all five trait families; desktop iced shell rendering a TODO list. ~2 weeks. **Proves: kernel substrate works for non-Nostr-shaped data; codegen pipeline functional.**
-- **1a.2** First Nostr protocol module + desktop avatar slice — `nmp-nip01` (Event, Filter, Keys, Profile `ViewModule`); `useProfile` wrapper; primal connection via `nostr-sdk`; desktop iced avatar demo. The original ADR-0006 slice, now built on the kernel substrate from 1a.1. ~1 week.
-- **1a.3** iOS port of the avatar slice — UniFFI binding pipeline; xcframework build; SwiftUI shell; `AppManager`; generated iOS wrappers (`useProfile` Swift binding); `KeychainCapability` minimal. ~2 weeks (UniFFI / Xcode surprises front-loaded here).
-- **1a.4** LMDB + Contacts + seed-driven Timeline — LMDB backend swap via `Box<dyn EventStore>`; `nmp-nip02` Contacts view module; `nmp-nip01::TimelineViewModule`; multiple Contacts views (one per seed dev account) union into the timeline's author set; real breadth from launch without requiring login. ~1.5 weeks.
-- **1a.5** Login + Signer + Compose — `IdentityModule::HumanAccount` (local-key signer); `KeychainCapability` real impl; `nmp-nip01::SendNoteActionModule` with atomicity; iOS login UI + compose sheet; optional "Home" timeline switch to logged-in user's own follows. ~1.5 weeks.
-- **1a.6** Reactions + Thread + Reply — `nmp-nip25` (Reactions view module + React action); `nmp-nip10` (Thread view module with reply-marker handling); `SendNote` action extended for replies; iOS interaction loop. ~1 week.
-- **1a.7** Profile screen + diagnostics + polish + second fixture — author-tap → profile screen with author-filtered Timeline; pagination; ADR-0007 diagnostics screen; second non-Nostr fixture module (small notes app) demonstrating boundary breadth; pull-to-refresh; error states. ~1–2 weeks.
+- `nmp-nip77` protocol module: negentropy reconciliation client (use `nostr-sdk`'s implementation or `negentropy` crate directly).
+- Sync watermarks table active per-`(filter, relay)`.
+- Planner consults watermarks before issuing historical REQ; sync-first backfill with REQ as fallback (when relay doesn't support NIP-77).
+- Three built-in triggers: app foreground, view-open-with-gap, relay reconnect.
+- `RunSync` manual action module.
+- Per-relay NIP-77 capability negotiation (probe + cache result).
+- Bytes-saved counter in diagnostics.
 
-The desktop iced binary built in 1a.2 stays alive through 1a.7 as a non-FFI **reference target** — running the same kernel + modules without UniFFI to disambiguate "is it architecture or is it the toolchain?" debugging.
+**Exit gate.**
 
-Exit gate for the slice: per ADR-0008 §"Sub-phase plan" exit gates plus the broader Phase 1 exit gate below.
+- Cold open of a profile against primal: completes via negentropy, not REQ. Bytes-on-wire ≤ 5% of equivalent REQ on a 10k-event backfill.
+- Cache-miss against a fully-synced `(filter, relay)` pair answers authoritatively (no fallback fetch).
+- Relay reconnect after 10 min resumes from watermark; gap filled by sync.
+- Mixed-capability test (one NIP-77 relay, one non-NIP-77): both populate the same store; non-NIP-77 falls back to REQ; bytes-saved diagnostic reflects the split.
 
-**1b. Broader Phase 1 scope, layered on top of the slice.**
+**Runnable artifact.** iOS app with measurably faster profile cold-opens. Report in `docs/perf/m4/negentropy.md`.
 
-- LMDB and IndexedDB backends; swap from in-memory via `Box<dyn EventStore>`.
-- Full insert invariants from spec §7.1 (parameterized replaceable, kind-5 delete, NIP-40 expiration, dedup with provenance merge).
-- Claim-based GC.
-- Sync watermarks table (read/write API; populated by Phase 2 sync engine).
-- `nmp-gossip` outbox routing for both reads and writes per spec §7.3.
-- Subscription planner with coalescing, auto-close, EOSE detection, ≤60Hz buffering, reconnect re-establishment.
-- Live REQ tail working end-to-end against `MockRelay`.
-- Reverse index + projections architecture from `docs/design/reactivity.md` (§3–§6) — the slice already includes composite keys; broader Phase 1 fills in the projection caches.
-- **`reactivity-bench` stress harness** — already built (run 002 validated the model). Promoted to pre-merge CI per recommendations.
-- **`firehose-bench` capture + replay infrastructure** — already built; live mode unblocks scenario by scenario as adapters land.
+---
 
-**Prerequisite design docs.**
+### M5 — NIP-42 auth
 
-- `docs/design/reactivity.md` — reviewed and accepted (rev 1, post run 001).
-- `docs/design/view-catalog.md` — reviewed for Phase 1 view kinds.
-- `docs/design/firehose-bench.md` — reviewed; runtime adapters track against §6 phasing.
+**Demo product:** iOS app connects to an NIP-42-required relay (such as a private nostr.wine subscription) and successfully authenticates + receives content.
 
-**Exit gate (full Phase 1, not just the slice).**
+**Scope.** Per-relay auth state machine: relay sends `AUTH` challenge → kernel routes to active signer → signer produces kind:22242 → kernel sends `AUTH` back → relay acknowledges → subscriptions resume. Auth failures surface as `RelayAuthState::Failed` in diagnostics (ADR-0007 §1).
 
-- All bug-extinction tests pass (per `product-spec.md` §3.3).
-- Replaceable event correctness verified.
-- Provenance correctness verified.
-- Reactivity harness gates (per `reactivity.md` §10.3 rev 1):
-  - Lookup p99 ≤ 100 µs (run 002: passed).
-  - Per-view recompute p99 ≤ 1 ms (run 002: passed).
-  - ≤ 60 deltas/sec/view (run 002: passed across all scenarios).
-  - False-wakeup rate ≤ 0.10 (run 002: 0 in quiet_idle, 1.00 candidates/delta).
-  - Working-set memory ≤ 100 MB at 100 active views (run 002: ~20 MB modeled).
-  - Zero per-event allocations after warmup (run 002: passed via counting allocator).
-- **Firehose-bench live mode unblocked for cold_start + profile_thrashing** against a real relay, measured numbers (not modeled) within budgets.
-- Unit-test coverage on composite reverse index, coalescer, and domain-keyed wrapper lifecycle (per Phase 1 recommendation from firehose-bench run 001).
+**Subsystem deliverables.**
+
+- `nmp-nip42` protocol module: auth challenge handling, kind:22242 builder, per-relay auth state.
+- Planner pauses subscriptions on a relay while it's in `ChallengeReceived` / `Authenticating` states.
+- `KeyringCapability` minimal API used to sign auth events (full signer trait still M6).
+- Diagnostics: `RelayAuthState` rendered per relay.
+
+**Exit gate.**
+
+- Test relay configured with NIP-42 required: connection completes with auth, subscriptions deliver events.
+- Auth failure (wrong signer) produces a visible diagnostic state and a toast in the app; subscriptions stay paused until resolved.
+- Re-authentication on reconnect works without re-issuing logical subscriptions.
+
+**Runnable artifact.** iOS app working against an NIP-42-required relay. Report in `docs/perf/m5/nip42.md`.
+
+---
 
-**Regression tests added.** `tests/event_store_invariants.rs`, `tests/planner_coalesce.rs`, `tests/outbox_routing.rs`, `tests/reverse_index.rs`, `tests/coalescer.rs`, `tests/wrapper_lifecycle.rs`, `bin/reactivity-bench/scenarios/*`, `bin/firehose-bench/scenarios/cold_start.rs`, `bin/firehose-bench/scenarios/profile_thrashing.rs`.
+### M6 — Sessions + signers + write path
 
-### Phase 2 — Sync engine (negentropy first-class)
+**Demo product:** iOS app gets a login screen. After login the user can compose and publish a kind:1 note to primal that atomically appears in their own timeline.
 
-**Scope.** Per spec §7.8.
+**Scope.** Per `product-spec.md` §7.4, §7.5, §7.15:
 
-- NIP-77 negentropy reconciliation implementation (or integration with `nostr-sdk`'s if available).
-- Watermark read/write — the table from Phase 1 now actively populated.
-- Planner consults watermarks before issuing REQ for historical data.
-- Three built-in triggers: app foreground, view open with gap, relay reconnect.
-- `RunSync` manual action.
-- Per-relay capability negotiation (probe for NIP-77 support; cache result).
-- Bytes-on-wire vs equivalent-REQ-bytes measurement plumbed into `nmp-metrics`.
-- `SyncState` field of `AppState` populated and visible.
+**Subsystem deliverables.**
+
+- `IdentityModule::HumanAccount` with local-key signer (raw nsec, NIP-49 encrypted).
+- `IdentityModule::ExternalSigner` with NIP-46 (Nostr Connect / bunker) signer.
+- `KeychainCapability` real implementation: encrypted nsec storage via iOS Keychain, app-private access group.
+- Action ledger in `nmp-core::kernel::ledger`: durable rows with ULID action IDs, status transitions, retry/cancel, restart recovery.
+- Action atomicity contract: a `SendNote` action's publish to relays and local store insert happen as one actor message; partial failure rolls back.
+- `nmp-nip01::SendNoteActionModule` as the first write-path action.
+- Login UX (single nsec field for now; multi-step onboarding deferred to M16).
+
+**Exit gate.**
+
+- Bug-extinction #7 (action partial-success): inject "publish OK / store fail" and "store OK / publish fail" — both roll back atomically.
+- Bug-extinction #9 (NIP-46 lost on suspend): simulate suspend mid-publish; resume retries or surfaces failure as toast.
+- Bug-extinction #10 (re-publish keeps event id): re-publish of an event preserves `id` and `sig`.
+- Compose flow on iOS: login → compose → publish → note visible on primal externally and in local timeline within one ViewBatch.
+
+**Runnable artifact.** iOS Twitter slice with working compose. Report in `docs/perf/m6/write-path.md`.
+
+---
+
+### M7 — Reactions + Thread + Reply (the interaction loop)
+
+**Demo product:** Twitter slice user can like a post, reply to it, see the thread, and have the reply land in primal.
+
+**Scope.** `nmp-nip25` (Reactions view module + React action), `nmp-nip10` (Thread view module with NIP-10 reply-marker handling), `SendNote` extended for `reply_to`.
+
+**Subsystem deliverables.**
+
+- Reactions view module with NIP-25 emoji normalization (`+` and missing content → "like"; deduplicate by `(pubkey, emoji)`).
+- React action module on the action ledger.
+- Thread view module with reply-marker handling (NIP-10 `marker = reply | root | mention` plus legacy positional fallback). Orphan support.
+- iOS UI: like button on each timeline row; tap → thread screen with nested replies; reply composer.
 
 **Exit gate.**
 
-- Cold open of a profile against a NIP-77-supporting relay completes sync via negentropy, not REQ. Bytes saved ≥ 95% vs equivalent REQ on a 10k-event backfill.
-- Relay reconnect after 10 minutes resumes from the watermark; the gap is filled by sync, not by a fresh REQ scan.
-- Mixed-capability relay set: of N relays, those that support NIP-77 use sync; those that don't fall back to REQ; both populate the same store correctly.
-- Negentropy bytes-saved counter visible in `AppState.debug` in debug builds.
-- Cache-miss-against-fully-synced-relay answers authoritatively (no fallback fetch).
+- Tap-to-thread → see reply tree built correctly; orphan storm test (1000 replies in random order, 50% parents arriving after children) builds tree identical to known-good single-pass; build time ≤ 50 ms.
+- Reactions aggregation: 10k reactions over 30 s coalesce to ≤ 60 deltas/sec/view per ADR-0002.
+- Reply published from iOS arrives back via the live tail and slots into the thread tree without flicker.
+
+**Runnable artifact.** iOS Twitter slice with complete read/like/reply loop. Report in `docs/perf/m7/interaction-loop.md`.
+
+---
+
+### M8 — Multi-session (multi-account) clients
 
-**Regression tests added.** `tests/sync_engine.rs`, `tests/sync_fallback.rs`, `tests/watermarks.rs`.
+**Demo product:** Twitter slice gets an account switcher. Logged-in users can add a second account, switch between them, and each account's timeline / contacts / reactions are correctly isolated.
 
-### Phase 3 — Sessions + signers + actions
+**Scope.** Per spec doctrine D4 (single writer per fact) extended to account scope:
 
-**Scope.** Per spec §7.4, §7.5, §7.15.
+**Subsystem deliverables.**
 
-- `SessionState` and the multi-account model.
-- Signer trait with two initial implementations: local key (raw nsec) and NIP-46 bunker.
-- NIP-49 encrypted private key support.
-- Full action catalog from spec §6.3.
-- Action atomicity guarantee: publish + store-insert happen as one actor message.
-- Offline action queue with durable persistence; replay on reconnect.
-- Capability bridge for `KeyringCapability` defined (native shims come in Phase 4 platform shells).
+- Session model in the kernel: `SessionState { accounts, active, status }` with N accounts simultaneously valid.
+- View specs that depend on the active account (Timeline of "your follows", DMs inbox, zap history) get account-scoped composite keys.
+- Account switch is an action with full rebuild semantics — open views for the new active account, close the prior ones, projection caches stay populated across switches when overlap exists.
+- Per-account signer binding (each account has its own `IdentityId`).
+- Per-account secure storage namespacing in `KeychainCapability`.
 
 **Exit gate.**
 
-- Bug-extinction tests pass:
-  - #5 (account-context overlap): two accounts active, switch between them, assert no state bleed.
-  - #7 (action partial-success): inject "publish succeeds, store-insert fails" and "store-insert succeeds, publish fails" — both atomically rolled back.
-  - #9 (NIP-46 lost on suspend/resume): simulate suspend mid-action; assert resume restores pending state and either retries or surfaces failure as toast.
-  - #10 (re-publish keeps event id): re-publish of an event preserves its original `id` and `sig`.
-- All actions in spec §6.3 dispatched and verified against `MockRelay`.
-- Offline queue: 100 actions dispatched while offline, all replay correctly on reconnect in scheduled order.
+- Bug-extinction #5 (account-context overlap): two accounts active, switch between them, assert no state bleed. `AppState` snapshot for account A never contains data scoped to account B's session-aware views.
+- Switching accounts during an in-flight publish: the publish is account-tagged, completes correctly, lands in the originating account's timeline only.
+- Per-account signer never signs an event for the wrong account (test forces dispatch through a wrong-account signer; the action ledger rejects).
 
-**Regression tests added.** `tests/actions_catalog.rs`, `tests/atomicity.rs`, `tests/offline_queue.rs`, `tests/multi_account.rs`.
+**Runnable artifact.** Account switcher in iOS demo with two real test accounts. Report in `docs/perf/m8/multi-account.md`.
+
+---
 
-### Phase 4 — Views end-to-end through FFI
+### M9 — NIP-17 DMs + NSE
 
-**Scope.** Phase 1 built the reactive machinery and Phase-1 view kinds Rust-side. Phase 4 completes the loop through FFI to platforms, fills in the remaining view kinds, and runs the view-catalog scenarios end-to-end.
+**Demo product:** Twitter slice gets a DMs tab. End-to-end NIP-17 gift-wrapped messages between two test accounts. Background push triggers iOS Notification Service Extension decryption; opening the app shows the message already in place.
 
-- All 15 view kinds from `docs/design/view-catalog.md` §2 (the Phase 5/6-deferred ones still get stubs that compile).
-- `dispatch(OpenView)` / `dispatch(CloseView)` / `dispatch(RefreshView)` / `dispatch(AdvanceCursor)` action handling.
-- `ViewBatch` emission across FFI; per-view-kind `ViewDelta` variants serialized via UniFFI.
-- View warmth (30s cache after last claim drop).
-- Platform shims (generated by Phase 10's CLI, manually written for now) for iOS + Android + desktop + web: refcounted domain-keyed wrappers per ADR-0005 (`useProfile`, `@Profile`, `rememberProfile`); reconciler routes `ViewBatch` deltas into typed domain-keyed dictionaries; wrappers translate component mount/unmount into `OpenView`/`CloseView` with 30s eviction grace period.
-- The five view-catalog scenarios from `docs/design/view-catalog.md` §11 run against the harness with the Phase 4 implementation.
+**Scope.** Per spec §7.10 and §7.14:
 
-**Prerequisite design docs.**
+**Subsystem deliverables.**
 
-- `docs/design/view-catalog.md` — the per-view-kind spec. The five fully-detailed kinds (Profile, Timeline, Thread, Reactions, Conversation) are the template; stubs get filled in here.
+- `nmp-nip17` protocol module: Conversation view module + ConversationList view module; SendDm action module; NIP-44 encryption / NIP-59 gift-wrapping; outbox routing for DMs (recipient inbox relays only — never public).
+- `nmp-nip17-nse` companion crate: `decrypt_push()` API with bounded memory (≤ 24 MB peak, ≤ 200 ms p99), reading from shared keychain and shared App Group storage.
+- iOS NSE target wiring: silent push from APNs → NSE invokes `decrypt_push` → notification posted with decrypted preview.
+- Action atomicity for `SendDm`: gift-wrap → publish to all recipient inboxes → insert locally — atomic.
 
 **Exit gate.**
 
-- Best-effort doctrine enforced: timeline view renders posts whose authors have no kind:0 yet; placeholders are shown; when kind:0 arrives, in-place update.
-- Cached-data-never-withheld: any kind:0 in store is served immediately by profile view; background refresh does not gate.
-- LLM-friendliness test (§3.4 of spec): a developer or LLM given only docs implements a new "hashtag screen" view kind in ≤ 1 hour, with no edits to `nmp-core`, that passes outbox routing + GC + lifecycle correctness on first compile.
-- `ViewBatch` reduces per-event marshaling vs equivalent `FullState`: under hashtag firehose, `ViewBatch`/sec ≤ 60 and `FullState`/sec ≤ 0.1.
-- All five view-catalog scenarios from `view-catalog.md` §11 pass:
-  - Profile fan-out p99 ≤ 5ms end-to-end.
-  - Hashtag firehose stays ≤ 60Hz / ≤ 1000 deltas/sec.
-  - Thread orphan storm builds correctly in ≤ 50ms.
-  - Reactions aggregation coalesces to ≤ 60 deltas/sec.
-  - Conversation paging interleaves without actor starvation.
+- Bug-extinction #4 (DM to public): no API path can send a DM to a non-inbox relay; planner refuses non-inbox relays for `p`-tagged-only events.
+- DM round-trip in `MockRelay` (alice ↔ bob): content matches; no plaintext crosses FFI other than as `ConversationMessage.body`.
+- NSE decrypt of an incoming gift-wrap: p99 ≤ 200 ms, peak memory ≤ 24 MB.
+- Backgrounded app receives a push, NSE decrypts and posts notification, app foregrounded shows the message in place (no re-fetch from relay).
+
+**Runnable artifact.** iOS Twitter slice with working DMs + push notifications. Report in `docs/perf/m9/messaging.md`.
+
+---
+
+### M10 — Blossom + media + long-running capabilities
 
-**Regression tests added.** `tests/views.rs`, `tests/best_effort_rendering.rs`, `tests/view_warmth.rs`, `tests/view_catalog/*` (one per kind), `bin/reactivity-bench/scenarios/view_catalog_*`.
+**Demo product:** Twitter slice user can attach a photo to a compose, see upload progress, and the published note has a valid Blossom URL. Profile-picture upload also works.
 
-### Phase 5 — Messaging (NIP-17 + NSE)
+**Scope.** Per spec §7.11. Establishes the **long-running capability lifecycle pattern** that the podcast app (M11) builds on:
 
-**Scope.** Per spec §7.10, §7.14.
+**Subsystem deliverables.**
 
-- NIP-17 conversation layer over NIP-44 + NIP-59.
-- 1:1 and group DMs.
-- Conversation list + conversation views (using Phase 4 view machinery).
-- Action atomicity for `SendDm`: gift-wrap, publish to each recipient's inbox relays, insert locally — atomic.
-- `nmp-nse` crate: `decrypt_push()` with bounded memory; reads from shared keychain + shared storage; no actor.
-- iOS NSE shim demonstrating it.
-- Android `FirebaseMessagingService.onMessageReceived` shim demonstrating it.
+- `nmp-blossom` protocol module: upload action module + download action module + media view module + upload-status view (progress).
+- `FilePickerCapability` real implementation on iOS (PHPicker for photos / `UIDocumentPicker` for files).
+- `BlossomCapability` callback interface: kernel asks platform to perform an HTTP PUT with progress; platform reports progress + completion back via reverse callback into the actor.
+- Long-running action lifecycle: upload registers in the action ledger as `AwaitingCapability`; capability progress updates the ledger row; restart recovery resumes from the last checkpointed progress.
+- Resumable uploads (Blossom range support where the server allows).
+- BUD-01 / BUD-02 protocol support.
 
 **Exit gate.**
 
-- DM round-trip in `MockRelay`: alice sends, bob receives, content matches, no plaintext crossing FFI other than as conversation view payload field.
-- NSE crate decrypts a push event in ≤ 200 ms with ≤ 24 MB peak memory.
-- Bug-extinction test #4 (DM to public): cannot send a DM to a non-inbox relay through any public API path.
-- Background-decryption test: app backgrounded, push arrives, NSE decrypts, notification posted, app foregrounded — conversation view shows the message without re-fetching.
+- Upload a 5 MB photo on iOS, kill the app mid-upload, restart — upload resumes from the checkpoint, does not restart from byte 0.
+- Cancellation works mid-upload (capability reports back `Cancelled`; ledger row finalizes correctly).
+- Slow-network upload remains responsive — main UI is never blocked.
+- Profile picture update through compose → kind:0 republish with new Blossom URL → in-place refinement across all open Profile / Timeline payloads (per doctrine D1).
+
+**Runnable artifact.** iOS Twitter slice with media compose. Report in `docs/perf/m10/blossom.md`.
+
+---
+
+### M11 — Podcast app (the kernel-boundary proof in a non-social domain)
+
+**Demo product:** A podcast app built entirely as an extension-module set, sharing nothing app-specific with `nmp-core`. Subscribes to podcast feeds. Downloads episodes. Plays them with background audio. Resumes playback position across app launches. Pulls feed updates via Nostr where available, RSS where not.
+
+**This is the load-bearing kernel-boundary check.** If the kernel needs even one podcast noun to make this work, the boundary is wrong and we go back to fix it.
+
+**Scope.**
+
+**Subsystem deliverables (extension modules — not in `nmp-core`):**
+
+- `podcast-core` app crate:
+  - `DomainModule`s: `Podcast`, `Episode`, `Transcript`, `PlayerState`, `Subscription`.
+  - `ViewModule`s: `PodcastLibrary`, `EpisodeDetail`, `NowPlaying`, `EpisodeQueue`.
+  - `ActionModule`s: `SubscribePodcast`, `RefreshFeed`, `DownloadEpisode`, `Play`, `Pause`, `Seek`, `MarkPlayed`, `ImportRss`.
+  - `IdentityModule::AppLocal` if anonymous subscription syncing across devices is wanted.
+
+**Subsystem deliverables (capabilities added to the kernel's reusable set):**
+
+- `AudioPlaybackCapability`: kernel asks the platform to play a URL or local file; platform reports position events + state transitions back. iOS implementation via `AVPlayer` + background-audio entitlement.
+- `BackgroundWorkCapability`: kernel registers periodic background tasks (feed refresh, scheduled downloads); platform implements via BGTask scheduler (iOS) / WorkManager (Android).
+- `LocalNotificationCapability`: extended for episode-available alerts.
+- `HttpCapability`: extended for podcast feed fetch (long-running streaming response).
+
+**Subsystem deliverables (protocol modules):**
+
+- `nmp-podcast` (or whatever the Nostr podcast NIP is called, e.g. NIP-XX for podcast feed events): parsed feed events. If no NIP, the app uses RSS via the action ledger to fetch + parse, storing entries as domain records.
+
+**Exit gate (kernel boundary).**
+
+- **`nmp-core` gains zero podcast nouns.** No `Podcast`, `Episode`, `Transcript`, `Player`, `Feed` types added to the kernel. Verified by grep + manual review at the commit.
+- **The capability families added in M11 are general** (audio playback, background work, local notifications, HTTP). Their request/response shapes are not podcast-specific.
+- **Reactivity behavior is identical** to the social demo — composite-key dependencies, delta coalescing, claim-based GC, ADR-0007 diagnostics all work for podcast view modules.
+
+**Exit gate (product).**
+
+- Subscribe to 5 real podcasts (use any well-known Nostr-podcast feeds if available, plus RSS imports).
+- Download an episode in the background while the app is suspended.
+- Play it with background audio while the iPhone is locked.
+- Resume playback at the correct position after a kill-relaunch.
+- Push notification on a new episode arrival.
+
+**Runnable artifact.** A second iOS app (`ios/NmpPodcast`) — distinct binary, same Rust kernel, different module set. Report in `docs/perf/m11/podcast-app.md` documenting the kernel-boundary verification.
+
+---
+
+### M12 — Wallet (NWC + zaps + Cashu + nutzaps)
 
-**Regression tests added.** `tests/messaging.rs`, `tests/nse_memory.rs`, `tests/dm_routing.rs`.
+**Demo product:** Twitter slice gets a zap button on each post. Tapping it pays via NWC. Receiving zaps shows up in a zap-history view. Cashu nutzap claim works.
 
-### Phase 6 — Wallet + WoT + Blossom
+**Scope.** Per spec §7.9:
 
-**Scope.** Per spec §7.9, §7.7, §7.11.
+**Subsystem deliverables.**
 
-- NWC client; pay/receive lightning.
-- LUD-16 zaps; zap receipt verification automatic.
-- Cashu (NIP-60) + nutzaps (NIP-61).
-- Web-of-trust subsystem with default scoring (in-degree depth-weighted); pluggable trait.
-- Blossom client (BUD-01/02); upload + download; reactive `MediaState`.
+- `nmp-nwc` protocol module: NIP-47 client; pay/receive/balance.
+- `nmp-nip57` protocol module: LUD-16 discovery + zap request building + receipt verification.
+- `nmp-nip60` protocol module: Cashu wallet event types + proof state in domain store.
+- `nmp-nip61` protocol module: Nutzap action module; pending-nutzap claim flow.
+- `WalletBalance` view module; `ZapHistory` view module.
+- Zap action module: `Zap { target, sats, comment }` on the action ledger.
 
 **Exit gate.**
 
-- Pay a zap end-to-end against a mock LN node; receipt verifies; balance updates.
-- WoT toggle visibly reorders timeline based on score; off-toggle restores chronological order.
-- Blossom upload progresses through `MediaState`; cancellation works.
+- Pay a 100-sat zap via NWC to a real LUD-16 endpoint; receipt verifies; balance updates within one ViewBatch.
+- Receive a zap (test via a separate device or simulated): zap-history view reflects within one ViewBatch.
+- Nutzap claim from a Cashu mint: proofs land in the wallet; balance updates.
+- Wallet operations never block the UI thread.
+
+**Runnable artifact.** iOS Twitter slice with working zaps. Report in `docs/perf/m12/wallet.md`.
+
+---
+
+### M13 — Web-of-Trust
 
-**Regression tests added.** `tests/wallet.rs`, `tests/wot.rs`, `tests/blossom.rs`.
+**Demo product:** Twitter slice gets a "score-filtered timeline" toggle. With it on, low-WoT-score authors are de-prioritized; toggling off restores chronological order.
 
-### Phase 7 — Web target
+**Scope.** Per spec §7.7:
 
-**Scope.** Per spec §6 (web), §10 (open questions resolved here).
+**Subsystem deliverables.**
 
-- `nmp-wasm` mature: full `FfiApp` equivalent over wasm-bindgen.
-- IndexedDB storage backend; OPFS for browsers that support it.
-- NIP-07 capability bridge for web signing.
-- Web shell with TypeScript types and a reactive store.
+- `nmp-wot` protocol module:
+  - Action: `LoadFollowGraph { root: PubKey, depth: u8 }` — populates an in-memory follow graph.
+  - Projection cache: `wot_score: HashMap<PubKey, f32>`.
+  - View module: `WotRank` exposes per-pubkey score + reasoning.
+  - Filter view module wrapper: composes with Timeline to produce a score-filtered variant.
+- Pluggable scoring trait (default: depth-weighted in-degree).
 
 **Exit gate.**
 
-- Cross-platform consistency tests (§3.5 of spec) pass on web: same action sequence produces byte-identical `AppState` JSON as on iOS/Android/desktop.
-- Web cold-start to first painted timeline ≤ 2s on a modern browser.
-- Web works in incognito (no persistent storage) by falling back to in-memory store with a visible warning.
+- Load follow graph rooted at the active account to depth 2; computes scores for 10k+ pubkeys in ≤ 5 s on iPhone 12.
+- Score-filtered timeline visibly reorders / hides low-score authors; toggle off restores chronological.
+- New kind:3 arrival incrementally updates scores without full recompute.
 
-**Regression tests added.** `tests/web_consistency.rs`, `tests/web_storage_fallback.rs`.
+**Runnable artifact.** iOS Twitter slice with WoT toggle. Report in `docs/perf/m13/wot.md`.
 
 ---
 
-## Arc 2 — Proof app + performance pass (Phases 8–9)
+### M14 — UniFFI migration
 
-### Phase 8 — Build the proof app
+**Demo product:** iOS app, podcast app, and (incoming) Android/Desktop/Web shells all bind to the kernel via UniFFI-generated bindings produced by `nmp gen modules`, not raw C FFI.
 
-**Scope.** Per spec §4.5.
+**Scope.** Replace the current raw C FFI surface in `crates/nmp-core/src/ffi.rs` with the per-app generated `nmp-app-<name>` crate per ADR-0010. The iOS app stops importing `NmpCore.h` and instead imports the generated Swift module.
 
-Build `nmp-proof` on all four platforms. Feature set in the spec; the goal here is **wiring**, not new framework features. If a feature is hard to wire, that's a framework defect to be fixed back in Arc 1.
+**Subsystem deliverables.**
 
-- iOS: SwiftUI app with all proof-app screens.
-- Android: Compose app with all proof-app screens.
-- Desktop: iced app with all proof-app screens.
-- Web: TS/React or Solid shell with all proof-app screens.
-- Performance overlay implemented per-platform reading from `AppState.debug`.
-- Scripted scenario harness in `nmp-testing` driving the proof app through canonical flows.
+- `nmp-codegen` extended to produce UniFFI scaffolding in the generated per-app crate.
+- `apps/twitter/nmp-app-twitter` and `apps/podcast/nmp-app-podcast` as the first two real per-app crates.
+- `xcframework` build pipeline for each per-app crate.
+- Generated Swift wrappers: `useProfile`, `@Profile`, `useTimeline`, `@Wallet`, etc.
+- CI gate: `nmp gen modules --check` fails the build if bindings drift.
 
 **Exit gate.**
 
-- Proof app launches on all four platforms and successfully exercises every framework subsystem.
-- The cross-platform consistency test script runs against the proof app on all four platforms; `AppState` JSON snapshots match byte-for-byte at each checkpoint.
-- The performance overlay renders all counters from spec §7.16 live.
-- Total proof-app platform code stays within the budgets from spec §3.2.
+- iOS app builds and runs against UniFFI-generated bindings; no raw C FFI in the app target.
+- Cross-platform consistency test (next milestone) is unblocked because the FFI shape is now identical across platforms.
+- Codegen determinism: repeated runs produce byte-identical output.
 
-**Regression test added.** `tests/proof_app_consistency.rs` — the canonical scenario script.
+**Runnable artifact.** iOS Twitter + iOS Podcast apps both using UniFFI. Report in `docs/perf/m14/uniffi-migration.md`.
 
-### Phase 9 — Performance pass (firehose-bench + device measurements)
+---
 
-**Scope.** Take measurements on real hardware end-to-end. Fix budget regressions. Tune.
+### M15 — Cross-platform: Android + Desktop + Web
 
-The `firehose-bench` harness (per `docs/design/firehose-bench.md`) is the load-bearing tool here. It runs in three modes: **live** (real relays, real network), **capture** (records live to a trace), **replay** (deterministic re-execution against `MockRelay`). Replay is what CI uses; live + capture are for soak testing and trace refresh.
+**Demo product:** Same Twitter slice and (where capabilities allow) podcast slice running on Android (Compose), Desktop (iced), and Web (wasm + React/Solid TBD). Cross-platform consistency test passes — same scripted scenario produces byte-identical `AppState` JSON on all four platforms.
 
-Eight scenarios target distinct concerns (`firehose-bench.md` §3): cold_start, sustained_firehose, profile_thrashing, relay_disconnect_storm, multi_account, negentropy_efficiency, background_decryption, soak (24h live).
+**Scope.**
 
-The harness ships pieces earlier (per `firehose-bench.md` §6): `live` + `capture` infrastructure in Phase 1; cold_start + relay_disconnect_storm + negentropy_efficiency scenarios in Phase 2 (gating the sync engine); sustained_firehose + profile_thrashing in Phase 4 (gating views end-to-end + ADR-0005 wrappers); multi_account in Phase 3; background_decryption in Phase 5; full soak in Phase 9.
+**Android port (~3 weeks):**
 
-**Reference devices:**
+- Kotlin bindings via UniFFI; cargo-ndk + Gradle pipeline.
+- Compose shell mirroring the iOS SwiftUI shell.
+- `KeychainCapability` Android impl via `EncryptedSharedPreferences`.
+- `nmp-nip55` Amber external-signer capability module.
+- Android `FirebaseMessagingService` integration with `nmp-nip17-nse` for DM push.
 
-- **iOS:** iPhone 12 (mid-range, ~5 years old at v1 ship).
-- **Android:** Pixel 6a or equivalent.
-- **Desktop:** Linux laptop with integrated graphics; macOS M1.
-- **Web:** Firefox + Chrome + Safari on the above desktop.
+**Desktop port (~2 weeks):**
 
-**Measurements** (collected by `nmp-metrics`, dumped via `EmitDiagnosticSnapshot`):
+- iced shell (the development-time reference target lives on; this milestone graduates it to a shipping target).
+- macOS + Linux + Windows.
+- `KeychainCapability` impls per OS (macOS Keychain, Secret Service, Windows Credential Manager — already exists in `nostr-keyring`).
 
-- All counters from spec §7.16 under three workloads:
-  - **Idle** — app open, nothing happening.
-  - **Following timeline scroll** — user with 1k follows, scrolling at typical mobile flick speed.
-  - **Hashtag firehose** — `#nostr` or similar; 200+ events/sec.
-- Cold-start to first painted frame.
-- Memory footprint at idle, after 5 minutes of activity, after 1 hour.
-- Battery proxy (mobile): wakelock duration, CPU time.
+**Web port (~3 weeks):**
 
-**Budgets** (spec §7.16) are the targets. Failures are tracked as bugs and fixed in-arc.
+- `nmp-wasm` mature.
+- IndexedDB storage backend; OPFS where supported.
+- `nmp-nip07` browser-signer capability module.
+- Web shell stack TBD (React + signals / Solid / Svelte — pick at start of milestone).
 
-**Outputs:**
+**Subsystem deliverables.**
 
-- `docs/perf/v1.md`: written report with measurements, comparisons across platforms, identified bottlenecks, decisions made.
-- Revised budgets if reality dictates (with rationale).
-- Open issues for any deferrable bottlenecks.
+- Cross-platform consistency test in `nmp-testing` — drives same scripted action sequence on all four targets, snapshots `AppState` JSON at checkpoints, asserts byte-equal.
+- Per-platform performance reports.
 
 **Exit gate.**
 
-- All §7.16 budgets met on reference devices, OR explicitly waived with rationale documented.
-- No platform shows visible jank under the three workloads on reference devices.
-- `docs/perf/v1.md` published.
-- The **SQLite-as-shared-store hybrid** (spec §A2) decision is made on data: either v2 path declared, or marshaling pattern declared sufficient.
+- Twitter clone identical scripted scenario produces byte-identical `AppState` snapshots on iOS / Android / Desktop / Web.
+- All §7.16 performance budgets met on reference devices (iPhone 12, Pixel 6a, M1 mini, modern browsers).
+- Web works in incognito mode by falling back to in-memory store with a visible warning.
 
-**Regression test added.** `tests/perf_replay.rs` runs a canned workload in CI and asserts on the always-on counters in the proof app's reported snapshot. Catches regression between releases.
+**Runnable artifact.** Four-platform demo. Report in `docs/perf/m15/cross-platform.md`.
 
 ---
 
-## Arc 3 — Release (Phases 10–11)
+### M16 — CLI + starter app + recipe book
 
-### Phase 10 — CLI, starter app, docs
+**Demo product:** A developer with no prior framework knowledge runs `nmp init my-app`, follows recipes, ships a working hashtag-feed app on all four platforms in ≤ 2 hours.
 
-**Scope.** Per spec §8, §4.3, §4.5.
+**Scope.**
 
-- `nmp init` with all platform options.
-- `nmp add ios|android|desktop|web`.
-- `nmp gen bindings|view|action|screen`.
-- `nmp doctor`.
-- `nmp upgrade`.
-- The **starter app** (distinct from proof app; minimal): login + timeline + compose + profile + DMs. Stays under the platform LOC budgets from spec §3.2.
-- Documentation set: recipe book (`docs/recipes/`), NIP support matrix (`docs/nips.md`), migration guide (`docs/migration.md`).
+**Subsystem deliverables.**
+
+- `nmp init`, `nmp add module`, `nmp gen modules`, `nmp doctor`, `nmp upgrade` commands.
+- A minimal **starter app** (distinct from the proof/Twitter app) implementing only: login + timeline + compose + profile + DMs. Stays under the platform LOC budgets from spec §3.2.
+- Recipe book in `docs/recipes/`: one recipe per common app shape (timeline-only viewer, kind-filtered explorer, long-form reader, etc.).
+- NIP support matrix in `docs/nips.md`.
+- Migration guide in `docs/migration.md`.
 
 **Exit gate.**
 
-- A developer with no prior framework knowledge can `nmp init`, follow recipes, and have a working hashtag-feed app on all four platforms in ≤ 2 hours.
-- §3 of the spec (success criteria) is reproducible from published docs alone — no insider knowledge required.
+- §3 success criteria of the spec reproducible from published docs alone, no insider knowledge.
+- One external developer (or an LLM agent with no prior context) succeeds at building a small custom app from the starter + recipes in ≤ 2 hours.
+
+**Runnable artifact.** Public `nmp init` flow. Report in `docs/perf/m16/dx.md`.
+
+---
 
-### Phase 11 — v1 release
+### M17 — v1 release
 
 **Scope.**
 
 - Resolve naming (`aim.md` §7.7).
 - Publish crates to crates.io.
-- Publish CLI to npm as `@nmp/cli` (with final name substituted).
-- Tag release; publish bindings; deploy example apps; announce.
+- Publish CLI to npm as `@<name>/cli`.
+- Tag release; publish bindings; deploy example apps; write release announcement.
 
 **Exit gate.**
 
-- Public availability.
+- Public availability on crates.io and npm.
 - Three external developers ship a real app within 30 days of release.
+- v1 release report in `docs/perf/v1/release.md`.
 
 ---
 
-## Test pyramid
+## 3. Subsystem coverage matrix
 
-| Level | Tooling | What it covers | Where it lives |
-|---|---|---|---|
-| Unit | `cargo test` per crate | Pure-function correctness | Each crate's `tests/` |
-| Subsystem integration | `cargo test --test '*'` in `nmp-testing` | EventStore + planner + sync against MockRelay | `nmp-testing/tests/` |
-| Cross-FFI | `cargo test --features ffi` | Bindings round-trip, rev ordering, callback delivery | `nmp-ffi/tests/` |
-| Cross-platform consistency | Script harness | Same scenario on iOS sim + Android emu + desktop + headless web; assert `AppState` JSON equality | `nmp-testing/scenarios/` |
-| Proof-app smoke | XCUITest + Espresso + iced UI test + Playwright | End-to-end flows render without error | `nmp-proof/<platform>/tests/` |
-| Performance | `nmp-metrics` replay | Counters under canned workloads | `nmp-testing/perf/` |
-| Manual exploratory | Humans on reference devices | What the metrics can't catch | Phase 9 |
+Cross-reference of which milestone delivers which user-specified concern.
 
-The cross-platform consistency tests are the highest-value layer: they catch every drift between platforms and force the doctrine (Rust owns everything but rendering) to remain real.
+| Concern | Milestone(s) | Notes |
+|---|---|---|
+| **Outbox routing (NIP-65)** | M2 | First-class as a planner stage, not a side feature. Diagnostics show per-relay coverage. |
+| **NDK-style subscription aggregation** | M2 | Per `docs/design/ndk-applesauce-lessons.md` §7, the planner becomes a subscription compiler. Logical interests → per-relay plans → wire REQs, semantics-preserving merge/split. |
+| **Reactivity as planned** | M0–M7 | Already validated by reactivity-bench run 002 against the model; M1 runs the same code path against real iOS; subsequent milestones add view modules that exercise the contract under varied loads. |
+| **Non-Nostr data bridge** | M0 (substrate), M10 (long-running capabilities), M11 (podcast app proves it in production) | DomainModule trait + ADR-0007 bridge lanes; first proven by fixture-todo-core; production proof in podcast app. |
+| **NIP-42 auth** | M5 | Per-relay auth state machine; integrates with diagnostics; works with both local-key and NIP-46 signers. |
+| **Blossom** | M10 | Upload + download with resumable progress; long-running capability lifecycle. |
+| **Multi-session clients** | M8 | Per-account view-spec scoping; account switcher; isolation tests. |
+| **NIP-77 negentropy** | M4 | Sync engine with watermarks; planner consults before REQ; capability negotiation; bytes-saved diagnostic. |
+| **Podcast-class apps** | M11 (proof), M10 (capabilities prerequisite) | AudioPlaybackCapability, BackgroundWorkCapability, BlossomDownloadCapability all generic; podcast-specific domain in `podcast-core` app crate. |
 
----
+### NIP support roadmap at v1
 
-## Decision log (where we'll keep deviation receipts)
+| NIP | Module | Milestone | Status |
+|---|---|---|---|
+| 01 | nmp-nip01 | M1, M6 | partial (reads in M1; writes in M6) |
+| 02 | nmp-nip02 | M2 | follow-list parsing (contacts view) |
+| 04 | not v1 | — | superseded by NIP-44/17; not implemented |
+| 05 | nmp-nip01 | M1 | NIP-05 verification in Profile module |
+| 07 | nmp-nip07 | M15 | web-only browser signer |
+| 09 | nmp-nip01 | M3 | kind:5 deletes (full handling) |
+| 10 | nmp-nip10 | M7 | reply markers in thread building |
+| 17 | nmp-nip17 | M9 | DMs |
+| 19 | nmp-nip19 | M1 | bech32 utility used throughout |
+| 23 | not v1 | — | long-form reader is post-v1 |
+| 25 | nmp-nip25 | M7 | reactions |
+| 40 | nmp-nip01 | M3 | expiration scheduling |
+| 42 | nmp-nip42 | M5 | relay auth |
+| 44 | nmp-nip17 | M9 | encryption (via NIP-17) |
+| 46 | nmp-nip46 | M6 | bunker signer |
+| 47 | nmp-nwc | M12 | wallet connect |
+| 49 | nmp-nip01 / nmp-nip46 | M6 | encrypted-key import |
+| 55 | nmp-nip55 | M15 | Android Amber bridge |
+| 57 | nmp-nip57 | M12 | zaps |
+| 59 | nmp-nip17 | M9 | gift wrap (via NIP-17) |
+| 60 | nmp-nip60 | M12 | Cashu |
+| 61 | nmp-nip61 | M12 | nutzaps |
+| 65 | nmp-nip65 | M2 | mailboxes + outbox |
+| 77 | nmp-nip77 | M4 | negentropy |
+| Blossom BUD-01/02 | nmp-blossom | M10 | media |
+
+NIPs not in v1 (e.g., NIP-29 groups, NIP-23 long-form, NIP-71 video) become post-v1 extension modules; the kernel boundary makes them additive.
 
-`docs/decisions/` will hold one short markdown per non-trivial decision made during Arcs 1–3. Format:
+---
 
-```
-# ADR N: <title>
-Date: YYYY-MM-DD
-Status: proposed | accepted | superseded
+## 4. Parallelization opportunities
 
-## Context
-## Decision
-## Consequences
-## Alternatives considered
-```
+The ladder above is the **dependency order** — what must precede what — not a wall-clock schedule. Genuine parallel work tracks:
 
-Initial ADRs to write at the start of Phase 0 (from the spec itself):
+- **M2 (outbox), M3 (LMDB), M4 (negentropy)** can pipeline tightly: M3 + M4 are almost mechanically pluggable once M2's compiled-plan abstraction exists.
+- **M5 (NIP-42)** is independent of M3/M4 and can be done alongside.
+- **M6 (signer + write path) is a serialization point** — most downstream milestones (M7, M8, M9, M10, M12) depend on it. Land this fast.
+- **M15 (Android + Desktop + Web)** is three parallel tracks once M14 (UniFFI) lands.
+- **M11 (podcast app)** can begin as soon as M10 (Blossom + long-running capabilities) is in good shape, even if M12/M13 haven't started.
 
-1. Snapshots + ViewBatch from day one (vs snapshot-only MVP).
-2. Negentropy promoted to engine, not feature.
-3. View payloads are non-optional with placeholders (D1).
-4. SQLite-shared-store explicitly deferred to v2 pending Phase 9 data.
-5. Proof app is a v1 release gate.
-6. Starter app stays minimal even though we have a richer proof app.
+A team of two could run M5 alongside the M2–M4 sequence with no integration risk.
 
-ADRs already adopted:
+---
 
-- **ADR-0001:** Composite dependency keys (composite-first reverse index, broad axes guardrailed). Adopted 2026-05-17 from reactivity-bench run 001.
-- **ADR-0002:** Delta-volume budget is per-view (60/view/sec), not absolute. Adopted 2026-05-17 from reactivity-bench run 001.
-- **ADR-0003:** Memory budget is working-set, not total cached events. Adopted 2026-05-17 from reactivity-bench run 001.
-- **ADR-0004:** Allocation measurement plumbed via counting allocator (verifies zero-per-event invariant). Adopted 2026-05-17 from reactivity-bench run 001.
-- **ADR-0005:** Platform shadow is domain-keyed, not `ViewId`-keyed. Refcounted component wrappers (`useProfile`, `@Profile`, `rememberProfile`) generated per platform manage subscription lifecycle behind the domain-keyed API. `ViewId` remains an internal FFI token only.
-- **ADR-0006:** Vertical-slice-first delivery for Phase 1. Kind:0 profile-metadata path runs end-to-end (desktop component → wrapper → actor → in-memory store → real relay → back) before the broader Phase 1 scope (LMDB, outbox, full view kinds, FFI to iOS/Android) layers on top. Adopted 2026-05-17 from the firehose-bench run that revealed the live mode was blocked on real runtime adapters.
-- **ADR-0007:** Relay/subscription diagnostics and non-Nostr data use the same actor-owned `AppUpdate` bridge, but with explicit diagnostic/domain records instead of raw callbacks or fake Nostr events. Adopted 2026-05-17 to clarify network visibility and capability/domain-data flow before expanding the vertical slice.
-- **ADR-0008:** Phase 1a demo target is a simple Twitter-clone iOS app pulling from primal, with seed-driven timeline discovery (union of follow lists of hardcoded dev accounts) as the unauthenticated default. Sub-phases, each a walking skeleton. Desktop iced reference target preserved alongside iOS for UniFFI-vs-architecture debugging. Supersedes ADR-0006 in choice of demo target only; discipline preserved. Modified by ADR-0009.
-- **ADR-0009:** App extension kernel boundary. NMP is reframed as a Nostr-native app kernel with five extension trait families (`DomainModule`, `ViewModule`, `ActionModule`, `CapabilityModule`, `IdentityModule`). The kernel owns substrate; protocol modules and app crates own nouns. Closed enums in the API are replaced by per-app generated enums (see ADR-0010). Phase 1a restructured to build the kernel substrate (with a non-Nostr fixture module) before the first Nostr protocol module. Adopted 2026-05-17 from `docs/design/app-extension-kernel.md`.
-- **ADR-0010:** Per-app concrete enums generated at the FFI boundary. `nmp gen modules` reads `nmp.toml`, resolves the chosen module set, and produces a `nmp-app-<name>` crate exposing typed `AppAction` / `AppUpdate` / `ViewSpec` / capability traits via UniFFI. Compile-time type safety end-to-end; per-platform idiomatic enums; tree-shaking of unused modules. Codegen is critical-path v1 infrastructure.
+## 5. Test pyramid
 
-The ADRs are the durable record of why design decisions exist. New ADRs land alongside any new harness run that revises a design.
+| Level | Tooling | What it covers | Where it lives |
+|---|---|---|---|
+| Unit | `cargo test` per crate | Pure-function correctness, substrate trait invariants, codegen determinism | Each crate's `tests/` |
+| Subsystem integration | `cargo test --test '*'` in `nmp-testing` | EventStore + planner + sync against MockRelay | `crates/nmp-testing/tests/` |
+| Cross-FFI | UniFFI binding round-trip tests | Bindings stability, rev ordering, callback delivery | `apps/<name>/nmp-app-<name>/tests/` (post-M14) |
+| Cross-platform consistency | Script harness | Same scenario on iOS sim + Android emu + desktop + headless web; assert `AppState` JSON byte-equal | `nmp-testing/scenarios/` |
+| Reactivity bench | `reactivity-bench --standard --fail-on-gate` | Composite reverse index, delta coalescing, working-set memory, allocation gates | `crates/nmp-testing/bin/reactivity-bench/` |
+| Firehose bench (modeled) | `firehose-bench replay --standard --fail-on-gate` | Budget contract for the runtime | `crates/nmp-testing/bin/firehose-bench/` |
+| Firehose bench (live) | `firehose-bench live` against the real iOS app | Runtime evidence end-to-end | reports in `docs/perf/m<N>/` |
+| Per-app UI smoke | XCUITest + Espresso + iced UI test + Playwright | End-to-end flows render without error | `ios/<app>/UITests/` etc. |
+| Manual exploratory | Humans on reference devices | What metrics can't catch | per-milestone manual checklist |
+
+The cross-platform consistency tests are the highest-value tier post-M15.
 
-### The harness-first pattern
+---
 
-Every design doc has measurable gates. Gates run on the reactivity-bench harness (or a sibling for non-reactivity subsystems). Failures revise the design *before* implementation. Pre-implementation measurement is cheaper than post-implementation rework. Run 001 of reactivity-bench established the pattern: the reverse-index direction was validated (100×–1000× headroom), one design refinement landed (composite keys), and two budget bugs surfaced (per-view delta, working-set memory) — all before any view-kind code shipped.
+## 6. CI / pre-merge hygiene
 
-### Modeled budget contract vs runtime evidence
+Required CI gates (apply from the milestone they become possible):
 
-Two distinct claims about the same harness:
+- `cargo fmt --all -- --check` (always).
+- `cargo test --workspace` (always).
+- `cargo run -p nmp-codegen -- gen modules --check` (codegen determinism, from M0).
+- `cargo run -p nmp-testing --bin reactivity-bench --release -- --standard --fail-on-gate` (from M0).
+- `cargo run -p nmp-testing --bin firehose-bench --release -- replay --standard --fail-on-gate` (from M0).
+- iOS build (`just build-ios`) from M1.
+- iOS UI test (`xcrun simctl test`) from M1.
+- Android build from M15.
+- Desktop build from M15.
+- Web build from M15.
+- Cross-platform consistency test from M15.
 
-- **Modeled budget contract.** Replay mode runs deterministic synthetic workloads through a model of the runtime (modeled relay sockets, modeled storage, modeled UniFFI marshaling). Passing here proves the budgets are internally consistent and the harness scaffolding is sound. It does **not** prove the real runtime hits those budgets.
-- **Runtime evidence.** Live mode (or replay mode against a real adapter substituted for a modeled segment) runs against actual LMDB, actual WebSockets, actual UniFFI marshaling. Passing here is real evidence.
+Live firehose runs are not in pre-merge CI (would block on relay flakes); they run nightly on a dedicated runner and produce reports tagged `live` in `docs/perf/m<N>/`.
 
-Today's firehose-bench replay passes establish the contract. The vertical slice (ADR-0006) is what produces the first runtime evidence. Each subsequent phase replaces another modeled segment with a real adapter and graduates the corresponding firehose-bench scenarios from "modeled" to "measured."
+---
 
-Reports in `docs/perf/firehose-bench/` track which scenarios are measured vs modeled at each run. Live runs are explicitly tagged. CI runs replay against the current set of real adapters plus models for the rest; the boundary moves rightward as phases land.
+## 7. Decision log
 
-### CI / pre-merge hygiene
+ADRs live in `docs/decisions/`. Format per the template in older revisions of this plan. Currently:
 
-The recommended CI gates as of Phase 1:
+- **ADR-0001**: Composite dependency keys (composite-first reverse index; broad axes guardrailed).
+- **ADR-0002**: Per-view delta budget (60/view/sec, not absolute).
+- **ADR-0003**: Working-set memory budget (hot/cold split, not total events).
+- **ADR-0004**: Allocation measurement via counting allocator.
+- **ADR-0005**: Domain-keyed platform shadow + refcounted component wrappers.
+- **ADR-0006**: Vertical-slice-first delivery (modified by ADR-0009; the slice now layers on the kernel substrate).
+- **ADR-0007**: Diagnostics and non-Nostr data over the actor-owned bridge with explicit records, not raw callbacks or fake Nostr events.
+- **ADR-0008**: Twitter-clone iOS as the Phase 1a demo target (modified by ADR-0009 — repositioned as first canonical extension-module set).
+- **ADR-0009**: App-extension kernel boundary. Five trait families, four layers, no app nouns in nmp-core.
+- **ADR-0010**: Per-app concrete enums generated at the FFI boundary. Codegen is critical-path v1 infrastructure.
 
-- `cargo fmt --all -- --check` (formatting).
-- `cargo test --workspace` (all crates pass unit + integration).
-- `nmp gen modules --check` (codegen determinism — fails if regenerating would produce a diff against the checked-in `nmp-app-<name>/` output).
-- `cargo run -p nmp-testing --bin reactivity-bench --release -- --standard --fail-on-gate` (reactivity gates).
-- `cargo run -p nmp-testing --bin firehose-bench --release -- replay --standard --fail-on-gate` (firehose gates against the current model+adapter mix).
-- `git diff --check` (whitespace / conflict markers).
+New ADRs land alongside any milestone whose execution revises a design.
 
-Live firehose runs are not in pre-merge CI (they would block on relay flakes); they run nightly or on-demand and produce reports tagged `live` in `docs/perf/firehose-bench/`.
+### The harness-first pattern
 
-### Unit-test guidance from firehose-bench run 001
+Every design doc has measurable gates. Gates run on the reactivity-bench harness (or `firehose-bench` for end-to-end behavior). Failures revise the design **before** implementation. Pre-implementation measurement is cheaper than post-implementation rework. Run 001 of reactivity-bench established the pattern: the reverse-index direction was validated (100×–1000× headroom), one design refinement landed (composite keys), and two budget bugs surfaced (per-view delta, working-set memory) — all before any view-kind code shipped.
 
-Beyond integration tests, Phase 1 explicitly carries unit-test coverage for:
+### Modeled budget contract vs runtime evidence
+
+Two distinct claims about the same harness:
 
-- **Composite reverse index** — composite-key matching, false-wakeup rate measurement, broad-axis guardrail warnings.
-- **Coalescer** — per-view-kind merge rules (`UpdatedMany`, range-merged `Inserted`, `EmojiAdjusted` summing, etc.) preserve semantic equivalence to N un-coalesced deltas.
-- **Domain-keyed wrapper lifecycle** — refcount transitions, grace-period cancellation, eviction correctness, idempotent `Open`/`Close` calls.
+- **Modeled budget contract.** Replay mode runs deterministic synthetic workloads through a model of the runtime. Passing here proves budgets are internally consistent and the harness scaffolding is sound. Does **not** prove the real runtime hits those budgets.
+- **Runtime evidence.** Live mode (or replay mode with real adapters substituted for modeled segments) runs against actual LMDB, actual WebSockets, actual UniFFI marshaling. Passing here is real evidence.
 
-These three areas had the highest concentration of subtle invariants in the design docs; they get the highest test density.
+Each milestone moves the boundary rightward — replaces another modeled segment with a real adapter and graduates the corresponding firehose-bench scenarios from `modeled` to `measured` in `docs/perf/`.
 
 ---
 
-## What this plan is not
+## 8. What this plan is not
 
-- It is not a schedule. No dates, no person-months. Phases are sequential; their durations depend on team size and surface complexity.
-- It is not a marketing roadmap. v1 ships when the gates are met, not on a calendar.
-- It is not exhaustive. Sub-tasks within each phase will be planned in design docs as we enter the phase.
-- It does not commit to specific dependency versions; those go in `docs/nips.md` and `Cargo.toml` and evolve.
+- **Not a schedule.** No dates, no person-months. Milestones are sequential; their durations depend on team size and surface complexity. Estimates per milestone are guidance only.
+- **Not a marketing roadmap.** v1 ships when M17 gates are met, not on a calendar.
+- **Not exhaustive about post-v1 work.** NIP-29 groups, NIP-23 long-form, NIP-71 video, additional protocol modules, additional app demonstrations (Highlighter-lite, TENEX-lite, etc.) are post-v1 — they validate the kernel boundary further but are not v1 deliverables.
+- **Not silent about gaps.** §0 names exactly what is and isn't built. As the ladder progresses, §0 gets revised so the plan stays honest about state.
 
-The plan exists so that any single phase can be picked up cold by someone reading this doc + the spec + the relevant ADRs, and they can execute without bothering the rest of the team.
+The plan exists so that any single milestone can be picked up cold by someone reading this doc + `product-spec.md` + the relevant ADRs and design docs, and they can execute without bothering the rest of the team.

Review for: (1) doctrine compliance, (2) TODO/FIXME/unimplemented sneaking in, (3) file-size budget, (4) internal consistency with the plan's own stated scope. Be terse. If everything is fine, say so. If there is a real concern, name the file:line and what to fix.
codex
I’ll verify the checked-out `docs/plan.md` directly for line numbers, LOC, and any TODO-style markers, then cross-check the changed plan against the doctrines and its own stated milestone scope.
exec
/bin/zsh -lc 'wc -l docs/plan.md' in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
 succeeded in 0ms:
      63 docs/plan.md

exec
/bin/zsh -lc 'rg -n "TODO|FIXME|unimplemented|TBD|defer|deferred|later|post-v1|not implemented|not v1|maybe|whatever" docs/plan.md' in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
 succeeded in 0ms:
7:> **Each milestone is gated.** Every milestone ends with: a runnable artifact, automated tests in `nmp-testing`, a measured-numbers report in `docs/perf/m<N>/`, and an explicit ADR if a design decision was revised in flight. **No silent endings.** **No "for later" carve-outs** — if a slice is in the milestone scope, it ships in that milestone, or the milestone is not done.
22:- [Scope adjustments 2026-05-18](plan/scope-adjustments-2026-05-18.md) — M9 DMs + M12 Wallet deferred; M11.5 Highlighter added; framework-magic contract
23:- [Post-v1 milestones](plan/post-v1.md) — deferred M9 (DMs) and M12 (Wallet)
42:| ~~M9~~ | ~~NIP-17 DMs + NSE~~ | ~~Arc 1~~ | [deferred post-v1](plan/post-v1.md) |
47:| ~~M12~~ | ~~Wallet (NWC + zaps + Cashu + nutzaps)~~ | ~~Arc 3~~ | [deferred post-v1](plan/post-v1.md) |
60:- **Not exhaustive about post-v1 work.** NIP-23 long-form, NIP-71 video, and additional protocol modules are post-v1. NIP-29 groups ship in v1 via [M11.5](plan/m11.5-highlighter.md) (Highlighter rebuild). Additional app demonstrations (TENEX-lite, etc.) are post-v1.

exec
/bin/zsh -lc 'git show --stat --oneline e9cbafa && git show --name-only --format=medium e9cbafa' in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
 succeeded in 0ms:
e9cbafa docs(plan): consolidate into single milestone-driven plan with product checks
 docs/plan.md | 807 ++++++++++++++++++++++++++++++++++++++---------------------
 1 file changed, 519 insertions(+), 288 deletions(-)
commit e9cbafa93ebf0a7845879131584696265f37ee96
Author: Claude <noreply@anthropic.com>
Date:   Sun May 17 22:12:14 2026 +0000

    docs(plan): consolidate into single milestone-driven plan with product checks
    
    Rewrites docs/plan.md to reflect what's actually been built and to give
    a single ladder of seventeen milestones (M0-M17), each producing a
    runnable demoable product and a measured (not just modeled) report.
    
    What changed relative to the prior phased plan:
    
    - Section 0 honestly accounts for current state: kernel substrate
      (~3,800 LOC), live iOS app (~1,375 LOC Swift) pulling from
      primal, fixture-todo-core kernel-boundary proof, codegen tool
      with determinism, and the gaps (no LMDB, no outbox, no NIP-77,
      no NIP-42, no write path, no Blossom, no multi-account, no DM,
      no wallet/WoT, no Android/Desktop/Web, raw C FFI not UniFFI).
    - Replaced abstract phases with milestones tied to demoable
      products. Every milestone has an exit gate with measurable
      numbers and a runnable artifact.
    - Elevated subscription compilation (NDK/Applesauce lesson) to
      its own milestone M2, alongside NIP-65 outbox routing.
    - Added M5 NIP-42 auth as an explicit milestone (was not in the
      prior plan).
    - Added M11 podcast app as the load-bearing kernel-boundary proof
      in a non-social domain. Adds AudioPlaybackCapability,
      BackgroundWorkCapability, and proves no podcast nouns leak into
      nmp-core.
    - Expanded M10 Blossom to establish the long-running capability
      lifecycle pattern (resumable uploads, checkpointed action
      ledger) that M11 depends on.
    - Added explicit subsystem coverage matrix (section 3) mapping
      every user-specified concern (outbox, NDK-style aggregation,
      reactivity, non-Nostr bridge, NIP-42, Blossom, multi-session,
      NIP-77, podcast-class apps) to its delivering milestone(s).
    - Added NIP support roadmap with per-NIP module + milestone +
      status.
    - Identified parallelization opportunities in section 4.
    - Preserved test pyramid, CI gates, ADR log, harness-first
      pattern, and modeled-vs-runtime discipline from the prior plan.
    
    The plan stays honest: M0-M1 are largely done; M1 needs final
    hardening (firehose-bench live with measured numbers as the M1
    baseline). Everything from M2 onward is forward work.
    
    https://claude.ai/code/session_016fyLihxeomwWk3JP15SV2H

docs/plan.md

exec
/bin/zsh -lc 'git status --short && git rev-parse --short HEAD && git branch --show-current' in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
 succeeded in 0ms:
?? docs/perf/codex-reviews/55dd5f2.md
?? docs/perf/codex-reviews/d660735.md
?? docs/perf/codex-reviews/e9cbafa.md
b49f8f3
worktree-agent-a8c0acab50f455ada

codex
The worktree is ahead of that merge now, and `docs/plan.md` has already been split down to 63 lines. I’m going to review the commit object for `e9cbafa` itself so the findings match the merge you named, not the current follow-up state.
exec
/bin/zsh -lc 'git log --oneline --decorate -8' in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
 succeeded in 0ms:
b49f8f3 (HEAD -> worktree-agent-a8c0acab50f455ada, worktree-agent-a5a7f37aa1241fd59, worktree-agent-a352d07d96f605fad) docs(plan): fold scope-adjustments into per-milestone files (codex follow-up)
ab632c1 docs(plan): split plan.md into per-milestone files (≤300 LOC each)
3f5a2b3 docs(readme): align D0-D5 with canonical product-spec; log doctrine collision (PD-001)
446a212 docs(m11): fix risks.md §A.2 rig.rs scope conflict
51120cb perf(orchestration): HB4 (autonomous) — framework-magic landed + research + README
2477372 docs(m11): align screenshot --baseline-only flag across docs
810d0f8 docs(readme): top-level README with TL;DR, decisions, architecture map
8d633e8 research(applesauce+ndk): deep-dive notes for M2 outbox + framework-magic

exec
/bin/zsh -lc 'git show --format=fuller --no-patch e9cbafa' in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
exec
/bin/zsh -lc 'git show e9cbafa:docs/plan.md | wc -l' in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
 succeeded in 0ms:
     680

exec
/bin/zsh -lc 'git show e9cbafa:docs/plan.md | rg -n "TODO|FIXME|unimplemented|TBD|defer|deferred|later|post-v1|not implemented|not v1|maybe|whatever"' in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
 succeeded in 0ms:
commit e9cbafa93ebf0a7845879131584696265f37ee96
Author:     Claude <noreply@anthropic.com>
AuthorDate: Sun May 17 22:12:14 2026 +0000
Commit:     Claude <noreply@anthropic.com>
CommitDate: Sun May 17 22:12:14 2026 +0000

    docs(plan): consolidate into single milestone-driven plan with product checks
    
    Rewrites docs/plan.md to reflect what's actually been built and to give
    a single ladder of seventeen milestones (M0-M17), each producing a
    runnable demoable product and a measured (not just modeled) report.
    
    What changed relative to the prior phased plan:
    
    - Section 0 honestly accounts for current state: kernel substrate
      (~3,800 LOC), live iOS app (~1,375 LOC Swift) pulling from
      primal, fixture-todo-core kernel-boundary proof, codegen tool
      with determinism, and the gaps (no LMDB, no outbox, no NIP-77,
      no NIP-42, no write path, no Blossom, no multi-account, no DM,
      no wallet/WoT, no Android/Desktop/Web, raw C FFI not UniFFI).
    - Replaced abstract phases with milestones tied to demoable
      products. Every milestone has an exit gate with measurable
      numbers and a runnable artifact.
    - Elevated subscription compilation (NDK/Applesauce lesson) to
      its own milestone M2, alongside NIP-65 outbox routing.
    - Added M5 NIP-42 auth as an explicit milestone (was not in the
      prior plan).
    - Added M11 podcast app as the load-bearing kernel-boundary proof
      in a non-social domain. Adds AudioPlaybackCapability,
      BackgroundWorkCapability, and proves no podcast nouns leak into
      nmp-core.
    - Expanded M10 Blossom to establish the long-running capability
      lifecycle pattern (resumable uploads, checkpointed action
      ledger) that M11 depends on.
    - Added explicit subsystem coverage matrix (section 3) mapping
      every user-specified concern (outbox, NDK-style aggregation,
      reactivity, non-Nostr bridge, NIP-42, Blossom, multi-session,
      NIP-77, podcast-class apps) to its delivering milestone(s).
    - Added NIP support roadmap with per-NIP module + milestone +
      status.
    - Identified parallelization opportunities in section 4.
    - Preserved test pyramid, CI gates, ADR log, harness-first
      pattern, and modeled-vs-runtime discipline from the prior plan.
    
    The plan stays honest: M0-M1 are largely done; M1 needs final
    hardening (firehose-bench live with measured numbers as the M1
    baseline). Everything from M2 onward is forward work.
    
    https://claude.ai/code/session_016fyLihxeomwWk3JP15SV2H

 succeeded in 0ms:
19:- **Fixture proving the kernel boundary** in `crates/fixture-todo-core` (~304 LOC) plus generated `apps/fixture/nmp-app-fixture`: a non-Nostr TODO module implementing all five substrate trait families, with codegen producing the per-app crate. Proves the kernel works for arbitrary domains, not just Nostr.
25:### Designed but not implemented
60:3. **Capability layering is strict.** Each milestone adds exactly one new architectural ingredient on top of the previous demo. No "we'll wire it up later" — wiring is the milestone.
73:**Demo product:** `apps/fixture/nmp-app-fixture` — a TODO list app driven by the kernel substrate with no Nostr concepts in it.
227:- Login UX (single nsec field for now; multi-step onboarding deferred to M16).
362:- `nmp-podcast` (or whatever the Nostr podcast NIP is called, e.g. NIP-XX for podcast feed events): parsed feed events. If no NIP, the app uses RSS via the action ledger to fetch + parse, storing entries as domain records.
459:**Demo product:** Same Twitter slice and (where capabilities allow) podcast slice running on Android (Compose), Desktop (iced), and Web (wasm + React/Solid TBD). Cross-platform consistency test passes — same scripted scenario produces byte-identical `AppState` JSON on all four platforms.
482:- Web shell stack TBD (React + signals / Solid / Svelte — pick at start of milestone).
561:| 04 | not v1 | — | superseded by NIP-44/17; not implemented |
568:| 23 | not v1 | — | long-form reader is post-v1 |
585:NIPs not in v1 (e.g., NIP-29 groups, NIP-23 long-form, NIP-71 video) become post-v1 extension modules; the kernel boundary makes them additive.
677:- **Not exhaustive about post-v1 work.** NIP-29 groups, NIP-23 long-form, NIP-71 video, additional protocol modules, additional app demonstrations (Highlighter-lite, TENEX-lite, etc.) are post-v1 — they validate the kernel boundary further but are not v1 deliverables.

exec
/bin/zsh -lc "git show e9cbafa:docs/plan.md | nl -ba | sed -n '1,90p'" in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
exec
/bin/zsh -lc "git show e9cbafa:docs/plan.md | nl -ba | sed -n '210,245p'" in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
 succeeded in 0ms:
     1	# Build & Validation Plan
     2	
     3	> Companion to `docs/product-spec.md` (what we ship) and the design docs in `docs/design/` (how each subsystem works). This document defines **the single ladder of milestones**, each one a runnable product that proves a specific architectural claim with real (not modeled) evidence.
     4	
     5	> **Three arcs:** Kernel substrate + Nostr social stack (M0–M10) → kernel-boundary proof with a non-social-domain app (M11) → wallet/WoT + cross-platform + release (M12–M17).
     6	
     7	> **Each milestone is gated.** Every milestone ends with: a runnable artifact, automated tests in `nmp-testing`, a measured-numbers report in `docs/perf/m<N>/`, and an explicit ADR if a design decision was revised in flight. No silent endings.
     8	
     9	---
    10	
    11	## 0. Where we are right now
    12	
    13	Honest accounting before forecasting forward.
    14	
    15	### Implemented and running
    16	
    17	- **Kernel substrate** in `crates/nmp-core` (~3,800 LOC): actor on a dedicated OS thread, mailbox-driven (ADR feedback adopted — relay reads happen in tokio reader tasks, the actor blocks on its own channel with deadline timeouts), substrate trait families (`DomainModule`, `ViewModule`, `ActionModule`, `CapabilityModule`, `IdentityModule`) in `nmp-core/src/substrate/`, ingest pipeline (`kernel/ingest.rs`), claim/release refcounting for profile interest (commit `23ae829`), composite reverse-index dependency tracking.
    18	- **Live Nostr-connected iOS app** in `ios/NmpStress` (~1,375 LOC Swift): SwiftUI shell wired to the Rust kernel via raw C FFI. Connects to `wss://relay.primal.net` (content) + `wss://purplepag.es` (indexer). Renders seed-driven timeline from union of pablof7z + fiatjaf + jb55 follow lists. Profile resolution with placeholders → in-place refinement on kind:0 arrival per doctrine D1. Thread view. Diagnostics screen showing relay status, logical interests, wire subscriptions (ADR-0007).
    19	- **Fixture proving the kernel boundary** in `crates/fixture-todo-core` (~304 LOC) plus generated `apps/fixture/nmp-app-fixture`: a non-Nostr TODO module implementing all five substrate trait families, with codegen producing the per-app crate. Proves the kernel works for arbitrary domains, not just Nostr.
    20	- **Codegen tool** in `crates/nmp-codegen` (~423 LOC): reads `nmp.toml`, produces a per-app crate, has determinism tests.
    21	- **Benches** in `crates/nmp-testing`: `reactivity-bench` (composite-key reverse index + coalescer + working set; run 002 passed all ADR-0001..0004 gates) and `firehose-bench` (replay + capture + live modes; replay scenarios pass the modeled budget contract).
    22	- **Perf reports** in `docs/perf/` documenting reactivity-bench run 002, firehose-bench replay runs, and three iOS measurement reports (relay lifecycle, profile/thread subscriptions, the primal slice baseline).
    23	- **Architecture decisions** locked in 10 ADRs (`docs/decisions/0001`–`0010`).
    24	
    25	### Designed but not implemented
    26	
    27	- LMDB / IndexedDB persistent storage (in-memory only today).
    28	- NIP-65 outbox routing (hardcoded content + indexer relays today).
    29	- NIP-77 negentropy sync.
    30	- NIP-42 relay auth.
    31	- Multi-account / multi-session model and account switching.
    32	- Signer trait + local-key signer + NIP-46 bunker signer.
    33	- Action ledger + write path (compose / react / repost / quote).
    34	- NIP-17 messaging and the NSE companion crate.
    35	- Blossom uploads / downloads with resumable progress.
    36	- Wallet stack (NWC, NIP-57 zaps, Cashu, nutzaps).
    37	- Web-of-Trust subsystem.
    38	- UniFFI bindings (current iOS bridge is raw C FFI).
    39	- Android shell, Desktop shell, Web shell.
    40	- The `nmp` CLI scaffolding tool.
    41	- A non-Nostr-shaped product (podcast app) demonstrating the kernel boundary in production.
    42	
    43	### Gaps in the prior plan that this rewrite addresses
    44	
    45	- The prior plan was phase-numbered (Phase 1, 2, …) without explicit *demoable products* per phase.
    46	- NIP-42 wasn't covered.
    47	- Subscription compilation (the load-bearing NDK/Applesauce lesson) wasn't elevated as its own milestone.
    48	- Blossom and media-capability lifecycle (long-running, resumable, background) were one bullet under Phase 6.
    49	- No milestone proved the kernel boundary for a fundamentally non-social product.
    50	- The plan didn't reflect that M0 and M1 are largely done.
    51	
    52	The plan below is a single ladder of seventeen milestones (M0–M17), each producing a runnable artifact, ordered so that each milestone strictly adds capabilities to the prior demoable product.
    53	
    54	---
    55	
    56	## 1. Principles of execution
    57	
    58	1. **Each milestone is a runnable product.** Not a feature branch; a thing you can build, launch on real hardware, and demo. Unit tests verify correctness; the milestone product validates the architecture.
    59	2. **Real measured evidence over modeled budgets.** Modeled passes in `firehose-bench` replay establish the budget contract. Real passes in `firehose-bench live` against the iOS / Android / Desktop / Web app are the actual gate.
    60	3. **Capability layering is strict.** Each milestone adds exactly one new architectural ingredient on top of the previous demo. No "we'll wire it up later" — wiring is the milestone.
    61	4. **The doctrine rubric is final.** Every PR is reviewed against the cardinal doctrines (`product-spec.md` §1.5, D0–D5). A change that makes any doctrine harder to enforce is rewritten or rejected.
    62	5. **The kernel never grows app nouns.** ADR-0009 doctrine D0 is enforced by review and by the M11 podcast-app proof.
    63	6. **No phase ends silently.** Each milestone exit produces: regression tests added to `nmp-testing`, a perf report in `docs/perf/m<N>/`, an ADR if a design decision was revised, and a runnable artifact tagged in git.
    64	
    65	---
    66	
    67	## 2. The milestone ladder
    68	
    69	Each milestone has: **demo product**, **scope (what gets built)**, **subsystem deliverables**, **exit gate (measurable)**, and **runnable artifact**. Estimates are for one experienced developer focused on the work; they are not commitments.
    70	
    71	### M0 — Kernel substrate + non-Nostr fixture *(DONE)*
    72	
    73	**Demo product:** `apps/fixture/nmp-app-fixture` — a TODO list app driven by the kernel substrate with no Nostr concepts in it.
    74	
    75	**Scope.** Five extension trait families. Composite reverse index. Delta buffer with coalescing. Claim-based GC. Codegen producing a working per-app crate from a fixture module.
    76	
    77	**Subsystem deliverables.** `nmp-core::substrate`, `nmp-codegen`, `fixture-todo-core`, `nmp-testing` harness skeletons.
    78	
    79	**Exit gate.** ✅ Fixture compiles and runs; codegen determinism test passes; substrate registry test passes (`crates/nmp-core/tests/substrate_registry.rs`).
    80	
    81	**Runnable artifact.** `cargo test --workspace`; the fixture module loads in any host.
    82	
    83	---
    84	
    85	### M1 — Read-only Twitter slice on iOS *(LARGELY DONE; final hardening in flight)*
    86	
    87	**Demo product:** `ios/NmpStress` — SwiftUI app pulling live from primal, rendering seed-driven timeline, profile cards, threads, diagnostics screen.
    88	
    89	**Scope.** Per ADR-0006 + ADR-0008 + ADR-0009: kind:0 Profile path end-to-end against a real relay, on iOS, through real FFI. Seed-driven discovery (union of follow lists from pablof7z + fiatjaf + jb55). Refcounted claim/release pattern per ADR-0005 (profile interest commit `23ae829`). Diagnostics surface per ADR-0007.
    90	

 succeeded in 0ms:
   210	
   211	---
   212	
   213	### M6 — Sessions + signers + write path
   214	
   215	**Demo product:** iOS app gets a login screen. After login the user can compose and publish a kind:1 note to primal that atomically appears in their own timeline.
   216	
   217	**Scope.** Per `product-spec.md` §7.4, §7.5, §7.15:
   218	
   219	**Subsystem deliverables.**
   220	
   221	- `IdentityModule::HumanAccount` with local-key signer (raw nsec, NIP-49 encrypted).
   222	- `IdentityModule::ExternalSigner` with NIP-46 (Nostr Connect / bunker) signer.
   223	- `KeychainCapability` real implementation: encrypted nsec storage via iOS Keychain, app-private access group.
   224	- Action ledger in `nmp-core::kernel::ledger`: durable rows with ULID action IDs, status transitions, retry/cancel, restart recovery.
   225	- Action atomicity contract: a `SendNote` action's publish to relays and local store insert happen as one actor message; partial failure rolls back.
   226	- `nmp-nip01::SendNoteActionModule` as the first write-path action.
   227	- Login UX (single nsec field for now; multi-step onboarding deferred to M16).
   228	
   229	**Exit gate.**
   230	
   231	- Bug-extinction #7 (action partial-success): inject "publish OK / store fail" and "store OK / publish fail" — both roll back atomically.
   232	- Bug-extinction #9 (NIP-46 lost on suspend): simulate suspend mid-publish; resume retries or surfaces failure as toast.
   233	- Bug-extinction #10 (re-publish keeps event id): re-publish of an event preserves `id` and `sig`.
   234	- Compose flow on iOS: login → compose → publish → note visible on primal externally and in local timeline within one ViewBatch.
   235	
   236	**Runnable artifact.** iOS Twitter slice with working compose. Report in `docs/perf/m6/write-path.md`.
   237	
   238	---
   239	
   240	### M7 — Reactions + Thread + Reply (the interaction loop)
   241	
   242	**Demo product:** Twitter slice user can like a post, reply to it, see the thread, and have the reply land in primal.
   243	
   244	**Scope.** `nmp-nip25` (Reactions view module + React action), `nmp-nip10` (Thread view module with NIP-10 reply-marker handling), `SendNote` extended for `reply_to`.
   245	

exec
/bin/zsh -lc "git show e9cbafa:docs/plan.md | nl -ba | sed -n '450,505p'" in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
exec
/bin/zsh -lc "git show e9cbafa:docs/plan.md | nl -ba | sed -n '330,405p'" in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
 succeeded in 0ms:
   450	- Cross-platform consistency test (next milestone) is unblocked because the FFI shape is now identical across platforms.
   451	- Codegen determinism: repeated runs produce byte-identical output.
   452	
   453	**Runnable artifact.** iOS Twitter + iOS Podcast apps both using UniFFI. Report in `docs/perf/m14/uniffi-migration.md`.
   454	
   455	---
   456	
   457	### M15 — Cross-platform: Android + Desktop + Web
   458	
   459	**Demo product:** Same Twitter slice and (where capabilities allow) podcast slice running on Android (Compose), Desktop (iced), and Web (wasm + React/Solid TBD). Cross-platform consistency test passes — same scripted scenario produces byte-identical `AppState` JSON on all four platforms.
   460	
   461	**Scope.**
   462	
   463	**Android port (~3 weeks):**
   464	
   465	- Kotlin bindings via UniFFI; cargo-ndk + Gradle pipeline.
   466	- Compose shell mirroring the iOS SwiftUI shell.
   467	- `KeychainCapability` Android impl via `EncryptedSharedPreferences`.
   468	- `nmp-nip55` Amber external-signer capability module.
   469	- Android `FirebaseMessagingService` integration with `nmp-nip17-nse` for DM push.
   470	
   471	**Desktop port (~2 weeks):**
   472	
   473	- iced shell (the development-time reference target lives on; this milestone graduates it to a shipping target).
   474	- macOS + Linux + Windows.
   475	- `KeychainCapability` impls per OS (macOS Keychain, Secret Service, Windows Credential Manager — already exists in `nostr-keyring`).
   476	
   477	**Web port (~3 weeks):**
   478	
   479	- `nmp-wasm` mature.
   480	- IndexedDB storage backend; OPFS where supported.
   481	- `nmp-nip07` browser-signer capability module.
   482	- Web shell stack TBD (React + signals / Solid / Svelte — pick at start of milestone).
   483	
   484	**Subsystem deliverables.**
   485	
   486	- Cross-platform consistency test in `nmp-testing` — drives same scripted action sequence on all four targets, snapshots `AppState` JSON at checkpoints, asserts byte-equal.
   487	- Per-platform performance reports.
   488	
   489	**Exit gate.**
   490	
   491	- Twitter clone identical scripted scenario produces byte-identical `AppState` snapshots on iOS / Android / Desktop / Web.
   492	- All §7.16 performance budgets met on reference devices (iPhone 12, Pixel 6a, M1 mini, modern browsers).
   493	- Web works in incognito mode by falling back to in-memory store with a visible warning.
   494	
   495	**Runnable artifact.** Four-platform demo. Report in `docs/perf/m15/cross-platform.md`.
   496	
   497	---
   498	
   499	### M16 — CLI + starter app + recipe book
   500	
   501	**Demo product:** A developer with no prior framework knowledge runs `nmp init my-app`, follows recipes, ships a working hashtag-feed app on all four platforms in ≤ 2 hours.
   502	
   503	**Scope.**
   504	
   505	**Subsystem deliverables.**

 succeeded in 0ms:
   330	- Slow-network upload remains responsive — main UI is never blocked.
   331	- Profile picture update through compose → kind:0 republish with new Blossom URL → in-place refinement across all open Profile / Timeline payloads (per doctrine D1).
   332	
   333	**Runnable artifact.** iOS Twitter slice with media compose. Report in `docs/perf/m10/blossom.md`.
   334	
   335	---
   336	
   337	### M11 — Podcast app (the kernel-boundary proof in a non-social domain)
   338	
   339	**Demo product:** A podcast app built entirely as an extension-module set, sharing nothing app-specific with `nmp-core`. Subscribes to podcast feeds. Downloads episodes. Plays them with background audio. Resumes playback position across app launches. Pulls feed updates via Nostr where available, RSS where not.
   340	
   341	**This is the load-bearing kernel-boundary check.** If the kernel needs even one podcast noun to make this work, the boundary is wrong and we go back to fix it.
   342	
   343	**Scope.**
   344	
   345	**Subsystem deliverables (extension modules — not in `nmp-core`):**
   346	
   347	- `podcast-core` app crate:
   348	  - `DomainModule`s: `Podcast`, `Episode`, `Transcript`, `PlayerState`, `Subscription`.
   349	  - `ViewModule`s: `PodcastLibrary`, `EpisodeDetail`, `NowPlaying`, `EpisodeQueue`.
   350	  - `ActionModule`s: `SubscribePodcast`, `RefreshFeed`, `DownloadEpisode`, `Play`, `Pause`, `Seek`, `MarkPlayed`, `ImportRss`.
   351	  - `IdentityModule::AppLocal` if anonymous subscription syncing across devices is wanted.
   352	
   353	**Subsystem deliverables (capabilities added to the kernel's reusable set):**
   354	
   355	- `AudioPlaybackCapability`: kernel asks the platform to play a URL or local file; platform reports position events + state transitions back. iOS implementation via `AVPlayer` + background-audio entitlement.
   356	- `BackgroundWorkCapability`: kernel registers periodic background tasks (feed refresh, scheduled downloads); platform implements via BGTask scheduler (iOS) / WorkManager (Android).
   357	- `LocalNotificationCapability`: extended for episode-available alerts.
   358	- `HttpCapability`: extended for podcast feed fetch (long-running streaming response).
   359	
   360	**Subsystem deliverables (protocol modules):**
   361	
   362	- `nmp-podcast` (or whatever the Nostr podcast NIP is called, e.g. NIP-XX for podcast feed events): parsed feed events. If no NIP, the app uses RSS via the action ledger to fetch + parse, storing entries as domain records.
   363	
   364	**Exit gate (kernel boundary).**
   365	
   366	- **`nmp-core` gains zero podcast nouns.** No `Podcast`, `Episode`, `Transcript`, `Player`, `Feed` types added to the kernel. Verified by grep + manual review at the commit.
   367	- **The capability families added in M11 are general** (audio playback, background work, local notifications, HTTP). Their request/response shapes are not podcast-specific.
   368	- **Reactivity behavior is identical** to the social demo — composite-key dependencies, delta coalescing, claim-based GC, ADR-0007 diagnostics all work for podcast view modules.
   369	
   370	**Exit gate (product).**
   371	
   372	- Subscribe to 5 real podcasts (use any well-known Nostr-podcast feeds if available, plus RSS imports).
   373	- Download an episode in the background while the app is suspended.
   374	- Play it with background audio while the iPhone is locked.
   375	- Resume playback at the correct position after a kill-relaunch.
   376	- Push notification on a new episode arrival.
   377	
   378	**Runnable artifact.** A second iOS app (`ios/NmpPodcast`) — distinct binary, same Rust kernel, different module set. Report in `docs/perf/m11/podcast-app.md` documenting the kernel-boundary verification.
   379	
   380	---
   381	
   382	### M12 — Wallet (NWC + zaps + Cashu + nutzaps)
   383	
   384	**Demo product:** Twitter slice gets a zap button on each post. Tapping it pays via NWC. Receiving zaps shows up in a zap-history view. Cashu nutzap claim works.
   385	
   386	**Scope.** Per spec §7.9:
   387	
   388	**Subsystem deliverables.**
   389	
   390	- `nmp-nwc` protocol module: NIP-47 client; pay/receive/balance.
   391	- `nmp-nip57` protocol module: LUD-16 discovery + zap request building + receipt verification.
   392	- `nmp-nip60` protocol module: Cashu wallet event types + proof state in domain store.
   393	- `nmp-nip61` protocol module: Nutzap action module; pending-nutzap claim flow.
   394	- `WalletBalance` view module; `ZapHistory` view module.
   395	- Zap action module: `Zap { target, sats, comment }` on the action ledger.
   396	
   397	**Exit gate.**
   398	
   399	- Pay a 100-sat zap via NWC to a real LUD-16 endpoint; receipt verifies; balance updates within one ViewBatch.
   400	- Receive a zap (test via a separate device or simulated): zap-history view reflects within one ViewBatch.
   401	- Nutzap claim from a Cashu mint: proofs land in the wallet; balance updates.
   402	- Wallet operations never block the UI thread.
   403	
   404	**Runnable artifact.** iOS Twitter slice with working zaps. Report in `docs/perf/m12/wallet.md`.
   405	

exec
/bin/zsh -lc "git show e9cbafa:docs/plan.md | nl -ba | sed -n '540,590p'" in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
exec
/bin/zsh -lc "git show e9cbafa:docs/plan.md | nl -ba | sed -n '270,325p'" in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
exec
/bin/zsh -lc "git show e9cbafa:docs/plan.md | nl -ba | sed -n '620,680p'" in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
 succeeded in 0ms:
   270	
   271	- Session model in the kernel: `SessionState { accounts, active, status }` with N accounts simultaneously valid.
   272	- View specs that depend on the active account (Timeline of "your follows", DMs inbox, zap history) get account-scoped composite keys.
   273	- Account switch is an action with full rebuild semantics — open views for the new active account, close the prior ones, projection caches stay populated across switches when overlap exists.
   274	- Per-account signer binding (each account has its own `IdentityId`).
   275	- Per-account secure storage namespacing in `KeychainCapability`.
   276	
   277	**Exit gate.**
   278	
   279	- Bug-extinction #5 (account-context overlap): two accounts active, switch between them, assert no state bleed. `AppState` snapshot for account A never contains data scoped to account B's session-aware views.
   280	- Switching accounts during an in-flight publish: the publish is account-tagged, completes correctly, lands in the originating account's timeline only.
   281	- Per-account signer never signs an event for the wrong account (test forces dispatch through a wrong-account signer; the action ledger rejects).
   282	
   283	**Runnable artifact.** Account switcher in iOS demo with two real test accounts. Report in `docs/perf/m8/multi-account.md`.
   284	
   285	---
   286	
   287	### M9 — NIP-17 DMs + NSE
   288	
   289	**Demo product:** Twitter slice gets a DMs tab. End-to-end NIP-17 gift-wrapped messages between two test accounts. Background push triggers iOS Notification Service Extension decryption; opening the app shows the message already in place.
   290	
   291	**Scope.** Per spec §7.10 and §7.14:
   292	
   293	**Subsystem deliverables.**
   294	
   295	- `nmp-nip17` protocol module: Conversation view module + ConversationList view module; SendDm action module; NIP-44 encryption / NIP-59 gift-wrapping; outbox routing for DMs (recipient inbox relays only — never public).
   296	- `nmp-nip17-nse` companion crate: `decrypt_push()` API with bounded memory (≤ 24 MB peak, ≤ 200 ms p99), reading from shared keychain and shared App Group storage.
   297	- iOS NSE target wiring: silent push from APNs → NSE invokes `decrypt_push` → notification posted with decrypted preview.
   298	- Action atomicity for `SendDm`: gift-wrap → publish to all recipient inboxes → insert locally — atomic.
   299	
   300	**Exit gate.**
   301	
   302	- Bug-extinction #4 (DM to public): no API path can send a DM to a non-inbox relay; planner refuses non-inbox relays for `p`-tagged-only events.
   303	- DM round-trip in `MockRelay` (alice ↔ bob): content matches; no plaintext crosses FFI other than as `ConversationMessage.body`.
   304	- NSE decrypt of an incoming gift-wrap: p99 ≤ 200 ms, peak memory ≤ 24 MB.
   305	- Backgrounded app receives a push, NSE decrypts and posts notification, app foregrounded shows the message in place (no re-fetch from relay).
   306	
   307	**Runnable artifact.** iOS Twitter slice with working DMs + push notifications. Report in `docs/perf/m9/messaging.md`.
   308	
   309	---
   310	
   311	### M10 — Blossom + media + long-running capabilities
   312	
   313	**Demo product:** Twitter slice user can attach a photo to a compose, see upload progress, and the published note has a valid Blossom URL. Profile-picture upload also works.
   314	
   315	**Scope.** Per spec §7.11. Establishes the **long-running capability lifecycle pattern** that the podcast app (M11) builds on:
   316	
   317	**Subsystem deliverables.**
   318	
   319	- `nmp-blossom` protocol module: upload action module + download action module + media view module + upload-status view (progress).
   320	- `FilePickerCapability` real implementation on iOS (PHPicker for photos / `UIDocumentPicker` for files).
   321	- `BlossomCapability` callback interface: kernel asks platform to perform an HTTP PUT with progress; platform reports progress + completion back via reverse callback into the actor.
   322	- Long-running action lifecycle: upload registers in the action ledger as `AwaitingCapability`; capability progress updates the ledger row; restart recovery resumes from the last checkpointed progress.
   323	- Resumable uploads (Blossom range support where the server allows).
   324	- BUD-01 / BUD-02 protocol support.
   325	

 succeeded in 0ms:
   540	
   541	Cross-reference of which milestone delivers which user-specified concern.
   542	
   543	| Concern | Milestone(s) | Notes |
   544	|---|---|---|
   545	| **Outbox routing (NIP-65)** | M2 | First-class as a planner stage, not a side feature. Diagnostics show per-relay coverage. |
   546	| **NDK-style subscription aggregation** | M2 | Per `docs/design/ndk-applesauce-lessons.md` §7, the planner becomes a subscription compiler. Logical interests → per-relay plans → wire REQs, semantics-preserving merge/split. |
   547	| **Reactivity as planned** | M0–M7 | Already validated by reactivity-bench run 002 against the model; M1 runs the same code path against real iOS; subsequent milestones add view modules that exercise the contract under varied loads. |
   548	| **Non-Nostr data bridge** | M0 (substrate), M10 (long-running capabilities), M11 (podcast app proves it in production) | DomainModule trait + ADR-0007 bridge lanes; first proven by fixture-todo-core; production proof in podcast app. |
   549	| **NIP-42 auth** | M5 | Per-relay auth state machine; integrates with diagnostics; works with both local-key and NIP-46 signers. |
   550	| **Blossom** | M10 | Upload + download with resumable progress; long-running capability lifecycle. |
   551	| **Multi-session clients** | M8 | Per-account view-spec scoping; account switcher; isolation tests. |
   552	| **NIP-77 negentropy** | M4 | Sync engine with watermarks; planner consults before REQ; capability negotiation; bytes-saved diagnostic. |
   553	| **Podcast-class apps** | M11 (proof), M10 (capabilities prerequisite) | AudioPlaybackCapability, BackgroundWorkCapability, BlossomDownloadCapability all generic; podcast-specific domain in `podcast-core` app crate. |
   554	
   555	### NIP support roadmap at v1
   556	
   557	| NIP | Module | Milestone | Status |
   558	|---|---|---|---|
   559	| 01 | nmp-nip01 | M1, M6 | partial (reads in M1; writes in M6) |
   560	| 02 | nmp-nip02 | M2 | follow-list parsing (contacts view) |
   561	| 04 | not v1 | — | superseded by NIP-44/17; not implemented |
   562	| 05 | nmp-nip01 | M1 | NIP-05 verification in Profile module |
   563	| 07 | nmp-nip07 | M15 | web-only browser signer |
   564	| 09 | nmp-nip01 | M3 | kind:5 deletes (full handling) |
   565	| 10 | nmp-nip10 | M7 | reply markers in thread building |
   566	| 17 | nmp-nip17 | M9 | DMs |
   567	| 19 | nmp-nip19 | M1 | bech32 utility used throughout |
   568	| 23 | not v1 | — | long-form reader is post-v1 |
   569	| 25 | nmp-nip25 | M7 | reactions |
   570	| 40 | nmp-nip01 | M3 | expiration scheduling |
   571	| 42 | nmp-nip42 | M5 | relay auth |
   572	| 44 | nmp-nip17 | M9 | encryption (via NIP-17) |
   573	| 46 | nmp-nip46 | M6 | bunker signer |
   574	| 47 | nmp-nwc | M12 | wallet connect |
   575	| 49 | nmp-nip01 / nmp-nip46 | M6 | encrypted-key import |
   576	| 55 | nmp-nip55 | M15 | Android Amber bridge |
   577	| 57 | nmp-nip57 | M12 | zaps |
   578	| 59 | nmp-nip17 | M9 | gift wrap (via NIP-17) |
   579	| 60 | nmp-nip60 | M12 | Cashu |
   580	| 61 | nmp-nip61 | M12 | nutzaps |
   581	| 65 | nmp-nip65 | M2 | mailboxes + outbox |
   582	| 77 | nmp-nip77 | M4 | negentropy |
   583	| Blossom BUD-01/02 | nmp-blossom | M10 | media |
   584	
   585	NIPs not in v1 (e.g., NIP-29 groups, NIP-23 long-form, NIP-71 video) become post-v1 extension modules; the kernel boundary makes them additive.
   586	
   587	---
   588	
   589	## 4. Parallelization opportunities
   590	

 succeeded in 0ms:
   620	
   621	## 6. CI / pre-merge hygiene
   622	
   623	Required CI gates (apply from the milestone they become possible):
   624	
   625	- `cargo fmt --all -- --check` (always).
   626	- `cargo test --workspace` (always).
   627	- `cargo run -p nmp-codegen -- gen modules --check` (codegen determinism, from M0).
   628	- `cargo run -p nmp-testing --bin reactivity-bench --release -- --standard --fail-on-gate` (from M0).
   629	- `cargo run -p nmp-testing --bin firehose-bench --release -- replay --standard --fail-on-gate` (from M0).
   630	- iOS build (`just build-ios`) from M1.
   631	- iOS UI test (`xcrun simctl test`) from M1.
   632	- Android build from M15.
   633	- Desktop build from M15.
   634	- Web build from M15.
   635	- Cross-platform consistency test from M15.
   636	
   637	Live firehose runs are not in pre-merge CI (would block on relay flakes); they run nightly on a dedicated runner and produce reports tagged `live` in `docs/perf/m<N>/`.
   638	
   639	---
   640	
   641	## 7. Decision log
   642	
   643	ADRs live in `docs/decisions/`. Format per the template in older revisions of this plan. Currently:
   644	
   645	- **ADR-0001**: Composite dependency keys (composite-first reverse index; broad axes guardrailed).
   646	- **ADR-0002**: Per-view delta budget (60/view/sec, not absolute).
   647	- **ADR-0003**: Working-set memory budget (hot/cold split, not total events).
   648	- **ADR-0004**: Allocation measurement via counting allocator.
   649	- **ADR-0005**: Domain-keyed platform shadow + refcounted component wrappers.
   650	- **ADR-0006**: Vertical-slice-first delivery (modified by ADR-0009; the slice now layers on the kernel substrate).
   651	- **ADR-0007**: Diagnostics and non-Nostr data over the actor-owned bridge with explicit records, not raw callbacks or fake Nostr events.
   652	- **ADR-0008**: Twitter-clone iOS as the Phase 1a demo target (modified by ADR-0009 — repositioned as first canonical extension-module set).
   653	- **ADR-0009**: App-extension kernel boundary. Five trait families, four layers, no app nouns in nmp-core.
   654	- **ADR-0010**: Per-app concrete enums generated at the FFI boundary. Codegen is critical-path v1 infrastructure.
   655	
   656	New ADRs land alongside any milestone whose execution revises a design.
   657	
   658	### The harness-first pattern
   659	
   660	Every design doc has measurable gates. Gates run on the reactivity-bench harness (or `firehose-bench` for end-to-end behavior). Failures revise the design **before** implementation. Pre-implementation measurement is cheaper than post-implementation rework. Run 001 of reactivity-bench established the pattern: the reverse-index direction was validated (100×–1000× headroom), one design refinement landed (composite keys), and two budget bugs surfaced (per-view delta, working-set memory) — all before any view-kind code shipped.
   661	
   662	### Modeled budget contract vs runtime evidence
   663	
   664	Two distinct claims about the same harness:
   665	
   666	- **Modeled budget contract.** Replay mode runs deterministic synthetic workloads through a model of the runtime. Passing here proves budgets are internally consistent and the harness scaffolding is sound. Does **not** prove the real runtime hits those budgets.
   667	- **Runtime evidence.** Live mode (or replay mode with real adapters substituted for modeled segments) runs against actual LMDB, actual WebSockets, actual UniFFI marshaling. Passing here is real evidence.
   668	
   669	Each milestone moves the boundary rightward — replaces another modeled segment with a real adapter and graduates the corresponding firehose-bench scenarios from `modeled` to `measured` in `docs/perf/`.
   670	
   671	---
   672	
   673	## 8. What this plan is not
   674	
   675	- **Not a schedule.** No dates, no person-months. Milestones are sequential; their durations depend on team size and surface complexity. Estimates per milestone are guidance only.
   676	- **Not a marketing roadmap.** v1 ships when M17 gates are met, not on a calendar.
   677	- **Not exhaustive about post-v1 work.** NIP-29 groups, NIP-23 long-form, NIP-71 video, additional protocol modules, additional app demonstrations (Highlighter-lite, TENEX-lite, etc.) are post-v1 — they validate the kernel boundary further but are not v1 deliverables.
   678	- **Not silent about gaps.** §0 names exactly what is and isn't built. As the ladder progresses, §0 gets revised so the plan stays honest about state.
   679	
   680	The plan exists so that any single milestone can be picked up cold by someone reading this doc + `product-spec.md` + the relevant ADRs and design docs, and they can execute without bothering the rest of the team.

exec
/bin/zsh -lc "git show e9cbafa:docs/plan.md | rg -n \"M11\\.5|Highlighter|M9|M12|Wallet|NIP-17|DMs\"" in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8c0acab50f455ada
 succeeded in 0ms:
5:> **Three arcs:** Kernel substrate + Nostr social stack (M0–M10) → kernel-boundary proof with a non-social-domain app (M11) → wallet/WoT + cross-platform + release (M12–M17).
34:- NIP-17 messaging and the NSE companion crate.
36:- Wallet stack (NWC, NIP-57 zaps, Cashu, nutzaps).
272:- View specs that depend on the active account (Timeline of "your follows", DMs inbox, zap history) get account-scoped composite keys.
287:### M9 — NIP-17 DMs + NSE
289:**Demo product:** Twitter slice gets a DMs tab. End-to-end NIP-17 gift-wrapped messages between two test accounts. Background push triggers iOS Notification Service Extension decryption; opening the app shows the message already in place.
295:- `nmp-nip17` protocol module: Conversation view module + ConversationList view module; SendDm action module; NIP-44 encryption / NIP-59 gift-wrapping; outbox routing for DMs (recipient inbox relays only — never public).
307:**Runnable artifact.** iOS Twitter slice with working DMs + push notifications. Report in `docs/perf/m9/messaging.md`.
382:### M12 — Wallet (NWC + zaps + Cashu + nutzaps)
394:- `WalletBalance` view module; `ZapHistory` view module.
402:- Wallet operations never block the UI thread.
444:- Generated Swift wrappers: `useProfile`, `@Profile`, `useTimeline`, `@Wallet`, etc.
508:- A minimal **starter app** (distinct from the proof/Twitter app) implementing only: login + timeline + compose + profile + DMs. Stays under the platform LOC budgets from spec §3.2.
566:| 17 | nmp-nip17 | M9 | DMs |
572:| 44 | nmp-nip17 | M9 | encryption (via NIP-17) |
574:| 47 | nmp-nwc | M12 | wallet connect |
577:| 57 | nmp-nip57 | M12 | zaps |
578:| 59 | nmp-nip17 | M9 | gift wrap (via NIP-17) |
579:| 60 | nmp-nip60 | M12 | Cashu |
580:| 61 | nmp-nip61 | M12 | nutzaps |
595:- **M6 (signer + write path) is a serialization point** — most downstream milestones (M7, M8, M9, M10, M12) depend on it. Land this fast.
597:- **M11 (podcast app)** can begin as soon as M10 (Blossom + long-running capabilities) is in good shape, even if M12/M13 haven't started.
677:- **Not exhaustive about post-v1 work.** NIP-29 groups, NIP-23 long-form, NIP-71 video, additional protocol modules, additional app demonstrations (Highlighter-lite, TENEX-lite, etc.) are post-v1 — they validate the kernel boundary further but are not v1 deliverables.

codex
Findings:

- `docs/plan.md:1` — file is 680 LOC, over the 500-line hard ceiling for hand-authored docs. Split into milestone docs or linked plan sections.

- `docs/plan.md:287` / `docs/plan.md:382` — M9 DMs and M12 Wallet are still in the v1 ladder, but session scope says both are deferred. Move them to post-v1 and update roadmap/cross-links.

- `docs/plan.md:677` — Highlighter is listed as post-v1, but session scope says M11.5 Highlighter is pending. Add M11.5 and remove the post-v1 classification.

- `docs/plan.md:362` — “whatever the Nostr podcast NIP is called, e.g. NIP-XX” is a TODO placeholder. Replace with a concrete researched protocol decision or scope RSS-only/Nostr-feed fallback explicitly.

- `docs/plan.md:482` — “Web shell stack TBD” is another unresolved placeholder inside v1 scope. Pick the stack or make stack selection an explicit pre-M15 gate.

Not clean.
tokens used
49,995
Findings:

- `docs/plan.md:1` — file is 680 LOC, over the 500-line hard ceiling for hand-authored docs. Split into milestone docs or linked plan sections.

- `docs/plan.md:287` / `docs/plan.md:382` — M9 DMs and M12 Wallet are still in the v1 ladder, but session scope says both are deferred. Move them to post-v1 and update roadmap/cross-links.

- `docs/plan.md:677` — Highlighter is listed as post-v1, but session scope says M11.5 Highlighter is pending. Add M11.5 and remove the post-v1 classification.

- `docs/plan.md:362` — “whatever the Nostr podcast NIP is called, e.g. NIP-XX” is a TODO placeholder. Replace with a concrete researched protocol decision or scope RSS-only/Nostr-feed fallback explicitly.

- `docs/plan.md:482` — “Web shell stack TBD” is another unresolved placeholder inside v1 scope. Pick the stack or make stack selection an explicit pre-M15 gate.

Not clean.
