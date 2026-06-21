---
type: episode-card
date: 2026-05-19
session: 27a9cbf3-1348-44f6-bc0f-95a0a9c6ad84
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/27a9cbf3-1348-44f6-bc0f-95a0a9c6ad84.jsonl
salience: root-cause
status: active
subjects:
  - nmp-core-ffi
  - actor-thread-lifecycle
  - ios-stack-limit
supersedes: []
related_claims: []
source_lines:
  - 3202-3270
  - 3536-3548
  - 3596-3601
  - 3620-3634
  - 3671-3671
  - 3730-3750
captured_at: 2026-06-18T04:19:13Z
---

# Episode: Actor thread panic observability on iOS device

## Prior State

Actor thread panics (e.g. from secp256k1 context creation or Keys::generate()) silently killed the thread with no diagnostic surface — all FFI commands were swallowed and the host app saw nothing, making device-only hangs invisible and un diagnosable.

## Trigger

User reports that login buttons do nothing on physical iOS device. Theory: iOS pthreads get a 512 KB stack vs 8 MB on simulator/macOS, so deep Rust callchains (secp256k1, getrandom) may overflow or panic silently at runtime.

## Decision

Wrapped the actor thread's main loop in std::panic::catch_unwind so panic payloads are surfaced as a new "t":"panic" frame type through the existing Swift update callback. Added send_cmd helper on NmpApp that logs disconnection errors when the actor command channel is dead. Swift-side KernelBridge now recognizes and fault-logs panic frames as NMP_ACTOR_PANIC.

## Consequences

- Panics from the Rust actor are now observable from Swift; previously they were silently swallowed.
- New panic frame type ("t":"panic") requires host-side handling contract.
- Two Swift compilation blockers were fixed to unblock the device build: MarmotStore changed from kernel.relayEditRows (wrong type) to a closure injection from KernelModel, and nostrConnectURI gained its required relay parameter.
- Linker warnings about iOS-simulator version 26.5 objects linked against deployment target 17.0 remain unresolved but are non-fatal.

## Open Tail

- Root cause of device login freeze still unconfirmed — catch_unwind will reveal the exact panic message on next device run.
- If stack overflow is confirmed, may need to increase pthread stack size or lazily initialize secp256k1 context.
- Device was unavailable for install at session end; testing is pending.

## Evidence

- transcript lines 3202-3270
- transcript lines 3536-3548
- transcript lines 3596-3601
- transcript lines 3620-3634
- transcript lines 3671-3671
- transcript lines 3730-3750

