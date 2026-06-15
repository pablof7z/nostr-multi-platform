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
updated: 2026-06-15
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:78c8ec3a-f558-4738-98af-1f3af4978ec4
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
  - session:ab8061fc-b277-4ba4-bf55-1532bcb1aa90
  - session:c9a794f6-6ad7-4ee9-a620-fc342fd495c3
  - session:019eca68-85c6-77e0-b237-e58f6c894f72
  - session:78b50727-bccd-4088-8493-a07624a4fa83
---

# Interest Withdrawal

## Interest Withdrawal

Interest IDs are deterministic (group_message_interest_id over group_id_hex + relay_url); the kernel de-dupes via registry push replacing the slot, making re-register and account-switch safe without explicit interest withdrawal. The registry InterestRegistry already supports refcounted claims via Slot.owners (BTreeSet<SubOwnerKey>) with ensure_sub/drop_owner, providing dedup where multiple consumers of the same pubkey share one interest; this is a direct replacement for the bespoke profile_claims refcount map and supports the same claim/release semantics. push_interest_and_serve must be change-gated: set_sub/push must return whether the slot's interest actually changed (comparing the new shape against the existing slot interest), and InvalidateCompile plus the cache-serve must only be enqueued on a real change. When two consumers claim the same pubkey with different liveness (CacheOk + Live), the slot resolves to Tailing (the stronger lifecycle) via set_sub upgrade, and retains Tailing until all owners drop. The per-group kind:445 interest subscription has no remove_interest seam; on account switch the prior account's group interests linger in the registry until process exit (de-duplication makes receive correct without withdrawal, but the stale interests consume relay bandwidth). Issue #1281 exempts since=None from the T127 watermark rewrite so an all-time interest stays unbounded, while interests with Some(t) still get raised to max(t, watermark+1). ADR-0036 documents a composition-root interest expansion topology that was never built; the live owner is the kernel's sync_follow_feed_interests. Claim interests register with limit:None so same-shape author-union coalescing batches multiple avatar claims into a single kind:0 REQ, matching the prior bespoke batching behavior; this avoids a per-author REQ storm from Rule 5's refusal to merge shapes with limits. The F-TTL reverify mechanism (claim_replaceable) is independent of the registry and survives the migration unchanged. Profile claims and replaceable reverify are registered as owner-keyed LogicalInterests and OneShots on the InterestRegistry, with no direct REQ construction via req_for_relay remaining. NIP-17 DMs and NIP-57 zaps use registered LogicalInterests exclusively (PTagRouting::Nip17DmRelays and PTagRouting::Nip65ReadRelays respectively) with no bespoke REQ building; they are clean of the bypass anti-pattern, requiring no migration. drain_pending_reverify is migrated to register OneShot interests via OneshotApi::request (mirroring pending_discovery_oneshots), with F-TTL EOSE re-stamp preserved. The relay-connection can_send/park-until-connect behavior is subsumed by the registry + planner reconnect-replay; claim_profile always registers and the planner lands the REQ when an indexer connects. The SubscriptionLifecycle recompile_and_diff_with_lookup is the single generic chokepoint every authors-filtered interest flows through, and it already implements immediate app-relay fallback, batched kind:10002 probing for uncached authors, and progressive re-route on Nip65Arrived — this mechanism is inherited by the migrated profile claims. The InterestShape contract requires sorted, deduped containers (BTreeSet) for deterministic equality and hashing. Replacing BTreeSet<String> with BTreeSet<[u8;32]> for pubkeys is unsound because it changes the serde representation, which breaks canonical_filter_hash (computed as stable_hash64(serde_json::to_string(shape))), churning every sub_id and silently invalidating the watermark store. Vec<[u8;32]> must not be used for InterestShape authors because Vec is not sorted or deduped, which breaks the determinism contract required for plan-id stability and dedup correctness. If pubkey cloning remains a performance bottleneck after memoization, the correct fix is pubkey interning (e.g., Arc<[u8;32]> or a u32 interner handle) combined with a serialize_with/deserialize_with hex adapter that preserves the exact serde wire representation—not a bare type swap.

<!-- citations: [^78c8e-20] [^78c8e-48] [^02745-84] [^2e544-371] [^ab806-11] [^ab806-29] [^ab806-41] [^ab806-63] [^ab806-87] [^ab806-97] [^c9a79-20] [^c9a79-25] [^019ec-36] [^ab806-237] [^ab806-243] [^78b50-219] [^78b50-258] -->
