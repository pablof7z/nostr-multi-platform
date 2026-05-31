---
title: nmp-kinds — Canonical Layer-0 Kind Constants Crate
slug: nmp-kinds-layer0-canonical-crate
summary: Every NIP kind constant — including `KIND_GIFT_WRAP` — SHALL be defined in a single canonical zero-dependency Layer‑0 crate named `nmp-kinds`
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-30
updated: 2026-05-26
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
  - session:64f3e239-c4c1-4c32-82de-458516b28418
---

# nmp-kinds — Canonical Layer-0 Kind Constants Crate

## Canonical Kinds Crate

Every NIP kind constant — including `KIND_GIFT_WRAP` — SHALL be defined in a single canonical zero-dependency Layer‑0 crate named `nmp-kinds`. All other crates that expose kind constants (`nmp_core::kinds`, `nmp_nip59::kinds`, etc.) MUST re‑export from `nmp-kinds` rather than defining their own copies. This ensures a single source of truth available to every component. Kind 10050 is excluded from the default bootstrap self-kinds because NIP-17 runtime owns it.

<!-- citations: [^4edd4-229] [^64f3e-5] -->
## See Also

