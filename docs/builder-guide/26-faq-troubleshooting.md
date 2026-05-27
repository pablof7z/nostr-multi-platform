# 26 — FAQ + troubleshooting

**Status: SHIPS · audience: builders.** Practical answers for the common
build/run failures. The golden rule: **inspect the decoded kernel snapshot
before you touch Swift.** Almost every "it doesn't work" is visible in the snapshot's
`relay_statuses`, `logical_interests`, or `wire_subscriptions` arrays.

## FAQ (~15 items)

**Q1. `cargo build` fails with a workspace path / version mismatch.**
The app-core and per-app FFI crates use `version.workspace = true`. Build
from the workspace root, not the crate directory. Add new crates to the root
`Cargo.toml` `members` list before `cargo build -p <crate>`.

**Q2. `nmp gen modules --check` says "generated module crate is stale".**
The hand-written app-core changed but the FFI crate was not regenerated. Run
`cargo run -p nmp-codegen -- gen modules --manifest apps/<app>/nmp.toml`
(no `--check`) and commit the regenerated `nmp-app-*/src/*`. Never hand-fix
the symptom by editing generated files.

**Q3. Codegen drift in CI but the build is green locally.**
CI runs `gen modules --check`. Your local tree has uncommitted regenerated
output, or you edited a generated file. Regenerate, diff, commit.

**Q4. What does `nmp init` scaffold?**
`nmp init my-app` creates a Rust workspace with an app-core crate, an `nmp.toml`
manifest, a starter domain/view/action module, and a headless shell example. It does
**not** produce an Xcode project or Android Compose module — that's the platform shell
layer you wire yourself. See [17 — iOS shell](17-ios-shell.md) for the Swift wiring and
`apps/chirp/android/` as the Android reference.

**Q5. Where is UniFFI / the typed `AppUpdate` enum?**
**M14, PLANNED.** UniFFI is the binding/lifecycle/capability surface, not the
hot payload format. The runtime update transport target is FlatBuffers-only;
master still has historical raw-C JSON callback code while the migration is
incomplete. Code expecting typed UniFFI payload delivery will not compile
against master. See [27](27-discrepancies.md).

**Q6. iOS sim build can't find the Rust symbols (`nmp_app_new`, …).**
The static lib was not built for the simulator triple. Run
`cargo build -p nmp-app-<app> --target aarch64-apple-ios-sim` and confirm the
Xcode link path points at that `target/aarch64-apple-ios-sim/` output.

**Q7. `--features lmdb-backend` won't compile.**
`LmdbEventStore` is a feature-gated skeleton (LANDED, not SHIPS). For a
microblog you do not need it — the default `MemEventStore` is the supported
path. See [09](09-persistence-lmdb.md).

**Q8. No events ever arrive (empty feed).**
Snapshot first. Check `relay_statuses[].connection`. If it is not
`"connected"`, it is a relay problem (see the 3-step flow below). If it *is*
connected, check `logical_interests[].state` — `opening`/`backfilling` means
the data is still in flight, not missing.

**Q9. The feed shows old data and won't update.**
Stale `rev`. The Swift side guards on `rev` monotonicity. If `rev` is not
advancing in the `NMP_CORE` stdout logs, the kernel is not emitting — the
relay or interest is stuck, not the UI. Do **not** disable the rev guard.

**Q10. Avatars / display names are blank.**
That is correct behavior, not a bug. Display fields are non-`Option` with
deterministic placeholders (D1 — `kernel/types.rs:79-113`). A blank-looking
avatar with `author_avatar_source: "placeholder"` means kind:0 has not
arrived yet; the feed still renders. Never gate the feed on "profile loaded".

**Q11. A subscription seems to leak (REQ count climbs).**
Interests are refcounted. Every `open*` needs a matching `close*` /
`releaseProfile`. Check `wire_subscriptions[]` length and
`logical_interests[].refcount`. A refcount that only grows means a missing
release on view teardown.

