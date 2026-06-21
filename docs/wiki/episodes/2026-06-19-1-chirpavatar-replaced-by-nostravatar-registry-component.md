---
type: episode-card
date: 2026-06-19
session: 835e3f03-658e-4150-a31a-cd4986ab5308
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/835e3f03-658e-4150-a31a-cd4986ab5308.jsonl
salience: reversal
status: active
subjects:
  - chirp-avatar
  - nostr-avatar
  - thread-ui
supersedes: []
related_claims: []
source_lines:
  - 3-3
  - 5-27
  - 25-36
  - 29-38
captured_at: 2026-06-19T12:33:13Z
---

# Episode: ChirpAvatar replaced by NostrAvatar registry component

## Prior State

Chirp iOS was believed to use a custom `ChirpAvatar` component (initials + hex color only) for avatars across the app, including thread replies.

## Trigger

Code investigation revealed all thread reply views (ThreadNoteRow, ModularBlockView) actually use `NostrAvatar`, the registry `swiftui/user-avatar` component — the stored memory about ChirpAvatar was stale.

## Decision

`ChirpAvatar` is fully retired; `NostrAvatar` (the registry component) is the canonical avatar implementation everywhere, including thread replies. NostrAvatar provides brand gradient + initials fallback (not hex-color-only) and uses `.cacheOk` liveness mode.

## Consequences

- Stored memory/wiki referencing ChirpAvatar is stale and must be updated
- Avatar behavior in thread replies is now consistent with the rest of the app via the registry component
- Fallback semantics changed from hex-color-only initials to brand gradient + initials

## Open Tail

- Wiki update for ChirpAvatar→NostrAvatar migration status

## Evidence

- transcript lines 3-3
- transcript lines 5-27
- transcript lines 25-36
- transcript lines 29-38

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-19-1-chirpavatar-replaced-by-nostravatar-registry-component.json`](transcripts/2026-06-19-1-chirpavatar-replaced-by-nostravatar-registry-component.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-19-1-chirpavatar-replaced-by-nostravatar-registry-component.json`](transcripts/raw/2026-06-19-1-chirpavatar-replaced-by-nostravatar-registry-component.json)
