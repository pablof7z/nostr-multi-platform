# FFI hardening — harness architecture (§4)

Three runner paths share one report schema. The Rust harness drives the
FFI symbols directly (fastest iteration). The XCUITest target exercises
the real `NmpStress.app` on simulator + device (only path that catches
iOS-main-thread bugs). The Sonnet-agent runner produces screenshots and
unscripted UI exercises that catch what scripted tests miss.

---

## 1. Rust-side harness — `crates/nmp-testing/bin/ffi-stress/`

### 1.1 Layout

Modeled directly on `crates/nmp-testing/bin/firehose-bench/` (already
exists, 4 files: `main.rs`, `config.rs`, `report.rs`, `scenarios.rs`):

```
crates/nmp-testing/bin/ffi-stress/
├── main.rs                  # mode dispatch via subcommand
├── config.rs                # CLI parsing, scenario selection
├── report.rs                # JSON metrics + markdown report writer
├── allocator.rs             # counting allocator (vendored from reactivity-bench)
├── mock_relay.rs            # in-process flap-able WebSocket (S9)
├── scenarios/
│   ├── mod.rs               # scenario registry + dispatcher
│   ├── mount_unmount.rs     # S1
│   ├── dispatch_flood.rs    # S2
│   ├── snapshot_pressure.rs # S3
│   ├── reentrancy.rs        # S5
│   ├── lifecycle_storm.rs   # S6
│   ├── error_exhaustion.rs  # S7
│   ├── planner_dos.rs       # S8
│   ├── relay_flap.rs        # S9
│   └── long_suspend.rs      # S10 (conditional on M3+M4)
└── sonnet-runner.sh         # shell driver for the agent fleet (§3)
```

(S4 reconciler back-pressure is iOS-main-thread-only; lives in
StressUITests, not here.)

### 1.2 CLI shape

```
ffi-stress <scenario> [options]

scenarios:
  mount-unmount        S1
  dispatch-flood       S2
  snapshot-pressure    S3
  reentrancy           S5
  lifecycle-storm      S6
  error-exhaustion     S7
  planner-dos          S8
  relay-flap           S9
  long-suspend         S10 (skipped unless --experimental-suspend)
  all                  run every gated scenario

options:
  --duration <D>       wall-clock duration (e.g. 10m, 60s); default per scenario
  --threads <N>        caller-thread count (S2 default 4, others 1)
  --rate <R>           operations per second (default per scenario)
  --target <T>         sim | device | none — for trace/report tagging only
  --report-dir <PATH>  default: docs/perf/m10.5/<scenario>/
  --fail-on-gate       exit 2 if any gate fails
  --capture-trace      record FFI call log + emit log for replay
  --replay <PATH>      deterministic replay against a captured trace
  --instruments        on macOS, spawn `xctrace record` for the duration
```

### 1.3 Scenario module shape

Each scenario is a `fn run(cfg: &Config, report: &mut ScenarioReport)`
with the same signature so the dispatcher in `scenarios/mod.rs` stays
trivial. Sketch:

