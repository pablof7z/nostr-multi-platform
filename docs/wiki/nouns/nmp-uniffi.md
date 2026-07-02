---
type: noun-entry
slug: nmp-uniffi
name: "nmp-uniffi"
origin: extracted
source_refs:
  - transcript:900-902
---

# nmp-uniffi

Retired crate name. `crates/nmp-uniffi` was deleted in #2763 after the
reference framework facade had zero real consumers. Native apps now own their
own UniFFI facade crate over `nmp-native-runtime` and `nmp-uniffi-support`.
