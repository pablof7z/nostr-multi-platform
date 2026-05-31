---
title: Agent Fan-Out Gate — All PRs Merged Before New Dispatch
slug: fan-out-gate-all-prs-merged
summary: Do not fan out a new set of implementation agents until every existing PR is merged and all worktrees are cleaned up.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-30
updated: 2026-05-28
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
  - session:54ae9075-be27-4b86-b69a-6955d9e79c3c
---

# Agent Fan-Out Gate — All PRs Merged Before New Dispatch

## Gate Rule

Do not fan out a new set of implementation agents until every existing PR is merged and all worktrees are cleaned up. Agents always run in the background (run_in_background: true) for every Agent tool call.

<!-- citations: [^4edd4-220] [^54ae9-7] -->
## See Also

