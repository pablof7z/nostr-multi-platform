---
type: noun-entry
slug: walletbackend
name: "WalletBackend"
origin: extracted
source_refs:
  - transcript:312-312
---

# WalletBackend

A Rust trait representing the wallet backend seam. NWC (NIP-47) is the Lightning/BOLT-11 backend; Cashu (NIP-60) is the ecash backend. A composition layer selects which backend handles a given action.
