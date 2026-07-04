---
type: noun-entry
slug: readhost
name: "ReadHost"
origin: extracted
source_refs:
  - transcript:1034-1034
  - transcript:1115-1117
---

# ReadHost

A small host/context seam that runtimes implement once and concept crates consume. It is the interface through which a concept-crate doorway (e.g. open_replies) drives the shared engine without depending on any specific runtime crate. The dependency arrow runs concept-crate → engine ← runtime.
