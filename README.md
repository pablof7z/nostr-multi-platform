# NMP — Nostr Multi-Platform

> A Rust multiplatform framework for building Nostr apps. One Rust core + thin platform shells (iOS / Android / desktop / web). Goal: make it nearly impossible to ship a broken Nostr app — invariants enforced by the type system, actor ownership, and FFI surface rather than docs.

**Live status snapshot — see `docs/perf/orchestration-log.md` for per-heartbeat detail and `docs/perf/pending-user-decisions.md` for autonomous-mode decisions awaiting review.**

---

## TL;DR — where we are

> Full per-milestone status in [`docs/plan/status.md`](docs/plan/status.md); per-milestone scope in `docs/plan/m*.md`.

- **M0 (kernel substrate + non-Nostr fixture):** ✅ DONE.
- **M1 (read-only Twitter slice on live iOS):** 🟡 hardening (firehose-bench `live` mode + iPhone-12 baseline in flight via `m1-hardener`).
- **M2 (subscription compilation + outbox + NIP-65):** design landed + codex-reviewed; 10 issues being addressed in T13.
- **M3 (LMDB + insert invariants + claim GC):** design landed + codex-reviewed; 12 issues pending fix in T14.
- **M4–M8 (negentropy, NIP-42, signers+write, interaction loop, multi-session):** scoped, pending design after M2/M3 lock.
- **M10 (Blossom + long-running capabilities):** pending.
- **M10.5 (FFI hardening + iOS empirical proof):** design landed; **hard gate before M11**.
- **M11 (rebuild of `/Users/pablofernandez/src/podcast` on NMP):** design landed (13 sub-docs, codex-reviewed clean after 5 iterations); awaits M10.5 empirical pass.
- **M11.5 (rebuild of `/Users/pablofernandez/Work/hl/app` Highlighter + `nmp-nip29` crate):** design in flight (T18).
- **M13 (WoT), M14 (UniFFI), M15 (cross-platform), M16 (CLI), M17 (v1 release):** scoped, pending.
- **Framework-magic contract** (kind:3 auto-tracking, `bunker://` onboarding, new-nsec creation, outbox-by-default-on-publish, etc.): designed at [`docs/design/framework-magic.md`](docs/design/framework-magic.md) with 13 behaviors and 14 named tests in `crates/nmp-testing/tests/framework_magic_contract.rs`. Reconciliation pass T19 in flight to align with canonical D0–D5.
- **Plan structure:** `docs/plan.md` is the index; per-milestone files under `docs/plan/m*.md` (≤300 LOC each). Scope shifts captured in [`docs/plan/scope-adjustments-2026-05-18.md`](docs/plan/scope-adjustments-2026-05-18.md) — DMs (was M9) and Wallet (was M12) deferred to post-v1 (see [`docs/plan/post-v1.md`](docs/plan/post-v1.md)).
- **Research foundation:** NDK + Applesauce deep-dives at [`docs/research/ndk/`](docs/research/ndk/) and [`docs/research/applesauce/`](docs/research/applesauce/) — outbox routing, kind:3 auto-tracking gap, subscription compilation, signers, gotchas, missing features.

## High-level decisions

- **Cardinal doctrine (D0–D8).** Canonical wording from [`docs/product-spec/overview-and-dx.md` §1.5](docs/product-spec/overview-and-dx.md). Every PR is reviewed against this rubric. A change that makes any doctrine harder to enforce is rewritten or rejected. Conflicts resolve in the order listed. **Two kinds:** D0–D5 are *policy* doctrines (user-facing semantics); D6–D8 are *substrate invariants* (runtime / FFI / hot-path constraints). Both equally binding.
  - **D0** Kernel + extension modules — no app nouns in `nmp-core`.
  - **D1** Best-effort rendering — render now, refine in place. Placeholders are part of the type contract.
  - **D2** Negentropy first, REQ second. NIP-77 reconciliation with durable watermarks is the default backfill. (M4.)
  - **D3** Outbox routing is automatic; manual relay selection is the opt-out. (M2.)
  - **D4** Single writer per fact; caches derive. Cache invalidation is not a concept in the public API.
  - **D5** Snapshots bounded by what's open. `AppState` carries the projection through currently-open views.
  - **D6** Errors never cross FFI as exceptions. Surface as `toast: Option<String>` state fields. (RMP bible invariant #2.)
  - **D7** Capabilities report; never decide policy. Native bridges execute platform APIs; Rust decides retry/recovery/routing. (RMP bible rule #6.)
  - **D8** Reactivity contract: composite reverse index · ≤60 Hz/view · working-set bounded · zero per-event allocations after warmup. Validated by `reactivity-bench`. (ADR-0001..0004.)
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
