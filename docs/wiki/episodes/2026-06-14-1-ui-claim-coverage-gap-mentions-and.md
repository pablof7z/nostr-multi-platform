---
type: episode-card
date: 2026-06-14
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: root-cause
status: superseded
subjects:
  - chirp-ios-claim-coverage
  - nostr-profile-resolution
supersedes:
  - 2026-06-14-2-ui-claim-avatar-coupling-only-nostravatar
related_claims: []
source_lines:
  - 627-669
captured_at: 2026-06-14T21:37:23Z
---

# Episode: UI claim-coverage gap: mentions and reply-attributions never claimed profiles

## Prior State

Profile mentions inside note content (NoteContentView/NoteEntityViews), reply-attribution lines in HomeFeedView ('↳ name replied in thread'), and standalone NostrProfileName were render-only — they displayed author pubkeys but never called nmp_app_claim_profile, so the kernel never fetched those profiles regardless of relay configuration

## Trigger

Systematic audit of all author-displaying UI surfaces found three that displayed a pubkey but never claimed it; only NostrAvatar and ProfileView claimed correctly

## Decision

Add claim/release lifecycle at all missing surfaces: NostrProfileName self-claims (ported from nmp-gallery pattern, updated in registry source + web vendor mirror), NoteContentView claims all mention pubkeys via syncMentionClaims with refcount-balanced .onDisappear release, HomeFeedView ReplyAttributionLine self-claims attribution.authorPubkey

## Consequences

- All displayed author names/avatars now trigger kernel profile resolution — likely the single largest contributor to the ~50% raw-npub symptom
- No new FFI needed — thin-shell preserved, all resolution stays kernel-side
- Registry source + published web vendor mirror updated so changes survive future vendor syncs
- New test: ProfileClaimSurfaceTests.swift for mention-pubkey collection + claim/release refcount balance

## Open Tail

- PR held until kernel M2 design is approved so both land coherently

## Evidence

- transcript lines 627-669
