---
type: episode-card
date: 2026-06-13
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: root-cause
status: active
subjects:
  - nmp-marmot
  - group-message-resubscription
  - restart-liveness
supersedes: []
related_claims: []
source_lines:
  - 1-5
  - 4620-4662
  - 4666-4697
  - 4710-4734
  - 5000-5010
captured_at: 2026-06-13T19:18:42Z
---

# Episode: Post-restart MLS group messages never arrive — resubscribe all groups on register

## Prior State

After app restart, already-joined MLS groups lost their live message subscriptions. `register_with_keys` re-pushed the giftwrap inbox interest on startup but had no per-group kind:445 loop. The in-memory `Inner.group_relays` HashMap started empty, and `subscribe_group_messages` was only called from in-session ops (create/join). A user restarting the app would never receive new group messages until they created or joined another group.

## Trigger

Haiku-driven device testing (iOS simulator + Android emulator) revealed that after force-stop and relaunch, a message sent from one device never arrived on the other. Diagnostic agent traced the root cause: `register_with_keys` (ffi.rs) pushes `giftwrap_inbox_interest` but has no 'for each joined group, push its message interest' loop — the structural asymmetry. MDK persists group relays in SQLite but they are never restored+subscribed after restart.

## Decision

Added `MarmotService::group_relays` read seam (delegates to MDK's persisted `get_relays`), added `MarmotProjection::resubscribe_all_groups` that enumerates stored groups and routes each through the existing `cache_group_relays` choke point (seeds in-memory cache AND calls `subscribe_group_messages`), and called it from the register tail in `ffi.rs` immediately after the giftwrap inbox push. File-size gate required extracting `service_reads.rs` (65 LOC) and `projection/resubscribe.rs` (64 LOC) as sibling modules. PR #1261, merged.

## Consequences

- Post-restart live message receive works: verified on real devices — B's 'Fix-confirmed-live' decrypted on relaunched iOS A with no chat-reopen or nudge
- Gets store-replay for free: `push_interest_and_serve` enqueues a store cache-serve on every push, so re-pushed group interests also replay stored-but-unprocessed kind:445s with MDK dedup
- Idempotent: deterministic interest IDs + kernel de-dupe means re-register and account-switch are safe (slot overwrite, not double-subscribe)
- Three new file-backed two-session regression tests prove the relay cache is seeded after restart across fresh MarmotService instances

## Open Tail

- Interest-withdrawal asymmetry on unregister/account-switch: `nmp_marmot_unregister` does not withdraw per-group 445 interests (no `remove_interest` seam exists yet). De-dup makes receive correct, but prior-account group interests linger in registry until process exit
- Groups with empty relay sets are skipped (consistent with existing `cache_group_relays` empty-guard) — latent gap predating this PR

## Evidence

- transcript lines 1-5
- transcript lines 4620-4662
- transcript lines 4666-4697
- transcript lines 4710-4734
- transcript lines 5000-5010

