---
title: "NIP-17 DM Gating Policy & Kind:10050 Requirement"
slug: nip-17-dm-gating-policy
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
  - session:2c4adc99-0b1b-430c-8594-834da3ab4cef
  - session:1670fcb8-f275-498c-975b-8bd912331ded
---

# NIP-17 DM Gating Policy & Kind:10050 Requirement

## NIP-17 DM Gating Policy

NIP-17 DMs must not be available when no DM relay has been explicitly set. Gating the self side so a user without their own kind:10050 cannot open DM compose is the B1 requirement for NIP-65 publish UI. PR #228 (fail-closed when recipient has no kind:10050) already enforces this policy on the sender side. DM send logic (NIP-17) must not be in nmp-core. The kind:10050 DM-inbox cache lives in nmp-nip17, not in the router, because only nmp-nip17's DM send action reads it. DmInboxProjection exists but end-to-end receipt of NIP-17 DMs from a real relay is unverified. nmp_nip59::GIFT_WRAP_TOTAL_TIMEOUT is 12 seconds for the dm.rs outer wait to prevent silent mid-chain timeouts.

<!-- citations: [^156aa-3] [^156aa-4] [^2c4ad-5] [^1670f-4] -->
## See Also

