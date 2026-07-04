---
type: noun-entry
slug: readhost-seam
name: "ReadHost seam"
origin: extracted
source_refs:
  - transcript:1034-1034
  - transcript:1115-1117
---

# ReadHost seam

A small host/context seam that runtimes implement (NmpApp implements it once, generically) and concept crates consume, so the dependency arrow runs concept-crate → engine ← runtime, never concept-crate → runtime.
