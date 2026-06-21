---
type: episode-card
date: 2026-05-25
session: e7a1d168-3c58-4438-a544-aa645850c388
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/e7a1d168-3c58-4438-a544-aa645850c388.jsonl
salience: product
status: active
subjects:
  - identicon
  - user-avatar
  - cross-platform-parity
supersedes: []
related_claims: []
source_lines:
  - 1102-1112
captured_at: 2026-06-18T05:40:09Z
---

# Episode: Identicon cross-platform visual parity is broken

## Prior State

Registry user-avatar implementations (SwiftUI: palette+initials NostrIdenticonBox, Compose: 6-color palette with first-2-char initials) were assumed to provide cross-platform pixel parity for avatar rendering.

## Trigger

Audit of the gallery Android app revealed it uses a completely different 5×5 symmetric block canvas identicon algorithm (Identicon.kt), described as a 'byte-for-byte port' of the iOS gallery version. The registry identicon and the gallery identicon produce visually different outputs from the same input pubkey.

## Decision

Must unify all platforms on the 5×5 symmetric algorithm. Gallery Identicon.kt becomes the reference implementation and must be ported into registry SwiftUI and Compose user-avatar components.

## Consequences

- All platforms will need to update identicon rendering to match the 5×5 algorithm
- Gallery's Identicon.kt and MentionChip.kt are registry-quality and should be upstreamed or adopted into the main app
- Existing palette+initials registry identicons become historical/replaced

## Open Tail

- Whether to support both identicon variants (palette vs 5×5) as configuration, or hard-swap to 5×5 only
- Impact on existing apps that rely on the current palette identicon appearance

## Evidence

- transcript lines 1102-1112

