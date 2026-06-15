---
type: episode-card
date: 2026-06-15
session: c9a794f6-6ad7-4ee9-a620-fc342fd495c3
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/c9a794f6-6ad7-4ee9-a620-fc342fd495c3.jsonl
salience: product
status: active
subjects:
  - nostravatar-equatable
  - nmp-registry-user-avatar
  - swiftui-snapshot-invalidation
supersedes:
  - 2026-06-15-3-nostravatar-equatable-fix-registry-ownership-correct
related_claims: []
source_lines:
  - 762-831
  - 864-875
captured_at: 2026-06-15T08:33:24Z
---

# Episode: NostrAvatar Equatable conformance adopted at registry level for all NMP SwiftUI apps

## Prior State

NostrAvatar (both registry and Chirp copies) has no Equatable conformance. SwiftUI re-evaluates its body on every snapshot emission, including when profile data is byte-identical. ChirpColor.avatar(from:) constructs a LinearGradient on each re-evaluation. Fix was initially scoped as Chirp-specific (ios/Chirp/Chirp/Components/NostrUser/NostrAvatar.swift).

## Trigger

Time Profiler trace showed NostrAvatar.body re-evaluation in the 223ms SwiftUI AttributeGraph hotpath (hot stack #2). User corrected that NostrAvatar is a registry component (swiftui/user-avatar), not Chirp-specific — the canonical source is apps/nmp-gallery/ios/NmpGallery/Registry/NostrAvatar.swift.

## Decision

Add Equatable conformance on all rendered inputs (pubkey, url, colorHex, initials, size) to the registry NostrAvatar first, then Chirp's customized copy. Apply .equatable() or EquatableView wrapper at call sites — bare Equatable conformance is ignored by SwiftUI without it.

## Consequences

- All future NMP SwiftUI apps that copy from the registry inherit the fix automatically
- Must verify late picture arrival still repaints for url==nil (host-backed) avatars — the host projection update should re-render the parent, but this coupling must be confirmed
- Equatable must cover all rendered inputs — omitting initials or size would wrongly suppress updates when those fields differ
- Fix directly supports ADR-0055 incremental emission program and doctrine D8 invariant #9 (no high-frequency FFI loops above ~60Hz)

## Open Tail

- Agent dispatched to implement in registry + Chirp copy with PR + merge (line 922-931)

## Evidence

- transcript lines 762-831
- transcript lines 864-875
