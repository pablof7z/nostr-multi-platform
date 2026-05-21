# 18 — Testing: `nmp-testing`, benches, contract tests

The kernel is tested without networking. Every tier below runs deterministically
in CI; real relays appear only in opt-in live-bench modes. Source of truth for
the tier map: [`docs/plan/test-pyramid.md`](../plan/test-pyramid.md).

## The test pyramid

```
                    ┌───────────────────────────────┐
   manual           │ humans on reference devices   │  per-milestone checklist
                    ├───────────────────────────────┤
   per-app UI       │ XCUITest / Playwright          │  ios/<app>/UITests/
                    ├───────────────────────────────┤
   firehose-bench   │ replay (CI) · live (real iOS)  │  bin/firehose-bench/
   reactivity-bench │ composite index · alloc gates  │  bin/reactivity-bench/
                    ├───────────────────────────────┤
   cross-platform   │ same scenario, AppState byte=  │  (post-M15)
   cross-FFI        │ binding round-trip             │  (post-M14)
                    ├───────────────────────────────┤
   subsystem        │ store+planner+sync vs MockRelay│  nmp-testing/tests/
                    ├───────────────────────────────┤
   unit             │ pure fns · trait invariants    │  <crate>/tests/
                    └───────────────────────────────┘
```

Cutting *across* the pyramid: the **framework-magic contract** — 13 behavior
tests (C1–C13) + 1 coverage meta-test = 14 `#[test] fn` in
`crates/nmp-testing/tests/framework_magic_contract.rs`. It is the only test file
in `nmp-testing/tests/` that is **not** milestone-prefixed, on purpose: no single
milestone owns the contract.

| Tier | Tooling | Where |
|---|---|---|
| Unit | `cargo test -p <crate>` | each crate's `tests/` |
| Subsystem | `cargo test -p nmp-testing --test '*'` | `crates/nmp-testing/tests/` |
| Reactivity bench | `reactivity-bench --standard --fail-on-gate` | `crates/nmp-testing/bin/reactivity-bench/main.rs` |
| Firehose bench | `firehose-bench replay --standard --fail-on-gate` | `crates/nmp-testing/bin/firehose-bench/main.rs` |
| FFI stress (M10.5) | `ffi-stress` + Chirp smoke/UI tests | `crates/nmp-testing/bin/ffi-stress/main.rs` |

`nmp-testing`'s library surface stays deliberately thin
(`crates/nmp-testing/src/lib.rs`: `store_harness` + `crate_ready()`); the value
is in the `bin/` benches and the `tests/` suites.

## The `test-support` feature gate

Live-bench binaries need the actor internals (`run_actor`, `ActorCommand`,
`spawn_actor`). Production code must not. The gate
(`crates/nmp-core/src/lib.rs:23-56`) is
`#[cfg(any(test, feature = "test-support"))]` so `cargo test` always has access
without a flag, while a normal `nmp-core` consumer cannot reach `testing::`. If
a bench needs the actor, add `features = ["test-support"]` to its dev/bin dep —
never widen the gate.

## Worked example — "I added a ViewModule, what tests do I write?"

Three tiers, in order. Stop at tier 2 unless tier 3's trigger fires.

1. **Unit, in the owning crate's `tests/`.** Projection invariants over
   synthetic events: open the view, feed events, assert the snapshot/delta.
   Model on `crates/nmp-core/tests/substrate_registry.rs` (registers a module,
   asserts registry shape). No actor, no network. Cover empty-state,
   one-event, supersede, and remove paths — these map to the
   `on_event_inserted` / `on_event_replaced` / `on_event_removed` callbacks
   the view implements.

2. **Subsystem, in `crates/nmp-testing/tests/`.** Drive the view through the
   planner + store against `MockRelay`. Name milestone- or domain-prefixed to
   match the existing convention: `m2_*.rs` for planner-touching,
   `nip29_lifecycle.rs`-style for a protocol crate's end-to-end. Assert the
   *wire frames* the view's `dependencies()` produce, not just the payload.

3. **Framework-magic contract — only if the view exercises a contract
   behavior** (kind:3-driven recompile, account-switch rebind, in-place
   placeholder refinement, …). Then follow the recipe below. A plain typed
   projection that does none of those does **not** get a contract test;
   forcing one inflates the suite and dilutes the meta-test.

