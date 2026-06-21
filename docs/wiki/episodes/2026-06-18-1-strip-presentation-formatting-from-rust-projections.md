---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: architecture
status: superseded
subjects:
  - p1-presentation-formatting
  - nmp-core-projections
  - aim-md-d2
supersedes:
  - 2026-06-18-5-projection-formatting-moves-to-shells-d1
related_claims: []
source_lines:
  - 25-33
  - 453-462
  - 476-477
  - 519-530
  - 636-637
  - 959-972
captured_at: 2026-06-18T20:25:04Z
---

# Episode: Strip presentation formatting from Rust projections

## Prior State

Platform-neutral Rust projections computed display-oriented formatting (SF Symbol names like "person.fill", bucket_age/render_subtitle, display_name/initials/subtitle, npub) and exposed them through FFI to native shells. The in-code comments cited a bogus "§4.4/V-24" justification.

## Trigger

Issue #1493 P1 audit found that aim.md §2 "Anti-patterns" explicitly forbids presentation formatting in projection builders, snapshot types, and FFI paths — naming short_npub, avatar_initials, format_ago_secs as legitimate ONLY in TUI/CLI/tests. All five P1 findings were confirmed as real §2 violations.

## Decision

Remove all presentation formatting from Rust projections. Projections now emit raw data (published timestamp, age_secs, is_registered:bool, name/group_id/member_count, raw kind discriminants). Shells (iOS/Android) own display formatting via computed properties and helpers. Semantic-state tokens (*_tone) are retained — they enumerate state, not format prose. Three of five slices merged; two (publish_outbox SF-symbols, relay_diagnostics) held behind PR #1525.

## Consequences

- iOS uses computed-property extensions (no view changes needed); Android uses shared internal helpers (keyPackageSubtitle, bucketAge promoted from private to internal for cross-file reuse)
- Every slice required golden wire-fixture regeneration across Rust/Swift/Kotlin (triplicated hex copies) — discovered 6 missed reader sites including a production iOS TypedProjectionGlue path that would not have compiled
- The bogus §4.4 citation is void; §2 is now enforced as the authoritative rule
- SF Symbol names ("person.fill", "heart") must move out of nmp-core — blocked on PR #1525

## Open Tail

- Two remaining slices (publish_outbox SF-symbols/titles/labels, relay_diagnostics short_url/title_case/format_bytes) held behind PR #1525 which is still OPEN/UNSTABLE
- Codex-review artifacts were not saved for the three merged sub-agent PRs — post-hoc verification pass ordered, no doc-commits

## Evidence

- transcript lines 25-33
- transcript lines 453-462
- transcript lines 476-477
- transcript lines 519-530
- transcript lines 636-637
- transcript lines 959-972

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-1-strip-presentation-formatting-from-rust-projections.json`](transcripts/2026-06-18-1-strip-presentation-formatting-from-rust-projections.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-1-strip-presentation-formatting-from-rust-projections.json`](transcripts/raw/2026-06-18-1-strip-presentation-formatting-from-rust-projections.json)
