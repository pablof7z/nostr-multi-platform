---
title: Check-State Pass (NUT-07) and Money-Safe Reconciliation
slug: check-state-reconciliation
topic: wallet-architecture
summary: "The check-state pass groups held proofs by canonical mint and calls `MintClient::check_state` once per distinct mint"
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

# Check-State Pass (NUT-07) and Money-Safe Reconciliation

## Check-State Pass (NUT-07)

The check-state pass groups held proofs by canonical mint and calls `MintClient::check_state` once per distinct mint. It removes only proofs the mint affirmatively reports as Spent. Pending and Unspent proofs are kept. The `ProofSpendState` enum has no `#[serde(other)]` variant, so an unknown state string is a hard decode error rather than a silent drop. Only an exact `ProofSpendState::Spent` verdict removes a proof during the pass. <!-- [^91a86-1bb23] -->

The pass never drops an unspent proof on any mint HTTP failure — whether a network error, non-2xx response, malformed JSON, response length mismatch, or any other transport/decode failure. It continues past that mint with proofs completely untouched. Only an affirmative Spent verdict ever removes a proof. <!-- [^91a86-2cdd2] -->

The pass spawns a debounced single-flight guard that coalesces a burst of cold-start-replay token events into at most two outstanding passes against the same mint. <!-- [^91a86-0b87c] -->

The pass never holds the state Mutex across the blocking mint HTTP call. It snapshots proofs under a short lock, calls the mint unlocked, then re-locks to fold results and remove spent proofs. <!-- [^91a86-21103] -->

## Recover Action Integration

The `cashu.recover` action defers `RecordActionSuccess` until after the check-state pass completes, so a caller polling balance immediately after recover sees the reconciled unspent-only balance. <!-- [^91a86-57e1a] -->
