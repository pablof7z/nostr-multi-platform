# Repo state snapshot — HB43 pause-and-review (2026-05-18)

> Single honest snapshot of REPO STATE + MASTER INVARIANTS at master tip
> `0b85616` (HB43 reconcile commit). Gates run from this worktree against the
> rebased tip. Companion to `docs/perf/task-reconcile-and-next-steps-2026-05-18.md`
> (forward queue) — this doc is the **rear-view mirror**: what is _actually_ in
> the tree, with cites.

Master tip: `0b85616 docs(perf): task reconcile + next-steps synthesis — HB43 pause-and-review`.
Prior code tip: `e872c2a` (HB43 orchestration); `23ac82c` (relations integration).

---

## 1. Master invariants — gates run verbatim

| Gate | Command | Result | Notes |
|---|---|---|---|
| Workspace tests | `cargo test --workspace --no-fail-fast` | **1031 passed, 1 failed, 17 ignored** | One failure: `nmp-cli --test init init_scaffold_compiles_and_gen_is_deterministic` |
| Framework-magic contract | `cargo test -p nmp-testing --test framework_magic_contract` | **14/14 passed** | C1–C13 active, zero `#[ignore]` |
| Doctrine lint (nmp-core) | `cargo run -p nmp-testing --bin doctrine-lint -- --crate nmp-core` | **0 findings (D0/D6/D7/D8 clean)** | |
| Clippy (workspace, all targets, -D warnings) | `cargo clippy --workspace --all-targets -- -D warnings` | **Clean (exit 0)** | |

### Workspace-test failure detail (DOC DRIFT vs HB43 "workspace GREEN")

`crates/nmp-cli/tests/init.rs:53` asserts `nmp init <name> --path …` exits 0;
on master tip `0b85616` it exits 1 with `nmp: usage: nmp gen modules …`.

Root cause: **two workspace bins are both named `nmp`** —
`crates/nmp-cli/Cargo.toml:[[bin]] name = "nmp"` (the CLI) and
`crates/nmp-codegen/Cargo.toml:[[bin]] name = "nmp"` (the legacy codegen front-end).
They overwrite each other at `target/debug/nmp`; the codegen `nmp` (which only
knows `gen modules`) is winning, so the test's `CARGO_BIN_EXE_nmp` path
resolves to the wrong binary at runtime.

This is a **pre-existing bug on master**, not introduced by HB43. The failure
is **build-order-dependent** — whichever `nmp` bin compiles last wins
`target/debug/nmp`. On this run the codegen bin won and the test failed; a
clean rebuild could non-deterministically pass. The HB43 orchestration log
claim "workspace GREEN" (HB43 entry in `docs/perf/orchestration-log.md`) may
reflect a run where the cli bin won — but the underlying collision is a bug
either way. **DOC DRIFT to the extent HB43 implies the workspace is _reliably_
GREEN.** Filing as a separate task (one-line `Cargo.toml` rename) is
appropriate.

### Workspace + LOC inventory

`Cargo.toml:1-27` declares **26 workspace members**: 20 under `crates/`, 5 under
`apps/podcast/`, 1 under `apps/fixture/`.

LOC (rust only, `wc -l` per crate):

| Crate | files | LOC | tests |
|---|--:|--:|--:|
| `crates/nmp-core` | 118 | 23,674 | 292 |
| `crates/nmp-testing` | 91 | 16,509 | 143 |
| `crates/nmp-signers` | 22 | 3,813 | 55 |
| `crates/nmp-content` | 19 | 3,550 | 98 |
| `crates/nmp-nip29` | 31 | 3,244 | 34 |
| `crates/nmp-reactions` | 16 | 2,841 | 62 |
| `crates/nmp-nip51` | 15 | 2,624 | 64 |
| `crates/nmp-nip77` | 12 | 2,398 | 46 |
| `crates/nmp-content-fixtures` | 14 | 2,263 | 6 |
| `crates/nmp-nip23` | 18 | 1,954 | 55 |
| `crates/nmp-nip01` | 6 | 1,268 | 23 |
| `crates/nmp-nip22` | 6 | 1,031 | 23 |
| `crates/nmp-nip57` | 7 | 1,019 | 38 |
| `crates/nmp-nip42` | 6 | 820 | 18 |
| `crates/nmp-desktop` | 6 | 571 | 1 |
| `crates/nmp-codegen` | 5 | 557 | 4 |
| `crates/nmp-cli` | 4 | 401 | 2 |
| `crates/fixture-todo-core` | 1 | 304 | 2 |
| `crates/nmp-nip42-types` | 3 | 294 | 10 |
| `crates/nmp-highlighter-core` | 3 | 148 | 9 |
| `apps/podcast/podcast-core` | 6 | 434 | 2 |
| `apps/podcast/podcast-feeds` | 4 | 196 | 3 |
| `apps/podcast/podcast-llm` | 4 | 196 | 2 |
| `apps/podcast/podcast-rag` | 4 | 168 | 2 |
| `apps/podcast/podcast-audio` | 3 | 106 | 3 |
| `apps/fixture/nmp-app-fixture` | 7 | 91 | (generated) |

