---
title: Opus Architect Workflow — Plan, Validate, Execute, Audit
slug: opus-architect-workflow
summary: Opus agents produce architectural plans for cross-platform initiatives; plans are validated by Codex, approved by humans, and executed by Haiku agents.
tags:
  - workflow
  - architecture
  - agents
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:6a951af3-7b08-4d8d-adfd-361609270d50
  - session:38935d82-0cbf-4e85-98d3-a0f056fd450c
---

# Opus Architect Workflow — Plan, Validate, Execute, Audit

> Opus agents produce architectural plans for cross-platform initiatives; plans are validated by Codex, approved by humans, and executed by Haiku agents.

## Purpose

Opus agents are used to produce architectural plans for complex cross-platform initiatives. The agent reads the actual code across all platforms, identifies gaps and doctrine violations, proposes a concrete design, and surfaces open questions for human decision before any implementation begins. [^6a951-55]

## When to Use Opus

Use an Opus agent when: (1) the initiative spans multiple platforms (iOS, Android, TUI, Desktop); (2) business logic needs to be identified and moved into shared Rust; (3) the problem involves doctrine compliance across platforms; (4) the scope is too large for a single Haiku agent. Opus produces the plan; Haiku agents (in parallel worktrees with Sonnet review) execute it. [^6a951-56]

## Codex Validation Gate

Before fanning out agents to implement an Opus plan, the plan must be validated by Codex (via codex exec program). Codex reviews the plan for blockers: schema deficiencies, overly broad enforcement, hidden dependencies, and prerequisite misclassification. Codex feedback is incorporated into the plan before any implementation agent is dispatched. This gate caught 3 blockers and 1 high-severity issue in the nmp-gallery consolidation plan before implementation began. [^6a951-57]


Architecture plans for platform UI work must be verified against the full doctrine checklist before implementation: D8 (no polling) — the plan must consume kernel-pushed snapshots reactively; ADR-0025 (no bespoke pull symbols) — the plan must use the existing projection registry seam, minting zero new FFI symbols; One-way principle — one mechanism for projections, the plan must use it; ADR-0037 (typed transport) — the plan must read typed Swift/Kotlin structs, not raw JSON; Component-owned reactivity — components must signal their own data requirements via claim/release, kernel never pre-fetches. This verification was applied to the embed/kind registry plan, confirming every doctrine is satisfied before any code is written. [^38935-7]
## Open Questions Pattern

Opus plans surface open questions for human decision before proceeding. These cover areas where the architecture has legitimate tradeoffs: npub abbreviation standardization (per-platform vs unified), variant scope (which component categories), feature parity commitments (all platforms render vs placeholder-only), and file location decisions. Human answers to these questions become binding architectural decisions that the implementation agents must follow. [^6a951-58]

## Post-Implementation Audit

After implementation lands, an Opus audit reads the actual merged code and answers: what was actually accomplished vs. what was needed, where does per-platform business logic still exist, and what are the highest-value remaining tasks. This audit closes the loop — it prevents the surface-level work (catalog parity, de-polling, visual polish) from being mistaken for architectural completion. [^6a951-59]

## See Also
- [[nmp-gallery-cross-platform-consolidation|NMP Gallery Cross-Platform Consolidation — Registry-Driven Component Catalog]] — related guide
- [[multi-agent-integration-workflow|Multi-Agent Integration Workflow — Fan-Out with Integration Branch]] — related guide
- [[component-owned-reactivity-architecture|Component-Owned Reactivity Architecture]] — related guide

