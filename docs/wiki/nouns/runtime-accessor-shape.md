---
type: noun-entry
slug: runtime-accessor-shape
name: "runtime_accessor_shape"
origin: extracted
source_refs:
  - transcript:469-472
---

# runtime_accessor_shape

A concept-reads codegen registry field on FacadeRow with values 'ref' (default, emits self.<accessor>()) or 'closure' (emits self.<accessor>(|app| <concept_fn>(app, ...))); closure mode enables Android's guarded AppHandle::with_app accessor to participate in concept-read codegen.
