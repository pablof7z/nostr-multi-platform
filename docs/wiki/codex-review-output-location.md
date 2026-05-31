---
title: Codex Review Output Location
slug: codex-review-output-location
summary: Codex review output is stored in docs/perf/codex-reviews/ as the canonical location.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-26
updated: 2026-05-29
verified: 2026-05-26
compiled-from: conversation
sources:
  - session:f26050da-6d8a-4128-9179-4088a9df94b9
  - session:44c6cebb-bea4-4ca7-b836-0337e090a2a5
---

# Codex Review Output Location

## Output Location

Code reviews are never committed to the repository. (Previously: Codex review output was stored in docs/perf/codex-reviews/.) Review findings are directed to BACKLOG.md or durable docs; review prose is discarded.

<!-- citations: [^f2605-6] [^44c6c-1] -->

## Cleanup Actions

The docs/perf/codex-reviews/ directory and all its files are deleted. Junk files in docs/perf/ are also deleted: branch-cleanup-audit, chirp-parity-final-verification, execution-assessment, op-centric-feed-architect-review, orchestration-log, opus-direction-review-16, parallel-work-brainstorm, repo-state-snapshot, task-reconcile-and-next-steps, and audits/t126-*. Hardcoded links in docs/plan.md pointing to deleted codex-review files are removed. AGENTS.md replaces the passage naming docs/perf/codex-reviews/ as a valid location with an explicit ban on committing code reviews. Benchmark data in docs/perf/ is kept: firehose-bench, reactivity-bench, m10.5/S*, real-relay, pulse, screenshots, marmot, and pending-user-decisions. [^44c6c-2]
## See Also

