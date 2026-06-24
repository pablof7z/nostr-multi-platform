# Build & Validation Plan

> Temporal coordination plan for shipping NMP v1. Reconciled 2026-05-25 against HEAD `cc10148f` (post step 8 phase F — actor cut-over to Pool); migration-ladder, NWC-location, and C-ABI-freeze claims re-reconciled 2026-06-11 against HEAD `104c3f76` (audit session `nmp-codebase-audit-2026-06`).
>
> **Durable references and temporal trackers:**
> - **Architectural north star** — [`docs/aim.md`](aim.md) (immutable; read first on cold-start).
> - **Durable doctrine** — [`docs/product-spec/doctrine.md`](product-spec/doctrine.md).
> - **Durable architecture** — [`docs/architecture/`](architecture/) and [`docs/design/`](design/).
> - **Live in-flight tracker** — [`WIP.md`](../WIP.md) (temporal work currently on a branch).
> - **Tactical tracker** — GitHub Issues, sorted by `priority:*` labels, then narrowed by `category:*`, `phase:*`, `area:*`, `doctrine:*`, and `status:*`.
>
> **This file is not durable understanding.** It is the current release-plan view. Active items belong in `WIP.md` (in-flight) or GitHub Issues (queue). Implemented or invalidated plan detail should be deleted or reduced to the smallest remaining live follow-up; lasting understanding belongs in aim, product, design, architecture, ADR, builder-guide, or wiki docs.

---

## TL;DR — one screen