```rust
// crates/nmp-testing/bin/ffi-stress/scenarios/mount_unmount.rs
use crate::report::{Gate, ScenarioReport};
use nmp_testing::ffi_stress::CountingAllocator;
use std::ffi::{c_void, CString};
use std::time::{Duration, Instant};

pub fn run(cfg: &super::Cfg, report: &mut ScenarioReport) {
    let app = unsafe { nmp_core_ffi::nmp_app_new() };
    let ctx = Box::into_raw(Box::new(SinkCtx::default())) as *mut c_void;
    unsafe {
        nmp_core_ffi::nmp_app_set_update_callback(app, ctx, Some(sink_cb));
        nmp_core_ffi::nmp_app_start(app, 0, 80, 4);
    }

    let pubkeys = generate_test_pubkeys(100);
    let start = Instant::now();
    let mut cycles = 0u64;
    let baseline_rss = process_rss_bytes();

    while start.elapsed() < cfg.duration {
        let pk = &pubkeys[cycles as usize % pubkeys.len()];
        let consumer = format!("ffi-stress-{}", cycles);
        let pk_c = CString::new(pk.as_str()).unwrap();
        let cn_c = CString::new(consumer).unwrap();
        unsafe {
            nmp_core_ffi::nmp_app_claim_profile(app, pk_c.as_ptr(), cn_c.as_ptr());
        }
        std::thread::sleep(Duration::from_millis(1));
        unsafe {
            nmp_core_ffi::nmp_app_release_profile(app, pk_c.as_ptr(), cn_c.as_ptr());
        }
        cycles += 1;
        if cycles % 1000 == 0 {
            cfg.pacer.wait_for_next_second();
        }
    }

    let final_rss = process_rss_bytes();
    let rss_growth = final_rss.saturating_sub(baseline_rss);

    report.gates.push(Gate::numeric(
        "rss_growth_bytes",
        rss_growth as f64,
        op: "<=",
        threshold: 5 * 1024 * 1024,
    ));
    report.gates.push(Gate::numeric(
        "cycles_completed",
        cycles as f64,
        op: ">=",
        threshold: cfg.duration.as_secs() * 1000 * 90 / 100, // 90% of nominal
    ));

    unsafe {
        nmp_core_ffi::nmp_app_set_update_callback(app, std::ptr::null_mut(), None);
        nmp_core_ffi::nmp_app_free(app);
        drop(Box::from_raw(ctx as *mut SinkCtx));
    }
}
```

The shape mirrors the existing firehose-bench scenarios — same `Gate`
abstraction, same `ScenarioReport` builder, same JSON output.

### 1.4 Counting allocator

Vendored from `crates/nmp-testing/bin/reactivity-bench/allocator.rs`
(ADR-0004). Installed as `#[global_allocator]` in the harness binary
only (not in `nmp-core`). Used by S1, S2, S3, S6, S8 to detect heap
growth without Instruments.

### 1.5 Mock relay for S9 (relay flap)

