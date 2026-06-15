---
type: episode-card
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: product
status: superseded
subjects:
  - chirp-ios-claim-coverage
  - profile-liveness-semantics
supersedes:
  - 2026-06-14-2-ios-ui-only-claimed-profiles-for
related_claims: []
source_lines:
  - 1769-1801
captured_at: 2026-06-15T01:10:04Z
---

# Episode: iOS UI missing profile claims for mentions/attributions/reactions

## Prior State

iOS UI components only called claim_profile for a subset of displayed pubkeys (feed avatars, profile screen). Mention authors, reply attribution authors, reaction/repost authors, and name references never triggered a profile fetch, so their pubkeys displayed as unresolved even if the kernel could resolve them.

## Trigger

Investigation of the ~50% unresolved-profile symptom revealed that the iOS UI layer was not claiming profiles for all visible pubkey surfaces, compounding the kernel routing problem.

## Decision

Wire claim_profile calls for all display surfaces with liveness semantics: NostrAvatar.swift (feed avatar) → .cacheOk, ProfileView.swift (profile screen) → .live, NostrProfileName.swift (self-claim) → .cacheOk, NoteContentView.swift (mentions) → .cacheOk, HomeFeedView.swift ReplyAttributionLine → .cacheOk. Add ProfileLiveness enum to KernelBridge with .cacheOk=0 / .live=1. Protocol seam (NostrProfileHost) defaults to .cacheOk so registry leaves stay 2-arg-clean.

## Consequences

- All visible pubkeys now trigger profile resolution at the UI level
- Feed avatars get one-shot fetch (CacheOk); profile screen gets tailing subscription (Live) for reactive edits
- NmpCore.h FFI header updated with 5th liveness parameter
- iOS PR compiles clean with zero Swift errors, held for merge until kernel PR lands
- Android FFI (claims.rs:45) needs 4→5 arg update atomically with kernel PR to avoid workspace build break

## Open Tail

- iOS PR held until kernel PR #1436 merges and new NMP version is cut
- Registry/gallery components left at 2-arg claim (no liveness propagation yet) — separable follow-up

## Evidence

- transcript lines 1769-1801
