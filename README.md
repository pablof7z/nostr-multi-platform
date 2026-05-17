# NMP — Nostr Multi-Platform

> A Rust multiplatform framework for building Nostr apps. One Rust core + thin platform shells (iOS / Android / desktop / web). Goal: make it nearly impossible to ship a broken Nostr app — invariants enforced by the type system, actor ownership, and FFI surface rather than docs.

**Live status snapshot — see `docs/perf/orchestration-log.md` for per-heartbeat detail and `docs/perf/pending-user-decisions.md` for autonomous-mode decisions awaiting review.**

---

## TL;DR — where we are

- **M0 (kernel substrate + non-Nostr fixture):** ✅ DONE.
- **M1 (read-only Twitter slice on live iOS):** 🟡 hardening (firehose-bench `live` mode + iPhone-12 baseline in flight).
- **M2 (subscription compilation + outbox routing + NIP-65):** design landed; 8 codex-flagged issues being addressed (T13).
- **M3 (LMDB persistence + insert invariants + claim GC):** design landed; 10 codex-flagged issues being addressed (T14).
- **M4 (NIP-77 negentropy sync), M5 (NIP-42 auth), M6 (sessions + signers + write), M7 (interaction loop), M8 (multi-session):** designs pending after M2/M3 hardens.
- **M10 (Blossom + long-running capabilities):** pending.
- **M10.5 (FFI hardening + iOS empirical proof):** design landed; **hard gate before M11**.
- **M11 (rebuild of `/Users/pablofernandez/src/podcast` on NMP):** design landed; pixel-parity UI copy + Rust-backed business logic + `rig.rs` LLM + RAG + podcast-feeds; awaits M10.5 empirical pass before impl starts.
- **M11.5 (rebuild of `/Users/pablofernandez/Work/hl/app` Highlighter on NMP + `nmp-nip29` crate):** designed in flight (T18).
- **M13 (Web-of-Trust), M14 (UniFFI migration), M15 (Android+Desktop+Web), M16 (CLI+starter+recipes), M17 (v1 release):** pending.
- **Framework-magic contract** (kind:3 auto-tracking, `bunker://` URL onboarding, new-nsec creation, outbox-by-default-on-publish, etc.): designed ([docs/design/framework-magic.md](docs/design/framework-magic.md)) with 13 behaviors and 14 named tests in `crates/nmp-testing/tests/framework_magic_contract.rs`.

## High-level decisions

- **Cardinal doctrine (D0–D5).** Every PR is reviewed against this rubric. A change that makes any doctrine harder to enforce is rewritten or rejected.
  - **D0** Kernel never grows app nouns (proven by the M11 podcast rebuild and the existing fixture-todo-core)
  - **D1** Best-effort rendering: placeholders → in-place refinement, never withhold known data
  - **D2** Reactivity contract: composite reverse index · ≤60 Hz/view · working-set bounded
  - **D3** Errors never cross FFI — become `toast: Option<String>` state fields
  - **D4** One writer per fact
  - **D5** Capabilities report, never decide policy
- **Architecture (RMP bible, non-negotiable).** Elm-style (`AppState` + `KernelAction` + `handle_message`) on a single actor thread. `dispatch()` is fire-and-forget. Monotonic `rev: u64`. Snapshot semantics by default; granular updates as optimization. See `docs/aim.md` for the full distillation.
- **App-extension kernel boundary** (ADR-0009). NMP is a kernel + five extension trait families (`DomainModule`, `ViewModule`, `ActionModule`, `CapabilityModule`, `IdentityModule`). Per-app concrete enums generated at the FFI boundary via `nmp gen modules` (ADR-0010). Apps assemble themselves from `nmp-core` + protocol modules + their own `<app>-core` crate.
- **DMs (was M9) and Wallet (was M12) deferred to post-v1.** Documented in `docs/plan/scope-adjustments-2026-05-18.md`. M11.5 Highlighter takes their slot.
- **Outbox routing is automatic by default** — `nmp-core::planner` resolves author write relays + recipient inbox relays on every publish; explicit override is an audited opt-out. (Stronger than NDK's caller-responsibility model and Applesauce's caller-responsibility model.)
- **Kind:3 auto-tracking is framework-magic** — when the active account's follow list changes, every open subscription that depends on "current user's follows" auto-recompiles on the wire. Apps dispatch zero code. The Applesauce/NDK research (`docs/research/`) confirmed core NDK does NOT provide this automatically — NMP must build it as framework code.
- **Post-merge codex review.** After every push to master, `codex exec` reviews the diff against the doctrine + file-size rules + spec coherence. Codex output saved to `docs/perf/codex-reviews/<sha>.md`. Any real concerns become fix-it TaskList entries.
- **Empirical iOS proof before podcast rebuild (M10.5 gate).** No "we'll wire it up later" — the FFI surface ships rock-solid (stress harness + simulator-driven Sonnet-agent UI fleet + Instruments leaks audit + iPhone-12 baseline) before any line of M11 code is written.

## How it works — high-level

