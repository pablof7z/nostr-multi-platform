---
type: episode-card
date: 2026-06-09
session: 63af4b96-d3d3-45c3-ab96-9f899beafa1b
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/63af4b96-d3d3-45c3-ab96-9f899beafa1b.jsonl
salience: architecture
status: active
subjects:
  - nmp-conformance-scanner
  - doctrine-drift-gate
  - source-of-truth
supersedes: []
related_claims: []
source_lines:
  - 1-126
captured_at: 2026-06-11T23:10:26Z
---

# Episode: NMP conformance scanner: in-repo catalog + drift gate, not portable standalone

## Prior State

No portable way for downstream NMP apps to check conformance; doctrine-lint existed but was framework-builder-facing, Rust-only, and scoped to crates/

## Trigger

User proposed a self-contained portable skill for scanning any app codebase for guideline violations; advisor corrected that a free-floating catalog would be a parallel source of truth — the single most-enforced rule in the repo

## Decision

In-repo canon-adjacent catalog with a CI drift gate (every rule must cite a live D0–D10/C1–C13 bullet or CI fails, modeled on contract_surface_complete), plus a generated snapshot scaffolded into app repos via nmp-app-template for distribution. LLM-judgment workflow first; mechanical grep/AST CI tier deferred. Reports are ephemeral → promote to GitHub issues, never committed.

## Consequences

- No parallel source of truth — catalog drift is caught by CI, not by hand
- Consumer-side detection signatures are the only new content; why/examples stay in the builder-guide canon
- Distribution via nmp-app-template snapshot, not a standalone package

## Open Tail

- PR #1014 in draft; catalog enumeration (~35 rules) seeded but not yet validated against live apps

## Evidence

- transcript lines 1-126

