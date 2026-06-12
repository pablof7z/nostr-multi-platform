---
title: DM/Giftwrap Crypto Optimization
slug: dm-crypto-optimization
topic: dm-relay-ingest
summary: NMP should check whether DM and giftwrap unwrap paths re-derive ECDH+HKDF per message or reuse a ConversationKey
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

# DM/Giftwrap Crypto Optimization

## ConversationKey Reuse in DM/Giftwrap Paths

A first-class ConversationKey type should be provided for NIP-44 to enable efficient reuse of ECDH+HKDF-Extract across multiple messages. NMP should check whether DM and giftwrap unwrap paths re-derive ECDH+HKDF per message or reuse rust-nostr's ConversationKey. ECDH+HKDF-Extract is the expensive operation, so identifying and enabling key reuse where cryptographically sound is critical for optimization.

<!-- citations: [^954c5-3] [^954c5-12] -->