## Framework-magic test naming convention

- File: `crates/nmp-testing/tests/framework_magic_contract.rs` plus per-chapter
  sub-files under `framework_magic_contract/` (`c1_c4_c6_c9.rs`,
  `c5_c8_c13.rs`, `c7_c11.rs`, `c10.rs`, `c12.rs`), each ≤300 LOC.
- Behavior tests are `c<N>_<snake_summary>`, e.g.
  `c5_kind3_change_recompiles_follow_dependent_subs`. The number is the
  contract bullet; the suffix is the asserted behavior.
- Test names are **stable identifiers**. Renaming one is a contract revision:
  keep a shim `fn old_name() { c5_new() }` for at least one milestone cycle.
- The meta-test is `contract_surface_complete` — never `#[ignore]`, runs every
  CI run. Pending-milestone behavior tests are `#[ignore = "pending M_n"]`
  (the reason **must** name the milestone so
  `grep "pending M" framework_magic_contract.rs` is a per-milestone checklist);
  the meta-test still counts them — it asserts structural correspondence, not
  readiness.

Canon for the names: `docs/design/framework-magic.md:46-63` and
`docs/design/framework-magic/test-scaffolding.md` §1–§5.

## Recipe — where to add a contract bullet (C14)

`contract_surface_complete` parses the table in
`docs/design/framework-magic.md` and the `EXPECTED_TESTS` const and asserts
*all three* agree (doc rows, const list, `#[test] fn`s). To add C14 without
breaking the build, change all three plus an ADR — in one PR:

1. **ADR.** Bullet-count changes need a decision record
   (`framework-magic.md` line 71). Write `docs/decisions/00NN-*.md` first; it
   names the owning milestone and doctrine the bullet discharges.
2. **Doc table.** Add the `| C14 | … | sub-file | `c14_<summary>` | [PENDING
   M_n] | D… |` row to `docs/design/framework-magic.md` and the numbered list
   at lines 48–61.
3. **Test fn.** Add `#[test] fn c14_<summary>()` to the right
   `framework_magic_contract/` sub-file (create one if a cluster fills 300
   LOC). `#[ignore = "pending M_n"]` if its milestone is open.
4. **Const.** Add `"c14_<summary>"` to `EXPECTED_TESTS` in
   `framework_magic_contract.rs` (and the mirror list in
   `test-scaffolding.md` §4).

Skip any of 2–4 and `contract_surface_complete` fails locally before the PR
lands — by design. The milestone exit-gate "framework-magic delta" subsection
records which `#[ignore]` lines you removed.

## Anti-patterns

- **Platform tests for Rust logic.** Asserting kernel behavior through
  XCUITest/Swift instead of a `nmp-testing` subsystem test — slow, flaky,
  tests the bridge not the logic. Push the assertion down the pyramid.
- **Treating a bench as an integration test.** `reactivity-bench` /
  `firehose-bench` are *budget contracts* (alloc gates, delta budgets), not
  correctness suites. A green bench does not mean the view is correct; a unit
  test does.
- **Adding `#[test] fn c<N>_*` without the doc-table row** (or vice versa).
  `contract_surface_complete` fails on either side of the drift — and silently
  renaming a contract test breaks the doc↔test correspondence.
- **Skipping the meta-test mental check** when touching the contract — "I'll
  add the doc row in a follow-up" leaves CI red or, worse, the suite
  structurally lying about coverage.
- **Time-based flake.** `sleep`/wall-clock assertions instead of the harness's
  `SimulatedClock` / `advance_clock_ms`. The full contract suite budget is
  <5s deterministic; real time has no place in it.
- **Requiring real relays in CI.** Every subsystem test uses `MockRelay`; live
  modes (`firehose-bench live`) are opt-in and run against real relays only
  for on-device evidence, never as a gate's correctness oracle.

See also: [06 — Reactivity contract (D8)](06-reactivity-contract.md) · [21 — The framework-magic contract](21-framework-magic.md) · [22 — Doctrine compliance checklist](22-doctrine-checklist.md)
