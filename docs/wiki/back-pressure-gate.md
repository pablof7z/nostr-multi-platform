---
title: Back-Pressure Gate (G-S4)
slug: back-pressure-gate
summary: The actor_queue_depth in kernel/update.rs is hardcoded to 0, meaning the G-S4 reconciler back-pressure gate always passes trivially
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-18
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:7f0f0c78-d1aa-49db-b659-c9cf49827117
  - session:e50d12a1-0a49-4a45-9bb1-251fa0f434b6
---

# Back-Pressure Gate (G-S4)

## Back-Pressure Gate

The actor_queue_depth in kernel/update.rs is hardcoded to 0, meaning the G-S4 reconciler back-pressure gate always passes trivially. The mpsc channel backlog must be wired into actor_queue_depth in kernel/update.rs so that the G-S4 gate actually tests back-pressure. RSS is a misleading gate signal because it is run-dependent, whereas the counting-allocator metric is deterministic and reveals the true retained heap defect. The S2 dispatch-flood scenario revealed a genuine unbounded working-set overrun where RSS grew +45.89 MiB against a 20 MiB budget (2.29× over). The S2 drain tiebreaker data establishes that heap grows with the ~300k total operation count (~127 B per dispatch) instead of the 50-pubkey working-set size, indicating unbounded per-operation accumulation in the actor. The S2 drain measurement proved that the ~38 MiB of peak net heap is retained after the actor drains the backlog (only 0.13% reclaimed), foreclosing Option B (threshold revision) and making Option A (bounded channel fix) mandatory. The bounded channel fix must use a try-send + drop/coalesce policy that stays D6 fire-and-forget (never blocking the FFI call or erroring across the boundary). The S2 bounded-channel fix belongs to the kernel session's scope (crates/nmp-core/**), not the FFI-hardening workstream.

<!-- citations: [^7f0f0-1] [^e50d1-1] -->
## See Also

