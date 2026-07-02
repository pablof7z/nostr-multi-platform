---
type: noun-entry
slug: status-fact
name: "status fact"
origin: extracted
source_refs:
  - transcript:985-986
  - transcript:1011-1013
---

# status fact

A Rust-owned local publish intent / status object tracking the lifecycle of a write: pending → signed → stored → planned → sent → failed/exhausted. It makes dispatch ≠ success — the app gets honest, offline-first write state instead of fire-and-forget.
