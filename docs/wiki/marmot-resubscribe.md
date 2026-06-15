---
title: Marmot Resubscribe
slug: marmot-resubscribe
topic: mls
summary: "On register_with_keys (restart), MarmotProjection resubscribes per-group kind:445 message interests by enumerating persisted groups, reading their stored relays"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-15
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:78c8ec3a-f558-4738-98af-1f3af4978ec4
  - session:c9a794f6-6ad7-4ee9-a620-fc342fd495c3
---

# Marmot Resubscribe

## Restart Re-subscription

On register_with_keys (restart), MarmotProjection resubscribes per-group kind:445 message interests by enumerating persisted groups, reading their stored relays via MDK, and routing through the existing cache_group_relays/subscribe_group_messages choke point. Marmot group relays are persisted in MDK SQLite (group_relays table) and survive restart; create_group and accept_welcome both write them, get_relays reads them back.

<!-- citations: [^78c8e-23] [^78c8e-52] [^78c8e-68] [^78c8e-86] [^78c8e-106] -->
## Key Package Autopublish

Marmot (MLS) key-package autopublish fires on all local-key sign-in paths, not just nmp_marmot_register_active. The autopublish flag follows the rule: gaining a local signing key means flag set, consumed (atomic swap) at first register, one-shot. set_pending_mls_autopublish is pub(crate), not pub; tests exercise the real nmp_app_signin_nsec entry point rather than the raw atomic setter.

<!-- citations: [^78c8e-51] [^78c8e-105] -->
## Interest Withdrawal

Interest withdrawal for per-group kind:445 subscriptions on sign-out/account-switch has no seam yet (NmpApp has push_interest but not remove_interest); de-duplication makes the receive fix correct without it, but stale interests linger until process exit. <!-- [^78c8e-87] -->

## Snapshot Query Overhead

MarmotProjection::messages_since performs a live SQLite query on every snapshot cycle even when no MLS events arrived, with 26ms spent on a pread kernel syscall from SQLite WAL shared-lock. MlsGroup::load also calls SQLite on every snapshot cycle, loading the entire MLS group state from disk each time rather than keeping it in memory.

<!-- citations: [^c9a79-2] [^c9a79-13] -->
## Snapshot Query Optimization

MarmotProjection::messages_since must use a watermark and dirty-flag to skip the SQLite query on snapshot cycles with no new MLS events. <!-- [^c9a79-3] -->
