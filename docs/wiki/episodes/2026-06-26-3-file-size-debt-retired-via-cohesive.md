---
type: episode-card
date: 2026-06-26
session: 55264cfe-6420-4b06-a655-e0a935729211
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/55264cfe-6420-4b06-a655-e0a935729211.jsonl
salience: architecture
status: active
subjects:
  - file-size-governance
  - baseline-policy
  - module-extraction
  - actor-mod-rs
  - lib-rs
supersedes: []
related_claims: []
source_lines:
  - 2521-2693
  - 3050-3056
  - 3063-3065
captured_at: 2026-06-26T11:58:39Z
---

# Episode: File-size debt retired via cohesive splits, not baseline inflation — establish durable doctrine

## Prior State

Multiple core files sat at or above their baselines (actor/mod.rs 764, lib.rs 600, tests.rs 864), creating hard-cap debt. Prior practice implied baseline inflation was acceptable.

## Trigger

File-size gate reports hard-cap violations (lines 2666–2671); policy explicitly rejects baseline inflation ('split files and remove entries as debt is retired', line 2674).

## Decision

Extract three cohesive, behavior-preserving modules: (1) ActorConfig::build_pool helper to centralize pool construction and keep actor/mod.rs net-zero, (2) extract 185-line testing module from lib.rs to sibling testing.rs, (3) extract 56 jitter tests from relay_worker/tests.rs to jitter_tests.rs. Then ratchet baselines **down** to match new file sizes, not up.

## Consequences

- File-size gate establishes durable doctrine: **splits first, baselines only as debt is retired, never raised**
- New files (publish_failures.rs 79, outbound_tags.rs 123, jitter_tests.rs 65, testing.rs 187) all under soft cap (300 LOC)
- Prevents codebase from becoming 'god-file tax' culture where inflated baselines tolerate sprawl
- Visibility semantics changed for extracted functions (fn → pub(super) fn) for same-module-tree access (correct)
- Baselines lowered: actor/mod 764→761, lib removed (418 under 500), tests 864→812

## Open Tail

*(none)*

## Evidence

- transcript lines 2521-2693
- transcript lines 3050-3056
- transcript lines 3063-3065

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-26-3-file-size-debt-retired-via-cohesive.json`](transcripts/2026-06-26-3-file-size-debt-retired-via-cohesive.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-26-3-file-size-debt-retired-via-cohesive.json`](transcripts/raw/2026-06-26-3-file-size-debt-retired-via-cohesive.json)
