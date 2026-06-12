---
type: episode-card
date: 2026-06-03
session: 7f143c67-6e46-424a-90a8-5bf844947fee
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/7f143c67-6e46-424a-90a8-5bf844947fee.jsonl
salience: architecture
status: active
subjects:
  - d5-d8-doctrine
  - shell-business-logic
  - nip-10
supersedes: []
related_claims: []
source_lines:
  - 944-989
  - 1005-1057
captured_at: 2026-06-11T22:58:15Z
---

# Episode: Shell-side NIP-10 construction prohibited by D5/D8 doctrine

## Prior State

The correction attempt instructed iOS and Android agents to build full NIP-10 tags host-side from parent `NoteRecord` data in the snapshot projection.

## Trigger

Both platform agents independently discovered that `Nip10Refs` (root + mentioned_pubkeys) does not exist on the snapshot projection — `TimelineItem` has no reply-structural fields. The Android agent further identified that reimplementing `nmp-nip01::Note::reply_to` in Kotlin violates D5/D8 ("No Kotlin-side business logic or derived state").

## Decision

Protocol logic (NIP-10 tag construction) must never be reimplemented in shell code. The correct fix is either (a) kernel resolves parent and builds tags internally via a new action variant, or (b) Rust pre-computes `reply_tags` and includes them in the projection for shells to forward verbatim. Shells pass data, never construct protocol logic.

## Consequences

- iOS and Android remain on minimal reply markers until the projection layer gains pre-computed reply tags
- Rust apps can use `Note::reply_to` directly (same-crate access to `NoteRecord`)
- The `NoteRecord` → `EventRecord` rename must precede or accompany the projection change

## Open Tail

- Decide between kernel-internal tag resolution (new `PublishReply` variant) vs projection-carried `reply_tags` field

## Evidence

- transcript lines 944-989
- transcript lines 1005-1057

