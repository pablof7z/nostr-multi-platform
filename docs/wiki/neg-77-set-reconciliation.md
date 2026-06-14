---
title: NEG-77 Set Reconciliation
slug: neg-77-set-reconciliation
topic: event-acquisition
summary: Relay scores must be fed from mainline ingest (record Hit) rather than only from claims, activating the W4 warm filter and giving record_failure teeth for relay
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-14
updated: 2026-06-14
verified: 2026-06-14
compiled-from: conversation
sources:
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
---

# NEG-77 Set Reconciliation

## Mainline Ingest Scoring

Relay scores must be fed from mainline ingest (record Hit) rather than only from claims, activating the W4 warm filter and giving record_failure teeth for relay selection.

<!-- citations: [^2e544-277] [^2e544-351] [^2e544-448] -->
## NEG-OPEN Liveness and Fallback

NEG-OPEN has a liveness deadline via the existing wall-clock-gated `on_idle_tick` seam (30s, re-anchored on NEG-MSG); a silently accepting relay falls back to a plain REQ. Un-floored NEG-OPEN makes findings 1–3 self-healing for all NEG-eligible shapes without touching watermark code. NEG-OPEN carries the un-floored shape window so that set reconciliation can repair below-floor gaps. Etag/Ptag budget-truncated serves refuse the floor via a session-scoped truncation set, so the relay can fill the gap on re-fetch.

<!-- citations: [^2e544-278] [^2e544-352] [^2e544-374] [^2e544-396] [^2e544-414] [^2e544-449] [^2e544-466] -->
## Dead Enum Variants and Stale Docs

Dead enum variants (RelayHealth.last_ping_rtt, PoolEvent::Health, ClosedReason::Permanent/Shutdown) and stale doc comments must either be wired in production or listed in a checked-in dormant-surface inventory with deadlines. <!-- [^2e544-322] -->

## Connection Jitter

NIP-77 set reconciliation must use per-URL full random jitter per attempt (not deterministic hash-based jitter) to avoid synchronized reconnection herds from many clients hitting the same relay. <!-- [^2e544-323] -->

## Pin State and GC Integration

The persisted claims sub-db is the wrong fix for #1090; pin state should be derived from kernel event_claims and threaded into gc_step like now_secs already is, and the claims sub-db should be deleted. <!-- [^2e544-375] -->

## Merged PRs

The AUTH-relay publish-parking fix is merged as PR #1192. <!-- [^2e544-376] -->

## Permanent-Error Lifecycle

A 401/403 relay enters Denied{until} long-backoff instead of thread-exit, and mark_relay_dead gains a production caller. <!-- [^2e544-450] -->
