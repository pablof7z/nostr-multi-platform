# NMP — Nostr Multi-Platform

> A Rust multiplatform framework for building Nostr apps. One Rust core + thin platform shells (iOS / Android / desktop / web). Goal: make it nearly impossible to ship a broken Nostr app — invariants enforced by the type system, actor ownership, and FFI surface rather than docs.

**Live status snapshot — see `docs/perf/orchestration-log.md` for per-heartbeat detail and `docs/perf/pending-user-decisions.md` for autonomous-mode decisions awaiting review.**

---

## TL;DR — where we are

> Live milestone status. `cargo test --workspace` = **106 passing**. See `docs/plan.md` for the full ladder, `docs/perf/codex-reviews/` for the per-merge doctrine trail, `docs/perf/orchestration-log.md` for the heartbeat trail.

| Milestone | Status | What's on master |
|---|---|---|
| **M0** kernel substrate + non-Nostr fixture | ✅ **DONE** | 5 substrate trait families, `nmp-codegen`, `fixture-todo-core`, `nmp-testing` harnesses, reactivity-bench gates green |
| **M1** read-only Twitter slice on live iOS | ✅ **DONE** *(commit `701d0e5`)* | live firehose-bench cold_start = 258 ms first-item / 3 587 ms filled-timeline; profile_thrashing 0 leaked subs over 10 min; all M1 gates PASS against `wss://relay.primal.net` + `wss://purplepag.es`. Memory RSS gate → folded into M10.5; dispatch-rate platform debounce gate → folded into M14. |
| **M2** subscription compilation + outbox + NIP-65 | 🟡 **impl phase 1 + 5 codex rounds on master** *(latest `942034d`)* | `crates/nmp-core/src/planner/`: `interest.rs`, `lattice/*` (per-rule split, 8 merge rules incl. Rule 8 address-pointer union), `compiler/{mod,mailbox,plan_id,partition/{mod,case_a_authors,case_b_addresses,case_c_p_tags,case_d_no_author,inbox_helper}}`, `plan.rs`. Integration tests on master: `m2_subscription_compilation_audit.rs`, `m2_plan_id_stability.rs`, `m2_p_tag_inbox_routing.rs`. Phase-2 `requests/*` migration to compiler still deferred. |
| **M3** LMDB + insert invariants + claim GC | 🟡 **impl phase 1 + 3 codex rounds on master** *(latest `ed241d2`)* | `crates/nmp-core/src/store/` with `EventStore` trait + `MemEventStore` (split into `mem/{mod,insert,query,gc,domain,store_impl,tests}`) + `LmdbEventStore` skeleton + `types/` split. `VerifiedEvent` newtype + `nostr` crate sig verification. ADR-0011 LMDB env sharing. D4 outcome gating for kind:0/3/10002 local caches. Per-view + global GC ceilings (1000/20000), BTreeSet intra-call idempotency. 10 integration test files (`store_*.rs`). **T38 next:** split `kernel/mod.rs` 509 + `kernel/ingest.rs` 541 (HARD-cap breaches) + empty-relay-list edge. |
| **M4-M8** negentropy, NIP-42, signers, interaction loop, multi-session | ⏸ scoped, blocked on M2/M3 stabilising | per-milestone designs in `docs/plan/m{4,5,6,7,8}.md` |
| **M9 (DMs)** | ❌ **deferred to post-v1** | `docs/plan/post-v1.md`; structural ban (no DMs to non-inbox relays) preserved in M2 outbox planner |
| **M10** Blossom + long-running capabilities | ⏸ scoped | `docs/plan/m10-blossom.md` |
| **M10.5** FFI hardening + iOS empirical proof — **HARD GATE BEFORE M11** | 🟡 **impl phase 1 + 3 codex rounds on master** *(latest `0124726`)* | `crates/nmp-testing/bin/ffi-stress/`: `main`, `common`, `gate`, `report`, `allocator` + S1–S5 scenarios (`s1_mount_unmount`, `s2_dispatch_flood`, `s3_snapshot_pressure`, `s4_reconciler_backpressure`, `s5_reentrancy`). S5 epoch watchdog correct; S2 dispatch-flood verified 100k @ p99 0.020 ms; S4 configure-during-stall p99 ~7 µs. **T35 in flight:** real `VerifiedEvent` injection (S3/S4/S5 still use synthetic shortcut), full G-S1/G-S2/G-S4 spec gates, full G-S4 set (queue depth + stale-rev + emit drops + apply burst). iPhone-12 device runs + iOS XCUITest + Sonnet-agent UI fleet remain. |
| **M11** ../podcast rebuild on NMP | ⏸ **design landed**, blocked on M10.5 empirical PASS | 13 sub-docs in `docs/design/podcast{,/}.md`; 4 codex iterations clean; copy-first UI fidelity vs `/Users/pablofernandez/src/podcast` (8.8 k LOC Swift, 20 views); per-screen → ViewModule map; rig.rs LLM; podcast-rag; podcast-feeds; pixel-parity screenshot gate via XCUITest. |
| **M11.5** Highlighter + `nmp-nip29` rebuild | ⏸ **design landed**, blocked on M11 demo | 7 docs at `docs/design/nip29{,/}.md` + `docs/research/highlighter/*` + `docs/plan/m11.5-highlighter.md`; 14 codex iterations to convergence; `nmp-nip29` ships 13 DomainModules + 7 ViewModules + 15 ActionModules; ADR-0012 RelayPinnedInterest + ADR-0013 metadata-signer trust proposed. |
| **M12 (Wallet)** | ❌ **deferred to post-v1** | `docs/plan/post-v1.md` |
| **M13** WoT, **M14** UniFFI migration, **M15** cross-platform, **M16** CLI + starter + recipes, **M17** v1 release | ⏸ scoped | scoped per `docs/plan/m{13,14,15,16,17}-*.md`. M14 carries M1's dispatch-rate gate deferral. |
| **Framework-magic contract** (kind:3 auto-tracking, `bunker://` onboarding, new-nsec creation, outbox-by-default-on-publish, …) | 🟡 **design landed, tests planned** | [`docs/design/framework-magic.md`](docs/design/framework-magic.md) — 13 named behaviors, codex-reviewed aligned with canonical D0–D8. The companion `crates/nmp-testing/tests/framework_magic_contract.rs` is **not yet checked in** — lands alongside the first behaviors' implementation (M2 owns C5–C8, M3 owns C1–C4, M6 owns C11, M8 owns C12). |
| **Research foundation** | ✅ landed | NDK at [`docs/research/ndk/`](docs/research/ndk/) (outbox / kind:3 auto-tracking / subscription compilation / signers / gotchas / missing-features / meta-subscribe / wot-and-sessions / other-packages). Applesauce at [`docs/research/applesauce/`](docs/research/applesauce/) (event-store query builders / outbox / signers / gotchas / missing-features). Highlighter at [`docs/research/highlighter/`](docs/research/highlighter/). |
| **Scope shifts** | ✅ logged | DMs (was M9) and Wallet (was M12) → post-v1; M11.5 Highlighter takes their slot. See [`docs/plan/scope-adjustments-2026-05-18.md`](docs/plan/scope-adjustments-2026-05-18.md). |

**Active tasks** (pending + in-flight at master = `ac453e0`): T35 in flight (M10.5 round-3.5 — VerifiedEvent injection + full spec gates), T38 pending (M3 round-4 — HARD-cap splits of kernel/{mod,ingest}.rs + empty-relay-list edge).

**ADRs accepted so far:** ADR-0001..0010 (per `docs/decisions/`) + ADR-0011 LMDB env sharing.

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
| `docs/product-spec.md` + `docs/product-spec/` | What we ship at v1. The cardinal doctrine D0–D8 lives in §1.5. |
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
2. `docs/product-spec.md` §1.5 — the doctrine D0–D8.
3. `docs/plan.md` — the milestone ladder.
4. `docs/decisions/0009-app-extension-kernel-boundary.md` — why the kernel is a kernel.
5. `docs/decisions/0010-generated-app-enum-vs-type-erased-registry.md` — how FFI types are generated.
6. `docs/design/framework-magic.md` — what "just works" without app code.

---

*This file is regenerated on every heartbeat from the live state of the ladder + scope memo + last-commit summary. Manual edits between heartbeats are fine but will be folded back in by the next refresh.*
