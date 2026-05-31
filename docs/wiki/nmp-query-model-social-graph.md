---
title: NMP Query Model — Social Graph Filter Resolution
slug: nmp-query-model-social-graph
summary: "NMP provides a query model where apps can request 'authors:[current-users-follows]' and NMP handles fetching kind:3, kind:10002, relay connections, and adding t"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-19
updated: 2026-05-26
verified: 2026-05-19
compiled-from: conversation
sources:
  - session:5d180e52-7c43-4a99-bfc4-769eb40dc03f
  - session:1670fcb8-f275-498c-975b-8bd912331ded
  - session:6e4c3a3a-9515-4437-a4bf-b4228a10ae57
---

# NMP Query Model — Social Graph Filter Resolution

## Query Model: Social Graph

NMP provides a query model where apps can request 'authors:[current-users-follows]' and NMP handles fetching kind:3, kind:10002, relay connections, and adding the current user's pubkey to the authors filter automatically. Per-relay filter execution strategy (authors partitioning) is handled by nmp-planner (project_per_relay), not as a routing concern. The kernel must not hardcode social-app-specific knowledge like `kinds: {1, 6}` in `follow_feed_interest()`.

<!-- citations: [^5d180-3] [^1670f-13] [^6e4c3-2] -->
## See Also

