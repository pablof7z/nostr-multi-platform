---
type: episode-card
date: 2026-05-25
session: c8c2902c-43a6-4b1c-8215-1732dc266895
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/c8c2902c-43a6-4b1c-8215-1732dc266895.jsonl
salience: root-cause
status: superseded
subjects:
  - gallery-model
  - kernel-snapshot
  - profile-data-flow
supersedes:
  - 2026-05-25-1-gallery-profile-resolution-claim-profile-open
related_claims: []
source_lines:
  - 987-1197
captured_at: 2026-06-18T05:33:14Z
---

# Episode: Profile data flows via open_author + projections.author_view, not claim_profile + snapshot.profiles

## Prior State

GalleryModel.kt called claimProfile(pubkey, consumerId) and attempted to decode profile data from snapshot.profiles — a field that does not exist in the kernel's KernelSnapshot JSON output

## Trigger

Profile view showed 'Loading profile…' indefinitely; investigating the iOS GalleryModel.swift and nmp-core kernel types revealed that profiles are delivered exclusively via projections.author_view.profile (populated only after nmp_app_open_author is called), not via a top-level profiles map

## Decision

Rewrote GalleryModel.kt to (1) call bridge.openAuthor(DEMO_PUBKEY) instead of claimProfile, (2) decode from projections.author_view.profile as ProfileCard and projections.mention_profiles as secondary source, (3) use safe as? JsonObject casts instead of .jsonObject (which throws on JsonNull), and (4) synthesize npub_short client-side since the kernel's ProfileCard omits it

## Consequences

- Android gallery now correctly displays live user profile data (avatar, name, nip05, npub, card)
- ProfileWire.npub and npubShort made optional with defaults to tolerate kernel's ProfileCard lacking npub_short
- The openAuthor JNI method was added to both android.rs and KernelBridge.kt
- Android and iOS gallery apps now follow the same kernel data contract

## Open Tail

*(none)*

## Evidence

- transcript lines 987-1197

