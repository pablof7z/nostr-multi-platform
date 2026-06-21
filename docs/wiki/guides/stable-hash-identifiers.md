---
title: Stable Hash Identifiers
slug: stable-hash-identifiers
topic: ffi-runtime
summary: DefaultHasher is used in stable identifiers (contacts.rs, sub_key.rs, profile/thread request IDs) and is non-deterministic across processes; it must be replaced
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-22
updated: 2026-05-26
verified: 2026-05-22
compiled-from: conversation
sources:
  - session:95d02563-5473-4d84-96e1-cd342e1b04d1
  - session:6e4c3a3a-9515-4437-a4bf-b4228a10ae57
---

# Stable Hash Identifiers

## Stable Hash Identifiers

DefaultHasher is used in stable identifiers (contacts.rs, sub_key.rs, profile/thread request IDs) and is non-deterministic across processes; it must be replaced with a stable hash. The interest stable identity now includes the kinds hash, so changing what kinds are subscribed to cleanly tears down old interests and opens new ones, forcing a one-time CLOSE+REQ on upgrade.

<!-- citations: [^95d02-15] [^6e4c3-2] -->
