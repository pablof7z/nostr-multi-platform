---
type: episode-card
date: 2026-06-15
session: c9a794f6-6ad7-4ee9-a620-fc342fd495c3
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/c9a794f6-6ad7-4ee9-a620-fc342fd495c3.jsonl
salience: product
status: superseded
subjects:
  - nostr-avatar
  - nmp-gallery-registry
  - swiftui-user-avatar
supersedes: []
related_claims: []
source_lines:
  - 762-831
  - 864-876
captured_at: 2026-06-15T08:00:37Z
---

# Episode: NostrAvatar equatable fix: registry ownership + correct conformance scope

## Prior State

NostrAvatar fix was thought to be Chirp-specific; proposed Equatable conformance on (pubkey, url, colorHex) only.

## Trigger

User correction that avatar component comes from NMP UI registry; Opus review confirmed the fix belongs in the registry canonical source (apps/nmp-gallery/ios/NmpGallery/Registry/NostrAvatar.swift) and found the Equatable field list was incomplete.

## Decision

Fix lands in registry NostrAvatar first (then Chirp's customized copy). Equatable conformance must include ALL rendered inputs: pubkey, url, colorHex, initials, size. Must apply .equatable() or EquatableView wrapper at call sites — bare conformance is ignored by SwiftUI.

## Consequences

- All future NMP SwiftUI apps that copy from the registry inherit the fix
- @State properties (generatedConsumerID, claimedPubkey) are correctly excluded from Equatable — SwiftUI persists them by view identity, not by comparison
- Must verify late picture arrival still repaints for url==nil host-backed avatars — the environment read via profileHost could bypass Equatable if not confirmed

## Open Tail

- Confirm that a late profile arrival that doesn't change any stored input field still triggers re-render through the parent/host invalidation path

## Evidence

- transcript lines 762-831
- transcript lines 864-876
