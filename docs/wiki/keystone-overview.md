---
title: Keystone Overview
slug: keystone-overview
topic: codebase-patterns
summary: The three keystones are K1 (signer-session port covering sign|nip44_encrypt|nip44_decrypt with mailbox completions), K2 (instance-scoped registration replacing
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-13
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
---

# Keystone Overview

## Keystones

The three keystones are K1 (signer-session port covering sign|nip44_encrypt|nip44_decrypt with mailbox completions), K2 (instance-scoped registration replacing OnceLock globals and type-only ActionModule), and K3 (coverage ledger wiring the dormant WatermarkRow as the sole source of since floors). <!-- [^2e544-61] -->


Of 11 needs-decision issues, 10 were determined by documented product direction; only #1281 required a genuine owner product-contract choice. <!-- [^02745-131] -->
## 30-Day Call

The 30-day call is: K1 through gift-unwrap, K2 through the global-hook slots, and the sync-soundness pair (un-floored NEG-OPEN + slot-lifetime cache-serve marker), plus the durable money boundary dispatched in parallel. <!-- [^2e544-62] -->
