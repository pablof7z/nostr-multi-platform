# 18 — Testing: `nmp-testing`, transport bench, contract tests

The kernel is tested without networking. Every tier below runs deterministically
in CI; real relays appear only in scheduled/manual ignored-test modes. The test tiers are
owned by `crates/nmp-testing/`; the pyramid below is the canonical tier map.

## The test pyramid

```
                    ┌───────────────────────────────┐
   manual           │ humans on reference devices   │  per-milestone checklist
                    ├───────────────────────────────┤
   per-app UI       │ XCUITest / Playwright          │  ios/<app>/UITests/
                    ├───────────────────────────────┤
   ffi-transport    │ UniFFI byte update budget (CI) │  bin/ffi-transport-bench/
                    ├───────────────────────────────┤
   native x-platform│ same scenario, AppState byte=  │  (post-M15)
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
| FFI transport bench | `ffi-transport-bench --standard --fail-on-gate` | `crates/nmp-testing/bin/ffi-transport-bench/main.rs` |

`nmp-testing`'s library surface stays deliberately thin
(`crates/nmp-testing/src/lib.rs`: `store_harness` + `crate_ready()`); the value
is in the transport bench and the `tests/` suites.

## Automatic performance signal

The automatic performance signal after the clean-break gate reset is
`.github/workflows/perf-gates.yml` running
`ffi-transport-bench --standard --fail-on-gate`. It measures the current native
hot path: FlatBuffers update bytes crossing UniFFI as `Vec<u8>` through
`UpdateSink::on_update`.

The workflow runs automatically only for changes to the native byte-transport
owner (`crates/nmp-uniffi`, `crates/nmp-native-runtime`), the transport schema
surface (`crates/nmp-core/src/transport`), the benchmark, dependency manifests,
or the owning workflow/docs. It does not run for broad crate churn or retired
native cleanup artifacts; current native release evidence comes from UniFFI
byte transport and app-shell tests.

The failure threshold is not a historical timing number. The bench's
pre-registered rule fails unless the surcharged weighted-p99 UniFFI-vs-C delta
stays under 5% of a 16.67ms render frame and the UniFFI SMALL batch-mean p99
stays under 250us. A failure means the native byte lane no longer has synthetic
evidence for the UniFFI collapse decision and should be escalated to on-device
measurement or transport redesign.

Manual commands remain:

```bash
cargo run -p nmp-testing --bin ffi-transport-bench --release -- --standard --fail-on-gate
```

## The `test-support` feature gate

Test-only harnesses may need actor internals (`run_actor`, `ActorCommand`,
`spawn_actor`). Production code must not. The gate
(`crates/nmp-core/src/lib.rs:23-56`) is
`#[cfg(any(test, feature = "test-support"))]` so `cargo test` always has access
without a flag, while a normal `nmp-core` consumer cannot reach `testing::`. If
a harness needs the actor, add `features = ["test-support"]` to its dev/bin dep —
never widen the gate.

## Worked example — "I added a feed, source reducer, or projection; what tests do I write?"

Three tiers, in order. Stop at tier 2 unless tier 3's trigger fires.

1. **Unit, in the owning crate's `tests/`.** Reducer/projection invariants over
   synthetic events: feed source events into the reducer, assert the
   materialized child-interest set or projected state. No actor, no network.
   Cover empty-state, one-event, supersede, remove, and fail-closed paths.

2. **Subsystem, in `crates/nmp-testing/tests/`.** Drive the feed/source/ref
   claim through the registry + planner + store against `MockRelay`. Name
   milestone- or domain-prefixed to match the existing convention: `m2_*.rs`
   for planner-touching, `nip29_lifecycle.rs`-style for a protocol crate's
   end-to-end. Assert the *wire frames* the materialized interests produce, not
   just the payload.

