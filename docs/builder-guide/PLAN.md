# NMP Builder Guide — PLAN

> **Plan only.** TOC + per-section briefs. Parallel writer agents will produce `NN-*.md`. No prose docs here. Two audiences: **builders** (humans coming from NDK / Applesauce / raw `nostr-sdk`) and **agents** (LLMs extending NMP). Authoring rules: each file ≤ 300 LOC (AGENTS.md); cross-links `[NN — title](NN-name.md)`; cite `path:line` only if file exists today on master; distill not duplicate; ≥3 anti-patterns and ≥2 concrete deliverables per section.

## Status legend (citation discipline)

| Status | Writer may cite | Must NOT cite |
|---|---|---|
| **SHIPS** | `crates/**/*.rs:line`, `apps/**/*.rs:line`, `ios/**/*.swift:line`, any `docs/**` | speculative paths |
| **LANDED** | `docs/design/**/*.md`§, `docs/decisions/000N-*.md`, `docs/plan/m*.md`, plus partial `crates/` cites for scaffolded-but-incomplete code | `path:line` for code paths that don't exist on master |
| **PLANNED** | `docs/plan/m*.md`, ADRs, `docs/design/**/*.md`, scope memo | any `crates/`/`apps/`/`ios/` path absent from master |

Doctrine canon: `docs/product-spec/doctrine.md` (canonical D0–D10 file) + `docs/product-spec/overview-and-dx.md` §1.5 (in-page restatement). Research synthesis: `docs/research/sessions/synthesis.md` + `docs/design/ndk-applesauce-lessons.md` + `docs/research/{ndk,applesauce}/`. Current scope: kernel substrate + planner + EventStore/MemEventStore + subscription pool + NIP-42 auth gate + publish engine **all SHIP**; protocol crates `nmp-signers` / `nmp-nip29` / `nmp-nip42` / `nmp-nip77` **all SHIP**; Chirp is the active product shell. Podcast and Highlighter app surfaces are deferred until Chirp is complete. FFI today is raw C JSON-over-string (`crates/nmp-core/src/ffi.rs`); UniFFI migration is M14 (PLANNED).

## Master TOC (28 sections)

| # | Title | File | Status | Audience | Budget |
|--:|---|---|---|---|--:|
| 00 | How to read this guide | `00-how-to-read.md` | SHIPS | both | 500 |
| 01 | What NMP is + why it exists | `01-what-nmp-is.md` | SHIPS | builders | 1800 |
| 02 | Mental model — kernel + 5 trait families | `02-mental-model.md` | SHIPS | both | 2200 |
| 03 | Doctrine D0–D10 end-to-end | `03-doctrine-d0-d8.md` | SHIPS | both | 2500 |
| 04 | RMP bible + actor model (TEA on one thread) | `04-actor-and-tea.md` | SHIPS | both | 1800 |
| 05 | Kernel substrate — the 5 trait families | `05-substrate-traits.md` | SHIPS | both | 2800 |
| 06 | Reactivity contract (D8) | `06-reactivity-contract.md` | SHIPS | agents | 2400 |
| 07 | Subscription planner — Interest → CompiledPlan → wire | `07-subscription-planner.md` | SHIPS | both | 2600 |
| 08 | EventStore + insert invariants + GC | `08-eventstore.md` | SHIPS | both | 2400 |
| 09 | Persistence (LMDB) + watermarks | `09-persistence-lmdb.md` | LANDED | agents | 1800 |
| 10 | Outbox routing (NIP-65) | `10-outbox-routing.md` | SHIPS | both | 2000 |
| 11 | Sessions + signers + identity scopes (`nmp-signers`) | `11-sessions-signers.md` | SHIPS | both | 2200 |
| 12 | Publishing + the publish engine | `12-publish-and-ledger.md` | SHIPS | both | 2000 |
| 13 | Sync engine — `nmp-nip77` (NIP-77 first, REQ second) | `13-sync-engine.md` | SHIPS | agents | 1600 |
| 14 | Subscription lifecycle + relay manager + NIP-42 | `14-relay-manager.md` | SHIPS | agents | 1800 |
| 15 | Codegen — `nmp gen modules` + per-app FFI crate | `15-codegen-and-ffi.md` | SHIPS | both | 2200 |
| 16 | Capabilities (D7) | `16-capabilities.md` | SHIPS | both | 2000 |
| 17 | iOS shell — SwiftUI consumes the kernel | `17-ios-shell.md` | SHIPS | builders | 1800 |
| 18 | Testing — `nmp-testing`, benches, contract tests | `18-testing.md` | SHIPS | both | 2200 |
| 19 | Walkthrough — build a microblog app | `19-walkthrough-microblog.md` | SHIPS | builders | 3000 |
| 20 | Adding a new protocol module (`nmp-nip29` as reference) | `20-new-protocol-module.md` | SHIPS | agents | 2400 |
| 21 | The framework-magic contract | `21-framework-magic.md` | SHIPS | builders | 1200 |
| 22 | Doctrine compliance checklist | `22-doctrine-checklist.md` | SHIPS | agents | 1000 |
| 23 | Glossary | `23-glossary.md` | SHIPS | both | 800 |
| 24 | Reference cards | `24-reference-cards.md` | SHIPS | both | 800 |
| 25 | Migration — NDK / Applesauce → NMP | `25-migration-from-ndk-applesauce.md` | SHIPS | builders | 2000 |
| 26 | FAQ + troubleshooting | `26-faq-troubleshooting.md` | SHIPS | builders | 1500 |
| 27 | Doc/code discrepancies (orchestrator queue) | `27-discrepancies.md` | SHIPS | agents | 800 |

## Per-section briefs

Compact format. Each section: **covers** (1 line) · **cite** (file:line refs) · **deliver** (2–3 concrete artifacts) · **anti** (3 anti-patterns) · **xref** (other sections). `prereqs` implied by xref. `outcomes` implied by covers.

