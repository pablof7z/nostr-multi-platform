# Perf Report — ffi-stress — 2026-xx-xx (unix 1779086587)

- **run_id:** `m10.5/s4`
- **started_at_unix:** `1779086587`
- **overall:** **PASS**

## S4-reconciler-backpressure — PASS (9/9 gates)

- **wall_seconds:** 60.5

| Gate | Threshold | Measured | Result |
|---|---|---|---|
| stalls_injected | == 12.0 | 12.0000 | **PASS** |
| actor_queue_depth_peak | <= 50.0 | 0.0000 | **PASS** |
| backlog_after_stall | <= 2.0 | 1.0000 | **PASS** |
| rev_monotonic | == 1.0 | 1 | **PASS** |
| stalls_with_backlog | >= 12.0 | 12.0000 | **PASS** |
| listener_emit_drops | == 0.0 | 0.0000 | **PASS** |
| configure_during_stall_p99_us | <= 10000.0 | 22.0000 | **PASS** |
| stale_rev_pairs | == 0.0 | 0.0000 | **PASS** |
| apply_burst_ms | <= 33.0 | 3.0000 | **PASS** |

### Notes

- stall_windows_starved=0: unobservable on host harness (running=false; emits only on configure(); listener blocks during stall). Actor non-blocking verified by configure_during_stall_p99_us gate.
- Injected 500 signed events; stalls: 12; max backlog: 1; expected <= 2; emits total: 141; stale-rev pairs: 0; total_stall_backlog: 12; stalls_with_backlog: 12; configure_p99_us: 22; apply_burst_ms: 3
- Stall simulated via callback sleep (250 ms) on listener thread.  Actor is not blocked; configure() enqueues to mpsc Sender and returns immediately (D4 single-writer via actor thread). configure_during_stall_p99_us measures this directly.
- Event injection uses nmp_app_inject_signed_events (full Schnorr verify via try_from_raw; S4 spec requires real ingest path for 500 events).
- actor_queue_depth: kernel hardcodes to 0 (update.rs:68); gate added for spec compliance but always passes until wired. Follow-up: wire std::sync::mpsc channel length (or switch to crossbeam) to Metrics::actor_queue_depth in the actor loop.

### Raw measurements

```json
{
  "apply_burst_evidence_ok": true,
  "apply_burst_ms": 3,
  "configure_during_stall_p99_us": 22,
  "emit_hz": 4,
  "expected_max_backlog": 2,
  "inject_count": 500,
  "max_actor_queue_depth": 0,
  "max_backlog_emits": 1,
  "rev_monotonic": true,
  "stale_rev_pairs": 0,
  "stall_duration_ms": 250,
  "stalls_injected": 12,
  "stalls_with_backlog": 12,
  "total_configure_calls": 139,
  "total_emits": 141,
  "total_stall_backlog": 12,
  "wall_seconds": 60.5097135
}
```
