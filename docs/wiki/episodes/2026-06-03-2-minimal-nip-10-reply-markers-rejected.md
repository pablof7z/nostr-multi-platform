---
type: episode-card
date: 2026-06-03
session: 7f143c67-6e46-424a-90a8-5bf844947fee
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/7f143c67-6e46-424a-90a8-5bf844947fee.jsonl
salience: product
status: active
subjects:
  - nip-10
  - reply-tags
  - thread-structure
supersedes: []
related_claims: []
source_lines:
  - 880-886
  - 944-989
captured_at: 2026-06-11T22:58:15Z
---

# Episode: Minimal NIP-10 reply markers rejected as unauthorized technical debt

## Prior State

All three migration agents shipped `PublishRaw` with a minimal reply marker `["e", id, "", "reply"]` — missing root forwarding and p re-notification.

## Trigger

User rejected the "for now" framing: replies deeper than one level break thread structure without root forwarding, and parent authors get no notification without p tags. The agents had everything needed to do it correctly (`Note::reply_to` in Rust, parent event in snapshot).

## Decision

Full NIP-10 tag construction is required, not optional. Root forwarding and p re-notification must be included from the start.

## Consequences

- Rust apps (#918) corrected to route through `nmp_nip01::Note::reply_to(&NoteRecord)` which builds the complete tag set
- iOS (#919) and Android (#917) cannot produce full NIP-10 tags because `TimelineItem` lacks `Nip10Refs` fields — the data is not on the projection surface
- Minimal reply markers in iOS/Android are explicitly documented as incomplete, not acceptable final state

## Open Tail

- iOS and Android need `reply_tags: Vec<Vec<String>>` (or equivalent) added to the snapshot projection before full NIP-10 can work

## Evidence

- transcript lines 880-886
- transcript lines 944-989

