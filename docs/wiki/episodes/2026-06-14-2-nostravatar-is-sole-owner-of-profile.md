---
type: episode-card
date: 2026-06-14
session: 286c6f24-af4b-4e59-b72f-ed72e8b9d781
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/286c6f24-af4b-4e59-b72f-ed72e8b9d781.jsonl
salience: architecture
status: superseded
subjects:
  - nostr-avatar-claim-ownership
  - chirp-profile-resolution
supersedes: []
related_claims: []
source_lines:
  - 2219-2256
captured_at: 2026-06-14T22:17:11Z
---

# Episode: NostrAvatar is sole owner of profile claiming; row-level claims are redundant

## Prior State

Row views (NoteRowView, ProfileNoteRow, ThreadNoteRow) added their own claimProfile onAppear/onDisappear calls and read authorPictureUrl from the model, attempting to drive profile resolution at the row level. NostrAvatar also independently claimed profiles via the nostrProfileHost environment.

## Trigger

Architectural review during this session found that ChirpApp sets .environment(\.nostrProfileHost, model) globally (line 26), and NostrAvatar already claims the profile AND resolves the picture URL internally. Row-level claims are deduped by the kernel but add no value.

## Decision

Remove redundant claimProfile/authorPictureUrl code from NoteRowView, ProfileNoteRow, and ThreadNoteRow. NostrAvatar is the single owner of profile claiming; row views only need to render what the avatar already resolves.

## Consequences

- Profile-claim ownership is now unambiguous: NostrAvatar via nostrProfileHost environment
- Removed duplicate kernel requests that were being silently deduped anyway
- Any future profile-resolution features must be added at the NostrAvatar/NostrProfileHost layer, not at individual row views

## Open Tail

*(none)*

## Evidence

- transcript lines 2219-2256
