---
type: noun-entry
slug: read-lifecycle
name: "read lifecycle"
origin: extracted
source_refs:
  - transcript:679-685
---

# read lifecycle

The skeleton shared by every concept read: register demand with the kernel, replay what cache/store already has before going live, subscribe live, admit/filter arriving events, fold them into a bounded model, emit typed output with coalescing, and on close tear everything down symmetrically — withdraw exact demand in reverse order, plus tombstone output on account switch. Identical machinery regardless of whether the thing being read is feed rows or zap totals.
