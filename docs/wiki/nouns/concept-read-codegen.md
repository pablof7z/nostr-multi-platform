---
type: noun-entry
slug: concept-read-codegen
name: "concept-read codegen"
origin: extracted
source_refs:
  - transcript:1790-1790
  - transcript:1804-1804
---

# concept-read codegen

Each concept crate ships the FFI-shaped half of its own doorway (round-trippable handle parts, scalar/flat inputs, typed errors), and nmp-codegen generates each app's #[uniffi::export] facade slice plus Swift/Kotlin wrappers from a per-app JSON registry listing only the concepts that app composes. No central crate gains a dependency on any concept crate — codegen emits text naming concept symbols, it never links them.
