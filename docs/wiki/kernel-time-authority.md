---
title: Kernel Time Authority & App Timestamp Ban (D9)
slug: kernel-time-authority
summary: "App-side timestamps using `Utc::now().timestamp()` are invalid; all timestamps must come from the kernel via `kernel.now_secs()`."
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-03
updated: 2026-06-03
verified: 2026-06-03
compiled-from: conversation
sources:
  - session:d8869714-0ee5-4fe3-94db-1efd068b1c58
  - session:7f143c67-6e46-424a-90a8-5bf844947fee
---

# Kernel Time Authority & App Timestamp Ban (D9)

## Timestamp Authority

App-side timestamps using `Utc::now().timestamp()` are invalid; all timestamps must come from the kernel via `kernel.now_secs()`. The kernel owns the clock and stamps `created_at` at signing time.

<!-- citations: [^d8869-22] [^7f143-9] -->
## See Also

