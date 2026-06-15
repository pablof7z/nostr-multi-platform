---
title: Event Acquisition
slug: event-acquisition
topic: event-acquisition
summary: "The event-flow architecture plan (docs/plans/arch-fixes.md) covers Workstreams A (ingest: PR0â3), B (acquisition one-door), C (publish one-door), and F (doctr"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-15
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
  - session:ab8061fc-b277-4ba4-bf55-1532bcb1aa90
  - session:78b50727-bccd-4088-8493-a07624a4fa83
  - session:c9a794f6-6ad7-4ee9-a620-fc342fd495c3
---

# Event Acquisition

## Mechanism

The event-flow architecture plan (docs/plans/arch-fixes.md) covers Workstreams A (ingest: PR0–3), B (acquisition one-door), C (publish one-door), and F (doctrine gates); Workstreams D (signer/capability authority) and E (action/projection lifecycle) are split into a separate sibling plan (docs/plans/arch-authority-lifecycle.md). The plan is delivered as one atomic PR per workstream with no technical debt, no migrations, and no shims. ADR-0057 is the self-contained durable authority for the event-flow ingest decision; plan files are temporal coordination artifacts marked for deletion on merge, not sources of authority, while GitHub Issues #1440/#1442/#1443 carry the durable 'why' and tracking. Workstream D items 4 (BunkerBroker::cancel detach) and 6 (AppHost god-trait split) remain as clear PRs; items 2 and 5 are done via K2/ADR-0050; items 1, 3, and 7 are partial gaps. Workstream B (acquisition one-door) is NOT done: profile-claims and replaceable-reverify still build direct REQs via req_for_relay (not LogicalInterests on the InterestRegistry), and mailbox-discovery probes are permanent per session with no epoch/TTL lifecycle.

PublishBehavior is the single declared policy table for publish classification; raw kind==N literal guards in publish routing are banned and enforced by a source-scan doctrine gate. The publish policy one-door (Workstream C) classifies publish behavior via a typed PublishBehavior enum (ReservedBuilderOnly, PrivateFailClosed, DiscoveryIndexable, PublicRoutable) — the only function permitted to compare a publish kind to a named constant. PrivateFailClosed kinds (gift-wrap 1059, sealed 14) are enforced at the universal dispatch-emit site (dispatch_due) — a private event may emit only to a relay whose relay_reasons includes Explicit; all other selected relays are refused, the frame dropped, and the relay settled FailedAfterRetries. When the publish gate refuses a resumed private row, the publish row is terminal-finalized so it does not linger pending in the durable store and re-refuse on restart. The publish-policy reintroduction gate scans the full routing surface (action.rs, publish.rs, publish_cmd.rs, publish_engine.rs, engine/helpers.rs, engine/dispatch.rs) for any kind==/!= int or kind==/!= KIND_* guard outside policy.rs, including evasion shapes (match arms, matches!, .contains).

There is a single event-acquisition mechanism: serving from the local store is its first half, the planner's wire REQ is its refinement half, running through the same seam with no domain-specific stages. NIP-17 DM and NIP-57 zap subsystems use registered LogicalInterests exclusively with zero bespoke REQ construction.

The NMP transport pool dials arbitrary relay URLs on demand via send_outbound → ensure_relay_worker_with_kind, spawning a worker for any URL including third-party author relays, with RelayConnectionKind::Temporary and 60s idle teardown, requiring zero new transport capability for connecting to third-party author relays.

Greedy weighted max-coverage set-cover relay minimization exists in nmp-planner selection.rs and runs on every recompile before wire emission, reducing relay sets bounded by select_max_connections and select_max_per_user.

The single generic chokepoint for all authors-filtered interests is SubscriptionLifecycle::recompile_and_diff_with_lookup in subs/recompile.rs, which compiles every LogicalInterest on each drain_tick. This chokepoint provides three intrinsic behaviors for any authors-filtered interest: (a) immediate fallback REQ to app_relays for uncached authors, (b) batched kind:10002 fetch to indexers for uncached/unprobed authors (the D3 probe), and (c) progressive re-route to the author's own relays when their kind:10002 arrives via Nip65Arrived trigger. The follows feed and profile claim must not call any relay-list helper; they inherit 10002 acquisition and per-author routing as an intrinsic property of this underlying subscription/routing infrastructure that processes any authors-filtered interest.

`recompile_and_diff` must use plan-input memoization at the compile seam: hash the full input tuple (`iter_active()` shapes + mailbox-cache generation + `dead_relays` + `app_relays` + bootstrap sets + score-map generation + watermark store generation) and return an empty diff without invoking the compiler when the fingerprint is unchanged from the last compile. The memoization key must include the watermark store generation; omitting it will serve a stale `since` value and cause silent under-fetch. Plan-input memoization is the high-leverage architectural fix that neutralizes every spurious trigger at one chokepoint. Inbox-level dedup is the wrong layer for solving the trigger storm because `TriggerInbox` is a dumb FIFO that only coalesces multiple triggers within one tick, not across ticks; a single trigger per tick still forces a full compile. Gating `push_interest_and_serve` on shape change is a correct cleanup of the lone unconditional producer and should ship alongside plan-input memoization.

<!-- citations: [^da6b1-82] [^da6b1-15] [^2e544-8] [^2e544-9] [^2e544-10] [^4277-4280] [^3720-3721] [^2e544-11] [^2e544-12] [^2e544-13] [^2e544-14] [^2e544-15] [^2e544-16] [^2e544-17] [^2e544-18] [^2e544-19] [^1278-2238-2267] [^1278-2249-2255] [^1278-2253-2254] [^1280-2386-2436] [^1280-2396-2436] [^02745-7] [^2e544-55] [^02745-55] [^02745-99] [^2e544-349] [^2e544-369] [^2e544-463] [^2e544-480] [^ab806-28] [^ab806-37] [^78b50-8] [^c9a79-24] [^78b50-21] [^c9a79-28] [^ab806-203] [^78b50-126] [^78b50-138] [^78b50-151] [^78b50-160] [^78b50-170] [^78b50-180] [^78b50-198] [^78b50-203] [^78b50-214] [^78b50-238] -->
