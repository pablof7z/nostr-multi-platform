---
type: episode-card
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: architecture
status: superseded
subjects:
  - nmp-core-profile-claim
  - outbox-model-third-party
  - kind0-resolution
supersedes:
  - 2026-06-15-1-profile-resolution-outbox-bypass-registry-migration
related_claims: []
source_lines:
  - 59-84
  - 1596-1638
  - 1815-1849
captured_at: 2026-06-15T01:10:04Z
---

# Episode: Outbox model inert for third-party profile resolution — migrate to InterestRegistry

## Prior State

claim_profile used bespoke REQ path via route_outbox_subscription_relays + req_for_relay without registering a LogicalInterest. The outbox router machinery fully existed (Lane 1 NIP-65 write relays, Lane 6 indexer, Lane 7 app-relay fallback), but MailboxCache had no entries for third-party pubkeys because kind:10002 was only fetched for the self/active account at startup. So Lane 1 was always empty for strangers → queries hit only operator indexer relays. drain_pending_reverify had the same bespoke pattern.

## Trigger

User reported ~50% of pubkeys never resolve in Chirp iOS. Multi-agent investigation traced the kernel kind:0 acquisition path end-to-end and found the outbox model implemented but inert for third-party profiles. Full codebase audit confirmed the bypass is isolated to the profile-claim family (claim_profile + drain_pending_reverify); all other paths (claim_event, DMs, zaps, reactions, contacts, follows) correctly use the InterestRegistry.

## Decision

Migrate claim_profile and drain_pending_reverify to register LogicalInterests through the InterestRegistry chokepoint, inheriting the intrinsic D3 kind:10002 probe + outbox routing + set-cover + Nip65Arrived re-route. Add liveness hint (CacheOk→OneShot for feed avatars, Live→Tailing for profile screen), nprofile hints via claim_profile_with_hints, and probe retry-on-miss on indexer reconnect. Delete the bespoke path (profile_claim_request, pending_profile_claim_requests, ProfileRequestState). Use claim_event as the proven reference implementation.

## Consequences

- Profile resolution improves from 10.2% to 50.0% (~5×) for a 1054-user follow set
- FFI signature changes from 4-arg to 5-arg (adding liveness), requiring all callers (nmp-ffi, nmp-android-ffi, nmp-wasm, apps, ffi-stress) to be swept
- Warm-reclaim zero-REQ preserved (CacheOk claim of resident profile registers no network interest)
- Tailing-wins dedup: mixed CacheOk+Live claims resolve to Tailing via set_sub upgrade
- drain_pending_reverify now uses OneshotApi::request + pending_reverify_oneshots bridge, preserving F-TTL EOSE re-stamp
- Kernel PR #1436 opened, all core tests green (1541 nmp-core + 113 nmp-ffi + 60 doctrine)

## Open Tail

- Web Playwright test (feed.spec.ts:24 'renders after connect') fails 2/2 on PR branch but 8/8 green on master — under investigation as potential real regression from registry migration changing kernel timing via wasm path
- Merge of #1436 gated on resolving the web feed test failure

## Evidence

- transcript lines 59-84
- transcript lines 1596-1638
- transcript lines 1815-1849
