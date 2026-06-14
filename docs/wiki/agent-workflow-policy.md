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
---

# Agent Workflow Policy

## Pull Request Standards

Completed PR descriptions must include a short TLDR summary, a detailed overview of the work performed, and any subjective decisions including tradeoffs or assumptions. Completed work must not be opened as a draft pull request; draft PRs are only for intentionally incomplete work or when explicitly asked. <!-- [^019ec-1] -->

## Product Corrections

User product corrections must be treated as possible product-authority updates, not just implementation requests; a separate agent must research whether the correction should be represented in product specs, doctrine, canonical docs, docs/plan.md, GitHub Issues, or ADRs. If a product correction requires a documentation change, that update must be made in the same PR as the implementation unless the user explicitly scopes the work to docs only. <!-- [^019ec-2] -->
