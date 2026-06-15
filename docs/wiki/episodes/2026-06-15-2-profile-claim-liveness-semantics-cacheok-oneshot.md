---
type: episode-card
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: product
status: superseded
subjects:
  - liveness-parameter
  - claim-profile-api
  - profile-refresh
supersedes: []
related_claims: []
source_lines:
  - 1769-1800
  - 1822-1830
captured_at: 2026-06-15T00:22:07Z
---

# Episode: Profile claim liveness semantics — CacheOk (OneShot) vs Live (Tailing)

## Prior State

No distinction existed between how different UI contexts requested profile data — all claims were fire-once with no reactive update capability. Feed avatars and profile screens used the same claim path with identical semantics.

## Trigger

The migration to registry-based claims needed to handle different UX requirements: feed avatars don't need reactive subscription updates (wasteful bandwidth), but the profile screen should reactively update when kind:0 edits arrive.

## Decision

Added a liveness parameter to the claim_profile FFI: liveness=0 (CacheOk) registers a OneShot interest (no live sub, used for feed avatars and mentions), liveness=nonzero (Live) registers a Tailing interest (reactive kind:0 sub, used for profile screen). Mixed claims on the same pubkey resolve to Tailing via set_sub upgrade. The 4-arg FFI signature became 5-arg (void nmp_app_claim_profile(app, pubkey, consumer_id, force, liveness)). iOS call sites wired explicitly: NostrAvatar.swift → .cacheOk, ProfileView.swift → .live, NoteContentView mentions → .cacheOk, HomeFeedView ReplyAttributionLine → .cacheOk, NostrProfileName self-claim → .cacheOk.

## Consequences

- Profile screen now gets reactive updates when a user edits their kind:0; feed avatars remain cache-served without maintaining live subscriptions
- Android FFI call site (claims.rs:45) updated atomically in kernel PR to pass liveness=0
- Registry/gallery apps deliberately NOT migrated in this PR — left at 2-arg internally consistent defaults, separable follow-up
- Protocol extension: ProfileLiveness enum added to KernelBridge.swift with convenience default (.cacheOk) so existing call sites stay 2-arg-clean via protocol extension

## Open Tail

- Propagating ProfileLiveness into the registry source-of-truth (user-avatar protocol, web vendor, published JSON, gallery) is a clean separable follow-up not yet done

## Evidence

- transcript lines 1769-1800
- transcript lines 1822-1830
