---
type: noun-entry
slug: chirp-web
name: "Chirp Web"
origin: extracted
source_refs:
  - transcript:3514-3530
---

# Chirp Web

A thin TypeScript shell with zero protocol logic that loads the wasm module, runs pump/snapshot subscription loops, decodes FlatBuffers snapshots, renders components via @nmp/components-web, and brokers NIP-07/local-key sign requests on the main thread; all Nostr behavior is Rust-owned in nmp-browser-runtime
