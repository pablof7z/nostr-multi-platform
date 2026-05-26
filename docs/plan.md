# Build & Validation Plan

> Single overarching plan for shipping NMP v1. Reconciled 2026-05-25 against HEAD `cc10148f` (post step 8 phase F — actor cut-over to Pool).
>
> **Sources of truth:**
> - **Architectural north star** — [`docs/aim.md`](aim.md) (immutable; read first on cold-start).
> - **Architectural migration spec** — [`docs/architecture/crate-boundaries.md`](architecture/crate-boundaries.md) (12-step crate-boundary plan; §5 is the migration order).
> - **Live in-flight tracker** — [`WIP.md`](../WIP.md) (work currently on a branch).
> - **Tactical tracker** — [`docs/BACKLOG.md`](BACKLOG.md) (violations, pending user decisions, ordered v1 feature backlog, post-v1 list).
> - **Most recent strategic direction review** — [`docs/perf/codex-reviews/opus-direction-14-review-product-honesty.md`](perf/codex-reviews/opus-direction-14-review-product-honesty.md).
>
> **This file is the overview.** Active items belong in `WIP.md` (in-flight) or `BACKLOG.md` (queue). Update this file only when a milestone changes status, doctrine is amended, or the v1 exit criteria move.

---

## TL;DR — one screen

**Architecture migration (2026-05-24 + 2026-05-25, 31 PRs merged).** The 12-step crate-boundary plan in `docs/architecture/crate-boundaries.md` is structurally **~90% complete**. Steps 1 (substrate seams), 2 (`nmp-router`), 3 (kernel cut-over), 4 (V-41 LNURL), 5+6 (V-39+V-40 NIP-17), 8 phases A/B/C/E/F (extraction, Pool API, BrowserRelayDriver, NIP-42 split, actor cut-over), 9 (`nmp-store`+`nmp-planner`), 10 (`nmp-app-template`; Chirp −547 LOC), 11 partial+final (chirp-* moved, `nmp-ffi` extracted) ✅ merged. Substrate-honest debts A (router decides), B (delete `default_routing.rs`), C (`ProtocolCommandContext` capability traits), D (RwLock panics), V-08 (bunker DM send) ✅ merged. V-51 routing observability phases 1+2+4+5 ✅ merged. Step 7 (V-38 NWC) ⚠ PR #460 sitting deprioritized. Step 8 phase D (broker `Pool` dedupe) 🟡 PR #477 in CI. Remaining: step 8 phase D merge; V-51 phase 3 (iOS Chirp inspector UI — Swift). Step 12 (`nmp-marmot` return to `crates/`) 🟡 PR open — Path B (per-app FFI sanctioned per ADR-0025 update 2026-05-23; bespoke `nmp_marmot_dispatch` write-side already retired in PR 3 2026-05-23).

