---
type: noun-entry
slug: read-lifecycle-the-skeleton
name: "read lifecycle (the skeleton)"
origin: extracted
source_refs:
  - transcript:681-685
  - transcript:786-796
---

# read lifecycle (the skeleton)

The common spine every concept read shares: register demand with the kernel, replay cache/store before going live, subscribe live, admit/filter arriving events, fold them into a bounded model, emit typed output with coalescing, and on close tear everything down symmetrically — withdraw exact demand, in reverse order, plus tombstone on account switch. One implementation behind many concept-shaped doors.
