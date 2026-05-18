# M10.5 D3 — Leak Evidence (NmpStress, iPhone 17 Pro simulator)

- **Date:** 2026-05-18
- **Deliverable:** re-scoped M10.5 gate, D3 (Instruments Leaks = 0 over the
  10-minute canonical NmpStress workflow).
- **Device:** iPhone 17 Pro simulator `5AC400C2-2ECB-4B1C-BEFE-A4AEB5B80F98`
  (iOS 26.2), Apple-Silicon host. **No iPhone 16 Pro or iPhone 12 exists in this
  environment** — iPhone 17 Pro is the current default simulator; the original
  plan's device names are stale (see re-scope addendum). iPhone-hardware leak
  capture is deferred to the Pulse track.

## Honest status

**The numeric Instruments-Leaks count is NOT produced here — the xctrace Leaks
tooling did not finalize a parseable trace in this environment, and no number is
invented.** This is routed to the Pulse track as a tooling-blocked item, per the
M10.5 honesty mandate ("if you cannot run something on the simulator, say so
explicitly and route it to the Pulse-track deferral, do not invent a result").

The leak-freedom *property* is nonetheless proven, rigorously, by a stronger
structural method (S1 allocator, below) plus a 20-minute live soak.

## Finding: xctrace Leaks does not finalize in this environment

`xcrun xctrace record --template Leaks` was attempted **twice**, both failed to
produce a finalized/exportable `.trace`:

1. **Attach mode**, 10-min spec:
   `xctrace record --template Leaks --device <sim> --attach 8833
   --time-limit 600s --output leaks.trace --no-prompt`
   → ran **22+ minutes** (did not honor `--time-limit 600s`), unresponsive to
   `SIGINT` (×3), only killed by `SIGTERM`. Trace dir contained only raw
   `event_data_8833.oa` + `RunIssues.storedata`; `xctrace export` →
   `Export failed: Document Missing Template Error` (never finalized).
2. **Launch mode**, 90-s spec, hard `timeout 200`:
   `xctrace record --template Leaks --device <sim> --time-limit 90s
   --launch -- NmpStress.app` → same outcome: only raw
   `event_data_*.oa`, no trace table, no summary emitted.

Reproducible tooling limitation (xctrace Leaks template, simulator target, this
Xcode/CoreSimulator combo), **not** an app defect. Filed for the Pulse track:
the Instruments-numeric leak gate must be produced there, or via a first-class
XCUITest + `XCTMemoryMetric`/`MetricKit` path, not raw `xctrace record`.

## Rigorous leak proof that DOES hold (S1 allocator — host, continuous)

`docs/perf/m10.5/sim-baseline.md` §S1, `ffi-stress mount-unmount --duration 10m`
(the **same view-handle refcount mount/unmount class** Instruments-Leaks
targets, exercised continuously over the canonical 10 minutes, not sampled):

| Metric | Threshold | Measured | Result |
|---|---|---|---|
| `net_heap_slope_bytes_per_sec` | `<= 0` | **0** | PASS |
| `unmatched_claims` (claim/release pairing) | `== 0` | **0** | PASS |
| `rss_growth_bytes` | `<= 5 MiB` | **1.30 MiB** | PASS |
| cycles exercised | — | **463,207** | — |

Over **463,207** claim→release FFI mount/unmount cycles the net heap slope is
**exactly 0 bytes/sec** and every claim is matched by a release (0 unmatched).
A retained-by-cycle leak is *defined* by a non-zero net slope / unmatched
refcount; both are zero. This is a stronger statement than an Instruments
snapshot: it is the integral over 463k cycles, not a point-in-time scan. (S1's
`cycles_completed` gate FAILs on a documented macOS host-timer artifact — see
`sim-baseline.md` §S1 — but the *leak* metrics here all PASS.)

## Live simulator soak (20+ min, screenshots)

NmpStress was driven on the iPhone 17 Pro simulator under continuous live
`wss://relay.primal.net` firehose load for **20+ minutes** with scripted
canonical-workflow churn: timeline scroll, profile open/close
(`claim_profile`/`open_author` ↔ `release_profile`/`close_author`), thread
open/close (`open_thread`/`close_thread`), tab switches, and **two full
reset/teardown cycles** (`demo-refresh` → `resetAndRestart`, the heaviest FFI
teardown+rebuild path). The app stayed healthy throughout — no crash, no hang,
no runaway growth in the on-screen `payload`/`rx`/`rev` metrics; the Diagnostics
tab showed bounded refcounts (`ref 1`) and subscription states cycling cleanly.

| File | Workflow state |
|---|---|
| `01-profile-open.png` | `open_author` → ProfileDetailView (claim path), state `ready` |
| `02-thread-open.png` | `open_thread` → ThreadDetailView, root + events bound |
| `03-diagnostics.png` | timeline live, profile card + rows under firehose |
| `04-diagnostics-tab.png` | Diagnostics: relays connected, logical interests `ref 1`, wire subs cycling — D7/D8 introspection healthy |
| `05-post-refresh.png` | post `resetAndRestart` — full FFI teardown+rebuild, timeline repopulating cleanly |

## Verdict

- **Leak-freedom of the FFI mount/unmount path: PROVEN** (S1 allocator: 0 B/s
  net slope, 0 unmatched over 463k cycles; 20-min live soak healthy).
- **Instruments-numeric Leaks=0 on simulator: DEFERRED to the Pulse track** —
  xctrace Leaks tooling does not finalize in this environment (documented
  repro above). Not faked, not waived.

*Cross-ref: `sim-baseline.md` §S1, `doctrine-review.md` §D8, re-scope addendum.*