Total kernel + protocol + testing LOC: **~69,283** across all members.

---

## 2. Shipped crates inventory

Status legend: **real** = production code, has integration tests, used by FFI/iOS;
**scaffold** = compile-clean placeholder, intentional, milestone-tagged;
**stub** = trait-impl-only or library shim.

| Crate | LOC | Tests | Status | Notes |
|---|--:|--:|---|---|
| `nmp-core` | 23,674 | 292 | **real** | The kernel substrate. Actor + ingest + planner + outbox resolver + store + publish + FFI |
| `nmp-testing` | 16,509 | 143 | **real** | Framework magic contract (14/14), firehose-bench, reactivity-bench, ffi-stress, doctrine-lint, store harness |
| `nmp-signers` | 3,813 | 55 | **real** | Local nsec + bunker (NIP-46) signer trait + impls |
| `nmp-content` | 3,550 | 98 | **real** | Text-note parsing, mentions, URLs, NIP-19/21 decoders, content gallery views |
| `nmp-nip29` | 3,244 | 34 | **real** | NIP-29 (group chat) decoder + builder + views |
| `nmp-reactions` | 2,841 | 62 | **real** | Cross-NIP Relations facade (commit `9001660` HB43) |
| `nmp-nip51` | 2,624 | 64 | **real** | NIP-51 (lists) |
| `nmp-nip77` | 2,398 | 46 | **real** | NIP-77 negentropy |
| `nmp-content-fixtures` | 2,263 | 6 | **real** | Shared test fixtures |
| `nmp-nip23` | 1,954 | 55 | **real** | NIP-23 long-form |
| `nmp-nip01` | 1,268 | 23 | **real** | NIP-01 + NIP-10 reply builder + Replies/Thread views (commit `5eb0b7f` HB43) |
| `nmp-nip22` | 1,031 | 23 | **real** | NIP-22 standalone comment decoder (commit `d715b55` HB43) |
| `nmp-nip57` | 1,019 | 38 | **real** | NIP-57 zap receipt decoder + bolt11 amount + zap-request builder (commit `37acab3` HB43) |
| `nmp-nip42` | 820 | 18 | **real** | NIP-42 relay auth |
| `nmp-desktop` | 571 | 1 | **real** (small) | Desktop shell scaffolding |
| `nmp-codegen` | 557 | 4 | **real** | Codegen library + `nmp` legacy bin (collides with nmp-cli) |
| `nmp-cli` | 401 | 2 | **real** (1 broken) | `nmp init` + `nmp gen modules`; init test fails — see §1 |
| `fixture-todo-core` | 304 | 2 | **real** | Non-Nostr TODO domain proving the kernel boundary |
| `nmp-nip42-types` | 294 | 10 | **real** | Shared NIP-42 types |
| `nmp-highlighter-core` | 148 | 9 | **scaffold** | M11.5 Step 0 placeholder (`crates/nmp-highlighter-core/src/lib.rs:10-19`); placeholders.rs + lib.rs only |
| `apps/podcast/podcast-core` | 434 | 2 | **stub** | Domain stubs (Episode/Podcast/Chapter); no real ingest or audio |
| `apps/podcast/podcast-feeds` | 196 | 3 | **stub** | RSS/Atom/JSON-Feed parsing stub returns `not_implemented` (`podcast_feeds_parser_stub_returns_not_implemented`) |
| `apps/podcast/podcast-llm` | 196 | 2 | **stub** | Prompts non-empty + Apple Intelligence default route only |
| `apps/podcast/podcast-rag` | 168 | 2 | **stub** | Vector store stub returns `not_implemented` (`podcast_rag_vector_store_stub_returns_not_implemented`) |
| `apps/podcast/podcast-audio` | 106 | 3 | **stub** | Playback state default + capability event serialization only |
| `apps/fixture/nmp-app-fixture` | 91 | (gen) | **real** | Codegen output for fixture-todo-core |

