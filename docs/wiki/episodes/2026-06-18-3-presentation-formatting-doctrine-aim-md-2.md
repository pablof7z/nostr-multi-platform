---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: architecture
status: superseded
subjects:
  - adr-0032
  - aim-md-section-2
  - presentation-formatting
  - signer-state
  - publish-outbox
supersedes:
  - 2026-06-18-5-presentation-formatting-removed-from-kernel-projections
related_claims: []
source_lines:
  - 1587-1609
captured_at: 2026-06-18T22:54:46Z
---

# Episode: Presentation formatting doctrine: aim.md §2 supersedes ADR-0032 for display labels

## Prior State

ADR-0032/#1099 directed that labels be precomputed in Rust so native code doesn't branch on discriminants. This resulted in English prose strings ("Connecting to bunker relays…", status tones, SF Symbol names like "person.fill") baked into platform-neutral nmp-core FlatBuffers projections.

## Trigger

Issue #1493 P1 audit (SF Symbols in kernel) + p9's PR3 direction conflict: ADR-0032 said precompute in Rust, but aim.md §2 says Rust emits raw data and shells own formatting. Every sibling lane (#1568 publish_outbox, #1536 marmot, #1537 nip29) had already reversed this precompute pattern.

## Decision

Direction A confirmed: aim.md §2 is the immutable north star and overrides ADRs for display labels. Rust emits raw semantic tokens (signer_kind, connection_state, stage, event kind, status, attempt). Shells map token→localized label via shared, parity-consistent helpers. ADR-0032 is superseded for this class of projection. The reconciliation preserves ADR-0032's real concern (no iOS/Android divergence) through shared helpers rather than Rust precomputation.

## Consequences

- signer_state status_label/status_tone removed from Rust; shells render from raw state/stage tokens
- publish_outbox SF Symbols and English strings removed; shells own title/icon/preview/label formatting
- marmot and nip29 labels already reversed in sibling PRs
- Future label additions go to shell helpers, not nmp-core projections
- ADR-0032 remains valid for its original concern (preventing divergence) but the mechanism is shared shell helpers, not Rust precompute

## Open Tail

- p1 relay_diagnostics slice still in CI (#1577)
- p9 PR3 (signer-labels-to-shells) in progress

## Evidence

- transcript lines 1587-1609

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-3-presentation-formatting-doctrine-aim-md-2.json`](transcripts/2026-06-18-3-presentation-formatting-doctrine-aim-md-2.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-3-presentation-formatting-doctrine-aim-md-2.json`](transcripts/raw/2026-06-18-3-presentation-formatting-doctrine-aim-md-2.json)
