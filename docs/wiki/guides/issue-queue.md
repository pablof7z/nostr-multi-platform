---
title: Issue Queue as Canonical Tracker
slug: issue-queue
topic: issue-queue
summary: The issue queue is the single canonical temporal tracker for the project â not a museum
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-29
updated: 2026-06-29
verified: 2026-06-29
compiled-from: conversation
sources:
  - session:3c942260-311d-4e00-8bcc-204045ea87b3
---

# Issue Queue as Canonical Tracker

## Purpose and Role

The issue queue is the single canonical temporal tracker for the project — not a museum. It holds all deferred and future work, and predating the epic is not by itself a reason to remove an issue. Deleting backlog to trust memory is forbidden — the queue is the source of truth for what remains to be done.

Pre-epic issues touching surfaces the campaign is reshaping must be reconciled against the reset before they're actionable, with a comment noting the governing ADR so the next agent doesn't execute a stale plan. <!-- [^3c942-9d78d] -->

<!-- citations: [^3c942-a82e4] [^3c942-89f78] -->
## Issue Structure

Issues should express what, not how. Write each issue as a problem statement with constraints and open questions rather than a prescriptive plan. <!-- [^3c942-51485] -->


Issue slices should follow a self-contained structure: Problem → Evidence (with real file paths on master) → Target state → Scope → Out-of-scope → Acceptance criteria → Verification commands. <!-- [^3c942-57599] -->
## Banned Patterns

A single prose-backlog issue that holds a list of tasks is banned. Such an issue acts as a scattered to-do list or parallel roadmap living inside one issue, defeating the purpose of discrete, trackable work items. Issues must stay discrete and queryable with per-item priority, area, and phase metadata, and each issue must carry Closes-N PR linkage so completed work is automatically closed.

<!-- citations: [^3c942-9c79f] [^3c942-31130] [^3c942-fec21] -->
## Granularity

Architecture and debt/reconcile issues must stay discrete because agents and PRs attach to them imminently. Each should be its own issue so that work can be directly linked and progressed.

The post-v1 product tail (Cashu, WoT, Blossom) may be consolidated into a single post-v1 roadmap checklist issue only because those items are not being worked and not agent-bound; architecture/debt/reconcile items must stay discrete. This is the sole exception to the discrete-issue rule, permitted because these items are far enough out that no agent or PR is likely to attach to them soon.

Stash-reconcile housekeeping issues that reference deleted crates and already-closed issues (e.g. #2298, #2299) are verify-and-close items, not folded into a backlog doc. They should be verified against current master and closed if superseded, not mass-closed by age.

<!-- citations: [^3c942-cbb31] [^3c942-8bb1e] [^3c942-84650] -->
## Slice Naming Convention

Slices use a strict naming convention: `SLICE-NS-{READ,WRITE,M5}-NNN`. <!-- [^3c942-f96e4] -->

## Dependency Ordering

When an issue cannot safely be picked up before another lands, declare the dependency explicitly — e.g. #2371 (delete anonymous explicit-route defaults) must declare `Depends on: #2369, #2370` so an agent doesn't pick it up before the typed-provenance replacement is wired. <!-- [^3c942-1bab5] -->
