# NMP — Nostr Multi-Platform

> A Rust multiplatform framework for building Nostr apps. One Rust core + thin platform shells (iOS / Android / desktop / web). Goal: make it nearly impossible to ship a broken Nostr app — invariants enforced by the type system, actor ownership, and FFI surface rather than docs.

**Live status snapshot — see `docs/perf/execution-assessment-2026-05-21.md` for the latest repo-grounded assessment, `docs/perf/orchestration-log.md` for historical heartbeat detail, and `docs/perf/pending-user-decisions.md` for autonomous-mode decisions awaiting review.**

---

## TL;DR — where we are

> Full per-milestone status in [`docs/plan/status.md`](docs/plan/status.md); per-milestone scope in `docs/plan/m*.md`.

- **M0 (kernel substrate + non-Nostr fixture):** ✅ DONE.
- **M1 (Chirp social baseline on live iOS):** ✅ DONE (`701d0e5` — firehose-bench `live` cold_start 258ms first-item / 3587ms filled-timeline; profile_thrashing 0 leaked subs). Chirp's broader goal is the full NMP showcase: every reusable feature NMP ships should become visible, testable, and debuggable there.
- **M2 (subscription compilation + outbox + NIP-65):** ✅ planner + 5-case partition + Rule 9 group-id merge on master. 5 rounds of codex follow-up landed. Address-pointer support. **T105 keystone wired CompiledPlan to the live REQ + publish path** (5-commit chain `167d4bc..fada22b`); T132 reconciled dual cache seams (`f7ea534`); T129 addSinceFromCache (`0e32024`); T121 thread hydration outbox (`17d164a`); T122 firehose hashtag inbox; T104 typed `OneshotKind` retires `oneshot-disc-` string-prefix routing (`fd9e92e`); T100b kind:3 re-fan timeline (`91e948b`). Seven-lane routing model per joint ADR-0020+ADR-0021 (`4ce7ba6`). **T142 landed (`5deb4d4`): `drain_tick()` now driven from the actor idle loop — the M2 planner is on the live path (empty-registry no-op safe).** **T140 keystone CUTOVER COMPLETE (`abf23c52`):** the original T140 landed but a codex post-merge review (verdict REVERT) + orchestrator verification proved it had only *added* M2 alongside a still-live M1 `seed-timeline-*` path (duplicate wire REQs). The **T140-FF fix-forward** actually retired M1 — `maybe_open_timeline()` no longer emits the follow-feed REQ (seed feed covered by `startup_requests`), M2 follow-feed subs are EOSE-keep-live via `InterestLifecycle::Tailing`, `timeline_authors` is single-sourced from the M2 projection, `drain_tick()` is D6-clean (`last_planner_error()`), empty-follows clears stale interests. Proven by a negative-existence gate test (`live_follow_feed_path_emits_no_seed_timeline_req`). Residuals R1 (logout→None still leaks; #168) + R2 (`subs/mod.rs` 1453 LOC split; #169) filed.
- **M3 (LMDB + insert invariants + claim GC):** ✅ **REAL on master** (`77ac7e0` T136b — `LmdbEventStore` 33-method trait + Mem-parity behind `--features lmdb-backend`; ADR-0012 `480f3b1` documents `MemEventStore`-canonical write-path policy per D4). EventStore trait + MemEventStore (sole writer per D4) + verify_and_persist outcome gating for kind:0/3/10002. Local `nmp-nostr-lmdb` fork (`4e8ca2d`) carries the env-injection seam upstream lacked. 1234 workspace tests passing.
- **M4 (NIP-77 negentropy):** ✅ LANDED (`076173d` — `crates/nmp-nip77/` reconciler + wire + capability + capability_domain).
- **M5 (NIP-42 relay auth):** ✅ LANDED (`e69c3a4` — `crates/nmp-nip42/` state + parsers + builder + driver; T58 kernel wiring — `kernel/auth.rs` + `ingest/auth_handlers.rs` — inlines the FSM because NIP-42 is wire-layer like NIP-01 framing, not an app noun; 7 integration tests in `kernel/auth_tests.rs` pin AUTH-required-for-read, fail-surface, replay-on-auth, retry-on-auth-required, D8 no-rev-bump invariant, no-signer-bound hold path, and view-open REQ partition).
- **M6 (sessions + signers):** ✅ LANDED (`9944bed` — `crates/nmp-signers/` with Signer trait + Local/Nip46/Nip07 + AccountManager + kind:3 auto-rewire). ADR-0015 captures design. Sessions research synthesis at `docs/research/sessions/synthesis.md` informed the design (AccountPublic/AccountSecret split, signer-mismatch post-conditions, bunker:// strict-hex parser).
- **M7 (publishing pipeline):** ✅ LANDED (`08fc01f` — Publish action + per-relay state machine + NIP-65 auto-route + durable retry queue + PublishStatusView). T117 wired PublishEngine FSM to live REQ path (`6711b01`); T127 actor-tick + boot-resume (`2e249a6`); T128 terminal status + per-relay outcomes + iOS lockstep (`1486eed`).
- **M8 (subscription lifecycle / RelayManager):** ✅ LANDED (`9ca90c9` — registry + trigger inbox + wire emitter + connection pool + auth gate).
- **M10 (Blossom + long-running capabilities):** pending.
- **M10.5 (FFI hardening + iOS empirical proof):** ✅ **§G-S2 gate CLOSED** (`83430ca` T114b — all 7 numeric gates green; retained heap 0.15–0.52 MiB ≤ 1 MiB ceiling; 250× reduction from 38 MiB pre-fix). S2 6/6 + S3 6/6 + S4 6/6 + S5 5/5 PASS with VerifiedEvent injection. **Five computed-but-not-on-wire gaps all CLOSED** (T105 outbox keystone, T117 publish engine FSM, T118 iOS scenePhase, T119 NIP-46 bunker — empirically proven `1f6ae64`, T116 reconnect-replay). Historical full-workspace result: **1234 passed / 0 failed / 17 ignored** on master (post-T136b LMDB + PD-025 fixes); refresh before using that count as current.
- **Deferred app proofs:** Podcast and Highlighter app surfaces are removed from active scope until Chirp is a polished, complete showcase. Reusable protocol infrastructure that came from those explorations, such as `nmp-nip29`, remains because it is generic Nostr substrate.
- **M13 (WoT), M14 (UniFFI), M15 (cross-platform), M16 (CLI), M17 (v1 release):** scoped, pending.
- **Framework-magic contract** (kind:3 auto-tracking, `bunker://` onboarding, new-nsec creation, outbox-by-default-on-publish): designed at [`docs/design/framework-magic.md`](docs/design/framework-magic.md). Live proof target `cargo test -p nmp-testing --test framework_magic_contract` is **14 passed, 0 failed, 0 ignored** on the current tree.
- **E2E pipeline tests** (`d0b7df6`): 6-scenario `crates/nmp-testing/tests/e2e_full_pipeline.rs` + audit companion forcing test-de-ignoring as milestones land.
- **Sessions research** (`de9e7b4` + `a3bf036`): NDK + applesauce deep-dives + NMP M6 synthesis at `docs/research/sessions/`.
- **Plan structure:** `docs/plan.md` is the index; per-milestone files under `docs/plan/m*.md` (≤300 LOC each). Scope shifts captured in [`docs/plan/scope-adjustments-2026-05-18.md`](docs/plan/scope-adjustments-2026-05-18.md) — DMs (was M9) and Wallet (was M12) deferred to post-v1 (see [`docs/plan/post-v1.md`](docs/plan/post-v1.md)).
- **Research foundation:** NDK + Applesauce deep-dives at [`docs/research/ndk/`](docs/research/ndk/) and [`docs/research/applesauce/`](docs/research/applesauce/) — outbox routing, kind:3 auto-tracking gap, subscription compilation, signers, gotchas, missing features.

## High-level decisions

- **Cardinal doctrine (D0–D10).** Canonical wording from [`docs/product-spec/overview-and-dx.md` §1.5](docs/product-spec/overview-and-dx.md). Every PR is reviewed against this rubric. A change that makes any doctrine harder to enforce is rewritten or rejected. Conflicts resolve in the order listed. **Two kinds:** D0–D5 are *policy* doctrines (user-facing semantics); D6–D10 are *substrate invariants* (runtime / FFI / hot-path, kernel-time, and provenance constraints). Both equally binding.
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
- **NIP-17 DMs actively built as Chirp feature work.** The M9 DM milestone scope (standalone milestone) is deferred, but NIP-17 is being shipped incrementally: kind:10050 publish (`nmp.nip17.publish_relay_list` — PR #151), iOS consumer auto-dispatch (PR #163), `DmInboxProjection` + `DmListView`/`DmConversationView` all wired. Wallet (was M12) remains post-v1. Chirp is the only active product proof.
- **Outbox routing is automatic by default** — `nmp-core::planner` resolves author write relays + recipient inbox relays on every publish; explicit override is an audited opt-out. (Stronger than NDK's caller-responsibility model and Applesauce's caller-responsibility model.)
- **Kind:3 auto-tracking is framework-magic** — when the active account's follow list changes, every open subscription that depends on "current user's follows" auto-recompiles on the wire. Apps dispatch zero code. The Applesauce/NDK research (`docs/research/`) confirmed core NDK does NOT provide this automatically — NMP must build it as framework code.
- **Post-merge codex review.** After every push to master, `codex exec` reviews the diff against the doctrine + file-size rules + spec coherence. Codex output saved to `docs/perf/codex-reviews/<sha>.md`. Any real concerns become fix-it TaskList entries.
- **Empirical proof before additional app rebuilds.** No "we'll wire it up later" — the FFI surface ships rock-solid in Chirp before any additional app proof is reopened.

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
   │ 10│      │ 42  │    │…       │    │  chirp,  │   FFI crate, never into
   │ 25│      │     │    │        │    │  hl, …)  │   nmp-core
   └───┘      └─────┘    └────────┘    └──────────┘
```

**Single source of truth, multiple delivery paths.** The kernel is compiled as `cdylib + staticlib + rlib`. Desktop and CLI consumers link the rlib directly (no FFI). iOS links the staticlib via xcframework. Android links the cdylib via cargo-ndk. Web (wasm32 via wasm-bindgen) is an aspirational target — no `wasm_bindgen` surface exists yet.

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
| `docs/plan/chirp-showcase.md` | Chirp's standing goal as the full-featured NMP reference client. |
| `docs/plan/scope-adjustments-2026-05-18.md` | Historical scope shifts and deferrals. |
| `docs/product-spec.md` + `docs/product-spec/` | What we ship at v1. The cardinal doctrine D0–D10 lives in §1.5. |
| `docs/decisions/` | ADR-0001..0010 (and counting). |
| `docs/design/` | Per-subsystem design docs — subscription compilation, LMDB schema, FFI hardening, framework-magic, NIP-29 crate. |
| `docs/research/` | Reverse-engineering notes on NDK + Applesauce — outbox, kind:3 auto-tracking, signers, gotchas, missing-features deltas. |
| `docs/perf/` | Empirical measurements + heartbeats + codex reviews + debt inventories. |
| `crates/` | `nmp-core` (substrate), `nmp-codegen` (per-app FFI crate generator), `nmp-testing` (mock relay, harnesses, scenarios), `fixture-todo-core` (non-Nostr extension-module proof). |
| `apps/` | Generated per-app crates for active proofs (`apps/fixture/nmp-app-fixture`, `apps/chirp/nmp-app-chirp`). |
| `ios/Chirp` | Production Nostr client and full NMP showcase. Former NmpStress diagnostics and NmpPulse smoke coverage now live here. |
| `AGENTS.md` | Rules: file-size limit 300 LOC soft / 500 hard. |

## Worth reading before contributing

1. `docs/aim.md` — the north star.
2. `docs/product-spec.md` §1.5 — the doctrine D0–D10.
3. `docs/plan.md` — the milestone ladder.
4. `docs/decisions/0009-app-extension-kernel-boundary.md` — why the kernel is a kernel.
5. `docs/decisions/0010-generated-app-enum-vs-type-erased-registry.md` — how FFI types are generated.
6. `docs/design/framework-magic.md` — what "just works" without app code.
7. Enable the file-size pre-commit hook (AGENTS.md 300/500 LOC rule): `git config core.hooksPath .githooks`

---

*This file is regenerated on every heartbeat from the live state of the ladder + scope memo + last-commit summary. Manual edits between heartbeats are fine but will be folded back in by the next refresh.*
