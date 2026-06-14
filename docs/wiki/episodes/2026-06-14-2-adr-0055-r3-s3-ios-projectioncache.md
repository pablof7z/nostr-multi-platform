---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: architecture
status: superseded
subjects:
  - ios-projection-cache
  - incremental-emission
  - chirp-ios
  - decode-before-commit
supersedes:
  - 2026-06-14-2-ios-projectioncache-single-decode-architecture
related_claims: []
source_lines:
  - 8984-9157
captured_at: 2026-06-14T09:52:50Z
---

# Episode: ADR-0055 R3-S3: iOS ProjectionCache interposer — first host to enable incremental_apply

## Prior State

iOS received full snapshot frames every tick with no incremental merging capability. KernelBridge.swift was deliberately FlatBuffers-free. KernelModel updated every @Published slot every frame regardless of whether data changed.

## Trigger

ADR-0055 ladder S3 — first host to enable incremental_apply, converting the measured 81% per-frame waste into savings. Opus review caught a build-breaking double-decode that would have silently reintroduced per-tick waste.

## Decision

Codegen-generated ProjectionMergeCache runs before existing typed decoders, with decode-before-commit (D3-4): cache and rev advance only inside the success branch; decode failure sets needsResync=true and leaves prior entry untouched. Session/epoch scalars threaded from the single existing decode pass (not re-parsed). changedKeys: Set<String> gates @Published slot updates in KernelModel.apply; frame-level slots (envelope, flatFeeds, error toast) remain ungated. needsResync for self-healing on decode failure. Rebaseline clears cache atomically on session/epoch change.

## Consequences

- iOS is the first host realizing incremental-emission savings — ChirpTests 179/0 failures, ProjectionCacheTests 14/14 pass
- KernelBridge.swift stays FlatBuffers-free (session/epoch carried via decode tuple, not second parse)
- All 58 existing Swift test sites compile unchanged — no app-developer-facing API break
- ProjectionCache.generated.swift is codegen-checked (cargo run -p nmp-codegen -- gen projection-cache --check exits 0)
- The 4Hz double-decode waste path that would have existed was caught and eliminated before merge

## Open Tail

- R3-S4 (Android interposer) structurally identical, not yet started
- R3-S5/S6 capstone and empirical 81%→<5% proof pending

## Evidence

- transcript lines 8984-9157

