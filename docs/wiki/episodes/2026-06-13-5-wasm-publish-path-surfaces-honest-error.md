---
type: episode-card
date: 2026-06-13
session: 027459be-7102-4e1a-b6d4-02e8e7863642
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/027459be-7102-4e1a-b6d4-02e8e7863642.jsonl
salience: product
status: active
subjects:
  - nmp-wasm-publish
  - capability-failure
  - d6-error-handling
supersedes: []
related_claims: []
source_lines:
  - 9116-9183
captured_at: 2026-06-13T21:35:37Z
---

# Episode: Wasm publish path surfaces honest error instead of silent drop

## Prior State

nmp-wasm publish path silently dropped requests with NoTargets — no error surfaced to the consumer

## Trigger

Audit found silent NoTargets drop violated D6 (errors never cross FFI as panics) and D0 honesty doctrine

## Decision

Replace silent NoTargets drop with explicit CapabilityFailure via publish_not_supported_in_web_preview_reason(); honest-disable the wasm publish path

## Consequences

- Wasm consumers receive a real error explaining publish is not supported in web preview
- No silent data loss on publish attempts
- D6/D0 compliance for wasm publish surface

## Open Tail

*(none)*

## Evidence

- transcript lines 9116-9183