DOC DRIFT vs `docs/plan/status.md:7-16` "Implemented and running": status.md still
quotes "Kernel substrate (~3,800 LOC)" — actual kernel LOC is **23,674** (6×
growth since that paragraph was written). Same paragraph quotes `ios/NmpStress`
at "~1,375 LOC Swift" — actual is **1,476 LOC** across 10 files.

---

## 3. Milestone reality vs claims (M0–M17)

Skipping M9 (DMs) and M12 (Wallet) — deferred post-v1 per
`docs/plan/post-v1.md`. Evidence cites are at master tip `0b85616`.

| M | Topic | Claimed (status.md / plan) | Evidence | Verdict |
|---|---|---|---|---|
| M0 | Fixture | DONE | `crates/fixture-todo-core/src/lib.rs:1-304` non-Nostr TODO ships all five substrate trait families; `apps/fixture/nmp-app-fixture` codegen output compiles | **DONE** |
| M1 | Twitter slice | DONE | `ios/NmpStress` 1,476 LOC Swift, 15 `nmp_app_*` FFI calls; live `wss://relay.primal.net` + `wss://purplepag.es` per `docs/plan/status.md:10`; `docs/perf/m1/nmpstress-sim-boot.png` evidence | **DONE** |
| M2 | Subscription compilation | DONE | `crates/nmp-core/src/planner/` (compiler/, lattice/, partition/); `framework_magic_contract::c5_c8_c13::c5_kind3_change_recompiles_follow_dependent_subs` passes; `c8_subscriptions_coalesce_autoclose_and_buffer` passes | **DONE** |
| M3 | Persistence | PARTIAL (in-memory only per status.md:19) | `crates/nmp-core/src/store/mem/*.rs` (2,200 LOC mem store split into insert/query/gc/domain/store_impl); `crates/nmp-core/src/store/lmdb.rs:38-62` LMDB **skeleton past `open()` only** — `not_enabled()` stubs (confirms §27 row 2) | **PARTIAL** (LMDB skeleton) |
| M4 | Negentropy | DONE | `crates/nmp-nip77` 2,398 LOC, 46 tests; `docs/builder-guide/PLAN.md` lists negentropy under DONE | **DONE** |
| M5 | NIP-42 relay auth | DONE (kernel) | `crates/nmp-nip42` 820 LOC, 18 tests; `crates/nmp-nip42-types` 294 LOC, 10 tests | **DONE** (kernel); iOS binding deferred per PD-005 |
| M6 | Signers + write | DONE | `crates/nmp-signers` 3,813 LOC, 55 tests (local nsec + bunker NIP-46) | **DONE** |
| M7 | Interaction loop / publishing | PARTIAL | `crates/nmp-core/src/publish/{engine/,nip65/}` shipped; T117 publish-engine-wire FSM unwired (HB41 dispatch, no commits since `167d4bc`); `c7_publish_routes_outbox_and_private_fails_closed` passes (publish path covered for outbox routing) | **PARTIAL** (FSM unwired; outbox routing done) |
| M8 | Multi-account / subscription lifecycle | DONE | `framework_magic_contract::c12::c12_account_switch_rebinds_views_without_imperative_dance` passes; 11 canonical triggers in `crates/nmp-core/src/subs/trigger.rs:66-67` (DOC DRIFT vs `m8-subscription-lifecycle.md:21` which says "10/ten triggers" — §27 row 10) | **DONE** |
| M9 | DMs | (deferred post-v1) | — | DEFERRED |
| M10 | Blossom | NOT STARTED | No `crates/nmp-blossom-*`; M10.5 re-scope addendum (PD-021) explicitly defers M10 UI scenarios | **NOT-STARTED** |
| M10.5 | FFI hardening | PARTIAL (re-scoped) | `docs/perf/m10.5/sim-baseline.md`, S1-S5 dirs, `ui-fleet/` (F1-F9 screenshots), `leak-evidence/` (5 screenshots); §G-S2 working-set retention OPEN per `docs/perf/m10.5/s2-drain-analysis.md` — verdict "RETAINED, not transient"; gates T114b retention audit | **PARTIAL** (gate open) |
| M11 | Podcast app (kernel-boundary proof) | NOT STARTED | `apps/podcast/podcast-*` 5 crates, **all stubs** (total ~1,100 LOC, 12 tests, mostly `*_returns_not_implemented` or default-state assertions); `ios/NmpPodcast` 31 Swift files / 5,091 LOC but **zero** `nmp_app_*` FFI refs — Step 0 copy-step scaffolds only (matches §27 row 6) | **NOT-STARTED** (scaffolds only) |
| M11.5 | Highlighter | NOT STARTED | `crates/nmp-highlighter-core` 148 LOC scaffold + `placeholders.rs` only; `ios/NmpHighlighter` 158 Swift files / 39,118 LOC but **zero** `nmp_app_*` FFI refs (verbatim copy of upstream Swift app, no NMP wiring) | **NOT-STARTED** (scaffolds only) |
| M12 | Wallet | (deferred post-v1) | — | DEFERRED |
| M13 | WoT | NOT STARTED | No `crates/nmp-wot`; mentioned only in plan | **NOT-STARTED** |
| M14 | UniFFI | NOT STARTED | `crates/nmp-codegen/src/generate.rs:108-138` emits plain `#[derive]`, no uniffi (confirms §27 row 3 + 4); live FFI is hand-written raw C JSON in `crates/nmp-core/src/ffi.rs` | **NOT-STARTED** |
| M15 | Cross-platform shells | PARTIAL | `crates/nmp-desktop` 571 LOC desktop shell exists; Android shell absent; Web shell absent | **PARTIAL** (desktop only) |
| M16 | CLI starter | PARTIAL | `crates/nmp-cli` ships `init` + `gen modules` (401 LOC, 2 tests); 1 test failing (bin-name collision §1); §27 row 5 also notes "no `nmp` binary exists" — that row is now stale (the binary exists; the row 5 claim is outdated per HB43) — **DOC DRIFT** | **PARTIAL** (init test broken) |
| M17 | Release | NOT STARTED | No release artifacts in tree | **NOT-STARTED** |

