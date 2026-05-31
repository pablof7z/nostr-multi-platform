---
title: Discovery Slot Timeout Eviction
slug: discovery-timeout-eviction
summary: Oneshot discovery slots must have a timeout-based eviction mechanism to prevent a misbehaving relay that never sends EOSE from pinning both slots forever and st
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-05-29
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:7b4ae585-801c-441f-811d-5308e1002f08
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# Discovery Slot Timeout Eviction

## Timeout-Based Eviction

Oneshot discovery slots must have a timeout-based eviction mechanism to prevent a misbehaving relay that never sends EOSE from pinning both slots forever and starving all discovery. Eviction cleans every secondary index including relay_index, pinned events provably survive, and write-on-read is safe and bounded to point-reads.

<!-- citations: [^7b4ae-2] [^4edd4-6] -->
## See Also

