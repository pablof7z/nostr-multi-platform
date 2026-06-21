---
title: Kernel Clock RefCell Bug
slug: kernel-clock-refcell-bug
topic: ffi-runtime
summary: "The `KernelClockAdapter::now_secs()` in `dispatch.rs` has a RefCell re-entrancy bug where `borrow()` panics when `borrow_mut()` is already held, caught by `catc"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-25
updated: 2026-05-27
verified: 2026-05-25
compiled-from: conversation
sources:
  - session:7b06d382-8fc2-4d52-bef5-f4d90e38cb2a
  - session:95156e27-58fe-4e26-9530-1778033c4559
  - session:e4861768-9a00-4d83-b7a3-a39d07749d1c
  - session:cd2b6122-2b7c-43fc-941b-c51e79ffc691
---

# Kernel Clock RefCell Bug

## KernelClockAdapter RefCell Re-entrancy Bug

The `KernelClockAdapter::now_secs()` in `dispatch.rs` has a RefCell re-entrancy bug where `borrow()` panics when `borrow_mut()` is already held, caught by `catch_unwind` returning 0. This causes `created_at=0` in kind:9734 when signed via the actor path, which is an invalid timestamp. The fix is to change `KernelClockAdapter::now_secs()` to use `try_borrow()` with a `SystemTime::now()` fallback so it returns a valid timestamp even when `borrow_mut()` is held. However, when a kernel handle is attached, `ProtocolCommandContext::now_secs()` calls `self.kernel.as_deref().now_secs()` directly, bypassing `KernelClockAdapter` and its RefCell entirely, avoiding the double-borrow panic.

The browser wasm WebSocket handler panics with 'time not implemented on this platform' (std::time::Instant::now in mark_lane_connected) followed by 'RefCell already borrowed' in relay_pool.rs:100 — this is a pre-existing issue in nmp-network/nmp-wasm relay handling, not introduced by PR #582, and should be filed as a separate ticket. <!-- [^e4861-16] -->

Additionally, kernel initialization silently degrades to an in-memory store when LMDB open failure occurs, with no user notification of the fallback. <!-- [^cd2b6-4] -->

<!-- citations: [^7b06d-3] [^95156-1] -->
