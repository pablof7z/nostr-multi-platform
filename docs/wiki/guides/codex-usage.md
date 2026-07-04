---
title: Codex Usage Policy
slug: codex-usage
topic: codex-usage
summary: Codex is used sparingly, only at major milestones
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-29
updated: 2026-07-03
verified: 2026-06-29
compiled-from: conversation
sources:
  - session:3c942260-311d-4e00-8bcc-204045ea87b3
  - session:91a86fdf-624c-446e-9b38-0fb02085121f
---

# Codex Usage Policy

## When Codex Is Consulted

Codex is used sparingly, only at major milestones. Routine slices like the #2388 benchmark methodology are not sent to codex because a single slice is not a milestone. After the #2389 scaffold design pass, codex is consulted for feedback because the scaffold is the foundational binding pattern every later surface migration copies.

Money-handling code requires a codex problem/solution design pass — covering crate module layout and the journal/saga/trail split — before any implementation. The work happens in an isolated worktree on a feature branch, never pushed to master. Codex reviews the diff before the PR is opened, and the worktree is cleaned up once the work is done. Codex review findings on wallet diffs are promoted to GitHub issues; the review artifact itself is discarded and never committed to the repository.

<!-- citations: [^3c942-92b19] [^91a86-da265] [^91a86-405ed] -->
