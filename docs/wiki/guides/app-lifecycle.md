---
title: NmpApp Lifecycle and Shutdown
slug: app-lifecycle
topic: app-lifecycle
summary: The UniFFI runtime object exposes an explicit idempotent `shutdown()` method (not `close`, to avoid Kotlin friction from #2149)
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-29
updated: 2026-07-04
verified: 2026-06-29
compiled-from: conversation
sources:
  - session:3c942260-311d-4e00-8bcc-204045ea87b3
  - session:dcc80382-bcc0-45ea-8b9c-1a2fc741f872
---

# NmpApp Lifecycle and Shutdown

## Shutdown

The UniFFI runtime object exposes an explicit idempotent `shutdown()` method (not `close`, to avoid Kotlin friction from #2149). The #2397 scaffold fixed a real `shutdown()` deadlock where `actor_starter`'s captured `update_tx` kept the listener thread alive forever when the actor was never started; the fix adds `actor_starter.take()` to `shutdown()`.

`NmpApp::drop` in native-runtime drains the lifecycle gate before joining the actor. <!-- [^3c942-3ca70] -->

<!-- citations: [^3c942-0377b] [^3c942-2f987] -->

## Quiescence Testing

Quiescence tests for the UniFFI scaffold must be deterministic (channel/Condvar/Barrier-based, no sleeps) and regression-proven. The shutdown-during-in-flight-callback test must create a genuinely in-flight `on_update`, block it at a barrier, call `shutdown()`, release the barrier, and assert shutdown returns without deadlock or UAF. <!-- [^3c942-f858a] -->

## Callback Drain Gates (#2429)

#2429 is the M14-D blocker: drain gates (in_flight counter + Condvar) must be added to both ActionRegistry and LifecycleObserverSlot in nmp-core before the legacy C-ABI is deleted and the deferred `set_lifecycle_callback` is migrated.

Both `ActionRegistry` and `LifecycleObserverSlot` in nmp-core required new `in_flight` counter + Condvar drain gates (copied from `CapabilityCallbackGate`) before their callback interfaces could be safely exposed through UniFFI.

The `ResultObserverGate` in `nmp-core` snapshots and increments `in_flight` under the lock, then invokes the observer callback with no lock held, and `clear_result_observer` drains in-flight before returning. (Previously: ActionRegistry in nmp-core has no drain/clear gate for the result observer — it holds a mutex across the call — so its callback-interface migration and M14-D deletion are blocked until a drain gate is added; LifecycleObserverSlot uses a snapshot-then-call pattern with no in_flight counter + Condvar drain gate, so `set_lifecycle_callback` could not migrate until a drain gate is added.)

The UniFFI `LifecycleSink` callback interface uses `on_lifecycle_transition(phase: u32)`, copies the phase before the foreign call, holds no wrapper lock across the call, forbids reentrancy, and contains panics.

<!-- citations: [^3c942-e6ad9] -->
#2429 is the M14-D blocker: drain gates (in_flight counter + Condvar) must be added to both ActionRegistry and LifecycleObserverSlot in nmp-core before the legacy C-ABI is deleted and the deferred `set_lifecycle_callback` is migrated.

ActionRegistry in nmp-core has no drain/clear gate for the result observer — it holds a mutex across the call — so its callback-interface migration and M14-D deletion are blocked until a drain gate is added.

LifecycleObserverSlot in nmp-core uses a snapshot-then-call pattern with no in_flight counter + Condvar drain gate, so `set_lifecycle_callback` could not migrate until a drain gate is added; this is the same structural gap as ActionRegistry. <!-- [^3c942-ec5ec] -->

## Reset/Start Desync (NMP#2932)

`LifecycleCommand::Reset` rebuilds the kernel with an empty `active_account` but leaves `IdentityRuntime::active` set, so the following `Start`'s `restore_active_session` early-returns and never re-syncs the account — a silent desync. <!-- [^dcc80-ea0c6] -->
