# ADR-0024 — Async capability protocol for non-blocking HTTP executors

**Date:** 2026-05-21
**Status:** Proposed (NIP-57 zaps precondition — async capability seam)
**Related:** ADR-0023 (`HttpCapability` over the synchronous capability socket)
**Doctrines invoked:** D3 (single-actor invariant — one command at a time, no
stalls), D7 (host owns transport, kernel owns policy and correlation), D8 (no
async runtime in the kernel — the actor advances by `ActorCommand`, not by
`.await`)

## Context

ADR-0023 shipped `HttpCapability` over the **synchronous** capability socket
and named its own escape hatch: "a second ADR will specify the non-blocking
design before any executor uses `HttpCapability`." This is that ADR.

`dispatch_capability()` blocks the actor thread until the host callback
returns. For `KeyringCapability` that is sound — a Keychain read is
microseconds. For HTTP it is not: an LNURL GET/POST (NIP-57 zap legs) takes
**seconds**. Blocking the actor for seconds violates the single-actor
invariant (D3) — while it waits, no other `ActorCommand` runs, no snapshot
tick emits, no relay frame is serviced. The `ZapModule` executor therefore
cannot use `HttpCapability` until a non-blocking path exists.

## Decision

Add a **two-phase async capability protocol**. The synchronous socket is kept
for microsecond-class capabilities; only HTTP-class capabilities use this.

**Phase 1 — outbound (fire-and-forget).** The executor mints a `correlation_id`
(nanoid), calls `dispatch_capability()`, and **returns immediately**. The
request is handed to a host callback that runs the HTTP call on its own
thread. The `correlation_id` is included in the C callback so the host can
echo it back. The actor thread is never blocked.

**Phase 2 — inbound re-entry.** When the host finishes, it calls a new C-ABI
symbol `nmp_app_deliver_capability_result(app, correlation_id, result_json)`.
That enqueues `ActorCommand::CapabilityResultReady { correlation_id, result_json }`.
The actor resumes the executor's pending state — keyed by `correlation_id` —
inside the normal actor tick.

**Executors become resumable state machines.** An executor that needs an async
capability submits the request, records its pending `correlation_id`, and
returns. A `resume(correlation_id, result_json)` entry point on `ActionModule`
(or an `ExecutorFn` that dispatches a follow-up `ActorCommand` when it sees
`CapabilityResultReady` in the registry) handles the result arm.

## Alternatives considered

- **Keep `dispatch_capability` synchronous for HTTP.** Rejected: a multi-second
  actor stall is a direct D8/D3 violation; ADR-0023 already scoped this as MVP-
  only.
- **Thread pool inside the executor.** Rejected: spawning worker threads that
  hold result state outside the actor creates shared mutable state — a D3
  violation. The actor must remain the single owner of progress.
- **Tokio async runtime.** Rejected: D8 (the kernel has no async runtime — the
  actor advances by `ActorCommand`) and it adds a large dependency surface to
  `nmp-core`. The `ActorCommand` re-entry already gives us resumption.

## Consequences

- **`ZapModule` (NIP-57) is a saga.** build kind:9734 `UnsignedEvent` → LNURL
  GET (async) → LNURL POST (async) → bolt11 → wallet. That is **two async
  capability hops**, so `ZapModule` needs a small state machine: `Idle →
  AwaitingLnurlInfo → AwaitingInvoice → Done`, each transition driven by a
  `CapabilityResultReady` arm.
- **`correlation_id` is executor-minted.** A nanoid created when the request is
  submitted; carried through the C callback and echoed back by the host —
  the same correlation discipline already used for `dispatch_action` results.
- **The synchronous path is untouched.** `KeyringCapability` and any
  microsecond-class capability keep using `dispatch_capability` as-is. Only
  HTTP-class capabilities opt into the async protocol — no migration churn.
- **`ActionModule` gains a re-entry seam.** Either a `resume(correlation_id,
  result_json) -> ExecutorFn` method, or the existing `ExecutorFn` learns to
  read `CapabilityResultReady` from the registry and dispatch the next step.
  The exact shape is settled when the seam is built.

## Implementation checklist

Required before `ZapModule` can land — none of this is done in this PR:

- [ ] Add `ActorCommand::CapabilityResultReady { correlation_id: String, result_json: String }`
- [ ] Add C-ABI `nmp_app_deliver_capability_result` in `crates/nmp-core/src/ffi/`
- [ ] Add `resume()` to the `ActionModule` trait (or equivalent mechanism)
- [ ] Swift: call `nmp_app_deliver_capability_result` from the `URLSession` completion handler
- [ ] `ZapModule` state machine: two async HTTP hops (LNURL GET, LNURL POST)

## Validation

Documentation only — no code changes in this PR. The checklist above gates the
`ZapModule` implementation work that follows.
