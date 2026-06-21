# Perf Report — ffi-stress — 2026-xx-xx (unix 1779086651)

- **run_id:** `m10.5/s5`
- **started_at_unix:** `1779086651`
- **overall:** **PASS**

## S5-reentrancy — PASS (5/5 gates)

- **wall_seconds:** 30.5

| Gate | Threshold | Measured | Result |
|---|---|---|---|
| deadlocks | == 0.0 | 0.0000 | **PASS** |
| reentrant_dispatches | >= 100.0 | 11959.0000 | **PASS** |
| rev_monotonic | == 1.0 | 1 | **PASS** |
| avg_cb_ms | <= 2.0 | 0.0089 | **PASS** |
| dispatch_loss | == 0.0 | 0.0000 | **PASS** |

### Notes

- Emits: 11959; reentrant dispatches: 11959; deadlocks: 0; avg callback: 0.009 ms
- External watchdog: shared OnceLock<Instant> epoch; CB_IN_FLIGHT_TS_MS=0 means idle; non-zero stores epoch-relative entry ms. Watchdog fires exit(1) when (epoch.elapsed_ms - entry_ms) > 5000 — correctly measures wall-time in flight.
- Reentrant dispatch is fire-and-forget (bible #3): nmp_app_open_author enqueues to actor mpsc channel; does not block listener thread or re-lock any mutex.

### Raw measurements

```json
{
  "avg_cb_ms": 0.008915864955263818,
  "deadlocks": 0,
  "dispatch_loss": 0,
  "emit_count": 11959,
  "inject_count": 200,
  "reentrant_dispatches": 11959,
  "rev_monotonic": true,
  "wall_seconds": 30.508947542,
  "watchdog_fired": false
}
```
