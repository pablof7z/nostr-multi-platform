---
type: episode-card
date: 2026-06-14
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: root-cause
status: superseded
subjects:
  - profile-resolution
  - outbox-model
  - claim-profile
  - interest-registry
supersedes:
  - 2026-06-14-1-profile-resolution-bespoke-bypass-registry-migration
related_claims: []
source_lines:
  - 1-5
  - 37-46
  - 73-83
  - 1331-1378
  - 1596-1637
  - 1815-1849
  - 2003-2012
captured_at: 2026-06-14T23:14:44Z
---

# Episode: Third-party profile resolution inert: outbox model bypassed by bespoke REQ path

## Prior State

claim_profile and drain_pending_reverify used bespoke REQ-building (route_outbox_subscription_relays + req_for_relay) that bypassed the InterestRegistry. The outbox model existed in the router but was inert for third-party profiles because kind:10002 (NIP-65 relay lists) were only fetched for the self account at startup. Profile resolution for strangers relied solely on indexer relays (purplepag.es + primal.net). ~50% of user pubkeys never resolved.

## Trigger

User reported ~50% of pubkeys fail to resolve in Chirp iOS. Multi-agent investigation traced the end-to-end path and found: (1) claim_profile bypasses the registry → no D3 10002 probe → no outbox routing for strangers, (2) drain_pending_reverify has the same bespoke bypass (F-TTL re-verify silently misses forever if 10002 not cached), (3) purplepag.es is AUTH-walled returning 0 kind:0 to anonymous queries — effectively leaving only primal.net as a working indexer. Baseline measurement confirmed only 10.2% of follows resolve via indexer-only path vs 50.0% via outbox/own relays.

## Decision

Migrate claim_profile + drain_pending_reverify from bespoke REQ-building to registering LogicalInterests through the InterestRegistry chokepoint, inheriting D3 kind:10002 probe + outbox routing + Nip65Arrived re-route. Add liveness hint parameter (CacheOk=OneShot for feed avatars, Live=Tailing for profile screen) via new 5th FFI arg. claim_event confirmed as the proven reference implementation. Full codebase audit confirmed the anti-pattern is isolated to the profile-claim family only — all other paths (claim_event, DMs, zaps, reactions, contacts, follows) correctly use the registry.

## Consequences

- ~5× improvement in profile resolution (10.2% → 50.0%, +420 profiles resolved)
- New 5-arg FFI signature: nmp_app_claim_profile(void *app, const char *pubkey, const char *consumer_id, int force, int liveness) — all 4-arg callers swept across nmp-core, nmp-ffi, nmp-android-ffi, nmp-wasm, apps, ffi-stress
- Warm-reclaim zero-REQ preserved: CacheOk claim of a resident profile registers no network interest
- Probe retry-on-miss on indexer reconnect via relay_connected_url clearing probed_mailboxes
- Deleted bespoke infrastructure: profile_claim_request, pending_profile_claim_requests, ProfileRequestState/profile_requests, refresh_profile_after_mailbox, relay_lifecycle re-queue
- nip60 hardcoded purplepag.es relay pin filed as separate low-priority follow-up (issue #1434)
- Resolution ceiling now gated by how many follows publish a NIP-65 list (608/1054); users without relay lists remain unreachable

## Open Tail

- Resolution ceiling: ~50% of follows lack NIP-65 relay lists — outbox model cannot help them; indexer coverage or relay-list discovery heuristics needed for the remaining gap
- purplepag.es AUTH-wall means anonymous clients get zero profiles from it — may need authenticated relay queries or replacement indexer
- Registry/gallery apps not yet wired with liveness parameter (2-arg default remains CacheOk)
- iOS PR held pending kernel PR #1436 merge and CI green

## Evidence

- transcript lines 1-5
- transcript lines 37-46
- transcript lines 73-83
- transcript lines 1331-1378
- transcript lines 1596-1637
- transcript lines 1815-1849
- transcript lines 2003-2012
