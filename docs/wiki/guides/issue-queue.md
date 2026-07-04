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
updated: 2026-07-04
verified: 2026-06-29
compiled-from: conversation
sources:
  - session:3c942260-311d-4e00-8bcc-204045ea87b3
  - session:91a86fdf-624c-446e-9b38-0fb02085121f
  - session:d8bc6df1-32a3-48e1-8db6-3dbff7c4c0e5
  - session:dcc80382-bcc0-45ea-8b9c-1a2fc741f872
---

# Issue Queue as Canonical Tracker

## Purpose and Role

The issue queue is the single canonical temporal tracker for the project — not a museum. It holds all deferred and future work, and predating the epic is not by itself a reason to remove an issue. Deleting backlog to trust memory is forbidden — the queue is the source of truth for what remains to be done.

Issues are not blindly implemented just because they exist — an issue's existence does not mean it should be done. Each issue must be triaged and judged on its merits before any work begins.

Pre-epic issues touching surfaces the campaign is reshaping must be reconciled against the reset before they're actionable, with a comment noting the governing ADR so the next agent doesn't execute a stale plan.

The session goal is that all pre-v1 issues are completed and proven by session end.

<!-- citations: [^3c942-9d78d] [^3c942-a82e4] [^3c942-89f78] [^d8bc6-9cd9f] [^d8bc6-d9f8c] -->
## Issue Structure

Issues should express what, not how. Write each issue as a problem statement with constraints and open questions rather than a prescriptive plan.

Issue slices should follow a self-contained structure: Problem → Evidence (with real file paths on master) → Target state → Scope → Out-of-scope → Acceptance criteria → Verification commands.

Validation evidence screenshots are committed to the repository branch and referenced by raw GitHub URL on the issues, not linked via external hosting.

<!-- citations: [^3c942-51485] [^3c942-57599] [^dcc80-7d1ee] -->
## Banned Patterns

A single prose-backlog issue that holds a list of tasks is banned. Such an issue acts as a scattered to-do list or parallel roadmap living inside one issue, defeating the purpose of discrete, trackable work items. Issues must stay discrete and queryable with per-item priority, area, and phase metadata, and each issue must carry Closes-N PR linkage so completed work is automatically closed.

<!-- citations: [^3c942-9c79f] [^3c942-31130] [^3c942-fec21] -->
## Granularity

Architecture and debt/reconcile issues must stay discrete because agents and PRs attach to them imminently. Each should be its own issue so that work can be directly linked and progressed.

The post-v1 product tail (Cashu, WoT, Blossom) may be consolidated into a single post-v1 roadmap checklist issue only because those items are not being worked and not agent-bound; architecture/debt/reconcile items must stay discrete. This is the sole exception to the discrete-issue rule, permitted because these items are far enough out that no agent or PR is likely to attach to them soon. Issues classified as post-v1 receive the `phase:post-v1` label to distinguish them from un-phased p2 blockers.

Stash-reconcile housekeeping issues that reference deleted crates and already-closed issues (e.g. #2298, #2299) are verify-and-close items, not folded into a backlog doc. They should be verified against current master and closed if superseded, not mass-closed by age.

<!-- citations: [^3c942-cbb31] [^3c942-8bb1e] [^3c942-84650] [^d8bc6-edf18] -->
## Slice Naming Convention

Slices use a strict naming convention: `SLICE-NS-{READ,WRITE,M5}-NNN`. <!-- [^3c942-f96e4] -->

## Dependency Ordering

When an issue cannot safely be picked up before another lands, declare the dependency explicitly — e.g. #2371 (delete anonymous explicit-route defaults) must declare `Depends on: #2369, #2370` so an agent doesn't pick it up before the typed-provenance replacement is wired. <!-- [^3c942-1bab5] -->

## Deferred Decisions

If the owner explicitly defers a decision on an issue — including a directive not to re-ask or re-litigate — that deferral is itself a decision and must be honored. Do not re-open, re-prompt, or re-litigate a deferred issue without a new user instruction. For example, the milestone-shape decision on issue #1001 was deferred twice (2026-07-02 and 2026-07-03) with the instruction "Do not re-ask or re-litigate"; the issue must not be prompted for that decision again. <!-- [^91a86-21616] -->

## Triage Workflow

Triage classifies each issue by complexity and routes it accordingly. Subjective issues — those requiring judgment about scope, validity, or product direction — are routed to an opus agent for review with authority to close or defer, not just implement. Straightforward slam-dunk issues are dispatched to a sonnet agent to PR and land.

In practice, a full triage pass can substantially reduce the open-issue count: the Chirp tracker was triaged from 23 open issues down to 14, with 9 verified-resolved closes.

<!-- citations: [^d8bc6-b8596] [^dcc80-a56d2] -->
## Blocked Dependencies

Some pre-v1 issues cannot be completed within the session because they are blocked on upstream releases outside the project's control. Issue #2711 (quick-xml RUSTSEC) is blocked on an upstream wayland-scanner release that allows quick-xml ≥0.41; it has a deadline of 2026-09-30 and cannot be forced. Such issues remain in the queue as tracked, blocked work rather than being mass-closed or dropped. <!-- [^d8bc6-e55a5] -->
