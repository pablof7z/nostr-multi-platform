---
title: Outbox Resolver
slug: outbox-resolver
topic: relay-routing
summary: The OutboxResolver must apply the blocked-relay filter on publish, not just subscribe, to prevent publishing to user-blocked relays
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-13
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
---

# Outbox Resolver

## Blocked-Relay Filter Bypass

The OutboxResolver must apply the blocked-relay filter on publish, not just subscribe, to prevent publishing to user-blocked relays. The publish path applies this filter across all lanes including explicit targets. The publish-engine holds an Arc<dyn BlockedRelayLookup> resolved per publish so that publish and subscribe share one blocked-relay cache.

<!-- citations: [^02745-13] [^02745-40] [^02745-61] -->
## URL Canonicalization

Relay URL canonicalization must be shared between kind:10002 (ingest) and kind:10006 (blocked relays) so that casing and trailing-slash mismatches resolve to the same key.

<!-- citations: [^02745-14] [^02745-41] [^02745-62] -->
## Lane 6 Indexer-Relay Discovery Kinds

Lane 6 must record a per-relay discovery-kind scope so that mixed interests (e.g., [1, 3]) only expose kind:3 to indexer relays while all-discovery interests leave no override, rather than broadcasting all-interest kinds to indexer relays.

<!-- citations: [^02745-42] [^02745-63] -->
## Greedy Merge Deterministic Sort

Greedy merge must use a total canonical sort key (not just the spec tuple) to produce deterministic REQ output regardless of input order. <!-- [^02745-43] -->

## Wildcard-Kinds Merge Refusal

Wildcard-kinds × concrete-kinds merges must be refused (mirroring Rule 9) to prevent an all-kinds privacy/bandwidth leak. <!-- [^02745-44] -->

## Auth-Relay Publish Parking

Publishing to AUTH-required relays must park the event until Authenticated instead of failing, as implemented in PR #1192. <!-- [^2e544-65] -->

## Score Map Ingest from Mainline

The score map must be fed from mainline ingest—recording Hit on every EVENT attributed to (author, relay_url)—rather than only from claims. This activates the dormant W4 warm filter and triggers the record_failure decrement path. <!-- [^2e544-66] -->
