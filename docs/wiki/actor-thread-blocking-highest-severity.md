---
title: Synchronous Blocking on the Kernel Actor Thread Is a Correctness Showstopper
slug: actor-thread-blocking-highest-severity
summary: Any synchronous wait or blocking call on the kernel actor thread must be treated as highest-severity — it causes visible device freezes that unit tests cannot detect.
tags:
  - architecture
  - correctness
  - actor
  - threading
  - severity
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-28
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
  - session:3a906f87-ee2b-4d3a-9d5f-e82ccab29349
---

# Synchronous Blocking on the Kernel Actor Thread Is a Correctness Showstopper

> Blocking the kernel actor thread — even briefly — stalls the entire event loop and produces visible device freezes. This is a correctness defect, not architectural debt, and must be treated with higher severity than structural violations because it is invisible to unit tests yet immediately user-visible.

## Details

### Why It Is Highest Severity
- The kernel actor thread serializes all message dispatch. Any synchronous wait blocks every other message from being processed until the wait completes.
- Freezes are user-visible (UI hangs, dropped inputs, watchdog kills on mobile) but do not appear in unit or integration tests, which typically run without real timing constraints.
- Unlike architectural seam violations, actor-thread blocking can cause production incidents on the first real-world use.

### Known Violation Sites (as of audit)
- `nmp-nip17/src/dm_send.rs:221` — calls `op.wait(GIFT_WRAP_TOTAL_TIMEOUT)` blocking the entire kernel loop for up to **12 seconds** during a bunker DM send.
- `nmp-ffi/src/capability.rs:56` — performs synchronous Keychain dispatch on the actor thread.

### Flaky-Test Interaction
- `executor_failure_returns_correlation_id_and_enqueues_failed_terminal` is flaky because it asserts `depth_after > depth_before` on an actor queue that can drain on its own thread before the test reads the depth. This is a direct manifestation of the same underlying concurrency hazard.

### Detection Heuristics
- Search for `.wait(`, `thread::sleep`, `block_on(`, `std::sync::Mutex::lock` (held across await points), or any FFI call documented as synchronous inside actor `handle` / `receive` methods.
- Any timeout parameter passed to a blocking call on the actor thread is a red flag — the timeout is the *maximum* freeze duration, not a mitigation.

### Remediation Pattern
- Move the blocking work to a dedicated `tokio::task::spawn_blocking` or a separate thread, then send the result back to the actor via a message/channel.
- For Keychain and other OS-synchronous APIs, wrap in an async shim that dispatches off the actor thread before awaiting.

### Triage Priority
- Treat actor-thread blocking bugs as **P0** — above architectural violations (P1) and above normal debt (P2/P3).
- Do not defer to a migration window; patch immediately or gate the feature behind a flag until fixed.

<!-- citations: [^3a906-1] -->
### Why It Is Highest Severity
- The kernel actor thread serializes all message dispatch. Any synchronous wait blocks every other message from being processed until the wait completes.
- Freezes are user-visible (UI hangs, dropped inputs, watchdog kills on mobile) but do not appear in unit or integration tests, which typically run without real timing constraints.
- Unlike architectural seam violations, actor-thread blocking can cause production incidents on the first real-world use.

### Known Violation Sites (as of audit)
- `nmp-nip17/src/dm_send.rs:221` — calls `op.wait(GIFT_WRAP_TOTAL_TIMEOUT)` blocking the entire kernel loop for up to **12 seconds** during a bunker DM send.
- `nmp-ffi/src/capability.rs:56` — performs synchronous Keychain dispatch on the actor thread.

### Detection Heuristics
- Search for `.wait(`, `thread::sleep`, `block_on(`, `std::sync::Mutex::lock` (held across await points), or any FFI call documented as synchronous inside actor `handle` / `receive` methods.
- Any timeout parameter passed to a blocking call on the actor thread is a red flag — the timeout is the *maximum* freeze duration, not a mitigation.

### Remediation Pattern
- Move the blocking work to a dedicated `tokio::task::spawn_blocking` or a separate thread, then send the result back to the actor via a message/channel.
- For Keychain and other OS-synchronous APIs, wrap in an async shim that dispatches off the actor thread before awaiting.

### Triage Priority
- Treat actor-thread blocking bugs as **P0** — above architectural violations (P1) and above normal debt (P2/P3).
- Do not defer to a migration window; patch immediately or gate the feature behind a flag until fixed.

## See Also
- [`half-landed-migration-is-not-done`](half-landed-migration-is-not-done)
- [`noop-substrate-types-are-intentional`](noop-substrate-types-are-intentional)
- [[half-landed-migration-is-not-done|half landed migration is not done]] — related guide
- [[noop-substrate-types-are-intentional|noop substrate types are intentional]] — related guide
