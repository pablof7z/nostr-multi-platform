---
title: AGENTS.md Policy
slug: agents-md-policy
topic: codebase-patterns
summary: The week-one act before any keystone is landing the supersession-deletion policy in AGENTS.md plus the empty mechanism_census test.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-13
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

The AGENTS.md policy has three rules: supersession = deletion (winning mechanism must delete predecessor or land dated deprecation), wire-or-delete (no vocabulary without production writer/caller unless registered in dormant inventory with deadline), and cross-module contract comments must cite a test. <!-- [^2e544-46] -->


Agents must never move the base repo away from master; work must be done in git worktrees. <!-- [^02745-71] -->

Always check for an existing PR before dispatching a fix to avoid duplicates (the #1324/#1319 duplication on #1250 was a miss). <!-- [^02745-72] -->

When determining if an owner has ratified a decision, the signal is issue closure, label change, or explicit approve/go text — not just comment-author (since all sessions authenticate as the same GitHub account). <!-- [^02745-73] -->
## Structural Tests

The mechanism_census test fails CI the moment a second generation of any capability appears, serving as structural teeth behind the supersession policy. The dormant-surface inventory test maintains a checked-in list of intentionally-unwired public surfaces with issue links and deadlines; it fails on unlisted dead vocabulary. <!-- [^2e544-47] -->


R4 planner defects 2 and 4 (T129 watermark rewrite and Rule 1 wildcard absorption) reversed documented, tested features; the agent hard-broke them and rewrote their tests, but the assistant caught this and reverted those two changes, filing them as owner-decision issues #1281 and #1282 rather than merging autonomously. <!-- [^02745-74] -->
## Doctrine-Lint Rules

New doctrine-lint rules are D20 (no ambient authority with burning allowlist), D21 (correlation linearity), and D22 (presence-Floor ban after K3 lands). <!-- [^2e544-48] -->
