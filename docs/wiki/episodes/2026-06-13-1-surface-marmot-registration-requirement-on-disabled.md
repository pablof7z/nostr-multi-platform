---
type: episode-card
date: 2026-06-13
session: b925f8c0-91f1-4d90-90d6-4a362bbaee79
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/b925f8c0-91f1-4d90-90d6-4a362bbaee79.jsonl
salience: product
status: active
subjects:
  - marmot-create-group-button
  - key-package-subtitle
supersedes: []
related_claims: []
source_lines:
  - 1-850
captured_at: 2026-06-13T21:31:08Z
---

# Episode: Surface Marmot registration requirement on disabled Create Group button

## Prior State

The Create Group button for private (MLS/Marmot) groups was permanently disabled with no explanation when the user was signed in via bunker/NIP-46 (no local nsec). The Swift MarmotKeyPackage.empty.subtitle was blank (""), so no hint existed to display.

## Trigger

User reported the button was always disabled. Root-cause: isRegistered is false for bunker users (no local nsec → Marmot never registers), and the .empty fallback subtitle was empty — no user-facing reason was ever surfaced.

## Decision

Surface the Rust-owned registration-requirement subtitle on the disabled button. Changed MarmotKeyPackage.empty.subtitle from "" to "Sign in with an nsec to enable" (mirroring Rust SUBTITLE_NOT_REGISTERED), and added a caption line in NewGroupSheet below the disabled Create Group button that displays the key-package subtitle when !model.marmot.isRegistered.

## Consequences

- Bunker/NIP-46 users now see "Sign in with an nsec to enable" instead of a silently disabled button
- NIP-29 public group button remains correctly gated on relay URL + group ID fields being filled
- The .empty subtitle is only used as the fallback when no Marmot projection exists; real registered users continue to receive the Rust-generated published/stale/age subtitle via the typed FlatBuffers path

## Open Tail

*(none)*

## Evidence

- transcript lines 1-850