### §27 row verification (specifically requested)

- **Row 1 (outbox-on-wire):** **NOW DONE** for the follow-feed + author + publish paths.
  Evidence chain: `167d4bc` (CompiledPlan→live REQ/publish wire) + `5c5d417`
  (outbox resolver) + `e74247c` (fan timeline/author/claim REQs by resolved
  write relays) + `0849fd2` (recompilation trigger A1 + 3 integration tests) +
  `fada22b` (URL-keyed transport pool — `HashMap<String, RelayControl>` with
  on-demand worker spawn) + `24cad7c` (thread `relay_url` through ingest for
  store provenance). Verified by codex review at `docs/perf/codex-reviews/t105-167d4bc-5c5d417.md:138-170`
  ("Row 1 status: DONE for the follow-feed + author + publish paths"). Residuals
  (R1 thread hydration, R2 firehose inbox-side) are separate concerns, filed
  T121/T122 — not the D3 follow-feed outbox claim.

- **Row 2 (LmdbEventStore = skeleton past `open()`):** **STILL ACCURATE.**
  `crates/nmp-core/src/store/lmdb.rs:38-53` `open()` only creates directories;
  every other `EventStore` method returns `not_enabled()` (verified at
  `lmdb.rs:57-62` per the original §27 evidence). No commit since the row was
  written has changed lmdb.rs substantively. Closing this is M3 phase 2 scope.

---

## 4. iOS apps inventory

