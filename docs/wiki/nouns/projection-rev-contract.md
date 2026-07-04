---
type: noun-entry
slug: projection-rev-contract
name: "projection_rev contract"
origin: extracted
source_refs:
  - transcript:4056-4057
---

# projection_rev contract

Monotonic u64 per key that advances when content changes; Rung 3 omits Unchanged. Built-in keys derive rev from SourceVersions write-chokepoint counters; app-owned keys (absent from the kernel's builtin rev manifest) derive a content-driven rev via fingerprint comparison so the rev advances iff content changed.
