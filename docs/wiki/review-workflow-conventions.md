---
title: Review Workflow & Diff Conventions
slug: review-workflow-conventions
summary: Future merge reviews use a chunked-diff approach (per-file summaries or per-commit reviews) instead of full diffs to stay under codex's effective budget.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-29
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:423f3c56-7275-4e62-998e-e8f37be564da
  - session:575288b2-1197-44d2-ba9b-d72e8d74f9a6
  - session:1c093fa5-0f0e-4dee-bf38-99781e763f13
  - session:c8c2902c-43a6-4b1c-8215-1732dc266895
  - session:f26050da-6d8a-4128-9179-4088a9df94b9
  - session:752b523f-231e-4fca-ab86-748c35b5dd74
  - session:44c6cebb-bea4-4ca7-b836-0337e090a2a5
  - session:1d30779f-b6ee-44ad-a1f1-bdc17f26ebdd
---

# Review Workflow & Diff Conventions

## Review Format

Future merge reviews use a chunked-diff approach (per-file summaries or per-commit reviews) instead of full diffs to stay under codex's effective budget. 10 parallel Sonnet agents are used for codebase review, with 1 Opus agent consolidating their reports.

All work follows the PR → `codex exec` review → fix → merge to master workflow. Codex is called again for assessment only after all previously suggested fixes have been landed on master.

Each completed PR must be immediately merged to master with origin/master and master kept in sync, followed by a codex review of the diff. Pre-existing CI failures on master from unrelated commits do not block merging a PR that passes its own diff.

<!-- citations: [^423f3-19] [^1c093-33] [^57528-17] [^c8c29-5] [^f2605-13] -->
## Status Reporting

Status reports must be derived from actual source code rather than markdown status files. Code reviews must never be committed to the repository; review prose must be discarded after findings are recorded to BACKLOG.md or other durable docs. Committing raw conversation transcript dumps is disallowed by the repo's no-committed-reviews discipline. Codex output (if saved) goes to docs/perf/codex-reviews/ (the existing canonical location), not a new directory. Dated historical review snapshots must not be rewritten when cleaning up stale references, to preserve the repo's single-source/temporal discipline.

<!-- citations: [^57528-18] [^f2605-14] [^752b5-8] [^44c6c-4] [^1d307-5] -->
## See Also

