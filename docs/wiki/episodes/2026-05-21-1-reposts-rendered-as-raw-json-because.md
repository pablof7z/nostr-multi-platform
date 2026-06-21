---
type: episode-card
date: 2026-05-21
session: 17ef19cd-8549-4fa9-b09c-5266aaf480a7
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/17ef19cd-8549-4fa9-b09c-5266aaf480a7.jsonl
salience: root-cause
status: active
subjects:
  - timeline-item-kind
  - repost-rendering
  - chirp-feed
supersedes: []
related_claims: []
source_lines:
  - 3-7
  - 694-700
  - 711-783
captured_at: 2026-06-18T04:46:22Z
---

# Episode: Reposts rendered as raw JSON because TimelineItem lacked a kind field

## Prior State

TimelineItem (Rust + Swift) had no `kind` field, so Swift had no way to distinguish a kind:6 repost from a kind:1 note. NoteRowView used an `effectiveContent` heuristic that checked for a `sig` field in the embedded JSON — which many real-world reposts strip. ThreadNoteRow had no heuristic at all, so every repost rendered as raw JSON. Tapping a repost navigated to the wrapper event's thread (which the kernel never expands) instead of the original note's thread.

## Trigger

User reported that reposts on Chirp show up as just events in the timeline instead of showing the reposted event, with screenshot evidence.

## Decision

Add `kind: u32` as a first-class field on `TimelineItem` (Rust `types.rs`/`update.rs` + Swift `KernelBridge.swift`), populated from `event.kind`. Swift now branches on `item.kind == 6` to show a 'Repost' badge + extracted inner text and navigate to the inner note's ID on tap. Deleted the fragile `effectiveContent` heuristic and refactored `ThreadNoteRow` and `ModularBlockView.syntheticItem` to propagate and respect `kind`.

## Consequences

- Reposts now render with a 'Repost' badge and the inner note's text instead of raw JSON
- Tapping a repost navigates to the original note's thread, not the unexpanded wrapper event
- The thin-shell rule is preserved: kind is Rust-authoritative, Swift only decides display
- The old `sig`-based heuristic is deleted; kind:6 detection is deterministic and protocol-correct
- Plain-text rendering of inner content is a D1 best-effort gap — a follow-up could have the kernel emit a `contentTree` for the inner event so reposts render with full entity decoration

## Open Tail

- Kernel could emit a `contentTree` for the inner event of kind:6 reposts so the body renders with entity decoration rather than plain text

## Evidence

- transcript lines 3-7
- transcript lines 694-700
- transcript lines 711-783

