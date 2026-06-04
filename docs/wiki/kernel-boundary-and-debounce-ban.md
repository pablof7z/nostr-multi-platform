---
title: Kernel Boundary & UI Debounce Ban
slug: kernel-boundary-and-debounce-ban
summary: The kernel must not contain UI-level debounce or dedup guards; double-tap prevention is the host app's responsibility
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-03
updated: 2026-06-03
verified: 2026-06-03
compiled-from: conversation
sources:
  - session:7f143c67-6e46-424a-90a8-5bf844947fee
  - session:cf071d35-ee9b-4a1f-a3b8-885c651e8cce
---

# Kernel Boundary & UI Debounce Ban

## Kernel Boundary: No UI-Level Debounce or Dedup

The kernel must not contain UI-level debounce or dedup guards; double-tap prevention is the host app's responsibility. The `inflight_dispatches` double-tap dedup guard must be removed from `nmp_app_dispatch_action`. The `creating_account_inflight` debounce guard in `identity.rs` must also be removed; preventing double-keypair-mint on account creation is a UI concern, not a kernel safety property. The 'card' concept does not belong at the framework layer; it smuggles UI decisions into the kernel.

<!-- citations: [^7f143-20] [^cf071-1] -->
## See Also