### 00 — How to read this guide  *(SHIPS · both · 500)*
- covers: who the guide is for; SHIPS/LANDED/PLANNED legend; reading paths for builder vs agent; doc-bug filing route via section 27.
- cite: this `PLAN.md`; `README.md`; `AGENTS.md`; `docs/plan/status.md`.
- deliver: 2 ordered reading paths ("ship an app" / "extend the kernel"); ASCII graph of section dependencies.
- anti: reading sections out of order; copying PLANNED code into a real app; assuming a section is wrong when status flag explicitly marks aspirational content.
- xref: 01, 02, 22.

### 01 — What NMP is + why it exists  *(SHIPS · builders · 1800)*
- covers: one-paragraph definition; the "make broken Nostr apps impossible" thesis; concrete contrast with NDK / Applesauce / raw `nostr-sdk` (one paragraph each); the current arcs (M0–M10 social · M10.5 hardening · Chirp proof · M13–M17 release); status snapshot from `docs/plan/status.md`.
- cite: `README.md:1-117`; `docs/aim.md:9-89`; `docs/plan.md:30-52`; `docs/perf/orchestration-log.md:38-41`; `docs/design/ndk-applesauce-lessons.md:1-65`; `docs/research/sessions/synthesis.md`; `docs/research/ndk/missing-features-for-nmp.md`; `docs/research/applesauce/missing-features-for-nmp.md`.
- deliver: comparison table NDK/Applesauce/raw `nostr-sdk`/NMP × 6 axes (state ownership, outbox, kind:3 auto, reactivity, signers, FFI); "what NMP is NOT" box; "deferred to post-v1" callout (M9 DMs, M12 Wallet).
- anti: framing NMP as "Rust NDK"; implying NDK feature parity (NDK has DMs/Wallet, NMP defers both); implying v1 ships everything in `docs/aim.md` (post-v1 scope memo deferrals stand).
- xref: 02, 03, 25.

### 02 — Mental model — kernel + 5 trait families  *(SHIPS · both · 2200)*
- covers: the 4-layer stack (kernel · protocol modules · app core · platform shell); one-paragraph per trait family (Domain, View, Action, Capability, Identity); the no-app-nouns-in-kernel rule; what crosses FFI and what doesn't; concrete map "where does Profile live?" (`nmp-nip29` for groups, app-owned crates for app nouns, etc.).
- cite: `docs/decisions/0009-app-extension-kernel-boundary.md:22-62`; `docs/design/app-extension-kernel.md:55-120`; `crates/nmp-core/src/substrate/mod.rs:1-79`; `crates/fixture-todo-core/src/lib.rs:13-265`; `crates/nmp-nip29/src/lib.rs:1-57` (concrete protocol-crate boundary statement).
- deliver: ASCII stack diagram (shell → generated FFI crate → kernel + protocol + app cores) with the 6 real shipped crates labeled in the right layer; D0/D4/D5 callouts on the diagram.
- anti: putting `Highlight`/`Episode`/`Project` in `nmp-core`; conflating ViewModule with platform UI components; bypassing ViewModule to render raw events in SwiftUI; adding a 6th trait family without an ADR.
- xref: 03, 05, 15, 20.

### 03 — Doctrine D0–D10 end-to-end  *(SHIPS · both · 2500)*
- covers: each D0–D10 with statement · what it forbids · where it's enforced today · regression test (or `[PENDING M_n]`); note D0–D5 are policy / D6–D10 are substrate; conflict resolution = listed order; pointers to in-crate doctrine map comments.
- cite: `docs/product-spec/doctrine.md:1-98` (canonical); `docs/product-spec/overview-and-dx.md:27-118` (in-page restatement); `docs/aim.md` §6; `docs/decisions/0001-..0004-*.md` (D8 substrate); `docs/design/framework-magic.md:24-72` (contract table); `crates/nmp-testing/tests/framework_magic_contract.rs` (named tests); `crates/nmp-core/src/publish/mod.rs:1-40` (D3/D4/D5/D6/D7/D8 in-crate map); `crates/nmp-nip77/src/lib.rs:25-44` (D2/D6/D8 in-crate map); `crates/nmp-nip29/src/lib.rs:11-19` (D0 boundary statement).
- deliver: 9-row table (doctrine · statement · forbids · enforced-by · test); reusable PR-review rubric code-block; "doctrine map comment" template for new modules.
- anti: `Result<T,E>` across FFI; AppState growing beyond open-view projection; per-event hot-path allocations; native code deciding retry policy; growing `nmp-core` to host app nouns.
- xref: 05, 06, 10, 16, 22.

### 04 — RMP bible + actor model (TEA on one thread)  *(SHIPS · both · 1800)*
- covers: TEA in NMP vocabulary (AppState · KernelAction · KernelUpdate · handle_message); the actor + tokio split; fire-and-forget dispatch; monotonic `rev: u64`; snapshot-default emit; the 10 RMP invariants restated in NMP terms; the actor's current modular layout (`actor/{mod,relay_mgmt,tick}.rs`).
- cite: `docs/aim.md:19-90`; `crates/nmp-core/src/actor/mod.rs:1-80`; `crates/nmp-core/src/actor/tick.rs`; `crates/nmp-core/src/actor/relay_mgmt.rs`; `crates/nmp-core/src/app.rs:1-30`; `crates/nmp-core/src/lib.rs:1-50` (the `testing::spawn_actor` test-support gate).
- deliver: sequence diagram (UI → dispatch → flume → actor → mutation → update_tx → reconciler → UI); "which thread runs what" table; the `#[cfg(any(test, feature = "test-support"))]` policy explained.
- anti: expecting dispatch to return a result; expecting synchronous state read after dispatch; spawning ad-hoc threads in apps; holding mutable state in Swift/Kotlin; depending on `spawn_actor` from production code.
- xref: 05, 06, 17.

