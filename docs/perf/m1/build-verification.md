# M1/M3 Build Verification — Clean-Build Baseline

**Date:** 2026-05-18  
**HEAD SHA:** `55dd5f25688d55e98c3a06ec4e12ef963be12557`  
**Branch:** `master` (up to date with origin)

## Machine

| Field | Value |
|---|---|
| CPU | Apple M3 Max |
| RAM | 36 GB |
| macOS | 26.5 (build 25F5042g) |
| Xcode | 26.5 (build 17F42) |
| Rust | rustc 1.92.0-nightly (6501e64fc 2025-10-23) |
| Cargo | 1.92.0-nightly (344c4567c 2025-10-21) |
| iOS sim target | aarch64-apple-ios-sim (installed) |

## Git Status

Working tree was clean at HEAD. Only `.claude/` (untracked, not committed) was present.

---

## Gate Results Summary

| # | Gate | Command | Exit | Wall time | Result |
|---|---|---|---|---|---|
| 1 | fmt | `cargo fmt --all -- --check` | 0 | 0.33 s | **PASS** |
| 2 | workspace tests | `cargo test --workspace` | 0 | 4.35 s | **PASS** |
| 3 | codegen check | `cargo run -p nmp-codegen -- gen modules --manifest … --check` | 0 | 0.45 s | **PASS** |
| 4 | reactivity-bench | `cargo run -p nmp-testing --bin reactivity-bench --release -- --standard --fail-on-gate` | 0 | 1.04 s | **PASS** |
| 5 | firehose-bench | `cargo run -p nmp-testing --bin firehose-bench --release -- replay --standard --fail-on-gate` | 0 | 0.68 s | **PASS** |
| 6 | iOS Rust sim build | `cargo build -p nmp-core --target aarch64-apple-ios-sim` | 0 | 1.72 s | **PASS** |
| 7 | iOS Xcode sim build | `mcp__xcode__build_sim` (scheme NmpStress, iPhone 17 / iOS 26.5) | — | — | **PASS** (warnings) |
| 8 | Simulator boot/run | install → launch → screenshot | — | — | **PASS** |

---

## Detailed Command Log

### 1. `cargo fmt --all -- --check`

```
(no output — formatter found no issues)
real 0.33s
EXIT: 0
```

### 2. `cargo test --workspace`

```
Compiling nmp-core, fixture-todo-core, nmp-app-fixture
Finished `test` profile in 2.35s

fixture_todo_core: 2 passed
nmp_codegen: 2 passed (+ 1 integration: generation_is_byte_deterministic)
nmp_core: 5 passed
  - parse_relay_list_splits_nip65_markers
  - profile_claims_are_ui_driven_and_deduped_by_pubkey
  - open_thread_emits_context_and_reply_reqs
  - open_author_emits_profile_and_note_reqs
  - close_author_refcounts_and_closes_view_subscriptions
substrate_registry: 2 passed

Total: 12 unit/integration tests, 0 failed
real 4.35s   EXIT: 0
```

### 3. `cargo run -p nmp-codegen -- gen modules --manifest apps/fixture/nmp.toml --out apps/fixture/nmp-app-fixture --check`

Note: The task spec omitted `--manifest` and `--out` flags. The canonical invocation comes from the justfile `gen-modules-check` recipe. That full form was used.

```
nmp gen modules --check: ok
real 0.45s   EXIT: 0
```

### 4. `reactivity-bench --standard --fail-on-gate`

Note: `--fail-on-gate` IS wired (verified in `crates/nmp-testing/bin/reactivity-bench/config.rs`). `--no-write-report` added to suppress disk write during verification.

All 7 scenarios passed all gates:

| Scenario | lookup p99 ns | recompute p99 ns | view_batch_hz | false_wakeup_rate | mem (MiB) |
|---|---|---|---|---|---|
| quiet_idle | 84 / 100 000 | 42 / 1 000 000 | 0.10 / 60 | 0.0 / 0.10 | 2.6 / 100 |
| following_timeline_scroll | 84 | 84 | 39.78 | 0.0 | 4.0 |
| hashtag_firehose | 42 | 42 | 58.9 | 0.0 | 17.3 |
| profile_fanout | 333 | 2833 | 50.0 | 0.0 | 3.9 |
| thread_blowup | 42 | 42 | 55.6 | 0.0 | 1.7 |
| account_switch | 83 | 42 | 1.0 | 0.0 | 2.6 |
| working_set_100_views | 208 | 125 | 50.0 | 0.0 | 18.9 |

```
overall_passed: true
real 1.04s   EXIT: 0
```

### 5. `firehose-bench replay --standard --fail-on-gate`

Note: `--fail-on-gate` IS wired (verified in `crates/nmp-testing/bin/firehose-bench/config.rs`). Mode positional arg (`replay`) placed before flags. `--no-write-report` added.

All 8 scenarios passed all gates:

