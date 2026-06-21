---
type: episode-card
date: 2026-05-26
session: 37e351ee-aa2b-43eb-9793-482de338f883
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/37e351ee-aa2b-43eb-9793-482de338f883.jsonl
salience: product
status: active
subjects:
  - flatbuffers-decode
  - update-transport
  - kernel-degradation-metric
supersedes:
  - 2026-05-26-1-update-transport-error-handling-silent-degradation
related_claims: []
source_lines:
  - 229-244
  - 258-344
  - 194-221
  - 166-179
  - 146-165
  - 400-429
captured_at: 2026-06-18T05:53:12Z
---

# Episode: Update transport rejects malformed data instead of silently degrading to null

## Prior State

decode_value silently degraded all error paths to Value::Null — missing nested fields, unknown value kinds, non-finite floats, and missing strings all produced null instead of errors. On the encode side, serde_json::to_value failure produced an empty {} with no diagnostics or metrics.

## Trigger

Review feedback identified that silent degradation hides value-shape drift, making malformed or impossible wire data invisible in diagnostics instead of surfacing it.

## Decision

decode_value now returns Result<Value, UpdateFrameDecodeError> with explicit error variants (InvalidValue for non-finite floats, missing strings, missing nested structures, unknown kinds). Encode-side serde_json failure now produces a structured degradation payload (with rev, metrics, error category/toast) and increments a monotonic update_frame_degradations_total counter.

## Consequences

- Non-finite floats (NaN, Inf) now fail decode explicitly instead of becoming null
- Missing FlatBuffers fields that were silently null now surface as InvalidValue errors
- Future wire-format drift will produce observable errors rather than silently corrupted snapshots
- Degradation events are now trackable via the monotonic metric counter and NMP_DEGRADATION log lines
- Downstream consumers must handle UpdateFrameDecodeError::InvalidValue

## Open Tail

*(none)*

## Evidence

- transcript lines 229-244
- transcript lines 258-344
- transcript lines 194-221
- transcript lines 166-179
- transcript lines 146-165
- transcript lines 400-429

