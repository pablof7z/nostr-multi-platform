---
type: episode-card
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: root-cause
status: superseded
subjects:
  - profile-resolution
  - outbox-model
  - interest-registry
  - claim-profile
  - nmp-kernel
supersedes:
  - 2026-06-15-1-outbox-model-inert-for-third-party
related_claims: []
source_lines:
  - 1-44
  - 48-83
  - 1597-1639
  - 1815-1850
  - 1998-2014
captured_at: 2026-06-15T01:36:35Z
---

# Episode: Third-party profile resolution was inert — migrate claim_profile through InterestRegistry

## Prior State

The outbox model (NIP-65 relay routing) was implemented in the kernel but inert for third-party profile resolution: claim_profile and drain_pending_reverify bypassed the InterestRegistry, querying only operator indexer relays. kind:10002 was fetched only for the self-account at startup, so stranger relay lists were never learned. Combined with purplepag.es requiring NIP-42 AUTH (returning 0 kind:0 anonymously), only ~10% of followed users' profiles resolved. The iOS UI also did not claim profiles for mentions/attributions, leaving those pubkeys entirely unrequested.

## Trigger

User reported ~50% of pubkeys never resolve in Chirp iOS. Multi-agent investigation traced the full acquisition path and found claim_profile going exclusively to Indexer-role relays with no D3 10002 probe, no Nip65Arrived re-route, and no retry on miss. Baseline measurement confirmed 10.2% resolution (108/1054 follows) under indexer-only querying.

## Decision

Migrate both claim_profile and drain_pending_reverify through the InterestRegistry (using claim_event as the reference implementation), inheriting D3 kind:10002 probe, outbox routing, set-cover, and Nip65Arrived re-route. Add a liveness hint (CacheOk=OneShot for feed avatars, Live=Tailing for profile screens) to the FFI signature. Add nprofile relay hints via claim_profile_with_hints. Add probe-epoch retry: clear probed_mailboxes on indexer reconnect so uncached authors get re-probed. Delete the bespoke profile_claim_request pipeline, pending_profile_claim_requests, ProfileRequestState, and relay_lifecycle re-queue.

## Consequences

- Baseline measurement: 10.2% → 50.0% profile resolution (~5×, +420 of 1054 follows) with outbox querying
- Resolution ceiling is ~57.6% (only 608/1054 follows publish NIP-65 kind:10002) — remaining gap requires NIP-42 AUTH or broader relay coverage
- A real web regression was introduced: relay_lifecycle.rs clear_probed_mailboxes + IndexerSetChanged recompile on every indexer connect churns feed subscriptions in the web fixture scenario (3/3 CI failures, 8/8 green on master); fix in progress
- Android FFI call site (claims.rs:45) needs arity update from 4→5 args (liveness=0) atomically with the kernel PR
- nip60 wallet relay pin to purplepag.es (separate from this arc) filed as #1434 for follow-up

## Open Tail

- Web Playwright regression on feed-renders-after-connect test (3/3 failures vs 8/8 green on master) — localized to relay_lifecycle indexer-connect recompile churn, hypothesis handed to investigation agent, fresh fix agent queued if silent
- Merge of kernel PR #1436 gated on resolving the web regression
- iOS PR held until kernel PR merges (FFI signature locked, liveness wired, Swift compiles clean)
- Version cut + consumer app updates + device installs queued behind merge

## Evidence

- transcript lines 1-44
- transcript lines 48-83
- transcript lines 1597-1639
- transcript lines 1815-1850
- transcript lines 1998-2014
