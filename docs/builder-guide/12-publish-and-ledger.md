# 12 — Publishing + the publish engine

*Status: SHIPS · Audience: both*

> **Scope note.** "M8" here means **M8-multi-account** (the signer that
> feeds this engine — see [11 — sessions/signers](11-sessions-signers.md)).
> The relay-manager / subscription-lifecycle M8-subs split is
> [14 — relay manager](14-relay-manager.md). Keep them distinct: §11/§12
> are the *write* path; §14 is the *connection* path.

## The one publish API

Apps publish through exactly one action surface. There is no
"build → sign → send" you call yourself. `PublishAction`
(`crates/nmp-core/src/publish/action.rs:40-50`):

```rust
enum PublishAction {
    Publish { handle: PublishHandle, event: SignedEvent, target: PublishTarget },
    Cancel  { handle: PublishHandle },
}
```

`PublishTarget` (`action.rs:28-32`) is `Auto` (NIP-65 via
`OutboxResolver`, per **D3**) or `Explicit { relays }` (the named D3
opt-out). The event arrives **pre-signed**: the kernel ledger signs
once via the active `IdentityModule` and never re-signs on retry — id +
sig are preserved across the whole lifecycle (`action.rs:34-39`).

`PublishModule` (`action.rs:85-164`) is the `ActionModule` impl
(`crates/nmp-core/src/substrate/action.rs:10-84`). `start()` rejects an
event with empty `id`/`sig` (`action.rs:99-104`). The action ledger sees
a coarse `PublishStep` (`Planning`/`Dispatching`/`Waiting`/`Done`); the
fine per-relay timing is the engine's, not the ledger's
(`action.rs:53-60`, `subsystems.md:155`).

## Read-vs-write API split

| Concern | Mechanism | Surface |
|---|---|---|
| **Write** an event | dispatch `PublishAction::Publish` | action only — no direct relay/store call |
| **Cancel** a publish | dispatch `PublishAction::Cancel { handle }` | action only |
| **Observe** publish status | open `PublishStatusView` | store/view subscription — never a return value |
| **Read** events back | normal `ViewModule` subscription | store subscription |
| Pick relays | `OutboxResolver` (D3 automatic) | engine-internal; app only via `Explicit` opt-out |
| Decide retry | `classify_ack` in the engine | engine-internal; never the dispatcher, never native (D7) |

Writes are **actions**; reads are **store/view subscriptions**. There is
no API that returns a publish result synchronously and no API that lets
you publish without a renderable ledger row (`subsystems.md:153`).

## Publish-action state diagram

Per-(event, relay) state machine
(`crates/nmp-core/src/publish/state.rs:152-176`). The engine
(`engine.rs`) drives time; the state machine is pure
(`state.rs:18-21` — no wall clock, no threads, no sockets).

```text
PublishAction::Publish(handle, signed_event, target)
        │
        ▼
   [Planning]  OutboxResolver.resolve(author, p_tags, target)
        │            │
        │            └─ empty set → emit_no_targets → RecentFailure
        │                            + Err(NoTargets)            [terminal]
        ▼
   persist RelayPlan (store.upsert)  ◄── crash here resumes from store
        │
        ▼
   per relay r in resolved set:
   ┌──────────────────────────────────────────────────────────┐
   │  Pending ──dispatch──► InFlight{sent_at_ms, attempt}        │
   │                              │                              │
   │            RelayAck arrives (on_ack) → classify_ack         │
   │      ┌───────────────┬──────────────────┬───────────────┐  │
   │   ok=true        AckClass::         AckClass::       AckClass:: │
   │      │           Permanent         AuthRequired      Transient  │
   │      ▼               ▼                 │                 │     │
   │   Ok{acked}   FailedAfterRetries   Reauth (signer,   ScheduleRetry │
   │  [terminal]    [terminal]          ≤1 retry) │       (backoff:  │
   │                                    └─►InFlight│        1s,4s,16s)│
   │                                               └─►InFlight       │
   │                  (retries exhausted) ────────────► FailedAfterRetries │
   └──────────────────────────────────────────────────────────┘
        │
        ▼
   all relays terminal → PublishOutcome derived (ledger):
     all Ok                → Accepted { relays }
     some Ok, some failed  → Mixed { accepted, failed }
     none Ok               → FailedAfterRetries { failed }
     no relays at all      → NoTargets
     Cancel dispatched     → Cancelled
```

`Cancel` (`engine.rs:185-204`) removes the in-flight row and marks every
non-terminal relay `FailedAfterRetries{reason:"cancelled"}`, then deletes
the store row. Late acks for a settled relay are held idempotently
(`state.rs:266-275` — D7 capability idempotence).

Retry policy (`state.rs:213-242`): default 3 transient attempts
(initial + 2 retries), backoff `1s → 4s` (factor 4), 1 auth-required
re-auth. The 16s slot is `transient_max_retries = 4`.

## `RelayAck` envelope schema

