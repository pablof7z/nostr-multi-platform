---
type: noun-entry
slug: rebuilt-chirp-web
name: "rebuilt Chirp Web"
origin: extracted
source_refs:
  - transcript:3336-3342
---

# rebuilt Chirp Web

A thin TypeScript shell with zero Nostr protocol logic; loads the wasm module, runs the pump/snapshot loops via @nmp/runtime-web, decodes FlatBuffers snapshots, renders @nmp/components-web, and brokers NIP-07/local-key sign requests on the main thread — all behavior is Rust-owned in nmp-browser-runtime
