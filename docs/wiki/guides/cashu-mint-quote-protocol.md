---
title: Cashu Mint Quote Protocol
slug: cashu-mint-quote-protocol
topic: marmot
summary: The Cashu NUT-04 mint-quote protocol is fundamentally poll-based over HTTP with no push/webhook primitive; the client must GET the quote status until state=PAID
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-18
updated: 2026-06-18
verified: 2026-06-18
compiled-from: conversation
sources:
  - session:019edc02-4996-7cc0-8470-8fd907d283e4
  - session:11850f79-923f-4a2a-a921-a4b9bec47c6c
---

# Cashu Mint Quote Protocol

## Poll-Based Quote Flow

The Cashu NUT-04 mint-quote protocol is fundamentally poll-based over HTTP with no push/webhook primitive; the client must GET the quote status until state=PAID. (Previously: wallet-poc sleep loops and nip60 complete_deposit polling were flagged as findings, but the wallet-poc crate was deleted by PR #1509 and a single status read is the correct NUT-04/23 behavior.)

<!-- citations: [^019ed-18] [^11850-157] -->
