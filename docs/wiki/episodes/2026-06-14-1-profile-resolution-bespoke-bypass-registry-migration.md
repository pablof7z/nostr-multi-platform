---
type: episode-card
date: 2026-06-14
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: root-cause
status: superseded
subjects:
  - profile-resolution
  - claim-profile
  - outbox-model
  - nip-65
supersedes:
  - 2026-06-14-1-profile-claims-bypassed-registry-making-outbox
related_claims: []
source_lines:
  - 1-5
  - 48-83
  - 1233-1246
  - 1326-1378
  - 1596-1638
  - 1815-1850
captured_at: 2026-06-14T22:51:54Z
---

# Episode: Profile resolution: bespoke bypass → registry migration

## Prior State

claim_profile used a bespoke routing path (route_outbox_subscription_relays + req_for_relay) that bypassed the LogicalInterest registry. The outbox model existed in the router substrate but was inert for third-party profiles because the kernel never fetched kind:10002 (NIP-65 relay lists) for non-self pubkeys. Kind:0 queries for strangers were sent only to operator indexer relays (purplepag.es), which only carry profiles explicitly pushed there. ~50% of users never resolved because their kind:0 lived only on their own declared write relays.

## Trigger

User reported ~50% of pubkeys failing to resolve in Chirp iOS and demanded root-cause investigation across all layers (iOS UI, NMP kernel, relay querying strategy).

## Decision

Migrate claim_profile and drain_pending_reverify from bespoke REQ-building to the LogicalInterest registry path (same as claim_event), inheriting automatic D3 kind:10002 probe, outbox routing, set-cover, and Nip65Arrived re-route. Add liveness hint to the FFI (CacheOk=OneShot for feed avatars, Live=Tailing for profile screens; mixed claims resolve to Tailing). Delete the bespoke path entirely (profile_claim_request, pending_profile_claim_requests, ProfileRequestState, profile_requests dedup/retry state).

## Consequences

- Strangers' profiles now resolve through their own NIP-65 write relays, not just indexer relays — the ~50% failure should drop dramatically
- FFI signature changes from 4-arg to 5-arg (liveness parameter); all consumers (iOS, Android, WASM, ffi-stress) must update
- drain_pending_reverify (F-TTL refresh) also migrated — stale kind:0 re-verification now routes to author relays, not indexers-only
- Warm-reclaim zero-REQ preserved: a CacheOk claim of an already-resident profile registers no network interest
- Probe-epoch retry: reconnect + new-indexer triggers clear probed_mailboxes, allowing fresh 10002 discovery attempts
- Full codebase audit confirmed the bypass is isolated to the profile-claim family; claim_event, DMs, zaps, reactions, contacts, follows all use the registry correctly
- nmp-nip60 hardcoded purplepag.es relay is a separate minor instance of the same anti-pattern (filed as #1434)

## Open Tail

- Verify resolution improvement with before/after measurement on live follow set
- Expand iOS claim coverage to all pubkey surfaces not yet calling claim_profile (reaction/repost authors)
- Propagate ProfileLiveness to registry/gallery source-of-truth (currently Chirp-only)
- nip60 hardcoded purplepag.es relay pin migration (low priority, separate issue #1434)

## Evidence

- transcript lines 1-5
- transcript lines 48-83
- transcript lines 1233-1246
- transcript lines 1326-1378
- transcript lines 1596-1638
- transcript lines 1815-1850
