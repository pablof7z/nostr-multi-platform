---
title: Kernel Boot Initial Emit — Guaranteed Post-Start Snapshot Frame
slug: kernel-boot-initial-emit-guarantee
summary: The kernel guarantees one initial snapshot emit after Start because changed_since_emit starts as true. store_open_failure rides this first frame. If it's not delivered, the bug is in the app shell's listener timing.
tags:
  - kernel
  - actor
  - startup
  - snapshot
volatility: cold
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:d0690875-a693-48ef-ac6f-31a92f5699cc
---

# Kernel Boot Initial Emit — Guaranteed Post-Start Snapshot Frame

> The kernel guarantees one initial snapshot emit after Start because changed_since_emit starts as true. store_open_failure rides this first frame. If it's not delivered, the bug is in the app shell's listener timing.

## Guaranteed Initial Emit

The kernel is constructed with `changed_since_emit: true` (`kernel/mod.rs:1718`). `store_open_failure` is set at construction time (`:1757`). The first running tick after `Start` flushes this because `flush_due` = `running && changed_since_emit()` (`tick.rs:38-49`). This means the kernel is designed to always emit an initial snapshot frame after `Start`, and that frame carries `store_open_failure` if the store failed to open. [^d0690-15]

## When the Emit Fires

`emit_now` / `flush_due` fire only when `running && kernel.changed_since_emit()` (`tick.rs:38-49`). The actor blocks on `command_rx.recv()` before even constructing the kernel (`mod.rs:1175`). The actor deliberately suppresses emits when state is unchanged — `idle_ticks_do_not_emit_snapshots_when_state_unchanged` is guarded as a 'D8 regression' in `tick.rs:108`. However, the boot path guarantees at least one emit because `changed_since_emit` starts as `true`. [^d0690-16]

## store_open_failure Delivery

`store_open_failure` is set at kernel construction and rides the first post-Start frame. If the first frame is not delivered to the shell's `apply()` callback, the root cause is in the app shell — either the listener is subscribed after `Start`, or a rev-guard off-by-one is dropping the first frame. It is not a missing NMP capability. [^d0690-17]

## See Also
- [[podcast-player-polling-incident|Podcast-Player Polling Incident — Second-App ADR-0025 Anti-Pattern]] — related guide
- [[d1-snapshot-before-relay-io|D1 Doctrine — First Snapshot Must Precede Relay I/O]] — related guide
- [[lmdb-corruption-and-diagnostics|LMDB Diagnostics — Corruption Counters and Open-Failure Surfacing]] — related guide

