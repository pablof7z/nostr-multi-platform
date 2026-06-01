---
title: Replaceable Event Freshness Cache & Lazy Re-Fetch
slug: replaceable-event-freshness-cache
summary: Replaceable events use a per-identity timestamp (`check_again_after`) stored in an LMDB sub-db `replaceable_freshness` and hot-cached in memory to indicate when
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-01
updated: 2026-06-01
verified: 2026-06-01
compiled-from: conversation
sources:
  - session:37035e20-9c1c-418f-88f1-68e464b51ec7
---

# Replaceable Event Freshness Cache & Lazy Re-Fetch

## Replaceable Event Freshness Cache

Replaceable events use a per-identity timestamp (`check_again_after`) stored in an LMDB sub-db `replaceable_freshness` and hot-cached in memory to indicate when the event should be re-fetched. Lazy TTL re-fetch provides automatic, transparent freshness checking: when `claim_replaceable` is called and `now > check_again_after`, the kernel enqueues a background re-verification REQ, serving the cached value immediately. Default TTLs for lazy re-fetch are 1 hour for kind:0 profiles and 6 hours for all other replaceable kinds. The `check_again_after` timestamp is updated on REQ dispatch (in-flight guard), EOSE receipt, and event ingestion. An unchanged duplicate ingestion resets the clock rather than causing a re-fetch loop. Replaceable freshness classification (`is_replaceable` / `is_addressable`) uses the `nostr` crate's kind ranges, making any current or future replaceable kind trackable automatically without code changes.

<!-- citations: [^37035-4] [^37035-8] -->
## Force-Refresh Mechanism

The force-refresh capability for replaceable events is implemented by adding a `force: bool` parameter to both `nmp_app_claim_profile` and `nmp_app_claim_event`, rather than adding a new C-ABI function. When `force=true` is passed to `claim_profile` or `claim_event`, the kernel zeros out `check_again_after` before the TTL gate check, causing an immediate re-fetch enqueue. The `nmp_app_refresh_replaceable` C-ABI function and the `RefreshReplaceable` actor command are removed entirely, with their capability absorbed by `ClaimProfile` and `ClaimEvent`. `claim_event` with an `naddr` URI encodes (kind, pubkey, d-tag), generically covering force-refresh for all parameterized replaceable kinds (e.g., kind:30023, kind:10002).

<!-- citations: [^37035-5] [^37035-9] -->
