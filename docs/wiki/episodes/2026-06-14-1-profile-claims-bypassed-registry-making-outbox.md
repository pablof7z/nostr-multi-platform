---
type: episode-card
date: 2026-06-14
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: root-cause
status: superseded
subjects:
  - profile-claim-registry-migration
  - outbox-model-third-party
  - kind0-resolution
supersedes:
  - 2026-06-14-1-stranger-profile-resolution-broken-outbox-model
related_claims: []
source_lines:
  - 1-101
  - 233-1192
  - 1249-1637
captured_at: 2026-06-14T22:41:51Z
---

# Episode: Profile claims bypassed registry, making outbox model inert for third-party profiles

## Prior State

claim_profile used a bespoke path (profile_claim_request → route_outbox_subscription_relays → req_for_relay) that bypassed the LogicalInterest registry and recompile chokepoint. The outbox model existed in the router substrate but was inert for third-party profiles: kind:10002 was never proactively fetched for strangers, so MailboxCache had no entry → Lane 1 empty → query sent only to operator indexer relays. drain_pending_reverify (F-TTL) had the same bespoke bypass. The 10002 probe was fire-once-never-retry.

## Trigger

User reported ~50% of pubkeys never resolve in Chirp iOS. Multi-agent investigation traced the root cause to three faults: (1) the bespoke profile-claim path bypasses the registry and thus the D3 10002 probe + Nip65Arrived re-route, (2) the 10002 probe never retries on miss, (3) iOS UI only claims profiles from NostrAvatar/ProfileView, not mentions/reactions/reposts. Full codebase audit confirmed the bypass is isolated to the profile-claim family — claim_event and all other subsystems correctly use the registry.

## Decision

Migrate claim_profile and drain_pending_reverify onto the registry path (ensure_sub/drop_owner of LogicalInterest{kinds:[0], authors:[P], limit:None}), inheriting the 10002 probe, set-cover relay minimization, progressive re-route, and nprofile hints from the recompile chokepoint. Delete the bespoke path (profile_claim_request, pending_profile_claim_requests, ProfileRequestState, refresh_profile_after_mailbox). Add liveness hint to FFI (CacheOk→OneShot for feed avatars, Live→Tailing for ProfileView; Tailing wins on mixed claims). Add epoch-gated probe retry-on-miss (epoch bumps on indexer reconnect + new-indexer-added). Seed nprofile-embedded relay hints into the claim interest.

## Consequences

- Bespoke profile REQ path entirely deleted; profile.rs loses its relay-awareness (layering principle satisfied)
- Third-party profile claims now inherit automatic 10002 discovery → outbox routing to author's write relays
- Nip65Arrived recompile automatically re-routes registered claims off indexer fallback when 10002 lands
- Feed avatars use OneShot lifecycle (no live subs in scrolling feed); ProfileView uses Tailing (reactive to profile edits)
- Mixed CacheOk+Live claims on same pubkey resolve to Tailing (stronger lifecycle wins)
- Probe retries on indexer reconnect and new-indexer-added via epoch gating (no polling)
- nprofile relay hints allow resolution of authors whose 10002 is on no indexer
- drain_pending_reverify folded into same migration — F-TTL refreshes also inherit 10002 discovery
- New liveness c_int parameter on nmp_app_claim_profile FFI (no new symbol, same precedent as force)
- nip60 hardcoded purplepag.es in relay.rs:104 flagged as separate low-priority follow-up (issue #1434)

## Open Tail

- Implementation in progress (kernel worktree + iOS PR); baseline before/after measurement running against live network
- iOS Fault-A (missing claimProfile calls for mentions/reactions/reposts) is a separate PR blocked on kernel FFI signature
- Version cut + consumer-app updates (podcast-player, hl, win-the-day, tenex-off) queued after both PRs merge
- nip60 hardcoded purplepag.es relay pin is a separate tracked issue, not part of this migration

## Evidence

- transcript lines 1-101
- transcript lines 233-1192
- transcript lines 1249-1637
