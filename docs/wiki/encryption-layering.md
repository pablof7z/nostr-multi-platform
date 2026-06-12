---
title: Encryption Layering and Primitives
slug: encryption-layering
topic: nostr-protocol
summary: Encryption primitives (NIP-04/44) should live in the core library as stateless free functions, with signer-based delegation deferred to a later layer.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-12
updated: 2026-06-12
verified: 2026-06-12
compiled-from: conversation
sources:
  - session:954c56b2-d292-4021-8b55-977d3fd8df4d
---

# Encryption Layering and Primitives

## Core Encryption Primitives

Encryption primitives (NIP-04/44) should live in the core library as stateless free functions, with signer-based delegation deferred to a later layer. <!-- [^954c5-13] -->

Dependency choices should stay within the RustCrypto ecosystem (aes, cbc, chacha20, hmac, hkdf, sha2) plus secp256k1, avoiding mixing bitcoin_hashes or ring. <!-- [^954c5-14] -->

secp256k1::SharedSecret::new must not be used for nostr encryption because it SHA-256-hashes the x-coordinate; the raw shared_secret_point call is required. <!-- [^954c5-15] -->

The x-only public key should be reconstructed to a full PublicKey by prepending 0x02 (even-parity lift), which is deterministic and standard across all implementations. <!-- [^954c5-16] -->
