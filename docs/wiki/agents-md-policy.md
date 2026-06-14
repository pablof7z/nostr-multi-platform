---
title: AGENTS.md Policy
slug: agents-md-policy
topic: codebase-patterns
summary: The week-one act before any keystone is landing the supersession-deletion policy in AGENTS.md plus the empty mechanism_census test
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-14
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
---

# AGENTS.md Policy

## Week-One Keystone

The week-one act before any keystone is landing the supersession-deletion policy in AGENTS.md plus the empty mechanism_census test. <!-- [^2e544-45] -->

## AGENTS.md Policy Rules

The AGENTS.md policy requires that a PR introducing a mechanism that supersedes another must delete the predecessor or land a dated deprecation with a tracking issue in the same milestone, and must update the mechanism_census test. A wire-or-delete policy requires that a PR may not merge vocabulary (enum variants, hooks, ports) without a production writer/caller unless registered in a dormant-surface inventory with a deadline. Comments stating cross-module contracts must cite a test; untestable contract claims are review-rejectable. Stale contract comments that misdirect future authors must be corrected to match actual code behavior. Agents must never move the base repo away from master; work must be done in git worktrees. Always check for an existing PR before dispatching a fix to avoid duplicates (the #1324/#1319 duplication on #1250 was a miss). When determining if an owner has ratified a decision, the signal is issue closure, label change, or explicit approve/go text — not just comment-author (since all sessions authenticate as the same GitHub account).

<!-- citations: [^2e544-46] [^02745-71] [^02745-72] [^02745-73] [^2e544-341] [^2e544-384] [^2e544-406] [^2e544-424] [^2e544-444] -->
## Structural Tests

The mechanism_census test asserts per-capability mechanism counts and fails CI when a second generation appears, enforcing the single-mechanism invariant. The dormant-surface inventory test maintains a checked-in list of intentionally-unwired public surfaces each with an issue link and deadline, and fails on unregistered additions. R4 planner defects 2 and 4 (T129 watermark rewrite and Rule 1 wildcard absorption) reversed documented, tested features; the agent hard-broke them and rewrote their tests, but the assistant caught this and reverted those two changes, filing them as owner-decision issues #1281 and #1282 rather than merging autonomously.

<!-- citations: [^2e544-47] [^02745-74] [^2e544-385] [^2e544-425] -->
## Doctrine-Lint Rules

New doctrine-lint rules are D20 (no ambient authority with burning allowlist), D21 (correlation linearity), and D22 (presence-Floor ban after K3 lands). <!-- [^2e544-48] -->
