---
title: Multi-Agent Exploration & Synthesis Pattern
slug: multi-agent-exploration-synthesis
summary: 10 parallel Sonnet agents explore distinct codebase facets, then one Opus synthesizer produces recommendations.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-05-29
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:1c093fa5-0f0e-4dee-bf38-99781e763f13
  - session:eb342a0d-84e3-4289-9873-88a947ca8144
  - session:cd2b6122-2b7c-43fc-941b-c51e79ffc691
  - session:f3d8d762-5bb9-4db7-b127-667085e512bf
---

# Multi-Agent Exploration & Synthesis Pattern

## Multi-Agent Exploration and Synthesis

10 parallel Sonnet agents explore distinct codebase facets, then one Opus synthesizer produces recommendations. The Opus agent also provides UX/UI feedback covering display, creation, and navigation. Fallback search agents are instructed to err heavily on the side of false positives; a single Opus agent triages Haiku agent fallback findings against D6/doctrine to distinguish genuinely problematic fallbacks from intentional doctrined patterns. Scaffolding search uses 10 Haiku agents to find unjustifiable scaffolding across the codebase, followed by an Opus validation agent to confirm findings. Implementation uses an integration branch with 10 parallel Haiku agents each working in their own git worktree; this can be scaled up to 100 parallel agents. No Haiku agent should ever run `cargo test` — testing happens during merge. After each Haiku agent's work is merged, a Sonnet agent reviews the diff before merging into the integration branch. When enough significant work is done to justify a PR, it should be sent and landed in master. Agents must push to `feature/<task_id>` branches — never directly to master. A Sonnet merge agent cherry-picks the specific commit hash onto master after review.

<!-- citations: [^1c093-10] [^eb342-8] [^cd2b6-11] [^f3d8d-12] -->
## See Also

