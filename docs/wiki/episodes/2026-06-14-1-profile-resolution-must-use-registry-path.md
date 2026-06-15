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
  - nmp-kernel
supersedes:
  - 2026-06-14-1-profile-resolution-root-cause-claim-profile
related_claims: []
source_lines:
  - 1-5
  - 48-84
  - 1414-1479
  - 1596-1639
  - 1815-1849
  - 2000-2013
captured_at: 2026-06-14T23:57:22Z
---

# Episode: Profile resolution must use registry path, not bespoke REQ

## Prior State

claim_profile built REQs directly via route_outbox_subscription_relays + req_for_relay, bypassing the LogicalInterest registry. This meant no D3 kind:10002 probe, no Nip65Arrived re-route, and fire-once-no-retry — so strangers whose kind:0 wasn't on an indexer relay would silently never resolve. drain_pending_reverify had the same bespoke pattern. The rest of the codebase (claim_event, DMs, zaps, reactions, contacts) correctly used the registry.

## Trigger

User reported ~50% of profile pictures never resolve on Chirp iOS. Multi-agent investigation traced this to the bespoke bypass in claim_profile: the kernel never fetches kind:10002 for third-party pubkeys, so the outbox model is inert for stranger profile resolution — queries go only to indexer relays. Systematic audit confirmed claim_profile (+ drain_pending_reverify) is the sole straggler. Baseline measurement showed only 10.2% of follows resolve via indexers; 50.0% resolve when own-relay outbox is used. purplepag.es (one of two indexers) returns 0 kind:0 anonymously (NIP-42 AUTH-walled), so the entire current baseline comes from primal.net alone.

## Decision

Migrate claim_profile and drain_pending_reverify to the LogicalInterest registry path using claim_event as the proven-correct reference implementation. Introduce liveness semantics (CacheOk=OneShot for feed avatars, Live=Tailing for profile screen), nprofile hints via claim_profile_with_hints, and probe-epoch retry on indexer reconnect. Delete the bespoke REQ code path entirely (profile_claim_request, pending_profile_claim_requests, ProfileRequestState). FFI signature changed from 4-arg to 5-arg (liveness).

## Consequences

- ~5× improvement in profile resolution (10.2%→50.0% measured on user's follow set of 1054)
- purplepag.es is AUTH-walled for anonymous queries — all 108 current resolutions come from primal.net alone
- NIP-60 hardcoded purplepag.es relay pin filed as separate follow-up (#1434, low priority, off-actor worker code)
- FFI 4→5 arg arity change required cross-crate sweep (nmp-ffi, nmp-android-ffi, nmp-wasm, apps, ffi-stress bins)
- drain_pending_reverify folded into the same migration (same subsystem: cold claim + TTL refresh of replaceable identity)
- Web Playwright test regression (feed.spec.ts 'renders after connect') gating merge — twice red on PR branch, green on master, under investigation
- iOS liveness wiring complete (PR held pending kernel merge): feed avatars → .cacheOk, profile screen → .live, mention/attribution claims → .cacheOk

## Open Tail

- Web Playwright regression blocking kernel PR #1436 merge — agent dispatched to reproduce and determine flake vs real regression
- iOS PR held until kernel PR merges
- Version cut + consumer-app updates + device installs queued behind merge
- Profile liveness propagation to registry/gallery shared source-of-truth deferred as separable follow-up

## Evidence

- transcript lines 1-5
- transcript lines 48-84
- transcript lines 1414-1479
- transcript lines 1596-1639
- transcript lines 1815-1849
- transcript lines 2000-2013
