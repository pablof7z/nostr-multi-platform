---
title: Multi-Agent Integration Workflow — Fan-Out with Integration Branch
slug: multi-agent-integration-workflow
summary: "Large-scale implementation uses a fan-out pattern: haiku agents in isolated worktrees, sonnet review gate, merged into an integration branch, then PR'd to master when substantial."
tags:
  - workflow
  - agents
  - integration
  - parallel
  - worktree
volatility: hot
confidence: medium
created: 2026-05-29
updated: 2026-05-31
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:9a2c7cd8-95ab-4291-bbc8-6f38c5941c0a
  - session:d0690875-a693-48ef-ac6f-31a92f5699cc
  - session:f3d8d762-5bb9-4db7-b127-667085e512bf
  - session:38935d82-0cbf-4e85-98d3-a0f056fd450c
---

# Multi-Agent Integration Workflow — Fan-Out with Integration Branch

> Large-scale implementation uses a fan-out pattern: haiku agents in isolated worktrees, sonnet review gate, merged into an integration branch, then PR'd to master when substantial.

## Overview

For large-scale implementation work, use a fan-out pattern: dispatch multiple haiku agents in parallel, each working in its own git worktree on isolated tasks. A sonnet agent reviews each piece of work before merging into an integration branch. No haiku agent should ever run cargo test — testing happens during merge review. [^9a2c7-11]


