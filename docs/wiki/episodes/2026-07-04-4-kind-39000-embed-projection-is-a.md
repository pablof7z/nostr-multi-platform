---
type: episode-card
date: 2026-07-04
session: d8bc6df1-32a3-48e1-8db6-3dbff7c4c0e5
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform--claude-worktrees-fix-2962-flaky-auto-arm/d8bc6df1-32a3-48e1-8db6-3dbff7c4c0e5.jsonl
salience: root-cause
status: active
subjects:
  - embed-kind-projection
  - issue-2928
  - nip-29-group-card
  - flatbuffers-wire
supersedes: []
related_claims: []
source_lines:
  - 350-364
  - 369-375
captured_at: 2026-07-04T12:25:02Z
---

# Episode: kind:39000 embed projection is a cross-cutting wire change, not a registry add

## Prior State

Issue #2928 was filed as a simple registry add (~5 files: one card per platform + manifest), analogous to the prior content-kind-9802 addition, implying the domain model was already owned by nmp-nip29.

## Trigger

Agent traced the full implementation path end-to-end and found no kind:39000 embed projection exists in the domain layer. Adding it requires a new EmbedKindProjection variant (GroupProjection), FlatBuffers embed_sidecar wire table + encode/decode arms, 4 native renderer dispatch updates, gallery preview updates, and only then the registry manifests — ~15-20 files, not ~5.

## Decision

Reclassified #2928 as post-v1 Backlog. Recommended splitting into (a) prerequisite domain/wire issue (EmbedKindProjection + FlatBuffers + adapter seam for nmp-nip29) and (b) platform card components + registry manifests. No code written — agent refused to implement a mis-scoped issue.

## Consequences

- kind:39000 requires a new register_group_projection_adapter seam since nmp-nip29 doesn't depend on nmp-content (avoiding cycle)
- Every EmbedKindProjection variant has exhaustive match obligations across FlatBuffers wire, 4 platform registries, gallery previews, and golden tests
- Issue scoping for embed content kinds must account for the full projection→wire→renderer pipeline, not just platform card files

## Open Tail

- Split issue (a) domain/wire and (b) platform components not yet opened
- GroupProjection extraction from single raw event is new code vs nmp-nip29's existing multi-event accumulator

## Evidence

- transcript lines 350-364
- transcript lines 369-375

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-07-04-4-kind-39000-embed-projection-is-a.json`](transcripts/2026-07-04-4-kind-39000-embed-projection-is-a.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-07-04-4-kind-39000-embed-projection-is-a.json`](transcripts/raw/2026-07-04-4-kind-39000-embed-projection-is-a.json)