A minimal in-process WebSocket server that accepts a connection,
serves canned events, and can be `kill()`-ed externally on a schedule.
Lives in `mock_relay.rs`. Reuses `nostr-relay-builder` if available;
otherwise a hand-rolled `tungstenite::accept` loop is enough for the
flap test (we're testing reconnection logic, not protocol fidelity).

### 1.6 Linking

`nmp-core` already compiles as `cdylib + staticlib + rlib`. The
harness binary depends on it as an rlib through a new
`nmp-core-ffi-decls` crate that re-exports the `extern "C"` symbols
as Rust function declarations:

```rust
// crates/nmp-core-ffi-decls/src/lib.rs
extern "C" {
    pub fn nmp_app_new() -> *mut std::ffi::c_void;
    pub fn nmp_app_free(app: *mut std::ffi::c_void);
    pub fn nmp_app_set_update_callback(
        app: *mut std::ffi::c_void,
        context: *mut std::ffi::c_void,
        callback: Option<extern "C" fn(*mut std::ffi::c_void, *const std::ffi::c_char)>,
    );
    // ... (rest of the 14 declarations)
}
```

(Alternative: use `nmp-core` directly as a crate dep and avoid the
extern declarations. Either works; the extern-decl path matches what
Swift sees more faithfully and surfaces ABI-mismatch bugs.)

---

## 2. iOS XCUITest target — `ios/NmpStress/StressUITests/`

### 2.1 Layout

New target alongside the existing `NmpStressUITests/`:

```
ios/NmpStress/StressUITests/
├── StressUITests.swift             # target entry, shared helpers
├── S1MountUnmountChurn.swift
├── S2DispatchFlood.swift
├── S3SnapshotPressure.swift
├── S4ReconcilerBackpressure.swift  # iOS-only
├── S5Reentrancy.swift
├── S6LifecycleStorms.swift
├── S7ErrorExhaustion.swift
├── S8PlannerDOS.swift
├── S9RelayFlap.swift               # nightly device only
└── S10LongSuspend.swift            # conditional on M3+M4
```

The existing `NmpStressUITests/NmpStressUITests.swift` (102 LOC) is
kept for the boot-render smoke test; the StressUITests target is for
the harness.

### 2.2 Test class skeleton

```swift
import XCTest

@MainActor
final class S1MountUnmountChurn: XCTestCase {
    func testMountUnmountChurn_10Min() throws {
        let app = XCUIApplication()
        app.launchEnvironment["NMP_STRESS_SCENARIO"] = "S1"
        app.launchEnvironment["NMP_STRESS_DURATION_SEC"] = "600"
        app.launchEnvironment["NMP_VISIBLE_LIMIT"] = "80"
        app.launchEnvironment["NMP_EMIT_HZ"] = "4"
        app.launch()

        // The app, when launched with NMP_STRESS_SCENARIO=S1, runs the
        // mount/unmount churn from inside the Swift bridge, exercising
        // the real KernelHandle pathway (not just the C ABI).
        let metricsExporter = app.staticTexts["stress-metrics-exporter"]
        XCTAssertTrue(metricsExporter.waitForExistence(timeout: 620))

        let payload = metricsExporter.label  // JSON blob
        let metrics = try JSONDecoder().decode(StressMetrics.self,
                                                from: Data(payload.utf8))

        XCTAssertEqual(metrics.unmatchedClaims, 0)
        XCTAssertLessThanOrEqual(metrics.rssGrowthBytes, 5 * 1024 * 1024)
        XCTAssertEqual(metrics.instrumentsLeakCount, 0,
                       "Instruments-Leaks must be 0 — see Instruments.trace bundle")
    }
}
```

The pattern: the `NmpStress` app honors a `NMP_STRESS_SCENARIO`
launch-env that puts it in a driven mode (no human interaction
needed), runs the scenario, then exposes a JSON metrics blob as an
accessibility label the XCUITest can read. This is the same pattern
the existing `NmpStressUITests` already uses for its
`relay-state-value` / `metric-events-value` accessibility-ID
exposures (NmpStressUITests.swift:13–28).

### 2.3 Performance metrics

Use `XCTMetric` for what XCUITest measures natively:

- `XCTHitchMetric` — main-thread hitches (S2, S3, S4).
- `XCTClockMetric` — wall time (S1, S6).
- `XCTMemoryMetric` — RSS sample (S1, S3, S8).
- `XCTCPUMetric` — CPU usage (S2, S3).
- `XCTApplicationLaunchMetric` — cold-start (used by the M1–M10
  perf reruns, not by these scenarios directly).

Instruments-Leaks integration is via `xcrun xctrace record --template
'Leaks' --launch -- /path/to/NmpStress.app`, captured by the
harness shell script that drives `xcodebuild test` (see ci.md §3).

---

## 3. Sonnet-agent runner — `sonnet-runner.sh`

### 3.1 What it does

Spawns **N parallel `claude` agent processes**, each given a system
prompt that scopes them to a single user flow (e.g., "open the
profile of pubkey X, scroll, tap a thread, return"). Each agent
drives the simulator via the `mcp__xcode__*` tool family
(`boot_sim`, `launch_app_sim`, `tap`, `swipe`, `screenshot`,
`describe_ui`, `stop_app_sim`).

The point is to catch what scripted UI tests miss: real human-shaped
interleavings, unexpected tap targets, race conditions between gesture
and emit, and visual regressions that XCTAssert can't see.

### 3.2 Concrete invocation sketch

```bash
#!/usr/bin/env bash
# crates/nmp-testing/bin/ffi-stress/sonnet-runner.sh
# Usage: ./sonnet-runner.sh <scenario> <parallel-agent-count> <duration-min>

set -euo pipefail

SCENARIO="${1:-default}"
N="${2:-4}"
DURATION_MIN="${3:-5}"

REPORT_DIR="docs/perf/m10.5/sonnet/${SCENARIO}-$(date +%s)"
mkdir -p "$REPORT_DIR"

# Boot a fresh simulator
SIM_ID=$(xcrun simctl list devices available | grep "iPhone 16 Pro" \
  | head -1 | grep -oE '[A-F0-9-]{36}')
xcrun simctl boot "$SIM_ID" || true
xcrun simctl install "$SIM_ID" ios/DerivedData/Build/Products/Debug-iphonesimulator/NmpStress.app

# Spawn N agents in parallel
for i in $(seq 1 "$N"); do
  AGENT_DIR="$REPORT_DIR/agent-$i"
  mkdir -p "$AGENT_DIR/screenshots"
  (
    claude --print --output-format=json \
      --max-turns 200 \
      --append-system-prompt "$(cat <<EOF
You are a stress-testing agent for the NmpStress iOS app. You have $DURATION_MIN
minutes to exercise the app via mcp__xcode__* tools on simulator $SIM_ID.
Bundle ID: com.example.NmpStress. Goal: stress the FFI surface by mounting
and unmounting profile views via aggressive tap/back navigation. After every
5 actions, call mcp__xcode__screenshot and save the output. Append every
assertion (UI element present? rev increased? no error toast?) to
$AGENT_DIR/assertions.log. Stop after $DURATION_MIN minutes wall-clock.
EOF
)" \
      "Begin stress run #$i for scenario $SCENARIO" \
      > "$AGENT_DIR/transcript.json"
  ) &
done

wait

# Aggregate
python3 scripts/sonnet-aggregate.py "$REPORT_DIR" \
  > "$REPORT_DIR/aggregate-report.md"
```

### 3.3 Output bundle (per agent)

```
docs/perf/m10.5/sonnet/<scenario>-<unix-ts>/
├── agent-1/
│   ├── transcript.json       # full Claude conversation
│   ├── assertions.log        # one line per assertion: PASS/FAIL <description>
│   └── screenshots/
│       ├── 0001.png
│       ├── 0002.png
│       └── ...
├── agent-2/ ...
└── aggregate-report.md       # union of assertions, screenshot grid
```

### 3.4 Why this is separate from XCUITest

XCUITest assertions are scripted and deterministic. Sonnet agents
make unscripted choices. They will find:
- UI states the scripted test didn't think to navigate to;
- gesture sequences that exercise FFI corners the scripted test
  doesn't reach;
- visual regressions (the screenshot trail is human-reviewable).

Trade-off: non-determinism makes flaky CI. Mitigation: nightly only,
not pre-merge; treated as advisory unless multiple agents in one
run hit the same failure.

### 3.5 Number of agents

Default **N=4** (matches a single iPhone 16 Pro simulator's
comfortable concurrency budget — multiple sims is overkill for
M10.5). N up to 8 in nightly runs on the Mac mini self-hosted
runner where multiple simulators can boot in parallel.

---

## 4. Shared report schema

All three runners produce `metrics.json` with the same schema so the
aggregation in `docs/perf/m10.5/` is uniform:

```json
{
  "schema_version": "1",
  "scenario": "S1",
  "runner": "rust-harness" | "xcuitest" | "sonnet",
  "device": "iPhone 16 Pro Simulator" | "iPhone 12" | "macOS-host",
  "started_at_unix": 1779100000,
  "duration_sec": 600,
  "passed": true,
  "gates": [
    {"name": "rss_growth_bytes", "value": 3145728, "op": "<=", "threshold": 5242880, "passed": true},
    {"name": "unmatched_claims", "value": 0, "op": "==", "threshold": 0, "passed": true}
  ],
  "metrics": { /* scenario-specific KV pairs */ },
  "limitations": [],
  "observations": []
}
```

The schema matches the existing `firehose-bench` `FirehoseReport`
shape (see `crates/nmp-testing/bin/firehose-bench/report.rs`) so the
aggregator script reuses the same parser.

---

## 5. What's intentionally out of scope

- **Network simulation.** No `tc netem`-style packet loss; the mock
  relay's flap behavior is binary on/off. Realistic-loss scenarios
  are deferred.
- **iOS background-extension stress.** NSE decryption load (firehose
  §3.7) is a separate harness, not part of M10.5.
- **Multi-account.** S5/S6 use a single account; multi-account
  concurrent stress is firehose §3.5.
- **Cross-platform.** Android / desktop / web FFI surfaces are not
  exercised; M10.5 is iOS-only.
