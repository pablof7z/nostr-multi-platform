---
title: Interest Withdrawal
slug: interest-withdrawal
topic: cache-serve
summary: Interest IDs are deterministic (group_message_interest_id over group_id_hex + relay_url); the kernel de-dupes via registry push replacing the slot, making re-re
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-14
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:78c8ec3a-f558-4738-98af-1f3af4978ec4
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
---

# Interest Withdrawal

## Interest Withdrawal

Interest IDs are deterministic (group_message_interest_id over group_id_hex + relay_url); the kernel de-dupes via registry push replacing the slot, making re-register and account-switch safe without explicit interest withdrawal. The per-group kind:445 interest subscription has no remove_interest seam; on account switch the prior account's group interests linger in the registry until process exit (de-duplication makes receive correct without withdrawal, but the stale interests consume relay bandwidth). Issue #1281 exempts since=None from the T129 watermark rewrite so an all-time interest stays unbounded, while interests with Some(t) still get raised to max(t, watermark+1). ADR-0036 documents a composition-root interest expansion topology that was never built; the live owner is the kernel's sync_follow_feed_interests.

<!-- citations: [^78c8e-20] [^78c8e-48] [^02745-84] [^2e544-371] -->
