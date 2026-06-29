# 26 — FAQ + troubleshooting

**Status: SHIPS · audience: builders.** Practical answers for the common
build/run failures. The golden rule: **inspect the decoded kernel snapshot
before you touch Swift.** Almost every "it doesn't work" is visible in the snapshot's
`relay_statuses`, `logical_interests`, or `wire_subscriptions` arrays.

## FAQ (~15 items)

**Q1. `cargo build` fails with a workspace path / version mismatch.**
App-core and thin staticlib crates use `version.workspace = true`. Build
from the workspace root, not the crate directory. Add new crates to the root
`Cargo.toml` `members` list before `cargo build -p <crate>`.

**Q2. What does `nmp init` scaffold?**
`nmp init my-app` creates a thin Rust workspace: a `<name>-core` crate that
owns explicit NMP composition and registers app-specific seams, an `nmp.toml`
manifest (used by `nmp doctor` / `nmp upgrade`), a starter domain/view/action
module, and a headless `examples/shell.rs` using `NmpAppBuilder`. It does
**not** produce an Xcode project or Android Compose module — that's the platform
shell layer you wire yourself. See
[17 — iOS shell](17-ios-shell.md) for the Swift wiring and `apps/chirp/android/`
as the Android reference.

**Q3. Where is UniFFI?**
UniFFI is the public native binding: lifecycle, callbacks, capability objects,
and byte action/update doorways cross there. UniFFI is not the hot payload
format: runtime updates remain binary `nmp.transport.UpdateFrame` bytes. Browser
hosts use the wasm-bindgen runtime surface instead. See
[15](15-codegen-and-ffi.md).

**Q4. iOS sim build can't find the generated binding module.**
Regenerate/import the UniFFI bindings for the simulator target and confirm the
native shell links the Rust library for that same target. Raw `nmp_app_*` symbol
lookups are a sign you are following the old transitional C-ABI path, not the
current public native recipe.

**Q5. `--features lmdb-backend` won't compile.**
`LmdbEventStore` is real but feature-gated. The type exists in default builds,
but durable operation requires compiling the relevant crate with
`--features lmdb-backend`; otherwise `open()` returns an explicit feature-off
error. For throwaway examples, use the default `MemEventStore`. For native
durable apps, enable the feature and surface any open failure as diagnostics.
See [09](09-persistence-lmdb.md).

**Q6. No events ever arrive (empty feed).**
Snapshot first. Check `relay_statuses[].connection`. If it is not
`"connected"`, it is a relay problem (see the 3-step flow below). If it *is*
connected, check `logical_interests[].state` — `opening`/`backfilling` means
the data is still in flight, not missing.

**Q7. The feed shows old data and won't update.**
Stale `rev`. The Swift side guards on `rev` monotonicity. If `rev` is not
advancing in the `NMP_CORE` stdout logs, the kernel is not emitting — the
relay or interest is stuck, not the UI. Do **not** disable the rev guard.

**Q8. Avatars / display names are blank.**
That is correct behavior, not a bug. Display fields are non-`Option` with
deterministic placeholders (D1 — `kernel/types.rs:79-113`). A blank-looking
avatar with `author_avatar_source: "placeholder"` means kind:0 has not
arrived yet; the feed still renders. Never gate the feed on "profile loaded".

**Q9. A subscription seems to leak (REQ count climbs).**
Interests are refcounted. Every `open*` needs a matching `close*` /
`releaseProfile`. Check `wire_subscriptions[]` length and
`logical_interests[].refcount`. A refcount that only grows means a missing
release on view teardown.

**Q10. NIP-42 relay rejects my subscription.**
Check `relay_statuses[].auth`. Values: `not_required`, `challenge_received`,
`authenticating`, `authenticated`, `failed` (`kernel/types.rs:209-213`). The
kernel drives the challenge/response; the app does not. `failed` with a
`last_error` means the signer could not satisfy the challenge.

**Q11. How do I read relay health programmatically?**
Decode the snapshot and read `relay_statuses` (per-role: `connection`,
`auth`, `bytes_rx/tx`, `reconnect_count`, `last_error`) — the Swift mirror is
`KernelBridge.swift:183-197`.

**Q12. How do I enable debug diagnostics?**
The guardrail checker runs only under `cfg(debug_assertions)` (debug builds):
bech32-where-hex, `limit` on replaceable filters, empty `authors`, cache miss
with no fallback loader, etc. Violations produce a `DebugDiagnostics` entry
plus an `eprintln!` with a doc URL. Release cost is zero
(`subsystems.md:323-336`). Build in debug to see them.

**Q13. Where do I file a doc/code discrepancy?**
Correct the owning doc in place when it is wrong. If the mismatch represents active work rather than bad guidance, open or update the GitHub issue instead.

## Debug a missing snapshot in 3 steps

1. **Is `rev` advancing?** Watch stdout for `NMP_CORE` lines
   (`kernel/status.rs:312`). If `rev` is frozen, the kernel is not emitting —
   the problem is upstream of the UI; continue to step 2. Do not debug in
   Swift yet.