| App | Swift files | Swift LOC | `nmp_app_*` refs | Last commit | Sim-built evidence |
|---|--:|--:|--:|---|---|
| `ios/NmpStress` | 10 | 1,476 | 15 | `2cd423a` (kernel claim/release wiring) | `docs/perf/m1/nmpstress-sim-boot.png`; M10.5 fleet `docs/perf/m10.5/ui-fleet/F1–F9` (9 screenshots) + leak-evidence (5 screenshots) |
| `ios/NmpPulse` | 15 | 1,928 | 42 | `9ecaec4` | `docs/perf/pulse/01-05*.png` + `pulse/smoke/00-06*.png` (11 screenshots) |
| `ios/Chirp` | 22 | 3,978 | 24 | `ae38c10` | None in `docs/perf/` |
| `ios/NmpGallery` | 8 | 1,037 | 0 | `2e3121f` | `docs/perf/content-gallery/01-06*.png` (7 screenshots) — but **zero FFI** to kernel, must be data-stubbed |
| `ios/NmpPodcast` | 31 | 5,091 | 0 | `4a7e0f3` | None — Step 0 copy-step scaffold per `m11-podcast.md:35` |
| `ios/NmpHighlighter` | 158 | 39,118 | 0 | `4d7a1e6` | None — verbatim Swift copy, no NMP wiring per §27 row 6 |

**Real NMP-wired iOS apps: 3** (NmpStress, NmpPulse, Chirp). Three "Step 0"
scaffolds (NmpGallery has FFI refs of 0 but has screenshots; NmpPodcast +
NmpHighlighter are pure copy-steps).

DOC DRIFT vs `docs/plan/status.md:10`: status.md mentions only NmpStress as
"live Nostr-connected" — NmpPulse (42 FFI refs, 11 screenshots) and Chirp (24
FFI refs) are also wired and not enumerated in status.md.

---

## 5. Critical-path honest list (top 5 HIGH-severity, dependency-ordered)

Verified against current master + HB43 reconcile (`docs/perf/task-reconcile-and-next-steps-2026-05-18.md`).

**Reorder from the task's suggested order** (T114b → T117 → T116 → T118 →
T119): I list T117 first because the HB41/HB42 anchor explicitly holds
T116/T118/T119 _behind_ T117 — verifying or completing T117 unblocks three
HIGH items at once. T114b stays #2 (independent of T117, closes PD-021).

1. **T117 publish-engine-wire** — per-(event, relay) retry FSM still
   unwired. HB41 dispatch, no publish-side commits since `167d4bc`. Holds
   T116/T118/T119 release per HB41/HB42 anchor. Verify-first action before
   re-dispatch (per HB43 reconcile finding).

2. **T114b retention audit + M10.5 §G-S2 close** — independent of T117;
   closes PD-021 line-11. Evidence already gathered
   (`docs/perf/m10.5/s2-drain-analysis.md`: ~38 MiB net heap allocated under
   flood, 0.13% reclaimed after drain; verdict RETAINED). Gate already has
   `retained_heap_after_drain_bytes ≤ 1 MiB` regression target.

3. **T116 reconnect-replay** — G1 from `docs/research/relay-lifecycle-and-pools.md`;
   substrate-correct, live-wire-ignores-it pattern. Held behind T117 actor
   settle.

4. **T118 app-lifecycle FFI** — G3 from same research doc; no `nmp_app_*`
   foreground/background/suspend signals. Held behind T114 (channel-bound)
   + T117.

5. **T119 NIP-46 transport** — G6 from same research doc; bunker URL
   parses, signing path exists (`crates/nmp-signers`), but transport not
   wired to ConnectionPool. HIGH-product (blocks NIP-46 product flow).

Note: row 5 here is **product severity**, not strict dep-order. If user
prioritizes shipping the publish path before product expansion, T119 swaps
out for **T121 thread-hydration outbox routing** (R1 residual from T105
codex review).

Also gating: **the nmp-cli bin-name collision** (workspace test failure §1)
is a one-line `Cargo.toml` rename — file as quick-win, blocks any honest
"workspace GREEN" claim in README.

---

## Summary

- **Workspace gates: 3/4 PASS, 1 FAIL** (nmp-cli init test; bin-name collision).
- **Total shipped crates: 26** (20 in `crates/`, 6 in `apps/`).
- **Total iOS targets: 6** (3 NMP-wired: Stress/Pulse/Chirp; 3 scaffolds: Gallery/Podcast/Highlighter).
- **Top 5 critical-path:** T117 → T114b → T116 → T118 → T119/T121.
- **Most critical DOC DRIFT:** HB43 "workspace GREEN" claim is wrong (nmp-cli failure); status.md kernel LOC stale (3,800 → 23,674); §27 row 5 stale (`nmp` binary now exists).
