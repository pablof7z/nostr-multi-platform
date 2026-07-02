---
type: noun-entry
slug: nmp-ffi-nmp-native-runtime-crate-roles
name: "nmp-ffi / nmp-native-runtime (crate roles)"
origin: extracted
source_refs:
  - transcript:804-804
  - transcript:2394-2394
---

# nmp-ffi / nmp-native-runtime (crate roles)

nmp-ffi is C-ABI glue; nmp-native-runtime owns lifecycle. Under the UniFFI collapse, nmp-native-runtime also owns the FFI-free dispatch core that both C-ABI and UniFFI consume.
