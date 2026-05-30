# 04 — Actor model (TEA on one thread)

> **Status: SHIPS** · audience: builders + agents · cites verified at master tip.

NMP's execution skeleton is **The Elm Architecture on a single actor thread.**
This section translates that into the vocabulary you will actually see in
`crates/nmp-core/src/actor/` and tells you which thread runs what.

## TEA in NMP vocabulary

| Concept | NMP type | Where |
|---|---|---|
| `AppState` | `AppState { rev, open_view_count }` | `app.rs:55-59` |
| `AppAction` | `KernelAction` (`Start`/`OpenView`/`OpenUri`/…) | `app.rs:21-36` |
| message in | `ActorCommand` (internal) | `actor/mod.rs:26-51` |
| `handle_message` | `dispatch_command(...)` | `actor/mod.rs:162-` |
| state emission | `AppUpdate` frame via `update_tx`; canonical runtime payload is FlatBuffers | `app.rs:38-48`, `tick.rs:53-62` |

Data flow is **strictly unidirectional**: user interaction →
action dispatch → actor processes synchronously → state emission → platform
re-renders. One OS thread owns all mutable state. No locks, no concurrent
mutation, no race conditions.

## Sequence: a dispatch round-trip

```
 UI (Swift/Kotlin)
   │  dispatch(action)            fire-and-forget, no return value
   ▼
 command channel  ── std::sync::mpsc, NOT a return path
   │
   ▼
 bridge_commands thread  (relay_mgmt.rs:13-21)
   │  wraps as ActorMsg::Command
   ▼
 ACTOR THREAD  (run_actor, actor/mod.rs:63)
   │  next_actor_msg() recv        (tick.rs:11-43)
   │  dispatch_command()           mutates Kernel, bumps rev (actor/mod.rs:162)
   │  kernel.open_*/start/...      builds Vec<OutboundMessage>
   │  emit_now()                   kernel.make_update() → FlatBuffers frame
   ▼
update_tx                         snapshot by default
   │
   ▼
 listener / reconciler (host)      hops to UI thread
   │  compare incoming rev to last applied → skip if stale
   ▼
 UI re-renders                     @Observable / mutableStateOf / signal
```

Outbound relay traffic is a side-channel, not the return path: the actor hands
`OutboundMessage`s to per-relay worker threads (`relay_mgmt.rs:77-109`); their
replies re-enter the **same** actor loop as `ActorMsg::Relay`
(`actor/mod.rs:133-152`), so all state mutation stays single-writer.

## Which thread runs what

| Thread | Owns / does | Channel it reads | Source |
|---|---|---|---|
| **Actor thread** | `Kernel` + all `AppState`; runs `handle_message`; the only writer | `actor_rx` (`ActorMsg`) | `actor/mod.rs:63-159` |
| `bridge_commands` | forwards host `ActorCommand` → `ActorMsg::Command` | `command_rx` | `relay_mgmt.rs:13-21` |
| `bridge_relays` | forwards relay events → `ActorMsg::Relay` | `relay_rx` | `relay_mgmt.rs:23-31` |
| Per-relay worker (1/role) | blocking `tungstenite` WebSocket I/O | `RelayCommand` | `relay_worker.rs:66-72` |
| Host listener | drains `update_tx`, hops to UI thread, replaces shadow | `update_rx` | host shell |

There is **one** writer (the actor). Everything else is a courier.

> **Reality note (cite drift, see [27]).** `aim.md:31` describes the spec's
> reference model as a `flume` channel plus a separate **tokio** runtime for
> async I/O. The shipped kernel realizes the same TEA contract with
> `std::sync::mpsc` channels, `std::thread`, and blocking `tungstenite`
> sockets — no `flume`, no tokio runtime. The *contract* (single-writer actor,
> fire-and-forget dispatch, snapshot emit) is identical; the thread/channel
> primitives differ. Build against the contract, not the spec's prose.

## The four load-bearing invariants

1. **Monotonic `rev`.** Every state change bumps `rev: u64` inside `AppState`
   (`app.rs:57`). The host compares incoming `rev` to last applied and drops
   stale snapshots. Do not disable this guard.
2. **`dispatch()` is fire-and-forget.** No return value, never blocks. Results
   come back only as later snapshots. There is no synchronous "read state
   after dispatch" path — the command channel is one-way.
3. **Errors do not cross FFI (D6).** `KernelAction`/`KernelUpdate` carry only
   protocol-neutral primitives; fallible paths resolve to a typed update such
   as `KernelUpdate::UriRejected { uri, reason }` (`app.rs:46-48`,
   `app.rs:183`), never a panic across the seam.
