---
type: episode-card
date: 2026-06-15
session: c9a794f6-6ad7-4ee9-a620-fc342fd495c3
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/c9a794f6-6ad7-4ee9-a620-fc342fd495c3.jsonl
salience: architecture
status: active
subjects:
  - nostr-avatar-registry
  - nmp-ui-component-ownership
  - swiftui-user-avatar
supersedes: []
related_claims: []
source_lines:
  - 762-831
  - 864-876
captured_at: 2026-06-15T08:45:33Z
---

# Episode: NostrAvatar Equatable fix belongs in registry, not Chirp shell

## Prior State

NostrAvatar was assumed to be a Chirp-specific SwiftUI component; the Equatable fix would only benefit Chirp.

## Trigger

User correction pointing out NostrAvatar is an NMP UI registry component; source code confirms docstring 'Registry component swiftui/user-avatar' and canonical copy exists at apps/nmp-gallery/ios/NmpGallery/Registry/NostrAvatar.swift.

## Decision

The authoritative source is the registry NostrAvatar. Fix lands in registry first, then Chirp's customized copy inherits it. All NMP SwiftUI apps that copy from the registry benefit.

## Consequences

- Must conform Equatable on all rendered inputs — include initials and size, not just (pubkey, url, colorHex)
- Must apply .equatable() or EquatableView wrapper at call sites — bare Equatable conformance is ignored by SwiftUI without it
- Must verify late picture arrival still repaints for url==nil (host-backed) avatars — the one correctness risk
- PR opened (feat/nostr-avatar-equatable) with CI passing, pending merge

## Open Tail

- Merge of PR 1441 still pending final CI green
- Other NMP SwiftUI apps should be audited for the same un-Equatable leaf-view pattern

## Evidence

- transcript lines 762-831
- transcript lines 864-876
