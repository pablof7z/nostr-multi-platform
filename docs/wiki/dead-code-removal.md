---
title: Dead Code and App Removals
slug: dead-code-removal
topic: code-cleanup
summary: The chirp-repl app is deleted entirely — it is not used.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-03
updated: 2026-06-11
verified: 2026-06-03
compiled-from: conversation
sources:
  - session:7f143c67-6e46-424a-90a8-5bf844947fee
  - session:d8869714-0ee5-4fe3-94db-1efd068b1c58
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
---

# Dead Code and App Removals

## Removed Applications

The chirp-repl app is deleted entirely — it is not used. <!-- [^7f143-1] -->

## Removed Enum Variants

The `AccountError` variants `SignerMismatch` and `SignerError` are deleted; only `NotFound` remains. The `value_from_transport_payload` function and the entire `Value`-codec family are deleted from nmp-core; chirp-tui and chirp-desktop decode snapshots exclusively from typed channels (FlatBuffers sidecars + Tier-3 envelope).

<!-- citations: [^d8869-1] [^da6b1-60] -->
