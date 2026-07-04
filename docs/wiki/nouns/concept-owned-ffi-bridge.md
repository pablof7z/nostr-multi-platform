---
type: noun-entry
slug: concept-owned-ffi-bridge
name: "concept-owned FFI bridge"
origin: extracted
source_refs:
  - transcript:1790-1790
---

# concept-owned FFI bridge

Each concept crate ships the FFI-shaped half of its own doorway (round-trippable handle parts, scalar/flat inputs, typed errors), and nmp-codegen generates each app's #[uniffi::export] facade slice plus Swift/Kotlin wrappers from a per-app registry file listing only the concepts that app composes. No central crate gains a concept dependency.
