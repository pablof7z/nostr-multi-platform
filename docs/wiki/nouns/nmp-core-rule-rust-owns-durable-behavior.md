---
type: noun-entry
slug: nmp-core-rule-rust-owns-durable-behavior
name: "NMP core rule (Rust owns durable behavior)"
origin: extracted
source_refs:
  - transcript:155-159
  - transcript:256-256
---

# NMP core rule (Rust owns durable behavior)

NMP inherits RMP's core rule: Rust owns durable behavior and each platform renders native UI. Anything a second platform would have to reimplement to stay correct (relay choice, signer choice, tag mutation, publish retry, queue truth, nav meaning) belongs in Rust.
