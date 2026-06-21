---
type: episode-card
date: 2026-05-26
session: 54fc9b94-b995-46c6-8372-59c4abe0f95a
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/54fc9b94-b995-46c6-8372-59c4abe0f95a.jsonl
salience: product
status: superseded
subjects:
  - update-frame-decode
  - update-frame-encode
  - degradation-counter
supersedes: []
related_claims: []
source_lines:
  - 301-356
  - 363-478
  - 609-620
captured_at: 2026-06-18T05:51:50Z
---

# Episode: Update transport error handling: silent degradation → explicit error propagation

## Prior State

decode_value silently degraded invalid or missing FlatBuffers values to JSON null; serde_json::to_value failure on the encode path fell back to an empty JSON object, losing all diagnostic context

## Trigger

Review feedback on PR #582 FlatBuffers transport identified that silent degradation hides data corruption and makes malformed snapshots indistinguishable from legitimately empty ones

## Decision

Decoding now returns Result and propagates errors: NaN/Infinity floats, missing optional fields (string_value, list, map, pair.value), and unknown value kinds all produce UpdateFrameDecodeError::InvalidValue instead of null. Encoding failures now emit a minimal but informative degraded snapshot (schema_version, rev, running, degradations counter, error category) rather than an empty object, with a monotonic update_frame_degradations_total counter surfaced through KernelMetrics

## Consequences

- Swift FlatBuffers decoder must handle decode errors rather than assuming null defaults
- Malformed data no longer silently corrupts snapshot state
- Degradations counter visible in KernelMetrics and NMP_DEGRADATION log lines
- Non-finite float rejection explicitly tested
- The generic FlatBuffers value tree is confirmed as an interim shape; typed snapshot tables are the next performance step (per BACKLOG measurement)

## Open Tail

- Typed FlatBuffers snapshot tables as the follow-up performance step when foreground logs approach tick budget

## Evidence

- transcript lines 301-356
- transcript lines 363-478
- transcript lines 609-620

