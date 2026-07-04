---
type: noun-entry
slug: read-lifecycle-engine
name: "read lifecycle engine"
origin: extracted
source_refs:
  - transcript:71-83
  - transcript:681-685
  - transcript:828-841
---

# read lifecycle engine

One shared internal implementation of the read skeleton (demand registration, replay-before-live, live activation, admission delivery, typed output registration, coalesced emission, handle registry, symmetric close, account/source-switch tombstoning). Concept owners supply only semantic parameters (spec, demand compiler, admission predicate, reducer, output encoder, teardown policy); they never implement lifecycle code. The engine is private plumbing; public APIs stay concept-shaped.
