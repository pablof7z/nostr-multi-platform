# FFI hardening — Sonnet-agent runner, report schema, out-of-scope (§4)

See [`harness.md`](./harness.md) for the Rust harness (§1) and
iOS XCUITest target (§2).

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
  "runner": "rust-harness",
  "device": "iPhone 16 Pro Simulator",
  "started_at_unix": 1779100000,
  "duration_sec": 600,
  "passed": true,
  "gates": [
    {"name": "rss_growth_bytes", "value": 3145728, "op": "<=", "threshold": 5242880, "passed": true},
    {"name": "unmatched_claims", "value": 0, "op": "==", "threshold": 0, "passed": true}
  ],
  "metrics": {},
  "limitations": [],
  "observations": []
}
```

`runner` is one of `"rust-harness"`, `"xcuitest"`, or `"sonnet"`.
`device` is one of `"iPhone 16 Pro Simulator"`, `"iPhone 12"`, or
`"macOS-host"`.

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
