---
title: nmp-store LMDB Tombstones — Dead Code Removal and Inline GC Logic
slug: nmp-store-lmdb-tombstones
summary: The nip40_row() function in nmp-store/src/lmdb/tombstones.rs was deleted as dead code since gc.rs inlines its own logic.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-27
updated: 2026-05-27
verified: 2026-05-27
compiled-from: conversation
sources:
  - session:cd2b6122-2b7c-43fc-941b-c51e79ffc691
---

# nmp-store LMDB Tombstones — Dead Code Removal and Inline GC Logic

## Dead-Code Removals

The nip40_row() function in nmp-store/src/lmdb/tombstones.rs was deleted as dead code since gc.rs inlines its own logic. [^cd2b6-5]

## See Also

