---
title: Zap Protocol
slug: zap-protocol
topic: zap-scope
summary: The fetched bolt11 amount must be validated against the requested amount before auto-pay using the in-crate amount_msats parser
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-14
updated: 2026-06-14
verified: 2026-06-14
compiled-from: conversation
sources:
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
---

# Zap Protocol

## Payment Validation

The fetched bolt11 amount must be validated against the requested amount before auto-pay using the in-crate amount_msats parser. (Previously: The fetched bolt11 amount is never validated against the requested amount before auto-pay; the validating parser already exists in-crate but is never called on the invoice being paid.) The bolt11 amount-validation fix (fail-closed on mismatched and amountless invoices) landed as PR #1189.

Unknown zap payment outcomes must be transitioned to an Unknown state (not Failed), surfaced as 'payment pending confirmation', with a durable tri-state record persisted before the irreversible 23194 send and NwcMethod::LookupInvoice reconciliation on reconnect/restart/TTL. No saga coordinator is built for zaps because compensation is impossible in Lightning; one durable boundary plus tri-state plus lookup reconciliation suffices. (Previously: Unknown zap payment outcomes must be reported as Unknown-pending-reconciliation, never as Failed, and a durable tri-state payment record must be persisted before the irreversible 23194 send. No funds-in-flight state survives process death, creating a double-pay vector.) The durable money boundary fix (tri-state payment record persisted before the 23194 frame, TTL sweep and disconnect drain transition to Unknown never Failed, NwcMethod::LookupInvoice reconciliation on reconnect/restart) landed as PR #1211.

The proof-of-payment (preimage) must be kept in the durable payment record; the current code discards it and the comment claiming otherwise is false. (The preimage-retention fix also landed as PR #1211.)

<!-- citations: [^2e544-383] [^2e544-334] [^2e544-335] [^2e544-336] [^2e544-337] [^2e544-360] [^2e544-382] [^2e544-404] [^2e544-422] [^2e544-443] [^2e544-476] -->
## Connection Management

The WalletConnection pending diagnostic map (the heartbeat get_info probes) must be drained on response, not insert-only, to prevent unbounded in-session growth. <!-- [^2e544-338] -->

## Encryption

NWC request encryption uses NIP-04 (not NIP-44 as previously claimed in the journey); only decrypt falls back to NIP-44. <!-- [^2e544-339] -->