### 05 — Kernel substrate — the 5 trait families  *(SHIPS · both · 2800; split if >280 LOC into 05a/05b)*
- covers: each trait family signature · associated types · lifecycle · when to use it; the canonical `fixture-todo-core` impl per family (non-Nostr proof); concrete examples from `nmp-nip29` (Nostr-shaped 13 domains + 7 views + 15 actions) and app-shaped modules; ModuleRegistry composition.
- cite: `crates/nmp-core/src/substrate/domain.rs:1-49`; `crates/nmp-core/src/substrate/view.rs:37-80`; `crates/nmp-core/src/substrate/action.rs:10-84`; `crates/nmp-core/src/substrate/capability.rs:1-24`; `crates/nmp-core/src/substrate/identity.rs:8-76`; `crates/nmp-core/src/substrate/mod.rs:19-79`; `crates/fixture-todo-core/src/lib.rs:13-265` (canonical 5-family impl); `crates/nmp-nip29/src/lib.rs:30-56` (real Nostr-shaped module manifest); `docs/design/kernel-substrate.md`.
- deliver: per-family ~15-line "shape" code block; decision tree "I want X — which trait?"; annotated fixture-todo-core walkthrough; sidebar "how nmp-nip29 uses all 5 families."
- anti: business policy in CapabilityModule (D7 violation); long-lived state in IdentityContext; ViewModule with empty `dependencies()` (forces table scan); skipping migrations in DomainModule; putting Nostr nouns in `nmp-core` substrate.
- xref: 02, 06, 16, 20.

### 06 — Reactivity contract (D8)  *(SHIPS · agents · 2400)*
- covers: the actor reactive loop; composite vs broad reverse-index keys; per-view delta budget; working-set / cold-store split; allocation budget after warmup; how ViewModule plugs in; `reactivity-bench` validation harness (run 002 passed all gates per status.md).
- cite: `docs/design/reactivity/loop-and-reverse-index.md:30-100`; `docs/design/reactivity/scheduling-and-data-model.md`; `docs/design/reactivity/view-deltas-and-projections.md`; `docs/decisions/0001-composite-dependency-keys.md` → `0004-allocation-measurement.md`; `crates/nmp-testing/bin/reactivity-bench/main.rs`; `docs/perf/reactivity-bench/` (run reports).
- deliver: the loop diagram lifted from design; composite-vs-broad key table (5 + 5 with guardrail callouts); current budgets table; `reactivity-bench` results table excerpt.
- anti: broad single-axis keys for typed views; emitting `Some(delta)` when nothing changed; allocations in `on_event_inserted`; polling instead of observing from UI.
- xref: 05, 07, 18.

### 07 — Subscription planner — Interest → CompiledPlan → wire  *(SHIPS · both · 2600)*
- covers: why string-formatter planners fail; LogicalInterest (id, scope, shape, hints, lifecycle); the 4-stage compiler pipeline (resolve → fallback → merge → plan-id); the 9 merge-lattice rules including Rule 9 `relay_pin` (third routing lane); CompiledPlan + plan-id stability; logical-vs-wire split; recompilation triggers; coalescing + auto-close + EOSE; partition cases (incl. Case E for relay-pinned).
- cite: `crates/nmp-core/src/planner/mod.rs:1-40`; `crates/nmp-core/src/planner/interest.rs`; `crates/nmp-core/src/planner/plan.rs`; `crates/nmp-core/src/planner/compiler/` (module dir); `crates/nmp-core/src/planner/lattice/` (module dir); `docs/design/subscription-compilation/intro.md` §1-§2; `docs/design/subscription-compilation/compiler.md`; `docs/design/subscription-compilation/recompilation.md`; `docs/design/subscription-compilation/outbox.md`; `docs/design/subscription-compilation/tests.md`; `docs/decisions/0012-relay-pinned-interest-and-third-routing-lane.md`; `docs/plan/m2-subscription-compilation.md`.
- deliver: CompiledPlan ASCII for "5 followed authors × 2 relays each"; recompilation triggers table; worked example "kind:3 arrives → CLOSE/REQ deltas"; relay_pin/Case E callout (NIP-29 host pin).
- anti: assuming 1 filter == 1 REQ; passing relay URLs to view-open APIs; hand-rolled dedup in app code; forgetting to close interests on view destruction; emitting plan-id churn on trivial recompile.
- xref: 06, 08, 10, 14, 20.

### 08 — EventStore + insert invariants + GC  *(SHIPS · both · 2400)*
- covers: the insert path; replaceable (kinds 0/3/10000-19999) supersession; parameterized replaceable (30000-39999); kind:5 delete + tombstone; NIP-40 expiration; NIP-26 delegation; ephemeral events; provenance merge; claim-based GC; fallback loader contract; `MemEventStore` vs `LmdbEventStore`.
- cite: `crates/nmp-core/src/store/mod.rs:1-50`; `crates/nmp-core/src/store/events.rs`; `crates/nmp-core/src/store/types/mod.rs` (InsertOutcome, RejectReason, GcBudget, etc.); `crates/nmp-core/src/store/mem/mod.rs` (referenced via mod.rs:18); `docs/product-spec/subsystems.md:7-55`; `docs/design/framework-magic/replaceable.md`; `docs/design/lmdb/gc.md`; `docs/design/lmdb/watermarks.md`; `crates/nmp-core/src/kernel/ingest/mod.rs`.
- deliver: "what happens on insert" by kind table; tombstone state diagram; fallback-loader interface sketch with the 4 miss types; `InsertOutcome` enum variants table.
- anti: bypassing the insert path; mutating events after insert; treating cache-miss as "definitely not on relay" without watermark; manual delete-by-event from app code.
- xref: 07, 09, 13, 21.

