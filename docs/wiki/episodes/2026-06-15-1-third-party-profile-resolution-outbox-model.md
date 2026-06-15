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
  - nmp-kernel
  - interest-registry
  - nip-65
supersedes:
  - 2026-06-15-1-third-party-profile-resolution-was-inert
related_claims: []
source_lines:
  - 1-4
  - 59-84
  - 2003-2012
  - 2195-2253
captured_at: 2026-06-15T01:58:12Z
---

# Episode: Third-party profile resolution: outbox model inert, migrated to registry chokepoint

## Prior State

The outbox model machinery existed (NIP-65 write relay routing via MailboxCache, GenericOutboxRouter) but was inert for third-party profile resolution: kind:10002 was only fetched for the self/active account at startup, so MailboxCache had no entry for any other pubkey, and kind:0 queries fell through to indexer relays only. claim_profile was a bespoke path bypassing the InterestRegistry chokepoint, with fire-once-no-retry on 10002 probes. Purplepag.es (one of two indexers) is AUTH-walled, returning 0 kind:0 anonymously, meaning the indexer-only path effectively depended on primal.net alone.

## Trigger

User reported ~50% of pubkeys never resolve in Chirp iOS; multi-agent investigation traced the full path from UI claim through kernel relay subscription to relay network, confirming outbox routing was dead for third-party profiles.

## Decision

Migrate claim_profile and drain_pending_reverify onto the InterestRegistry chokepoint, inheriting intrinsic D3 kind:10002 probe, outbox routing, set-cover, and Nip65Arrived re-route. Introduce a liveness hint (CacheOk→OneShot for feed avatars, Live→Tailing for profile screens) via FFI. Add nprofile relay hints and probe-epoch retry-on-miss on indexer reconnect.

## Consequences

- ~5× profile resolution improvement measured: 10.2% → 50.0% on a follow set of 1054 users (+420 profiles)
- Deleted bespoke profile_claim_request, ProfileRequestState/profile_requests, refresh_profile_after_mailbox, and relay_lifecycle re-queue — registry is now sole path
- FFI expanded 4→5 args (liveness param); all in-repo Rust/Android callers updated atomically
- Resolution ceiling is ~57.6% (NIP-65 adoption rate in follow set); non-NIP-65 users still depend on indexers
- purplepag.es AUTH-wall means the old indexer-only path relied on primal.net alone — far worse than assumed
- Web feed regression discovered: clear_probed_mailboxes + forced recompile on every indexer connect churns feed subscriptions during initial load (3/3 CI failures vs 8/8 green on master), localized to relay_lifecycle.rs — fix in progress
- iOS wired with liveness: feed avatars→CacheOk, profile screen→Live; registry/gallery left at 2-arg as separable follow-up

## Open Tail

- Web feed regression fix pending — localized to relay_connected_url indexer-connect handler, fresh fix agent dispatched
- Propagating ProfileLiveness into registry/gallery source-of-truth (currently only Chirp)
- nip60 follow-up filed as #1434

## Evidence

- transcript lines 1-4
- transcript lines 59-84
- transcript lines 2003-2012
- transcript lines 2195-2253
