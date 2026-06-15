---
type: episode-card
date: 2026-06-14
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: root-cause
status: superseded
subjects:
  - nmp-kernel-profile-resolution
  - outbox-model-nip65
  - kind-10002-acquisition
  - profile-request-dedup
supersedes: []
related_claims: []
source_lines:
  - 73-84
  - 101-114
  - 348-373
  - 471-561
  - 589-589
captured_at: 2026-06-14T21:20:46Z
---

# Episode: Third-party NIP-65 relay-list acquisition + fire-and-never-retry fix

## Prior State

The 7-lane GenericOutboxRouter was implemented and wired for profile resolution, but kind:10002 (NIP-65 relay lists) was only fetched for the active logged-in account at startup (startup.rs:30 SELF_KINDS_TAILING). The MailboxCache was therefore empty for all third-party authors, causing Lane 1 (author's own write relays) to always return nothing — every kind:0 query fell through to indexer-only Lane 6/7. Additionally, profile_requests.requested was marked at REQ-emit time and never cleared on empty-EOSE miss, permanently abandoning any profile not found on the two operator indexers (relay.primal.net + purplepag.es).

## Trigger

User reported ~50% of pubkeys never resolve in Chirp iOS. Multi-agent investigation traced the full path from UI claim through kernel routing to relay subscription, revealing that (1) MailboxCache is empty for third-party authors so the outbox model is inert, and (2) empty-EOSE misses are never retried. purplepag.es AUTH-gating/rate-limiting further amplifies silent failures.

## Decision

Introduce a shared kernel routine `ensure_relay_lists_for_authors(authors)` called from both `sync_follow_feed_interests` (follows feed, declares kind:1/6) and `claim_profile` (any pubkey, follow or stranger, declares kind:0). It batches `{kinds:[10002],authors:[...]}` REQs to already-open indexer sockets (≤50/batch). New `RelayListRequestState` mirrors `ProfileRequestState` with dedup + retry-on-indexer-reconnect AND retry-on-new-relay-added. Embedded relay hints from nprofile/nevent are used for direct fetching when indexers lack the author's 10002. kind:0 remains claim-only — follow-membership does not auto-fetch kind:0. Progressive enhancement is free via existing Lane 6/7 fallback (feed never blank, then refines to per-author relays as 10002s land).

## Consequences

- Third-party kind:10002 acquisition fills MailboxCache, enabling Lane 1 routing to author's own relays for both content and profile queries
- Existing `on_mailbox_changed → refresh_profile_after_mailbox` machinery now actually fires for third-party authors, re-routing kind:0 to their write relays
- Fire-and-never-retry defect class is fixed for both relay-list requests and (via same pattern) profile requests — retries now trigger on indexer-reconnect and on new-relay-added
- No new transport capability needed — pool already dials arbitrary relay URLs on demand with Temporary connection kind and 60s idle teardown
- Relay intersection optimization (not artificial capping) governs connection scale for large follow sets
- Relay hints from nprofile/nevent provide a direct path when indexers lack an author's 10002, breaking the self-sealing dependency where 10002 and kind:0 both fail on the same two indexers

## Open Tail

- Design is approved but no code written yet — implementation pending
- Profile-request dedup (profile_requests.requested) still needs the same retry-on-new-relay-added trigger applied to it, not just to the new RelayListRequestState
- Relay admission/health backoff (relay_admission.rs) can silently shrink the indexer lane — may need investigation if resolution rate still degrades after this fix

## Evidence

- transcript lines 73-84
- transcript lines 101-114
- transcript lines 348-373
- transcript lines 471-561
- transcript lines 589-589
