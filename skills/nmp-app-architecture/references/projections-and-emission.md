# Projections and Incremental Emission

> Primary authority: ADR-0055 (implemented), ADR-0037, ADR-0044, ADR-0045, ADR-0053 (folded →
> ADR-0070). For the app-facing read lifecycle that drives projections, see
> `read-sessions.md`. Verify symbols below against `crates/nmp-core/src/` before citing.

## What a projection is

A **typed projection** is a push-frame slice of kernel state serialized as opaque FlatBuffers
bytes by the crate that owns it, carried under a string key in the snapshot frame's
`typed_projections` sidecar (`TypedProjectionData`). `nmp-core` never interprets the bytes —
it carries, routes, and omits them. The generic `payload:Value` / JSON projection lane is
permanently deleted from the wire (`update_envelope.rs`; ADR-0044). Every projection encodes
its own `schema_id` / `schema_version` / `file_identifier`.

## Registration seam

`SnapshotRegistry` (`crates/nmp-core/src/kernel/snapshot_registry.rs`) is the single
registration surface; the public seam for reusable crates is the `SnapshotProjectionRegistrar`
trait (`substrate/app_host/projection.rs`).

```rust
registry.register_typed("app.workspace.overview", || {
    Some(TypedProjectionData { key, schema_id, schema_version, file_identifier, payload, .. })
}) -> TypedAdmission
```

- Re-registering the same key replaces the closure (last-writer-wins; no stale-closure CPU).
- **D5 cap:** `MAX_SNAPSHOT_PROJECTIONS = 64` (`snapshot_registry/bounds.rs`). A key beyond the
  cap returns `TypedAdmission::DroppedFull` (loud no-op). Derive ledger disposition from the
  returned `TypedAdmission`, never from a pre-insertion check.
- `remove(key)` enqueues one-shot `WireProjectionState::Cleared`. Use it when a transient feed
  closes — never silently stop returning `Some`.

## Emission contract (ADR-0055)

### Incremental is the default, not an optimization

`make_update` (≤ `DEFAULT_EMIT_HZ = 4` Hz) runs every tick:

```
encode projection → Vec<u8>
byte-equality compare with last_emitted
  same   → omit (absent from frame)
  differ → emit Changed + advance emit_rev
  gone   → emit Cleared (no payload)
```

Full snapshot (every projection, every tick) is the **cold-start / resync** shape, not the
steady state. The self-healing invariant holds: a `Changed` row carries the **full current
value** of that projection (state, not a delta), so a dropped frame is superseded by the next
Changed frame, never lost.

### Change detection: byte-equality, not hashing

`TypedProjectionEmissionState::should_emit` does an exact `Vec<u8>` compare against
`last_emitted`. No hash — a collision would permanently freeze the projection. Cost: an O(1)
rev increment on change, `None` on unchanged.

### Three logical states, two on the wire

| State | Wire representation | Host action |
|---|---|---|
| `Changed` | Present row + payload | Decode + apply |
| `Cleared` | Present row, no payload | `cache.remove(key)` |
| `Unchanged` | **Absent from frame** | Retain cached value |

Cleared is always explicit; absence can never mean Cleared (it means Unchanged). The encoder
emits exactly one `Cleared` row on the non-empty→empty transition for conditional-presence
keys.

### Frame identity and baseline reset

`FrameIdentity = (session_id, snapshot_epoch)` where `session_id =
TimingMilestones::started_unix_ms` (changes on Reset/rebuild) and `snapshot_epoch =
ProjectionRevTracker::epoch` (account-switch / schema bump). When either changes,
`TypedProjectionEmissionState` resets (`last_emitted = None`, `emit_rev = 0`) and forces a
full baseline re-emit — in lockstep with the host clearing its projection cache on the same
signal.

### Capability gate: `declare_incremental_apply`

`app.declare_incremental_apply()` must be called before `nmp_app_start` (single-writer). Until
called, the kernel emits full rows every tick (byte-identical to pre-ADR-0055 behavior). After,
the kernel may omit Unchanged projections. The gate is a shared `Arc<AtomicBool>`; calling it
post-start returns `Err(AlreadyStarted)`. This is durable architecture, not a compat shim.

### Drain vs copy-with-TTL projections

| Projection | Semantics | Unchanged when |
|---|---|---|
| `action_results`, `signed_events` | Drain-on-emit (consume) | Empty this tick (absence correct) |
| `action_stages`, `action_lifecycle` | Copy-with-TTL | Unchanged TTL contents |
| Feed / follow_list / session keys | Always-Changed (not yet rev-gated) | Only when Cleared (removed) |

Drain projections must never be "reused from cache" — each Changed tick carries only newly
settled entries.

## Projection closures: D8 requirements

Every registered closure runs **on the actor thread** inside `make_update`. It MUST be
non-blocking (no I/O, no mutex the actor could hold), MUST NOT allocate in steady state
(post-warmup D8 gate), and MUST read pre-computed engine state rather than trigger store
scans. Panic inside a closure is caught by `catch_unwind` (D6); the key is absent for that
tick.

## Store → projection fan-out (ADR-0045)

`project_accepted_event` is the single post-insert projection seam for both live relay delivery
and local store replay. Replay never goes through `store.insert` (re-inserting returns
Duplicate → no fan-out). Replay is interest-scoped, newest-window-bounded, budgeted per actor
tick (D8, D1), and idempotent. Both paths carry `Provenance` (relay vs `LocalStore`);
supersession and fan-out are provenance-agnostic.

## Retired vocabulary (do not teach)

- **Tier-1/Tier-2/Tier-3** projection-tier vocabulary — retired (ADR-0053 folded into
  ADR-0070).
- **`declare_consumed_projections` as a composition step** — it is output transport narrowing,
  not a product read API.
- **`open_interest` as the product read API** — substrate/diagnostic only; use typed read
  sessions.
- **`register_gated` / `ChangeGate`** — the old closure-memoization gate; superseded by
  byte-equality omit. Do not resurrect.

## Violations (blocking)

- Sending raw events, event store, or `serde_json::Value` across FFI — D5.
- Full re-emit every tick without `declare_incremental_apply` where an incremental path exists
  — D8 performance regression.
- Treating absence of a key as Cleared — protocol contract violation.
- Blocking, awaiting, or allocating (post-warmup) inside a registered closure — D8.
- Re-composing a product screen with raw `open_interest` + `declare_consumed_projections` +
  manual observer sinks — retired; use typed read sessions.

## Key symbols (verify before citing)

| Symbol | File |
|---|---|
| `TypedProjectionEmissionState`, `FrameIdentity` | `crates/nmp-core/src/projection_emission.rs` |
| `SnapshotRegistry`, `TypedAdmission` | `crates/nmp-core/src/kernel/snapshot_registry.rs` |
| `MAX_SNAPSHOT_PROJECTIONS` | `crates/nmp-core/src/kernel/snapshot_registry/bounds.rs` |
| `WireProjectionState` | `crates/nmp-core/src/update_envelope/projection_state.rs` |
| `TypedProjectionData` | `crates/nmp-core/src/update_envelope.rs` |
| `SnapshotProjectionRegistrar` | `crates/nmp-core/src/substrate/app_host/projection.rs` |
| `DEFAULT_EMIT_HZ` (= 4) | `crates/nmp-core/src/relay.rs` |
| `project_accepted_event` | `crates/nmp-core/src/kernel/cache_serve/` |
