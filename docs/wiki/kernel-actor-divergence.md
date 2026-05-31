---
title: Kernel Actor Implementation Divergence
slug: kernel-actor-divergence
summary: "The shipped kernel actor uses `std::sync::mpsc` and `std::thread` with blocking tungstenite, diverging from the aim.md design which describes `flume` and `tokio"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-29
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:7f0f0c78-d1aa-49db-b659-c9cf49827117
  - session:582fca30-be51-4861-bb16-3788610c6fb7
  - session:fe79b2c4-3f04-4fc9-8dde-08f19a3190b4
  - session:c0765978-d977-4400-8274-96df7682b126
  - session:2c4adc99-0b1b-430c-8594-834da3ab4cef
  - session:cd2b6122-2b7c-43fc-941b-c51e79ffc691
  - session:594b7c34-efd1-4461-81ad-9fa33a6e76f9
  - session:3a906f87-ee2b-4d3a-9d5f-e82ccab29349
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# Kernel Actor Implementation Divergence

## Kernel Actor Implementation Divergence

The shipped kernel actor uses `std::sync::mpsc` and `std::thread` with blocking tungstenite, diverging from the aim.md design which describes `flume` and `tokio`. Putting tokio rt-multi-thread inside an iOS staticlib compiled via UniFFI creates unnecessary complexity and binary size bloat; tokio is used only in the nmp-nostr-lmdb crate for sync primitives (oneshot channels) and test harness, not as an async runtime, and Doctrine D8 (enforced by doctrine-lint) explicitly bans tokio::time::sleep and tokio::time::sleep_until in all production code. The relay transport uses synchronous tungstenite on a dedicated relay thread with recv_timeout gating and feeds events through a flume channel to the sync actor. WasmRuntime drives the real KernelReducer, the same reducer used by the native actor. All Rust actor output frames are wrapped in a T103 envelope as `{"t":"snapshot","v":{...}}` or `{"t":"update","v":{...}}`, and the Swift bridge decode() must unwrap this envelope before decoding the inner KernelUpdate. The actor loop uses a dual-channel priority architecture where command_rx (unbounded) is drained via try_recv at the top of every loop iteration before waiting on relay_rx via recv_timeout, ensuring UI commands have near-zero latency regardless of relay event volume. The synchronous tungstenite send_event path is removed; all blocking dispatch wrappers run on a background queue, and publish_key_package is moved off the main thread. The Rust→Swift callback pipeline uses DispatchQueue.main.async with MainActor.assumeIsolated to call apply() on the MainActor from a Rust listener thread. V-59 tracks that the EventStore trait is missing kernel clock injection and uses SystemTime::now() in watermarks and queries. The executor_failure_returns_correlation_id test is a known flaky test caused by an actor-queue-depth timing race where the actor thread can drain the command before the test reads the depth. kernel/mod.rs grew to 2358 LOC, actor/dispatch.rs to 1967 LOC, and actor/mod.rs to 1852 LOC, worse than prior backlog numbers. The actor-thread freeze cluster (V-90 + V-54) blocks the entire kernel loop for up to 12 seconds during a bunker DM via op.wait(GIFT_WRAP_TOTAL_TIMEOUT), synchronous Keychain dispatch, and blocking sign_active calls. The V-90 capability-worker uses a single serialized FIFO mpsc thread (blocking recv, D8-compliant) rather than per-op thread spawn to prevent Keychain persist/forget reordering races during account switch. A result for a removed account in the capability-worker is dropped (D6 trace), never misapplied.

<!-- citations: [^7f0f0-10] [^582fc-18] [^fe79b-6] [^c0765-1] [^2c4ad-4] [^cd2b6-8] [^594b7-3] [^3a906-3] [^42908-7] [^4edd4-10] -->
## See Also