**Q12. NIP-42 relay rejects my subscription.**
Check `relay_statuses[].auth`. Values: `not_required`, `challenge_received`,
`authenticating`, `authenticated`, `failed` (`kernel/types.rs:209-213`). The
kernel drives the challenge/response; the app does not. `failed` with a
`last_error` means the signer could not satisfy the challenge.

**Q13. How do I read relay health programmatically?**
Decode the snapshot and read `relay_statuses` (per-role: `connection`,
`auth`, `bytes_rx/tx`, `reconnect_count`, `last_error`) — the Swift mirror is
`KernelBridge.swift:183-197`.

**Q14. How do I enable debug diagnostics?**
The guardrail checker runs only under `cfg(debug_assertions)` (debug builds):
bech32-where-hex, `limit` on replaceable filters, empty `authors`, cache miss
with no fallback loader, etc. Violations produce a `DebugDiagnostics` entry
plus an `eprintln!` with a doc URL. Release cost is zero
(`subsystems.md:323-336`). Build in debug to see them.

**Q15. Where do I file a doc/code discrepancy?**
[27 — Doc/code discrepancies](27-discrepancies.md). Most "the doc says X but
the code does Y" cases are *milestone not landed yet* (UniFFI M14, `nmp init`
M16), not bugs. Don't change the spec to match incomplete code; file it.

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

The canonical shape is the `KernelUpdate` struct
([`crates/nmp-core/src/kernel/types.rs:306-326`](../../crates/nmp-core/src/kernel/types.rs));
the Swift shadow mirror is `KernelBridge.swift:119-138`. On master this shape
is still decoded from the legacy JSON callback; the FlatBuffers migration makes
the same logical fields arrive through generated FlatBuffers readers. 18
top-level fields:

| Field | Type | Use |
|---|---|---|
| `rev` | u64 | monotonic emit counter; the staleness guard |
| `update_kind` | string | why this emit fired (snapshot vs delta) |
| `running` | bool | actor loop alive |
| `relay_url` | string | primary content relay (legacy single field) |
| `test_npub` | string | seed identity for the demo shell |
| `profile` | ProfileCard | active/target profile card (D1 placeholders) |
| `items` | [TimelineItem] | current bounded feed window |
| `author_view` | AuthorViewPayload? | populated only if an author view is open |
| `thread_view` | ThreadViewPayload? | populated only if a thread is open |
| `inserted` | [TimelineItem] | delta: items added this emit |
| `updated` | [TimelineItem] | delta: items changed this emit |
| `removed` | [string] | delta: item ids dropped this emit |
| `metrics` | Metrics | counters (events_rx, payload_bytes, queue depth, …) |
| `relay_status` | RelayStatus | primary content relay health |
| `relay_statuses` | [RelayStatus] | **per-role** relay health (start here) |
| `logical_interests` | [LogicalInterestStatus] | one row per open interest + state |
| `wire_subscriptions` | [WireSubscriptionStatus] | live wire REQs + close_reason |
| `logs` | [string] | last ≤80 `NMP_CORE` log lines |

Debug order: `relay_statuses` → `logical_interests` → `wire_subscriptions` →
`logs`. The `inserted`/`updated`/`removed` deltas are bounded by what is open
(D5); `metrics` is for perf, not correctness.

## Anti-patterns

- **Blaming the relay for a stale `rev`.** A frozen `rev` with
  `connection: "connected"` is an emit/interest problem, not a relay one.
  Read `logical_interests` before accusing the relay.
- **Debugging in Swift instead of the decoded snapshot.** The snapshot is the
  source of truth across FFI. Decode and inspect it first; Swift only
  renders what the snapshot already decided.
- **Editing generated code to fix a symptom.** Stale FFI crate → regenerate,
  don't patch. Patches are erased on the next `gen modules`.
- **Disabling the rev guard to "make the UI update".** The guard is correct;
  a non-advancing `rev` is a real upstream stall. Disabling it hides the bug
  and shows torn state.

See also: [17 — iOS shell — SwiftUI consumes the kernel](17-ios-shell.md) ·
[18 — Testing — `nmp-testing`, benches, contract tests](18-testing.md) ·
[27 — Doc/code discrepancies (orchestrator queue)](27-discrepancies.md)
