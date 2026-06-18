# perf-sanity — new-architecture sanity pressure test

**Goal (not before/after):** detect whether the **current** NMP architecture
**misbehaves** under real load — CPU pegging / busy-spin / polling, memory leaks
(unbounded RSS), latency cliffs, dropped events, or correctness breaks. The gates
are **absolute thresholds**, not deltas.

This is the **harness + absolute gates**. It is built from NEW files only and
reuses (never edits) the existing churning benches' hooks:
`firehose-bench` gate constants, `ffi-stress` mach-RSS + capture-cb pattern,
`reactivity-bench` CountingAllocator, the typed `decode_snapshot_envelope`, and
the `nmp_app_*` public FFI (sign-in-as-account).

## Pieces

| File | Role |
|---|---|
| `crates/nmp-testing/bin/sanity-gate/` | Rust driver + in-process gates. Drives the **real Chirp composition** via the public FFI (`nmp_app_chirp_register` → `nmp_app_signin_nsec` → `nmp_app_add_relay` → `nmp_app_start` → `nmp_app_chirp_open_home_feed`), reads in-process counters, asserts the absolute gates, writes `docs/perf/<run>/sanity-report.{json,md}`. |
| `scripts/perf-sanity/run.sh` | Orchestrator. Starts `nak serve` (local) or targets a public relay (`--live`), resolves a high-follow account, launches `sanity-gate`, aligns the OS sampler, merges metrics. |
| `scripts/perf-sanity/os-sampler.sh` | OS CPU%/RSS/per-thread sampler (`ps -o %cpu,rss`, `ps -M`/`top -H`). Writes the per-phase JSON the Rust bin merges via `--os-metrics`. |
| `scripts/perf-sanity/capture-real-events.sh` | `nak req` dump of real kinds 1,6,7,0,3,10002,1059,30023,9735 → `artifacts/real-events.jsonl`. |
| `scripts/perf-sanity/resolve-account.py` | `accounts.json` lookup + npub→hex (no deps). |
| `scripts/perf-sanity/accounts.example.json` | Fixture template (copy to gitignored `accounts.json`). |

## Run it — local `nak serve` (testable on bare master)

```bash
# 1. capture a real corpus (optional; a synthetic Schnorr-signed burst is used if absent)
scripts/perf-sanity/capture-real-events.sh --relay wss://relay.primal.net --limit 500

# 2. full run against a LOCAL relay replaying the corpus
scripts/perf-sanity/run.sh --phase all --soak-secs 120 --account high-follow

# or just the in-process Rust bin directly (no OS sidecar; OS gates report BLOCKED):
cargo run -p nmp-testing --bin sanity-gate -- \
  --phase firehose --relay ws://127.0.0.1:10547 --run-id sanity-local
```

## Run it — `--live` real public relays

```bash
scripts/perf-sanity/run.sh --live --relay wss://relay.primal.net \
  --account high-follow --phase all --soak-secs 1800
```

`--live` only changes the report `mode` + SKIP semantics; a relay miss still
**SKIPs LOUD** (`Verdict::SkipRelayMiss`) — never a faked green.

## Absolute gates