4. **Snapshot-default emit.** State crosses the runtime bridge as a full
   snapshot frame by default (`emit_now`, `tick.rs:53-62`); granular
   `ViewBatch` deltas are an optimization layered on top (see [06]), not the
   default. The canonical hot payload format is FlatBuffers, not JSON and not
   UniFFI records.

## Organizing TEA code

External TEA sources converge on one rule that matters here: organize around
the owner of state, not around technical roles. In NMP vocabulary, a cohesive
feature / view module / protocol module should keep its state shape, action or
message vocabulary, reducer/update logic, projection payload, and tests near
the same owner.

Do **not** introduce top-level `model`, `update`, `view`, `state`, or `actions`
trees that separate every feature by role. That recreates MVC-style boundary
debates and makes it harder to see the full state transition. If a module grows,
split under the same owner namespace by concrete sub-type, sub-protocol, or
helper responsibility while keeping the owner obvious. The repository LOC
ceiling still wins; Elm's tolerance for very long files is not a license to
break `AGENTS.md`.

For multi-screen composition, use the Iced pattern as the mental model: nested
screen/module messages may be mapped back to the parent, and child reducers may
return explicit parent-visible actions. The parent decides routing or
cross-screen effects; children do not mutate global state directly. On native
platforms this composition is generated or bridged through the Rust actor, not
implemented as local Swift/Kotlin component state.

### Emit only when state changed (D8)

The actor never emits on a bare idle tick. `next_actor_msg`/`flush_due`/
`emit_now` (`tick.rs:11-62`) gate every send on `kernel.changed_since_emit()`
and pace it to `emit_hz` (default ~60Hz, `actor/mod.rs:88-91`,
`actor/mod.rs:155-157`). The regression test
`idle_ticks_do_not_emit_snapshots_when_state_unchanged` (`tick.rs:79-100`)
locks this: an unstarted actor over 1 s of idle polls emits **zero**
snapshots. This is the D8 zero-false-wakeup invariant at the actor seam.

## The `#[cfg(any(test, feature = "test-support"))]` policy

The actor module is private (`mod actor`, not `pub mod actor`, `lib.rs:1`).
`ActorCommand`/`run_actor`/`spawn_actor` are `pub` *inside the crate* but
reach the outside world **only** through the gated `testing` re-export
(`lib.rs:37-56`). In a normal build nothing re-exports them, so they are
effectively crate-private.

The gate is deliberately `any(test, feature = "test-support")` so `cargo test`
always has access without an explicit feature flag, while production FFI
builds never see these symbols. The same gate guards
`ActorCommand::IngestPreVerifiedEvents` (`actor/mod.rs:49-50`) — its doc
comment states the rationale verbatim: *"Test-support only (D0: not part of
production FFI surface)."*

**Why this matters for you:** `testing::spawn_actor` (`lib.rs:51-56`) and the
`nmp_app_*` FFI re-exports (`lib.rs:23-29`) exist for benches and the
ffi-stress harness — they let Rust-side test code drive the actor directly.
They are not an app API. Production code that depends on `spawn_actor` will
fail to compile without the feature, and turning the feature on in a shipped
app is a D0 violation: you would be reaching past the generated FFI surface
into kernel internals.

## Anti-patterns

- **Expecting `dispatch` to return a result.** It is fire-and-forget. Model the
  outcome as a future snapshot, not a return value or a thrown error.
- **Reading state synchronously after dispatch.** The command channel is
  one-way; there is no "get current state" call. Observe `update_tx`.
- **Spawning ad-hoc threads in app code to "wait" for the actor.** All
  concurrency is owned by the kernel. App threads cannot mutate `AppState` and
  only invite races the architecture rules out.
- **Holding mutable state in Swift/Kotlin.** Native is rendering plus
  capability execution; no caches, no derived state. Shadow the snapshot, do
  not author it.
- **Depending on `testing::spawn_actor` / `nmp_app_*` from production.** D0
  violation — those symbols are gated test-support, not the FFI contract
  (`lib.rs:37-56`, `actor/mod.rs:46-48`).

See also: [05 — Kernel substrate — traits + seams](05a-substrate-traits.md) ·
[06 — Reactivity contract (D8)](06-reactivity-contract.md) ·
[17 — iOS shell — SwiftUI consumes the kernel](17-ios-shell.md) ·
[27 — Doc/code discrepancies (orchestrator queue)](27-discrepancies.md)
