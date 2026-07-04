---
type: noun-entry
slug: projection-rev-app-owned-keys
name: "projection_rev (app-owned keys)"
origin: extracted
source_refs:
  - transcript:3994-3999
  - transcript:4056-4058
---

# projection_rev (app-owned keys)

ADR-0070 Rung 2 wire contract: rev advances on content change. For app-owned (non-manifest) keys, a per-key content-fingerprint counter that increments when the payload changes — because the kernel has no write-chokepoint visibility into opaque host-registered projection payloads. Built-in (Tier-2) keys derive rev from SourceVersions counters instead.
