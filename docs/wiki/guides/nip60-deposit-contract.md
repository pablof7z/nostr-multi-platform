---
title: NIP-60 Deposit Contract
slug: nip60-deposit-contract
topic: marmot
summary: The nmp-nip60 complete_deposit contract returning QuoteNotPaid for unpaid quotes is the correct library boundary and must not be changed; the kernel or caller i
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-18
updated: 2026-06-19
verified: 2026-06-18
compiled-from: conversation
sources:
  - session:019edc02-4996-7cc0-8470-8fd907d283e4
  - session:019edc13-83b1-7143-8631-b0e695ea4afd
  - session:11850f79-923f-4a2a-a921-a4b9bec47c6c
---

# NIP-60 Deposit Contract

## Library Boundary: Quote Payment and Retry Responsibility

The nmp-nip60 complete_deposit contract returning QuoteNotPaid for unpaid quotes is the correct library boundary and must not be changed; the kernel or caller is responsible for any retry scheduling. <!-- [^019ed-19] -->

nmp-nip60 is parked and caller-less; adding a callback or observer API such as complete_deposit_when_paid inside it would be over-engineering and risks putting protocol polling policy into a parked library. <!-- [^019ed-20] -->

A future kernel-facing API could offer a wall-clock-gated observer such as DepositQuoteObservedPaid driven by the kernel scheduler for NIP-60 reactivation. <!-- [^019ed-21] -->

## Workspace Status and Actionability

The `nmp-wallet-poc` sleep-loop finding is stale (crate deleted by PR #1509). The `nmp-nip60` `complete_deposit` caller-poll is non-actionable — nmp-nip60 is parked/excluded from workspace, Cashu NUT-04/NUT-23 mint-quote is request/response with no push primitive, and a doc-comment is added citing the protocol constraint. nmp-nip60 is a parked crate excluded from the workspace with zero callers; its Cashu NUT-04/NUT-23 mint-quote is request/response with no push primitive, so the library correctly does a single status read and returns QuoteNotPaid.

P6 (nmp-nip60/src/relay.rs as a self-contained second framework inside a Layer-4 crate) was not assigned as a work lane in this campaign.

<!-- citations: [^11850-76] [^019ed-22] [^11850-36] [^11850-75] [^11850-100] [^11850-238] -->
## Kind Constants

NIP-60 kind constants changed from local u16 declarations to pub use re-exports of nmp-kinds u32 constants, with `as u16` casts at EventBuilder call sites. <!-- [^019ed-73] -->
