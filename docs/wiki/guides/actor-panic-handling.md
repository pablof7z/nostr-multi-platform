---
title: Actor Panic Handling
slug: actor-panic-handling
topic: ffi-runtime
summary: "The actor thread in nmp-core uses `std::panic::catch_unwind` to capture panic messages from `Keys::generate()`, `Secp256k1::new()`, or any other Rust code, and"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-19
updated: 2026-05-21
verified: 2026-05-19
compiled-from: conversation
sources:
  - session:27a9cbf3-1348-44f6-bc0f-95a0a9c6ad84
  - session:c4b2e655-ca6b-42d2-9383-89bf52215d0a
  - session:1c093fa5-0f0e-4dee-bf38-99781e763f13
---

# Actor Panic Handling

## Panic Capture and Reporting

The actor thread in nmp-core uses `std::panic::catch_unwind` to capture panic messages from `Keys::generate()`, `Secp256k1::new()`, or any other Rust code, and sends them to the Swift callback instead of silently killing the thread. Every FFI dispatch via the `send_cmd` helper on `NmpApp` logs an error if the actor command channel is disconnected, indicating the thread died. On the Swift side, the `KernelBridge` callback recognizes `"t":"panic"` frames from Rust and logs them as `NMP_ACTOR_PANIC` at fault level. PR-J (#208) wrapped three host observer invocation sites (action_registry deliver_result, event_observer notify, raw_event_observer notify) plus the FFI action-result-observer callback in `catch_unwind`, and added the D15 lint. Do not wrap the actor command drain in `catch_unwind`; internally-generated commands should panic-loud on real bugs. 65 `panic!` calls exist in nmp-core production code (mostly relay pool assertions); none are on hot FFI paths. The Rust kernel's relay worker treats Interrupted (EINTR) as a retryable timeout alongside WouldBlock and TimedOut, preventing tight reconnect loops. The actor loop's compute_wait function clamps to a 1ms minimum floor to prevent busy-waiting.

<!-- citations: [^27a9c-1] [^c4b2e-1] [^1c093-1] -->
## Stack Size Constraints on iOS

The iOS device default thread stack is 512 KB (vs 8 MB on simulator/macOS), which is the suspected cause of the actor thread panicking during secp256k1 context creation or `Keys::generate()` on physical devices. <!-- [^27a9c-2] -->
