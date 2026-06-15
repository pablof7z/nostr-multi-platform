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
  - nip-65
  - interest-registry
  - claim-profile
  - liveness-hint
supersedes:
  - 2026-06-15-1-third-party-profile-resolution-outbox-model
related_claims: []
source_lines:
  - 1-5
  - 48-95
  - 1698-1712
  - 2003-2014
  - 2198-2287
captured_at: 2026-06-15T02:23:32Z
---

# Episode: Third-party profile resolution migrated from indexer-only to outbox model via InterestRegistry

## Prior State

~50% of user pubkeys never resolved because: (1) claim_profile bypassed the InterestRegistry and queried only RelayRole::Indexer relays; (2) the outbox model (NIP-65 kind:10002 discovery) existed but was inert for third-party profiles since kind:10002 was only fetched for the active/self account at startup; (3) the kind:10002 probe was fire-once with no retry on failure or indexer reconnect; (4) mentions/attributions in the feed never called claim_profile at all. Proactive profile fetch at ingest time had been deliberately removed (F-CR-00), so kind:0 was only fetched on a UI component claim.

## Trigger

User reported ~50% of pubkeys don't resolve and demanded root-cause investigation across the stack (Chirp iOS, NMP UI components, NMP kernel), explicitly asking whether only purplepag.es was queried and whether app relays were used.

## Decision

Migrated claim_profile/release_profile onto the InterestRegistry chokepoint, inheriting outbox routing, kind:10002 probe, Nip65Arrived re-route, and set-cover. Added liveness hint (CacheOk→OneShot for feed avatars, Live→Tailing for profile screen) as a 5th FFI arg. Added probe-epoch retry on indexer reconnect. Migrated drain_pending_reverify to OneshotApi. Extended UI claim coverage to mentions/attributions. Deleted bespoke profile_requests state and relay_lifecycle re-queue.

## Consequences

- Profile resolution improved from 10.2% to 50.0% (~5×) in live measurement of 1054 follows
- purplepag.es (one of two indexers) is AUTH-walled — returns 0 kind:0 anonymously — so the entire pre-fix baseline came from primal.net alone
- Cold-start kind:0 queries now route to app/content relays (owner decision #1), not just indexers
- The kind:10002 probe routes to indexer relays; kind:0 routes to the author's own write relays via outbox
- FFI signature changed from 4-arg to 5-arg (liveness), requiring atomic update across nmp-ffi, nmp-android-ffi, nmp-wasm, and iOS KernelBridge
- A regression in the PR — clear_probed_mailboxes + forced recompile on every indexer connect in relay_lifecycle.rs — broke the web Playwright E2E feed test (3/3 failures, 8/8 green on master)
- Warm-reclaim zero-REQ preserved: a CacheOk claim of a resident profile registers no network interest

## Open Tail

- The relay-connect churn regression (clear_probed_mailboxes on every indexer connect) needs a fix that gates to genuine reconnects/new-indexers while preserving retry-on-miss intent
- iOS and Android FFI wiring PRs held pending kernel PR #1436 merge
- nprofile hints implemented via claim_profile_with_hints but not yet surfaced through all UI paths
- ProfileLiveness propagation into registry/gallery source-of-truth is a separable follow-up not yet done
- The measurement ceiling is capped by how many follows publish a NIP-65 list (608/1054 resolved 528)

## Evidence

- transcript lines 1-5
- transcript lines 48-95
- transcript lines 1698-1712
- transcript lines 2003-2014
- transcript lines 2198-2287
