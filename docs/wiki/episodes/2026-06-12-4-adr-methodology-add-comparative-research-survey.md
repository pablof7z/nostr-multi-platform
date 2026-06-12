---
type: episode-card
date: 2026-06-12
session: 954c56b2-d292-4021-8b55-977d3fd8df4d
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/954c56b2-d292-4021-8b55-977d3fd8df4d.jsonl
salience: workflow
status: active
subjects:
  - adr-process
  - m2-open-interest-adr
supersedes: []
related_claims: []
source_lines:
  - 1208-1210
captured_at: 2026-06-12T06:08:15Z
---

# Episode: ADR methodology: add comparative-research survey step before design decisions

## Prior State

NMP ADRs record decisions and rationale but skip the comparative survey step — they document what was decided, not how multiple implementations approach the same problem or why alternatives were rejected

## Trigger

Coracle-rust's research→plan→write pipeline surveys 7 implementations before each design decision and records where they diverge and why the chosen approach differs. This produces auditable rationale with rejected alternatives that the current NMP ADRs lack

## Decision

Adopt a comparative-research step into NMP's ADR process: before recording the decision, survey how welshman/applesauce/NDK/rust-nostr model the same surface. First candidate is the pending M2 nmp_app_open_interest ADR

## Consequences

- ADRs will include rejected alternatives with reasons, improving future revisitation
- The M2 open_interest ADR will benefit from welshman's getFilterLimit-style intrinsic limit computation insights
- Nearly free with agents to run the survey, but adds a step to the ADR pipeline

## Open Tail

- Define ADR template section for comparative survey
- Run survey for M2 open_interest ADR

## Evidence

- transcript lines 1208-1210

