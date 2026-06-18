# Codex review — #1493 P5: event-driven handshake cancel (drop 200ms poll)

Branch: `fix/1493-p5-handshake`
Scope: `crates/nmp-signer-broker/` — replace the 200ms `recv_timeout` cancel-flag
re-poll in the NIP-46 handshake with a `crossbeam_channel::select_biased!` over
(cancel_rx, deadline timer, inbound_rx). D8 "no polling, ever".

## Verdict: clean — no correctness issues found (diff review only)

Codex confirmed the load-bearing paths:

- **Cancel liveness cannot be missed.** `cancel()` does `cancel.store(true, Release)`
  then `cancel_tx.try_send(())`. If the send somehow does not deliver, `guard.take()`
  drops the session's `cancel_tx`, so `recv(cancel_rx)` wakes as *disconnected*.
  Treating either a delivered `()` or a disconnect as `Cancelled` is correct for this
  one-shot ownership model.
- **`select_biased!` ordering is correct.** Cancel first → liveness under inbound
  noise. Deadline before inbound → a flooded inbound queue cannot extend the step
  timeout.
- **`after(remaining)` per iteration is a deadline-bound wait, not polling.**
  `remaining` is recomputed from a fixed `Instant`, so stray events cannot accumulate
  timeout drift beyond normal scheduling overrun.
- **`mpsc` → `crossbeam_channel::unbounded` is behavior-preserving** for the
  steady-state dispatcher: `while let Ok(event) = inbound_rx.recv()` still exits when
  all senders drop.
- **`never_cancel()` test helper** (leaks a `Sender` via `mem::forget` so `cancel_rx`
  never disconnects) is acceptable, process-scoped, test-only scaffolding.
- **No spurious `Cancelled`.** The `cancel_tx` is owned by `ActiveSession`; a
  disconnect means the session was cancelled, superseded, or removed — all of which
  *should* abort the in-flight handshake.

Tests: all 45 `nmp-signer-broker` tests pass (incl. the now-channel-driven
`cancellation_aborts_with_cancelled_error`); `--workspace-d8` no-polling sweep clean
(0 findings).
