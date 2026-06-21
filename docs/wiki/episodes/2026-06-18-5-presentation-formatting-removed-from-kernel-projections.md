---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: product
status: superseded
subjects:
  - p1-presentation-formatting
  - sf-symbols-in-kernel
  - flat-projections
supersedes:
  - 2026-06-18-4-presentation-formatting-moved-from-kernel-to
related_claims: []
source_lines:
  - 25-26
  - 959-971
  - 1148-1158
captured_at: 2026-06-18T21:31:23Z
---

# Episode: Presentation formatting removed from kernel projections

## Prior State

SF Symbol names ('person.fill', 'heart'), display labels (display_name, initials, subtitle), and other presentation logic were baked into Rust FlatBuffers projections in platform-neutral nmp-core.

## Trigger

#1493 P1 finding: presentation formatting in Rust projections is the worst D1 violation. Owner confirmed SF Symbols and display logic belong in shells, not NMP.

## Decision

Projections now emit raw domain data (published/age_secs/stale for KeyPackageStatus; name/group_id/public/open/member_count for DiscoveredGroup; etc.). Shells compute display (iOS computed-property extensions, Android shared helpers). AuthorDisplay.npub and Nip10ReplyAttribution flat mirrors removed; shells read nested authorDisplay + nmp_app_encode_profile. relay_diagnostics uses *_tone semantic tokens.

## Consequences

- Every P1 slice required golden fixture regeneration (triplicated: Rust .fb.hex + Kotlin POPULATED_HEX + Swift populatedHex) + actual shell compilation — CI cargo test does NOT compile apps/* crate tests, so wire-shape breaks merge green (real E0560 in nmp-app-chirp caught by codex post-hoc, fixed via #1551).
- CI gap filed as #1553: compile apps/* crate tests to prevent future silent wire-shape regressions.
- Remaining 2 slices (publish_outbox SF-symbols + relay_diagnostics) blocked on #1525, now unblocked.
- Codex post-hoc review gate enforced going forward (codex-review-before-PR + artifact saved).

## Open Tail

- 2 remaining P1 slices (publish_outbox SF-symbols — the headline fix — + relay_diagnostics) in progress.

## Evidence

- transcript lines 25-26
- transcript lines 959-971
- transcript lines 1148-1158

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-5-presentation-formatting-removed-from-kernel-projections.json`](transcripts/2026-06-18-5-presentation-formatting-removed-from-kernel-projections.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-5-presentation-formatting-removed-from-kernel-projections.json`](transcripts/raw/2026-06-18-5-presentation-formatting-removed-from-kernel-projections.json)
