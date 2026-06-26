---
type: noun-entry
slug: generatedactionbuilders
name: "GeneratedActionBuilders"
origin: extracted
source_refs:
  - transcript:1070-1079
---

# GeneratedActionBuilders

generated Kotlin byte-builder system in ActionBuilders.kt (ADR-0064 §3) from codegen registry; each function encodes per-crate FlatBuffers payload for one action_namespace, stamps it with namespace and schema_version into DispatchEnvelope
