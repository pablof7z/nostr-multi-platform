---
title: NIP-17 DM Relay Requirement
slug: nip17-dm-relay-requirement
summary: NIP-17 DMs must not be available when no DM relay has been explicitly set
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-05-23
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:156aa64b-42e1-4d3b-96ce-25b31fc06fec
  - session:95d02563-5473-4d84-96e1-cd342e1b04d1
  - session:1670fcb8-f275-498c-975b-8bd912331ded
---

# NIP-17 DM Relay Requirement

## DM Relay Requirement

NIP-17 DMs must not be available when no DM relay has been explicitly set. The self side must be gated so a user without their own kind:10050 cannot open DM compose. All Chirp apps must allow configuring kind 10050. PR #228 (codex/nip17-dm-relay-fail-closed) enforces sender-side fail-close when a recipient has no kind:10050. NIP-17 (DMs) must not live in nmp-core — it should be in nmp-nip17. The kind:10050 DM-inbox cache lives in nmp-nip17 (the NIP crate that reads it), not in nmp-router.

<!-- citations: [^156aa-5] [^95d02-14] [^1670f-9] -->
## See Also

