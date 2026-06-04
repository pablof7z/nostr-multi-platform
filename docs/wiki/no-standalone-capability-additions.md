---
title: No Standalone Capability Additions
slug: no-standalone-capability-additions
summary: HttpCapability must not be added without its consumer in the same PR.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-06-03
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:1c093fa5-0f0e-4dee-bf38-99781e763f13
  - session:f1b740a8-d601-4b63-8633-072c83a6de22
---

# No Standalone Capability Additions

## Rule

HttpCapability must not be added without its consumer in the same PR. HTTP transport for Blossom uploads stays in the sibling podcast crate; nmp-core deliberately has no HTTP client.

<!-- citations: [^1c093-31] [^f1b74-24] -->