### 09 — Persistence (LMDB) + watermarks  *(LANDED · agents · 1800)*
- covers: durable storage layout; key/value encoding; watermarks table; tombstones; GC pins; feature-gated `LmdbEventStore` skeleton; ADR-0011 (NMP owns the LMDB env); backend abstraction (LMDB native / mem / future web).
- cite: `crates/nmp-core/src/store/lmdb.rs` (feature-gated skeleton); `crates/nmp-core/src/store/types/mod.rs` (`WatermarkRow`, `TombstoneRow`); `crates/nmp-core/src/store/mod.rs:15-50` (StorageBackend enum); `docs/decisions/0011-lmdb-env-sharing.md`; `docs/design/lmdb-schema.md`; `docs/design/lmdb/keys.md`; `docs/design/lmdb/watermarks.md`; `docs/design/lmdb/gc.md`; `docs/design/lmdb/trait.md`; `docs/design/lmdb/tests.md`; `docs/plan/m3-persistence.md`.
- deliver: key-encoding table (composite → bytes); watermarks row spec; "survives restart" bullet list; the `lmdb-backend` feature build matrix.
- anti: app-side persistence parallel to EventStore; cross-process LMDB sharing; sharing an `lmdb::Env` from another crate (ADR-0011 violation); treating cache as source of truth (D4 violation).
- xref: 08, 13, 27.

