---
type: episode-card
date: 2026-07-04
session: d8bc6df1-32a3-48e1-8db6-3dbff7c4c0e5
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform--claude-worktrees-fix-2962-flaky-auto-arm/d8bc6df1-32a3-48e1-8db6-3dbff7c4c0e5.jsonl
salience: direction
status: active
subjects:
  - v1-release-scope
  - issue-2864
  - issue-2858
  - issue-2927
  - issue-2974
supersedes: []
related_claims: []
source_lines:
  - 285-311
  - 315-337
captured_at: 2026-07-04T12:25:02Z
---

# Episode: V1 scope boundary: four ambiguous epics ruled non-blockers, v1 = owner-gated publish only

## Prior State

Four open issues (#2864 wallet, #2858 X-Ray, #2927 NIP-AD, #2974 Marmot MLS keyring) appeared as un-phased p2 items against #2690's v1 exit criteria, creating ambiguity about whether code work remained to block the v1 release.

## Trigger

Opus scope-review agent grounded analysis in two authoritative surfaces: #2690 exit criteria (v1 = name/version/publish/prove) and docs/nips.md (the v1 pre-release truth source). Verified #2927 deliverables on master via grep; confirmed #2974 bug is real but in a non-v1-claimed feature; confirmed #2864 and #2858 are feature-app/dev-tool surfaces excluded from release artifacts.

## Decision

None of the four are genuine v1 code blockers. #2927 closed as delivered (core NIP-AD verified on master). #2864, #2858, #2974 labeled phase:post-v1 and moved to Backlog. v1 is now defined as exactly the owner-gated publish act (name → rc rehearsal → 1.0.0 tag → crates.io/npm → external-consumption proof), gated only on owner approval.

## Consequences

- Zero genuine framework code work blocks v1 — only the irreversible publish act remains for the owner
- #2690 remains the sole 'To triage' item on the board, representing the owner-gated publish
- #2974 stays open as a real bug but explicitly deferred to post-v1 DM/groups milestone
- An issue existing ≠ it must ship in v1; docs/nips.md is the binding scope truth source

## Open Tail

- #2711 (quick-xml RUSTSEC) blocked on upstream wayland-scanner release with 2026-09-30 deadline
- Owner must explicitly approve the 1.0.0 publish act

## Evidence

- transcript lines 285-311
- transcript lines 315-337

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-07-04-1-v1-scope-boundary-four-ambiguous-epics.json`](transcripts/2026-07-04-1-v1-scope-boundary-four-ambiguous-epics.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-07-04-1-v1-scope-boundary-four-ambiguous-epics.json`](transcripts/raw/2026-07-04-1-v1-scope-boundary-four-ambiguous-epics.json)