**Architecture migration — COMPLETE (verified on master 2026-06-11).** The 12-step crate-boundary plan in `docs/architecture/crate-boundaries.md` is fully merged: steps 1 (substrate seams), 2 (`nmp-router`), 3 (kernel cut-over), 4 (V-41 LNURL), 5+6 (V-39+V-40 NIP-17), 7 (V-38 NWC → `nmp-nip47`, PR #460 merged 2026-05-25), 8 phases A–F (extraction, Pool API, BrowserRelayDriver, broker `Pool` dedupe PR #477 merged 2026-05-25, NIP-42 split, actor cut-over), 9 (`nmp-store`+`nmp-planner`), 10 (`nmp-defaults`), 11 partial+final (chirp-* moved, `nmp-ffi` extracted), 12 (`nmp-marmot` back under `crates/`, Path B per ADR-0025 update 2026-05-23). Substrate-honest debts A–D and V-08 (bunker DM send) ✅ merged. V-51 routing observability phases 1+2+4+5 ✅ merged; phase 3 (iOS/web Chirp inspector UI plus `chirp-tui` relay/settings diagnostics) is the remaining open item.

**Live validation works.** `cargo test -p nmp-testing --test routing_trace_real_nostr -- --ignored` fetches pablof7z's real NIP-65 from `wss://relay.damus.io`, hands it to `nmp_router::GenericOutboxRouter::route_subscription`, asserts the resolved set (`r.f7z.io`, `relay.damus.io`, `relay.primal.net`) is attributed to `Nip65/Read` lane with zero `AppRelay/Fallback` leak. The kernel **actually consumes** the router's output for live REQ-relay selection (PR #462 + PR #468 cut-over; observe-only → decision authority).

**Known partial state** (honest about what's not yet clean):
- V-08 bunker DM is wired (seam restored, test un-ignored) but the regression test runs against a `StubRemoteSigner`, not a live NIP-46 bunker.
- Substrate D0 noun leaks in `nmp-core` (4 items flagged 2026-05-25, closed by this PR): `Kernel::nip42_drivers` / `Nip42DriverState` renamed to `auth_drivers` / `AuthDriverState`; `RelayStatus::nip77_negentropy` / `RelayHealth::nip77_probe_state` / `Kernel::set_nip77_probe_state` renamed to `negentropy_probe` / `negentropy_probe_state` / `set_negentropy_probe_state`; `kernel/nip17_dm_inbox_routing_tests.rs` renamed to `kernel/dm_inbox_routing_tests.rs`; the `#[allow(unused_imports)]` cluster in `actor/mod.rs` replaced with structural `#[cfg(...)]` gates.

**What works on master** (~140k LOC, 33 crates): kernel substrate (`nmp-core`, mostly NIP-clean post-migration) · LMDB persistence (`nmp-store`) · planner (`nmp-planner`) · single-algorithm router (`nmp-router`) with NIP-65 outbox + Indexer (discovery kinds) + AppRelay fallback + blocked-relay filter · publish engine explicit relay pinning via `PublishTarget::Explicit` · push-model `Pool` with generational `RelayHandle` + `PoolEvent` channel in `nmp-network` · routing-trace observability projection (FFI + wasm) · NIP-77 negentropy · NIP-42 relay auth (wire/FSM split across `nmp-network` + `nmp-nip42` + `nmp-core::subs::AuthGate`) · signers (local / NIP-07 / NIP-46) + write path · multi-account + `switch_active` · NWC wallet (`nmp-nip47`; V-38 step 7 merged — kernel no longer deps `nmp-nwc`) · NIP-57 zaps (LNURL fetcher in `nmp-nip57`) · NIP-17 DMs (full stack in `nmp-nip17`, bunker NIP-46 sealing seamed) · Marmot/MLS encrypted groups · NIP-29 generic group infra · NIP-59 gift-wrap · content rendering · codegen tool · iOS Chirp + Android Chirp shells · desktop shell · LMDB CI · android-ffi `cargo check` · `nmp_app_debug_info` (routing-decisions + composition-report domains) FFI + wasm `recent_routing_decisions()` surface for iOS/web inspectors.

**Active transport migration (2026-05-26):** the intended Rust-to-frontend
runtime update transport is one canonical FlatBuffers schema for `FullState`,
`ViewBatch`, and side-effect frames. UniFFI remains the generated
binding/lifecycle/capability surface; it is not the hot payload format. Legacy
JSON update payloads are historical raw-C/migration surface only, not a
production fallback. Track under [F-10 issue #991](https://github.com/pablof7z/nostr-multi-platform/issues/991).

**What does not work yet** (v1 blockers):
1. **F-02** — DM cold-start receive-side not yet verified against live relays (Rust pipeline test passes).
3. **F-05 / F-10 — DONE** — Full typed-projection coverage + `payload:Value` deletion complete. All consumers (chirp-tui, chirp-desktop, nmp-gallery TUI + desktop) decode typed-first; `encode_snapshot_with_envelope` no longer emits `payload:Value`; `decode_snapshot_payload` has zero callers. Frame-size delta: 14,504 B → 3,384 B (−76.7%) for an empty frame (the 4,457 B JSON blob = 31% overhead is now gone). See [#979](https://github.com/pablof7z/nostr-multi-platform/issues/979), [#991](https://github.com/pablof7z/nostr-multi-platform/issues/991) (both closed).

**Web/wasm scope moved post-v1 (2026-06-11).** `nmp-wasm` stages 2–3c
landed (PRs #372/#375/#378/#385), but browser persistence, OPFS-SQLite
storage, and the browser parity claim are no longer v1 exit criteria. The v1
platform contract is iOS, Android, and desktop. Web/wasm resumes in the
post-v1 web milestone tracked by [#1007](https://github.com/pablof7z/nostr-multi-platform/issues/1007)
and [#1008](https://github.com/pablof7z/nostr-multi-platform/issues/1008).

**Framework thesis — CLOSED (2026-06-11, Decision B):** the second-app gate is satisfied by external consumer apps. Owner-verified evidence: `~/Work/podcast-player` is an external workspace pinning `nmp-defaults`/`nmp-core`/`nmp-ffi`/`nmp-signer-broker` at git rev `104c3f76`, with `apps/nmp-app-podcast` (~56k LOC Rust composing `ffi/register.rs`, `nmp_dispatch.rs`, `android.rs`) and a ~100k-LOC Swift iOS app; the owner also operates `win-the-day` and `hl` as NMP apps. The external-consumer relationship is documented in [`docs/architecture/external-consumers.md`](architecture/external-consumers.md). See [PD-033-A issue #975](https://github.com/pablof7z/nostr-multi-platform/issues/975) (closed).

**C-ABI surface — legitimate API, not debt** *(updated 2026-06-19)*: the named `nmp_app_*` C-ABI symbols are framework API — lifecycle, callbacks, capability sockets, observer + projection registration, identity/relay ops, and the generic `nmp_app_dispatch_action` path. Legitimate API does **not** mean compatibility freeze before v1: dead slots, app-named generic surfaces, and compatibility shims must be deleted or renamed when they fail the durable doctrine. The former freeze gate (`ci/check-ffi-surface-freeze.sh`, frozen at 54) was deliberately deleted in PR #933; net-new symbols are governed by review + ADR/issue convention, while cleanup is tracked through the issue queue (for example [#1607](https://github.com/pablof7z/nostr-multi-platform/issues/1607), [#1609](https://github.com/pablof7z/nostr-multi-platform/issues/1609), and [#1611](https://github.com/pablof7z/nostr-multi-platform/issues/1611)). The generic `nmp_app_open_interest(filter_json, consumer_id, scope)` landed in the M2 migration (PR #923); retiring the legacy `open_author`/`open_thread` stack (which still hardcodes Chirp's social kinds `{1,6}`) is tracked in [#958](https://github.com/pablof7z/nostr-multi-platform/issues/958) (V-68 Stage 2) and [#957](https://github.com/pablof7z/nostr-multi-platform/issues/957) (V-112). Follow/feed app APIs declare primary kinds and reactive sources; protocol adapters derive wrapper acquisition.

---

## Doctrine Checkpoint

The durable doctrine lives in [`docs/product-spec/doctrine.md`](product-spec/doctrine.md). The list here is only the current release-plan checkpoint used while deciding whether v1 can ship. If this summary drifts, update or delete it rather than treating the plan as durable doctrine.

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

Corollary — **no hacks, no fragmentation, no debt**: temporary workarounds, stubs, "for now" branches, and silent failures are forbidden. Staging is allowed only when the staging plan is written in a GitHub issue labeled `status:staged` and progress advances each sprint.

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
| ~~M12~~ Wallet (NWC + zaps + Cashu) | deferred post-v1 | ✅ NWC + NIP-57 shipped; zap send (NWC `pay_invoice` → kind:9735 → `ZapsAggregateProjection`) E2E-harness-verified (PR #1076, F-04 closed); **further zap work post-v1 by owner decision 2026-06-12**; Cashu/nutzaps post-v1 |
| M13 Web-of-Trust | pending | ❌ Not built (post-v1) |
| M14 UniFFI migration | pending | ❌ Not started (post-v1) |
| M15 Native cross-platform | pending | 🟡 Desktop (egui) + Android shells; wasm Stages 2–3c are merged but web persistence/parity is post-v1; v1 platform contract = iOS + Android + desktop (egui) |
| M16 CLI + starter | pending | 🟡 `nmp-cli` exists; starter recipes not; component-registry/content-kit plan added in [`plan/m16-component-registry.md`](plan/m16-component-registry.md) |
| M17 v1 release | pending | ❌ Pending |

Detail per milestone lives in [`docs/plan/m*.md`](plan/). Active violations,
pending decisions, and queued feature work live in GitHub Issues.

---

## v1 exit — what has to be true to ship

v1 ships when **all of the following** hold:

1. **No open `category:violation` issue blocks v1** (or every such issue has a `status:staged` plan that crosses the v1 line with progress per sprint).
2. **Every `phase:v1-blocker` feature issue is closed.** ✅ **SATISFIED as of v0.5.0 — zero open `phase:v1-blocker` issues.** F-02 [#977](https://github.com/pablof7z/nostr-multi-platform/issues/977) **CLOSED** — kernel-driven cold-start verified, kind:10050 recompile defect fixed (PR #1080). gc_step wiring [#1069](https://github.com/pablof7z/nostr-multi-platform/issues/1069) **CLOSED** — wired to the actor idle tick (PR #1072), budgets hardened (PR #1094), expiration index (PR #1106). F-12 store→projection replay [#1086](https://github.com/pablof7z/nostr-multi-platform/issues/1086) **CLOSED** — ADR-0045 universal cache-serve complete (PRs #1107 E1, #1117 E2+E3); universal acceptance test `cache_serve_universal_tests` passes: feed + DM inbox + thread replies + long-form all render from the store on second launch with zero relay connections. The owner-decided v1 exit criterion (*launch twice, second launch offline, every open interest's projection renders from the store*) is **satisfied as of v0.5.0**. F-04 [#978](https://github.com/pablof7z/nostr-multi-platform/issues/978) **CLOSED** — zap E2E harness verified (PR #1076). F-05 [#979](https://github.com/pablof7z/nostr-multi-platform/issues/979) and F-10 [#991](https://github.com/pablof7z/nostr-multi-platform/issues/991) **CLOSED** — payload:Value zeroing complete. Web/wasm issues [#1007](https://github.com/pablof7z/nostr-multi-platform/issues/1007) and [#1008](https://github.com/pablof7z/nostr-multi-platform/issues/1008) are post-v1.
3. **Every pending `category:decision` issue that blocks v1 is resolved** (today: PD-033-C, PD-037 closed; PD-033-A [#975](https://github.com/pablof7z/nostr-multi-platform/issues/975) **CLOSED 2026-06-11** — second-app gate met via external consumer apps per owner decision; see [`docs/architecture/external-consumers.md`](architecture/external-consumers.md)).
4. **Second-app gate** — ✅ MET (2026-06-11, Decision B): external consumer apps (`podcast-player`, `win-the-day`, `hl`) pin NMP by git rev and compose framework seams. See [PD-033-A issue #975](https://github.com/pablof7z/nostr-multi-platform/issues/975) (closed) and [`docs/architecture/external-consumers.md`](architecture/external-consumers.md).
5. **The v1 platform claim is honest.** v1 claims iOS, Android, and desktop (egui). Browser/web/wasm is a non-persistent preview until OPFS-SQLite persistence and the NmpApp-actor-in-Worker port land (both post-v1, tracked by [#1007](https://github.com/pablof7z/nostr-multi-platform/issues/1007) and [#1008](https://github.com/pablof7z/nostr-multi-platform/issues/1008)).
6. **Native cross-platform consistency is proven** — RESOLVED by re-scoping (2026-06-11, Decision A): the v1 cross-platform gate covers iOS + Android + desktop (egui). Web is a non-persistent preview and does not gate v1. See [#1008](https://github.com/pablof7z/nostr-multi-platform/issues/1008).
7. **The C-ABI surface governance is enforced: no net-new `nmp_app_*` symbol without a merged ADR.** The former CI freeze gate (`ci/check-ffi-surface-freeze.sh`, `.github/workflows/ffi-surface-freeze.yml`) was deleted in PR #933; governance is now via `ffi-drift.yml` (header-vs-Rust diff gate) plus review + ADR convention for net-new symbols. V-68 Stage 2 **COMPLETED** (2026-06-12, ADR-0042 amendment): `nmp_app_open_timeline` deleted. Issue #1626 replaces the old contact-feed/kind-list vocabulary with active-follows feed declarations: apps/defaults declare primary content kinds from a reactive perspective, and protocol adapters derive repost-wrapper acquisition below the app boundary.
8. **Snapshot serialization has a CI regression gate.** ✅ done — `make_update_us` + `serialize_us` instrumented in `crates/nmp-core/src/kernel/update.rs`. Gate: `snapshot_perf_firehose_gate` in `crates/nmp-core/src/kernel/perf_tests.rs` asserts `make_update_us < 15_000` μs (`MAX_MAKE_UPDATE_US`) and `serialize_us < 8_000` μs (`MAX_SERIALIZE_US`) over a 1k-event firehose with `visible_limit = 500`. Thresholds = ≈ 11–12 × the observed dev-hardware debug contention baseline; sized to catch a regression on `ubuntu-latest` debug CI without flaking on shared-runner jitter. (The earlier 250 000 / 150 000 μs ceilings were tightened to these values in `perf_tests.rs`.) The `NMP_PERF` log line in `kernel::update` remains the live monitoring signal in production. Test runs on every PR via `test.yml` (no new workflow required).
9. **All M0–M8 + M10.5 milestones gates are met against the current code** (the table above is honest; no silent endings).
10. **Doctrine D0–D14 enforced by lint** (doctrine-lint scoped run is part of CI on master).

---

## Post-v1 — explicitly deferred

Deliberately deferred. See GitHub Issues labeled `phase:post-v1` and [`plan/post-v1.md`](plan/post-v1.md).

- Web/wasm browser support: OPFS-SQLite persistence, NmpApp-actor-in-Worker port, and the honest web cross-platform claim ([#1007](https://github.com/pablof7z/nostr-multi-platform/issues/1007), [#1008](https://github.com/pablof7z/nostr-multi-platform/issues/1008))
- Blossom uploads/downloads (M10)
- Web-of-Trust (M13)
- UniFFI migration (M14)
- Further zap work: receipt `nostrPubkey` author verification (#1043), `ZapRequestBuilder` sentinel-value API fix (#610), `zap_subscription` typed-sidecar shape decision (#1022), any zap UX hardening — owner decision 2026-06-12; v1 ships current capability: send via NWC, kind:9735 ingest + `ZapsAggregateProjection`, E2E-harness-verified
- Cashu / nutzaps (NIP-60/61)
- V-06 NIP-42+NIP-46 Stages 2-3 — via the signer-session capability port ([#960](https://github.com/pablof7z/nostr-multi-platform/issues/960))
- V-08 NIP-17 DM bunker receive Stage 3 — per-envelope remote-RPC plan rejected 2026-06-12 (O(2N) sequential bunker round-trips); re-scoped to signer-session port + delegated-decrypt-vs-product-policy decision ([#961](https://github.com/pablof7z/nostr-multi-platform/issues/961))
- ADR-0025 Marmot C-ABI cluster relocation out of Chirp binary

---

## Working agreements — agent + heartbeat conventions

These are not negotiable; they exist because each was learned the hard way. Full detail in memory.

- **Agents always run in the background, in worktree isolation** (`isolation: "worktree"`, `run_in_background: true`). Never name the main repo path as the agent's workdir.
- **Agents push to their worktree branch and open a PR.** Heartbeat sweeps orphan `worktree-agent-*` branches with commits not on master and cherry-picks them.
- **Agents must NEVER run full-workspace `cargo test`.** Scoped tests only — the orchestrator owns the full-suite pre-merge gate.
- **Heartbeat commits MUST be pathspec-scoped** (`git commit -- <file>`); land via throwaway worktree when the main tree is dirty.
- **README + this file are heartbeat-maintained.** Refresh dynamic parts only at each heartbeat; ≤200 LOC budget for the README, ≤250 LOC for this file.
- **After every merge to master, review the diff for notable findings.** Promote actionable items into GitHub Issues; do not commit code reviews to the repo.

---

## Supporting documents

Where to look for detail:

- [`docs/aim.md`](aim.md) — architectural north star (immutable)
- [`docs/product-spec.md`](product-spec.md) + [`docs/product-spec/doctrine.md`](product-spec/doctrine.md) — full doctrine
- GitHub Issues — active violations, pending decisions, v1 queue; sort by `priority:*` labels
- [`WIP.md`](../WIP.md) — live in-flight tracker
- [`docs/plan/principles.md`](plan/principles.md) — execution principles
- [`docs/plan/subsystem-matrix.md`](plan/subsystem-matrix.md) — subsystem coverage + NIP roadmap
- [`docs/plan/parallelization.md`](plan/parallelization.md) — parallelization opportunities
- [`docs/plan/test-pyramid.md`](plan/test-pyramid.md) — test structure
- [`docs/plan/ci-hygiene.md`](plan/ci-hygiene.md) — CI / pre-merge hygiene
- [`docs/plan/decision-log.md`](plan/decision-log.md) — decision log
- [`docs/plan/post-v1.md`](plan/post-v1.md) — deferred work detail
- [`docs/plan/marmot-mls.md`](plan/marmot-mls.md) — Marmot/MLS detail
- [`docs/plan/m16-component-registry.md`](plan/m16-component-registry.md) — app-owned component registry and native content kits
- [`docs/plan/m12-wallet.md`](plan/m12-wallet.md) – [`m17-release.md`](plan/m17-release.md) — per-milestone detail (active/future only)
- [`docs/architecture-audit/`](architecture-audit/) — PD-033-C plan, codegen plan
- [`docs/decisions/`](decisions/) — ADRs 0001–0038

---

## What this plan is not

- **Not a schedule.** Milestones are sequential; durations depend on team size and surface complexity. No dates, no person-months.
- **Not a marketing roadmap.** v1 ships when the exit criteria above are met, not on a calendar.
- **Not durable understanding.** Implemented or invalidated plan detail must be removed, not preserved as reference documentation.
- **Not the active-work tracker.** `WIP.md` owns in-flight; GitHub Issues own the queue. This file is only the current release-plan view.
- **Not exhaustive about post-v1.** Additional protocol modules (NIP-23 long-form is in, more video/long-form work post-v1), app demonstrations, and the framework GA are scoped only after v1.
