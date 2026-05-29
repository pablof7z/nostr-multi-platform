---
title: Relay Reconnect Backoff — Healthy-Session Reset and Rate-Limit Floor
slug: relay-reconnect-backoff
summary: Relay reconnect uses 3s→300s exponential backoff; resets to 3s after a healthy 5-minute session; uses a 60s floor for rate-limited CLOSED frames.
tags:
  - relay
  - network
  - backoff
  - reconnect
  - v58
  - v92
volatility: cold
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
---

# Relay Reconnect Backoff — Healthy-Session Reset and Rate-Limit Floor

> Relay reconnect uses 3s→300s exponential backoff; resets to 3s after a healthy 5-minute session; uses a 60s floor for rate-limited CLOSED frames.

## How Reconnect Backoff Works

The relay reconnect worker (`crates/nmp-network/src/relay_worker/mod.rs`) uses exponential backoff starting at `RELAY_RECONNECT_DELAY_INITIAL` (3 seconds), doubling on each reconnect, capped at 300 seconds. [^42908-26]

## V-92: Backoff Resets After Long Healthy Session

A relay that was connected for 5+ minutes (defined by `RELAY_BACKOFF_RESET_AFTER_SECS = Duration::from_secs(300)`) is considered healthy. On disconnect, its backoff resets to `RELAY_RECONNECT_DELAY_INITIAL` (3s) rather than inheriting the accumulated exponential value. Rapid reconnect-disconnect cycles still back off progressively.

Implemented in commit 5da5942c. `connected_at: Instant::now()` is recorded when the socket successfully opens and checked on disconnect. [^42908-27]

## V-58: Rate-Limited CLOSED Frames Set Backoff Hint

A NIP-01 `CLOSED` frame closes a *subscription*, not the socket. When a relay issues `CLOSED ["rate-limited: ..."]`, the kernel enqueues a `BackoffHint::RateLimited` for that relay URL. The actor drains these hints on each `Frame` dispatch and calls `pool.set_backoff_hint(handle, BackoffClass::RateLimited)`.

The reconnect worker applies a 60-second floor (`RELAY_RECONNECT_DELAY_RATE_LIMITED`) when a `RateLimited` hint is active, overriding the normal healthy-session reset. This prevents amplifying load on rate-limiting relays.

Key new types: `BackoffClass` enum (`Transient | RateLimited`), `RelayCommand::SetBackoffHint(BackoffClass)`, `apply_reconnect_backoff(hint, &mut backoff, elapsed) -> Duration` (pure, tested). [^42908-28]

## See Also

