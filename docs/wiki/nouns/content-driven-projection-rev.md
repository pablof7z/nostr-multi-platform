---
type: noun-entry
slug: content-driven-projection-rev
name: "content-driven projection_rev"
origin: extracted
source_refs:
  - transcript:3998-3999
---

# content-driven projection_rev

For app-owned (non-manifest) keys, a per-key counter that increments when the payload fingerprint changes — so the rev advances iff content changed. Cleared rows keep rev 0; built-in keys derive rev from source-version write-chokepoint counters instead.
