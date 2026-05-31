---
title: Follow-Feed Backfill & Limit
slug: follow-feed-backfill
summary: The follow-feed REQ sent to relays carries a limit parameter that caps backfill at a configurable number of events (kinds 1+6) per followed pubkey per relay
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-27
updated: 2026-05-27
verified: 2026-05-27
compiled-from: conversation
sources:
  - session:ad738906-eb99-4e37-9894-c5637626e6b0
---

# Follow-Feed Backfill & Limit

## Follow Feed Backfill

The follow-feed REQ sent to relays carries a limit parameter that caps backfill at a configurable number of events (kinds 1+6) per followed pubkey per relay. FOLLOW_FEED_LIMIT is set to 1000, causing the follow-feed REQ to request up to 1000 events per followed pubkey on backfill. [^ad738-1]

## See Also

