---
type: noun-entry
slug: projection-rev
name: "projection_rev"
origin: extracted
source_refs:
  - transcript:3994-3999
---

# projection_rev

A per-key monotonic u64 that advances when content changes (ADR-0070 Rung 2 wire contract). App-owned keys absent from the kernel's builtin rev manifest must derive a content-driven rev so rev-aware host caches don't skip changed frames.
