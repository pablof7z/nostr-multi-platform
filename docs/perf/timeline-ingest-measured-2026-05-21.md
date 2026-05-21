# Timeline Ingest Measured Remediation

Date: 2026-05-21

Command and environment:

```sh
cargo test -p nmp-core timeline_ingest_perf --release -- --ignored --nocapture
```

The harness lives in `crates/nmp-core/src/kernel/timeline_perf_tests.rs`. It
pre-generates 5,000 signed kind:1 notes, then times only
`Kernel::ingest_timeline_event` through the real store/verification/timeline
path. Signing is outside the timed section.

## Baseline

Current `origin/master` at `0fa59c5c`:

```text
timeline_ingest_perf events=5000 visible_limit=500 elapsed_ms=328 per_event_us=65.74 sort_calls=5000 cloned_ids_estimate=2375250
```

Finding: every visible event append rebuilt a `Vec` from the bounded timeline,
sorted it, truncated it to 500, and replaced the deque. For this harness that
means 5,000 full sorts and an estimated 2,375,250 cloned ids.

## Change

`ingest_timeline_event` now inserts the new event id directly into the already
sorted bounded timeline. It removes an existing matching id defensively, finds
the insertion point by the existing ordering contract (`created_at` descending,
id ascending), inserts there, and truncates to the same 500-entry cap.

The old full-sort helper remains available only for test-support batch
injection, where callers deliberately defer one sort until after a batch.

## After

Same command, same worktree, after the incremental insert change:

```text
timeline_ingest_perf events=5000 visible_limit=500 elapsed_ms=229 per_event_us=45.95 ordering=incremental_sorted_insert legacy_sort_calls_avoided=5000 legacy_cloned_ids_avoided_estimate=2375250
```

Result: 328 ms -> 229 ms for the measured ingest region, a 99 ms reduction
(30.2%). Per-event timed cost dropped from 65.74 us to 45.95 us.

## Validation

```sh
cargo test -p nmp-core incremental_timeline_insert_matches_sort_order_and_cap
rustfmt --check --config skip_children=true \
  crates/nmp-core/src/kernel/ingest/mod.rs \
  crates/nmp-core/src/kernel/ingest/timeline.rs \
  crates/nmp-core/src/kernel/ingest/timeline_order.rs \
  crates/nmp-core/src/kernel/timeline_perf_tests.rs \
  crates/nmp-core/src/kernel/timeline_order_tests.rs
```

`cargo fmt --check` for the full workspace currently reports unrelated
pre-existing formatting drift outside this PR's write set, so this PR used a
file-scoped rustfmt check for the files it changed.

## Still Missing Queue

- Snapshot emission rebuild/serialization cost still needs separate measurement.
- Swift JSON/FFI hot path still needs device-side measurement before changes.
- Diagnostics/Home dictionary rebuilds still need a focused UI/runtime counter.
