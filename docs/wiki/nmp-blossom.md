---
title: NMP Blossom Crate & Upload Pipeline
slug: nmp-blossom
summary: NMP will create an `nmp-blossom` protocol crate that owns the Build→Sign→Upload pipeline via the existing `ProtocolCommand` seam, allowing apps to dispatch `nmp
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-03
updated: 2026-06-04
verified: 2026-06-03
compiled-from: conversation
sources:
  - session:83b5dae5-d3f4-4f4d-b12f-9d04d17c9139
---

# NMP Blossom Crate & Upload Pipeline

## nmp-blossom Protocol Crate

NMP will create an `nmp-blossom` protocol crate that owns the Build→Sign→Upload pipeline via the existing `ProtocolCommand` seam, allowing apps to dispatch `nmp.blossom.upload` and receive a blob descriptor through a projection without handling keys, HTTP, or continuation-scanning. The crate will follow the `nmp-nip57` precedent: the ActionModule emits a boxed ProtocolCommand whose worker thread handles signing and HTTP, keeping nmp-core HTTP-free and noun-free. The v1 scope is upload-only (BUD-02 PUT) with a namespace built to extend. NMP owns key custody and signing (via signer registry and sign-for-return); the app owns its Blossom HTTP transport and uses NMP for signing, not for HTTP operations. The app supplies the Blob server list for Blossom uploads in v1. Binary blob payloads are passed to the Blossom worker by filesystem path rather than base64-encoded in a JSON action; the worker streams and hashes off-thread, nmp-core touches zero bytes. The crate's auth module builds kind:24242 events with a 5-minute TTL and base64 encodes them for the Authorization header.

<!-- citations: [^83b5d-15] [^83b5d-20] [^83b5d-27] -->
## See Also

The `nmp-blossom` crate is included in the NMP release manifest as a public crate.

<!-- citations: [^83b5d-28] -->
