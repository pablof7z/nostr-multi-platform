# M1 Performance Baseline — Live firehose-bench

**Date:** 2026-05-18
**HEAD SHA:** (see `git log -1 --format=%H` at run time)
**Branch:** `claude/review-rmp-spec-8a7VX` → `master`

## Machine

| Field | Value |
|---|---|
| CPU | Apple M3 Max |
| RAM | 36 GB |
| macOS | 26.5 (build 25F5042g) |
| Rust | rustc 1.92.0-nightly |
| firehose-bench mode | `live` (real WebSocket I/O) |
| Relays | `wss://relay.primal.net` + `wss://purplepag.es` |

---

## M1 Exit Gates — Live Results

### Scenario: `cold_start`

Real kernel actor against real relays.  No mocks.  No replay data.

| Gate | Target | Measured | Pass |
|---|---|---|---|
| first_item_ms (time to first visible timeline item) | ≤ 800 ms | 301 ms | **PASS** |
| filled_timeline_ms (time to 200 visible items) | ≤ 5 000 ms | 3 635 ms | **PASS** |

Observations:
- `peak_visible_items=200` — full timeline reached before gate expired
- Memory not directly measured in M1 scope; requires OS-level instrumentation
- Relay connection adds ~50–100 ms to wall time before measurement starts

### Scenario: `profile_thrashing`

50 ClaimProfile/ReleaseProfile cycles per second for 10 minutes (30 000 mount cycles, 60 000 command dispatches).

| Gate | Target | Measured | Pass |
|---|---|---|---|
| dedup_ratio (new wire subs ÷ mount cycles) | ≤ 0.01 | 0.000000 | **PASS** |
| leaked_subscriptions (net new wire subs after thrashing) | = 0 | 0 | **PASS** |
| relay_connected | = 1 | 1 | **PASS** |

Observations:
- `baseline_wire_subs=1 final_wire_subs=1` — seed-timeline tailing sub unchanged
- `mount_cycles=30000` confirms 50/sec over 600 s (spec: 10 min)
- Kernel dedup kept zero new profile-claim REQs for the 3 seed pubkeys
- The spec's "OpenView/CloseView dispatch rate ≤ 60 % of mount rate" describes
  the **platform debounce layer** (ADR-0005 shadow), not the kernel actor.  The
  kernel-side dedup invariant validated here (no new wire REQs per
  already-cached pubkey, `dedup_ratio=0`) is **necessary-but-not-sufficient**;
  the platform debounce gate (OpenView/CloseView dispatch rate ≤ 60 % of mount
  rate) remains deferred to M14 per T22.

---

## Supporting Benchmarks

### reactivity-bench (standard, --fail-on-gate)

| Scenario | lookup p99 ns | recompute p99 ns | view_batch_hz | false_wakeup_rate | mem MiB |
|---|---|---|---|---|---|
| quiet_idle | 84 / 100 000 | 42 / 1 000 000 | 0.10 / 60 | 0.0 | 2.6 |
| following_timeline_scroll | 84 | 84 | 39.78 | 0.0 | 4.0 |
| hashtag_firehose | 42 | 42 | 58.9 | 0.0 | 17.3 |
| profile_fanout | 333 | 2833 | 50.0 | 0.0 | 3.9 |
| thread_blowup | 42 | 42 | 55.6 | 0.0 | 1.7 |
| account_switch | 83 | 42 | 1.0 | 0.0 | 2.6 |
| working_set_100_views | 208 | 125 | 50.0 | 0.0 | 18.9 |

`overall_passed: true`

### firehose-bench (replay, standard, --fail-on-gate)

All 8 replay scenarios pass (deterministic simulation).

| Scenario | Key gate | Measured | Budget |
|---|---|---|---|
| cold_start | first_item_ms | 60 | 800 |
| cold_start | filled_timeline_ms | 740 | 5 000 |
| sustained_firehose | ingest_to_emit_p99_ms | 25.6 | 50 |
| sustained_firehose | view_batch_hz | 58.0 | 60 |
| sustained_firehose | memory_drift_mb | 33.0 | 50 |
| profile_thrashing | open_close_dispatch_rate | 22.5 | 30 |
| profile_thrashing | leaked_subscriptions | 0 | 0 |
| relay_disconnect_storm | reconnect_p99_ms | 18 500 | 30 000 |
| negentropy_efficiency | nip77_bytes_ratio | 0.036 | 0.05 |
| background_decryption | decrypt_p99_ms | 3.01 | 200 |
| soak | memory_growth_mb | 38 | 100 |

`overall_passed: true`

---

## Kernel Changes Required to Make Live Mode Work

Two changes were made to `nmp-core` to enable live firehose-bench:

### 1. `actor.rs` — 250 ms poll when kernel is running

The actor's `next_actor_msg` previously blocked indefinitely when no mailbox
message arrived.  This prevented time-gated events (the 3-second
`contacts_deadline`) from firing between relay messages.

Added a 250 ms `recv_timeout` when `running=true`.  The `Ok(None)` arm flushes
`pending_view_requests()` so the timer check fires on every tick.

**Doctrine review (D0–D8, policy doctrines D0–D5 + substrate invariants D6–D8):**
- D0 (kernel boundary): `ActorCommand` and `spawn_actor` are now gated behind
  `feature = "test-support"`; they do not appear in the stable `nmp-core` API.
- D1 (best-effort rendering): unchanged — still one actor thread; no render path touched
- D2 (negentropy first): no subscription policy changed
- D3 (outbox automatic): no relay code touched
- D4 (single writer per fact): the poll only calls `pending_view_requests()`,
  which is idempotent and reads already-computed state
- D5 (snapshots bounded): no new view kinds added
- D6 (errors never cross FFI): no FFI surface changed
- D7 (capabilities report): no capability decisions added
- D8 (reactivity contract): `emit_now` is now gated on `changed_since_emit()`
  in the idle-tick path, restoring the zero-false-wakeup invariant

### 2. `requests.rs` — `maybe_open_timeline()` in `pending_view_requests()`

`maybe_open_timeline()` was previously called only from `handle_text()` (relay
messages).  With the 250 ms poll, the actor now calls `pending_view_requests()`
every tick, which checks the `contacts_deadline` and opens the seed-timeline when
it expires, even if no relay traffic has arrived.

**Doctrine review (D0–D8):**
- D0–D5: Same reasoning as above.  `maybe_open_timeline()` is idempotent; it
  sets a flag after opening so it only fires once.  No new domain nouns added.
- D6–D8: No FFI, capability, or reactivity-budget changes.

---

## Failures

None.  All M1 live gates passed.

---

## Limitations

- Memory RSS not measured in M1 live mode.  Requires `getrusage`/`task_info`
  instrumentation.  Planned for M1.5.
- Scenarios requiring LMDB, NIP-65 outbox, NIP-77, NIP-42 auth, multi-account,
  NSE, or soak are not live-tested in M1 scope.
- Live mode requires network access to `wss://relay.primal.net` and
  `wss://purplepag.es`.  CI without egress must use `--scenario replay`.
