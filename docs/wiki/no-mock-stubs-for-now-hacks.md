---
title: No Mock, Stub, or 'For Now' Hacks Permitted
slug: no-mock-stubs-for-now-hacks
summary: Any mock, stub, or 'for now' hack that deviates from perfect architectural execution is completely forbidden and must be fixed immediately.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-23
updated: 2026-05-28
verified: 2026-05-23
compiled-from: conversation
sources:
  - session:c6c4eedd-935c-4304-bff1-e041952f2def
  - session:8bd548b9-af6d-4108-bc64-16ebf8dfa4f7
---

# No Mock, Stub, or 'For Now' Hacks Permitted

## No Mocks, Stubs, or 'For Now' Hacks

Any mock, stub, or 'for now' hack that deviates from perfect architectural execution is completely forbidden and must be fixed immediately. Additionally, render_test_data must be gated behind #[cfg(test)] and must not be used for live operation.

<!-- citations: [^c6c4e-5] [^8bd54-3] -->
## See Also