`crates/nmp-core/src/publish/state.rs:47-95`. The dispatcher reports
*transport facts only* (D7) — no policy hint, no `is_transient` flag:

```rust
struct RelayAck {
    relay_url: RelayUrl,
    ok:        bool,                       // NIP-20 OK boolean / transport success
    code:      String,                     // "" | "blocked" | "pow" | "rate-limited"
                                           //  | "restricted" | "invalid" | "duplicate"
                                           //  | "auth-required" | "timeout" | "io"
                                           //  | "connection-reset"
    message:   String,                     // human-readable from relay/transport
    details:   Option<serde_json::Value>,  // NIP-42 challenge / NIP-13 difficulty / retry-after
}
```

Classification is the engine's job (`classify_ack`, `state.rs:133-150`):
`auth-required → AuthRequired`; the permanent set
(`blocked`/`pow`/`rate-limited`/`restricted`/`invalid`/`duplicate`) →
`Permanent` (no retry); **everything else, including unknown tokens →
`Transient`** (conservative retry-once-with-backoff). `ok=true` never
reaches the classifier. The dispatcher trait
(`crates/nmp-core/src/publish/traits.rs:121-123`) returns
`Vec<RelayAck>` and **must not** call `classify_ack`.

## Durable retry queue + resume

`PublishStore` (`traits.rs:174-193`) is the M3-LMDB seam; today the
shim is `InMemoryPublishStore`. The engine persists a `PublishRecord`
**before any send** (`engine.rs:179`) so a crash mid-dispatch resumes.
`pending_retries` (`traits.rs:185-192`) stores per-relay retry
deadlines so a process that died one tick after scheduling a 4s retry
resumes with the same wait — no thundering herd, no silent drop.
`resume_from_store` (`engine.rs:109-130`) replays pending records at
kernel boot. This is the offline action queue from
`docs/product-spec/subsystems.md:377-390` (`scheduled_at` order, 7-day
TTL, `created_at` fixed at original dispatch).

## D6: errors never cross FFI

Two failure planes, both observable-state-only
(`crates/nmp-core/src/publish/mod.rs:11-31`):

- **Per-relay** → `RecentFailure` rows on the snapshot +
  `PublishOutcome::Mixed`/`FailedAfterRetries` on the ledger.
- **Engine-level** (`PublishEngineError::DuplicateHandle`/`NoTargets`/
  `Store`, `engine.rs:41-46`) → returned in-process so the actor can
  branch, then mapped via `record_engine_error` /
  `engine_error_to_failure` (`engine.rs:273-283`) into the *same*
  `RecentFailure` shape before the boundary crosses. The FFI boundary
  only ever sees state — never an exception, never `Result<T,E>`.

## `PublishStatusView` snapshot (D5 + D8)

`crates/nmp-core/src/publish/view.rs:55-61` —
`PublishStatusSnapshot { rev, in_flight, recent_ok, recent_errors }`.
`in_flight` is bounded by what the app dispatched; `recent_ok` /
`recent_errors` are ring-buffer-capped (default 32 each,
`view.rs:20-21`). `rev` is monotonic (`view.rs:114-116`) so the
projection bridge coalesces under the D8 ≤60 Hz/view budget. The view's
`dependencies()` is a single projection key
(`view.rs:135-146`) — it is engine-driven, not kernel-event-driven, so
`on_event_inserted` returns `None` (no per-event allocation).

Observe it by opening the view; never poll the engine.

## Anti-patterns

1. **Bypassing the publish engine.** Building, signing, and websocket-
   sending an event by hand. No ledger row, no retry, no resume, no
   outbox routing — defeats D3/D4 and `subsystems.md:153`.
2. **Per-action error types across FFI.** `PublishEngineError` /
   `SignerError` are Rust-internal. The boundary sees `RecentFailure`
   rows and a coarse `PublishOutcome` only (D6).
3. **Storing pending publishes in the platform.** SwiftData/Room
   parallel to `PublishStore` double-tracks state and breaks D4
   single-writer + the resume contract.
4. **Native deciding retry policy.** A capability/dispatcher that
   returns "retry after Ns" or an `is_transient` flag. Classification
   is the engine's via `classify_ack` (D7).
5. **Passing relays to the publish call for the common case.**
   `Explicit` is the audited opt-out, not the default. `Auto` +
   `OutboxResolver` is D3.
6. **Polling `snapshot()` from a render loop.** Open
   `PublishStatusView` and react to `rev` deltas; the snapshot
   accessor is for the actor/tests.

## See also

- [05 — Kernel substrate — the 5 trait families](05-substrate-traits.md)
- [10 — Outbox routing (NIP-65)](10-outbox-routing.md)
- [11 — Sessions + signers + identity scopes](11-sessions-signers.md)
- [14 — Subscription lifecycle + relay manager + NIP-42](14-relay-manager.md)
- [16 — Capabilities (D7)](16-capabilities.md)
