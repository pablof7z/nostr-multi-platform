---
title: Agent Workflow Policy
slug: agent-workflow-policy
topic: codebase-patterns
summary: Completed PR descriptions must include a short TLDR summary, a detailed overview of the work performed, and any subjective decisions including tradeoffs or assu
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-14
updated: 2026-06-14
verified: 2026-06-14
compiled-from: conversation
sources:
  - session:019ec57a-fb01-7081-80c8-d7107f302049
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
  - session:78c8ec3a-f558-4738-98af-1f3af4978ec4
---

# Agent Workflow Policy

## Pull Request Standards

Completed PR descriptions must include a short TLDR summary, a detailed overview of the work performed, and any subjective decisions including tradeoffs or assumptions. Completed work must not be opened as a draft pull request; draft PRs are only for intentionally incomplete work or when explicitly asked. <!-- [^019ec-1] -->


Direct orchestration — assigning one implementer per rung and merging via a background gh-checks waiter — proved effective, landing roughly 15 PRs cleanly. Delegating to 'lead' agents stalled due to checkpoint-and-exit behavior, duplicate-ownership conflicts, and CI-monitor waits, and should be avoided. <!-- [^2e544-361] -->

The agent will not self-merge PRs regardless of coordinator requests; standing instructions reserve merge authority for the user/owner. <!-- [^78c8e-461] -->

Architecture must be proper with no technical debt, no migrations, and no avoiding breaking changes; decisions should be anchored in empirical measurement when possible. <!-- [^78c8e-480] -->
## Product Corrections

User product corrections must be treated as possible product-authority updates, not just implementation requests; a separate agent must research whether the correction should be represented in product specs, doctrine, canonical docs, docs/plan.md, GitHub Issues, or ADRs. If a product correction requires a documentation change, that update must be made in the same PR as the implementation unless the user explicitly scopes the work to docs only. <!-- [^019ec-2] -->

## Agent Orchestration Loop

Use the opus+codex+sonnet+opus-review loop: opus for planning/design, codex for feedback/research, sonnet for coding, and opus for review. Parallelism should be limited. <!-- [^78c8e-481] -->
