# FFI hardening — CI tiers and M10.5 close protocol (§9)

See [`ci.md`](./ci.md) for the local run recipes (`just stress`,
`just stress-gate-fast`, `just stress-close-gate`) and the gate
script pseudocode.

---

## §C. CI integration

### C.1 Pre-merge tier (every PR)

**Runner.** GitHub Actions `macos-14` (Apple Silicon, ~10 min budget).

**Scenarios.** S1 (short — 60 s), S2 (30 s), S3 (30 s), S5 (30 s),
S7 (full matrix), S8 (60 s). **Not S4** (iOS-main-thread, slow XCUITest
boot) — runs nightly instead. **Not S6** (5 min) — runs nightly.
**Not S9** (10 min) — nightly. **Not S10** (conditional on M3+M4).

**Workflow.** `.github/workflows/stress-pre-merge.yml`:

```yaml
name: FFI stress (pre-merge)
on:
  pull_request:
    paths:
      - 'crates/nmp-core/**'
      - 'crates/nmp-testing/**'
      - 'ios/NmpStress/**'

jobs:
  stress-fast:
    runs-on: macos-14
    timeout-minutes: 15
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: just stress
      - run: just stress-report
      - run: just stress-gate-fast
      - uses: actions/upload-artifact@v4
        if: always()
        with:
          name: stress-pre-merge-${{ github.run_id }}
          path: docs/perf/m10.5/
```

**Gating.** The `just stress-gate-fast` step exit code is the PR gate.
Fast gate checks S1, S2, S3, S5, S7, S8 only — no doctrine review,
no debt-inventory check (both are milestone-close artifacts only).

### C.2 Nightly tier

**Runner.** Mac mini self-hosted runner with an iPhone 12 wired
in. ~90 min budget.

**Scenarios.** All ten at full duration: S1 (10 min), S2 (60 s),
S3 (10 emits × 100 k events), S4 (60 s × 12 stalls), S5 (30 s),
S6 (1 000 cycles), S7 (full matrix), S8 (5 storms), S9 (10 min ×
100/min), S10 (60 s suspend — *only if M3+M4 are complete; the
harness skips with a noted "deferred" if not*).

**Workflow.** `.github/workflows/stress-nightly.yml`:

```yaml
name: FFI stress (nightly)
on:
  schedule:
    - cron: '0 7 * * *'  # 07:00 UTC daily
  workflow_dispatch: {}

jobs:
  stress-full:
    runs-on: [self-hosted, macos, iphone12-attached]
    timeout-minutes: 90
    steps:
      - uses: actions/checkout@v4
      - run: just stress-full
      - run: just stress-sonnet
      - run: just stress-report
      - run: just stress-gate-fast || echo "::warning::nightly gate failed"
      - uses: actions/upload-artifact@v4
        if: always()
        with:
          name: stress-nightly-${{ github.run_id }}
          path: docs/perf/m10.5/
```

**Gating.** Nightly failures emit a GH warning + Slack notification
but do not block merge. They block the M10.5 milestone-close
declaration.

### C.3 On-demand tier (release candidates)

**Trigger.** Manual `workflow_dispatch`, or a git tag matching
`v*-rc*`.

**Scenarios.** Soak versions:
- S1: 8-hour mount/unmount churn.
- S2: 1 M dispatches at 10 k/sec (~ 100 s, repeated for an hour).
- S9: 24-hour relay flap.
- Sonnet-agent: 8 agents × 4-hour parallel run.

**Runner.** Lab device (iPhone 12 + iPhone 16 Pro + dedicated Mac
mini); manual sign-off required.

**Reporting.** Output bundle goes to
`docs/perf/m10.5/rc-<tag>/`. Sign-off recorded in the release notes.

### C.4 Trace-based regression detection

The Rust harness supports `--capture-trace` (records all FFI
calls + timestamps + emit payload hashes) and `--replay <PATH>`
(deterministic replay). One capture per scenario is checked into
`crates/nmp-testing/bin/ffi-stress/traces/` (LFS-tracked). Nightly
replay against the same trace must produce byte-identical metrics
± 5 %; deviations flag a regression even if the gate passes.

This is the same pattern firehose-bench uses (see
`docs/design/firehose-bench.md` §5).

### C.5 What does not block CI

- **Sonnet-agent runs** are advisory. Flake by design; failures are
  triaged manually. Two-or-more agents hitting the same failure in
  one nightly = upgraded to a tracked bug.
- **iPhone 12 hardware-only scenarios** (S9 device variant, S4 device
  variant) skip if the device is detached/offline; the missing
  results are noted in the report and the gate script
  treats "device-absent" as a deferred-not-failed state.
- **S10 if M3+M4 are not complete:** scenario reports as `skipped:
  prereq` with a note in `metrics.json`; gate script omits S10
  from nightly gate check until M3+M4 land. S10 is not used as
  doctrine sign-off evidence for M10.5 (see gates.md §D1 note).

---

## §C.6 CI artifact retention

| Tier | Retention | Notes |
|---|---|---|
| Pre-merge | 14 days | Per-PR; bulk delete |
| Nightly | 90 days | Per-run; archived to S3 quarterly |
| On-demand (RC) | indefinite | Release-attached artifact |

Instruments traces are large (50–500 MiB per scenario). Pre-merge
runs omit `--instruments` to stay within 15 min; only nightly + RC
capture traces.

---

## §C.7 The M10.5 close protocol

1. `just stress-close-gate` exits 0 — runs the full battery
   (S1–S9, excluding S10 while M3+M4 are pending) plus doctrine
   review, debt-inventory, and grep checks.
2. `docs/perf/m10.5/debt-inventory.md` must-fix list = empty.
3. §7.1 grep gate = 0 hits.
4. Doctrine review (D0–D5) signed off in `doctrine-review.md`.
5. iPhone 12 baseline = published in `iphone12-baseline.md` with no
   p99 regression > 5 % vs M10 baseline (plan.md M10.5 exit-gate
   row 2).
6. M11 podcast app scoping begins.

A single broken row in any of 1–5 means M10.5 stays open. There
is no partial close.