3. **Framework-magic contract — only if the feature exercises a contract
   behavior** (source-reducer recompile, account-switch rebind, in-place
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
- **Treating a budget gate as an integration test.** The FFI transport bench
  proves the native byte lane budget; it does not prove product correctness.
  Product behavior belongs in unit, subsystem, or framework-magic tests.
- **Adding `#[test] fn c<N>_*` without the doc-table row** (or vice versa).
  `contract_surface_complete` fails on either side of the drift — and silently
  renaming a contract test breaks the doc↔test correspondence.
- **Skipping the meta-test mental check** when touching the contract — "I'll
  add the doc row in a follow-up" leaves CI red or, worse, the suite
  structurally lying about coverage.
- **Time-based flake.** `sleep`/wall-clock assertions instead of the harness's
  `SimulatedClock` / `advance_clock_ms`. The full contract suite budget is
  <5s deterministic; real time has no place in it.
- **Requiring real relays in PR CI.** Every subsystem test uses `MockRelay`;
  real-relay tests are scheduled/manual ignored rows for public-relay evidence,
  never a PR gate's correctness oracle.

## E2E shell seams — relay override + headless sign-in

Two deterministic seams let CI / device test harnesses point the live app shells
at a local relay (e.g. `nak serve`) and inject a test identity **without
human interaction**. Both follow the thin-shell rule: all parsing and relay
policy live in Rust; the platform shells ferry the raw strings verbatim.

### iOS — environment variables (XCUITest + `xcodebuild` launch args)

| Env var | Format | Behaviour |
|---|---|---|
| `NMP_TEST_NSEC` | `nsec1…` bech32 | Skip keyring restore; sign in with this nsec |
| `NMP_TEST_RELAYS` | `[["ws://…","role"],…]` JSON | Replace default relay bootstrap with this set |

Both are read in `KernelModel.start()`. When `NMP_TEST_RELAYS` is absent, the
shell starts with its app-owned relay configuration. For the external Chirp
consumer, that means `wss://relay.primal.net` with role `"both,indexer"` for
write-capable content proof plus connected discovery, and `wss://purplepag.es`
with role `"indexer"` for an additional discovery lane. When present the JSON
array **entirely replaces** the app defaults — no merging.

Example XCUITest launch arg:

```swift
app.launchEnvironment["NMP_TEST_NSEC"] = "nsec1..."
app.launchEnvironment["NMP_TEST_RELAYS"] = #"[["ws://127.0.0.1:10547","both"]]"#
```

### Web — query parameters (Playwright + local preview)

| Query parameter | Format | Behaviour |
|---|---|---|
| `relay_bootstrap` | JSON array of `[url, role]` pairs, for example `[[\"ws://127.0.0.1:1001\",\"indexer\"],[\"ws://127.0.0.1:1002\",\"both,indexer\"]]` | Replaces the shell's default relay bootstrap while preserving relay roles. Use this for outbox tests that must prove pure indexers stay discovery-only and write-capable relays seed the local publish lane. |
| `relay` | repeated URL param | Legacy one-relay smoke override. Each URL is treated as `both,indexer` so the single fixture can serve discovery and writes. |

### Android — intent extras (`adb shell am start`)

| Extra key | Type | Behaviour |
|---|---|---|
| `nmp.test_nsec` | String | Skip keyring restore; sign in with this nsec |
| `nmp.test_relays` | String | Replace default relay bootstrap with this set |

Both extras are read in `MainActivity.onCreate()` and are **only honoured in
debug builds** (`BuildConfig.DEBUG`). Kotlin passes them verbatim to
`KernelModel.start(context, storagePath, testNsec, testRelays)`, where they ride
on top of the single unconditional launch path (keyring capability + identity
restore run in production too) → `KernelBridge.seedRelays`. Parsing the relay
JSON and seeding relays happens in Rust through the test-only launch path.

Example adb invocation:

```bash
adb shell am start -n org.nmp.android/.MainActivity \
    -e nmp.test_nsec "nsec1..." \
    -e nmp.test_relays '[["ws://127.0.0.1:10547","both"]]'
```

### Relay format

Both platforms share the same JSON shape: a JSON array of two-element string
arrays `[url, role]` where role is one of `"both"`, `"read"`, `"write"`, or
`"indexer"`. Example with a local `nak serve` relay and a remote indexer:

```json
[["ws://127.0.0.1:10547","both"],["wss://purplepag.es","indexer"]]
```

Rust validates each entry through the relay-seeding helper; a malformed entry is
silently skipped (D6). If the entire JSON array is malformed or empty, Rust
falls back to the app-owned reference relay set so the kernel is never left
without any relay.

See also: [06 — Reactivity contract (D8)](06-reactivity-contract.md) · [21 — The framework-magic contract](21-framework-magic.md) · [22 — Doctrine compliance checklist](22-doctrine-checklist.md)
