---
title: Cross-Mint Nutzap Transfer Saga
slug: cross-mint-transfer
topic: wallet-architecture
summary: "NMP implements cross-mint nutzap funding via Lightning: when no recipient-accepted mint has balance, it gets a mint-quote (bolt11) from the recipient's target m"
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
  - session:d8bc6df1-32a3-48e1-8db6-3dbff7c4c0e5
---

# Cross-Mint Nutzap Transfer Saga

## Cross-Mint Nutzap Transfer

NMP implements cross-mint nutzap funding via Lightning: when no recipient-accepted mint has balance, it gets a mint-quote (bolt11) from the recipient's target mint and melts proofs from a source mint that has balance to pay it. This melt→mint path is atomic via journaling — unlike NDK's implementation, which loses the payment if melt succeeds but mint fails, NMP journals the melt and reconciles the paid quote on restart, preventing double-spends and fund loss. The `nutzap.send` flow is self-sufficient: the app says 'pay X' and the wallet decides intra-mint vs cross-mint internally, sizing in fee headroom as needed — the app does not need to compose cross-mint transfer or set_mints. <!-- [^91a86-23772] -->

When the recipient's kind:10019 relay list is not yet available, `nutzap.send` registers a one-shot event-driven continuation that re-drives the send when the info arrives, instead of failing immediately. <!-- [^91a86-ac704] -->

## Melt Primitives

The MintClient exposes NUT-05 melt primitives: `create_melt_quote(bolt11)`, `get_melt_quote_status(quote_id)`, and `melt(quote_id, fee_reserve, inputs, keyset)`. <!-- [^91a86-f75e6] -->

## Journaling & Crash Recovery

The cross-mint transfer saga journals consumed source inputs and `melt_quote_id` before the melt HTTP call, so a non-PAID or transport-failed melt goes Unknown and reconciles only via `get_melt_quote_status` on cold-restart. The target `mint_tokens` is write-if-absent fenced so a resume never double-mints. The cross-mint melt stays terminal — PAID advances; anything else goes to Unknown+resume and is never retried against a fresh source, respecting the do-not-advance-on-Unknown rule.

<!-- citations: [^91a86-6b403] [^d8bc6-16be1] -->
## Fee Sizing

The auto-fallback cross-mint transfer (triggered when `on_settled.is_some()`) sizes `funded_amount = amount_sats + fee_headroom`, fetching the target mint's keyset fee rate and falling back to a 2-sat minimum headroom if the fetch fails. The standalone `cross_mint_transfer` action, by contrast, requests exactly `amount_sats` without added headroom. The send worker derives its swap fee from conservation (`selected_total - new_total`), working for both live and WAL-resume paths. <!-- [^91a86-1d2e6] -->

## Source Mint Selection

Cross-mint source selection walks an ordered candidate list (largest balance first, settleable only) rather than making one blind pick. It falls through to the next candidate on any pre-melt failure that moves no funds. Known valueless test mints (the testnut.cashu.space host family) are excluded from source candidacy via `is_known_valueless_mint` — the WAL proves testnut's melt quote succeeds and only fails PENDING after the irreversible melt, so selection is the only safe guard against a fake mint entering a real Lightning melt. Post-melt Unpaid-to-restore-to-advance is not implemented because the WAL case is PENDING (not definite Unpaid) and adding a reversal on the irreversible leg is unnecessary risk.

<!-- citations: [^91a86-61e98] [^d8bc6-7d5d5] -->
## Retry & Operation Identity

The cross-mint auto-fallback's retried send derives its journal operation id from the mint-issued `target_quote_id` instead of the caller's correlation_id, preventing DuplicateOperation drops on retry. <!-- [^91a86-cad82] -->

## History Display

CrossMintTransfer operations do not surface a history row of their own — only the resulting SendNutzap row carries the mint/fee display fields. <!-- [^91a86-39e0d] -->