```
   ┌──────────────────────────────────────────────────────┐
   │  iOS / Android / Desktop / Web shells                 │  thin: rendering only
   │  SwiftUI / Compose / iced / wasm+TS                   │
   └────────────────────────────┬─────────────────────────┘
                                │ dispatch(Action) — fire-and-forget
                                │ reconcile(Update) — callback into platform
   ┌────────────────────────────▼─────────────────────────┐
   │  Generated per-app FFI crate (nmp-app-<name>)         │  UniFFI / wasm-bindgen
   │  AppAction / AppUpdate / ViewSpec composed from       │  produced by `nmp gen modules`
   │  kernel + chosen protocol modules + app-core          │
   └────────────────────────────┬─────────────────────────┘
   ┌────────────────────────────▼─────────────────────────┐
   │  nmp-core kernel                                      │  one actor thread; no business
   │  • Substrate: 5 trait families                        │  logic; pure dispatch +
   │  • Composite reverse index + delta buffer + GC        │  reactivity machinery
   │  • Subscription planner (compiler stage)              │
   │  • Outbox routing (NIP-65) — automatic by default     │
   │  • EventStore (LMDB / IndexedDB / in-memory)          │
   └─┬───────────┬───────────┬──────────────┬─────────────┘
     │           │           │              │
   ┌─▼─┐      ┌──▼──┐    ┌───▼────┐    ┌────▼─────┐
   │NIP│      │NIP  │    │NIP-29  │    │  app     │   protocol + app crates
   │ 01│      │ 65  │    │groups  │    │  cores   │   = "extension modules"
   │ 02│      │ 77  │    │…       │    │ (twitter,│   compiled into the per-app
   │ 10│      │ 42  │    │…       │    │  podcast,│   FFI crate, never into
   │ 25│      │     │    │        │    │  hl, …)  │   nmp-core
   └───┘      └─────┘    └────────┘    └──────────┘
```

**Single source of truth, four delivery paths.** The kernel is compiled as `cdylib + staticlib + rlib`. Desktop and CLI consumers link the rlib directly (no FFI). iOS links the staticlib via xcframework. Android links the cdylib via cargo-ndk. Web compiles to wasm32-unknown-unknown via the wasm crate.

## Execution mode

- **Parallel-agent orchestration.** Multiple specialized subagents work in isolated git worktrees in parallel. Each commits + pushes via `fetch → rebase → push` (never force-push). The orchestrator dispatches, the heartbeat ticks every 15 minutes (`docs/perf/orchestration-log.md` is the durable trail).
- **Codex review on every merge.** `codex exec` is the post-merge doctrine reviewer. Saved transcripts in `docs/perf/codex-reviews/`.
- **No silent endings.** Every milestone exit produces: regression tests in `crates/nmp-testing/`, a perf report in `docs/perf/m<N>/`, an ADR if design was revised, and a runnable artifact tagged in git.
- **Autonomous mode.** When the user is asleep / unavailable the orchestrator decides + logs in `docs/perf/pending-user-decisions.md`; never blocks waiting.

## Documentation map

| Where | What |
|---|---|
| `docs/aim.md` | Project north star + RMP-bible distillation. **Read first.** |
| `docs/plan.md` + `docs/plan/` | Milestone ladder (M0–M17) with exit gates per milestone. |
| `docs/plan/scope-adjustments-2026-05-18.md` | Live scope shifts (DMs + Wallet deferred; Highlighter added; framework-magic contract). |
| `docs/product-spec.md` + `docs/product-spec/` | What we ship at v1. The cardinal doctrine D0–D5 lives in §1.5. |
| `docs/decisions/` | ADR-0001..0010 (and counting). |
| `docs/design/` | Per-subsystem design docs — subscription compilation, LMDB schema, FFI hardening, podcast rebuild, framework-magic, NIP-29 crate. |
| `docs/research/` | Reverse-engineering notes on NDK + Applesauce — outbox, kind:3 auto-tracking, signers, gotchas, missing-features deltas. |
| `docs/perf/` | Empirical measurements + heartbeats + codex reviews + debt inventories. |
| `crates/` | `nmp-core` (substrate), `nmp-codegen` (per-app FFI crate generator), `nmp-testing` (mock relay, harnesses, scenarios), `fixture-todo-core` (non-Nostr extension-module proof). |
| `apps/` | Generated per-app crates (`apps/fixture/nmp-app-fixture`, future `apps/twitter/nmp-app-twitter`, `apps/podcast/nmp-app-podcast`, `apps/hl/nmp-app-hl`). |
| `ios/NmpStress` | iOS SwiftUI shell wired to the Rust kernel via raw C FFI (will migrate to UniFFI in M14). |
| `AGENTS.md` | Rules: file-size limit 300 LOC soft / 500 hard. |

## Worth reading before contributing

1. `docs/aim.md` — the north star.
2. `docs/product-spec.md` §1.5 — the doctrine D0–D5.
3. `docs/plan.md` — the milestone ladder.
4. `docs/decisions/0009-app-extension-kernel-boundary.md` — why the kernel is a kernel.
5. `docs/decisions/0010-generated-app-enum-vs-type-erased-registry.md` — how FFI types are generated.
6. `docs/design/framework-magic.md` — what "just works" without app code.

---

*This file is regenerated on every heartbeat from the live state of the ladder + scope memo + last-commit summary. Manual edits between heartbeats are fine but will be folded back in by the next refresh.*
