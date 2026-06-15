---
type: episode-card
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: root-cause
status: superseded
subjects:
  - nmp-core-profile-resolution
  - chirp-ios
  - nmp-ffi-claim-profile
  - outbox-model
  - nip-65
supersedes:
  - 2026-06-15-1-profile-resolution-overhaul-outbox-model-activated
  - 2026-06-15-1-third-party-profile-outbox-discovery-via
  - 2026-06-15-4-ios-self-claim-profiles-for-all
related_claims: []
source_lines:
  - 1-5
  - 59-84
  - 2566-2567
  - 2636-2638
  - 3162-3177
captured_at: 2026-06-15T05:10:12Z
---

# Episode: Profile resolution overhaul — outbox discovery for third-party pubkeys

## Prior State

Profile claims for third-party pubkeys used a bespoke path (profile_claim_request) that bypassed NMP's outbox/registry system. The outbox model (NIP-65 kind:10002 relay lists) existed in the router but was inert for third-party profiles because kind:10002 was only fetched for the self/active account at startup. Kind:0 queries were sent only to operator/indexer relays (including purplepag.es, which AUTH-walls anonymous connections). A prior proactive fetch on note ingest had been deliberately removed (F-CR-00). Result: ~50% of pubkeys never resolved.

## Trigger

User reported ~50% of pubkeys in Chirp iOS never resolve and requested root-cause investigation across all layers (iOS UI, NMP kernel, indexer relay strategy). Multi-agent investigation revealed the outbox model gap as the root cause: the kernel never proactively fetches kind:10002 for third-party pubkeys, so the MailboxCache is empty and queries only hit indexers.

## Decision

Migrated profile claims onto the registry chokepoint (register_profile_claim_interest → recompile_and_diff_with_lookup). Third-party profiles now trigger kind:10002 relay-list discovery (D3 probe via batched subscription), which feeds back into the outbox router to route kind:0 queries to the author's own relays. Added retry-on-miss with probed_mailboxes re-arm gating (only on genuine reconnect, not every connect). Added liveness hint to FFI (nmp_app_claim_profile 4→5 args: CacheOk for feed avatars, Live for profile-screen views). iOS UI surfaces (mentions, reply attributions, standalone names) now self-claim profiles. Web/wasm dispatch arm skips snapshot on claim to avoid SolidJS <For> remount infinite loop.

## Consequences

- Kind:0 resolution rate improved from ~10.2% to ~50.0% (~5× improvement)
- Breaking FFI change: nmp_app_claim_profile 4→5 args; all consumer apps must pass liveness (CacheOk=0 or Live=1)
- drain_pending_reverify bug (same class as original bespoke bypass) migrated to registry path
- Web/wasm feed no longer snapshots on claim (prevents SolidJS remount loop regression)
- nmp-blossom release-manifest CI red fixed as side effect of v0.8.0 version cut
- nip60 follow-up issue (#1434) filed from investigation findings
- All four NMP-consuming iOS apps rebuilt and sideloaded on device with v0.8.0

## Open Tail

- Remaining ~50% of pubkeys still unresolved (likely users with no kind:10002 relay list at all; further strategies needed for fallback discovery)
- Consumer app upgrade branches (podcast-player, tenex-off, hl) not yet PR'd to their upstreams
- nmp-feedback was pushed directly to main to unblock podcast-player pin; may need coordination

## Evidence

- transcript lines 1-5
- transcript lines 59-84
- transcript lines 2566-2567
- transcript lines 2636-2638
- transcript lines 3162-3177
