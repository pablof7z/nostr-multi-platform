---
title: Channel Disconnect Handling
slug: channel-disconnect-handling
topic: ffi-runtime
summary: Rust channels must block with recv()/recv_timeout() or drain with try_recv() (not in a sleep loop); iOS must consume ViewBatch snapshots pushed by the kernel an
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-26
updated: 2026-06-19
verified: 2026-05-26
compiled-from: conversation
sources:
  - session:f26050da-6d8a-4128-9179-4088a9df94b9
  - session:cd2b6122-2b7c-43fc-941b-c51e79ffc691
  - session:019edc84-6e5c-74a2-9ed9-57938dae31a1
  - session:019edc92-b628-7ce1-be8a-c3d1013f2969
  - session:11850f79-923f-4a2a-a921-a4b9bec47c6c
  - session:019edcba-b578-71f3-be33-f670962f11a7
---

# Channel Disconnect Handling

## Error Handling

Rust channels must block with recv()/recv_timeout() or drain with try_recv() (not in a sleep loop); iOS must consume ViewBatch snapshots pushed by the kernel and use AVFoundation/NWPathMonitor/NotificationCenter callbacks for OS events; background persistence must piggy-back on an existing event tick with a wall-clock gate.

The `RecvTimeoutError::Disconnected` case must be distinguished from `Timeout`. `Disconnected` means the channel closed and the loop should break, whereas `Timeout` is a normal idle tick and the loop should continue.

The NIP-47 wallet connection has no heartbeat or reconnect path, which allows connections to silently go stale after UNAUTHORIZED errors or relay disconnects.

Android `WalletScreen.is_connected` binds the Rust-computed `WalletStatus.is_connected` bool directly, rather than deriving it from the tone discriminant (which wrongly showed a Disconnect button for errored wallets). Android's imperative `openTimeline` call is removed from `signInNsec`/`createAccount`/`switchAccount`; the View layer already drives it via `TimelineScreen.LaunchedEffect` (matching iOS).

No temporary hacks are permitted; 'for now' workarounds, stubs that stay, and TODO comments left in production code are categorically forbidden.

<!-- citations: [^11850-114] [^f2605-2] [^cd2b6-1] [^019ed-108] [^019ed-118] [^019ed-126] [^019ed-150] -->
