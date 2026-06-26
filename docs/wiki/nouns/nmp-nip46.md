---
type: noun-entry
slug: nmp-nip46
name: "nmp-nip46"
origin: extracted
source_refs:
  - transcript:3666-3672
  - transcript:4332-4340
---

# nmp-nip46

Transport-agnostic NIP-46 protocol core: a pure event-reducer state machine with no thread spawning or socket opening, wasm-safe and always-compiled, that handles bunker and nostrconnect handshakes by emitting effects (Subscribe/SendFrame/SignerReady/Error)
