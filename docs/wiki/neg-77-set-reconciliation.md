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
updated: 2026-06-15
verified: 2026-06-14
compiled-from: conversation
sources:
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
  - session:ab8061fc-b277-4ba4-bf55-1532bcb1aa90
  - session:78b50727-bccd-4088-8493-a07624a4fa83
---

# NEG-77 Set Reconciliation

## Mainline Ingest Scoring

Relay scores must be fed from mainline ingest (record Hit) rather than only from claims, activating the W4 warm filter and giving record_failure teeth for relay selection. Greedy weighted max-coverage set-cover relay minimization exists in crates/nmp-planner/src/selection.rs and runs on every recompile, reducing relay sets (e.g. 287→~30 for 1048 follows), roughly to select_max_connections with a per-author cap of select_max_per_user. Duplicate relay echoes must remain projection-silent (preserving D4 single-fire) but must still bump the cached relay_count on kind:1/6 events. Relay bookkeeping (JSON parse, per-relay counters, transport provenance, wire-sub diagnostics, claim-expansion lookup) at ingest/mod.rs lines 252–281 is relay-only and must stay outside the shared chokepoint; the shared seam starts at the kind-match at line 296. The SubscriptionLifecycle recompile chokepoint (subs/recompile.rs) already implements intrinsic per-author NIP-65 discovery: uncached authors get an immediate app-relay/indexer fallback REQ plus a batched kind:10002 probe to indexers, and progressive re-route fires via Nip65Arrived when each 10002 lands. The probe re-arm on relay_connected_url must be gated to genuine reconnects (indexer_socket_was_down), not every indexer connect event, to avoid feed subscription churn/oscillation. nprofile TLV relay hints must seed into the profile claim's LogicalInterest.hints so that authors whose kind:10002 is on no indexer can still resolve. Relay admission = valid signature + arrived on a sub we opened (loose, ~today's behavior), not 'matches a registered interest filter' (strict); this preserves current behavior where off-filter events from misbehaving relays are still stored. NIP-17 DMs and NIP-57 zaps use registered LogicalInterests exclusively with zero bespoke REQs; NIP-17 is fail-closed (emits no subscription if kind:10050 isn't cached rather than falling back to bootstrap relays). NIP-60 wallet fetch_nip65_relays hardcodes wss://purplepag.es as an indexer relay, bypassing the kernel's D3 outbox router for kind:10002 discovery; this is a minor, low-impact follow-up issue.

<!-- citations: [^ab806-44] [^ab806-45] [^ab806-46] [^ab806-13] [^2e544-277] [^2e544-351] [^2e544-448] [^ab806-43] [^ab806-133] [^ab806-238] [^78b50-142] [^78b50-221] -->
## NEG-OPEN Liveness and Fallback

NEG-OPEN inherits the watermark-floored `since`, so set reconciliation cannot repair below-floor gaps — the exact failure class NIP-77 exists to fix. NEG-OPEN must carry the shape's un-floored window (not the presence-derived floor) so that reconciliation can repair gaps the heuristic creates. ReplaceOneShot interception has no liveness deadline: a relay that silently ignores NEG-OPEN starves the interest forever with no fallback; the existing wall-clock-gated `on_idle_tick` seam (30s, re-anchored on NEG-MSG) provides the deadline, and a silently accepting relay falls back to a plain REQ. Un-floored NEG-OPEN makes findings 1–3 self-healing for all NEG-eligible shapes without touching watermark code. Etag/Ptag budget-truncated serves refuse the floor via a session-scoped truncation set, so the relay can fill the gap on re-fetch.

Merge Rule 5 refuses coalescing when any shape has a limit, so per-author limit:1 claim interests would produce N separate REQs (a storm); registering claim interests with limit:None avoids this since kind:0 is replaceable and relays return at most one per author. <!-- [^ab806-14] -->

<!-- citations: [^2e544-278] [^2e544-352] [^2e544-374] [^2e544-396] [^2e544-414] [^2e544-449] [^2e544-466] [^2e544-483] -->
## Dead Enum Variants and Stale Docs

Dead enum variants (RelayHealth.last_ping_rtt, PoolEvent::Health, ClosedReason::Permanent/Shutdown) and stale doc comments must either be wired in production or listed in a checked-in dormant-surface inventory with deadlines. <!-- [^2e544-322] -->

## Connection Jitter

NIP-77 set reconciliation must use per-URL full random jitter per attempt (not deterministic hash-based jitter) to avoid synchronized reconnection herds from many clients hitting the same relay. <!-- [^2e544-323] -->

## Pin State and GC Integration

The persisted claims sub-db is the wrong fix for #1090; pin state should be derived from kernel event_claims and threaded into gc_step like now_secs already is, and the claims sub-db should be deleted. <!-- [^2e544-375] -->

## Merged PRs

The AUTH-relay publish-parking fix is merged as PR #1192. <!-- [^2e544-376] -->

## Permanent-Error Lifecycle

Permanent relay errors (401/403) must enter a Denied{until} long-backoff state instead of thread-exit, and mark_relay_dead must have a production caller.

<!-- citations: [^2e544-450] [^2e544-484] -->
