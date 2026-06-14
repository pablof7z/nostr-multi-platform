---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: architecture
status: superseded
subjects:
  - projection-cache
  - incremental-apply
  - decode-before-commit
  - changed-keys
supersedes:
  - 2026-06-14-2-projectioncache-codegen-generated-interposer-enables-incremental
related_claims: []
source_lines:
  - 8984-9407
captured_at: 2026-06-14T11:34:03Z
---

# Episode: ProjectionCache interposer enables incremental_apply on mobile

## Prior State

Mobile hosts received full re-serialized FlatBuffers frames every tick (~81% of serialized bytes were unchanged data being re-sent). No host-side incremental apply mechanism existed; apps had no way to opt in.

## Trigger

ADR-0055 Rung 3 implementation directive: enable incremental_apply with a codegen-generated, NMP-owned cache-merge layer that runs before typed decoders, keeping app code oblivious to delta mechanics.

## Decision

iOS and Android each get a codegen-generated ProjectionMergeCache. Key architectural invariants: (1) session_id/snapshot_epoch threaded from the single existing decode pass — no second buffer parse; (2) decode-before-commit prevents corrupt payload commits (both platforms effectively catch empty-payload only; non-empty corrupt bytes fail-closed on re-decode via try/catch + identifier check and self-heal on next good rev); (3) iOS uses per-slot changedKeys gating on @Published properties; Android reinstates cached bytes for omitted keys via merged envelope set; (4) needsResync flag forces full snapshot on decode failure; (5) capability is off-by-default, advertised via nmp_app_declare_incremental_apply.

## Consequences

- Both mobile hosts realize savings from incremental emission without any app-code changes
- iOS became the first host to enable the capability (PR #1409), Android followed (PR #1410)
- The iOS initial implementation had a build break (double-decode introducing FlatBuffers dependency into a deliberately-clean file); fixed by threading scalars from single decode
- D3-4 parity confirmed: both platforms honor decode-before-commit to the same degree — iOS's per-key decoder preflight is theater for non-empty payloads since FlatBuffers getRoot is unchecked on both platforms
- Android init-time app pointer leak on declare_incremental_apply error path fixed; corrupt-payload regression test added

## Open Tail

- Tier-1 (feed) projections remain always-Changed this rung, leaving the larger byte savings for a future rung

## Evidence

- transcript lines 8984-9407

