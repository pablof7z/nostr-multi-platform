---
title: "Nutzap Verification Gate: DLEQ, P2PK, and Fail-Closed Checks"
slug: nutzap-verification-gate
topic: wallet-architecture
summary: The redeem verification gate checks mint-trust â pubkey â P2PK-lock â privkey â DLEQ â fold/publish in that order
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-07-04
updated: 2026-07-04
verified: 2026-07-04
compiled-from: conversation
sources:
  - session:91a86fdf-624c-446e-9b38-0fb02085121f
---

# Nutzap Verification Gate: DLEQ, P2PK, and Fail-Closed Checks

## Redeem Verification Gate Order

The redeem verification gate checks mint-trust → pubkey → P2PK-lock → privkey → DLEQ → fold/publish in that order. The privkey check runs before the DLEQ check, and both fail closed independently. <!-- [^91a86-7e72d] -->

The fail() telemetry calls in the verification gate record WalletConsumedInput (event_id + nutzap-derived mint/amount) before transitioning to Failed. <!-- [^91a86-70e2a] -->

## DLEQ Fail-Closed Behavior

NMP's nutzap DLEQ verification fails closed on missing DLEQ proof and missing blinding factor, returning hard errors instead of continuing. <!-- [^91a86-17fd0] -->

Nutzap receive-path DLEQ verification binds to the proof's claimed keyset id, returning an error if `proof.id != keyset.id` before the mint pubkey lookup. <!-- [^91a86-8c5b7] -->

## Build Configuration & Feature Gating

The `verify_nutzap_dleq_against_keyset` function is gated with `#[cfg(any(feature = "native", test))]` so it does not produce dead-code warnings on wasm32 builds. The DLEQ challenge transcript helper (`dleq_challenge`) is test-only (`#[cfg(test)] pub(crate)`), with `sha2` in dev-dependencies only and no production sha2 dependency. <!-- [^91a86-08b81] -->
