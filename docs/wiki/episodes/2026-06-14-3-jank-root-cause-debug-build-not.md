---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: root-cause
status: active
subjects:
  - chirp-jank-root-cause
  - adr-0055-option-b-decision
  - equatable-shield
supersedes:
  - 2026-06-14-1-feed-byte-equality-gating-shipped-idle
related_claims: []
source_lines:
  - 10867-11051
captured_at: 2026-06-14T20:54:15Z
---

# Episode: Jank root cause: Debug build, not emission; Option B not warranted

## Prior State

Felt idle jank was attributed to 4Hz snapshot emission re-rendering the timeline; Option B (feed row-deltas) was considered as a potential further optimization

## Trigger

Release A/B measurement showed timeline body re-evals/sec = 0 in both incremental ON and OFF arms; .equatable() at HomeFeedView.swift:147 short-circuits body recompute even when @Published reassigns the feed; on-device Time Profiler showed zero hangs, zero hang-risks, Nominal thermal, no dominant hotspot

## Decision

Option B (row-deltas) NOT pursued — it does nothing for the idle case (already shielded by .equatable()) and on mutating frames the List must re-render anyway because a card genuinely changed; felt jank attributed to Debug build (~17.6× slower encode); stop optimizing feed/emission path

## Consequences

- .equatable() is the load-bearing idle-render shield, not feed-omission — incremental emission is a valuable-but-redundant second layer (cuts FFI bytes + @Published fan-out + array alloc)
- The ADR-0055 work stands as bandwidth/battery/architecture win, not the jank fix
- Only reopen Option B if release-device data shows mutating-feed frames missing frame budget while idle is clean
- Residual per-tick feed decode flagged but not auto-fixed (in tension with bounded-native-state doctrine D5)

## Open Tail

- ≥50-card busy-account + forced-120Hz run remains as stricter non-blocking confirmation (fixture account only had ~3 cards)
- relay_diagnostics decoded on home feed during scroll — minor wasted CPU, not jank cause

## Evidence

- transcript lines 10867-11051
