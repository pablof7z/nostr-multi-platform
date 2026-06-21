# ADR-0024 — Async capability protocol for non-blocking HTTP executors

**Date:** 2026-05-21
**Status:** Accepted (implemented as the in-kernel `capability_worker` FIFO
thread). The shipped design differs from the original proposal below: there is
**no** inbound C symbol `nmp_app_deliver_capability_result`, **no** `resume()`
on `ActionModule`, and **no** `ZapModule` saga state machine. The actor enqueues
a `CapabilityWorkItem` onto a single dedicated worker thread that drains via
blocking `recv` and re-enters the actor with
`ActorCommand::CapabilityResultReady` — see
`crates/nmp-core/src/actor/capability_worker.rs` and ADR-0040 (capability-worker
seam). The Context and the "non-blocking re-entry through a single owner"
principle below remain accurate; the named API surface was superseded.
**Related:** ADR-0023 (`HttpCapability` over the synchronous capability socket),
ADR-0040 (capability-worker seam — the as-built design)
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

## Decision (as built)

The actor must never block on a multi-second capability. The non-blocking path
is the in-kernel **`capability_worker` FIFO thread**, not a host re-entry symbol:

- The actor enqueues a `CapabilityWorkItem` (carrying a `correlation_id` and
  the active `account_id`) onto a single dedicated worker thread instead of
  calling `dispatch_capability` inline.
- The worker drains its queue via blocking `recv` (D8 — never a poll), runs the
  capability off the actor thread, and re-enters the actor with
  `ActorCommand::CapabilityResultReady`.
- The actor applies the result on its own thread (D4 — single writer), checking
  the carried `account_id` against live identity state so a result for a
  removed/switched account is dropped, never misapplied.

The single-FIFO-worker shape (as opposed to per-op thread spawn) is required for
correct per-account persist/forget ordering; see ADR-0040 §3 for the full
rationale and `crates/nmp-core/src/actor/capability_worker.rs` for the code.

> **Superseded proposal.** The original ADR specified a host-driven two-phase
> protocol: Phase 1 fire-and-forget through `dispatch_capability`, Phase 2
> inbound re-entry via a new C symbol `nmp_app_deliver_capability_result`, plus
> a `resume()` entry point on the `ActionModule` trait. None of that was built
> — the C symbol does not exist, `ActionModule` has no `resume()`, and the
> result re-enters through the in-kernel worker described above. NIP-57 zaps
> likewise did not become a multi-state saga (see Consequences).

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

- **The actor never stalls on a multi-second capability.** The single-actor
  invariant (D3) holds: while an HTTP-class capability runs on the worker
  thread, the actor keeps servicing `ActorCommand`s, snapshot ticks, and relay
  frames.
- **The synchronous socket is untouched for microsecond-class capabilities.**
  Microsecond-class work (Keychain-class) can still resolve inline; the
  worker-thread path is what removes multi-second I/O from the actor.
- **No new FFI surface.** Re-entry is fully in-kernel via
  `ActorCommand::CapabilityResultReady` — the originally-proposed
  `nmp_app_deliver_capability_result` C symbol and the Swift completion-handler
  callback were never needed.
- **NIP-57 zaps did not become a multi-state saga.** The shipped NIP-57 path
  performs its LNURL two-leg fetch as a single blocking capability call routed
  through the worker thread, not an `Idle → AwaitingLnurlInfo →
  AwaitingInvoice → Done` state machine driven by repeated re-entry. The
  `ActionModule` trait gained no `resume()` arm.
