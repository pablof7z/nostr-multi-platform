# Perf Report — ffi-stress — 2026-xx-xx (unix 1779086489)

- **run_id:** `m10.5/s3`
- **started_at_unix:** `1779086489`
- **overall:** **PASS**

## S3-snapshot-pressure — PASS (6/6 gates)

- **wall_seconds:** 91.9

| Gate | Threshold | Measured | Result |
|---|---|---|---|
| callback_p99_ms | <= 20.0 | 0.0390 | **PASS** |
| max_payload_bytes | <= 2097152.0 | 490038.0000 | **PASS** |
| apply_us_p99 | <= 16000.0 | 38.0000 | **PASS** |
| emit_hz | <= 60.0 | 6.4269 | **PASS** |
| rev_monotonic | == 1.0 | 1 | **PASS** |
| net_heap_per_emit_bytes | <= 980076.0 | 22.1538 | **PASS** |

### Notes

- Injected 100000 signed events (full Schnorr verify); emits observed: 13; burst window: 2.0 s; Hz: 6.4
- Event injection: 100000 events via nmp_app_inject_signed_events (real ingest path, full try_from_raw Schnorr verify; D0: cfg-gated, not in production ABI).
- D8 alloc gate: max_payload_bytes=490038; threshold=2×payload=980076; net_heap_per_emit=22 bytes (spec §G-S3 row 5).

### Raw measurements

```json
{
  "alloc_threshold_bytes": 980076.0,
  "burst_elapsed_ms": 2022,
  "burst_hz": 6.426856349564132,
  "configure_bursts": 10,
  "emit_count": 13,
  "inject_count": 100000,
  "max_payload_bytes": 490038,
  "net_heap_delta_burst_bytes": 288,
  "net_heap_per_emit_bytes": 22.153846153846153,
  "p99_apply_us": 38,
  "p99_callback_ms": 0.038958,
  "p99_callback_ns": 38958,
  "rev_monotonic": true,
  "rss_growth_bytes": 167575552,
  "wall_seconds": 91.929245
}
```
