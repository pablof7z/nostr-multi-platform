---
type: episode-card
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: root-cause
status: superseded
subjects:
  - claim-profile-outbox-bypass
  - profile-resolution
  - logical-interest-registry
  - nmp-core-kernel
supersedes:
  - 2026-06-15-1-profile-resolution-broken-for-50-of
  - 2026-06-14-1-third-party-nip-65-relay-list
  - 2026-06-15-2-profile-claim-liveness-semantics-cacheok-oneshot
related_claims: []
source_lines:
  - 1-5
  - 48-83
  - 1596-1639
  - 1815-1849
  - 2003-2013
captured_at: 2026-06-15T00:48:49Z
---

# Episode: Profile resolution outbox bypass → registry migration

## Prior State

claim_profile and drain_pending_reverify built REQs directly via route_outbox_subscription_relays + req_for_relay without registering LogicalInterests, bypassing the D3 kind:10002 probe and Nip65Arrived re-route. Third-party profiles were queried only on indexer relays. Additionally, purplepag.es (one of two indexers) is AUTH-walled for anonymous queries, meaning the entire baseline came from primal.net alone — yielding only 10.2% profile resolution for a 1054-follow set.

## Trigger

User reported ~50% of pubkeys never resolve in Chirp iOS; multi-agent investigation revealed the outbox model machinery exists but is inert for third-party profiles because kind:10002 is never proactively fetched for non-self pubkeys, so MailboxCache has no entry → Lane 1 empty → queries fall to indexer-only. Full audit confirmed claim_profile + drain_pending_reverify are the sole straggler paths; all other subsystems (claim_event, DMs, zaps, reactions, contacts, follows) correctly use the registry.

## Decision

Migrate claim_profile and drain_pending_reverify to register LogicalInterests through the InterestRegistry (using claim_event as the proven reference implementation), inheriting D3 kind:10002 probe, outbox routing, set-cover, and Nip65Arrived re-route. Add liveness hint parameter (CacheOk→OneShot for feed avatars, Live→Tailing for profile screen) via FFI. Add nprofile relay hints (claim_profile_with_hints) and probe-epoch retry on indexer reconnect. Delete the bespoke profile_claim_request / pending_profile_claim_requests / ProfileRequestState path entirely.

## Consequences

- Measured ~5× profile resolution improvement: 10.2% → 50.0% (108 → 528 of 1054 follows), capped by the 57.6% of follows that publish a NIP-65 list
- purplepag.es AUTH wall means current indexer-only path yields zero from that relay — all 108 resolved profiles come from primal.net alone
- FFI signature changes from 4-arg to 5-arg (nmp_app_claim_profile adds liveness), requiring atomic update of nmp-android-ffi and all in-repo callers
- drain_pending_reverify migrated alongside claim_profile as a coherent unit (same subsystem: cold fetch + refresh of replaceable identity)
- claim_event confirmed as the reference implementation pattern for all future interest-based subscriptions
- nip60 hardcoded purplepag.es relay pin filed as separate low-priority follow-up (#1434) — not part of this migration
- iOS PR adds ProfileLiveness enum (.cacheOk / .live) with protocol-extension convenience default; Chirp call sites wired (feed avatars→.cacheOk, profile screen→.live, mentions/attributions→.cacheOk)
- Kernel PR #1436 open with all core tests green (1541+113+60 passed); one web Playwright E2E test under investigation before merge

## Open Tail

- Web Playwright test 'feed renders real signed notes after connect' failing on PR branch (green on master) — investigation agent dispatched, merge gated on verdict
- iOS PR held until kernel PR merges
- Remaining pipeline queued: version cut → consumer-app updates → device installs → docs update

## Evidence

- transcript lines 1-5
- transcript lines 48-83
- transcript lines 1596-1639
- transcript lines 1815-1849
- transcript lines 2003-2013
