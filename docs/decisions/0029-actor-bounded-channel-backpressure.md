# ADR-0029 — Actor Queue Observability And Backpressure Policy

- **Status:** Accepted
- **Date:** 2026-05-22
- **Updated:** 2026-06-28

## Decision

The native FFI actor inbox is bounded. It uses one waking `ActorMail` channel
for commands and relay events, with a capacity of 4096 queued mail items.
Native dispatch remains fire-and-forget: callers use a nonblocking send path
and never wait for actor capacity.

When the inbox is full, newly-arriving commands are shed at the Rust command
sender. Already-accepted commands retain FIFO order. The shed-load policy is
drop-newest, not drop-oldest, because actor commands can carry user intent and
must not be reordered by removing earlier accepted commands. Every command shed
because of a full inbox increments a monotone command-drop counter on the shared
sender handle.

The diagnostic contract is:

- `actor_queue_depth` reports accepted commands waiting for actor dispatch;
- the command-drop counter reports bounded-inbox shed-load;
- relay backlog drops remain counted by the actor lane scheduler;
- command dispatch never blocks native callers.

Backpressure still belongs at typed workload boundaries where the workload has
domain-specific semantics. The bounded actor inbox is the memory ceiling and
last-resort shed-load primitive for FFI/native command floods; it does not move
retry, recovery, or routing policy into native code.

## Consequences

- Do not add sleep/poll loops to manage actor pressure.
- Do not make native callers decide which commands are recoverable.
- Do not replace the bounded actor inbox with `mpsc::channel`.
- Do not report a command as queued unless the bounded send accepted it.
- Preserve accepted-command ordering; if the lane is full, shed the new command
  and count the drop.
