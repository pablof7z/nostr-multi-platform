---
title: "The Read Door: Typed Read Sessions and API Surface"
slug: read-door
topic: read-door
summary: The read door follows the typed sessions architecture established in ADR-0070
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-29
updated: 2026-06-29
verified: 2026-06-29
compiled-from: conversation
sources:
  - session:898a41b5-68e0-4b0f-b16c-c6072454bd6a
  - session:3c942260-311d-4e00-8bcc-204045ea87b3
---

# The Read Door: Typed Read Sessions and API Surface

## Read Door (Typed Sessions)

The read door follows the typed sessions architecture established in ADR-0070. It requires `open_interest`, `ObservedProjection`, and `ReducedSource` to be crate-private, with raw feed/interest C ABI symbols deleted. This makes typed read sessions the sole app-facing read API. The app-visible read model is one typed session descriptor and handle owning the complete lifecycle: acquisition → route policy → bounded replay → live sink → admission → typed output → wake sources → teardown. `open_interest` is demoted to acquisition-only substrate; `ObservedProjection` and `ReducedSource` are private machinery, not app vocabulary. Empty dynamic source sets in read sessions fail closed and never silently become wildcard relay demand. Shells must not hand-author filters or projections.

The read door is not yet 100% complete. The product-read cutover (#2399) and the search read doors (#2418/#2427) are still open.

<!-- citations: [^898a4-84131] [^898a4-83357] [^3c942-35224] -->