2. **Are relays connected?** In the snapshot, every entry of
   `relay_statuses[].connection` should be `"connected"`. If any is
   `"offline"`/`"connecting"` with a `last_error`, the snapshot is empty
   because there is nothing to project — go to the relay flow below.
3. **Are interests progressing?** Check `logical_interests[].state`. The
   states progress `opening` → `backfilling`/`tailing` → `complete`
   (`kernel/status.rs:40-199`). A stuck `opening` means the REQ never went
   out; a `tailing` with `cache_coverage: "warming"` means data is arriving —
   wait, don't restart.

## Debug a non-connecting relay in 3 steps

1. **`relay_statuses[].connection` + `last_error`.** `offline` with a
   `last_error` (DNS/TLS/refused) is a network or URL problem. `connecting`
   that never advances with a rising `reconnect_count` is a relay that
   accepts the socket but drops it.
2. **`relay_statuses[].auth`.** If it is `challenge_received` or `failed`,
   the relay is NIP-42-gated. `failed` means the active signer could not
   answer the challenge — check that an account is active and the signer is
   loaded ([11](11-sessions-signers.md)).
3. **`wire_subscriptions[].close_reason`.** A populated `close_reason`
   (e.g. `closed_by_relay`) tells you the relay actively rejected the REQ
   (rate limit, bad filter, auth-required). Match the `wire_id` back to the
   `logical_interests[].key` that owns it.

## Snapshot — top-level field reference

The canonical runtime frame is `nmp.transport.UpdateFrame`
([`crates/nmp-core/schema/nmp_update.fbs`](../../crates/nmp-core/schema/nmp_update.fbs)).
For normal state updates, `kind = Snapshot` and the `snapshot` table carries
typed envelope fields plus `typed_projections`. `kind = Panic` is the terminal
actor-thread failure frame. Hosts decode the frame with generated FlatBuffers
readers; there is no production JSON snapshot fallback. Product view state
comes from typed projection sidecars such as `nmp.feed.home`,
`nmp.feed.author.<pubkey>`, and `nmp.feed.thread.<event_id>`.

| Field | Type | Use |
|---|---|---|
| `typed_projections` | [TypedProjection] | per-key typed sidecars; host view models decode from these |
| `rev` | u64 | monotonic emit counter; the staleness guard |
| `kernel_schema_version` | u32 | kernel snapshot schema version |
| `last_tick_ms` | u64 | actor liveness/timing stamp |
| `update_kind` | string | run-state label (`ViewBatch` today) |
| `running` | bool | actor loop alive |
| `metrics` | Metrics | counters (`events_rx`, payload bytes, queue depth, etc.) |
| `relay_status` | RelayStatus | aggregate connection summary |
| `relay_statuses` | [RelayStatus] | **per-relay** health (start here) |
| `logical_interests` | [LogicalInterestStatus] | one row per open interest + state |
| `wire_subscriptions` | [WireSubscriptionStatus] | live wire REQs + close reason |
| `logs` | [string] | last bounded `NMP_CORE` log lines |
| `last_error_toast` / `last_error_category` / `last_planner_error` / `store_open_failure` | string? | user-visible and diagnostic failure state |
| `no_configured_relays` | bool? | startup/configuration diagnostic |
| `negentropy_sync_stats` | NegentropySyncStats? | NIP-77 sync counters |
| `snapshot_epoch` / `session_id` | u64 | ADR-0055 frame-level cache identity |

Debug order: `relay_statuses` → `logical_interests` → `wire_subscriptions` →
`logs`. Product view state comes from typed projection sidecars; add a typed
sidecar for new Swift/Kotlin UI state instead of relying on generic JSON.
For dynamic typed projections, `Changed` replaces the cached value, `Cleared`
drops it, and omission in an incremental frame means retain the last decoded
value. `metrics` is for perf, not correctness.

## Anti-patterns

- **Blaming the relay for a stale `rev`.** A frozen `rev` with
  `connection: "connected"` is an emit/interest problem, not a relay one.
  Read `logical_interests` before accusing the relay.
- **Debugging in Swift instead of the decoded snapshot.** The snapshot is the
  source of truth across FFI. Decode and inspect it first; Swift only
  renders what the snapshot already decided.
- **Editing generated binding code to fix a symptom.** UniFFI bindings and
  typed decoders are projections of Rust/source schemas. If they drift,
  regenerate from source instead of patching generated output. There is no
  `gen modules` step; the app-core composition root is hand-written glue.
- **Disabling the rev guard to "make the UI update".** The guard is correct;
  a non-advancing `rev` is a real upstream stall. Disabling it hides the bug
  and shows torn state.

See also: [17 — iOS shell — SwiftUI consumes the kernel](17-ios-shell.md) ·
[18 — Testing — `nmp-testing`, benches, contract tests](18-testing.md).
