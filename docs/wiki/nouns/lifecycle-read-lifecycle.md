---
type: noun-entry
slug: lifecycle-read-lifecycle
name: "lifecycle (read lifecycle)"
origin: extracted
source_refs:
  - transcript:681-685
  - transcript:400-413
---

# lifecycle (read lifecycle)

The common skeleton identical regardless of what is being read: register demand with the kernel, replay cache/store before going live, subscribe live, admit/filter arriving events, fold into a bounded model, emit typed output with coalescing, and on close tear everything down symmetrically. What differs per concept is only the semantics (demand, admission, reducer, output encoder).
