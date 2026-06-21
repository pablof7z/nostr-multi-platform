---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: architecture
status: superseded
subjects:
  - sf-symbols
  - nmp-core
  - presentation-formatting
  - d1-doctrine
supersedes:
  - 2026-06-18-1-strip-presentation-formatting-from-rust-projections
related_claims: []
source_lines:
  - 25-26
  - 959-970
  - 1149-1158
captured_at: 2026-06-18T21:02:14Z
---

# Episode: Presentation formatting moved from kernel to shells (D1 enforcement)

## Prior State

SF Symbol names ('person.fill', 'heart'), display labels (subtitle, actionLabel, ageDisplay), and formatting logic were baked into platform-neutral nmp-core — a D1 doctrine violation. Kernel projections carried rendered strings instead of raw data.

## Trigger

P1 audit finding: SF Symbol names inside platform-neutral nmp-core identified as the worst D1 instance. User directive to fix P1.

## Decision

Kernel now emits raw data fields (published/age_secs/is_registered for marmot; name/group_id/public/open/member_count for nip29; nested authorDisplay for nip01). Shells own rendering via computed-property extensions (iOS) and shared helpers (Android keyPackageSubtitle/bucketAge promoted from private to internal). Three slices merged: nip01 attribution (#1528), marmot subtitle (#1536), nip29 discovered groups (#1537). Two remaining (publish_outbox SF-symbols + relay_diagnostics) unblocked after #1525 merged.

## Consequences

- Golden wire fixtures must be regenerated across triplicated copies (Rust .fb.hex, Kotlin POPULATED_HEX, Swift populatedHex)
- Both shell compilations (iOS + Android) must be verified — CI's cargo test does NOT compile apps/* crate tests
- Codex post-hoc review caught a real E0560 in nmp-app-chirp that CI missed, prompting #1553
- Every future wire-shape change risks the same CI blind spot until apps/* test compilation is added to CI

## Open Tail

- publish_outbox (SF-symbols headline fix) and relay_diagnostics still in progress
- CI gap filed as #1553

## Evidence

- transcript lines 25-26
- transcript lines 959-970
- transcript lines 1149-1158

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-4-presentation-formatting-moved-from-kernel-to.json`](transcripts/2026-06-18-4-presentation-formatting-moved-from-kernel-to.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-4-presentation-formatting-moved-from-kernel-to.json`](transcripts/raw/2026-06-18-4-presentation-formatting-moved-from-kernel-to.json)
