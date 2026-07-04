---
title: CI Gate Policies During Migration
slug: ci-gates
topic: ci-gates
summary: During the migration, CI checks that help identify issues as we build are kept, but unnecessary CI gates that slow things down while things are supposed to be b
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-29
updated: 2026-07-03
verified: 2026-06-29
compiled-from: conversation
sources:
  - session:898a41b5-68e0-4b0f-b16c-c6072454bd6a
  - session:019f0dc3-5b56-79d1-a14b-5746c93e5879
  - session:5ad70acc-1442-4343-92a7-f79b2fc59071
  - session:91a86fdf-624c-446e-9b38-0fb02085121f
---

# CI Gate Policies During Migration

## CI Gates

During the migration, CI checks that help identify issues as we build are kept, but unnecessary CI gates that slow things down while things are supposed to be broken mid-refactor are removed or disabled. The perf-gates CI check is disabled — converted to `workflow_dispatch` (manual-only) and tied to epic #2340, preserving the full pipeline so it can be re-enabled with a one-line change. Before the migration ends, the disabled perf-gates must be replaced with a meaningful automatic perf signal, not left permanently off.

All other CI gates must remain strict throughout the migration: doctrine-lint, codegen-drift, test, browser-runtime, and supply-chain gates stay enforcing. Doctrine-lint and doctrine grep (D0/D6/D7/D8) gates enforce the exact boundaries the clean-break is establishing; relaxing them during the migration would be relaxing the migration itself. Doc and vocabulary ratchets are pro-migration and must also remain strict.

The test gate runs `cargo nextest run --workspace`, so workspace-member crates are automatically covered by CI.

File-size gate violations must be fixed by splitting files — never by raising the baseline.

The user permits taking any shortcuts to expedite the epic, including deleting or disabling wholesale test suites that no longer apply, removing CI, and moving nmp-gallery apps out of this repo into its own repo to be updated after the fact.

No temporary hacks, stubs that stay, or "TODO: fix this properly" comments are allowed in production code; a staged fix is allowed only when a GitHub issue labeled `status:staged` documents every stage with a completion deadline.

The known "agent rests after launching CI" pattern causes agents to go idle before CI finishes, requiring the orchestrator to take the merge via a background waiter that polls CI and squash-merges when all checks are green.

<!-- citations: [^898a4-c4fc1] [^019f0-05ac8] [^898a4-e3fc4] [^5ad70-89739] [^91a86-006f6] -->