### 10 — Outbox routing (NIP-65)  *(SHIPS · both · 2000)*
- covers: routing table from spec §7.3 (read fan-out, write fan-out, DMs fail-closed); per-pubkey relay-list lifecycle; the `MailboxCache` trait + `InMemoryMailboxCache` impl; explicit-override audit path (Stage 1 outbox in planner); how kind:3 auto-tracking propagates (`FollowListChanged` trigger).
- cite: `crates/nmp-core/src/planner/compiler/` (InMemoryMailboxCache); `crates/nmp-core/src/planner/mod.rs:18-25` (InMemory wiring example); `docs/product-spec/subsystems.md:72-101`; `docs/design/subscription-compilation/outbox.md`; `docs/design/subscription-compilation/nip65.md`; `docs/design/framework-magic/outbox.md`; `docs/design/framework-magic/kind3.md`; `docs/research/ndk/outbox.md`; `docs/research/applesauce/outbox.md`.
- deliver: routing table verbatim from spec; override-call-site checklist; kind:3-arrives sequence diagram; "what's still on the constant relay" reality-check note (kernel demo still uses `relay.primal.net` + `purplepag.es` until the planner is wired into the actor's REQ path).
- anti: passing relays to `SendNote` (no such surface); DM fallback to public relays; reading mailboxes for a publish without recipients; per-call relay lists in app code.
- xref: 07, 11, 12, 21.

### 11 — Sessions + signers + identity scopes  *(SHIPS · both · 2200)*
- covers: SessionState shape; the `Signer` trait + 4 concrete impls (`LocalKeySigner`, `Nip46Signer`, `Nip07Signer`, future Amber NIP-55 via capability); `AccountManager` with synchronous active-switch and applesauce-style mismatch post-conditions; bunker URI parser (fuzz target); kind:3 auto-rewire on switch; IdentityScopeKind (HumanAccount / AppLocal / ExternalSigner / Ephemeral); the doctrine D0 placement of identity in `nmp-signers` (not `nmp-core`).
- cite: `crates/nmp-signers/src/lib.rs:1-37`; `crates/nmp-signers/src/signers/traits.rs`; `crates/nmp-signers/src/signers/local.rs`; `crates/nmp-signers/src/signers/nip46/mod.rs`; `crates/nmp-signers/src/signers/nip07.rs`; `crates/nmp-signers/src/bunker/parser.rs`; `crates/nmp-signers/src/identity/manager.rs`; `crates/nmp-signers/src/identity/rewire.rs`; `crates/nmp-core/src/substrate/identity.rs:8-76`; `docs/decisions/0015-m6-signer-design.md`; `docs/design/framework-magic/sessions.md`; `docs/design/framework-magic/signers.md`; `docs/research/sessions/synthesis.md`; `docs/plan/m6-signers-write.md`; `docs/plan/m8-multi-account.md`.
- deliver: signer-kind comparison table (latency, security, UX, capability deps); switch-account action-to-state diagram; IdentityScopeKind decision tree; `parse_bunker_uri` worked example.
- anti: account switch as tear-down/rebuild; HumanAccount scope for app-local agents; signer calls direct from UI; signer-mismatch publishes (ADR-0015 post-condition violation); UI guards on "is logged in?" that withhold cached content (D1).
- xref: 10, 12, 16.

### 12 — Publishing + the publish engine  *(SHIPS · both · 2000)*
- covers: `PublishAction`; the engine's per-(event, relay) state machine; `RelayAck` D7 envelope; `PublishOutcome::Mixed`/`FailedAfterRetries`; durable retry queue contract; `PublishStatusView` snapshot; the read/write API split (reads = store subscriptions; writes = actions only); engine-error → `RecentFailure` mapping for D6 FFI cleanliness.
- cite: `crates/nmp-core/src/publish/mod.rs:1-40`; `crates/nmp-core/src/publish/action.rs`; `crates/nmp-core/src/publish/engine.rs`; `crates/nmp-core/src/publish/state.rs`; `crates/nmp-core/src/publish/view.rs`; `crates/nmp-core/src/publish/traits.rs`; `crates/nmp-core/src/substrate/action.rs:10-84`; `docs/product-spec/subsystems.md:137-156`; `docs/product-spec/subsystems.md:377-390` (offline queue); `docs/plan/m7-publishing.md`.
- deliver: publish-action state diagram (Pending → SignerPrompt → PerRelayAttempt[] → final); `RelayAck` envelope schema; read-vs-write API split table.
- anti: bypassing the publish engine; per-action error types across FFI; storing pending publishes in the platform; build-sign-publish manually; native deciding retry policy (D7).
- xref: 05, 10, 11, 14, 16.

### 13 — Sync engine — `nmp-nip77` (NIP-77 first, REQ second)  *(SHIPS · agents · 1600)*
- covers: the `Reconciler` (deterministic step API over `negentropy::Negentropy`); NIP-77 wire frames `NEG-OPEN`/`NEG-MSG`/`NEG-CLOSE`; per-relay capability cache + probe state machine; coverage gate (`SyncStrategy::{SkipReq,NegThenReq,ReqSince}`); planner-gate hook; three triggers (foreground, view-open-gap, relay-reconnect); per-(filter,relay) `bytes_saved_vs_req` metrics; `RunSync` action.
- cite: `crates/nmp-nip77/src/lib.rs:1-44`; `crates/nmp-nip77/src/reconciler.rs`; `crates/nmp-nip77/src/wire.rs`; `crates/nmp-nip77/src/capability.rs`; `crates/nmp-nip77/src/capability_domain.rs`; `crates/nmp-nip77/src/coverage_gate.rs`; `crates/nmp-nip77/src/planner_gate.rs`; `crates/nmp-nip77/src/triggers.rs`; `crates/nmp-nip77/src/metrics.rs`; `crates/nmp-nip77/src/run_sync.rs`; `docs/product-spec/subsystems.md:241-292`; `docs/design/framework-magic/sync.md`; `docs/plan/m4-negentropy.md`.
- deliver: triggers table; SyncStrategy decision matrix (CompleteAsOf × supports_nip77 × gap-size); `RunSync` invocation example; metrics counters table.
- anti: assuming all relays speak NIP-77 (probe + cache); gating live reads on sync completion; manual REQ scans in app code for backfill; treating a watermark as "everything ever" rather than "complete as of T."
- xref: 07, 08, 14.

### 14 — Subscription lifecycle + relay manager + NIP-42  *(SHIPS · agents · 1800)*
- covers: the M8-subs split — `InterestRegistry` (D4 single-writer), trigger inbox (FIFO + per-tick coalesce), wire-emitter (CompiledPlan→WireFrame diff), `ConnectionPool` (uniform send path), `AuthGate` (NIP-42), `LifecycleGate`; `nmp-nip42` builder + flow + state; reconnect + replay; CLOSED handling.
- cite: `crates/nmp-core/src/subs/mod.rs:1-40`; `crates/nmp-core/src/subs/registry.rs`; `crates/nmp-core/src/subs/inbox.rs`; `crates/nmp-core/src/subs/trigger.rs`; `crates/nmp-core/src/subs/wire.rs`; `crates/nmp-core/src/subs/pool.rs`; `crates/nmp-core/src/subs/auth_gate.rs`; `crates/nmp-core/src/subs/lifecycle_gate.rs`; `crates/nmp-nip42/src/lib.rs`; `crates/nmp-nip42/src/builder.rs`; `crates/nmp-nip42/src/flow.rs`; `crates/nmp-nip42/src/state.rs`; `crates/nmp-nip42/src/frame.rs`; `crates/nmp-core/src/relay.rs`; `crates/nmp-core/src/relay_worker.rs`; `crates/nmp-core/src/actor/relay_mgmt.rs`; `docs/plan/m5-nip42.md`; `docs/plan/m8-subscription-lifecycle.md` (note: distinct from `m8-multi-account.md`).
- deliver: connection-state diagram; NIP-42 challenge → response → re-emit sequence; "what happens on reconnect to live REQs" bullets; the four M8-subs seams listed.
- anti: re-sending REQs from app code on reconnect; treating CLOSED as fatal vs transient; opening per-view connections; blind write replay after auth; confusing M8-subs (this section) with M8-multi-account ([11]).
- xref: 07, 12, 13.

### 15 — Codegen — `nmp gen modules` + per-app FFI crate  *(SHIPS · both · 2200)*
- covers: AppManifest format (`nmp.toml`); generated outputs (`AppAction`, `AppUpdate`, `ViewSpec`, `FfiApp`, capability/domain constants); kernel + protocol + app composition; current raw C FFI today vs UniFFI M14; web wasm target; the determinism test; the `apps/fixture/` reference output.
- cite: `crates/nmp-codegen/src/manifest.rs:1-97`; `crates/nmp-codegen/src/generate.rs:12-163`; `crates/nmp-codegen/src/lib.rs:1-61`; `crates/nmp-codegen/tests/determinism.rs`; `apps/fixture/nmp.toml`; `apps/fixture/nmp-app-fixture/src/action.rs:1-15`; `apps/fixture/nmp-app-fixture/src/ffi.rs:1-23`; `apps/fixture/nmp-app-fixture/src/lib.rs`; `crates/nmp-core/src/ffi.rs:44-310` (current raw C FFI); `docs/decisions/0010-generated-app-enum-vs-type-erased-registry.md`; `docs/plan/m14-uniffi.md` (PLANNED); `docs/plan/m16-cli-starter.md` (`nmp init` PLANNED).
- deliver: annotated `nmp.toml` example; "before vs after generate" diff for AppAction; current-vs-future FFI box (raw C JSON today, UniFFI at M14, `nmp init` at M16).
- anti: hand-editing generated files; expecting UniFFI today; assuming `nmp init` exists yet; type-erased registries (rejected by ADR-0010); domain types in `nmp-core`.
- xref: 02, 05, 17.

### 16 — Capabilities (D7)  *(SHIPS · both · 2000)*
- covers: capability contract (request → native execution → result envelope); v1 capability catalog (Keyring, Push, ExternalSigner, NetworkMonitor, BlobPicker); protocol-module-specific capabilities such as NIP-77 transport; idempotence + bounded scope; lifecycle (start/stop/restart safe N times).
- cite: `crates/nmp-core/src/substrate/capability.rs:1-24`; `docs/product-spec/api-surface.md:193-228`; `docs/design/framework-magic/capabilities.md`; `crates/nmp-nip77/src/capability.rs`.
- deliver: capability-shape Rust block; "decides vs reports" table × 8 worked examples (mixing core + podcast capabilities); idempotence checklist.
- anti: native retry policy; capability holding cached state beyond OS handles; capability returning `Result`-typed errors instead of envelopes; native deciding which relay to publish to; non-idempotent start.
- xref: 03, 05, 11, 12, 17.

### 17 — iOS shell — SwiftUI consumes the kernel  *(SHIPS · builders · 1800)*
- covers: the bridge (raw C FFI today); `KernelHandle` Swift wrapper; `KernelModel` ObservableObject; JSON-snapshot wire format; `@Published` state-shadow pattern; capability injection from Swift; what a SwiftUI view that consumes the kernel looks like; current Chirp shell inventory.
- cite: `crates/nmp-core/src/ffi.rs:44-310`; `ios/Chirp` Swift bridge files.
- deliver: annotated `KernelHandle` Swift snippet; Rust-side emit -> SwiftUI re-render sequence; JSON-update shape with `rev` guard; current app status box.
- anti: caching state in Swift; calling C FFI off the main actor without hopping; mutating `@Published` from background; business logic in SwiftUI; "if missing { spinner }" gates (D1).
- xref: 04, 15, 16.

### 18 — Testing — `nmp-testing`, benches, contract tests  *(SHIPS · both · 2200)*
- covers: testing-crate surface; framework-magic contract tests (13 behavior tests + coverage meta-test, 14 total); `reactivity-bench` (run 002 passed); `firehose-bench` (replay/capture/live modes); FFI-stress harness from M10.5; `test-support` feature gate; the "no networking required" principle.
- cite: `crates/nmp-testing/src/lib.rs`; `crates/nmp-testing/bin/firehose-bench/main.rs`; `crates/nmp-testing/bin/reactivity-bench/main.rs`; `crates/nmp-core/tests/substrate_registry.rs`; `crates/nmp-core/src/lib.rs:20-50` (test-support gate); `docs/design/framework-magic/test-scaffolding.md`; `docs/design/framework-magic.md:46-63` (test name canon); `docs/design/firehose-bench.md`; `docs/plan/test-pyramid.md`; `docs/plan/m10.5-ffi-hardening.md`; `docs/perf/reactivity-bench/`; `docs/perf/firehose-bench/`; `docs/perf/m10.5/`.
- deliver: test-pyramid diagram; worked example "I added a ViewModule — what tests do I write?"; framework-magic-test naming convention recap; "where to add a contract bullet" recipe.
- anti: platform tests for Rust logic; treating benches as integration tests; skipping the contract meta-test when adding a bullet; flake-prone time-based tests; requiring real relays in CI.
- xref: 06, 21, 22.

### 19 — Walkthrough — build a microblog app  *(SHIPS · builders · 3000)*
- covers: end-to-end build of a kind:1 microblog: `nmp.toml`, app-core crate (one DomainModule + one ViewModule + one ActionModule wired to publish engine), regenerate, wire SwiftUI, run on simulator. Mirrors `fixture-todo-core` structurally. Today's wiring uses raw C FFI; UniFFI path is M14.
- cite: `crates/fixture-todo-core/src/lib.rs:1-304` (canonical 5-family example); `apps/fixture/nmp.toml`; `apps/fixture/nmp-app-fixture/src/*.rs`; `crates/nmp-core/src/publish/action.rs` (publish wiring target); `crates/nmp-signers/src/signers/local.rs` (the signer the example will use); `docs/plan/m7-publishing.md`.
- deliver: complete file tree of the example; per-file ~30-line skeletons; build/run cheatsheet (`cargo build`, codegen invocation, iOS sim run); "what publishes today vs tomorrow" milestone matrix (publish substrate ships today, multi-account M8, UniFFI M14).
- anti: adding nostr types to `nmp-core`; manual REQ in app code; per-platform SwiftData/Room caching parallel to AppState; skipping ViewModule and rendering raw events; building signing in the app; making the example "Twitter-shaped" (defeats the D0 demo).
- xref: 02, 05, 12, 15, 17, 20, 22.

### 20 — Adding a new protocol module (`nmp-nip29` as reference)  *(SHIPS · agents · 2400)*
- covers: when to add a protocol module vs an app module; 5-trait-family checklist; cargo dep wiring; manifest updates; integration tests; the `nmp-nip29` exit-gate as the canonical reference (boundary statement "does NOT import any other `nmp-nip*` crate"; "`nmp-core` gains zero group nouns"); the `relay_pin` third routing lane as a kernel-substrate change that survived D0.
- cite: `crates/nmp-nip29/src/lib.rs:1-57` (boundary statement); `crates/nmp-nip29/src/group_id.rs`; `crates/nmp-nip29/src/kinds.rs`; `crates/nmp-nip29/src/interest.rs`; `crates/nmp-nip29/src/domain/mod.rs`; `crates/nmp-nip29/src/view/mod.rs`; `crates/nmp-nip29/src/action/mod.rs`; `crates/nmp-nip29/src/cache/mod.rs`; `crates/nmp-nip29/src/moderation.rs`; `crates/nmp-nip29/src/tests.rs`; `docs/design/nip29-crate.md`; `docs/design/nip29/routing.md`; `docs/design/nip29/kinds.md`; `docs/design/nip29/moderation.md`; `docs/decisions/0009-app-extension-kernel-boundary.md:44-62`; `docs/decisions/0012-relay-pinned-interest-and-third-routing-lane.md`; `docs/decisions/0013-nip29-metadata-signer-trust-model.md`; `docs/design/view-catalog/template-and-enumeration.md:1-60`; `crates/fixture-todo-core/src/lib.rs:1-304` (smaller comparison).
- deliver: "protocol vs app module" decision table; per-trait minimum-required-impl checklist; PR-ready file list ("must add", "may add", "must NOT add"); "when a kernel change is justified" rubric using relay_pin as worked example.
- anti: protocol module with app-specific deps; protocol module owning policy (vs reusable mechanism); protocol module mutating session state; skipping integration tests against MockRelay; adding capability variants in `nmp-core`; importing other `nmp-nip*` from a protocol module.
- xref: 05, 07, 15, 18, 22.

### 21 — The framework-magic contract  *(SHIPS · builders · 1200)*
- covers: thin pointer to the existing 13-bullet contract; per-bullet one-line "what the app gets for free"; doctrine each bullet discharges; owning milestone; the 13 behavior tests plus the coverage meta-test in `framework_magic_contract.rs` (14 total); how to add a new bullet (ADR).
- cite: `docs/design/framework-magic.md:24-72`; `docs/design/framework-magic/intro.md`; `docs/design/framework-magic/{kind3,replaceable,outbox,subs,sync,signers,sessions,capabilities,test-scaffolding}.md`; `crates/nmp-testing/tests/framework_magic_contract.rs`; `docs/plan/scope-adjustments-2026-05-18.md` "framework-magic contract" section.
- deliver: C1–C13 table reproduced with "what the app gets" column added; status column ([DONE]/[PARTIAL]/[PENDING M_n]); mini-recipe for adding C14 (ADR template + test name convention).
- anti: assuming the status column without checking both `framework-magic.md` and the active test file; paraphrasing into stronger claims than the contract makes; app-side fallback code "just in case"; re-implementing kind:3 watch in SwiftUI.
- xref: 03, 08, 10, 11, 18.

### 22 — Doctrine compliance checklist  *(SHIPS · agents · 1000)*
- covers: ~25-item checklist (≥1 per doctrine) as yes/no questions; PR-template usage; how post-merge codex review consumes it (per `docs/perf/codex-reviews/`); the "doctrine map comment" convention shipping in `publish/mod.rs` and `nip77/lib.rs`.
- cite: `docs/product-spec/doctrine.md:1-98`; `docs/product-spec/overview-and-dx.md:27-118`; `docs/perf/orchestration-log.md` (codex cadence); `docs/perf/codex-reviews/`; `AGENTS.md`; `docs/plan/ci-hygiene.md`; `crates/nmp-core/src/publish/mod.rs:1-40` (canonical doctrine-map comment).
- deliver: checklist as markdown checkbox list; one paragraph per D0–D10 of red-flag patterns; "doctrine map comment" template; "when in doubt, file an ADR" footer.
- anti: ticking boxes mechanically; skipping D8 because "perf is fine in dev"; silent doctrine waivers; "future PR will fix it" carve-outs; PRs that grow `nmp-core` to make app X work.
- xref: 03, 05, 18, 27.

### 23 — Glossary  *(SHIPS · both · 800)*
- covers: definitions for actor · AccountManager · AppState · AppAction · AppUpdate · capability · claim · CompiledPlan · descriptor · DomainModule · EventStore · FfiApp · IdentityScopeKind · InsertOutcome · InterestRegistry · kernel · LogicalInterest · MailboxCache · ModuleRegistry · plan-id · PublishEngine · provenance · `relay_pin` · RelayAck · RelayRole · rev · scope · snapshot · substrate · SyncStrategy · TombstoneRow · VerifiedEvent · ViewModule · ViewPayload · ViewSpec · watermark · WireFrame.
- cite: substrate trait files; planner modules; `crates/nmp-core/src/store/types/mod.rs`; `crates/nmp-nip77/src/coverage_gate.rs`; `docs/product-spec/api-surface.md`; `docs/design/subscription-compilation/intro.md`.
- deliver: alphabetized list with 1–3 sentences and `defined in:` link per term.
- anti: inventing terms not present in the codebase; re-defining a term differently per section; conflating ViewSpec (input) with ViewPayload (output); using "session" and "account" interchangeably; conflating M8-subs and M8-multi-account.
- xref: linked from every section's first use of each term.

### 24 — Reference cards  *(SHIPS · both · 800)*
- covers: today's KernelAction variants; today's KernelUpdate variants; today's KernelViewSpec variants; the 5 trait families one-liner each; v1 capability catalog per spec §6.5; the 11 doctrines D0–D10 one-liners; the 4-stage planner pipeline; the 9 merge-lattice rules; SyncStrategy decision matrix.
- cite: `crates/nmp-core/src/app.rs:1-30`; `crates/nmp-core/src/substrate/*.rs`; `crates/nmp-core/src/planner/mod.rs`; `crates/nmp-nip77/src/coverage_gate.rs`; `docs/product-spec/api-surface.md:193-228`; `docs/product-spec/doctrine.md:1-98`.
- deliver: 6 single-page tables suitable for bookmarking.
- anti: linking outdated variant lists; conflating long-term catalog with what ships today; aspirational entries without a status marker.
- xref: 03, 05, 16.

### 25 — Migration — NDK / Applesauce → NMP  *(SHIPS · builders · 2000)*
- covers: mental-model translations: NDK relay-set / Applesauce relay-map → CompiledPlan; NDK `subscribe(filter)` → `OpenView(spec)` + `LogicalInterest`; Applesauce `EventModel(...)` → `ViewModule`; Applesauce action runner → `ActionModule` + publish engine; NDK signers → `nmp-signers::Signer` + KeyringCapability; NDK sessions store → kernel + `AccountManager`; kind:3 auto-tracking is framework-magic (NDK requires app/Svelte runes per kind3-auto-tracking.md research).
- cite: `docs/design/ndk-applesauce-lessons.md`; `docs/research/sessions/synthesis.md`; `docs/research/ndk/{outbox,signers,kind3-auto-tracking,subscription-compilation,wot-and-sessions,gotchas,missing-features-for-nmp,other-packages}.md`; `docs/research/applesauce/{event-store-query-builders,signers,outbox,gotchas,missing-features-for-nmp}.md`.
- deliver: 3-column translation table (NDK term · Applesauce term · NMP term); "things NMP does for you" list; "things you must not do" list.
- anti: 1:1 porting NDK code; reading Applesauce `model()` as identical to ViewModule (Applesauce is RxJS, NMP is actor-owned); importing NDK relay-policy patterns; reinventing Applesauce's `claimLatest` in app code; expecting JS event-stream ergonomics across FFI.
- xref: 01, 02, 07, 10, 11.

### 26 — FAQ + troubleshooting  *(SHIPS · builders · 1500)*
- covers: common build errors (Cargo workspace mismatches, codegen drift, sim toolchain, `--features lmdb-backend`); runtime issues (no events → relay-status check; snapshot stale → rev guard / emit pacing; subscription leak → claim/release); reading `RelayStatus` / `LogicalInterestStatus` / `WireSubscriptionStatus` in the JSON snapshot; how to enable + read `DebugDiagnostics`; logs.
- cite: `crates/nmp-core/src/kernel/status.rs`; `crates/nmp-core/src/kernel/types.rs`; `crates/nmp-core/src/kernel/mod.rs`; `ios/Chirp/Chirp/Bridge/KernelBridge.swift` (decode path); `docs/perf/ios-demo/`; `docs/product-spec/subsystems.md:323-336` (guardrails); `docs/perf/m10.5/`.
- deliver: Q&A list (~15 items); "debug a missing snapshot in 3 steps" flow; "debug a non-connecting relay in 3 steps" flow; the JSON-snapshot top-level field reference.
- anti: blaming relays for stale `rev` guards; debugging in Swift instead of inspecting JSON snapshot first; editing generated code to fix symptoms; disabling the rev guard.
- xref: 17, 18, 27.

### 27 — Doc/code discrepancies (orchestrator queue)  *(SHIPS · agents · 800)*
- covers: a running list of places where docs claim more than code delivers today: e.g. spec talks about full UniFFI `AppUpdate::ViewBatch` but the live FFI emits a single JSON snapshot; `nmp init` CLI is M16 (not built); UniFFI is M14 (not built); non-Chirp app proofs are deferred; M9 DMs + M12 Wallet are post-v1 deferrals; the actor still uses constants `relay.primal.net` + `purplepag.es` even though the planner can route to mailboxes (wiring gap). Each entry: claim · evidence · status · owning milestone · severity.
- cite: `docs/product-spec/api-surface.md` §6.1, §6.4 (claimed shape); `crates/nmp-core/src/ffi.rs:44-310` (actual surface today); `crates/nmp-core/src/app.rs:1-30` (actual KernelUpdate); `crates/nmp-core/src/relay.rs:1-45` (hardcoded relays); `docs/plan/m16-cli-starter.md`; `docs/plan/m14-uniffi.md`; `docs/plan/m15-cross-platform.md`; `docs/plan/post-v1.md`; `docs/plan/scope-adjustments-2026-05-18.md`.
- deliver: 5-col table (claim · evidence · status · owner-milestone · severity).
- anti: treating every discrepancy as a bug (most are "milestone not landed yet" or deliberate scope deferral); silently changing spec to match incomplete code; silently expanding code beyond milestone scope.
- xref: every section.

## Coverage audit

| Doctrine | Sections | | Trait family | Sections |
|---|---|---|---|---|
| D0 kernel/extension boundary | 02, 03, 05, 19, 20, 22 | | DomainModule | 05, 09, 19, 20 |
| D1 best-effort rendering | 03, 08, 17, 21 | | ViewModule | 05, 06, 07, 19, 20 |
| D2 negentropy first | 03, 13, 21 | | ActionModule | 05, 12, 19, 20 |
| D3 outbox automatic | 03, 10, 21 | | CapabilityModule | 05, 16, 17 |
| D4 single writer per fact | 03, 08, 12, 14, 22 | | IdentityModule | 05, 11 |
| D5 snapshots bounded | 03, 06, 17 | | | |
| D6 no errors across FFI | 03, 12, 13, 16, 17 | | | |
| D7 capabilities report | 03, 12, 14, 16, 17 | | | |
| D8 reactivity contract | 03, 06, 14, 18 | | | |

Shipping crates → sections: `nmp-core` (substrate/planner/store/subs/publish) → 02, 04–12, 14, 22; `nmp-signers` → 11; `nmp-nip29` (reference protocol module) → 20; `nmp-nip42` → 14; `nmp-nip77` → 13; `nmp-codegen` → 15; `nmp-testing` → 18; `fixture-todo-core` → 05, 19; `apps/chirp/*` → 02, 15, 17, 19; `ios/Chirp` → 17.

Milestone status → sections: ✅ M0 → 02,05,15,18,19; ✅ M1 → 17,26; ✅ M2 → 07,10,21; ✅ M3 → 08,09; ✅ M4 → 13; ✅ M5 → 14; ✅ M6 → 11,12; ✅ M7 (publishing + interaction loop) → 05,12,21; ✅ M8 (multi-account + M8-subs lifecycle) → 11,14; ✅ M10.5 → 15,17,18; deferred non-Chirp app proofs → 02,16,19,27; ⌛ M10 Blossom → 27; ⌛ M13 WoT → 27; ⌛ M14 UniFFI → 15,27; ⌛ M15 cross-platform → 15,17,27; ⌛ M16 CLI/starter → 19,26,27; ⌛ M17 release → 22,27; ❌ M9 DMs + M12 Wallet (post-v1) → 01,27.

## Writer-agent process

1. Read this PLAN + your section's brief + cited prereqs (briefs or drafted sections) before drafting.
2. Honor the `status` field strictly. SHIPS may cite `crates/`/`apps/`/`ios/` `path:line`; LANDED cites design + ADRs + partial code; PLANNED cites plan files + ADRs only.
3. Verify every `path:line` you cite by reading the file at the current master tip.
4. Keep your file ≤ 300 LOC. If exceeded, split per the brief's hint (e.g. 05a/05b) and update the TOC in a follow-up.
5. Include ≥3 anti-patterns and ≥2 concrete deliverables from the brief.
6. End every section with `See also:` listing `xref` exactly as `[NN — title](NN-name.md)`.
7. Distill — do not duplicate spec/design content. Your value-add is audience translation, not restatement.
8. If you encounter a cite that does not exist or has shifted lines, fix the cite in place and add a row to [27] noting the drift; do not silently change the brief.
