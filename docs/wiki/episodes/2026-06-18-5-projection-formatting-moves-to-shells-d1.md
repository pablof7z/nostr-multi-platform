---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: architecture
status: superseded
subjects:
  - nmp-core
  - nmp-nip01
  - nmp-marmot
  - nmp-nip29
  - projection-formatting
supersedes:
  - 2026-06-18-1-aim-md-2-confirms-presentation-formatting
related_claims: []
source_lines:
  - 25-26
  - 519-531
  - 672-700
captured_at: 2026-06-18T20:12:30Z
---

# Episode: Projection formatting moves to shells (D1 doctrine enforcement)

## Prior State

Platform-neutral nmp-core projections contained presentation formatting: SF Symbol names (person.fill, heart), display-name/npub mirrors, age formatters, emoji/pluralization/initials, and pre-computed labels. In-code comments claimed these were justified by '§4.4/V-24' (bogus — §4.4 is NIP-65 routing). aim.md §2 explicitly forbids formatting in projections, snapshots, and FFI paths.

## Trigger

Issue #1493 audit (P1) identified 5 violations of the D1 doctrine. aim.md §2 confirmed they were genuine violations; the in-code justification citations were stale/bogus.

## Decision

Projections emit raw semantic tokens and data only (kind, timestamp, count, is_registered booleans, tone discriminants like active/warning/error); all human-facing formatting (strings, colors, SF Symbols, npub truncation, age strings, emoji) moves to shell renderers. The *_tone fields are retained as raw semantic tokens (not formatting).

## Consequences

- Nip01Attribution flat mirrors (author_display_name, author_picture_url) and AuthorDisplay.npub removed; shells use nested authorDisplay + nmp_app_encode_profile.
- KeyPackageStatus emits raw published/age_secs/stale + is_registered:bool; removed bucket_age/render_subtitle/action_label.
- DiscoveredGroup emits raw name/group_id/public/open/member_count; removed display_name/initials/subtitle.
- publish_outbox and relay_diagnostics (the most damning findings — SF Symbols in kernel) are HELD behind PR #1525 (snapshot-projector refactor) due to hard type collisions.
- Golden fixture discipline hardened: every projection removal requires regenerating test fixtures in all three native shells.

## Open Tail

- publish_outbox (SF Symbols, titles, status labels) and relay_diagnostics (short_url, title_case, format_bytes) are blocked on #1525 merging.

## Evidence

- transcript lines 25-26
- transcript lines 519-531
- transcript lines 672-700

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-5-projection-formatting-moves-to-shells-d1.json`](transcripts/2026-06-18-5-projection-formatting-moves-to-shells-d1.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-5-projection-formatting-moves-to-shells-d1.json`](transcripts/raw/2026-06-18-5-projection-formatting-moves-to-shells-d1.json)