**Live validation works.** `cargo test -p nmp-testing --test routing_trace_real_nostr -- --ignored` fetches pablof7z's real NIP-65 from `wss://relay.damus.io`, hands it to `nmp_router::GenericOutboxRouter::route_subscription`, asserts the resolved set (`r.f7z.io`, `relay.damus.io`, `relay.primal.net`) is attributed to `Nip65/Read` lane with zero `AppRelay/Fallback` leak. `scripts/validate-routing.sh` drives chirp-repl end-to-end. The kernel **actually consumes** the router's output for live REQ-relay selection (PR #462 + PR #468 cut-over; observe-only → decision authority).

**Known partial state** (honest about what's not yet clean):
- `Nip65OutboxResolver` (279 LOC publish-side NIP-65 algorithm) still sits in `crates/nmp-core/src/publish/nip65/` — spec §271 forbids it; should move into `nmp-router`.
- V-08 bunker DM is wired (seam restored, test un-ignored) but the regression test runs against a `StubRemoteSigner`, not a live NIP-46 bunker.
- `fixture-todo-core` is still in `crates/` because `nmp-codegen`'s `path = "../../../crates/{}"` hardcode hasn't been generalized — step 11 final closed but `fixture-todo-core` is the lone exception.
- `crates/nmp-core/src/wallet/` (311 LOC) + the `wallet` Cargo feature still in `nmp-core` — V-38 deprioritized.
- Substrate D0 noun leaks in `nmp-core` (4 items flagged 2026-05-25, closed by this PR): `Kernel::nip42_drivers` / `Nip42DriverState` renamed to `auth_drivers` / `AuthDriverState`; `RelayStatus::nip77_negentropy` / `RelayHealth::nip77_probe_state` / `Kernel::set_nip77_probe_state` renamed to `negentropy_probe` / `negentropy_probe_state` / `set_negentropy_probe_state`; `kernel/nip17_dm_inbox_routing_tests.rs` renamed to `kernel/dm_inbox_routing_tests.rs`; the `#[allow(unused_imports)]` cluster in `actor/mod.rs` replaced with structural `#[cfg(...)]` gates.

**What works on master** (~140k LOC, 33 crates): kernel substrate (`nmp-core`, mostly NIP-clean post-migration) · LMDB persistence (`nmp-store`) · planner (`nmp-planner`) · single-algorithm router (`nmp-router`) with NIP-65 outbox + Indexer (discovery kinds) + AppRelay fallback + blocked-relay filter + `explicit_targets` override seam · push-model `Pool` with generational `RelayHandle` + `PoolEvent` channel in `nmp-network` · routing-trace observability projection (FFI + wasm) · NIP-77 negentropy · NIP-42 relay auth (wire/FSM split across `nmp-network` + `nmp-nip42` + `nmp-core::subs::AuthGate`) · signers (local / NIP-07 / NIP-46) + write path · multi-account + `switch_active` · NWC wallet (NIP-47, still in `nmp-core` — V-38 deprioritized) · NIP-57 zaps (LNURL fetcher in `nmp-nip57`) · NIP-17 DMs (full stack in `nmp-nip17`, bunker NIP-46 sealing seamed) · Marmot/MLS encrypted groups · NIP-29 generic group infra · NIP-59 gift-wrap · content rendering · codegen tool · iOS Chirp + Android Chirp shells · desktop shell · LMDB CI · android-ffi `cargo check` · chirp-repl `routing-trace` subcommand + `scripts/validate-routing.sh` end-to-end smoke · `nmp_app_recent_routing_decisions` FFI + wasm surface for iOS/web inspectors.

**What does not work yet** (v1 blockers):
1. **V-01** — `nmp-wasm` no longer a stub: `WasmRuntime` drives the real `KernelReducer` (Stage 2, PR #372), owns a `BrowserRelayDriver` pool (Stage 3, PR #375), NIP-07 signer + async snapshot push (Stage 3b, PR #378), publish-path wire + multi-role bootstrap (Stage 3c, PR #385 — merged 2026-05-24). **Only F-01 IndexedDB persistence remains v1-blocking.** No persistent chirp-web features may be added until F-01 lands.
2. **F-02** — DM cold-start receive-side not yet verified against live relays (Rust pipeline test passes).
3. **F-04** — Zap E2E round-trip (NWC `pay_invoice` → kind:9735 → `ZapsAggregateProjection`) not verified against a live wallet.
4. **F-05** — `nmp-codegen` Swift `Decodable` pilot for `TimelineBlock` + `KernelUpdate`; deletes the 1,988-LOC handwritten counterpart in `KernelBridge.swift`.

**Framework thesis — NEEDS REVALIDATION (re-opened 2026-05-24 by Opus #13):** `apps/notes/` (PR #377) is 299 LOC Swift with zero new C-ABI symbols — but code-grounded inspection found it bypasses every defining framework property: raw kind filter instead of `LogicalInterest`, Swift-side timeline ordering, Swift `JSONSerialization` of event data, synchronous `isSignedIn` flip with no handshake gate. The LOC count is accurate; the proof is not. PD-033-A remains open until Notes is either rewritten against the real framework seams (LogicalInterest, kernel-owned timeline projection, handshake gate) or deleted with a written acknowledgement that the substrate is not yet expressive enough to support it. See [PD-033-A](BACKLOG.md#pd-033-a--framework-thesis--second-non-social-app--needs-revalidation).

**Largest accumulated debt:** 48 bespoke `nmp_app_*` FFI symbols in `crates/nmp-core/src/ffi/` (mod.rs alone is 1,559 LOC). Calendar written 2026-05-23 (see [PD-039](BACKLOG.md#pd-039--bespoke-ffi-deprecation-calendar-d11-expansion--decision-made-2026-05-23)): 16 are migration debt (user-intent verbs that bypass `dispatch_action`), 26 are structural-permanent under Theme A (lifecycle / callbacks / capability sockets / observer + projection registration / NWC connection lifecycle / publish control plane / liveness probe), 4 are test-only, 1 canonical, 1 already a thin shim. Target: 0 migration-debt symbols at v1-B. D11 covers publish; D11 expansion (PD-039) now covers the rest.

---

## Doctrine — final

The doctrine is final ([`product-spec.md` §1.5, D0–D10](product-spec.md)). Every PR is reviewed against this rubric; a change that makes any doctrine harder to enforce is rewritten or rejected.

- **D0** kernel never grows app nouns
- **D1** best-effort rendering with placeholders
- **D2** negentropy first, REQ second
- **D3** outbox routing automatic, manual relay is the opt-out
- **D4** single writer per fact; caches derive
- **D5** snapshots bounded by open views
- **D6** errors never cross FFI as exceptions
- **D7** capabilities report, never decide policy
- **D8** reactivity contract (composite reverse index, ≤60 Hz/view, working-set bounded)
- **D9** kernel owns time; relay-supplied `created_at` untrusted
- **D10** provenance; private events never escape to public relays
- **D11** publish goes through `dispatch_action` (in force; bespoke `nmp_app_publish_note` deleted PR #56)
- **D12** action_stages substrate with ack-based retention (in force)
- **D14** relay slots are typed projections (in force)

Corollary — **no hacks, no fragmentation, no debt**: temporary workarounds, stubs, "for now" branches, and silent failures are forbidden. Staging is allowed only when the staging plan is written in `BACKLOG.md` and progress advances each sprint.

---

## Doctrine corollaries — execution rules

- **Use rust-nostr.** `nostr` crate NIP modules are the protocol foundation. `nmp-nipXX` crates are thin NMP adapters, never crypto reimplementations.
- **No polling.** Sleep+check loops are forbidden at every layer. Use blocking recv, OS callbacks, or wall-clock-gated observers.
- **PR workflow.** Agents commit to a worktree branch and open a PR. Never push to `master` directly. Orchestrator merges.
- **Doctrine-lint scoped before push.** Banned tokens (`nip29` in `nmp-core`, etc.) tracked in `d0_doctrine_lint_banned_tokens` memory.

---

## Where we are — actual state on master

The original M0–M17 ladder predates the current codebase by a wide margin. Most of M2–M9 work landed without the ladder being updated. The honest mapping:

| Milestone | Original ladder claim | Actual state on master |
|---|---|---|
| M0 Kernel substrate + fixture | done | ✅ Built |
| M1 Chirp social baseline on iOS | hardening | ✅ Built (iOS Chirp + Android shells) |
| M2 Subscription compilation + outbox + kind:3 | design + impl | ✅ Planner/compiler built; **V-04 dual-system violation pending** |
| M3 Persistence (LMDB) | design + impl | ✅ `nmp-nostr-lmdb` + `lmdb-backend` feature |
| M4 NIP-77 negentropy | pending | ✅ `nmp-nip77` built + wired |
| M5 NIP-42 relay auth | pending | ✅ Built; **V-06 NIP-46 incompatibility pending (post-v1)** |
| M6 Sessions + signers + write | pending | ✅ Built (local-key/NIP-07/NIP-46 + broker) |
| M7 Reactions + thread + reply | pending | ✅ `nmp-reactions` + `nmp-threading` built |
| M8 Multi-session | pending | ✅ Multi-account + `switch_active` built |
| ~~M9~~ DMs | deferred post-v1 | 🟡 Gift-wrap built; conversation layer + **F-02 cold-start verification pending**; **V-08 bunker silent-fail pending (post-v1)** |
| M10 Blossom + media | pending | ❌ Not built (post-v1) |
| M10.5 FFI hardening | design done | ✅ S2/S3/S4/S5 gates closed; native CI coverage still a gap |
| ~~M11~~ Podcast rebuild | deferred | Skipped — see `nmp-only-two-agents` memory |
| ~~M11.5~~ Highlighter app proof | deferred | `nmp-nip29` retained as generic infra; app shell removed |
| ~~M12~~ Wallet (NWC + zaps + Cashu) | deferred post-v1 | 🟡 NWC + NIP-57 built; **F-04 E2E pending**; Cashu/nutzaps post-v1 |
| M13 Web-of-Trust | pending | ❌ Not built (post-v1) |
| M14 UniFFI migration | pending | ❌ Not started (post-v1) |
| M15 Cross-platform | pending | 🟡 Desktop + Android shells; wasm Stages 2–3c all merged (PR #372/#375/#378/#385); **F-01 IndexedDB is the sole remaining v1-blocking item** |
| M16 CLI + starter | pending | 🟡 `nmp-cli` exists; starter recipes not; component-registry/content-kit plan added in [`plan/m16-component-registry.md`](plan/m16-component-registry.md) |
| M17 v1 release | pending | ❌ Pending |

Detail per milestone lives in [`docs/plan/m*.md`](plan/). Active violations,
pending decisions, and feature backlog items live in [`docs/BACKLOG.md`](BACKLOG.md).

---

## v1 exit — what has to be true to ship

v1 ships when **all of the following** hold:

1. **No `BACKLOG.md` Section 1 violation is open** (or every open one has a staged plan that crosses the v1 line with progress per sprint).
2. **Every `BACKLOG.md` Section 4 v1-blocker item is closed.** Today: F-01, F-02, F-04, F-05.
3. **Every pending user decision in Section 3 is resolved** (today: PD-033-C, PD-037 closed; PD-033-A **re-opened 2026-05-24** — Notes bypasses framework seams, needs honest rewrite or deletion).
4. **Stateful second-app spike is run** — ⚠️ re-opened (PR #377 was counted as done 2026-05-23; Opus #13 found it bypasses LogicalInterest / kernel timeline / handshake gate — see [PD-033-A in BACKLOG.md](BACKLOG.md#pd-033-a--framework-thesis--second-non-social-app--needs-revalidation)).
5. **`nmp-wasm` is no longer a stub.** ✅ Stages 2–3c all complete (PRs #372/#375/#378/#385). **Only F-01 IndexedDB persistence remains** before chirp-web can claim full parity — see F-01 in BACKLOG.md.
6. **Cross-platform claim is honest.** Either wasm runs a real `NmpApp` actor on a Web Worker, or "cross-platform" is rewritten as "iOS + macOS + Android" in `aim.md` and product copy.
7. **No new bespoke `nmp_app_*` FFI symbol has been added since the deprecation calendar started.** ✅ calendar written 2026-05-23 — see [PD-039 in BACKLOG.md](BACKLOG.md#pd-039--bespoke-ffi-deprecation-calendar-d11-expansion--decision-made-2026-05-23). 48 symbols inventoried; 16 classified as migration debt, 26 as structural-permanent (Theme A), 4 as test-only, 1 canonical, 1 already a thin shim. Enforcement: the existing `ci/check-ffi-surface-freeze.sh` gate (`.github/workflows/ffi-surface-freeze.yml`) rejects net-additions by default; the single ADR override (`nmp_app_is_alive` / ADR-0028) is the precedent for future genuinely-structural additions.
8. **Snapshot serialization has a CI regression gate.** ✅ done — `make_update_us` + `serialize_us` instrumented in `crates/nmp-core/src/kernel/update.rs`. Gate: `snapshot_perf_firehose_gate` in `crates/nmp-core/src/kernel/perf_tests.rs` asserts `make_update_us < 250_000` μs and `serialize_us < 150_000` μs over a 1k-event firehose with `visible_limit = 500`. Thresholds = ≈ 10 × the observed dev-hardware debug baseline (~25 ms / ~15 ms, 5-run variance < 5 %); sized to catch a 10 × regression on `ubuntu-latest` debug CI without flaking on shared-runner jitter. The `NMP_PERF` log line in `kernel::update` remains the live monitoring signal in production. Test runs on every PR via `test.yml` (no new workflow required).
9. **All M0–M8 + M10.5 milestones gates are met against the current code** (the table above is honest; no silent endings).
10. **Doctrine D0–D14 enforced by lint** (doctrine-lint scoped run is part of CI on master).

Items 6–8 are the honest-cross-platform / deprecation-calendar / perf-gate triad from the 2026-05-23 direction review. Items 7 (deprecation calendar) and 8 (perf gate) are now closed; item 6 (honest cross-platform) is the remaining open item in this triad and must be added to `BACKLOG.md` if work is going to start on it.

---

## Post-v1 — explicitly deferred

Deliberately deferred. See [`BACKLOG.md` §5](BACKLOG.md#section-5--post-v1) and [`plan/post-v1.md`](plan/post-v1.md).

- Blossom uploads/downloads (M10)
- Web-of-Trust (M13)
- UniFFI migration (M14)
- Cashu / nutzaps (NIP-60/61)
- `nmp-codegen` full Swift bridge (pilot F-05 must land first)
- Second non-social app **as a product** (the v1 spike is a thesis test, not a shipped product)
- V-06 NIP-42+NIP-46 Stages 2-3 (broker `sign_auth_challenge` RPC)
- V-08 NIP-17 DM bunker support Stage 3 (`unwrap_gift_wrap` via remote signer RPC)
- ADR-0025 Marmot C-ABI cluster relocation out of Chirp binary

---

## Working agreements — agent + heartbeat conventions

These are not negotiable; they exist because each was learned the hard way. Full detail in memory.

- **Agents always run in the background, in worktree isolation** (`isolation: "worktree"`, `run_in_background: true`). Never name the main repo path as the agent's workdir.
- **Agents push to their worktree branch and open a PR.** Heartbeat sweeps orphan `worktree-agent-*` branches with commits not on master and cherry-picks them.
- **Agents must NEVER run full-workspace `cargo test`.** Scoped tests only — the orchestrator owns the full-suite pre-merge gate.
- **Heartbeat commits MUST be pathspec-scoped** (`git commit -- <file>`); land via throwaway worktree when the main tree is dirty.
- **README + this file are heartbeat-maintained.** Refresh dynamic parts only at each heartbeat; ≤200 LOC budget for the README, ≤250 LOC for this file.
- **After every merge to master, ask codex for a post-merge review** and record findings in `docs/perf/codex-reviews/`.

---

## Supporting documents

Where to look for detail:

- [`docs/aim.md`](aim.md) — architectural north star (immutable)
- [`docs/product-spec.md`](product-spec.md) + [`docs/product-spec/doctrine.md`](product-spec/doctrine.md) — full doctrine
- [`docs/BACKLOG.md`](BACKLOG.md) — active violations, pending decisions, v1 backlog
- [`WIP.md`](../WIP.md) — live in-flight tracker
- [`docs/plan/principles.md`](plan/principles.md) — execution principles
- [`docs/plan/subsystem-matrix.md`](plan/subsystem-matrix.md) — subsystem coverage + NIP roadmap
- [`docs/plan/parallelization.md`](plan/parallelization.md) — parallelization opportunities
- [`docs/plan/test-pyramid.md`](plan/test-pyramid.md) — test structure
- [`docs/plan/ci-hygiene.md`](plan/ci-hygiene.md) — CI / pre-merge hygiene
- [`docs/plan/decision-log.md`](plan/decision-log.md) — decision log
- [`docs/plan/scope-adjustments-2026-05-18.md`](plan/scope-adjustments-2026-05-18.md) — historical scope changes
- [`docs/plan/post-v1.md`](plan/post-v1.md) — deferred work detail
- [`docs/plan/marmot-mls.md`](plan/marmot-mls.md) — Marmot/MLS detail
- [`docs/plan/m16-component-registry.md`](plan/m16-component-registry.md) — app-owned component registry and native content kits
- [`docs/plan/m0-fixture.md`](plan/m0-fixture.md) – [`m17-release.md`](plan/m17-release.md) — per-milestone detail
- [`docs/architecture-audit/`](architecture-audit/) — 2026-05-23 13-agent audit, PD-033-C plan, codegen plan
- [`docs/perf/codex-reviews/`](perf/codex-reviews/) — post-merge codex reviews + opus direction reviews
- [`docs/decisions/`](decisions/) — ADRs 0001–0027

---

## What this plan is not

- **Not a schedule.** Milestones are sequential; durations depend on team size and surface complexity. No dates, no person-months.
- **Not a marketing roadmap.** v1 ships when the exit criteria above are met, not on a calendar.
- **Not the active-work tracker.** `WIP.md` owns in-flight; `BACKLOG.md` owns the queue. This file is the durable overview.
- **Not exhaustive about post-v1.** Additional protocol modules (NIP-23 long-form is in, more video/long-form work post-v1), app demonstrations, and the framework GA are scoped only after v1.