| gate | threshold | tool | hook |
|---|---|---|---|
| `idle-cpu` | `< 2.0 %` sustained ≥60s | `ps -o %cpu` / `top` (sidecar) | `os-sampler cpu_pct_mean` |
| `no-spin-per-thread` | `< 90 %` any thread while ingest queue empty | `ps -M` / `top -H` (sidecar) | `os-sampler max_thread_cpu_pct` |
| `load-cpu-soft` | `<= 150 %` (informational, never fails) | `ps`/`top` (sidecar) | `os-sampler cpu_pct_peak` |
| `cold-start-first-item` | `<= 800 ms` | `decode_snapshot_envelope` | `SnapshotEnvelope.visible_items >= 1` |
| `cold-start-filled-timeline` | `<= 5000 ms` | `decode_snapshot_envelope` | `visible_items >= 200` |
| `load-older` | `<= 5000 ms` | `nmp_app_load_older_feed` + envelope | `visible_items` growth |
| `ingest-to-emit-amortised` | `<= 50 ms/event` | wall clock (inject→note) | `SnapshotEnvelope.note_events` |
| `encode-p99` | `<= 50 ms` | `decode_snapshot_envelope` | `SnapshotEnvelope.serialize_us` p99 |
| `ramp-peak-rss` | `<= 200 MB` | `task_info(MACH_TASK_BASIC_INFO)` | `metrics::process_rss_mb` |
| `soak-peak-rss` | `<= 200 MB` | `ps -o rss` (sidecar) / `task_info` | `os-sampler rss_peak_mb` |
| `memory-slope` | `<= 100 MB/hr` | `ps -o rss` loop (sidecar) | `os-sampler rss_slope_mb_per_hr` |
| `memory-drift-30m` | `<= 50 MB` over 30m | `task_info` | `process_rss_mb` start vs end |
| `no-dropped-events` | `0 dropped` | `nmp_app_inject_signed_event_json` + envelope | `note_events` delta vs kind:1 count |
| `dedup-no-growth-on-duplicate` | `0 extra rows` | re-inject corpus | `visible_items` must not grow |
| `follow-feed-not-truncated` | `>= 200 items` (2k-follow account) | `decode_snapshot_envelope` | `visible_items` (proves #1500 500-cap didn't truncate) |

Latency + memory thresholds are **reused verbatim** from
`crates/nmp-testing/bin/firehose-bench/config.rs` (cited in
`bin/sanity-gate/config.rs`). Verdicts: `PASS | FAIL | SKIP-relay-miss | BLOCKED`.

## Documented hook gaps (measured what we can; wire in a follow-up)

These are surfaced as `BLOCKED` rows + `findings` in every report — the harness
does **not** touch the churning store/diagnostics code to close them:

1. **per-event ingest→emit p99** — the per-event ingest-timestamp counter is
   `#[cfg(test)]` in the store. We report an **amortised** inject→note cost.
   Follow-up: a process-lifetime ingest-latency histogram read hook (sibling to
   `nmp_app_read_projection_churn_stats`).
2. **LRU-eviction evidence** — `kernel::ram_eviction::RamEvictionReport` is
   internal to `run_gc_step` with no FFI/diagnostics read seam. Follow-up:
   `nmp_app_read_ram_eviction_stats`.
3. **follow-feed author subset** — the typed `SnapshotEnvelope` carries
   `visible_items` (count) but not per-item author hex, nor the active follow-set
   p-tags. The `authors ⊆ follow-set ∪ self` half is `BLOCKED`. Follow-up:
   `nmp_app_read_feed_authors`.
4. **replaceable supersession** — no FFI read for the resolved replaceable value.
   Follow-up: `nmp_app_read_replaceable(pubkey, kind)`. The `dedup-no-growth`
   gate exercises the same insert chokepoint as a partial proxy.
5. **`query_visit` conversion counter** — `#[cfg(test)]` (#1522); not wired.

## Blocked on unmerged work

`#1552` (dispatcher), `#1541` (wakeups), and the pull cursor: the `idle-cpu` and
`no-spin-per-thread` gates are the exact detectors for a wakeup-storm / poll
regression those changes touch, and `load-older` exercises the pull cursor.
Re-run `--phase idle_soak` / `--phase cold_start` once they land — the scenarios
are already wired; they only need the OS sidecar numbers.

## iOS / Android capture (stubs left for the xcode MCP)

### §iOS — Instruments / xctrace
Drive the Chirp iOS shell under a profiling trace and map its tracks onto the
SAME gate names:

```bash
xcrun xctrace record --template 'Time Profiler' --template 'Allocations' \
  --device <udid> --launch -- <Chirp.app>
# idle window: hold the app on the home feed with no new events for >=60s.
```
- `idle-cpu` ← xctrace **CPU Usage** track, process %, idle window.
- `no-spin-per-thread` ← xctrace per-thread **sample %** (Time Profiler).
- `memory-slope` / `soak-peak-rss` ← **Allocations** persistent-bytes track.

Emit an `os-metrics.json` with the same `{ "<phase>": { cpu_pct_mean, ... } }`
shape and pass it to `sanity-gate --os-metrics`. (The xcode MCP can script
`xctrace` + parse the `.trace` export.)

### §Android — dumpsys / perfetto
- coarse CPU%/RSS: `adb shell dumpsys cpuinfo`, `adb shell dumpsys meminfo <pkg>`.
- per-thread scheduling (no-spin gate): a perfetto trace
  (`record_android_trace -o trace -t 60s sched`) → parse per-thread CPU.

Map onto the same gate names + `os-metrics.json` shape.