| Scenario | Key gate | Measured | Budget |
|---|---|---|---|
| cold_start | first_item_ms | 60 | 800 |
| cold_start | filled_timeline_ms | 740 | 5 000 |
| cold_start | peak_memory_mb | 56 | 200 |
| sustained_firehose | ingest_to_emit_p99_ms | 25.6 | 50 |
| sustained_firehose | view_batch_hz | 58.0 | 60 |
| sustained_firehose | memory_drift_mb | 33.0 | 50 |
| profile_thrashing | open_close_dispatch_rate | 22.5 | 30 |
| profile_thrashing | leaked_subscriptions | 0 | 0 |
| relay_disconnect_storm | reconnect_p99_ms | 18 500 | 30 000 |
| multi_account | cross_account_bleed | 0 | 0 |
| negentropy_efficiency | nip77_bytes_ratio | 0.036 | 0.05 |
| background_decryption | decrypt_p99_ms | 3.01 | 200 |
| soak | memory_growth_mb | 38 | 100 |
| soak | fd_growth | 0 | 0 |

```
overall_passed: true
status: "prototype"   (all scenarios are deterministic simulations — see Limitations below)
real 0.68s   EXIT: 0
```

### 6. `cargo build -p nmp-core --target aarch64-apple-ios-sim`

```
Compiling nmp-core v0.1.0
Finished `dev` profile in 1.67s
real 1.72s   EXIT: 0
```

### 7. iOS Xcode Simulator Build

- Simulator chosen: **iPhone 17 / iOS 26.5** (UUID `C380BA01-AD22-4F4A-8524-A309DA15144C`)
- Note: Task requested iPhone 16 Pro, which is not available on this machine. Closest match used is iPhone 17 (iOS 26.5), which also matches the justfile `build-ios` recipe destination.
- Scheme: `NmpStress`
- DerivedData: `ios/DerivedData`

**Linker warnings (non-fatal):** The Rust static library (`libnmp_core.a`) was compiled with `IPHONEOS_DEPLOYMENT_TARGET=26.5` (Rust default on this SDK), while the Xcode project's deployment target is `17.0`. This produces ~20 `ld: warning: object file … was built for newer 'iOS-simulator' version (26.5) than being linked (17.0)` messages for cryptographic assembly files from the `ring` crate.

**Result:** Build succeeded despite warnings. App bundle at:  
`ios/DerivedData/Build/Products/Debug-iphonesimulator/NmpStress.app`  
Bundle ID: `com.example.NmpStress`

### 8. Simulator Boot / Install / Launch / Screenshot

- Simulator was already booted (iPhone 17 / iOS 26.5, UUID `C380BA01-…`)
- `install_app_sim`: success
- `launch_app_sim`: success
- Screenshot taken after 10 s settle time

**Screenshot:** `docs/perf/m1/nmpstress-sim-boot.png`

App displayed:
- Status: **CONNECTED** to `wss://relay.primal.net`
- Events: 91 visible, 80 on screen
- Profile loaded: **PABLOF7z** (`_@f7z.io`)
- Timeline items rendering with live relay data
- Key latencies: RELAY→RUST 85 ms, CB→SCREEN 1752 µs, MAX APPLY 142 µs

---

## Limitations / Caveats

1. **firehose-bench is a prototype harness.** All scenarios run deterministic simulations — real relay sockets, LMDB/SQLite writes, UniFFI marshaling, and NSE calls are modeled, not measured. Gates validate planned budgets, not real I/O behavior.

2. **Linker version mismatch warning.** The `ring` crate's assembly objects target iOS-sim 26.5 while the Xcode project deploys to 17.0. This should be aligned by either pinning `IPHONEOS_DEPLOYMENT_TARGET` in the Cargo build (via `RUSTFLAGS`) or by raising the Xcode project's minimum deployment target. Not a blocking failure today.

3. **iPhone 16 Pro not available.** Only iPhone 17 simulators are present on this machine. The justfile also targets iPhone 17 / iOS 26.5.

4. **No iPhone 16 Pro (M0/M1 context).** The task title says "M0/M1 build" but the machine is an M3 Max. Results represent the M3 Max baseline; an actual M1 run would be slower on compile times but gate measurements should be identical (deterministic simulation).

---

## Failures

None. All 8 gates passed.

---

## Follow-up Actions

| Priority | Item |
|---|---|
| P1 | Align iOS deployment target: set `IPHONEOS_DEPLOYMENT_TARGET=17.0` in Rust build script (or justfile `rust-ios-sim` recipe via `CARGO_ENCODED_RUSTFLAGS`) to eliminate linker warnings |
| P2 | Run firehose-bench in `live` mode once real relay adapter + storage backend exist |
| P3 | Run verification on actual M1 hardware to get compile-time baseline for that chip class |
| P4 | Add `reactivity-bench` and `firehose-bench` to CI (already `--fail-on-gate` wired) |
