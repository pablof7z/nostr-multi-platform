---
title: Kernel Actor Thread Model — Synchronous Single-Actor State Machine
slug: kernel-actor-thread-model
summary: The nmp-core kernel is a single-actor synchronous state machine with no async runtime dependency, runnable on any thread including the iOS main thread via UniFF
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-20
updated: 2026-05-26
verified: 2026-05-20
compiled-from: conversation
sources:
  - session:c0765978-d977-4400-8274-96df7682b126
  - session:1670fcb8-f275-498c-975b-8bd912331ded
  - session:8a8aefe0-93b0-405d-99da-0c8ac39114c8
  - session:95156e27-58fe-4e26-9530-1778033c4559
---

# Kernel Actor Thread Model — Synchronous Single-Actor State Machine

## Kernel Actor & Thread Model

The nmp-core kernel is a single-actor synchronous state machine with no async runtime dependency, runnable on any thread including the iOS main thread via UniFFI. UniFFI is used for command/action calls and capability hooks, and requires interior mutability (Arc<Mutex<>> or atomics) because foreign code may call from multiple threads. The pure synchronous KernelReducer design (host drives the clock, no embedded event loop) is architecturally justified because embedding tokio would create two-loop conflicts on iOS (GCD), Android (Looper), and browser (JS event loop). nmp-core cannot adopt nostr-relay-pool because it spawns tokio tasks with no external-step API, which is incompatible with NMP's zero-tokio synchronous reducer model. The nmp-nostr-lmdb crate uses Tokio only for its sync feature (oneshot channels) and test harness, not as an async runtime. A UniFFI sweep replaces handwritten Decodables and bespoke nmp_app_* symbols, scoped to commands independently of FlatBuffers progress. ProtocolCommandContext::now_secs() uses direct kernel.now_secs() access when a kernel handle is attached, bypassing the KernelClockAdapter RefCell to avoid a double-borrow panic.

<!-- citations: [^c0765-3] [^1670f-8] [^8a8ae-6] [^95156-4] -->
## See Also

