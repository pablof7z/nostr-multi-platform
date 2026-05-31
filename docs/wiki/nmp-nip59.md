---
title: nmp-nip59 — Gift Wrap and DM Timeout Constants
slug: nmp-nip59
summary: GIFT_WRAP_TOTAL_TIMEOUT is 12 seconds for the dm.rs outer wait, fixing the silent mid-chain timeout.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-05-27
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:156aa64b-42e1-4d3b-96ce-25b31fc06fec
  - session:cd2b6122-2b7c-43fc-941b-c51e79ffc691
---

# nmp-nip59 — Gift Wrap and DM Timeout Constants

## Gift Wrap Timeout

GIFT_WRAP_TOTAL_TIMEOUT is 12 seconds for the dm.rs outer wait, fixing the silent mid-chain timeout. The gift_wrap() function in nmp-nip59/src/wrap.rs was deleted as dead code superseded by gift_wrap_with_signer.

<!-- citations: [^156aa-7] [^cd2b6-4] -->
## See Also