The Chirp iOS embed system (PR #795) demonstrated a refined variant: 12 agents across 5 sequential phases where each phase's output is a dependency for the next phase, but agents within a phase run in parallel. Phase 1 (Foundation, 3 parallel) → Phase 2 (Bridge + Registry, 3 parallel) → Phase 3 (Views, 2 parallel) → Phase 4 (Wire, 2 parallel) → Phase 5 (Ship, 1 agent). This contrasts with wave-based organization where all tasks in a wave have zero inter-dependencies — here the phases represent genuine build dependencies (Phase 3 views import Phase 2 types, Phase 4 wires import Phase 3 views). The final step is always project file regeneration (xcodegen generate for iOS) before opening the PR. [^38935-31]

When a user requests a specific agent count (e.g., "launch 10 haiku agents"), the correct response is to first scope the actual work by exploring the codebase and attempting a build, then determine the appropriate number of agents. Blindly dispatching the requested count without understanding the scope leads to unnecessary work. In one case, a 10-agent request for "explore and fix" was correctly scoped down: a build attempt revealed exactly two compilation errors, which needed only 2 parallel fix agents, not 10. The remaining work (embed system) was addressed separately once the build was green and architectural compliance was verified. [^38935-40]
## Agent Lifecycle

Each haiku agent works in its own git worktree on a dedicated branch. After completing work, a sonnet agent reviews the changes. Once reviewed and approved, the work is merged into the integration branch. After merge, the next haiku agent in the queue is dispatched. Agents are dispatched in waves — start with a proven batch size, then scale up once the pattern is validated. [^9a2c7-12]


**Complexity-based agent selection**: When a Haiku agent fails on a complex task (e.g., FlatBuffers typed projection), re-dispatch with a Sonnet agent for that specific task. The fix batch pattern targets complex failures with Sonnet and simpler fixes with Haiku. [^f3d8d-16]
## Scaling

Start with a small batch (5–10 parallel haiku agents) to prove the workflow. Once validated, scale up to 100 parallel agents. No haiku agent should ever run cargo test — that happens during the merge review by the sonnet reviewer. [^9a2c7-13]


For backlog-clearance workflows, use a three-phase structure: Audit (verify every listed item is still live at HEAD, classify mechanical-fix vs needs-design/ADR), Design (Opus architect produces concrete plan/ADR for consequential items like new-crate or threading-model changes — never blind auto-implementation), Implement (PR agents in worktree isolation for well-scoped items). The key judgment is deciding which items get designed vs auto-coded: new crates, threading-model changes, and type-enforced ordering changes get designed first; well-scoped fixes like store cluster changes or UI defaults go straight to implementation. [^d0690-55]

Batch sizing: Start with 10 parallel agents to prove the pattern, then scale up. Batch 2 used 10 agents; batch 3 scaled to 15. The user's goal is to scale to 100 parallel agents once the workflow is validated.

**Wave organization by platform and crate ownership**: Tasks within a wave must have zero dependencies on each other. Organize tasks by file ownership — one agent per file when possible (e.g., one Sonnet agent owns `KernelModel.kt`, another owns `MainActivity.kt`). For a single file with multiple concerns (e.g., `app.rs`), assign different sections to different agents. Group by platform (Android, Desktop) and by architecture crate changes.

**Within-wave parallelism constraint**: Within a wave, each task has zero dependencies on other tasks in the same wave. Dependencies only exist across wave boundaries — Wave 2 depends on Wave 1 foundations being in place. [^f3d8d-17]
## Integration Branch

## Integration Branch

All worktree agent work merges into a single integration branch (no individual PRs for each agent). The integration branch accumulates work until there is enough to justify a PR to master. When significant work is done, the integration branch is sent as a PR and landed in master. 

**Critical protocol**: Agents must push to feature branches (e.g., `feature/<task-id>`), never directly to master. In batch 1, agents followed AGENTS.md push protocol and pushed directly to master — this bypassed the integration branch. The protocol was corrected in batch 2: agents explicitly create `feature/<task_id>` branches and push there. The Sonnet merge agent then cherry-picks the specific commit hash onto master after review.

<!-- citations: [^9a2c7-14] [^f3d8d-14] -->
## Review Gate

A sonnet agent reviews each haiku agent's work before it is merged into the integration branch. The review is the quality gate — testing (cargo test) happens during this review, not by the haiku agents themselves. [^9a2c7-15]


When an integration branch is in use, merges happen sequentially to avoid conflicts. The pipeline is: Haiku agent completes work in isolated worktree → Sonnet agent reviews diff for doctrine compliance → merge into integration branch → run `cargo build` + scoped `cargo test` → push. The sequential merge gate prevents concurrent merge conflicts on the integration branch. [^f3d8d-15]
## Component Extraction Awareness

During implementation, agents should watch for components that should be extracted into the nmp-gallery — any component that would work well as a primitive, or any complex component that most applications would likely end up recreating. These extraction opportunities are added to the backlog but not performed during the current work. [^9a2c7-16]


Integration Branch

The integration branch uses a naming convention like `ios/nmp-component-adoption` with its worktree at `.claude/worktrees/<name>`. For example, the nmp-component-adoption worktree is at `.claude/worktrees/nmp-integration`. [^9a2c7-38]

Scaling

Tasks are organized into waves. A wave is a set of parallel agents that all start simultaneously once the previous wave's work is fully merged. Within a wave, each task has zero dependencies on other tasks in the same wave. Dependencies only exist across wave boundaries — Wave 2 tasks depend on Wave 1 foundations being in place, and Wave 3 depends on Wave 2. [^9a2c7-39]

The PR to master is triggered when a meaningful threshold of work lands — for example, ≥8 tasks merged. At that point, run `xcodegen generate` (for iOS projects using XcodeGen) before opening the PR to ensure generated project files are current. [^9a2c7-40]

Component Extraction Awareness

Extraction candidates are components that would work well as primitives, or complex components that most applications would likely end up recreating. These are recorded in the project's `BACKLOG.md` file, not extracted during the current work. The extraction audit happens during implementation, not as a separate pass. [^9a2c7-41]

Batch 3 scaled to 15 parallel agents (Android 8, Desktop 3, Architecture 4). Result: 11/15 merged on first pass, 3/4 in the fix round, 1 fixed inline by the orchestrator. The batch confirmed that 15-way parallelism is viable when tasks are organized by file ownership. The fix round used Sonnet agents for complex failures (Android navigation wire, desktop DM register, real parity tests) and Haiku for simpler fixes. The desktop-chirpclient migration failed twice with Sonnet and was ultimately fixed inline by the orchestrator, establishing the pattern that a task failing twice at the same signature mismatch should be taken over by the orchestrator rather than re-dispatched. [^f3d8d-37]
## See Also
- [[agent-push-to-master-violation|Sub-Agents in Worktrees Must Push to Branch and Open a PR — Never Push to Master]] — related guide
- [[disk-pressure-kills-agent-fleet|Accumulated Worktrees Cause Disk Exhaustion — Prune After Every Merge]] — related guide
- [[worktree-live-agent-pid-check|Worktree Removal — Check Live Agent PIDs Before Force-Removing Locks]] — related guide
- [[bespoke-pull-symbol-cleanup-workflow|Bespoke Pull-Symbol Cleanup — Four-Phase Fan-Out Workflow]] — related guide
- [[chirp-cross-platform-parity-plan|Chirp Cross-Platform Parity — Plan, Root Causes, and Ordered Work]] — related guide
- [[chirp-client-typed-api|ChirpClient Typed API — Single Action Facade for All Shells]] — related guide
- [[shared-snapshot-types|Shared Snapshot Types — Public Types in nmp-app-chirp]] — related guide
- [[android-write-capability|Android Write Capability — Dispatch Door and Write Baseline]] — related guide
- [[chirp-desktop-feature-parity|Chirp Desktop Feature Parity — What Landed and Remaining Gaps]] — related guide
- [[nmp-gallery-cross-platform-consolidation|NMP Gallery Cross-Platform Consolidation — Registry-Driven Component Catalog]] — related guide
- [[opus-architect-workflow|Opus Architect Workflow — Plan, Validate, Execute, Audit]] — related guide
- [[cross-platform-qa-code-review-workflow|Cross-Platform QA and Code-Review Fan-Out — Build, Run, Review, Synthesize]] — related guide
- [[backlog-prioritization-workflow|Backlog Prioritization — Opus-Led Ranking, Sonnet Review Gate, Parallel Dispatch]] — related guide
- [[collision-handling-two-agents-one-branch|Agent Collision Handling — Two Agents Targeting One Branch]] — related guide

