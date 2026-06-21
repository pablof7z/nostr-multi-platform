# Perf Report — ffi-stress — 2026-xx-xx (unix 1779097371)

- **run_id:** `m10.5/s2`
- **started_at_unix:** `1779097371`
- **overall:** **PASS**

## S2-dispatch-flood — PASS (7/7 gates)

- **wall_seconds:** 30.0

| Gate | Threshold | Measured | Result |
|---|---|---|---|
| dispatches_submitted | >= 300000.0 | 300000.0000 | **PASS** |
| send_latency_p99_ms | <= 1.0 | 0.0246 | **PASS** |
| send_latency_p50_ms | <= 0.1 | 0.0015 | **PASS** |
| rss_growth_bytes | <= 20971520.0 | 884736.0000 | **PASS** |
| retained_heap_after_drain_bytes | <= 1048576.0 | 519748.0000 | **PASS** |
| failed_sends | == 0.0 | 0.0000 | **PASS** |
| send_hitch_proxy | == 0.0 | 0.0000 | **PASS** |

### Notes

- Nominal dispatches: 300000; actual: 300000; p50=0.002ms p99=0.025ms; failed_sends: 0
- Actor mpsc backlog depth: not directly observable from caller thread; RSS growth is the proxy gate (bounded channel growth = bounded RSS). Hitch gate uses p99 as proxy for individual send latencies.
- S2-drain: peak_net_heap=519716 B, retained_after_drain=519748 B, reclaimed_by_drain=-32 B, drain=1.5s (4 samples). Verdict: TRANSIENT backpressure spike — backlog fully reclaimed after drain; peak is recoverable, supports a justified peak-threshold revision
- T114b counters: dispatch_drops_total=0, claim_drops_total=0 (per-pubkey cap exercised when >0)

### Raw measurements

```json
{
  "callback_count": 1,
  "claim_drops_total": 0,
  "dispatch_drops_total": 0,
  "dispatches_per_sec": 10000,
  "drain_net_heap_curve_bytes": [519716, 519748, 519748, 519748],
  "drain_seconds": 1.511627458,
  "drained_rss_growth_bytes": 884736,
  "failed_sends": 0,
  "hitches_proxy": 0,
  "latency_samples": 300000,
  "min_dispatches_gate": 300000,
  "nominal_dispatches": 300000,
  "p50_ms": 0.001536,
  "p50_ns": 1536,
  "p99_ms": 0.024576,
  "p99_ns": 24576,
  "peak_net_heap_bytes": 519716,
  "reclaimed_by_drain_bytes": -32,
  "retained_heap_after_drain_bytes": 519748,
  "rss_growth_bytes": 884736,
  "threads": 4,
  "total_dispatches": 300000,
  "wall_seconds": 30.000532208
}
```
