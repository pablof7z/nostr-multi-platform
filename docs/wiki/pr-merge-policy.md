---
title: PR Merge Policy & Manual Intervention
slug: pr-merge-policy
summary: PR #739 must be merged manually by the user rather than by an automated agent
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-28
updated: 2026-05-29
verified: 2026-05-28
compiled-from: conversation
sources:
  - session:f5503f3a-d44c-4626-b8de-0492ad1f2a6c
  - session:055efacc-c4f7-49a4-b5f4-644bcd80f294
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
  - session:38935d82-0cbf-4e85-98d3-a0f056fd450c
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# PR Merge Policy & Manual Intervention

## Merge Policy

PR #739 must be merged manually by the user rather than by an automated agent. PR #737 is superseded by PR #740 and must be closed instead of merged. PR #734 (nmp-gallery-desktop) is merged immediately as it was already clean. Conflicting PR #735 (chirp-desktop) is resolved by rebasing onto master and resolving trivial `nmp-release.toml` conflicts where both PR #734 and #735 added the `chirp-desktop` entry. Auto-merge is enabled on rebased PRs (#735, #747) while CI runs. PRs must never be merged while cargo test is in pending status; the master-tip commit's test conclusion must be verified green before merging or tagging. PRs must not merge with red CI that breaks the build. Every PR must be review-gated before merge, and the review gate must force genuine fixes rather than wave issues through. Each merged PR's worktree is pruned immediately after merge, local master is re-synced, and live-agent worktrees are never touched.

<!-- citations: [^f5503-6] [^055ef-4] [^42908-22] [^38935-9] [^4edd4-30] -->
## See Also

