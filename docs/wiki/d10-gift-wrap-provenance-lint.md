---
title: D10 Lint — Gift-Wrapped Events Must Route Only Through Recipient DM Relays
slug: d10-gift-wrap-provenance-lint
summary: "D10 lint enforces provenance: gift-wrapped events must route only through `recipient_dm_relays` or `PublishTarget::Explicit`, preventing leaks to the general ou"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-05-21
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:1c093fa5-0f0e-4dee-bf38-99781e763f13
---

# D10 Lint — Gift-Wrapped Events Must Route Only Through Recipient DM Relays

## Provenance Enforcement

D10 lint enforces provenance: gift-wrapped events must route only through `recipient_dm_relays` or `PublishTarget::Explicit`, preventing leaks to the general outbox. [^1c093-13]

## See Also

