---
type: episode-card
date: 2026-06-26
session: 55264cfe-6420-4b06-a655-e0a935729211
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/55264cfe-6420-4b06-a655-e0a935729211.jsonl
salience: architecture
status: active
subjects:
  - doctrine-d13
  - cfg-gate-scope
  - test-code-extraction
supersedes: []
related_claims: []
source_lines:
  - 3260-3261
  - 3314-3320
  - 3340-3351
captured_at: 2026-06-26T11:58:39Z
---

# Episode: D13 doctrine-lint scope: cfg-gated code extraction requires per-line opt-out in new files

## Prior State

`mls_local_nsec` read in the testing module passed D13 lint when in cfg-gated lib.rs; moving the module to a separate file (testing.rs) caused D13 to trigger — the cfg-gate scope no longer protected the line.

## Trigger

D13 doctrine-lint failure (lines 3260–3261) on extracted testing.rs:79: 'read of `mls_local_nsec` outside `crates/nmp-marmot/` violates D13'.

## Decision

Apply the sanctioned per-line `// doctrine-allow: D13 — reason` opt-out at the line (Arc::new(Mutex::new(None))) that triggered the violation. The line is a benign slot initialization in test-support code, not a live key read — exactly the case the per-line allow exists for.

## Consequences

- D13 doctrine understanding deepened: cfg-gate scope is a real boundary; moving code out of lib.rs can change its legal status under doctrine rules
- Establishes durable pattern: extracting cfg-gated code from a file requires auditing it against the new file's doctrine context, not just mechanical splitting
- Per-line allow is the sanctioned recovery; it must be explicit and reason-documented
- Future file extractions must include doctrine review

## Open Tail

*(none)*

## Evidence

- transcript lines 3260-3261
- transcript lines 3314-3320
- transcript lines 3340-3351

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-26-4-d13-doctrine-lint-scope-cfg-gated.json`](transcripts/2026-06-26-4-d13-doctrine-lint-scope-cfg-gated.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-26-4-d13-doctrine-lint-scope-cfg-gated.json`](transcripts/raw/2026-06-26-4-d13-doctrine-lint-scope-cfg-gated.json)
